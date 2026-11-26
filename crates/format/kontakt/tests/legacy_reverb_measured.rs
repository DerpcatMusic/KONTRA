//! The legacy Reverb (0x15) against the reference 8.13's own renders
//! (docs/re/engine/fx-measured/legacy-reverb.md): an impulse into the left
//! input across size, colour, damping, stereo narrowing, and predelay; per case
//! the frame of the first echo and the wet level of four windows, dB re the impulse.

use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;

const TABLE: &str = include_str!("../../../../docs/re/engine/fx-measured/legacy-reverb.tsv");

#[test]
fn the_legacy_reverb_matches_the_reference_render() {
    let windows = [(0.06, 0.1), (0.1, 0.2), (0.3, 0.4), (0.6, 0.8)];
    for line in TABLE.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let num = |i: usize| f[i].parse::<f32>().unwrap();
        let reverb =
            kontra_kontakt_effects::legacy_reverb::reverb(num(1), [num(2), num(3), num(4), num(5)]);
        let slot = ir::Effect {
            kind: ir::EffectKind::Reverb(reverb),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(&slot), 48_000, &Responses(&[], &[])) else {
            panic!("{}: no processor", f[0]);
        };
        let mut io = vec![[0.0f32; 2]; 48_000];
        io[0][0] = 1.0;
        for chunk in io.chunks_mut(256) {
            unit.process(chunk);
        }
        let left: Vec<f64> = io.iter().map(|s| f64::from(s[0])).collect();
        let first = left
            .iter()
            .skip(2)
            .position(|v| v.abs() > 1e-5)
            .map(|p| p + 2);
        assert_eq!(first, Some(f[6].parse().unwrap()), "{}: first echo", f[0]);
        for (k, (a, b)) in windows.iter().enumerate() {
            let w = &left[(a * 48_000.0) as usize..(b * 48_000.0) as usize];
            let db = 10.0 * (w.iter().map(|v| v * v).sum::<f64>() / w.len() as f64).log10();
            let want = f64::from(num(7 + k));
            assert!(
                (db - want).abs() < 0.3,
                "{} {a}-{b} s: {db:.2} dB, reference {want:.2}",
                f[0]
            );
        }
    }
}
