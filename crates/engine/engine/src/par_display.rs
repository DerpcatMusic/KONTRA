//! `get_engine_par_disp`: which [`Readout`] a parameter's knob shows.

use crate::{Engine, EnginePar, params::stage_ms};
use kontra_ir as ir;
use kontra_script_ui::readout::{Readout, write};
use kontra_vm::Text;

impl Engine {
    /// `get_engine_par_disp(par, group, slot, generic)`, or with `ext` its `_ext`
    /// form, which shows its second argument. False when not carried.
    pub(crate) fn par_text(&mut self, ext: bool, n: impl Fn(usize) -> i32, out: &mut Text) -> bool {
        let par = self.script().and_then(|s| s.engine_pars.get(n(0)));
        let at = usize::from(ext) + 1;
        let raw = match ext {
            true => Some(n(1)),
            false => self.get_engine_par(n(0), n(at), n(at + 1), n(at + 2)),
        };
        *out = Text::EMPTY;
        // The reverb's time shows its law's seconds in ms (oracle 2026-10-29).
        if let Some((ir::ReverbPar::Seconds, s)) =
            par.zip(raw).and_then(|(p, r)| self.reverb_value(p, r))
        {
            return write(Readout::Padded(1000.0 * f64::from(s)), 0.0, out);
        }
        par.zip(raw).is_some_and(|(p, raw)| display(p, raw, out))
    }
}

/// Writes the display of `par` at `raw` (0..=1000000); false when unknown.
pub(crate) fn display(par: EnginePar, raw: i32, out: &mut Text) -> bool {
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
