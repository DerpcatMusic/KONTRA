//! Sampler behaviour as engine data: round robin, crossfades, triggers, groups, buses.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::ir::Join;
use kontra_engine::{
    Atom, Bus, Control, Crossfade, Engine, Plan, RoundRobin, RrCounter, Timed, Trigger, Zone,
};
use kontra_events::{Event, HostNote, HostPattern};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::Region;

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
        loops: vec![],
        reverse: false,
    })
}

/// A zone whose output level is `level` at full velocity, centre pan, no velocity scaling.
fn zone(store: &mut Store, level: f32) -> Zone {
    Zone::new(region(store, level, 48_000).unwrap(), 60, RATE)
}

/// A second of a 1 kHz sine at 0.1.
fn sine(store: &mut Store) -> Region {
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
        sample,
        start: 0,
        end: frames,
        loops: vec![],
        reverse: false,
    }
}

/// The plan's groups ignore velocity.
fn engine(store: &Store, mut plan: Plan) -> Engine {
    for g in &mut plan.groups {
        g.mods.clear();
    }
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
            z.start = vec![(
                Join::And,
                Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Key,
                    position,
                    length: 2,
                    random: false,
                }),
            )];
            z.bus = bus;
            z.pan = if bus == 0 { -1.0 } else { 1.0 };
            zones.push(z);
        }
    }
    let mut plan = Plan::new(zones, 16);
    plan.buses.push(Bus {
        name: "Room".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: Vec::new(),
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

/// Kontakt steps its round robin on every note-on, before group predicates
/// (KONTAKT_START_CONDITIONS, 0x144f17ea0): a note that plays no round-robin
/// zone still moves the cycle on.
#[test]
fn the_global_round_robin_steps_on_every_note() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut zones = Vec::new();
    for (position, level) in [(0u8, 0.1f32), (1, 0.2)] {
        let mut z = zone(&mut store, level);
        z.keys = 36..=36;
        z.start = vec![(
            Join::And,
            Atom::RoundRobin(RoundRobin {
                counter: RrCounter::Global,
                position,
                length: 2,
                random: false,
            }),
        )];
        zones.push(z);
    }
    let mut plain = zone(&mut store, 0.5);
    plain.keys = 60..=60;
    zones.push(plain);
    let mut e = engine(&store, Plan::new(zones, 16));
    let mut play = |key| {
        let l = level(&mut e, &[on(key, 1.0)])[0];
        level(&mut e, &[off(key)]);
        l
    };
    let first = play(36);
    play(60);
    assert!(close(play(36), first), "two note-ons later, the same step");
}

#[test]
fn random_round_robin_never_repeats_a_step() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = (0..3u8)
        .map(|position| {
            let mut z = zone(&mut store, 0.1 * f32::from(position + 1));
            z.start = vec![(
                Join::And,
                Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Global,
                    position,
                    length: 3,
                    random: true,
                }),
            )];
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

/// Kontakt's key latch: switch keys select the articulation, a key in a gap
/// between switches still plays ordinary zones and clears the latch.
#[test]
fn a_keyswitch_latches_and_a_gap_key_clears_it() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut legato = zone(&mut store, 0.3);
    legato.keys = 40..=127;
    legato.start = vec![(Join::And, Atom::Switch(0))];
    let mut staccato = zone(&mut store, 0.6);
    staccato.keys = 40..=127;
    staccato.start = vec![(Join::And, Atom::Switch(1))];
    let mut gap = zone(&mut store, 0.1);
    gap.keys = 30..=30;
    let mut plan = Plan::new(vec![legato, staccato, gap], 8);
    plan.switches = vec![24..=24, 36..=36];
    plan.default_keyswitch = Some(24);
    let mut e = engine(&store, plan);
    let mut play = |key| {
        let l = level(&mut e, &[on(key, 1.0)])[0];
        level(&mut e, &[off(key)]);
        for _ in 0..4 {
            level(&mut e, &[]);
        }
        l
    };
    assert!(close(play(60), 0.3), "the default articulation");
    play(36);
    assert!(close(play(62), 0.6), "switched");
    assert!(close(play(30), 0.1), "a gap key sounds");
    assert_eq!(play(64), 0.0, "and cleared the latch");
}

#[test]
fn a_release_trigger_plays_on_key_up_quieter_the_longer_the_key_was_held() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let attack = zone(&mut store, 0.5);
    let mut tail = zone(&mut store, 0.4);
    tail.group = 1;
    tail.trigger = Trigger::Release;
    tail.release_decay_db = 6.0; // per second held
    let mut plan = Plan::new(vec![attack, tail], 8);
    plan.groups[0].envelope.release = 16;
    let mut e = engine(&store, plan);
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
fn velocity_layers_crossfade_linearly() {
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
    assert!(
        close(l + r, 1.0),
        "Kontakt's linear amplitude ramp: {l} {r}"
    );
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
    looped.loops = vec![kontra_voice::Loop {
        start: 0,
        end: 48_000,
        crossfade: 0,
        ..Default::default()
    }];
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
        pan: 0.0,
        inserts: Vec::new(),
        enabled: true,
    });
    let mut e = engine(&store, plan);
    e.set_bus_enabled(1, false);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.5));
    assert_eq!(e.active_voices(), 1);
}

#[test]
fn a_voice_group_steals_by_its_rule_and_an_exclusive_set_silences_the_others() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut keyed = |key: u8, level: f32, vg: u32| {
        let mut z = zone(&mut store, level);
        z.keys = key..=key;
        z.voice_group = Some(vg);
        z
    };
    let zones = vec![
        keyed(60, 0.1, 0),
        keyed(64, 0.2, 0),
        keyed(62, 0.4, 0),
        keyed(70, 0.8, 1),
    ];
    let mut plan = Plan::new(zones, 8);
    let group = |voices, steal| kontra_ir::VoiceGroup {
        voices,
        steal,
        prefer_released: true,
        fade_ms: 1.0,
        exclusive: Some(3),
    };
    plan.voice_groups = vec![
        group(2, kontra_ir::Steal::Highest),
        group(4, kontra_ir::Steal::Oldest),
    ];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0), on(64, 1.0)]);
    level(&mut e, &[on(62, 1.0)]);
    assert!(
        close(level(&mut e, &[])[0], 0.5),
        "the highest key (64) gave way"
    );
    level(&mut e, &[on(70, 1.0)]);
    assert!(close(level(&mut e, &[])[0], 0.8), "group 0 was excluded");
}

#[test]
fn a_one_shot_ignores_key_up() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut hit = zone(&mut store, 0.5);
    hit.one_shot = true;
    let mut e = engine(&store, Plan::new(vec![hit], 8));
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[off(60)]);
    assert!(
        close(level(&mut e, &[])[0], 0.5),
        "still sounding at full level"
    );
}

#[test]
fn pitch_bend_moves_sounding_voices_by_its_route() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].mods = vec![kontra_engine::ir::Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: kontra_engine::ir::ModKind::Pitch {
            source: kontra_engine::ir::Source::PitchBend,
            semitones: 2.0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let bend = |lsb, msb| Timed {
        at: SampleTime(0),
        body: Event::midi1(0xE0, lsb, msb),
    };
    // Source frames one 256-frame block advances.
    let advance = |e: &mut Engine, events: &[Timed<Event>]| {
        let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
        let from = at(e);
        level(e, events);
        (at(e) - from) / 256.0
    };
    level(&mut e, &[on(60, 1.0)]);
    assert!((advance(&mut e, &[]) - 1.0).abs() < 1e-6);
    let up = advance(&mut e, &[bend(0x7F, 0x7F)]);
    assert!(
        (up - 2f64.powf(2.0 * 8191.0 / 8192.0 / 12.0)).abs() < 1e-6,
        "{up}"
    );
    let down = advance(&mut e, &[bend(0, 0)]);
    assert!((down - 2f64.powf(-2.0 / 12.0)).abs() < 1e-6, "{down}");
}

#[test]
fn a_sample_start_route_moves_the_start_within_its_range() {
    use kontra_engine::ir::{Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // A ramp: each frame's value is its index / 4096, so the output names the frame.
    let frames = 4096u64;
    let bytes = (0..frames)
        .flat_map(|i| (i as f32 / 4096.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "ramp".into(),
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
    let mut z = Zone::new(
        Region {
            sample,
            start: 0,
            end: frames,
            loops: vec![],
            reverse: false,
        },
        60,
        RATE,
    );
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    for (depth, start) in [(0.5, 500.0), (3.0, 1000.0)] {
        plan.groups[0].mods = vec![Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind: ModKind::SampleStart {
                source: Source::Constant,
                depth,
            },
        }];
        let mut e = Engine::new(Arc::new(plan.clone()), store.client(), RATE);
        let frame = level(&mut e, &[on(60, 1.0)])[0] * 4096.0;
        assert!(
            (frame - (start + 128.0)).abs() < 1.0,
            "depth {depth}: {frame}"
        );
    }
}

#[test]
fn a_group_eq_shapes_its_voices() {
    use kontra_engine::ir::{Bell, Effect, EffectKind};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let region = sine(&mut store);
    let peak = |gain_db: f32| {
        let mut plan = Plan::new(vec![Zone::new(region.clone(), 60, RATE)], 8);
        plan.groups[0].mods.clear();
        let bell = Bell {
            hz: 1000.0,
            q: 1.0,
            gain_db,
            ..Bell::FLAT
        };
        plan.groups[0].inserts = vec![Some(Effect {
            kind: EffectKind::Eq([bell, Bell::FLAT, Bell::FLAT, Bell::FLAT]),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut out = vec![[0.0; 2]; 4800];
        e.process(&mut out, &[on(60, 1.0)]);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (flat, boosted) = (peak(0.0), peak(12.0));
    assert!((flat - 0.1).abs() < 0.005, "{flat}");
    assert!(
        (boosted / flat - 10f32.powf(0.6)).abs() < 0.05,
        "{flat} -> {boosted}"
    );
}

#[test]
fn a_controller_pitch_route_moves_sounding_voices() {
    use kontra_engine::ir::{Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Pitch {
            source: Source::Controller(1),
            semitones: 12.0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
    level(&mut e, &[on(60, 1.0)]);
    let from = at(&e);
    level(&mut e, &[cc(1, 127)]);
    assert!(
        (at(&e) - from - 512.0).abs() < 1e-6,
        "an octave up: {}",
        at(&e) - from
    );
}

#[test]
fn an_envelope_pitch_route_rises_holds_and_releases() {
    use kontra_engine::ir::{Breakpoint, Breakpoints, Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    let point = |level| Breakpoint {
        ms: 1.0,
        level,
        curve: 0.0,
    };
    plan.groups[0].envelopes = vec![Breakpoints {
        start: 0.0,
        points: vec![point(1.0), point(0.0)],
        sustain: Some(0),
        loop_from: None,
    }];
    plan.groups[0].envelope.release = RATE;
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Pitch {
            source: Source::Envelope(0),
            semitones: 12.0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let advance = |e: &mut Engine, events: &[Timed<Event>]| {
        let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
        let from = at(e);
        level(e, events);
        (at(e) - from) / 256.0
    };
    level(&mut e, &[on(60, 1.0)]);
    assert!((advance(&mut e, &[]) - 2.0).abs() < 1e-6, "held at the top");
    level(&mut e, &[off(60)]);
    let after = advance(&mut e, &[]);
    assert!(
        (after - 1.0).abs() < 1e-6,
        "released to the bottom: {after}"
    );
}

#[test]
fn a_controller_opens_a_voice_filter() {
    use kontra_engine::ir::{Effect, EffectKind, Filter, Mod, ModKind, Pass, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // A 4-pole low pass at 100 Hz that a controller opens by 6 octaves.
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Low,
            hz: 100.0,
            stages: 2,
            pole: false,
            damping: [1.85, 0.77, 0.0],
            gain: 1.0,
            octaves: 6.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Cutoff {
            source: Source::Controller(74),
            insert: 0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let peak = |e: &mut Engine, events: &[Timed<Event>]| {
        let now = e.now();
        let input: Vec<_> = events
            .iter()
            .map(|ev| Timed {
                at: now,
                body: ev.body,
            })
            .collect();
        let mut out = vec![[0.0f32; 2]; 4800];
        e.process(&mut out, &input);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    peak(&mut e, &[on(60, 1.0)]);
    let closed = peak(&mut e, &[]);
    peak(&mut e, &[cc(74, 127)]);
    let open = peak(&mut e, &[]);
    assert!(closed < 0.001, "closed {closed}");
    assert!((open - 0.1).abs() < 0.01, "open {open}");
}

/// A group with one EQ insert, `eq`, and the given routes.
fn eq_group(
    region: Region,
    eq: [kontra_engine::ir::Bell; 4],
    mods: Vec<kontra_engine::ir::Mod>,
) -> Plan {
    use kontra_engine::ir::{Effect, EffectKind};
    let mut plan = Plan::new(vec![Zone::new(region, 60, RATE)], 8);
    plan.groups[0].mods = mods;
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Eq(eq),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan
}

#[test]
fn a_velocity_moved_eq_band_makes_a_hard_note_louder_at_its_frequency() {
    use kontra_engine::ir::{Bell, Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let region = sine(&mut store);
    // The 1 kHz band moves +24 dB across velocity: 12 dB at 0.5, 24 dB at 1.
    let route = Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::EqGain {
            source: Source::Velocity,
            insert: 0,
            band: 0,
            depth_db: 24.0,
            limit_db: 36.0,
            span_db: 72.0,
        },
    };
    let bell = Bell {
        hz: 1000.0,
        q: 1.0,
        gain_db: 0.0,
        ..Bell::FLAT
    };
    let peak = |velocity: f64| {
        let mut e = Engine::new(
            Arc::new(eq_group(
                region.clone(),
                [bell, Bell::FLAT, Bell::FLAT, Bell::FLAT],
                vec![route.clone()],
            )),
            store.client(),
            RATE,
        );
        let mut out = vec![[0.0; 2]; 4800];
        e.process(&mut out, &[on(60, velocity)]);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (soft, hard) = (peak(0.5), peak(1.0));
    let ratio = hard / soft;
    assert!(
        (ratio / 10f32.powf(12.0 / 20.0) - 1.0).abs() < 0.05,
        "soft {soft} hard {hard} ratio {ratio}"
    );
}

#[test]
fn an_lfo_moved_resonance_changes_the_response_over_time() {
    use kontra_engine::ir::{
        Clock, Effect, EffectKind, Filter, Lfo, Mod, ModKind, Pass, Rate, Source, Wave,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // A 1 kHz band pass with unit gain at its peak, its Q moved by a square
    // LFO at 1 Hz: its damping falls 4× (Q 4×) for one half, rises 4× for the other.
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Band,
            hz: 1000.0,
            stages: 1,
            pole: false,
            damping: [1.0; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 2.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Square { width: 0.5 },
        rate: Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Free,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Resonance {
            source: Source::Lfo(0),
            insert: 0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 48_000];
    e.process(&mut out, &[on(60, 1.0)]);
    let window = |from: f32, to: f32| {
        let (a, b) = ((from * 48_000.0) as usize, (to * 48_000.0) as usize);
        out[a..b].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (first, second) = (window(0.1, 0.4), window(0.6, 0.9));
    let swing = (first / second).max(second / first);
    assert!(swing > 4.0, "first {first} second {second}");
}

#[test]
fn a_group_formant_shapes_its_voices() {
    use kontra_engine::ir::{Effect, EffectKind, Formant};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Formant(Formant {
            talk: 0.5,
            sharp: 0.5,
            size: 0.5,
            set: kontra_engine::ir::VowelSet::First,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 9600];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out[4800..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    // Kontakt's Formant I at these knobs: -2.15 dB at 1 kHz.
    let db = 20.0 * (peak / 0.1).log10();
    assert!((db - -2.15).abs() < 0.3, "{db} dB");
}

#[test]
fn a_velocity_attack_route_shortens_soft_attacks() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].envelope.attack = 4800; // 100 ms, linear
    plan.groups[0].mods = vec![ir::Mod {
        name: "VEL_ATTACK".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Attack {
            source: ir::Source::Velocity,
            intensity: 1.0,
            floor_ms: 2.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let [hard, _] = level(&mut e, &[on(60, 1.0)]);
    assert!((hard - 0.5 * 128.0 / 4800.0).abs() < 1e-3, "{hard}");
    level(&mut e, &[off(60)]);
    for _ in 0..40 {
        level(&mut e, &[]);
    }
    // Half velocity halves the knob: 2·(102/2)^0.5 − 2 ≈ 12.3 ms.
    let [soft, _] = level(&mut e, &[on(60, 0.5)]);
    let frames = (2.0 * 51f32.sqrt() - 2.0) * 48.0;
    assert!((soft - 0.5 * 128.0 / frames).abs() < 2e-3, "{soft}");
}

#[test]
fn timed_modulation_does_not_depend_on_the_block_size() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Sine,
        rate: Rate::Hz(7.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Voice,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Volume {
            source: Source::Lfo(0),
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
        },
    }];
    let plan = Arc::new(plan);
    let render = |block: usize| {
        let mut e = Engine::new(Arc::clone(&plan), store.client(), RATE);
        let mut out = Vec::new();
        let mut buf = vec![[0.0; 2]; block];
        e.process(&mut buf, &[on(60, 1.0)]);
        out.extend_from_slice(&buf);
        while out.len() < 9_600 {
            e.process(&mut buf, &[]);
            out.extend_from_slice(&buf);
        }
        out.truncate(9_600);
        out
    };
    let reference = render(256);
    for block in [64, 100, 1000] {
        assert_eq!(render(block), reference, "block {block}");
    }
}

#[test]
fn an_instrument_convolution_adds_its_wet_path() {
    use kontra_engine::{Response, ir};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    // A delayed half-level tap: on a constant input the wet path is 0.25.
    plan.impulses = vec![Some(Response {
        left: vec![0.0, 0.0, 0.5],
        right: vec![0.0, 0.0, 0.5],
    })];
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Convolution { impulse: 0 },
        bypass: false,
        output: 1.0,
        dry: 0.5,
    })];
    let mut e = engine(&store, plan);
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    // Dry 0.5 × 0.5 plus wet 0.25.
    assert!(close(l, 0.5) && close(r, 0.5), "{l} {r}");
}

#[test]
fn an_instrument_reverb_rings_on_after_the_note() {
    use kontra_engine::ir;
    let reverb = ir::Effect {
        kind: ir::EffectKind::Reverb(ir::Reverb {
            seconds: 2.0,
            size: 1.0,
            damping: 0.5,
            diffusion: 1.0,
            predelay_ms: 0.0,
            high_cut_hz: 20_000.0,
            low_shelf_db: 0.0,
            stereo: 1.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    // The summed level of the last block, a second after key up.
    let tail = |inserts: Vec<Option<ir::Effect>>| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
        plan.inserts = inserts;
        let mut e = engine(&store, plan);
        let first = level(&mut e, &[on(60, 1.0)]);
        let mut buf = vec![[0.0; 2]; 256];
        for _ in 0..50 {
            e.process(&mut buf, &[]);
        }
        level(&mut e, &[off(60)]);
        for _ in 0..190 {
            e.process(&mut buf, &[]);
        }
        let sum: f32 = buf.iter().map(|[l, r]| l.abs() + r.abs()).sum();
        (first, sum)
    };
    let (dry_first, dry_tail) = tail(Vec::new());
    assert!(dry_first[0] > 0.1 && dry_tail == 0.0, "the voice has ended");
    let (first, wet) = tail(vec![Some(reverb)]);
    // Fully wet: nothing until the shortest line (30 ms) comes round.
    assert_eq!(first, [0.0; 2]);
    assert!(wet > 1e-3 && wet.is_finite(), "tail {wet}");
}

#[test]
fn a_compressor_slot_mixes_its_dry_level_from_the_input() {
    use kontra_engine::ir;
    let render = |output: f32| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
        // Squashes hard; the slot adds its dry signal to `output` of it.
        plan.inserts = vec![Some(ir::Effect {
            kind: ir::EffectKind::Compressor(ir::Compressor {
                threshold_db: -40.0,
                ratio: 20.0,
                knee_db: 0.0,
                attack_ms: 0.0,
                release_ms: 100.0,
                makeup_db: 0.0,
                link: true,
                mix: 1.0,
            }),
            bypass: false,
            output,
            dry: 1.0,
        })];
        let mut e = engine(&store, plan);
        level(&mut e, &[on(60, 1.0)])
    };
    let [l, r] = render(0.0);
    assert!(close(l, 0.5) && close(r, 0.5), "{l} {r}");
    // The compressed path adds a little on top of the dry 0.5.
    let [l, _] = render(1.0);
    assert!(l > 0.5 && l < 0.6, "{l}");
}

#[test]
fn a_group_waveshaper_clips_each_voice_not_their_sum() {
    use kontra_engine::ir::{DriveCurve, Effect, EffectKind, Shaper};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![
        Zone::new(sine(&mut store), 60, RATE),
        Zone::new(sine(&mut store), 60, RATE),
    ];
    let mut plan = Plan::new(zones, 8);
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Shaper(Shaper {
            curve: DriveCurve::Soft(1.0),
            lowpass_hz: None,
            highpass_hz: None,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 9600];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out[4800..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    // Each 0.1 sine clips to 0.64 (4x, then 2|x| − x²): 1.28 together. The
    // sum through one curve would peak at 0.96.
    assert!((peak - 1.28).abs() < 0.02, "{peak}");
}

#[test]
fn a_group_lofis_noise_fades_with_the_voice() {
    use kontra_engine::ir::{Crush, Effect, EffectKind};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    plan.groups[0].envelope.release = 2400;
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Crush(Crush {
            rate: 1.0,
            bits: 16.0,
            noise: 0.075,
            noise_tone: 1.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 4800];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out.iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    assert!(peak > 0.1, "{peak}");
    out.fill([0.0; 2]);
    e.process(&mut out, &[off(60)]);
    // The noise sits before the envelope, so it fades with the release
    // (after it, at 0.075, it would outlast the faded sine).
    let end = out[2300..2400]
        .iter()
        .map(|s| s[0].abs())
        .fold(0.0, f32::max);
    assert!(end < 0.02, "{end}");
}
