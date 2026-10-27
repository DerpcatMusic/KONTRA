//! Sampler behaviour as engine data: round robin, crossfades, triggers, groups, buses.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{
    Bus, Control, Crossfade, Engine, Plan, RoundRobin, RrCounter, Timed, Trigger, Zone,
};
use kontra_events::{Event, HostNote, HostPattern};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::{Ahdsr, Region};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// A constant-level sample: its level identifies which zone sounds.
fn region(store: &mut Store, level: f32, frames: u64) -> std::io::Result<Region> {
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
        sample,
        start: 0,
        end: frames,
        looped: None,
        reverse: false,
    })
}

/// A zone whose output level is `level` at full velocity, centre pan, no velocity scaling.
fn zone(store: &mut Store, level: f32) -> Zone {
    let mut z = Zone::new(region(store, level, 48_000).unwrap(), 60, RATE);
    z.velocity = kontra_engine::VelocityCurve::Flat;
    z.envelope = Ahdsr {
        release: 64,
        ..Ahdsr::GATE
    };
    z
}

fn engine(store: &Store, plan: Plan) -> Engine {
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

fn on(key: u8, velocity: f64) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: host(key),
            velocity,
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

fn cc(controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0, controller, value),
    }
}

/// Plays `events` at the start of a 256-frame block and returns the level at frame 128.
fn level(engine: &mut Engine, events: &[Timed<Event>]) -> [f32; 2] {
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

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn sequential_round_robin_cycles_and_mic_layers_agree() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // Two RR steps x two mics: close (bus 0) and room (bus 1). Levels name the step.
    let mut zones = Vec::new();
    for (position, level) in [(0u8, 0.1f32), (1, 0.2)] {
        for bus in 0..2 {
            let mut z = zone(&mut store, level + bus as f32 * 0.01);
            z.round_robin = Some(RoundRobin {
                counter: RrCounter::Key,
                position,
                length: 2,
                random: false,
            });
            z.bus = bus;
            z.pan = if bus == 0 { -1.0 } else { 1.0 };
            zones.push(z);
        }
    }
    let mut plan = Plan::new(zones, 16);
    plan.buses.push(Bus {
        name: "Room".into(),
        gain: 1.0,
        enabled: true,
    });
    let mut e = engine(&store, plan);
    let mut steps = Vec::new();
    for _ in 0..4 {
        let [l, r] = level(&mut e, &[on(60, 1.0)]);
        assert!(
            close(r - l, 0.01),
            "close and room play the same step: {l} {r}"
        );
        steps.push(l);
        level(&mut e, &[off(60)]);
    }
    assert!(
        close(steps[0], 0.1)
            && close(steps[1], 0.2)
            && close(steps[2], 0.1)
            && close(steps[3], 0.2),
        "{steps:?}"
    );
}

#[test]
fn random_round_robin_never_repeats_a_step() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = (0..3u8)
        .map(|position| {
            let mut z = zone(&mut store, 0.1 * f32::from(position + 1));
            z.round_robin = Some(RoundRobin {
                counter: RrCounter::Global,
                position,
                length: 3,
                random: true,
            });
            z
        })
        .collect();
    let mut e = engine(&store, Plan::new(zones, 16));
    let mut last = 0.0;
    for _ in 0..50 {
        let [l, _] = level(&mut e, &[on(60, 1.0)]);
        assert!(l > 0.05, "exactly one step plays");
        assert!(!close(l, last), "repeat {l}");
        last = l;
        level(&mut e, &[off(60)]);
        for _ in 0..4 {
            level(&mut e, &[]);
        }
    }
}

#[test]
fn a_keyswitch_selects_the_articulation_and_sounds_nothing() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut legato = zone(&mut store, 0.3);
    legato.keyswitch = Some(24);
    let mut staccato = zone(&mut store, 0.6);
    staccato.keyswitch = Some(25);
    let mut plan = Plan::new(vec![legato, staccato], 8);
    plan.keyswitches = Some(24..=35);
    plan.default_keyswitch = Some(24);
    let mut e = engine(&store, plan);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.3));
    level(&mut e, &[off(60)]);
    for _ in 0..4 {
        level(&mut e, &[]);
    }
    assert_eq!(
        level(&mut e, &[on(25, 1.0)])[0],
        0.0,
        "the switch key is silent"
    );
    assert_eq!(e.keyswitch(), Some(25));
    assert!(close(level(&mut e, &[on(62, 1.0)])[0], 0.6));
}

#[test]
fn a_release_trigger_plays_on_key_up_quieter_the_longer_the_key_was_held() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut attack = zone(&mut store, 0.5);
    attack.envelope = Ahdsr {
        release: 16,
        ..Ahdsr::GATE
    };
    let mut tail = zone(&mut store, 0.4);
    tail.trigger = Trigger::Release;
    tail.release_decay_db = 6.0; // per second held
    let mut e = engine(&store, Plan::new(vec![attack, tail], 8));
    level(&mut e, &[on(60, 1.0)]);
    for _ in 0..(RATE as usize / 256) {
        level(&mut e, &[]);
    }
    let [l, _] = level(&mut e, &[off(60)]);
    // ~1 s held: 0.4 at −6 dB ≈ 0.2 (the attack has faded by frame 128).
    assert!((l - 0.2).abs() < 0.02, "{l}");
}

#[test]
fn a_choke_group_fades_the_open_hat() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut open = zone(&mut store, 0.5);
    open.keys = 46..=46;
    open.group = 1;
    open.off_by = Some(2);
    let mut closed = zone(&mut store, 0.25);
    closed.keys = 42..=42;
    closed.group = 2;
    let mut e = engine(&store, Plan::new(vec![open, closed], 8));
    assert!(close(level(&mut e, &[on(46, 1.0)])[0], 0.5));
    level(&mut e, &[on(42, 1.0)]);
    assert!(
        close(level(&mut e, &[])[0], 0.25),
        "the open hat faded out (5 ms)"
    );
}

#[test]
fn group_polyphony_keeps_only_the_newest_voice() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut mono = zone(&mut store, 0.5);
    mono.group = 7;
    let mut plan = Plan::new(vec![mono], 8);
    plan.polyphony = vec![(7, 1)];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[on(64, 1.0)]);
    assert!(close(level(&mut e, &[])[0], 0.5), "one voice, not two");
    assert_eq!(e.playing_voices(), 1);
}

#[test]
fn velocity_layers_crossfade_with_equal_power() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut soft = zone(&mut store, 1.0);
    soft.pan = -1.0;
    soft.crossfades = vec![Crossfade {
        control: Control::Velocity,
        fade_in: None,
        fade_out: Some((40, 80)),
    }];
    let mut loud = zone(&mut store, 1.0);
    loud.pan = 1.0;
    loud.crossfades = vec![Crossfade {
        control: Control::Velocity,
        fade_in: Some((40, 80)),
        fade_out: None,
    }];
    let mut e = engine(&store, Plan::new(vec![soft, loud], 8));
    let [l, r] = level(&mut e, &[on(60, 60.0 / 127.0)]);
    assert!(close(l * l + r * r, 1.0), "{l} {r}");
    assert!(close(l, r), "midpoint: equal parts");
}

#[test]
fn a_cc_crossfade_moves_sounding_voices() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 1.0);
    z.crossfades = vec![Crossfade {
        control: Control::Cc(1),
        fade_in: Some((0, 127)),
        fade_out: None,
    }];
    let mut looped = z.region.clone();
    looped.looped = Some(kontra_voice::Loop {
        start: 0,
        end: 48_000,
    });
    z.region = looped;
    let mut e = engine(&store, Plan::new(vec![z], 8));
    let quiet = level(&mut e, &[cc(1, 10), on(60, 1.0)])[0];
    for _ in 0..3 {
        level(&mut e, &[cc(1, 127)]);
    }
    let loud = level(&mut e, &[])[0];
    assert!(quiet < 0.2 && close(loud, 1.0), "{quiet} -> {loud}");
}

#[test]
fn a_disabled_mic_bus_starts_no_voices() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let close_mic = zone(&mut store, 0.5);
    let mut room = zone(&mut store, 0.25);
    room.bus = 1;
    let mut plan = Plan::new(vec![close_mic, room], 8);
    plan.buses.push(Bus {
        name: "Room".into(),
        gain: 1.0,
        enabled: true,
    });
    let mut e = engine(&store, plan);
    e.set_bus_enabled(1, false);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.5));
    assert_eq!(e.active_voices(), 1);
}
