//! UVI `eq` effects onto `sampler_ir::fx::eq`.
//!
//! - `DigitalEq` (16 bands, seven band types, GainScale, OverallGain): every band is a one-band
//!   `Config` under `Law::UviDigital` (the native band design 0x1412840f0) so a scripted bus keeps
//!   one live lane set per band. Type numbering is the order of the type names in Workstation
//!   4.0.9's literal pool (0x141e73028: Low Pass, High Pass, Band Pass, Notch, Low Shelf, High
//!   Shelf, Peak). `Slope` 0..=7 is order 1, 2, 3, 4, 6, 8, 12, 16 for every type; Q and Bandwidth
//!   are separate native fields. Per-band `Channels`, M/S mode and key tracking are reported.
//! - `ThreeBandShelves`: level plus two first-order shelves relative to it (generic RBJ law).
//! - TwoBandEq, ThreeBandEq, ParametricEQ (alias Parametric), also inside an `FXPreset` wrapper:
//!   fixed band types under `Law::Cookbook` ([`Translation::legacy_eq`]). ParametricEQ's shelf
//!   slope constants are unread and reported. `BiquadFilter` belongs to the filter family.
use crate::{Translation, number};
use ir::fx::eq::{Band, Config, Kind};
use roxmltree::Node;
use sampler_ir as ir;

/// DigitalEq `Slope` 0..=7 to the band order (6..96 dB/oct, table 0x141e731a8).
fn digital_order(slope: f64) -> u8 {
    [1, 2, 3, 4, 6, 8, 12, 16][(slope as usize).min(7)]
}

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
        for (name, why) in [("StereoMode", "DigitalEq M/S mode"), ("KeyTracking", "DigitalEq key tracking")] {
            let v = number(node, name, 0.0)?;
            if v != 0.0 {
                self.unsupported(at, why, v);
            }
        }
        // Native band frequency: Freq * 2^(Transpose + KeyTracking (key - 60) / 12), key 60 here.
        let transpose = number(node, "Transpose", 0.0)?;
        if transpose != 0.0 && live {
            self.unsupported(at, "DigitalEq transpose on live band frequencies", transpose);
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
            let hz = get("Freq", 1000.0)? * if live { 1.0 } else { transpose.exp2() };
            let mut b = one(kind, hz, get("Q", std::f64::consts::FRAC_1_SQRT_2)?, gain_db, enabled);
            b.law = ir::fx::eq::Law::UviDigital;
            b.poles = digital_order(get("Slope", 1.0)?);
            b.bandwidth = get("Bandwidth", 1.0)? as f32;
            let channels = get("Channels", 0.0)?;
            if channels != 0.0 {
                self.unsupported(at, "DigitalEq per-band channel selection", channels);
            }
            out.push(ir::Processor::Eq(Config::band(b)));
        }
        // The overall level is scaled by GainScale too (0x14128b730).
        let overall = number(node, "OverallGain", 0.0)? * scale;
        if overall != 0.0 || live {
            out.push(ir::Processor::Gain(ir::Gain::Linear(10f64.powf(overall / 20.0))));
        }
        Ok(())
    }

    /// TwoBandEq, ThreeBandEq and ParametricEQ: fixed band types under the cookbook law (their
    /// coefficient routines, checked under emulation). Two/ThreeBandEq shelves have Q 0.5
    /// (`alpha = sin w0 / (8 * 0 + 1)`), the ThreeBandEq bell `Q = (8 MidQ + 1) / 2`; shelves sit at
    /// 80 Hz and 10 kHz there. ParametricEQ bands are HP, low shelf, four bells, high shelf, LP; its
    /// shelves are `A = 10^(dB/20)` (twice the cookbook gain) with slope `S` remapped from Q.
    pub(crate) fn legacy_eq(&mut self, node: Node, at: &str, out: &mut Vec<ir::Processor>) -> Result<(), String> {
        let cook = |kind, hz: f64, q: f64, db: f64, on| Band { law: ir::fx::eq::Law::Cookbook, ..one(kind, hz, q, db, on) };
        let bands = match node.tag_name().name() {
            "TwoBandEq" => vec![
                cook(Kind::LowShelf, number(node, "LowFreq", 80.0)?, 0.5, number(node, "LowGain", 0.0)?, true),
                cook(Kind::HighShelf, number(node, "HighFreq", 10000.0)?, 0.5, number(node, "HighGain", 0.0)?, true),
            ],
            "ThreeBandEq" => vec![
                cook(Kind::LowShelf, 80.0, 0.5, number(node, "LowGain", 0.0)?, true),
                cook(
                    Kind::Peak,
                    number(node, "MidFreq", 2500.0)?,
                    (8.0 * number(node, "MidQ", 0.0)? + 1.0) / 2.0,
                    number(node, "MidGain", 0.0)?,
                    true,
                ),
                cook(Kind::HighShelf, 10000.0, 0.5, number(node, "HighGain", 0.0)?, true),
            ],
            _ => {
                let kinds = [
                    Kind::HighPass,
                    Kind::LowShelf,
                    Kind::Peak,
                    Kind::Peak,
                    Kind::Peak,
                    Kind::Peak,
                    Kind::HighShelf,
                    Kind::LowPass,
                ];
                let hz = [30.0, 70.0, 180.0, 520.0, 1870.0, 3900.0, 7400.0, 16000.0];
                let mut bands = Vec::new();
                for (i, kind) in kinds.into_iter().enumerate() {
                    let get = |name: &str, default| number(node, &format!("{name}{}", i + 1), default);
                    if get("Enable", 0.0)? == 0.0 {
                        continue;
                    }
                    let q = get("Q", if i == 1 || i == 6 { 12.0 } else { 0.7 })?;
                    let gain = get("Gain", 0.0)?;
                    let band = if matches!(kind, Kind::LowShelf | Kind::HighShelf) {
                        // ponytail: slope S = lerp((Q - 0.2) / 11.8) between two doubles in the
                        // encrypted .data (0x14258c150/158); runs at S = 1 until they are read.
                        self.unsupported(at, "ParametricEQ shelf slope (Q to S constants unread, runs at S 1)", q);
                        cook(kind, get("Freq", hz[i])?, std::f64::consts::FRAC_1_SQRT_2, 2.0 * gain, true)
                    } else {
                        cook(kind, get("Freq", hz[i])?, q, gain, true)
                    };
                    bands.push(band);
                }
                bands
            }
        };
        if let Some(config) = Config::new(&bands) {
            out.push(ir::Processor::Eq(config));
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
