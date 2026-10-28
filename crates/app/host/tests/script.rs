//! KSP scripts driving the fresh engine, judged by what it plays.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{
    Atom, Engine, Event, HostNote, HostPattern, Plan, Region, SampleTime, Timed, Zone,
};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// A zone on `keys` whose constant level says which zone sounds.
fn zone(store: &mut Store, level: f32, group: u32) -> Zone {
    let frames = 96_000;
    let bytes = (0..frames).flat_map(|_| level.to_le_bytes()).collect();
    let key = BlobKey {
        file: format!("dc{level}").into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let region = Region {
        sample,
        start: 0,
        end: frames,
        looped: None,
        reverse: false,
    };
    let mut z = Zone::new(region, 60, RATE);
    z.group = group;
    z
}

fn engine(store: &Store, zones: Vec<Zone>, source: &str) -> Engine {
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    Engine::new(Arc::new(plan), store.client(), RATE)
}

fn host(key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    }
}

fn on(key: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host(key),
            velocity: 1.0,
            tune: 0.0,
        },
    }
}

fn off(key: u8) -> Timed<Event> {
    let pattern = HostPattern {
        port: 0,
        channel: 0,
        key: i32::from(key),
        id: -1,
        clap: true,
    };
    Timed {
        at: SampleTime(0),
        body: Event::NoteOff(pattern),
    }
}

/// Left level at the end of a 4096-frame block.
fn level(e: &mut Engine, input: &[Timed<Event>]) -> f32 {
    let mut out = vec![[0.0; 2]; 4096];
    e.process(&mut out, input);
    out.last().unwrap()[0]
}

#[test]
fn the_script_chooses_the_groups_that_play() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.25, 0), zone(&mut store, 0.5, 1)];
    let mut e = engine(
        &store,
        zones,
        "on note
            disallow_group($ALL_GROUPS)
            allow_group(1)
        end on",
    );
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
}

#[test]
fn an_ignored_note_plays_its_child_until_released() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5, 0);
    z.keys = 72..=72;
    let mut e = engine(
        &store,
        vec![z],
        "on init
            declare $child
        end on
        on note
            ignore_event($EVENT_ID)
            $child := play_note($EVENT_NOTE + 12, $EVENT_VELOCITY, 0, -1)
        end on
        on release
            message(\"released \" & $EVENT_NOTE)
        end on",
    );
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
    assert_eq!(level(&mut e, &[off(60)]), 0.0);
    // The child's own note-off runs the release callback too, after its parent's.
    assert_eq!(e.last_message(), "released 72");
    assert_eq!(e.active_voices(), 0);
}

#[test]
fn wait_starts_a_note_on_its_exact_sample_at_any_block_size() {
    let source = "on note
        ignore_event($EVENT_ID)
        wait(10000)
        play_note($EVENT_NOTE, 127, 0, -1)
    end on";
    for block in [64, 100, 480, 1024] {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let z = zone(&mut store, 0.5, 0);
        let mut e = engine(&store, vec![z], source);
        let mut rendered = Vec::new();
        let mut input = vec![on(60)];
        while rendered.len() < 2048 {
            let mut out = vec![[0.0; 2]; block];
            e.process(&mut out, &input);
            input.clear();
            rendered.extend(out.iter().map(|f| f[0]));
        }
        let onset = rendered.iter().position(|&v| v != 0.0);
        assert_eq!(onset, Some(480), "block {block}");
    }
}

#[test]
fn the_controller_callback_sees_and_can_swallow_a_controller() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut low = zone(&mut store, 0.25, 0);
    low.start = vec![(kontra_engine::ir::Join::And, Atom::Cc(1, 0..=63))];
    let mut high = zone(&mut store, 0.5, 0);
    high.start = vec![(kontra_engine::ir::Join::And, Atom::Cc(1, 64..=127))];
    let mut e = engine(
        &store,
        vec![low, high],
        "on controller
            message(%CC[$CC_NUM])
            if ($CC_NUM = 1)
                ignore_controller
            end if
        end on",
    );
    let cc = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, 1, 100),
    };
    let got = level(&mut e, &[cc, on(60)]);
    assert!((got - 0.25).abs() < 1e-3);
    assert_eq!(e.last_message(), "100");
}

/// Areia's script runs its init and note callbacks on the engine without faults.
#[test]
fn areia_runs_on_the_engine() {
    let path = "/data/dev/workspace/PluginArtifacts/layer-onset-20261024/areia-scripts/778d5bb709976a72.ksp";
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut e = engine(&store, Vec::new(), &source);
    for key in [48, 60, 72] {
        level(&mut e, &[on(key)]);
        level(&mut e, &[off(key)]);
    }
    let faults = e.script_faults()[0];
    eprintln!("areia: {faults:?}, stats {:?}", e.stats());
    eprintln!("areia unhandled: {:?}", e.unhandled_actions());
    assert_eq!(faults.bad_code, 0);
    assert_eq!(faults.stack_overflow, 0);
}

/// Saved control values are restored after init and read back through the control store.
#[test]
fn a_script_reads_its_restored_controls() {
    let source = "on init
            declare ui_switch $on
            declare %ids[1]
            %ids[0] := get_ui_id($on)
            make_persistent($on)
        end on
        on note
            message(get_control_par(%ids[0], $CONTROL_PAR_VALUE))
            set_control_par(%ids[0], $CONTROL_PAR_VALUE, 0)
            message(get_control_par(%ids[0], $CONTROL_PAR_VALUE) & \" \" & $on)
        end on";
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(Vec::new(), 16);
    plan.scripts = vec![kontra_host::script::plan(&script, &["$on 1".into()], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[on(60)]);
    assert_eq!(e.last_message(), "0 0");
    assert_eq!(e.stats().messages, 2);
}

#[test]
fn a_script_finds_a_modulation_by_name_and_drives_it() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].mods = vec![ir::Mod {
        name: "Dyn".into(),
        slot: 3,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Volume {
            source: ir::Source::Controller(110),
            intensity: 0.0,
            curve: ir::Curve::Depth { power: 1 },
        },
    }];
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 1000000, 0, find_mod(0, \"Dyn\"), -1)
            set_controller(110, 64)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // Full intensity: the level follows the controller linearly.
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.5 * 64.0 / 127.0).abs() < 1e-3, "{l}");
}

#[test]
fn a_script_sends_a_note_its_modulation_value() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].mods = vec![ir::Mod {
        name: "From script".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Volume {
            source: ir::Source::Script(1),
            intensity: 1.0,
            curve: ir::Curve::Depth { power: 1 },
        },
    }];
    let script = kontra_ksp::compile(
        "on note
            set_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 250000, 1)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.5 * 0.25).abs() < 1e-3, "{l}");
}

#[test]
fn a_note_played_in_one_slot_passes_the_slots_after_it() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.25, 0), zone(&mut store, 0.5, 1)];
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    let slot = |source: &str| {
        kontra_host::script::plan(&kontra_ksp::compile(source, &[]).unwrap(), &[], &[])
    };
    plan.scripts = vec![
        slot(
            "on note
                ignore_event($EVENT_ID)
                play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, -1)
            end on",
        ),
        slot(
            "on note
                if (get_event_par($EVENT_ID, $EVENT_PAR_SOURCE) = $CURRENT_SCRIPT_SLOT - 1)
                    disallow_group($ALL_GROUPS)
                    allow_group(1)
                end if
            end on",
        ),
    ];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // One note sounds, the played one, in the group the second slot chose.
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
}

#[test]
fn a_listener_timer_runs_its_callback() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            declare $played
            set_listener($NI_SIGNAL_TIMER_MS, 20000)
        end on
        on listener
            if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TIMER_MS and $played = 0)
                $played := 1
                play_note(60, 127, 0, 0)
            end if
        end on",
    );
    // 4096 frames is 85 ms: the 20 ms timer has fired and its note sounds.
    assert!((level(&mut e, &[]) - 0.5).abs() < 1e-3);
}

#[test]
fn a_pgs_key_carries_a_value_between_slots() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.25, 0), zone(&mut store, 0.5, 1)];
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    let slot = |source: &str| {
        kontra_host::script::plan(&kontra_ksp::compile(source, &[]).unwrap(), &[], &[])
    };
    plan.scripts = vec![
        slot(
            "on init
                pgs_create_key(GROUP, 1)
                pgs_set_key_val(GROUP, 0, 1)
            end on",
        ),
        slot(
            "on init
                declare $group
            end on
            on pgs_changed
                $group := pgs_get_key_val(GROUP, 0)
            end on
            on note
                disallow_group($ALL_GROUPS)
                allow_group($group)
            end on",
        ),
    ];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
}

#[test]
fn a_script_sets_a_send_effects_wet_and_dry_levels() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Gain(1.0),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    // Wet off, dry at unity (396851): only the dry path passes.
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN, 0, -1, 0, $NI_INSERT_BUS)
            set_engine_par($ENGINE_PAR_SEND_EFFECT_DRY_LEVEL, 396851, -1, 0, $NI_INSERT_BUS)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.5).abs() < 1e-3, "{l}");
}

#[test]
fn a_script_sets_a_send_level_by_the_volume_law() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Sends([0.0; 8]),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.sends = vec![Some(ir::Effect {
        kind: ir::EffectKind::Gain(1.0),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    // Chorus's reverb mix at 80 of 127; its saved level is 0.24994.
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_SENDLEVEL_0, 396821, -1, 0, $NI_INSERT_BUS)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let l = level(&mut e, &[on(60)]);
    // Dry 0.5 plus the send's return 0.5 × 0.25.
    assert!((l - 0.5 * 1.24997).abs() < 1e-3, "{l}");
}
