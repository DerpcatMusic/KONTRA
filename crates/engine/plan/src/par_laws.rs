//! The laws between an engine parameter's 0..=1 setting and what it means:
//! envelope stage times, the attack curve, and the text its knob shows.

use kontra_ir as ir;
use kontra_script_ui::readout::{Readout, write};
use kontra_vm::Text;

use crate::{EnginePar, EnginePars};

/// Attack and hold run 0..=15 s, decay and release 0..=25 s, exponentially
/// with a 2 ms shift: `exp(ln 2 + (ln(top + 2) − ln 2) × x) − 2` ms.
pub fn stage_ms(top_ms: f64, x: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    ((low + (high - low) * x).exp() - 2.0).max(0.0)
}

/// The attack curve knob (raw / 500000 − 1: −1 concave ..= 1 convex) as a
/// segment shape: the law the Kontakt translator applies to stored curves
/// (format/kontakt/src/laws.rs, docs/re/engine/k-mod-envelopes.md).
pub fn attack_shape(x: f64) -> f32 {
    let c = (x * 2.0 - 1.0).clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// Inverse of [`attack_shape`], as a 0..=1 knob.
pub fn attack_x(shape: f32) -> f64 {
    let k = f64::from(shape);
    if k == 0.0 {
        return 0.5;
    }
    let b = 1.0 / k.abs().exp_m1();
    let c = (1.0 - (b.ln() + 20_000f64.ln()) / 500_000f64.ln()).clamp(0.0, 1.0);
    0.5 * (1.0 - k.signum() * c)
}

/// The inverse of [`stage_ms`]: the 0..=1 setting of a stage `ms` long.
pub fn stage_x(top_ms: f64, ms: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    (((ms + 2.0).ln() - low) / (high - low)).clamp(0.0, 1.0)
}

/// Writes the text of parameter id `id` at `raw` (0..=1000000): its
/// translator readout, else the engine's own law; false when unknown.
pub fn text(pars: &EnginePars, id: i32, raw: i32, out: &mut Text) -> bool {
    let x = raw.clamp(0, 1_000_000);
    let Some(r) = pars.readout(id) else {
        return pars.get(id).is_some_and(|par| display(par, raw, out));
    };
    if let Some(end) = [0, 1_000_000]
        .iter()
        .zip(r.ends)
        .find_map(|(&at, e)| e.filter(|_| x == at))
    {
        out.push(end);
        return true;
    }
    let v = f64::from(r.law.eval(x as f32 / 1e6));
    let readout = match r.form {
        ir::Form::Padded => Readout::Padded(v),
        ir::Form::Decimal => Readout::Signed(v),
        ir::Form::Wide => Readout::Wide(v),
        ir::Form::Whole => Readout::Whole(v),
        ir::Form::Ratio => Readout::Ratio(v),
        ir::Form::Pan => Readout::Pan,
    };
    write(readout, v, out)
}

/// Writes the display of `par` at `raw` (0..=1000000); false when unknown.
pub fn display(par: EnginePar, raw: i32, out: &mut Text) -> bool {
    let x = f64::from(raw.clamp(0, 1_000_000)) / 1e6;
    let readout = match par {
        EnginePar::Volume => Readout::Db(12.0),
        // The Gainer's gain reads as an output level (oracle 2026-10-29).
        EnginePar::EffectOutput | EnginePar::GainerGain => Readout::Db(24.0),
        EnginePar::Sustain => Readout::Db(0.0),
        EnginePar::Tune => Readout::Tune,
        EnginePar::Attack | EnginePar::Hold => Readout::Padded(stage_ms(15_000.0, x)),
        EnginePar::Decay | EnginePar::Release => Readout::Padded(stage_ms(25_000.0, x)),
        EnginePar::Pan => Readout::Pan,
        EnginePar::RouteIntensity => Readout::Padded(100.0 * x * x),
        EnginePar::RouteDepth => Readout::Signed(100.0 * (2.0 * x - 1.0).powi(3)),
        _ => return false,
    };
    write(readout, x, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_curve_round_trips() {
        for x in [0.1, 0.3, 0.5, 0.7, 0.95] {
            assert!((attack_x(attack_shape(x)) - x).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn stage_times_follow_kontakts_shifted_exponential() {
        assert!(stage_ms(15_000.0, 0.0).abs() < 1e-9);
        assert!((stage_ms(15_000.0, 1.0) - 15_000.0).abs() < 1e-6);
        assert!((stage_ms(25_000.0, 0.5) - ((2.0f64 * 25_002.0).sqrt() - 2.0)).abs() < 1e-6);
        for x in [0.0, 0.25, 0.8, 1.0] {
            assert!((stage_x(25_000.0, stage_ms(25_000.0, x)) - x).abs() < 1e-9);
        }
    }

    fn show(par: EnginePar, raw: i32) -> String {
        let mut t = Text::EMPTY;
        assert!(display(par, raw, &mut t));
        t.as_str().to_owned()
    }

    /// Kontakt 8.13.1's answers (probe in the module docs).
    #[test]
    fn displays_match_kontakt() {
        use EnginePar as P;
        let cases: &[(P, &[(i32, &str)])] = &[
            (
                P::Volume,
                &[
                    (0, "-inf"),
                    (1000, "-167.4"),
                    (100_000, "-47.8"),
                    (400_000, "-11.8"),
                    (500_000, "-6.0"),
                    (600_000, "-1.3"),
                    (630_859, "0.0"),
                    (700_000, "2.7"),
                    (900_000, "9.3"),
                    (1_000_000, "12.0"),
                ],
            ),
            (
                P::EffectOutput,
                &[
                    (0, "-inf"),
                    (1000, "-155.4"),
                    (100_000, "-35.8"),
                    (250_000, "-12.0"),
                    (500_000, "6.0"),
                    (750_000, "16.5"),
                    (1_000_000, "24.0"),
                ],
            ),
            (
                P::RouteIntensity,
                &[
                    (0, " 0.00"),
                    (250_000, "  6.2"),
                    (500_000, " 25.0"),
                    (750_000, " 56.2"),
                    (1_000_000, "100.0"),
                ],
            ),
            (
                P::RouteDepth,
                &[
                    (0, "-100.0"),
                    (100_000, "-51.2"),
                    (400_000, "-0.8"),
                    (500_000, "0.0"),
                    (750_000, "12.5"),
                ],
            ),
            (
                P::GainerGain,
                &[
                    (0, "-inf"),
                    (300_000, "-7.3"),
                    (400_000, "0.2"),
                    (900_000, "21.3"),
                ],
            ),
            (
                P::Sustain,
                &[
                    (0, "-inf"),
                    (1000, "-179.4"),
                    (100_000, "-59.8"),
                    (250_000, "-36.0"),
                    (500_000, "-18.0"),
                    (750_000, "-7.5"),
                    (1_000_000, "0.0"),
                ],
            ),
            (
                P::Tune,
                &[
                    (0, "-36.00"),
                    (250_000, "-18.00"),
                    (500_000, "0.00"),
                    (504_000, "0.29"),
                    (1_000_000, "36.00"),
                ],
            ),
            (
                P::Attack,
                &[
                    (0, " 0.00"),
                    (1000, " 0.02"),
                    (100_000, "  2.9"),
                    (250_000, " 16.6"),
                    (500_000, "171.2"),
                    (750_000, "  1.6k"),
                    (1_000_000, " 15.0k"),
                ],
            ),
            (
                P::Release,
                &[
                    (0, " 0.00"),
                    (1000, " 0.02"),
                    (100_000, "  3.1"),
                    (250_000, " 19.1"),
                    (500_000, "221.6"),
                    (750_000, "  2.4k"),
                    (1_000_000, " 25.0k"),
                ],
            ),
            (
                P::Pan,
                &[
                    (0, "100 L"),
                    (1000, "100 L"),
                    (250_000, "50 L"),
                    (495_000, "1 L"),
                    (497_500, "Center"),
                    (500_000, "Center"),
                    (502_600, "Center"),
                    (505_000, "1 R"),
                    (507_500, "1 R"),
                    (750_000, "50 R"),
                    (1_000_000, "100 R"),
                ],
            ),
        ];
        for (par, values) in cases {
            for &(raw, want) in *values {
                assert_eq!(show(*par, raw), want, "{par:?} {raw}");
            }
        }
    }
}
