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
        mix: None,
        sample,
        start: 0,
        end: frames,
        loops: vec![],
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

#[test]
fn a_script_bypass_takes_a_playing_voices_filter_out() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    // A high pass a route moves runs per voice; it blocks the zone's DC.
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::High,
            hz: 1000.0,
            stages: 1,
            ladder: None,
            damping: [1.4, 1.4, 1.4],
            gain: 1.0,
            octaves: 1.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].mods = vec![ir::Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Cutoff {
            source: ir::Source::Controller(74),
            insert: 0,
            depth: 0.0,
        },
    }];
    let script = kontra_ksp::compile(
        "on controller
            set_engine_par($ENGINE_PAR_EFFECT_BYPASS, 1, 0, 0, -1)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert!(level(&mut e, &[on(60)]).abs() < 1e-3, "filtered");
    let cc = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, 1, 100),
    };
    let open = level(&mut e, &[cc]);
    assert!((open - 0.5).abs() < 1e-3, "bypassed: {open}");
}

/// A group waveshaper bypassed at load clips each voice once a script
/// switches it on.
#[test]
fn a_script_switches_on_a_waveshaper_bypassed_at_load() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Shaper(ir::Shaper {
            pre_lowpass_hz: None,
            curve: ir::DriveCurve::Soft(1.0),
            lowpass_hz: None,
            highpass_hz: None,
        }),
        bypass: true,
        output: 1.0,
        dry: 0.0,
    })];
    let script = kontra_ksp::compile(
        "on controller
            set_engine_par($ENGINE_PAR_EFFECT_BYPASS, 0, 0, 0, -1)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let dry = level(&mut e, &[on(60)]);
    assert!((dry - 0.5).abs() < 1e-3, "bypassed: {dry}");
    let cc = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, 1, 100),
    };
    level(&mut e, &[cc]);
    // 0.5 at four times the gain, then `2|x| − x²`: full scale.
    let on = level(&mut e, &[]);
    assert!((on - 1.0).abs() < 1e-2, "switched on: {on}");
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
            tau_ms: 0.0,
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
fn a_script_finds_a_modulator_that_carries_no_route() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].routes = vec![ir::Routes {
        slot: 33,
        name: "Phase_P1".into(),
        names: vec![String::new()],
    }];
    let script = kontra_ksp::compile(
        "on init
            message(find_mod(0, \"Phase_P1\") & \" \" & get_mod_idx(0, \"Phase_P1\") ...
                & \" \" & find_mod(0, \"none\") & \" \" & get_mod_idx(0, \"none\") ...
                & \" \" & $NI_NOT_FOUND)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    // Real Kontakt 8: a miss is 0 for find_mod, $NI_NOT_FOUND (-100) for get_mod_idx.
    assert_eq!(e.last_message(), "33 33 0 -100 -100");
}

#[test]
fn route_intensities_follow_kontakts_laws() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let route = |target| ir::Mod {
        name: "LFO".into(),
        slot: 3,
        target,
        bypass: false,
        kind: ir::ModKind::Volume {
            source: ir::Source::Controller(110),
            intensity: 0.5,
            curve: ir::Curve::Depth { power: 1 },
            tau_ms: 0.0,
        },
    };
    plan.groups[0].mods = vec![route(1), route(0)];
    let script = kontra_ksp::compile(
        "on init
            declare $a
            declare $b
            set_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 500000, 0, 3, 1)
            $a := get_engine_par($ENGINE_PAR_MOD_TARGET_INTENSITY, 0, 3, 1)
            set_engine_par($ENGINE_PAR_INTMOD_INTENSITY, 250000, 0, 3, -1)
            $b := get_engine_par($ENGINE_PAR_MOD_TARGET_MP_INTENSITY, 0, 3, 0)
            message($a & \" \" & $b & \" \" & get_engine_par($ENGINE_PAR_RELEASE_TRIGGER, 0, 0, -1))
            set_controller(110, 0)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // Real Kontakt 8 (probes/mod_par_laws.lua): the knob's depth is x², the
    // bipolar writers' (2x − 1)³; generic -1 is route 0.
    assert_eq!(e.last_message(), "500000 250000 0");
    // Route 1 at depth 0.25 and route 0 at -0.125, with the controller at 0.
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.5 * 0.75 * 1.125).abs() < 1e-3, "{l}");
}

#[test]
fn a_script_sets_the_stages_of_a_routed_ahdsr() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let point = |ms, level| ir::Breakpoint {
        ms,
        level,
        curve: 0.0,
    };
    let g = &mut plan.groups[0];
    g.envelopes = vec![ir::Breakpoints {
        start: 0.0,
        points: vec![
            point(500.0, 1.0),
            point(0.0, 1.0),
            point(500.0, 1.0),
            point(10.0, 0.0),
        ],
        sustain: Some(2),
        loop_from: None,
        stages: true,
    }];
    // A flex envelope of the same shape at slot 1: no stages to set.
    let flex = ir::Breakpoints {
        stages: false,
        ..g.envelopes[0].clone()
    };
    g.envelopes.push(flex);
    g.mods = vec![ir::Mod {
        name: "ENV_AHDSR".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Volume {
            source: ir::Source::Envelope(0),
            intensity: 1.0,
            curve: ir::Curve::Depth { power: 1 },
            tau_ms: 0.0,
        },
    }];
    g.mods.push(ir::Mod {
        name: "ENV_FLEX".into(),
        slot: 1,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Pan {
            source: ir::Source::Envelope(1),
            amount: 0.0,
        },
    });
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_ATTACK, 600000, 0, 1, -1)
            set_engine_par($ENGINE_PAR_ATTACK, 0, 0, 0, -1)
            set_engine_par($ENGINE_PAR_DECAY, 0, 0, 0, -1)
            set_engine_par($ENGINE_PAR_SUSTAIN, 500000, 0, 0, -1)
            set_engine_par($ENGINE_PAR_RELEASE, 250000, 0, 0, -1)
            message(get_engine_par($ENGINE_PAR_SUSTAIN, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_RELEASE, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_ATTACK, 0, 1, -1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // Real Kontakt (probes/flex_stage_writes.lua): a flex reads 0.
    assert_eq!(e.last_message(), "500000 250000 0");
    // No attack or decay: the note sits at the sustain level, 0.5³.
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.5 * 0.125).abs() < 1e-3, "{l}");
}

#[test]
fn script_knobs_set_effect_and_lfo_controls_through_their_laws() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let g = &mut plan.groups[0];
    g.lfos = vec![ir::Lfo {
        wave: ir::Wave::Multi {
            weights: [0.0; 6],
            width: 0.5,
            normalize: true,
        },
        rate: ir::Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Voice,
    }];
    g.mods = vec![ir::Mod {
        name: "LFO".into(),
        slot: 3,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Pan {
            source: ir::Source::Lfo(0),
            amount: 0.0,
        },
    }];
    g.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Crush(ir::Crush {
            rate: 1.0,
            bits: 16.0,
            noise: 0.0,
            noise_tone: 1.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_LFO_SINE, 865000, 0, 3, -1)
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY, 390018, 0, 3, -1)
            set_engine_par($ENGINE_PAR_BITS, 500000, 0, 0, -1)
            set_engine_par($ENGINE_PAR_INTMOD_PULSEWIDTH, 193000, 0, 3, -1)
            message(get_engine_par($ENGINE_PAR_LFO_SINE, 0, 3, -1) & \" \" & get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY, 0, 3, -1) & \" \" & get_engine_par($ENGINE_PAR_BITS, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_INTMOD_PULSEWIDTH, 0, 3, -1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "865000 390018 500000 193000");
}

#[test]
fn num_groups_counts_zoneless_groups_and_a_bus_volume_reads_its_gain() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let g = plan.groups[0].clone();
    plan.groups.extend([g.clone(), g]);
    let mut bus = plan.buses[0].clone();
    // ANALOG STRINGS' bus 2 in Kontakt: knob 593624, saved gain 0.8367.
    bus.gain = (593_624.0f64 / 629_960.524_947_436_1).powi(3) as f32;
    plan.buses.push(bus);
    let script = kontra_ksp::compile(
        "on init
            declare $before
            $before := get_engine_par($ENGINE_PAR_VOLUME, -1, -1, 1024)
            set_engine_par($ENGINE_PAR_VOLUME, 525135, -1, -1, 1024)
            message($NUM_GROUPS & \" \" & $before & \" \" & get_engine_par($ENGINE_PAR_VOLUME, -1, -1, 1024))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "3 593624 525135");
}

#[test]
fn a_stepped_lfos_rate_knob_sets_a_whole_count() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let g = &mut plan.groups[0];
    g.lfos = vec![ir::Lfo {
        wave: ir::Wave::Sine,
        rate: ir::Rate::Steps {
            beats: 1.0 / 6.0,
            count: 24.0,
        },
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Voice,
    }];
    g.mods = vec![ir::Mod {
        name: "LFO".into(),
        slot: 3,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Pan {
            source: ir::Source::Lfo(0),
            amount: 0.0,
        },
    }];
    // 48 / (0.010415 · 20465^0.5) Hz = 32.2: count 32, read back as 500679.
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_INTMOD_FREQUENCY, 500000, 0, 3, -1)
            message(get_engine_par($ENGINE_PAR_INTMOD_FREQUENCY, 0, 3, -1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "500679");
}

#[test]
fn slot_knobs_move_a_filters_fields_through_the_slots_own_laws() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let filter = Some(ir::Effect {
        kind: ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::Low,
            hz: 300.0,
            stages: 1,
            ladder: None,
            damping: [2.0; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    });
    plan.groups[0].inserts = vec![filter];
    plan.inserts = vec![None, filter];
    let mut bus = plan.buses[0].clone();
    bus.inserts = vec![filter];
    plan.buses.push(bus);
    let law = |at, slot, knob, field, law| ir::SlotLaw {
        at,
        slot,
        knob,
        field,
        law,
    };
    let (group, inserts) = (ir::RackAt::Group(0), ir::RackAt::Inserts);
    let (cutoff, resonance) = (ir::SlotKnob::Cutoff, ir::SlotKnob::Resonance);
    plan.slot_laws = vec![
        law(
            group,
            0,
            cutoff,
            ir::EffectParam::FilterHz,
            ir::Law::Exp {
                lo: 100.0,
                hi: 10_000.0,
            },
        ),
        law(
            group,
            0,
            resonance,
            ir::EffectParam::FilterDamping(0),
            ir::Law::Linear { lo: 2.0, hi: 0.1 },
        ),
        law(
            group,
            0,
            resonance,
            ir::EffectParam::FilterGain,
            ir::Law::Linear { lo: 1.0, hi: 0.5 },
        ),
        law(
            ir::RackAt::Bus(0),
            0,
            cutoff,
            ir::EffectParam::FilterHz,
            ir::Law::Linear {
                lo: 20.0,
                hi: 20_000.0,
            },
        ),
        law(
            inserts,
            1,
            cutoff,
            ir::EffectParam::FilterHz,
            ir::Law::Linear {
                lo: 20.0,
                hi: 20_000.0,
            },
        ),
    ];
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_CUTOFF, 500000, 0, 0, -1)
            set_engine_par($ENGINE_PAR_RESONANCE, 250000, 0, 0, -1)
            set_engine_par($ENGINE_PAR_CUTOFF, 750000, -1, 1, 1)
            set_engine_par($ENGINE_PAR_CUTOFF, 250000, -1, 0, 1024)
            message(get_engine_par($ENGINE_PAR_CUTOFF, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_RESONANCE, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_CUTOFF, -1, 1, 1) & \" \" & get_engine_par($ENGINE_PAR_CUTOFF, -1, 0, 1024))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "500000 250000 750000 250000");
}

/// `EFFECT_SUBTYPE` switches a filter to a shape its slot offers: the
/// resonance moves to the new last stage, the first goes critically damped.
#[test]
fn effect_subtype_switches_a_filter_to_its_slots_shape() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Filter(ir::Filter {
            pass: ir::Pass::Low,
            hz: 300.0,
            stages: 1,
            ladder: None,
            damping: [2.0; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let law = |knob, field, law| ir::SlotLaw {
        at: ir::RackAt::Group(0),
        slot: 0,
        knob,
        field,
        law,
    };
    plan.slot_laws = vec![
        law(
            ir::SlotKnob::Resonance,
            ir::EffectParam::FilterLastDamping,
            ir::Law::Linear { lo: 2.0, hi: 0.1 },
        ),
        // Read the first stage and the gain back.
        law(
            ir::SlotKnob::Attack,
            ir::EffectParam::FilterDamping(0),
            ir::Law::Linear { lo: 2.0, hi: 0.0 },
        ),
        law(
            ir::SlotKnob::Release,
            ir::EffectParam::FilterGain,
            ir::Law::Linear { lo: 0.0, hi: 1.0 },
        ),
    ];
    plan.slot_shapes.slots = vec![(ir::RackAt::Group(0), 0)];
    plan.slot_shapes.shapes = vec![ir::SlotShape {
        id: 103,
        filter: ir::Filter {
            pass: ir::Pass::Low,
            hz: 300.0,
            stages: 2,
            ladder: None,
            damping: [2.0; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 0.0,
        },
        laws: Vec::new().into(),
    }];
    // A shape with its own resonance law: the gain falls as it rises.
    let mut own = plan.slot_shapes.shapes[0].clone();
    own.id = 58;
    own.laws = vec![(
        ir::SlotKnob::Resonance,
        ir::EffectParam::FilterGain,
        ir::Law::Linear { lo: 1.0, hi: 0.0 },
    )]
    .into();
    plan.slot_shapes.shapes.push(own);
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_RESONANCE, 250000, 0, 0, -1)
            declare @s
            @s := get_engine_par($ENGINE_PAR_COMP_ATTACK, 0, 0, -1) & \" \"
            set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, 999, 0, 0, -1)
            @s := @s & get_engine_par($ENGINE_PAR_COMP_ATTACK, 0, 0, -1) & \" \"
            set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, 103, 0, 0, -1)
            @s := @s & get_engine_par($ENGINE_PAR_COMP_ATTACK, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_RESONANCE, 0, 0, -1) & \" \"
            set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, 58, 0, 0, -1)
            message(@s & get_engine_par($ENGINE_PAR_RESONANCE, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_COMP_DECAY, 0, 0, -1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "237500 237500 0 250000 250000 750000");
}

/// A switch to a shape with its own cutoff law (AR → Daft, say) keeps the
/// cutoff setting and moves the Hz to the new law.
#[test]
fn effect_subtype_keeps_the_cutoff_setting_through_the_new_law() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    let filter = ir::Filter {
        pass: ir::Pass::Low,
        hz: 300.0,
        stages: 1,
        ladder: None,
        damping: [2.0; 3],
        gain: 1.0,
        octaves: 0.0,
        q_octaves: 0.0,
    };
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Filter(filter),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let hz = |knob, lo, hi| ir::SlotLaw {
        at: ir::RackAt::Group(0),
        slot: 0,
        knob,
        field: ir::EffectParam::FilterHz,
        law: ir::Law::Linear { lo, hi },
    };
    // The delay-time knob reads the Hz back.
    plan.slot_laws = vec![
        hz(ir::SlotKnob::Cutoff, 100.0, 1100.0),
        hz(ir::SlotKnob::DelayTime, 0.0, 10_000.0),
    ];
    let own = (
        ir::SlotKnob::Cutoff,
        ir::EffectParam::FilterHz,
        ir::Law::Linear {
            lo: 1000.0,
            hi: 2000.0,
        },
    );
    plan.slot_shapes.slots = vec![(ir::RackAt::Group(0), 0)];
    plan.slot_shapes.shapes = vec![ir::SlotShape {
        id: 70,
        filter,
        laws: vec![own].into(),
    }];
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_CUTOFF, 500000, 0, 0, -1)
            declare @s
            @s := get_engine_par($ENGINE_PAR_DL_TIME, 0, 0, -1) & \" \"
            set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, 70, 0, 0, -1)
            message(@s & get_engine_par($ENGINE_PAR_CUTOFF, 0, 0, -1) & \" \" & get_engine_par($ENGINE_PAR_DL_TIME, 0, 0, -1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "60000 500000 150000");
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
            tau_ms: 0.0,
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
fn millisecond_and_beat_timers_run_side_by_side_and_keep_their_signal() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            declare $ms
            declare $beat
            set_listener($NI_SIGNAL_TIMER_MS, 10000)
            set_listener($NI_SIGNAL_TIMER_BEAT, 24)
            { Changed before the first firing: one schedule, not two. }
            change_listener_par($NI_SIGNAL_TIMER_MS, 10000)
        end on
        on listener
            wait(1)
            if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TIMER_MS)
                inc($ms)
            end if
            if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TIMER_BEAT)
                inc($beat)
            end if
            message($ms & \" \" & $beat)
        end on",
    );
    // 4096 frames is 85 ms: 8 millisecond firings and, at 120 bpm (20.8 ms
    // per 24th of a quarter), 4 beat firings, each signal surviving the wait.
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "8 4");
}

/// The callback-type and signal numbers scripts compare against are the
/// measured ones (probes/callback_constants.lua).
#[test]
fn callback_types_and_signals_carry_the_measured_numbers() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            message($NI_CALLBACK_TYPE & \" \" & $NI_SIGNAL_TIMER_MS & \" \" & $NI_SIGNAL_TRANSP_START & \" \" & $NI_BUS_OFFSET)
        end on",
    );
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "5 3 1 1024");
}

#[test]
fn the_transport_starts_the_listener_and_answers_its_queries() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            set_listener($NI_SIGNAL_TRANSP_START, 1)
        end on
        on listener
            if ($NI_SIGNAL_TYPE = $NI_SIGNAL_TRANSP_START)
                message($NI_TRANSPORT_RUNNING & \" \" & $NI_SONG_POSITION & \" \" ...
                    & ms_to_ticks(500000) & \" \" & ticks_to_ms(480) & \" \" ...
                    & $SIGNATURE_NUM & \"/\" & $SIGNATURE_DENOM & \" \" & $DURATION_BAR)
            end if
        end on
        on note
            message($NI_SONG_POSITION)
        end on",
    );
    // 120 bpm: a quarter is 500 ms and 960 ticks; a 3/4 bar is 1.5 s.
    e.set_transport(kontra_engine::Play::Running, 2.0, (3, 4));
    assert_eq!(e.last_message(), "1 1920 960 250000 3/4 1500000");
    // 4096 frames at 120 bpm move the song on 163.84 ticks.
    level(&mut e, &[]);
    level(&mut e, &[on(60)]);
    assert_eq!(e.last_message(), "2083");
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
    // `on pgs_changed` runs a sample after the write.
    level(&mut e, &[]);
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
}

#[test]
fn a_script_sets_a_send_effects_wet_and_dry_levels() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Gain(2.0),
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

/// `$NI_BUS_OFFSET + n` puts a group on bus n; −1 puts it back on the main bus.
#[test]
fn a_script_moves_a_group_onto_a_bus() {
    use kontra_engine::Bus;
    let play = |set: &str| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
        plan.buses.push(Bus {
            name: "Room".into(),
            gain: 0.5,
            pan: 0.0,
            inserts: Vec::new(),
            sends: Vec::new(),
            to_main: true,
            enabled: true,
            mods: Vec::new(),
            blends: Vec::new(),
        });
        let script = kontra_ksp::compile(
            &format!(
                "on init
                    set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, $NI_BUS_OFFSET, 0, -1, -1)
                    {set}
                    message(get_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, 0, -1, -1))
                end on"
            ),
            &[],
        )
        .unwrap();
        plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        (level(&mut e, &[on(60)]), e.last_message().to_owned())
    };
    let (l, read) = play("");
    assert!(
        (l - 0.25).abs() < 1e-3 && read == "1024",
        "on the bus: {l} {read}"
    );
    let (l, read) = play("set_engine_par($ENGINE_PAR_OUTPUT_CHANNEL, -1, 0, -1, -1)");
    assert!(
        (l - 0.5).abs() < 1e-3 && read == "-1",
        "on the main bus: {l} {read}"
    );
}

/// What a script reads back from its menus, texts and keys (probes/menu_readback.lua).
#[test]
fn a_script_reads_back_its_menus_texts_and_keys() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let e = engine(
        &store,
        zones,
        "on init
            declare ui_menu $m
            declare ui_label $l(1, 1)
            declare $id
            add_menu_item($m, \"a\", 10)
            add_menu_item($m, \"bb\", 20)
            add_menu_item($m, \"ccc\", 30)
            $id := get_ui_id($m)
            $m := 20
            set_menu_item_visibility($id, 0, 0)
            set_menu_item_str($id, 2, \"z\")
            set_control_par_str(get_ui_id($l), $CONTROL_PAR_TEXT, \"hello\")
            set_key_color(60, $KEY_COLOR_RED)
            set_key_name(60, \"C\")
            message(get_control_par($id, $CONTROL_PAR_NUM_ITEMS) & \" \" ...
                & get_control_par($id, $CONTROL_PAR_SELECTED_ITEM_IDX) & \" \" ...
                & get_menu_item_visibility($id, 0) & \" \" & get_menu_item_value($id, 2) & \" \" ...
                & get_menu_item_str($id, 2) & \" \" ...
                & get_control_par_str(get_ui_id($l), $CONTROL_PAR_TEXT) & \" \" ...
                & get_key_color(60) & \" \" & get_key_name(60) & \" \" & get_key_color(61))
        end on",
    );
    assert_eq!(e.last_message(), "3 1 0 30 z hello 4 C 0");
    assert_eq!(e.keys()[60].color, 4);
}

/// A menu's `$CONTROL_PAR_VALUE` is its selected index, read and written:
/// what Kontakt 8.13 answers for this script (oracle probe, 2026-10-29).
#[test]
fn a_menu_value_is_its_selected_index() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let e = engine(
        &store,
        zones,
        "on init
            declare ui_menu $m
            declare @s
            add_menu_item($m, \"a\", 10)
            add_menu_item($m, \"b\", 20)
            add_menu_item($m, \"c\", 30)
            add_menu_item($m, \"d\", 20)
            @s := $m & \" \" & get_control_par(get_ui_id($m), $CONTROL_PAR_VALUE)
            set_control_par(get_ui_id($m), $CONTROL_PAR_VALUE, 2)
            @s := @s & \" \" & $m & \" \" & get_control_par(get_ui_id($m), $CONTROL_PAR_VALUE)
            $m := 20
            @s := @s & \" \" & get_control_par(get_ui_id($m), $CONTROL_PAR_VALUE)
            set_control_par(get_ui_id($m), $CONTROL_PAR_VALUE, 7)
            message(@s & \" \" & $m)
        end on",
    );
    assert_eq!(e.last_message(), "10 0 30 2 1 0");
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

#[test]
fn an_ignored_release_still_lifts_the_key() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on note
            message(\"down \" & %KEY_DOWN[60])
        end on
        on release
            ignore_event($EVENT_ID)
        end on",
    );
    level(&mut e, &[on(60)]);
    // The voice plays on past its ignored key-up.
    assert!((level(&mut e, &[off(60)]) - 0.5).abs() < 1e-3);
    level(&mut e, &[on(61)]);
    assert_eq!(e.last_message(), "down 0");
}

/// Level of the reverb tail 0.25 s after a 2 s note, through a reverb in the
/// instrument insert whose time is 0.5 s unless `script` moves it.
fn reverb_tail(script: &str) -> f32 {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Reverb(ir::Reverb {
            seconds: 0.5,
            size: 1.0,
            network: None,
            damping: 0.0,
            diffusion: 1.0,
            predelay_ms: 0.0,
            high_cut_hz: 20_000.0,
            low_shelf_db: 0.0,
            stereo: 1.0,
            modulation_ms: 0.0,
            modulation_hz: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let script = kontra_ksp::compile(script, &[]).unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 4096];
    e.process(&mut out, &[on(60)]);
    // 26 blocks of 4096 frames: the note ends at 96000 frames, the last block starts 0.22 s later.
    for _ in 0..26 {
        e.process(&mut out, &[]);
    }
    let energy: f32 = out.iter().map(|f| f[0] * f[0]).sum();
    10.0 * (energy / 4096.0).max(1e-30).log10()
}

#[test]
fn a_script_moves_a_playing_reverbs_time() {
    let short = reverb_tail("on init\nend on");
    // RV2_TIME 500000 is x = 0.5: 0.5·e^(3.7·0.5) ≈ 3.2 s, set on the note as it plays.
    let long = reverb_tail(
        "on note
            set_engine_par($ENGINE_PAR_RV2_TIME, 500000, -1, 0, $NI_INSERT_BUS)
        end on",
    );
    assert!(long > short + 10.0, "short {short} dB, long {long} dB");
}

/// One 0.5-level zone under a Lua script.
fn lua_engine(store: &mut Store, source: &str) -> Engine {
    let zones = vec![zone(store, 0.5, 0)];
    let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
    Engine::new(Arc::new(plan), store.client(), RATE)
}

#[test]
fn a_uvi_note_on_an_oscillator_starts_only_that_zone_slot() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut second = zone(&mut store, 0.25, 0);
    second.slot = 1;
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0), second], 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    let source = "function onNote(e) playNote{note=e.note, velocity=e.velocity, oscIndex=1} end";
    let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
    plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let measured = level(&mut e, &[on(60)]);
    assert!((measured - 0.25).abs() < 1e-3, "{measured}");
}

#[test]
fn a_script_modulation_sent_at_init_scales_notes_that_start_later() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    for g in &mut plan.groups {
        g.mods = vec![kontra_engine::ir::Mod {
            name: "Script Event Modulation 1".into(),
            slot: 1,
            target: 0,
            bypass: false,
            kind: kontra_engine::ir::ModKind::Volume {
                source: kontra_engine::ir::Source::Script(1),
                intensity: 1.0,
                curve: kontra_engine::ir::Curve::Depth { power: 1 },
                tau_ms: 0.0,
            },
        }];
    }
    let source = "function onInit() sendScriptModulation(1, 0.5) end
        function onNote(e) postEvent(e) end";
    let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
    plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let measured = level(&mut e, &[on(60)]);
    assert!((measured - 0.25).abs() < 1e-3, "{measured}");
}

#[test]
fn a_script_modulation_index_keeps_its_value_after_many_others() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    for g in &mut plan.groups {
        g.mods = vec![kontra_engine::ir::Mod {
            name: "Script Event Modulation 20".into(),
            slot: 1,
            target: 0,
            bypass: false,
            kind: kontra_engine::ir::ModKind::Volume {
                source: kontra_engine::ir::Source::Script(20),
                intensity: 1.0,
                curve: kontra_engine::ir::Curve::Depth { power: 1 },
                tau_ms: 0.0,
            },
        }];
    }
    // Twelve indices no route reads go first; the one a route reads still lands.
    let source = "function onInit()
            for i = 1, 12 do sendScriptModulation(i, 1) end
            sendScriptModulation(20, 0.5)
        end
        function onNote(e) postEvent(e) end";
    let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
    plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let measured = level(&mut e, &[on(60)]);
    assert!((measured - 0.25).abs() < 1e-3, "{measured}");
}

#[test]
fn a_modulation_value_no_route_reads_is_still_read_back() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            set_event_par_arr($ALL_EVENTS, $EVENT_PAR_MOD_VALUE_ID, 300000, 7)
        end on
        on note
            message(get_event_par_arr($EVENT_ID, $EVENT_PAR_MOD_VALUE_ID, 7))
        end on",
    );
    level(&mut e, &[on(60)]);
    assert_eq!(e.last_message(), "300000");
}

#[test]
fn a_script_modulation_ramps_to_its_value() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    for g in &mut plan.groups {
        g.mods = vec![kontra_engine::ir::Mod {
            name: "Script Event Modulation 1".into(),
            slot: 1,
            target: 0,
            bypass: false,
            kind: kontra_engine::ir::ModKind::Volume {
                source: kontra_engine::ir::Source::Script(1),
                intensity: 1.0,
                curve: kontra_engine::ir::Curve::Depth { power: 1 },
                tau_ms: 0.0,
            },
        }];
    }
    // Silent at the note, then up to full over 200 ms.
    let source = "function onNote(e) postEvent(e) sendScriptModulation2(1, 0, 1, 200) end";
    let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
    plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let early = level(&mut e, &[on(60)]);
    for _ in 0..4 {
        level(&mut e, &[]);
    }
    let late = level(&mut e, &[]);
    assert!(early < 0.2, "early {early}");
    assert!((late - 0.5).abs() < 1e-3, "late {late}");
}

/// C5 on a UVI script: waits, spawned notes and a ramp render the same
/// audio whatever the host's block size.
#[test]
fn a_uvi_script_renders_the_same_at_any_block_size() {
    let source = "function onNote(e)
            postEvent(e)
            sendScriptModulation2(1, 0, 1, 50)
            spawn(function() wait(30) playNote(e.note + 12, 100, 40) end)
        end";
    let render = |block: usize| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
        for g in &mut plan.groups {
            g.mods = vec![kontra_engine::ir::Mod {
                name: "Script Event Modulation 1".into(),
                slot: 1,
                target: 0,
                bypass: false,
                kind: kontra_engine::ir::ModKind::Volume {
                    source: kontra_engine::ir::Source::Script(1),
                    intensity: 1.0,
                    curve: kontra_engine::ir::Curve::Depth { power: 1 },
                    tau_ms: 0.0,
                },
            }];
        }
        let compiled = kontra_lua::compile(source, &[], &[]).unwrap();
        plan.scripts = vec![kontra_host::script::lua_plan(compiled)];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut rendered = Vec::new();
        let mut input = vec![on(60)];
        while rendered.len() < 9600 {
            let mut out = vec![[0.0; 2]; block];
            e.process(&mut out, &input);
            input.clear();
            rendered.extend(out.iter().map(|f| f[0]));
        }
        rendered.truncate(9600);
        rendered
    };
    let reference = render(64);
    assert!(reference.iter().any(|&v| v != 0.0));
    for block in [100, 480, 1024] {
        assert_eq!(render(block), reference, "block {block}");
    }
}

#[test]
fn a_uvi_note_sounds_only_when_the_script_posts_it() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let held = "function onNote(e) end";
    let mut e = lua_engine(&mut store, held);
    assert_eq!(level(&mut e, &[on(60)]), 0.0);
    let posted = "function onNote(e) postEvent(e) end
        function onRelease(e) end";
    let mut e = lua_engine(&mut store, posted);
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
    // The script keeps the release to itself: the note plays on.
    assert!((level(&mut e, &[off(60)]) - 0.5).abs() < 1e-3);
}

/// A UVI script's own gated note ends without reaching its `onRelease`
/// (Augmented Orchestra's arpeggiator: a step's gate on the held key once
/// released every layer's held note and stopped the arpeggio).
#[test]
fn a_uvi_scripts_own_note_ending_does_not_run_its_release() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let source = "local held
        function onNote(e) held = playNote(e.note, 100) playNote(e.note, 100, 10) end
        function onRelease(e) releaseVoice(held) end";
    let mut e = lua_engine(&mut store, source);
    level(&mut e, &[on(60)]);
    let after_gate = level(&mut e, &[]);
    assert!((after_gate - 0.5).abs() < 1e-3, "{after_gate}");
    // The key-up still reaches it.
    level(&mut e, &[off(60)]);
    assert_eq!(level(&mut e, &[]), 0.0);
}

#[test]
fn uvi_waits_and_spawned_coroutines_land_on_time() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // 100 ms is 4800 frames: past the first 4096-frame block.
    let waited = "function onNote(e) wait(100) postEvent(e) end";
    let mut e = lua_engine(&mut store, waited);
    assert_eq!(level(&mut e, &[on(60)]), 0.0);
    assert!((level(&mut e, &[]) - 0.5).abs() < 1e-3);
    let spawned = "function onNote(e)
            spawn(function() wait(100) playNote(60, 100) end)
            print('spawned')
        end";
    let mut e = lua_engine(&mut store, spawned);
    assert_eq!(level(&mut e, &[on(60)]), 0.0);
    assert_eq!(e.last_message(), "spawned");
    assert!((level(&mut e, &[]) - 0.5).abs() < 1e-3);
}

#[test]
fn a_uvi_script_that_fails_counts_a_fault_and_the_engine_plays_on() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let broken = "function onNote(e) error('no') end";
    let mut e = lua_engine(&mut store, broken);
    assert_eq!(level(&mut e, &[on(60)]), 0.0);
    assert_eq!(e.script_faults().first().map(|f| f.bad_code), Some(1));
    let runaway = "function onNote(e) while true do end end";
    let mut e = lua_engine(&mut store, runaway);
    assert_eq!(level(&mut e, &[on(60)]), 0.0);
    assert_eq!(e.script_faults().first().map(|f| f.bad_code), Some(1));
}

#[test]
fn moving_a_control_runs_ui_controls_then_its_ui_control() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            declare ui_knob $k (0, 100, 1)
            declare ui_switch $s
            declare @log
            message(get_ui_id($k) & \" \" & get_ui_id($s))
        end on
        on ui_controls
            @log := @log & \"all \" & $NI_UI_ID & \" \"
            message(@log & $s)
        end on
        on ui_control ($k)
            @log := @log & \"k \" & $k & \" \" & $NI_CALLBACK_TYPE
            message(@log)
        end on",
    );
    let ids = e.last_message();
    let (id, s) = ids.split_once(' ').unwrap();
    let (id, s): (i32, i32) = (id.parse().unwrap(), s.parse().unwrap());
    e.set_control(0, id, 42);
    assert_eq!(e.last_message(), format!("all {id} k 42 6"));
    // Not a control of this script: nothing runs.
    e.set_control(0, id + 100, 1);
    assert_eq!(e.last_message(), format!("all {id} k 42 6"));
    // Without its own callback the switch still stores, and `on ui_controls`
    // runs (KSP manual: on any widget).
    e.set_control(0, s, 1);
    assert_eq!(e.last_message(), format!("all {id} k 42 6all {s} 1"));
}

#[test]
fn a_purge_reports_its_id_in_async_complete() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            declare $id
            $id := purge_group(0, 0)
        end on
        on async_complete
            message($NI_ASYNC_ID & \" \" & $id & \" \" & $NI_ASYNC_EXIT_STATUS & \" \" & $NI_CALLBACK_TYPE)
        end on",
    );
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "1 1 1 13");
}

/// `set_engine_par` as measured in Kontakt (probes/async_engine_par.ksp):
/// −1, also for an effect type set in `on init`; outside it, an effect type
/// change answers its async id and `on async_complete` follows.
#[test]
fn an_effect_type_change_outside_init_completes_asynchronously() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![zone(&mut store, 0.5, 0)];
    let mut e = engine(
        &store,
        zones,
        "on init
            declare $a
            declare $i
            declare $b
            $a := set_engine_par($ENGINE_PAR_EFFECT_BYPASS, 1, -1, 0, 1)
            $i := set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_NONE, -1, 0, 1)
        end on
        on note
            $b := set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_NONE, -1, 0, 1)
        end on
        on async_complete
            message($a & \" \" & $i & \" \" & $b & \" \" & $NI_ASYNC_ID)
        end on",
    );
    level(&mut e, &[on(60)]);
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "-1 -1 1 1");
}

/// An effect type loads into an empty slot: at once in `on init`; later the
/// host builds it off the audio thread, and `on async_complete` follows its
/// install.
#[test]
fn a_script_loads_an_effect_type_into_a_slot() {
    use kontra_engine::{Prepared, ir};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].mods.clear();
    plan.inserts = vec![None];
    let high = ir::Filter {
        pass: ir::Pass::High,
        hz: 10_000.0,
        stages: 1,
        ladder: None,
        damping: [2.0; 3],
        gain: 1.0,
        octaves: 0.0,
        q_octaves: 0.0,
    };
    let ty = |id, effect| ir::SlotType {
        id,
        subtype: false,
        reads: id * 10,
        effect,
        laws: Vec::new().into(),
    };
    let effect = ir::Effect {
        kind: ir::EffectKind::Filter(high),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    plan.slot_shapes.types = vec![ty(0, None), ty(7, Some(effect))];
    plan.inserts.push(None);
    plan.slot_shapes.subtypes = vec![(ir::RackAt::Inserts, 1, 90)];
    let script = kontra_ksp::compile(
        "on init
            declare $a
            declare $b
            $a := set_engine_par($ENGINE_PAR_EFFECT_TYPE, 7, -1, 0, 1)
            message(get_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, -1, 0, 1) & \" \" & get_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, -1, 1, 1))
        end on
        on note
            $b := set_engine_par($ENGINE_PAR_EFFECT_TYPE, 0, -1, 0, 1)
        end on
        on async_complete
            message($a & \" \" & $b & \" \" & $NI_ASYNC_ID)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // The loaded type's subtype, and slot 1's saved one.
    assert_eq!(e.last_message(), "70 90");
    // The high-pass loaded in init holds the DC back.
    assert!(level(&mut e, &[on(60)]).abs() < 1e-3);
    let mut requests = Vec::new();
    e.drain_requests(|r| requests.push(r));
    assert_eq!(requests.len(), 1);
    assert!(level(&mut e, &[]).abs() < 1e-3, "nothing until installed");
    assert_eq!(e.last_message(), "70 90");
    let old = e.install(Prepared::new(requests[0], None, RATE));
    assert!(old.request().effect.is_some(), "the high-pass comes back");
    assert!((level(&mut e, &[]) - 0.5).abs() < 1e-3);
    assert_eq!(e.last_message(), "-1 1 1");
}

/// `wait_async` holds its callback until the effect type it waits on is
/// installed, so the knob writes after it reach the new unit (Conflux's FX
/// menu: subtype, wait, cutoff).
#[test]
fn wait_async_holds_until_the_effect_type_is_installed() {
    use kontra_engine::{Prepared, ir};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.groups[0].mods.clear();
    plan.inserts = vec![None];
    let ty = |id| ir::SlotType {
        id,
        subtype: false,
        reads: id * 10,
        effect: None,
        laws: Vec::new().into(),
    };
    plan.slot_shapes.types = vec![ty(0), ty(7)];
    let script = kontra_ksp::compile(
        "on init
            set_engine_par($ENGINE_PAR_EFFECT_TYPE, 7, -1, 0, 1)
            message(\"init \" & get_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, -1, 0, 1))
        end on
        on note
            wait_async(set_engine_par($ENGINE_PAR_EFFECT_TYPE, 0, -1, 0, 1))
            message(\"after \" & get_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, -1, 0, 1))
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    assert_eq!(e.last_message(), "init 70");
    level(&mut e, &[on(60)]);
    let mut requests = Vec::new();
    e.drain_requests(|r| requests.push(r));
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "init 70", "still waiting");
    e.install(Prepared::new(requests[0], None, RATE));
    level(&mut e, &[]);
    assert_eq!(e.last_message(), "after 0");
}

/// `stop_wait` as measured in Kontakt (ANALOG, two `wait(10 s)` then
/// `play_note`, stopped from `on release`): 0 ends only the current wait,
/// 1 also lets every later wait pass. The first wait's own resume, still
/// queued, must not end the second wait early.
#[test]
fn stop_wait_ends_the_current_wait_or_every_wait() {
    for (rest, sounds_at_release) in [(0, false), (1, true)] {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let z = zone(&mut store, 0.5, 0);
        let source = format!(
            "on init
                declare $waiting
            end on
            on note
                ignore_event($EVENT_ID)
                $waiting := $NI_CALLBACK_ID
                wait(100000)
                wait(100000)
                play_note(60, 127, 0, 0)
            end on
            on release
                stop_wait($waiting, {rest})
            end on"
        );
        let mut e = engine(&store, vec![z], &source);
        assert_eq!(level(&mut e, &[on(60)]), 0.0);
        // Key up at 4096; the second wait (4800 frames) runs to 8896.
        let at_release = level(&mut e, &[off(60)]);
        assert_eq!(at_release != 0.0, sounds_at_release, "rest {rest}");
        assert!((level(&mut e, &[]) - 0.5).abs() < 1e-3, "rest {rest}");
    }
}

/// Kontakt keeps one release-trigger counter per key; its system script
/// restarts it on every attack, and a script that ignores the attack
/// restarts it with `reset_rls_trig_counter` (string and system script in
/// Kontakt8.exe at 0x144e8d2f0 / 0x144f17c23). A release trigger decays by
/// the time since that restart, not since the key went down.
#[test]
fn reset_rls_trig_counter_restarts_the_release_triggers_decay() {
    for (reset, expected) in [(false, 0.2), (true, 0.4)] {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let attack = zone(&mut store, 0.5, 0);
        let mut tail = zone(&mut store, 0.4, 1);
        tail.trigger = kontra_engine::Trigger::Release;
        tail.release_decay_db = 6.0; // per second held
        let script = kontra_ksp::compile(
            "on controller
                reset_rls_trig_counter(60)
                reset_rls_trig_counter(128)
            end on",
            &[],
        )
        .unwrap();
        let mut plan = Plan::new(vec![attack, tail], 16);
        for g in &mut plan.groups {
            g.mods.clear();
            g.envelope.release = 16;
        }
        plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        level(&mut e, &[on(60)]);
        for _ in 0..(RATE as usize / 4096) {
            level(&mut e, &[]);
        }
        let mut up = vec![off(60)];
        if reset {
            up.insert(
                0,
                Timed {
                    at: SampleTime(0),
                    body: Event::midi1(0xB0, 1, 1),
                },
            );
        }
        // ~1 s held: 0.4 at −6 dB ≈ 0.2, unless the counter restarted at key-up.
        let got = level(&mut e, &up);
        assert!((got - expected).abs() < 0.02, "reset {reset}: {got}");
    }
}

/// Output menus see the engine's one stereo pair; `disable_logging` is
/// answered, not counted as unhandled.
#[test]
fn output_channels_and_disable_logging_are_answered() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let z = zone(&mut store, 0.5, 0);
    let e = engine(
        &store,
        vec![z],
        "on init
            disable_logging($NI_LOG_MESSAGE)
            message($NUM_OUTPUT_CHANNELS)
        end on",
    );
    assert_eq!(e.last_message(), "2");
    assert_eq!(e.stats().actions_unhandled, 0);
}

/// `load_ir_sample` swaps an insert convolution's response for the library
/// one it names (any extension), prepared as the one it replaces: in init at
/// once, later through the host and an async id.
#[test]
fn a_script_loads_a_library_response_into_a_convolution() {
    use kontra_engine::{Prepared, Response, ir};
    // One frame at half scale, 16-bit stereo at 48 kHz.
    let half = 16_384i16.to_le_bytes();
    let mut wav = b"RIFF".to_vec();
    wav.extend(40u32.to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend([1, 0, 2, 0]);
    wav.extend(RATE.to_le_bytes());
    wav.extend((RATE * 4).to_le_bytes());
    wav.extend([4, 0, 16, 0]);
    wav.extend(b"data");
    wav.extend(4u32.to_le_bytes());
    wav.extend(half.iter().chain(&half));
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("half.wav");
    std::fs::write(&file, wav).unwrap();
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Convolution { impulse: 0 },
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.impulses = vec![Some(Response {
        left: vec![1.0],
        right: vec![1.0],
    })];
    plan.impulse_laws = vec![(
        "room".into(),
        ir::Impulse {
            sample: 0,
            reverse: false,
            normalize: None,
            predelay_ms: 0.0,
            cuts: None,
            envelope: None,
            mix: None,
        },
    )];
    plan.responses = vec![(
        "half".into(),
        ir::Sample {
            name: "Half.wav".into(),
            file,
            offset: 0,
            len: None,
            channels: 0,
            frames: 0,
            rate: 0,
            keystream: None,
        },
    )];
    let script = kontra_ksp::compile(
        "on init
            declare $id
            load_ir_sample(\"Half.ncw\", 0, $NI_INSERT_BUS)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let later = kontra_ksp::compile(
        "on init
            declare $id
        end on
        on note
            $id := load_ir_sample(\"Half.ncw\", 0, $NI_INSERT_BUS)
        end on
        on async_complete
            message($id & \" \" & $NI_ASYNC_ID)
        end on",
        &[],
    )
    .unwrap();
    let mut resumed = plan.clone();
    resumed.scripts = vec![kontra_host::script::plan(&later, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let l = level(&mut e, &[on(60)]);
    assert!((l - 0.25).abs() < 1e-3, "{l}");
    assert_eq!(e.stats().actions_unhandled, 0);

    // After init the host decodes it off the audio thread, and the id
    // completes once it plays.
    let resumed = Arc::new(resumed);
    let mut e = Engine::new(Arc::clone(&resumed), store.client(), RATE);
    assert!((level(&mut e, &[on(60)]) - 0.5).abs() < 1e-3);
    let mut requests = Vec::new();
    e.drain_requests(|r| requests.push(r));
    assert_eq!(requests.len(), 1);
    assert!(
        (level(&mut e, &[]) - 0.5).abs() < 1e-3,
        "nothing until installed"
    );
    let old = e.install(Prepared::new(requests[0], Some(&resumed), RATE));
    assert!(old.request().effect.is_some(), "the room comes back");
    let l = level(&mut e, &[]);
    assert!((l - 0.25).abs() < 1e-3, "{l}");
    assert_eq!(e.last_message(), "1 1");
    assert_eq!(e.stats().actions_unhandled, 0);
}

#[test]
fn a_bus_pan_glides_in_a_line_over_50_ms() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5, 0)], 16);
    for g in &mut plan.groups {
        g.mods.clear();
        g.bus = 1;
    }
    let bus = plan.buses[0].clone();
    plan.buses.push(bus);
    let script = kontra_ksp::compile(
        "on controller
            set_engine_par($ENGINE_PAR_PAN, 0, -1, -1, 1024)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 480];
    e.process(&mut out, &[on(60)]);
    let [l, r] = out[479];
    // Hard left in Kontakt 8: R falls 0.2 every 10 ms, L holds.
    let cc = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, 1, 100),
    };
    e.process(&mut out, &[cc]);
    assert!(
        (out[479][0] - l).abs() < 1e-4 && (out[479][1] - 0.8 * r).abs() < 1e-3,
        "{:?} from {l} {r}",
        out[479]
    );
    let mut rest = vec![[0.0; 2]; 1920];
    e.process(&mut rest, &[]);
    assert!(
        rest[1918][1] > 0.0 && rest[1919][1].abs() < 1e-6,
        "{:?}",
        &rest[1916..]
    );
}
