//! The rack: parts swap in while audio runs, listen on their channel, sum
//! into their output pair, and stop on panic.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, Region, Zone};
use kontra_host::rack::{Mix, Part, Rack, RackAudio};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

const RATE: u32 = 48_000;
const BLOCK: usize = 256;

/// An engine whose one zone plays a constant `level` on every key.
fn engine(store: &mut Store, level: f32) -> Box<Engine> {
    let plan = plan_of(store, level);
    Box::new(Engine::new(Arc::new(plan), store.client(), RATE))
}

/// The plan of [`engine`].
fn plan_of(store: &mut Store, level: f32) -> Plan {
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let frames = 96_000;
    let bytes = (0..frames).flat_map(|_| level.to_le_bytes()).collect();
    let key = BlobKey {
        file: format!("dc{level}").into(),
        offset: 0,
        fmt,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(fmt, bytes)),
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
    let mut zone = Zone::new(region, 60, RATE);
    zone.keys = 0..=127;
    let mut plan = Plan::new(vec![zone], 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan
}

fn part(name: &str) -> Part {
    Part {
        path: None,
        name: name.into(),
        diagnostics: vec![],
        faces: vec![],
        mix: Mix::DEFAULT,
        solo: false,
        mute: false,
        script: Default::default(),
        plan: None,
        delay_ms: None,
        view: None,
        slot_types: Vec::new(),
    }
}

fn note(channel: u8) -> Event {
    Event::NoteOn {
        note: HostNote {
            port: 0,
            channel,
            key: 60,
            id: -1,
            clap: true,
        },
        velocity: 1.0,
        tune: 0.0,
    }
}

fn rack() -> (Rack, RackAudio) {
    Rack::new(RATE, Store::new(StoreConfig::DEFAULT).unwrap(), BLOCK)
}

/// The mean of output pair `n`'s left side over its last block.
fn level(audio: &RackAudio, n: usize) -> f32 {
    let o = audio.output(n, BLOCK);
    o.iter().map(|s| s[0]).sum::<f32>() / o.len() as f32
}

/// A few blocks with `input` at the start of the first; the outputs' levels
/// at the end.
fn play(audio: &mut RackAudio, input: &[(u32, Event)]) -> [f32; 2] {
    audio.process(BLOCK, input);
    for _ in 0..40 {
        audio.process(BLOCK, &[]);
    }
    [level(audio, 0), level(audio, 1)]
}

#[test]
fn parts_listen_on_their_channel_and_play_into_their_output() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.25);
    let b = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.install(1, b, part("b"));
    rack.set_mix(
        0,
        Mix {
            channel: 1,
            output: 0,
            ..Mix::DEFAULT
        },
    );
    rack.set_mix(
        1,
        Mix {
            channel: 2,
            output: 1,
            ..Mix::DEFAULT
        },
    );
    // Channel 2 (index 1) plays part b alone, on output 1.
    let [l0, l1] = play(&mut audio, &[(0, note(1))]);
    assert!(l0.abs() < 1e-4, "part a heard channel 2: {l0}");
    assert!(l1 > 0.1, "part b silent: {l1}");
    // Channel 1 adds part a on output 0.
    let [l0, _] = play(&mut audio, &[(17, note(0))]);
    assert!(l0 > 0.05, "part a silent: {l0}");
}

#[test]
fn master_solo_and_panic_reach_the_audio_side() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    let b = engine(rack.store_mut(), 0.25);
    rack.install(0, a, part("a"));
    rack.install(1, b, part("b"));
    rack.set_mix(
        1,
        Mix {
            output: 0,
            ..Mix::DEFAULT
        },
    );
    let [both, _] = play(&mut audio, &[(0, note(0))]);
    rack.set_master(0.5);
    let [half, _] = play(&mut audio, &[]);
    assert!((half / both - 0.5).abs() < 0.02, "master: {both} → {half}");
    rack.set_solo_mute(1, true, false);
    let [solo, _] = play(&mut audio, &[]);
    assert!(solo < half * 0.5, "solo b kept a: {half} → {solo}");
    rack.panic();
    let [after, _] = play(&mut audio, &[]);
    // Meters hold the peak since they were last read: the fade-out, then
    // silence.
    assert!(rack.meters().master[0] > 1e-3);
    audio.process(BLOCK, &[]);
    assert!(after.abs() < 1e-4, "panic left sound: {after}");
    let m = rack.meters();
    assert!(m.master[0] < 1e-3);
}

#[test]
fn a_new_part_swaps_in_while_audio_runs_and_the_old_one_is_freed_off_audio() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.25);
    rack.install(0, a, part("a"));
    let [first, _] = play(&mut audio, &[(0, note(0))]);
    // The replacement is built here, on this (control) thread, while the
    // audio side keeps rendering the old part.
    let b = engine(rack.store_mut(), 0.5);
    audio.process(BLOCK, &[]);
    rack.install(0, b, part("b"));
    let [second, _] = play(&mut audio, &[(0, note(0))]);
    assert!(second > first * 1.5, "{first} → {second}");
    assert_eq!(rack.parts()[0].as_ref().unwrap().name, "b");
    rack.tick();
    rack.remove(0);
    let [gone, _] = play(&mut audio, &[(0, note(0))]);
    assert!(gone.abs() < 1e-4);
    rack.tick();
    assert!(rack.stats().is_some());
}

#[test]
fn the_audio_side_moves_to_the_audio_thread() {
    fn send<T: Send + 'static>(_: &T) {}
    let (rack, audio) = rack();
    send(&audio);
    send(&rack);
    std::thread::spawn(move || drop(audio)).join().unwrap();
}

#[test]
fn a_part_moved_to_another_channel_lets_go_of_the_old_channels_notes() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.set_mix(
        0,
        Mix {
            channel: 1,
            ..Mix::DEFAULT
        },
    );
    let [held, _] = play(&mut audio, &[(0, note(0))]);
    assert!(held > 0.1, "{held}");
    // Its note-off on channel 1 can no longer reach it once it listens on 2.
    rack.set_mix(
        0,
        Mix {
            channel: 2,
            ..Mix::DEFAULT
        },
    );
    let [after, _] = play(&mut audio, &[]);
    assert!(
        after.abs() < 1e-4,
        "the channel-1 note still sounds: {after}"
    );
}

#[test]
fn played_events_reach_every_part_whatever_its_channel() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.set_mix(
        0,
        Mix {
            channel: 2,
            ..Mix::DEFAULT
        },
    );
    rack.play(note(0));
    let [level, _] = play(&mut audio, &[]);
    assert!(level > 0.1, "{level}");
}

#[test]
fn a_panic_ends_what_was_played_before_it_and_the_player_needs_no_rack() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    let player = rack.player();
    let held = std::thread::spawn(move || {
        player.play(note(0));
        player.panic();
    });
    held.join().unwrap();
    let [level, _] = play(&mut audio, &[]);
    assert!(
        level.abs() < 1e-4,
        "the note played before the panic sounds: {level}"
    );
    rack.panic();
    rack.play(note(0));
    let [level, _] = play(&mut audio, &[]);
    assert!(
        level > 0.1,
        "the note played after the panic is gone: {level}"
    );
}

#[test]
fn a_held_part_sounds_its_hold_later_and_its_notes_still_end() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.set_hold(0, 3 * BLOCK as u32);
    audio.process(BLOCK, &[(0, note(0))]);
    for _ in 0..2 {
        assert!(level(&audio, 0).abs() < 1e-4, "the held note sounds early");
        audio.process(BLOCK, &[]);
    }
    assert!(level(&audio, 0).abs() < 1e-4, "the held note sounds early");
    let [on, _] = play(&mut audio, &[]);
    assert!(on > 0.1, "the held note never sounds: {on}");
    // A shorter hold lets the notes held under the longer one go.
    rack.set_hold(0, 0);
    let [off, _] = play(&mut audio, &[]);
    assert!(off.abs() < 1e-4, "a note outlived its hold change: {off}");
}

#[test]
fn hold_changes_before_one_block_keep_the_latest_release() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.set_hold(0, 8 * BLOCK as u32);
    audio.process(BLOCK, &[(0, note(0))]);
    // The note is due at 8 blocks; both changes land before one block.
    rack.set_hold(0, 0);
    rack.set_hold(0, 1);
    let [off, _] = play(&mut audio, &[]);
    assert!(
        off.abs() < 1e-4,
        "the held note outlived its release: {off}"
    );
}

/// A DC part whose script loads a high-pass into insert 0 in `on init`
/// and empties the slot again from `on note`, as Shreddage loads an EQ.
fn effect_loading(store: &mut Store) -> Box<Engine> {
    use kontra_engine::ir;
    let mut plan = plan_of(store, 0.5);
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
        reads: 0,
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
    let script = kontra_ksp::compile(
        "on init
            declare $a
            $a := set_engine_par($ENGINE_PAR_EFFECT_TYPE, 7, -1, 0, 1)
        end on
        on note
            $a := set_engine_par($ENGINE_PAR_EFFECT_TYPE, 0, -1, 0, 1)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    Box::new(Engine::new(Arc::new(plan), store.client(), RATE))
}

#[test]
fn an_effect_type_a_script_loads_is_built_off_audio_swapped_in_and_saved() {
    let (mut rack, mut audio) = rack();
    let e = effect_loading(rack.store_mut());
    rack.install(
        0,
        e,
        Part {
            path: Some("dc.nki".into()),
            ..part("a")
        },
    );
    // The high-pass from init holds the DC back; the note asks to empty it.
    let [held, _] = play(&mut audio, &[(0, note(0))]);
    assert!(held.abs() < 1e-3, "the init high-pass is not in: {held}");
    rack.tick();
    let [open, _] = play(&mut audio, &[]);
    assert!(
        (open - 0.5).abs() < 1e-3,
        "the emptied slot did not swap in: {open}"
    );
    let p = rack.parts()[0].as_ref().unwrap();
    assert_eq!(p.slot_types, [(kontra_engine::ir::RackAt::Inserts, 0, 0)]);
    // The old unit comes back to be freed here; the save carries the type.
    rack.tick();
    let saved = rack.save();
    let json = saved.write();
    assert_eq!(kontra_host::state::Saved::read(&json), saved);
    assert_eq!(saved.parts[0].slot_types.len(), 1, "{json}");
}
