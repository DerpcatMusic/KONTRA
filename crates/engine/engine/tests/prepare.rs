//! An IR instrument prepared into a plan plays what the IR describes.
#![cfg(test)]

use std::io;
use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, SampleTime, Timed, prepare};
use kontra_ir as ir;
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
    let bytes = (0..sample.frames)
        .flat_map(|_| level.to_le_bytes())
        .collect();
    Ok((Arc::new(MemSource::new(FMT, bytes)), FMT))
}

fn sample(name: &str, level_milli: u64) -> ir::Sample {
    ir::Sample {
        name: name.into(),
        file: "mem".into(),
        offset: level_milli,
        len: 0,
        codec: ir::Codec::Pcm {
            bits: 32,
            float: true,
            big_endian: false,
        },
        channels: 1,
        frames: 48_000,
        rate: RATE,
        keystream: None,
    }
}

fn zone(sample: usize) -> ir::Zone {
    ir::Zone {
        sample,
        keys: 0..=127,
        velocities: 1..=127,
        velocity_fade: (0, 0),
        root: 60,
        tune: 0.0,
        volume_db: 0.0,
        pan: 0.0,
        start: 0,
        end: 48_000,
        start_mod: 0,
        looped: None,
        reverse: false,
    }
}

fn group(key: u8, zones: Vec<ir::Zone>) -> ir::Group {
    ir::Group {
        velocity: ir::Velocity::Flat,
        start: vec![ir::StartRule::Keyswitch {
            key,
            range: 24..=25,
        }],
        zones,
        ..ir::Group::default()
    }
}

fn on(key: u8) -> Timed<Event> {
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
            velocity: 1.0,
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
        ..ir::Instrument::default()
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let (plan, diagnostics) = prepare(&instrument, RATE, &mut store, &mut open);
    assert_eq!(plan.zones.len(), 2, "the missing sample's zone is dropped");
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].impact, ir::Impact::Broken);
    assert_eq!(plan.keyswitches, Some(24..=25));

    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0; 2]; 256];
    e.process(&mut out, &[on(60)]);
    assert!((out[128][0] - 0.3).abs() < 1e-3, "legato: {:?}", out[128]);
    e.process(&mut out, &[on(25), on(62)]);
    assert!(
        (out[128][0] - 0.9).abs() < 1e-3,
        "staccato joins: {:?}",
        out[128]
    );
}
