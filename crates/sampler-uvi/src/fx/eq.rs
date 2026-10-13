//! UVI `eq` effects onto `sampler_ir::fx::eq`.
//!
//! - `DigitalEq` (16 bands, seven band types, GainScale, OverallGain): every band is a one-band
//!   `Config` so a scripted bus keeps one live lane set per band. Type numbering 5 (high shelf)
//!   and 6 (peak) is evidenced by installed banks; 0..=4 follow the manual order (reported).
//!   UVI's own band laws are not recovered: bands use the generic RBJ law.
//! - `ThreeBandShelves`: level plus two first-order shelves relative to it (generic RBJ law).
//! - `FXPreset` EQs (TwoBandEq, ThreeBandEq, Parametric) are not reachable: the translator has no
//!   FXPreset container, and their attribute ranges, corner frequencies and band types are
//!   UNPROVED in uvi-elements.tsv. `BiquadFilter` belongs to the filter family.
use crate::{Translation, engine_parameters, number};
use ir::fx::eq::{Band, Config, Kind};
use roxmltree::Node;
use sampler_ir as ir;

fn one(kind: Kind, frequency: f64, q: f64, gain_db: f64, enabled: bool) -> ir::Processor {
    let mut band = Band::new(kind, frequency as f32, q as f32, gain_db as f32);
    band.enabled = enabled;
    ir::Processor::Eq(Config::band(band))
}

impl Translation {
    pub(crate) fn digital_eq(
        &mut self,
        node: Node,
        at: &str,
        live: bool,
        out: &mut Vec<ir::Processor>,
    ) -> Result<(), String> {
        for (name, why) in [
            ("StereoMode", "DigitalEq M/S mode"),
            ("Transpose", "DigitalEq transpose"),
            ("KeyTracking", "DigitalEq key tracking"),
        ] {
            let v = number(node, name, 0.0)?;
            if v != 0.0 {
                self.unsupported(at, why, v);
            }
        }
        let scale = number(node, "GainScale", 1.0)?;
        let mut unproven = false;
        for band in 1..=16 {
            let get = |name: &str, default| number(node, &format!("{name}{band}"), default);
            // A scripted bus keeps all 16 bands and the overall level so
            // each one is a control (`engine_parameters::lanes`).
            let enabled = get("Enabled", 0.0)? != 0.0;
            if !enabled && !live {
                continue;
            }
            let gain_db = get("Gain", 0.0)? * scale;
            let shape = get("Type", 6.0)? as i64;
            // Installed libraries only show types 5 (gain, high-shelf-like
            // frequencies) and 6 (peak); 0 to 4 follow the manual's order.
            unproven |= (0..=4).contains(&shape);
            let kind = match shape {
                0 => Kind::LowPass,
                1 => Kind::HighPass,
                2 => Kind::BandPass,
                3 => Kind::Notch,
                4 => Kind::LowShelf,
                5 => Kind::HighShelf,
                _ => Kind::Peak,
            };
            let flat = matches!(kind, Kind::Peak | Kind::LowShelf | Kind::HighShelf) && gain_db.abs() < 1e-9;
            if flat && !live {
                continue;
            }
            let q = engine_parameters::band_q(|n| node.attribute(n).and_then(|v| v.parse().ok()), band);
            out.push(one(kind, get("Freq", 1000.0)?, q, gain_db, enabled));
        }
        let overall = number(node, "OverallGain", 0.0)?;
        if overall != 0.0 || live {
            out.push(ir::Processor::Gain(ir::Gain::Linear(10f64.powf(overall / 20.0))));
        }
        // The manual lists the seven shapes but not their numbering; types 5
        // and 6 are evidenced by the installed banks, 0 to 4 are its order.
        if unproven {
            self.unsupported(at, "DigitalEq band type numbering (manual order assumed)", "");
        }
        Ok(())
    }

    pub(crate) fn three_band_shelves(&mut self, node: Node, out: &mut Vec<ir::Processor>) -> Result<(), String> {
        let middle = number(node, "GainMid", 0.0)?;
        out.push(ir::Processor::Gain(ir::Gain::Linear(10f64.powf(middle / 20.0))));
        for (name, frequency, default, low) in [
            ("GainLow", "FreqLowMid", 200.0, true),
            ("GainHigh", "FreqMidHigh", 4000.0, false),
        ] {
            let kind = if low { Kind::FirstOrderLowShelf } else { Kind::FirstOrderHighShelf };
            out.push(one(
                kind,
                number(node, frequency, default)?,
                std::f64::consts::FRAC_1_SQRT_2,
                number(node, name, 0.0)? - middle,
                true,
            ));
        }
        Ok(())
    }
}
