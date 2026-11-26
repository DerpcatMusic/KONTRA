//! The legacy Chorus (0x11) against the reference 8.13's own renders
//! (docs/re/engine/fx-measured/legacy-chorus.md): static impulse taps at
//! 10 / 20 ms with 0.95 one-pole smoothing and 500 Hz swept-sine LFO depth/rate.

use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;

const TABLE: &str = include_str!("../../../../docs/re/engine/fx-measured/legacy-chorus.tsv");

#[test]
fn the_legacy_chorus_matches_the_reference_render() {
    for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| f[i].parse::<f32>().unwrap();
        let kind = (kontra_kontakt_effects::delays::CHORUS.build)(&[num(2), num(3), num(4), 0.0])
            .expect("chorus model");
        let slot = ir::Effect {
            kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(&slot), 48_000, &Responses(&[], &[])) else {
            panic!("{}: no processor", f[0]);
        };
        match f[1] {
            "impulse" => {
                let mut io = vec![[0.0f32; 2]; 2_048];
                io[0][0] = 1.0;
                for chunk in io.chunks_mut(256) {
                    unit.process(chunk);
                }
                let t1: usize = f[5].parse().unwrap();
                let t2: usize = f[6].parse().unwrap();
                // Allow ±1 frame for delay-line read-before/after-write indexing.
                let peak_near = |center: usize| {
                    (center.saturating_sub(1)..=center + 1)
                        .map(|i| (i, io[i][0]))
                        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                        .unwrap()
                };
                let (p1, v1) = peak_near(t1);
                let (p2, v2) = peak_near(t2);
                let want_peak = num(7);
                let want_tail = num(8);
                assert!(
                    (v1 - want_peak).abs() < 0.02 && (v2 - want_peak).abs() < 0.02,
                    "{}: tap peaks ({v1:.4} @ {p1}, {v2:.4} @ {p2}), want {want_peak:.4}",
                    f[0]
                );
                assert!(
                    (io[p1 + 1][0] - want_tail).abs() < 0.01
                        && (io[p2 + 1][0] - want_tail).abs() < 0.01,
                    "{}: one-pole tail ({:.4}, {:.4}), want {want_tail:.4}",
                    f[0],
                    io[p1 + 1][0],
                    io[p2 + 1][0]
                );
            }
            "sine" => {
                // Verify Delay IR depth_ms and rate match the measured sweep and LFO Hz,
                // and that processing 1 s of 500 Hz sine stays bounded and sums both heads (~2x).
                let ir::EffectKind::Delay(d) = &slot.kind else {
                    panic!("{}: not a delay", f[0]);
                };
                let want_swing = num(9);
                assert!(
                    (d.depth_ms - want_swing).abs() < 0.03,
                    "{}: depth_ms {:.4}, measured {:.4}",
                    f[0],
                    d.depth_ms,
                    want_swing
                );
                assert_eq!(d.rate, ir::Rate::Hz(num(3)));
                let mut io: Vec<[f32; 2]> = (0..48_000)
                    .map(|n| {
                        let s = 0.25 * (std::f32::consts::TAU * 500.0 * n as f32 / 48_000.0).sin();
                        [s, 0.0]
                    })
                    .collect();
                for chunk in io.chunks_mut(256) {
                    unit.process(chunk);
                }
                let rms = (io[4_800..]
                    .iter()
                    .map(|s| f64::from(s[0]).powi(2))
                    .sum::<f64>()
                    / (io.len() - 4_800) as f64)
                    .sqrt();
                // Two in-phase heads on a 0.25-peak (0.177 RMS) 500 Hz sine produce ~0.35 RMS.
                assert!(
                    (rms - 0.35).abs() < 0.04,
                    "{}: swept 500 Hz RMS {rms:.4}, expected ~0.35",
                    f[0]
                );
            }
            other => panic!("unknown kind {other}"),
        }
    }
}
