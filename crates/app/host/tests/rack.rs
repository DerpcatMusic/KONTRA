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
    Box::new(Engine::new(Arc::new(plan), store.client(), RATE))
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
    assert!(after.abs() < 1e-4, "panic left sound: {after}");
    let m = rack.meters().unwrap();
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
