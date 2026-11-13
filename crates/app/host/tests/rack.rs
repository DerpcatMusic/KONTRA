//! The rack: parts swap in while audio runs, listen on their channel, sum
//! into their output pair, and stop on panic.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, Region, Zone};
use kontra_host::rack::{Hold, Mix, Part, Rack, RackAudio};
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
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
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
        source: None,
        recall: kontra_engine::Recall::Restart,
        name: name.into(),
        diagnostics: vec![],
        faces: vec![],
        mix: Mix::DEFAULT,
        solo: false,
        mute: false,
        script: Default::default(),
        plan: None,
        delays: None,
        view: None,
        slot_types: Vec::new(),
        slot_responses: Vec::new(),
        modified: false,
        named: Default::default(),
        sounds: Default::default(),
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

/// The host stopping lets go of what it played, pedal down or not, as v2
/// did: a host that sends no note-off at stop leaves nothing sounding.
#[test]
fn the_host_stopping_lets_go_of_its_notes_and_the_pedal() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    audio.set_transport(true, 0.0, (4, 4));
    let pedal = Event::midi1(0xB0, 64, 127);
    let [held, _] = play(&mut audio, &[(0, pedal), (0, note(0))]);
    assert!(held > 0.1, "{held}");
    audio.set_transport(true, 1.0, (4, 4));
    let [running, _] = play(&mut audio, &[]);
    assert!(running > 0.1, "a running song let go: {running}");
    audio.set_transport(false, 1.0, (4, 4));
    let [stopped, _] = play(&mut audio, &[]);
    assert!(stopped.abs() < 1e-4, "the note still sounds: {stopped}");
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
    rack.set_hold(0, Hold::flat(3 * BLOCK as u32));
    audio.process(BLOCK, &[(0, note(0))]);
    for _ in 0..2 {
        assert!(level(&audio, 0).abs() < 1e-4, "the held note sounds early");
        audio.process(BLOCK, &[]);
    }
    assert!(level(&audio, 0).abs() < 1e-4, "the held note sounds early");
    let [on, _] = play(&mut audio, &[]);
    assert!(on > 0.1, "the held note never sounds: {on}");
    // A shorter hold lets the notes held under the longer one go.
    rack.set_hold(0, Hold::flat(0));
    let [off, _] = play(&mut audio, &[]);
    assert!(off.abs() < 1e-4, "a note outlived its hold change: {off}");
}

#[test]
fn hold_changes_before_one_block_keep_the_latest_release() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.5);
    rack.install(0, a, part("a"));
    rack.set_hold(0, Hold::flat(8 * BLOCK as u32));
    audio.process(BLOCK, &[(0, note(0))]);
    // The note is due at 8 blocks; both changes land before one block.
    rack.set_hold(0, Hold::flat(0));
    rack.set_hold(0, Hold::flat(1));
    let [off, _] = play(&mut audio, &[]);
    assert!(
        off.abs() < 1e-4,
        "the held note outlived its release: {off}"
    );
}

#[test]
fn a_legato_waits_its_own_hold_and_a_legato_script_keeps_the_order() {
    for mono in [false, true] {
        let (mut rack, mut audio) = rack();
        let a = engine(rack.store_mut(), 0.5);
        rack.install(0, a, part("a"));
        let hold = Hold {
            steps: [(127, 6 * BLOCK as u32, 0); 8],
            mono,
        };
        rack.set_hold(0, hold);
        audio.process(BLOCK, &[(0, note(0))]);
        // Another key is down: a legato, with no hold of its own.
        audio.process(BLOCK, &[(0, note(1))]);
        audio.process(BLOCK, &[]);
        let sounds = level(&audio, 0) > 0.1;
        assert_eq!(sounds, !mono, "a legato's onset, mono {mono}");
    }
}

/// A DC part whose script loads a high-pass into insert 0 in `on init`
/// and empties the slot again from `on note`, as Shreddage loads an EQ.
fn effect_loading(store: &mut Store) -> Box<Engine> {
    let plan = effect_plan(store, 7, 0);
    Box::new(Engine::new(Arc::new(plan), store.client(), RATE))
}

/// A part's plan whose script loads effect type `init` (7, a high-pass;
/// 0 empties) into insert slot 0 in `on init` and `note` on each note.
fn effect_plan(store: &mut Store, init: u16, note: u16) -> Plan {
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
        kind: id,
        effect,
        laws: Vec::new().into(),
        model: None,
    };
    let effect = ir::Effect {
        kind: ir::EffectKind::Filter(high),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    plan.slot_shapes.types = vec![ty(0, None), ty(7, Some(effect))];
    let script = kontra_ksp::compile(
        &format!(
            "on init
                declare $a
                $a := set_engine_par($ENGINE_PAR_EFFECT_TYPE, {init}, -1, 0, 1)
            end on
            on note
                $a := set_engine_par($ENGINE_PAR_EFFECT_TYPE, {note}, -1, 0, 1)
            end on"
        ),
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    plan
}

/// A preset recalled by restarting the part's engine keeps the effects its
/// script loaded since the part loaded: the restart's plan has them.
#[test]
fn a_restarted_recall_keeps_the_effects_a_script_loaded_after_init() {
    let (mut rack, mut audio) = rack();
    let plan = Arc::new(effect_plan(rack.store_mut(), 0, 7));
    let engine = Box::new(Engine::new(
        Arc::clone(&plan),
        rack.store_mut().client(),
        RATE,
    ));
    let base = Some(std::path::PathBuf::from("dc.nki"));
    let p = Part {
        path: base.clone(),
        source: base,
        plan: Some(plan),
        ..part("a")
    };
    rack.install(0, engine, p);
    play(&mut audio, &[(0, note(0))]);
    rack.tick();
    let high = |rack: &Rack| {
        let p = rack.parts()[0].as_ref().unwrap();
        let slot = p
            .plan
            .as_ref()
            .and_then(|plan| plan.inserts.first().cloned());
        (p.slot_types.clone(), slot.flatten().is_some())
    };
    // Type 7 is the plan's second slot type.
    let loaded = vec![(kontra_engine::ir::RackAt::Inserts, 0, 1)];
    assert_eq!(high(&rack).0, loaded, "the note's load is recorded");
    let preset = kontra_host::instrument::Preset {
        name: "b".into(),
        base: "dc.nki".into(),
        saved: Vec::new(),
        effects: false,
    };
    assert!(rack.recall(0, &preset, None));
    assert_eq!(high(&rack), (loaded, true), "the restart's plan has it");
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

/// A part whose insert convolves with a unit response until its note-on script
/// loads the plan's response "half" from `wav`, written as `name`; returns
/// the rack after that note's block, the convolver still on the unit.
fn loading_response(wav: &[u8], name: &str) -> (Rack, RackAudio) {
    use kontra_engine::{Response, ir};
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&file, wav).unwrap();
    let (mut rack, mut audio) = rack();
    let mut plan = plan_of(rack.store_mut(), 0.5);
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
    let law = ir::Impulse {
        sample: 0,
        reverse: false,
        normalize: None,
        predelay_ms: 0.0,
        cuts: None,
        envelope: None,
        mix: None,
    };
    plan.impulse_laws = vec![("room".into(), law)];
    let sample = ir::Sample {
        name: "Half.wav".into(),
        file,
        offset: 0,
        len: None,
        channels: 0,
        frames: 0,
        rate: 0,
        keystream: None,
    };
    plan.responses = vec![("half".into(), sample)];
    let script = kontra_ksp::compile(
        "on note
            load_ir_sample(\"Half.ncw\", 0, $NI_INSERT_BUS)
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let plan = Arc::new(plan);
    let e = Box::new(Engine::new(
        Arc::clone(&plan),
        rack.store_mut().client(),
        RATE,
    ));
    let info = Part {
        path: Some("ir.nki".into()),
        plan: Some(plan),
        ..part("a")
    };
    rack.install(0, e, info);
    let [room, _] = play(&mut audio, &[(0, note(0))]);
    assert!((room - 0.5).abs() < 1e-3, "{room}");
    (rack, audio)
}

#[test]
fn a_response_a_script_loads_later_is_decoded_off_audio_swapped_in_and_saved() {
    use kontra_engine::ir;
    // One frame at half scale, 16-bit stereo.
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
    let (mut rack, mut audio) = loading_response(&wav, "rack-half.wav");
    rack.tick();
    let [half, _] = play(&mut audio, &[]);
    assert!(
        (half - 0.25).abs() < 1e-3,
        "the response did not swap in: {half}"
    );
    let p = rack.parts()[0].as_ref().unwrap();
    assert_eq!(p.slot_responses, [(ir::RackAt::Inserts, 0, "half".into())]);
    assert!(p.slot_types.is_empty(), "a response is no type");
    rack.tick();
    let saved = rack.save();
    let json = saved.write();
    assert_eq!(kontra_host::state::Saved::read(&json), saved);
    assert_eq!(saved.parts[0].slot_responses[0].name, "half", "{json}");
    // The file goes bad: a second load fails, the slot and the save keep "half".
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("rack-half.wav");
    std::fs::write(file, b"not a wav").unwrap();
    play(&mut audio, &[(0, note(0))]);
    rack.tick();
    // Two held notes through "half"; the unit room would give 1.0.
    let [still, _] = play(&mut audio, &[]);
    assert!((still - 0.5).abs() < 1e-3, "{still}");
    rack.tick();
    assert_eq!(rack.save(), saved);
}

#[test]
fn a_response_that_fails_to_decode_keeps_the_slot_and_is_not_saved() {
    let (mut rack, mut audio) = loading_response(b"not a wav", "rack-bad.wav");
    rack.tick();
    let [out, _] = play(&mut audio, &[]);
    assert!((out - 0.5).abs() < 1e-3, "the slot changed: {out}");
    let p = rack.parts()[0].as_ref().unwrap();
    assert!(p.slot_responses.is_empty() && p.slot_types.is_empty());
    rack.tick();
    assert!(rack.save().parts[0].slot_responses.is_empty());
}

#[test]
fn the_hosts_transport_reaches_every_parts_scripts() {
    let (mut rack, mut audio) = rack();
    let mut plan = plan_of(rack.store_mut(), 0.5);
    let script = kontra_ksp::compile(
        "on note
            if ($NI_TRANSPORT_RUNNING = 0 or $NI_SONG_POSITION < 960 * 4)
                ignore_event($EVENT_ID)
            end if
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Box::new(Engine::new(Arc::new(plan), rack.store_mut().client(), RATE));
    rack.install(0, e, part("a"));
    audio.set_transport(false, 8.0, (4, 4));
    let [stopped, _] = play(&mut audio, &[(0, note(0))]);
    assert!(
        stopped.abs() < 1e-4,
        "played with the transport stopped: {stopped}"
    );
    // Running, at bar 3 of 4/4: the script lets the note through.
    audio.set_transport(true, 8.0, (4, 4));
    let [running, _] = play(&mut audio, &[(0, note(0))]);
    assert!(
        running > 0.1,
        "the running transport did not reach the script: {running}"
    );
}

#[test]
fn a_held_part_hears_the_song_position_where_its_notes_land() {
    let (mut rack, mut audio) = rack();
    let mut plan = plan_of(rack.store_mut(), 0.5);
    let script = kontra_ksp::compile(
        "on note
            if ($NI_SONG_POSITION < 960 * 4)
                ignore_event($EVENT_ID)
            end if
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Box::new(Engine::new(Arc::new(plan), rack.store_mut().client(), RATE));
    rack.install(0, e, part("a"));
    // Half a second at 120 bpm: one quarter.
    rack.set_hold(0, Hold::flat(RATE / 2));
    audio.set_tempo(120.0);
    // The song runs on while the note is held: 120 bpm, two quarters a second.
    let mut after_hold = |at: f64, input: &[(u32, Event)]| {
        audio.set_transport(true, at, (4, 4));
        audio.process(BLOCK, input);
        for block in 1..=(RATE as usize / 2 / BLOCK + 2) {
            let quarters = at + (block * BLOCK) as f64 / f64::from(RATE) * 2.0;
            audio.set_transport(true, quarters, (4, 4));
            audio.process(BLOCK, &[]);
        }
        level(&audio, 0)
    };
    // Played at 3.5, it lands at 4.5 and hears 3.5: the script lets it go.
    let early = after_hold(3.5, &[(0, note(0))]);
    assert!(
        early.abs() < 1e-4,
        "the held part heard the unshifted song: {early}"
    );
    let late = after_hold(4.5, &[(0, note(0))]);
    assert!(late > 0.1, "the held note never sounded: {late}");
}

#[test]
fn a_held_part_hears_the_host_stop_its_hold_later() {
    let (mut rack, mut audio) = rack();
    let mut plan = plan_of(rack.store_mut(), 0.5);
    let script = kontra_ksp::compile(
        "on note
            if ($NI_TRANSPORT_RUNNING = 0)
                ignore_event($EVENT_ID)
            end if
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let e = Box::new(Engine::new(Arc::new(plan), rack.store_mut().client(), RATE));
    rack.install(0, e, part("a"));
    rack.set_hold(0, Hold::flat(RATE / 2));
    // Played while the song runs; the host stops a block later. The held
    // note lands half a second on, when the part has not heard the stop yet.
    audio.set_transport(true, 0.0, (4, 4));
    audio.process(BLOCK, &[(0, note(0))]);
    audio.set_transport(false, 0.0, (4, 4));
    // The stop lets the note go a hold later too, so it sounds a while.
    let mut loudest = 0f32;
    for _ in 0..(RATE as usize / 2 / BLOCK + 2) {
        audio.process(BLOCK, &[]);
        loudest = loudest.max(level(&audio, 0));
    }
    let level = loudest;
    assert!(
        level > 0.1,
        "the held part heard the stop before its note: {level}"
    );
}

#[test]
fn a_sound_tab_decay_edit_is_heard_from_the_next_note() {
    use kontra_host::sound::{Param, Sounds};
    let (mut rack, mut audio) = rack();
    let e = engine(rack.store_mut(), 0.5);
    let sounds = Sounds::new(RATE, e.group_sounds());
    rack.install(
        0,
        e,
        Part {
            sounds,
            ..part("a")
        },
    );
    let [held, _] = play(&mut audio, &[(0, note(0))]);
    assert!(held > 0.4, "the held note is not heard: {held}");
    rack.panic();
    play(&mut audio, &[]);
    rack.set_sound(0, None, Param::Sustain, 0.0);
    rack.set_sound(0, None, Param::Decay, 0.02);
    let p = rack.parts().first().and_then(Option::as_ref).unwrap();
    assert_eq!(
        p.sounds.envelope(0).map(|e| (e[2], e[3])),
        Some((0.02, 0.0))
    );
    let [decayed, _] = play(&mut audio, &[(0, note(0))]);
    assert!(
        decayed.abs() < 1e-3,
        "the decay edit was not heard: {decayed}"
    );
}

/// A Lua script's state comes back on reload even when restoring it waits:
/// `onLoad` here, as UVI's layer loads do, finishes in a task after a
/// `wait`. The save must hold what the task set, not what was there before.
#[test]
fn a_lua_state_restored_after_a_wait_saves_as_restored() {
    use kontra_engine::ir;
    let source = r#"
        local layer = "default"
        function onSave() return {layer = layer} end
        function onLoad(data) run(function() wait(50) layer = data.layer end) end
    "#;
    let instrument = |saved: Vec<String>| ir::Instrument {
        scripts: vec![ir::Script {
            name: "state".into(),
            slot: 0,
            dialect: ir::Dialect::Lua,
            source: source.into(),
            saved,
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    };
    let state = |rack: &Rack| {
        let saved = rack.saved_scripts(0);
        saved[0].iter().find(|s| s.starts_with("__state=")).cloned()
    };
    let saved = vec![r#"__state={["layer"]="chosen",}"#.to_owned()];
    let (mut rack, mut audio) = rack();
    rack.load_ir(0, instrument(saved.clone()), None).unwrap();
    // A save asked for every block, over 200 ms: the wait ends inside it.
    for _ in 0..40 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
    assert_eq!(state(&rack), Some(saved[0].clone()));
}

/// A million-cell persistent array is captured a budget of cells a block,
/// never all in one; an idle script is not read again, and what a callback
/// wrote reaches the save once the next capture is read.
#[test]
fn a_big_persistent_array_is_captured_over_blocks_and_saves_what_changed() {
    use kontra_engine::ir;
    let source = "on init
            declare %big[100000]
            make_persistent(%big)
            %big[99999] := 7
        end on
        on note
            %big[99999] := %big[99999] + 1
        end on";
    let instrument = ir::Instrument {
        scripts: vec![ir::Script {
            name: "big".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source: source.into(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    };
    let (mut rack, mut audio) = rack();
    rack.load_ir(0, instrument, None).unwrap();
    let last = |rack: &Rack| {
        let saved = rack.saved_scripts(0).concat();
        let big = saved.into_iter().find(|s| s.starts_with("%big "));
        big.and_then(|b| b.rsplit(' ').next().map(str::to_owned))
    };
    // Blocks until a capture is back: 100,000 cells at 128 a frame of a
    // 256-frame block take four.
    let captured = |rack: &mut Rack, audio: &mut RackAudio| {
        rack.tick();
        let before = last(rack);
        (1..=20)
            .find(|_| {
                audio.process(BLOCK, &[]);
                rack.tick();
                last(rack) != before
            })
            .unwrap_or(0)
    };
    assert_eq!(captured(&mut rack, &mut audio), 4);
    assert_eq!(last(&rack).as_deref(), Some("7"));
    // Idle, the next capture reads nothing: back after one block.
    rack.tick();
    audio.process(BLOCK, &[]);
    rack.tick();
    assert_eq!(last(&rack).as_deref(), Some("7"));
    audio.process(BLOCK, &[(0, note(0))]);
    for _ in 0..8 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
    assert_eq!(last(&rack).as_deref(), Some("8"), "the note's write");
}

/// An instrument read from "a.nki" whose one script keeps `$knob` and
/// counts, in `$marker`, init (5) and each persistence-changed (+1); its
/// snapshot type is `kind`, its saved values `saved`.
fn snapshot_of(kind: u8, saved: &[&str]) -> kontra_engine::ir::Instrument {
    use kontra_engine::ir;
    let source = format!(
        "on init
            set_snapshot_type({kind})
            declare ui_knob $knob (0, 127, 1)
            make_persistent($knob)
            declare $marker
            make_persistent($marker)
            $marker := 5
        end on
        on persistence_changed
            $marker := $marker + 1
            $knob := 0
            read_persistent_var($knob)
        end on"
    );
    ir::Instrument {
        source_path: Some("a.nki".into()),
        scripts: vec![ir::Script {
            name: "s".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source,
            saved: saved.iter().map(|&s| s.to_owned()).collect(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    }
}

/// Part 0's saved script values, as of a capture taken now.
fn saved_now(rack: &mut Rack, audio: &mut RackAudio) -> Vec<String> {
    // A recall lends the engine and takes it back over a round of its own.
    for _ in 0..2 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    rack.tick();
    rack.saved_scripts(0).concat()
}

/// KSP manual, `set_snapshot_type`: type 1 recalls without `on init` (the
/// running script keeps its state), type 0 runs `on init` again; both run
/// `on persistence_changed` after the values land, and both stop the part
/// (a new generation). Neither reads or prepares the instrument again.
#[test]
fn a_snapshot_recalls_into_the_loaded_instrument_as_its_type_asks() {
    for (kind, marker, rebuilt) in [(1, "$marker 7", false), (0, "$marker 6", true)] {
        let (mut rack, mut audio) = rack();
        rack.load_ir(0, snapshot_of(kind, &[]), Some("a.nki".into()))
            .unwrap();
        audio.process(BLOCK, &[]);
        let p = rack.parts()[0].as_ref().unwrap();
        let (generation, plan) = (p.script.generation(), p.plan.clone().unwrap());
        let preset = kontra_host::instrument::Preset {
            name: "b".into(),
            base: "a.nki".into(),
            saved: vec![vec!["$knob 42".into()]],
            effects: false,
        };
        assert!(rack.recall(0, &preset, Some(std::path::Path::new("b.nksn"))));
        let saved = saved_now(&mut rack, &mut audio);
        let p = rack.parts()[0].as_ref().unwrap();
        assert!(
            saved.contains(&"$knob 42".to_owned()),
            "type {kind}: {saved:?}"
        );
        assert!(saved.contains(&marker.to_owned()), "type {kind}: {saved:?}");
        assert_eq!(p.path.as_deref(), Some(std::path::Path::new("b.nksn")));
        assert_ne!(p.script.generation(), generation, "type {kind}");
        // The plan is kept or copied, never prepared again from the file.
        let same = Arc::ptr_eq(p.plan.as_ref().unwrap(), &plan);
        assert_eq!(same, !rebuilt, "type {kind}");
        // The next snapshot recalls as well.
        assert!(rack.recall(0, &preset, None), "type {kind}: second recall");
    }
}

/// A snapshot that saves effects is rebuilt off the rack and installed
/// with them (here an instrument in the repo stands in for its file: its
/// effects are what a recall reads), and any later recall supersedes one
/// still being made: the part ends on the later pick.
#[test]
fn a_native_recall_installs_its_effects_and_a_later_recall_supersedes_it() {
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../tools/kontakt-reference/scenarios/cutoff_mod.nki"
    );
    let fixture = std::path::Path::new(fixture);
    let preset = |knob: &str, effects| kontra_host::instrument::Preset {
        name: "b".into(),
        base: "a.nki".into(),
        saved: vec![vec![format!("$knob {knob}")]],
        effects,
    };
    let inserts = |rack: &Rack| {
        let p = rack.parts()[0].as_ref().unwrap();
        p.plan.as_ref().unwrap().inserts.iter().flatten().count()
    };
    // Waits up to a second of ticks for the part's generation to move.
    let settled = |rack: &mut Rack, audio: &mut RackAudio, from: u64| {
        for _ in 0..200 {
            rack.tick();
            audio.process(BLOCK, &[]);
            if rack.parts()[0].as_ref().unwrap().script.generation() != from {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    };
    let (mut rack, mut audio) = self::rack();
    rack.load_ir(0, snapshot_of(1, &[]), Some("a.nki".into()))
        .unwrap();
    audio.process(BLOCK, &[]);
    assert_eq!(inserts(&rack), 0);
    let generation = rack.parts()[0].as_ref().unwrap().script.generation();
    assert!(rack.recall(0, &preset("11", true), Some(fixture)));
    assert!(settled(&mut rack, &mut audio, generation), "it installs");
    assert!(inserts(&rack) > 0, "with the snapshot's effects");
    assert!(saved_now(&mut rack, &mut audio).contains(&"$knob 11".to_owned()));
    // A native recall, then a script-only one before it lands.
    let (mut rack, mut audio) = self::rack();
    rack.load_ir(0, snapshot_of(1, &[]), Some("a.nki".into()))
        .unwrap();
    audio.process(BLOCK, &[]);
    let generation = rack.parts()[0].as_ref().unwrap().script.generation();
    assert!(rack.recall(0, &preset("11", true), Some(fixture)));
    assert!(rack.recall(
        0,
        &preset("42", false),
        Some(std::path::Path::new("c.nksn"))
    ));
    // The script-only recall lands; the first never does.
    assert!(settled(&mut rack, &mut audio, generation));
    for _ in 0..50 {
        rack.tick();
        audio.process(BLOCK, &[]);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(inserts(&rack), 0, "the first never lands");
    let p = rack.parts()[0].as_ref().unwrap();
    assert_eq!(p.path.as_deref(), Some(std::path::Path::new("c.nksn")));
    assert!(saved_now(&mut rack, &mut audio).contains(&"$knob 42".to_owned()));
}

/// What a snapshot's effects report reaches the part they are installed
/// in: here a snapshot whose file cannot be read lands its values and says
/// why its effects are the base's.
#[test]
fn a_native_recall_reports_what_its_effects_could_not_carry() {
    let (mut rack, mut audio) = self::rack();
    rack.load_ir(0, snapshot_of(1, &[]), Some("a.nki".into()))
        .unwrap();
    audio.process(BLOCK, &[]);
    let generation = rack.parts()[0].as_ref().unwrap().script.generation();
    let preset = kontra_host::instrument::Preset {
        name: "b".into(),
        base: "a.nki".into(),
        saved: vec![vec!["$knob 9".into()]],
        effects: true,
    };
    assert!(rack.recall(0, &preset, Some(std::path::Path::new("/nowhere/b.nksn"))));
    for _ in 0..200 {
        rack.tick();
        audio.process(BLOCK, &[]);
        if rack.parts()[0].as_ref().unwrap().script.generation() != generation {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let p = rack.parts()[0].as_ref().unwrap();
    assert_ne!(p.script.generation(), generation, "it installs");
    let unread = p
        .diagnostics
        .iter()
        .any(|d| d.feature == "snapshot effects unread");
    assert!(unread, "{:?}", p.diagnostics);
}

/// A preset the player changed is saved as changed; picking a preset
/// again (a recall) makes it the preset as saved.
#[test]
fn a_changed_preset_saves_as_changed_until_a_preset_is_picked() {
    let (mut rack, mut audio) = rack();
    rack.load_ir(0, snapshot_of(1, &[]), Some("a.nki".into()))
        .unwrap();
    audio.process(BLOCK, &[]);
    assert!(!rack.save().parts[0].modified, "a fresh load is the preset");
    rack.set_modified(0);
    assert!(rack.save().parts[0].modified);
    let preset = kontra_host::instrument::Preset {
        name: "b".into(),
        base: "a.nki".into(),
        saved: vec![vec!["$knob 42".into()]],
        effects: false,
    };
    assert!(rack.recall(0, &preset, Some(std::path::Path::new("b.nksn"))));
    assert!(
        !rack.save().parts[0].modified,
        "a picked preset is unchanged"
    );
}

/// A user preset renamed or deleted moves every part that plays it, by
/// one path: renamed, they keep their changed mark; deleted, they play on
/// as their base instrument, changed.
#[test]
fn a_renamed_or_deleted_user_preset_moves_every_part_playing_it() {
    use std::path::Path;
    let (mut rack, _audio) = rack();
    for part in [0, 1] {
        let instrument = snapshot_of(1, &[]);
        (rack.load_ir(part, instrument, Some("u/Mine.kontra".into()))).unwrap();
    }
    rack.set_modified(1);
    let playing = |rack: &Rack| {
        (rack.parts().iter().flatten())
            .map(|p| (p.path.clone().unwrap(), p.name.clone(), p.modified))
            .collect::<Vec<_>>()
    };
    rack.preset_moved(
        Path::new("u/Mine.kontra"),
        Some(Path::new("u/Yours.kontra")),
    );
    let yours = |modified| ("u/Yours.kontra".into(), "Yours".to_owned(), modified);
    assert_eq!(playing(&rack), [yours(false), yours(true)]);
    rack.preset_moved(Path::new("u/Yours.kontra"), None);
    // They keep the sound, named by their instrument, changed.
    let base = ("a.nki".into(), "a".to_owned(), true);
    assert_eq!(playing(&rack), [base.clone(), base]);
}

/// A folder of this test process alone, removed when dropped (also when
/// an assertion fails).
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let tmp = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
        let dir = tmp.join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// KSP manual (`save_array_str`, `load_array_str`): after `on init` both run
/// asynchronously, then `on async_complete`; the file holds the array's
/// name and its values, and a file naming another array loads nothing.
/// Saves go to the rack's save folder, never into the library; a save that
/// cannot be written reports 0. Only regular files no larger than a
/// script resource are read.
#[test]
fn an_array_saved_by_path_loads_back_and_a_file_of_another_array_does_not() {
    use kontra_engine::ir;
    let scratch = Scratch::new("array-files");
    let dir = scratch.0.clone();
    std::fs::create_dir_all(dir.join("dir.nka")).unwrap();
    std::fs::write(dir.join("other.nka"), "%other\n7\n7\n7\n").unwrap();
    // Valid values, then (sparse) padding past the ceiling.
    std::fs::write(dir.join("huge.nka"), "%a\n4\n5\n6\n").unwrap();
    let huge = std::fs::OpenOptions::new()
        .write(true)
        .open(dir.join("huge.nka"));
    (huge.unwrap())
        .set_len(kontra_format_kontakt::RESOURCE_LIMIT + 1)
        .unwrap();
    let saves = dir.join("saves");
    let mine = saves.join(dir.strip_prefix("/").unwrap_or(&dir));
    std::fs::create_dir_all(&mine).unwrap();
    // A file where a save needs a folder: that save cannot be written.
    std::fs::write(mine.join("blocked.nka"), "").unwrap();
    let source = format!(
        "on init
            declare %a[3] := (1, 2, 3)
            declare $step
            declare $id
            declare $result
            make_persistent($result)
        end on
        on note
            $id := save_array_str(%a, \"{0}/a.nka\")
        end on
        on async_complete
            select $step
                case 0
                    $result := $NI_ASYNC_EXIT_STATUS
                    %a[0] := 9
                    %a[1] := 9
                    %a[2] := 9
                    $id := load_array_str(%a, \"{0}/a.nka\")
                case 1
                    $result := $result * 1000 + %a[0] * 100 + %a[1] * 10 + %a[2]
                    $id := load_array_str(%a, \"{0}/other.nka\")
                case 2
                    $result := $result * 10 + $NI_ASYNC_EXIT_STATUS + %a[0] - 1
                    $id := save_array_str(%a, \"{0}/blocked.nka/a.nka\")
                case 3
                    $result := $result * 10 + $NI_ASYNC_EXIT_STATUS
                    $id := load_array_str(%a, \"{0}/huge.nka\")
                case 4
                    $result := $result * 10 + $NI_ASYNC_EXIT_STATUS + %a[0] - 1
                    $id := load_array_str(%a, \"{0}/dir.nka\")
                case 5
                    $result := $result * 10 + $NI_ASYNC_EXIT_STATUS
            end select
            inc($step)
        end on",
        dir.display()
    );
    let instrument = ir::Instrument {
        scripts: vec![ir::Script {
            name: "s".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source,
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    };
    let (mut rack, mut audio) = rack();
    rack.set_saves(saves.clone());
    rack.load_ir(0, instrument, None).unwrap();
    audio.process(BLOCK, &[(0, note(0))]);
    for _ in 0..16 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    let saved = saved_now(&mut rack, &mut audio);
    let written = std::fs::read_to_string(mine.join("a.nka"));
    assert_eq!(written.unwrap(), "%a\n1\n2\n3\n");
    assert!(
        !dir.join("a.nka").exists(),
        "a save never writes the library"
    );
    // Saved 1; loaded back 123; wrong name 0; unwritable 0; huge 0; dir 0.
    assert!(saved.contains(&"$result 11230000".to_owned()), "{saved:?}");
}

/// Some blocks and ticks, for a script's waits and the control side.
fn run(rack: &mut Rack, audio: &mut RackAudio, blocks: usize) {
    for _ in 0..blocks {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
}

/// An instrument at `source` with one script.
fn scripted(
    source: &std::path::Path,
    dialect: kontra_engine::ir::Dialect,
    code: &str,
) -> kontra_engine::ir::Instrument {
    use kontra_engine::ir;
    ir::Instrument {
        source_path: Some(source.to_owned()),
        scripts: vec![ir::Script {
            name: "s".into(),
            slot: 0,
            dialect,
            source: code.to_owned(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    }
}

/// `save_array(%a, 0)` and `load_array(%a, 0)` ask for a file: the editor
/// asks the player ([`Rack::pick`]) among the instrument's own saves, and
/// the answer goes on as a save or load by path under them.
#[test]
fn an_array_saved_by_dialog_loads_back_from_the_file_picked() {
    use kontra_engine::ir;
    let scratch = Scratch::new("array-dialog");
    let saves = scratch.0.join("saves");
    let source = scratch.0.join("lib/x.nki");
    let code = "on init
            declare %perf[2] := (4, 5)
            declare $step
            declare $id
            declare $result
            make_persistent($result)
        end on
        on note
            $id := save_array(%perf, 0)
        end on
        on async_complete
            if ($step = 0)
                %perf[0] := 0
                $id := load_array(%perf, 0)
            else
                $result := $NI_ASYNC_EXIT_STATUS * 100 + %perf[0] * 10 + %perf[1]
            end if
            inc($step)
        end on";
    let (mut rack, mut audio) = rack();
    rack.set_saves(saves.clone());
    rack.load_ir(0, scripted(&source, ir::Dialect::Ksp, code), None)
        .unwrap();
    audio.process(BLOCK, &[(0, note(0))]);
    run(&mut rack, &mut audio, 4);
    let pick = rack.pick().expect("the save asks for a file");
    assert!(pick.save && pick.names.is_empty(), "{pick:?}");
    rack.picked(Some("Mine"));
    run(&mut rack, &mut audio, 4);
    let pick = rack.pick().expect("the load asks for a file");
    assert_eq!((pick.save, pick.names), (false, vec!["Mine".to_owned()]));
    rack.picked(Some("Mine"));
    let saved = saved_now(&mut rack, &mut audio);
    let own = saves.join(source.strip_prefix("/").unwrap_or(&source));
    let written = std::fs::read_to_string(own.join("Mine.nka"));
    assert_eq!(written.unwrap(), "%perf\n4\n5\n");
    assert!(saved.contains(&"$result 145".to_owned()), "{saved:?}");
}

/// A UVI script's own presets: `browseForFile` asks the editor, `saveState`
/// is written under the instrument's saves, and a later load of the
/// instrument reads it back as the bank's own file.
#[test]
fn a_script_preset_saved_to_disk_loads_after_a_reload() {
    use kontra_engine::ir;
    let scratch = Scratch::new("script-presets");
    let saves = scratch.0.join("saves");
    let source = scratch.0.join("lib/x.uvip");
    let code = r#"
        local air = Knob('air', 0, 0, 100)
        function onNote(e)
            browseForFile('save', 'Save', '', '*.VHFPreset', function(t)
                air.value = 40
                saveState(t.result)
            end)
        end"#;
    let (mut rack, mut audio) = rack();
    rack.set_saves(saves.clone());
    rack.load_ir(0, scripted(&source, ir::Dialect::Lua, code), None)
        .unwrap();
    audio.process(BLOCK, &[(0, note(0))]);
    run(&mut rack, &mut audio, 4);
    let pick = rack.pick().expect("browseForFile asks for a file");
    assert!(pick.save, "{pick:?}");
    rack.picked(Some("Mine"));
    run(&mut rack, &mut audio, 40);
    let own = saves.join(source.strip_prefix("/").unwrap_or(&source));
    let written = std::fs::read_to_string(own.join("Mine.VHFPreset")).expect("written");
    assert!(written.contains(r#"["air"]=40"#), "{written}");
    // Loaded again, the script opens the preset it saved.
    let reload =
        "local air = Knob('air', 0, 0, 100)\nfunction onInit() loadState('Mine.VHFPreset') end";
    rack.load_ir(0, scripted(&source, ir::Dialect::Lua, reload), None)
        .unwrap();
    let saved = saved_now(&mut rack, &mut audio);
    assert!(saved.iter().any(|e| e == "air=40"), "{saved:?}");
}

/// An install fades out the engine it replaces instead of cutting its
/// sound dead, a second install before that fade ends keeps it fading, and
/// the faded engines are freed off audio.
#[test]
fn an_install_fades_out_the_engine_it_replaces() {
    let (mut rack, mut audio) = rack();
    let a = engine(rack.store_mut(), 0.25);
    rack.install(0, a, part("a"));
    let [before, _] = play(&mut audio, &[(0, note(0))]);
    let silent = engine(rack.store_mut(), 0.0);
    rack.install(0, silent, part("b"));
    let again = engine(rack.store_mut(), 0.0);
    rack.install(0, again, part("c"));
    audio.process(BLOCK, &[]);
    let fading = level(&audio, 0);
    assert!(fading > 0.0 && fading < before, "{before} → {fading}");
    let [after, _] = play(&mut audio, &[]);
    assert!(after.abs() < 1e-6, "{after}");
    rack.tick();
}

/// A rack with no save folder (a test, a tool) writes nothing: a script's
/// save reports 0 and no file appears anywhere.
#[test]
fn a_rack_without_a_save_folder_writes_nothing() {
    use kontra_engine::ir;
    let scratch = Scratch::new("no-saves");
    let at = scratch.0.join("a.nka");
    let source = format!(
        "on init
            declare %a[1] := (4)
            declare $id
            declare $status := 9
            make_persistent($status)
        end on
        on note
            $id := save_array_str(%a, \"{}\")
        end on
        on async_complete
            $status := $NI_ASYNC_EXIT_STATUS
        end on",
        at.display()
    );
    let instrument = ir::Instrument {
        scripts: vec![ir::Script {
            name: "s".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source,
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    };
    let (mut rack, mut audio) = rack();
    rack.load_ir(0, instrument, None).unwrap();
    audio.process(BLOCK, &[(0, note(0))]);
    for _ in 0..4 {
        rack.tick();
        audio.process(BLOCK, &[]);
    }
    let saved = saved_now(&mut rack, &mut audio);
    assert!(saved.contains(&"$status 0".to_owned()), "{saved:?}");
    assert!(!at.exists(), "nothing written next to the path");
}
/// KSP manual (`load_array`): an array loaded in `on init` is implicitly
/// persistent, so it saves; a text array saves each value on a line ended
/// by a newline, as Kontakt writes it.
#[test]
fn an_array_loaded_in_init_saves_one_ended_line_per_value() {
    use kontra_engine::ir;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::write(dir.join("init_names.nka"), "!names\nfirst\nsecond\n").unwrap();
    let source = format!(
        "on init
            declare !names[2]
            declare $n
            $n := load_array_str(!names, \"{}/init_names.nka\")
        end on",
        dir.display()
    );
    let instrument = ir::Instrument {
        scripts: vec![ir::Script {
            name: "s".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source,
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
        }],
        ..ir::Instrument::DEFAULT
    };
    let (mut rack, mut audio) = rack();
    rack.load_ir(0, instrument, None).unwrap();
    audio.process(BLOCK, &[]);
    let saved = saved_now(&mut rack, &mut audio);
    assert!(
        saved.contains(&"!names first\nsecond\n".to_owned()),
        "{saved:?}"
    );
}

/// A callback's `set_zone_par` calls reach the control side as one batch,
/// are applied together to the part's plan off the audio thread, and swap
/// in: the held note's zone left its group and fades, the next note-on plays
/// the zone through its new group (gain 2) at its new key.
#[test]
fn a_callbacks_zone_edits_apply_together_off_audio_and_swap_in() {
    let (mut rack, mut audio) = rack();
    let store = rack.store_mut();
    let mut zones = plan_of(store, 0.5).zones;
    let mut other = zones[0].clone();
    (other.id, other.group, other.keys) = (1, 1, 100..=100);
    zones[0].keys = 60..=60;
    zones.push(other);
    let mut plan = Plan::new(zones, 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    plan.groups[1].level.gain = 2.0;
    let script = kontra_ksp::compile(
        "on note
            if ($EVENT_NOTE = 60)
                set_zone_par(0, $ZONE_PAR_GROUP, 1)
                set_zone_par(0, $ZONE_PAR_LOW_KEY, 72)
                set_zone_par(0, $ZONE_PAR_HIGH_KEY, 72)
            end if
        end on",
        &[],
    )
    .unwrap();
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let plan = Arc::new(plan);
    let engine = Box::new(Engine::new(Arc::clone(&plan), store.client(), RATE));
    rack.install(
        0,
        engine,
        Part {
            plan: Some(plan),
            ..part("z")
        },
    );
    let [held, _] = play(&mut audio, &[(0, note(0))]);
    assert!((held - 0.5).abs() < 1e-3, "{held}");
    rack.tick();
    let p = rack.parts()[0].as_ref().unwrap();
    let zone = &p.plan.as_ref().unwrap().zones[0];
    assert_eq!((zone.group, zone.keys.clone()), (1, 72..=72));
    let [gone, _] = play(&mut audio, &[]);
    assert!(gone.abs() < 1e-4, "{gone}");
    let at = |key| match note(0) {
        Event::NoteOn {
            note,
            velocity,
            tune,
        } => Event::NoteOn {
            note: HostNote { key, ..note },
            velocity,
            tune,
        },
        e => e,
    };
    let [moved, _] = play(&mut audio, &[(0, at(72))]);
    assert!((moved - 1.0).abs() < 1e-3, "{moved}");
}

#[test]
fn a_note_played_before_its_heads_load_sounds_from_the_stream_and_survives_their_arrival() {
    let (mut rack, mut audio) = rack();
    rack.store_mut().defer_heads(true);
    let a = engine(rack.store_mut(), 0.5);
    let heads = rack
        .store_mut()
        .take_heads()
        .expect("the heads are deferred");
    rack.install(0, a, part("a"));
    // The note starts with no head in: it is silent until its pages stream in,
    // never dropped.
    audio.process(BLOCK, &[(0, note(0))]);
    let mut sounded = false;
    for _ in 0..400 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        audio.process(BLOCK, &[]);
        if level(&audio, 0) > 0.1 {
            sounded = true;
            break;
        }
    }
    assert!(sounded, "the note never sounded before its heads arrived");
    heads.load(2);
    // The same voice goes on at the same level once the heads are in.
    let [on, _] = play(&mut audio, &[]);
    assert!(
        (on - 0.5).abs() < 0.05,
        "the note changed when its heads arrived: {on}"
    );
}

/// A part whose one zone plays a ramp sample (frame `n` is `0.1 + n / 100_000`).
fn ramp_engine(store: &mut Store) -> Box<Engine> {
    let fmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 1,
    };
    let frames = 96_000u64;
    let bytes = (0..frames)
        .flat_map(|n| (0.1 + n as f32 / 100_000.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "ramp".into(),
        offset: 0,
        fmt,
    };
    let sample = (store.load(
        &key,
        Arc::new(MemSource::new(fmt, bytes)),
        frames,
        std::slice::from_ref(&(0..frames)),
    ))
    .unwrap();
    let region = Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut zone = Zone::new(region, 60, RATE);
    zone.keys = 0..=127;
    let mut plan = Plan::new(vec![zone], 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    Box::new(Engine::new(Arc::new(plan), store.client(), RATE))
}

/// The left output of a ramp part played from a note-on, block by block.
fn ramp_output(defer: bool) -> Vec<f32> {
    let (mut rack, mut audio) = rack();
    rack.store_mut().defer_heads(defer);
    let a = ramp_engine(rack.store_mut());
    // Left unread: the page the note needs has to come from the stream.
    let _heads = rack.store_mut().take_heads();
    rack.install(0, a, part("a"));
    audio.process(BLOCK, &[(0, note(0))]);
    let mut out: Vec<f32> = audio.output(0, BLOCK).iter().map(|s| s[0]).collect();
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        audio.process(BLOCK, &[]);
        out.extend(audio.output(0, BLOCK).iter().map(|s| s[0]));
    }
    out
}

#[test]
fn a_note_before_its_heads_load_starts_from_the_samples_first_frame() {
    let loaded = ramp_output(false);
    let waited = ramp_output(true);
    let first = |v: &[f32]| v.iter().position(|x| x.abs() > 1e-4).unwrap();
    let (a, b) = (first(&loaded), first(&waited));
    assert!(
        waited[b] < 0.12,
        "the note joined mid-sample at {}",
        waited[b]
    );
    for (x, y) in loaded[a..a + 300].iter().zip(&waited[b..b + 300]) {
        assert!((x - y).abs() < 1e-3, "the attack differs: {x} vs {y}");
    }
}
