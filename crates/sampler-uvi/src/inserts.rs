//! UVI insert effects as IR processors: aux-bus chains, effect racks and
//! keygroup inserts. Values are the program's static ones; a script that
//! writes them later does not reach these processors yet.
use super::{Translation, number, path};
use roxmltree::Node;
use sampler_ir as ir;

fn db(db: f64) -> ir::Gain {
    ir::Gain::Linear(10f64.powf(db / 20.0))
}

impl Translation {
    /// The enabled inserts of `parent` (an AuxEffect or a Keygroup), in order.
    /// `voice` chains have no convolution; `key` is the middle key of a
    /// keygroup, for key-tracked cutoffs.
    pub(super) fn inserts(
        &mut self,
        parent: Node,
        voice: bool,
        key: Option<u8>,
    ) -> Result<Vec<ir::Processor>, String> {
        let mut out = Vec::new();
        let Some(inserts) = parent.children().find(|n| n.has_tag_name("Inserts")) else {
            return Ok(out);
        };
        for node in inserts.children().filter(|n| n.is_element()) {
            if number(node, "Bypass", 0.0)? != 0.0 {
                continue;
            }
            let at = path(node);
            match node.tag_name().name() {
                "Gain" => {
                    out.push(ir::Processor::Gain(ir::Gain::Linear(number(
                        node, "Volume", 1.0,
                    )?)));
                }
                "GainMatrix" => {
                    // Gain_i_j: input i to output j. Channels 1 and 2 are the
                    // stereo pair; a stereo signal never feeds the others.
                    let g = |i: usize, j: usize| {
                        number(node, &format!("Gain_{i}_{j}"), f64::from(u8::from(i == j)))
                    };
                    out.push(ir::Processor::StereoMatrix([
                        [g(1, 1)?, g(2, 1)?],
                        [g(1, 2)?, g(2, 2)?],
                    ]));
                    let mut beyond = false;
                    for i in 1..=12 {
                        for j in 1..=12 {
                            if (i > 2 || j > 2) && g(i, j)? != f64::from(u8::from(i == j)) {
                                beyond = true;
                            }
                        }
                    }
                    if beyond {
                        self.unsupported(&at, "GainMatrix beyond the first two channels", "");
                    }
                }
                "DigitalEq" => self.digital_eq(node, &at, &mut out)?,
                "OnePole" => {
                    let tracking = number(node, "KeyTracking", 0.0)?;
                    let key = key.map_or(60.0, f64::from);
                    let hz = (number(node, "Freq", 1000.0)?
                        * 2f64.powf(tracking * (key - 60.0) / 12.0))
                    .clamp(20.0, 20000.0);
                    if tracking != 0.0 {
                        self.unsupported(&at, "OnePole key tracking (keygroup middle key)", tracking);
                    }
                    let poles = 1;
                    out.push(ir::Processor::Filter(ir::Filter {
                        kind: if number(node, "Mode", 0.0)? == 0.0 {
                            ir::FilterKind::LowPass { poles }
                        } else {
                            ir::FilterKind::HighPass { poles }
                        },
                        cutoff: ir::Frequency::Hertz(hz),
                        resonance: ir::Resonance::Decibels(0.0),
                    }));
                }
                "Convolver" | "SampledReverb" if !voice => {
                    let Some(sample) = node.attribute("SamplePath").filter(|p| !p.is_empty())
                    else {
                        continue;
                    };
                    let Some(asset) = self.asset(&at, sample) else {
                        continue;
                    };
                    self.ir.impulses.push(ir::Impulse {
                        rate: 48000,
                        left: vec![1.0],
                        right: vec![1.0],
                        asset: Some(asset),
                    });
                    if node.has_tag_name("SampledReverb") {
                        for name in ["Time", "DampingLow", "DampingHigh", "PreDelay", "Width"] {
                            let v = number(node, name, 0.0)?;
                            if v != 0.0 {
                                self.unsupported(&at, &format!("SampledReverb {name}"), v);
                            }
                        }
                    }
                    out.push(ir::Processor::Convolution {
                        impulse: ir::ImpulseRef(self.ir.impulses.len() - 1),
                        dry: number(node, "Dry", 0.0)?,
                        wet: number(node, "Wet", 1.0)?,
                    });
                }
                "TrackDelay" if number(node, "DelayTime", 0.0)? == 0.0 => {}
                "EffectRack" => {
                    // One live chain is a serial section at that chain's gain;
                    // several are parallel branches, which a serial chain cannot hold.
                    let live: Vec<_> = node
                        .descendants()
                        .filter(|n| n.has_tag_name("Chains"))
                        .flat_map(|c| c.children().filter(|c| c.has_tag_name("AuxEffect")))
                        .filter(|c| {
                            number(*c, "Bypass", 0.0).is_ok_and(|b| b == 0.0)
                                && number(*c, "Gain", 1.0).is_ok_and(|g| g != 0.0)
                        })
                        .collect();
                    match live[..] {
                        [] => {}
                        [chain] => {
                            out.push(ir::Processor::Gain(ir::Gain::Linear(number(
                                chain, "Gain", 1.0,
                            )?)));
                            let inner = self.inserts(chain, voice, key)?;
                            out.extend(inner);
                        }
                        _ => {
                            self.unsupported(&at, "EffectRack with parallel chains", live.len());
                            continue;
                        }
                    }
                }
                _ => continue,
            }
            self.used.push(node.id());
        }
        Ok(out)
    }

    fn digital_eq(
        &mut self,
        node: Node,
        at: &str,
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
            if get("Enabled", 0.0)? == 0.0 {
                continue;
            }
            let gain = db(get("Gain", 0.0)? * scale);
            let kind = match get("Type", 6.0)? as i64 {
                0 => ir::FilterKind::LowPass { poles: 2 },
                1 => ir::FilterKind::HighPass { poles: 2 },
                2 => ir::FilterKind::BandPass { poles: 2 },
                3 => ir::FilterKind::Notch { poles: 2 },
                4 => ir::FilterKind::LowShelf { gain },
                5 => ir::FilterKind::HighShelf { gain },
                _ => ir::FilterKind::Peak { gain },
            };
            let flat = matches!(
                kind,
                ir::FilterKind::Peak { gain: g }
                    | ir::FilterKind::LowShelf { gain: g }
                    | ir::FilterKind::HighShelf { gain: g }
                    if (g.linear() - 1.0).abs() < 1e-9
            );
            if flat {
                continue;
            }
            out.push(ir::Processor::Filter(ir::Filter {
                kind,
                cutoff: ir::Frequency::Hertz(get("Freq", 1000.0)?),
                resonance: ir::Resonance::Q(get("Q", std::f64::consts::FRAC_1_SQRT_2)?),
            }));
        }
        let overall = number(node, "OverallGain", 0.0)?;
        if overall != 0.0 {
            out.push(ir::Processor::Gain(db(overall)));
        }
        // The manual lists the seven shapes but not their numbering; this is
        // the listed order with peak last.
        self.unsupported(at, "DigitalEq band type numbering (manual order assumed)", "");
        Ok(())
    }
}

/// Decode each impulse that names an asset through `read` (rate and stereo
/// frames), before unused assets are dropped. A file that cannot be read
/// leaves the unit-impulse placeholder (the wet path then passes the input)
/// and is reported.
pub(super) fn fill_impulses(
    instrument: &mut ir::Instrument,
    locations: &[String],
    mut read: impl FnMut(usize) -> Result<(u32, Vec<[f32; 2]>), String>,
) {
    for index in 0..instrument.impulses.len() {
        let Some(asset) = instrument.impulses[index].asset.take() else {
            continue;
        };
        match read(asset.0) {
            Ok((rate, frames)) if !frames.is_empty() && frames.len() <= 1 << 24 => {
                let impulse = &mut instrument.impulses[index];
                impulse.rate = rate;
                impulse.left = frames.iter().map(|f| f[0]).collect();
                impulse.right = frames.iter().map(|f| f[1]).collect();
            }
            other => instrument.unsupported.push(ir::Unsupported {
                location: locations.get(asset.0).cloned().unwrap_or_default(),
                feature: "unreadable impulse response (passed through)".into(),
                value: other.err().unwrap_or_else(|| "empty or too long".into()),
                reason: ir::Reason::InvalidValue,
            }),
        }
    }
}
