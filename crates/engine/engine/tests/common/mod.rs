//! Helpers the engine's behaviour tests share.
#![cfg(test)]
// Each test file uses some of these.
#![allow(dead_code)]

use std::sync::Arc;

use kontra_engine::{Engine, Plan, Timed, Zone};
use kontra_events::{Event, HostNote, HostPattern};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store};
use kontra_time::SampleTime;
use kontra_voice::Region;

pub const RATE: u32 = 48_000;
pub const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// A constant-level sample: its level identifies which zone sounds.
pub fn region(store: &mut Store, level: f32, frames: u64) -> std::io::Result<Region> {
    let bytes = (0..frames).flat_map(|_| level.to_le_bytes()).collect();
    let key = BlobKey {
        file: format!("dc{level}").into(),
        offset: frames,
        fmt: FMT,
    };
    let sample = store.load(
        &key,
        Arc::new(MemSource::new(FMT, bytes)),
        frames,
        std::slice::from_ref(&(0..frames)),
    )?;
    Ok(Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    })
}

/// A zone whose output level is `level` at full velocity, centre pan, no velocity scaling.
pub fn zone(store: &mut Store, level: f32) -> Zone {
    Zone::new(region(store, level, 48_000).unwrap(), 60, RATE)
}

/// A second of a 1 kHz sine at 0.1.
pub fn sine(store: &mut Store) -> Region {
    let frames = 48_000u64;
    let bytes = (0..frames)
        .flat_map(|i| {
            let v = (std::f64::consts::TAU * 1000.0 * i as f64 / 48_000.0).sin() as f32 * 0.1;
            v.to_le_bytes()
        })
        .collect();
    let key = BlobKey {
        file: "sine".into(),
        offset: frames,
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
    Region {
        mix: None,
        table: None,
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    }
}

/// The plan's groups ignore velocity.
pub fn engine(store: &Store, mut plan: Plan) -> Engine {
    for g in &mut plan.groups {
        g.mods.clear();
    }
    Engine::new(Arc::new(plan), store.client(), RATE)
}

pub fn host(key: u8) -> HostNote {
    HostNote {
        port: 0,
        channel: 0,
        key,
        id: -1,
        clap: true,
    }
}

pub fn on(key: u8, velocity: f64) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host(key),
            velocity,
            tune: 0.0,
        },
    }
}

pub fn off(key: u8) -> Timed<Event> {
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

pub fn cc(controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, controller, value),
    }
}

/// Plays `events` at the start of a 256-frame block and returns the level at frame 128.
pub fn level(engine: &mut Engine, events: &[Timed<Event>]) -> [f32; 2] {
    let now = engine.now();
    let input: Vec<_> = events
        .iter()
        .map(|e| Timed {
            at: now,
            body: e.body,
        })
        .collect();
    let mut out = vec![[0.0; 2]; 256];
    engine.process(&mut out, &input);
    out[128]
}

pub fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}
