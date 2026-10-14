//! UVI `eq` effects onto `sampler_ir::fx::eq`.
//!
//! - `DigitalEq` (16 bands, seven band types, GainScale, OverallGain): every band is a one-band
//!   `Config` so a scripted bus keeps one live lane set per band. Type numbering is the order of
//!   the type names in Workstation 4.0.9's literal pool (0x141e73028: Low Pass, High Pass, Band
//!   Pass, Notch, Low Shelf, High Shelf, Peak), which the installed banks confirm for 5 and 6.
//!   `Slope` 0..=7 is 6/12/18/24/36/48/72/96 dB (the list that follows): 6..=24 dB are 1..=4
//!   poles, steeper slopes are reported and run at 24 dB. Per-band `Channels` other than 0 is
//!   reported. UVI's own band laws are not recovered: bands use the generic RBJ law.
//! - `ThreeBandShelves`: level plus two first-order shelves relative to it (generic RBJ law).
//! - `FXPreset` EQs (TwoBandEq, ThreeBandEq, Parametric) are not reachable: the translator has no
//!   FXPreset container, and their attribute ranges, corner frequencies and band types are
//!   UNPROVED in uvi-elements.tsv. `BiquadFilter` belongs to the filter family.
use crate::{Translation, engine_parameters, number};
use ir::fx::eq::{Band, Config, Kind};
use roxmltree::Node;
use sampler_ir as ir;

fn one(kind: Kind, frequency: f64, q: f64, gain_db: f64, enabled: bool) -> Band {
    let mut band = Band::new(kind, frequency as f32, q as f32, gain_db as f32);
    band.enabled = enabled;
    band
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
            let mut b = one(kind, get("Freq", 1000.0)?, q, gain_db, enabled);
            if matches!(kind, Kind::LowPass | Kind::HighPass) {
                let slope = get("Slope", 1.0)? as i64;
                if slope > 3 {
                    self.unsupported(at, "DigitalEq slope steeper than 24 dB/oct (runs at 24)", slope);
                }
                b.poles = slope.clamp(0, 3) as u8 + 1;
            }
            let channels = get("Channels", 0.0)?;
            if channels != 0.0 {
                self.unsupported(at, "DigitalEq per-band channel selection", channels);
            }
            out.push(ir::Processor::Eq(Config::band(b)));
        }
        let overall = number(node, "OverallGain", 0.0)?;
        if overall != 0.0 || live {
            out.push(ir::Processor::Gain(ir::Gain::Linear(10f64.powf(overall / 20.0))));
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
            out.push(ir::Processor::Eq(Config::band(one(
                kind,
                number(node, frequency, default)?,
                std::f64::consts::FRAC_1_SQRT_2,
                number(node, name, 0.0)? - middle,
                true,
            ))));
        }
        Ok(())
    }
}
