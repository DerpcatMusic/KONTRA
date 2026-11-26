//! The legacy Delay (0x10) against the reference 8.13's own renders
//! (docs/re/engine/fx-measured/legacy-delay.md): an impulse into the left
//! input across time, feedback, pan (ping-pong), and damping; per case the
//! delay frame, first two echo peaks, and 0.6 s wet RMS level.

use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;

const TABLE: &str = include_str!("../../../../docs/re/engine/fx-measured/legacy-delay.tsv");

#[test]
fn the_legacy_delay_matches_the_reference_render() {
    for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| f[i].parse::<f32>().unwrap();
        let delay = kontra_kontakt_effects::delays::delay(num(1), num(2), num(3), num(4), 0.0);
        let slot = ir::Effect {
            kind: ir::EffectKind::Delay(delay),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(&slot), 48_000, &Responses(&[], &[])) else {
            panic!("{}: no processor", f[0]);
        };
        let mut io = vec![[0.0f32; 2]; 28_800];
        io[0][0] = 1.0;
        for chunk in io.chunks_mut(256) {
            unit.process(chunk);
        }
        let df: usize = f[5].parse().unwrap();
        let e1 = io[df][0];
        let e2 = io[2 * df][0];
        assert!(
            (e1 - num(6)).abs() < 2e-3,
            "{}: echo1 at frame {df} got {e1:.5}, want {:.5}",
            f[0],
            num(6)
        );
        assert!(
            (e2 - num(7)).abs() < 2e-3,
            "{}: echo2 at frame {} got {e2:.5}, want {:.5}",
            f[0],
            2 * df,
            num(7)
        );
        let w = &io[100..28_800];
        let rms_db = 10.0
            * (w.iter().map(|s| f64::from(s[0]).powi(2)).sum::<f64>() / w.len() as f64).log10();
        let want_db = f64::from(num(8));
        assert!(
            (rms_db - want_db).abs() < 0.25,
            "{}: 0.6s RMS got {rms_db:.2} dB, want {want_db:.2} dB",
            f[0]
        );
    }
}
