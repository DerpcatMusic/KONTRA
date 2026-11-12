//! An IR instrument prepared into a plan plays what the IR describes.
#![cfg(test)]

use std::io;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, SampleTime, Timed};
use kontra_ir as ir;
use kontra_prepare::prepare;
use kontra_stream::{Encoding, MemSource, SampleFmt, Source, Store, StoreConfig};

const RATE: u32 = 48_000;
const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};

/// Each sample's `offset` is its constant level in thousandths.
fn open(sample: &ir::Sample) -> io::Result<(Arc<dyn Source>, SampleFmt)> {
    if sample.name == "missing" {
        return Err(io::ErrorKind::NotFound.into());
    }
    let level = sample.offset as f32 / 1000.0;
    // "morph": a silent cycle, then one of a sine.
    let at = |i: u64| match sample.name.as_str() {
        "morph" if i >= 2048 => (std::f32::consts::TAU * (i - 2048) as f32 / 2048.0).sin(),
        "morph" => 0.0,
        _ => level,
    };
    let bytes = (0..sample.frames)
        .flat_map(|i| at(i).to_le_bytes())
        .collect();
    Ok((Arc::new(MemSource::new(FMT, bytes)), FMT))
}

fn sample(name: &str, level_milli: u64) -> ir::Sample {
    ir::Sample {
        name: name.into(),
        file: "mem".into(),
        offset: level_milli,
        len: None,
        channels: 1,
        frames: 48_000,
        rate: RATE,
        keystream: None,
    }
}

fn zone(sample: usize) -> ir::Zone {
    ir::Zone {
        id: 0,
        mix: Vec::new(),
        sample,
        keys: 0..=127,
        velocities: 1..=127,
        velocity_fade: (0, 0),
        key_fade: (0, 0),
        root: 60,
        tune: 0.0,
        volume_db: 0.0,
        pan: 0.0,
        start: 0,
        end: 48_000,
        start_mod: 0,
        loops: Default::default(),
        reverse: false,
        slot: 0,
    }
}

fn group(key: u8, zones: Vec<ir::Zone>) -> ir::Group {
    ir::Group {
        start: vec![(ir::Join::And, ir::StartRule::Keyswitch { range: key..=key })],
        zones,
        ..ir::Group::DEFAULT
    }
}

fn on(key: u8) -> Timed<Event> {
    struck(key, 1.0)
}

fn struck(key: u8, velocity: f64) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: HostNote {
                port: 0,
                channel: 0,
                key,
                id: -1,
                clap: true,
            },
            velocity,
            tune: 0.0,
        },
    }
}

#[test]
fn a_prepared_instrument_plays_its_keyswitched_groups() {
    let instrument = ir::Instrument {
        samples: vec![
            sample("legato", 300),
            sample("staccato", 600),
            sample("missing", 0),
        ],
        groups: vec![group(24, vec![zone(0), zone(2)]), group(25, vec![zone(1)])],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (plan, diagnostics) = prepare(&instrument, RATE, &mut store, &open);
    assert_eq!(plan.zones.len(), 2, "the missing sample's zone is dropped");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].impact, ir::Impact::Broken);
    assert_eq!(plan.switches, [24..=24, 25..=25]);

    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 256];
    e.process(&mut out, &[on(60)]);
    assert!((out[128][0] - 0.3).abs() < 1e-3, "legato: {:?}", out[128]);
    e.process(&mut out, &[on(25), on(62)]);
    // The switch key plays the staccato zone mapped on it too: Kontakt's latch
    // silences no key. Legato 0.3 + two staccato notes.
    assert!(
        (out[128][0] - 1.5).abs() < 1e-3,
        "staccato joins: {:?}",
        out[128]
    );
}

#[test]
fn a_cycling_group_plays_one_slot_per_note_in_turn() {
    let slotted = |sample, slot| ir::Zone {
        slot,
        ..zone(sample)
    };
    let instrument = ir::Instrument {
        samples: vec![sample("a", 100), sample("b", 200), sample("c", 400)],
        groups: vec![ir::Group {
            start: vec![(ir::Join::And, ir::StartRule::Cycle { random: false })],
            zones: vec![slotted(0, 0), slotted(1, 3), slotted(2, 5)],
            ..ir::Group::DEFAULT
        }],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (plan, _) = prepare(&instrument, RATE, &mut store, &open);
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 256];
    // Each note adds one slot's level to the held ones: a, then b, then c, then a.
    for (key, sum) in [(60, 0.1), (62, 0.3), (64, 0.7), (65, 0.8)] {
        e.process(&mut out, &[on(key)]);
        assert!((out[128][0] - sum).abs() < 1e-3, "{key}: {:?}", out[128]);
    }
}

/// A zone `set_zone_par` moves into a wavetable group plays its sample as a
/// table there, built off the audio thread; at load only that group's
/// zones get one.
#[test]
fn a_zone_moved_into_a_wavetable_group_gets_its_table() {
    let wavetable = ir::Wavetable {
        position: 0.0,
        phase: 0.0,
        warps: [ir::Warp::OFF; 2],
    };
    let mut bank = zone(1);
    (bank.id, bank.end) = (1, 4096);
    let instrument = ir::Instrument {
        samples: vec![sample("table", 500), sample("bank", 500)],
        groups: vec![
            ir::Group {
                wavetable: Some(wavetable),
                zones: vec![zone(0)],
                ..ir::Group::DEFAULT
            },
            ir::Group {
                zones: vec![bank],
                ..ir::Group::DEFAULT
            },
        ],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut plan, _) = prepare(&instrument, RATE, &mut store, &open);
    let client = store.client();
    assert!(kontra_prepare::tables(&plan, &client).is_none());
    assert!(plan.zones[1].region.table.is_none());
    assert!(plan.edit_zone(1, ir::ZonePar::Group, 0));
    let plan = kontra_prepare::tables(&plan, &client).unwrap();
    assert!(plan.zones[1].region.table.is_some());
    assert_eq!(plan.zones[1].root, 69, "a table plays its cycle at A4");
}

/// A zone `set_zone_par` moves to another group takes what the loader gives
/// that group's own zones: start rules, trigger, choke, key tracking.
#[test]
fn a_zone_moved_to_another_group_takes_that_groups_rules() {
    let mut moved = zone(0);
    moved.id = 7;
    let mut released = group(25, vec![zone(0)]);
    (released.trigger, released.keytrack, released.one_shot) = (ir::Trigger::Release, 0.0, true);
    released.off_by = Some(0);
    let instrument = ir::Instrument {
        samples: vec![sample("a", 500)],
        groups: vec![group(24, vec![moved]), released],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut plan, _) = prepare(&instrument, RATE, &mut store, &open);
    assert!(plan.edit_zone(7, ir::ZonePar::Group, 1));
    let (z, own) = (&plan.zones[0], &plan.zones[1]);
    assert_eq!(
        (&z.start, z.trigger, z.off_by, z.keytrack, z.one_shot),
        (
            &own.start,
            own.trigger,
            own.off_by,
            own.keytrack,
            own.one_shot
        )
    );
    assert_eq!(z.member, u16::MAX, "no mix route of its new group names it");
}

/// A zone's table reads as its current group says (Codex r4547464027): moved
/// between wavetable groups it keeps its levels and takes the destination's
/// read; moved to a group without one it plays its sample at its own root.
#[test]
fn a_moved_zones_table_follows_its_group() {
    let read = |position| ir::Wavetable {
        position,
        phase: 0.0,
        warps: [ir::Warp::OFF; 2],
    };
    let instrument = ir::Instrument {
        samples: vec![sample("table", 500)],
        groups: vec![
            ir::Group {
                wavetable: Some(read(0.0)),
                zones: vec![zone(0)],
                ..ir::Group::DEFAULT
            },
            ir::Group {
                wavetable: Some(read(0.75)),
                ..ir::Group::DEFAULT
            },
            ir::Group::DEFAULT,
        ],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut plan, _) = prepare(&instrument, RATE, &mut store, &open);
    let levels = plan.zones[0].region.table.as_ref().unwrap().data.clone();
    assert!(plan.edit_zone(0, ir::ZonePar::Group, 1));
    let table = plan.zones[0].region.table.as_ref().unwrap();
    assert_eq!(table.read.position, 0.75, "the destination's read");
    assert!(Arc::ptr_eq(&table.data, &levels), "its levels reused");
    assert!(plan.edit_zone(0, ir::ZonePar::Group, 2));
    assert!(plan.zones[0].region.table.is_none());
    assert_eq!(plan.zones[0].root, 60, "its own root again");
}

/// A zone's fades keep their lengths when a script moves its key range
/// (Codex r4549008557): a high-key fade at 60..=70 moved to 80..=90 fades
/// at 90, as the loader's own zone there does, and sounds across 80..=85.
#[test]
fn a_moved_zones_fades_follow_its_range() {
    let faded = |keys| ir::Zone {
        keys,
        key_fade: (0, 5),
        ..zone(0)
    };
    let mut moved = faded(60..=70);
    moved.id = 7;
    let instrument = ir::Instrument {
        samples: vec![sample("a", 500)],
        groups: vec![group(24, vec![moved, faded(80..=90)])],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (mut plan, _) = prepare(&instrument, RATE, &mut store, &open);
    assert!(plan.edit_zone(7, ir::ZonePar::HighKey, 90));
    assert!(plan.edit_zone(7, ir::ZonePar::LowKey, 80));
    let (z, own) = (&plan.zones[0], &plan.zones[1]);
    assert_eq!((&z.keys, &z.crossfades), (&own.keys, &own.crossfades));
    assert_eq!(z.crossfades[0].gain(85), 1.0);
}

/// A velocity route moves each voice's own wavetable position: two held
/// notes, struck at 1 and ¼, read the sine cycle fully and a quarter of the
/// way from the silent one (positions 1 and ¼, a linear morph).
#[test]
fn a_velocity_route_moves_each_voices_table_position() {
    let mut table = sample("morph", 0);
    table.frames = 4096;
    let mut z = zone(0);
    z.end = 4096;
    let instrument = ir::Instrument {
        samples: vec![table],
        groups: vec![ir::Group {
            wavetable: Some(ir::Wavetable {
                position: 0.0,
                phase: 0.0,
                warps: [ir::Warp::OFF; 2],
            }),
            mods: vec![ir::Mod {
                name: "VEL -> WT POS".into(),
                slot: 0,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Wavetable {
                    source: ir::Source::Velocity,
                    value: ir::TableValue::Position,
                    depth: 1.0,
                },
            }],
            zones: vec![z],
            ..ir::Group::DEFAULT
        }],
        ..ir::Instrument::DEFAULT
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (plan, _) = prepare(&instrument, RATE, &mut store, &open);
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 4800];
    e.process(&mut out, &[struck(69, 1.0), struck(81, 0.25)]);
    e.process(&mut out, &[]);
    // A4 and A5 (a table plays its cycle at A4), over 4800 frames: whole cycles of both.
    let level = |hz: f32| {
        let (mut c, mut s) = (0.0f32, 0.0f32);
        for (i, f) in out.iter().enumerate() {
            let w = std::f32::consts::TAU * hz * i as f32 / RATE as f32;
            (c, s) = (c + f[0] * w.cos(), s + f[0] * w.sin());
        }
        2.0 * c.hypot(s) / out.len() as f32
    };
    let ratio = level(880.0) / level(440.0);
    assert!((ratio - 0.25).abs() < 0.01, "{ratio}");
}
