//! Kontakt's Reverb (0x59) from the translator through the engine, against
//! the emulated native kernel (`0x141ae2c60`).
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Event, HostNote, Plan, Region, SampleTime, Timed, Zone, ir};
use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};

mod vectors;

const RATE: u32 = 44_100;
const FRAMES: usize = 8192;

/// The kernel's input: left 1.0 at 0, right 0.5 at 5, as a stereo zone
/// through an instrument insert reverb at the kernel's own mix `m` (wet
/// `sin(mπ/2)`, dry `cos`).
fn render(hall: bool, c: [f32; 10]) -> Vec<[f32; 2]> {
    const FMT: SampleFmt = SampleFmt {
        encoding: Encoding::F32,
        channels: 2,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut frames = vec![[0.0f32; 2]; FRAMES];
    frames[0][0] = 1.0;
    frames[5][1] = 0.5;
    let bytes = frames
        .iter()
        .flatten()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "impulse".into(),
        offset: 0,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            FRAMES as u64,
            std::slice::from_ref(&(0..FRAMES as u64)),
        )
        .unwrap();
    let region = Region {
        mix: None,
        sample,
        start: 0,
        end: FRAMES as u64,
        loops: vec![],
        reverse: false,
    };
    let mut plan = Plan::new(vec![Zone::new(region, 60, RATE)], 16);
    for g in &mut plan.groups {
        g.mods.clear();
    }
    let (wet, dry) = (c[8] * std::f32::consts::FRAC_PI_2).sin_cos();
    let v = [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[9]];
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Reverb(kontra_format_kontakt::reverb(i32::from(hall), v)),
        bypass: false,
        output: wet,
        dry,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let note = HostNote {
        port: 0,
        channel: 0,
        key: 60,
        id: -1,
        clap: false,
    };
    let on = Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note,
            velocity: 1.0,
            tune: 0.0,
        },
    };
    let mut out = vec![[0.0f32; 2]; FRAMES];
    e.process(&mut out, &[on]);
    out
}

#[test]
fn room_and_hall_match_the_native_kernel() {
    for (name, (hall, c, want)) in [
        ("room a", vectors::ROOM_A),
        ("hall a", vectors::HALL_A),
        ("room b", vectors::ROOM_B),
        ("hall b", vectors::HALL_B),
    ] {
        let got = render(hall, c);
        let peak = want.iter().flatten().fold(0f32, |m, v| m.max(v.abs()));
        let mut worst = 0f32;
        for ch in 0..2 {
            for (k, &w) in want[ch].iter().enumerate() {
                worst = worst.max((got[k * 64][ch] - w).abs() / peak.max(1e-3));
            }
        }
        // The controls pass through physical units (ms, the Exp law), an
        // ulp off the native's own f32 arithmetic; Room B drifts most.
        assert!(worst <= 2e-5, "{name}: {worst:e} of peak");
    }
}
