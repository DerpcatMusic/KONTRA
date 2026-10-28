//! `get_engine_par_disp`: a knob's text without its unit, as Kontakt 8.13.1
//! printed it to oracle probes (2026-10-29); pan follows the binary's
//! formatter at 0x1407b7ac0 (f32, Center inside ±0.0075, else half up).

use crate::{Engine, engine_pars::EnginePar, params::stage_ms};
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
        par.zip(raw).is_some_and(|(p, raw)| display(p, raw, out))
    }
}

/// Writes the display of `par` at `raw` (0..=1000000); false when unknown.
pub(crate) fn display(par: EnginePar, raw: i32, out: &mut Text) -> bool {
    let x = f64::from(raw.clamp(0, 1_000_000)) / 1e6;
    // Gains run 6 dB per doubling of the cubed knob.
    let db = |top: f64, out: &mut Text| match x > 0.0 {
        true => out.push_fmt(format_args!("{:.1}", 18.0 * x.log2() + top)),
        false => out.push("-inf"),
    };
    let time = |top: f64, out: &mut Text| match stage_ms(top, x) {
        ms if ms < 1.0 => out.push_fmt(format_args!("{ms:5.2}")),
        ms if ms < 1000.0 => out.push_fmt(format_args!("{ms:5.1}")),
        ms => out.push_fmt(format_args!("{:5.1}k", ms / 1000.0)),
    };
    let pan = 2.0 * (x as f32) - 1.0;
    match par {
        EnginePar::Volume => db(12.0, out),
        EnginePar::EffectOutput => db(24.0, out),
        EnginePar::Sustain => db(0.0, out),
        EnginePar::Tune => out.push_fmt(format_args!("{:.2}", (2.0 * x - 1.0) * 36.0)),
        EnginePar::Attack | EnginePar::Hold => time(15_000.0, out),
        EnginePar::Decay | EnginePar::Release => time(25_000.0, out),
        EnginePar::Pan if f64::from(pan.abs()) < 0.0075 => out.push("Center"),
        EnginePar::Pan => {
            let side = if pan < 0.0 { "L" } else { "R" };
            out.push_fmt(format_args!("{} {side}", (pan.abs() * 100.0 + 0.5) as i32))
        }
        _ => return false,
    };
    true
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
