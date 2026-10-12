//! UVI insert effects as IR processors: aux-bus chains, effect racks and
//! keygroup inserts. Exact catalog fields with supported native DSP lanes
//! receive live controls in `engine_parameters`; other writes stay reported.
use super::{Translation, engine_parameters, number, path};
use roxmltree::Node;
use sampler_ir as ir;

fn db(db: f64) -> ir::Gain {
    ir::Gain::Linear(10f64.powf(db / 20.0))
}

/// Where an insert element's processors sit in its chain, so a script that
/// writes the insert's parameters can reach them. Every insert element has an
/// entry, a bypassed or unmodeled one with `count` 0. A DigitalEq's range is
/// one processor per enabled non-flat band in band order, then its
/// OverallGain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertNode {
    /// The element's `roxmltree::NodeId::get_usize()`.
    pub node: usize,
    pub chain: ir::ChainRef,
    pub first: usize,
    pub count: usize,
}

/// An insert's processors before its chain exists: element, first, count.
pub(super) type Placed = (usize, usize, usize);

impl Translation {
    /// A program/layer insert sees the sum of its children, not each voice.
    pub(super) fn insert_bus(
        &mut self,
        parent: Node,
        output: ir::Output,
    ) -> Result<ir::Output, String> {
        let (processors, placed) = self.inserts(parent, false, None)?;
        if placed.is_empty() {
            return Ok(output);
        }
        let bus = ir::BusRef(self.ir.buses.len());
        let chain = ir::ChainRef(self.ir.chains.len());
        self.ir.chains.push(ir::Chain {
            scope: ir::Scope::Bus(bus),
            pre_amplitude: processors,
            post_amplitude: Vec::new(),
        });
        self.place(chain, placed);
        self.ir.buses.push(ir::Bus {
            name: parent.attribute("Name").unwrap_or_default().into(),
            chain: Some(chain),
            sends: Vec::new(),
            output,
            gain: ir::Gain::UNITY,
        });
        Ok(ir::Output::Bus(bus))
    }

    /// Record `placed` entries (relative to one chain) once the chain is `chain`.
    pub(super) fn place(&mut self, chain: ir::ChainRef, placed: Vec<Placed>) {
        self.insert_nodes
            .extend(placed.into_iter().map(|(node, first, count)| InsertNode {
                node,
                chain,
                first,
                count,
            }));
    }

    /// The enabled inserts of `parent` (an AuxEffect or a Keygroup), in order.
    /// `voice` chains have no convolution; `key` is the middle key of a
    /// keygroup, for key-tracked cutoffs.
    pub(super) fn inserts(
        &mut self,
        parent: Node,
        voice: bool,
        key: Option<u8>,
    ) -> Result<(Vec<ir::Processor>, Vec<Placed>), String> {
        let mut out = Vec::new();
        let mut placed = Vec::new();
        let Some(inserts) = parent.children().find(|n| n.has_tag_name("Inserts")) else {
            return Ok((out, placed));
        };
        // A scripted program's inserts sit inside a Mix so `setParameter
        // ("Bypass")` can reach them at runtime (core `slot_control` path), and
        // its bus-scope inserts get the control-driven stages that
        // `engine_parameters::lanes` addresses.
        let scripted = !self.ir.behaviors.is_empty();
        let live = scripted && !voice;
        for node in inserts.children().filter(|n| n.is_element()) {
            let slot = out.len();
            let id = node.id().get_usize();
            let bypassed = number(node, "Bypass", 0.0)? != 0.0;
            let mix = scripted && engine_parameters::bypassable(node);
            if bypassed && !mix {
                placed.push((id, slot, 0));
                continue;
            }
            if mix {
                out.push(ir::Processor::Mix {
                    count: 0,
                    address: ir::SlotAddress {
                        group: -1,
                        slot: -1,
                        generic: i32::try_from(id).unwrap_or(-1),
                    },
                    dry: 0.0,
                    wet: 1.0,
                    bypass: bypassed,
                });
            }
            let first = out.len();
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
                "DigitalEq" => self.digital_eq(node, &at, live, &mut out)?,
                "ThreeBandShelves" => {
                    let middle = number(node, "GainMid", 0.0)?;
                    out.push(ir::Processor::Gain(db(middle)));
                    for (name, frequency, default, low) in [
                        ("GainLow", "FreqLowMid", 200.0, true),
                        ("GainHigh", "FreqMidHigh", 4000.0, false),
                    ] {
                        let gain = db(number(node, name, 0.0)? - middle);
                        out.push(ir::Processor::Filter(ir::Filter {
                            kind: if low {
                                ir::FilterKind::FirstOrderLowShelf { gain }
                            } else {
                                ir::FilterKind::FirstOrderHighShelf { gain }
                            },
                            cutoff: ir::Frequency::Hertz(number(node, frequency, default)?),
                            resonance: ir::Resonance::Q(std::f64::consts::FRAC_1_SQRT_2),
                        }));
                    }
                }
                "WaveShaper" => {
                    // DSP_FORMAT_SPECIFICATION "WaveShaper rectifier kernels":
                    // internal modes 6 (full) and 7 (half). Public numbering
                    // through the saved Mode is assumed equal to them.
                    // ponytail: unverified mapping; other modes, the pre/post
                    // filters, Amount, Knee and oversampling are not modelled.
                    let mode = number(node, "Mode", 0.0)?;
                    let rectifier = match mode {
                        6.0 => ir::Rectifier::Full,
                        7.0 => ir::Rectifier::Half,
                        _ => {
                            self.unsupported(&at, "WaveShaper mode", mode);
                            out.truncate(slot);
                            placed.push((id, slot, 0));
                            continue;
                        }
                    };
                    for (name, why, off) in [
                        ("Mix", "WaveShaper mix", 1.0),
                        ("PreFreq", "WaveShaper pre filter", 20000.0),
                        ("PostFreq", "WaveShaper post filter", 20.0),
                    ] {
                        let v = number(node, name, off)?;
                        if v != off {
                            self.unsupported(&at, why, v);
                        }
                    }
                    out.push(ir::Processor::Gain(db(number(node, "InputGain", 0.0)?)));
                    out.push(ir::Processor::Rectify(rectifier));
                    out.push(ir::Processor::Gain(db(number(node, "OutputGain", 0.0)?)));
                }
                "CompExp" => {
                    // Falcon manual, Compressor Expander: threshold dB, ratio,
                    // attack/release ms, manual makeup dB. The compressor law is
                    // the shared textbook one (ir::Compressor); UVI's own is not
                    // recovered, and the module has no stereo-link control.
                    for (name, why, off) in [
                        ("AutoMakeUp", "CompExp auto makeup", 0.0),
                        ("GateRatio", "CompExp gate (expander)", 1.0),
                    ] {
                        let v = number(node, name, off)?;
                        if v != off
                            && (name != "GateRatio"
                                || number(node, "GateThreshold", -130.0)? > -130.0)
                        {
                            self.unsupported(&at, why, v);
                        }
                    }
                    let mix = number(node, "Mix", 1.0)?;
                    if mix != 1.0 {
                        self.unsupported(&at, "CompExp mix", mix);
                    }
                    out.push(ir::Processor::Compressor(ir::Compressor {
                        threshold_db: number(node, "CompThreshold", 0.0)?,
                        ratio: number(node, "CompRatio", 10.0)?.max(1.0),
                        attack: ir::Time::Milliseconds(number(node, "CompAttack", 10.0)?.max(0.0)),
                        release: ir::Time::Milliseconds(
                            number(node, "CompRelease", 100.0)?.max(0.0),
                        ),
                        makeup: db(number(node, "MakeUpGain", 0.0)?),
                        link: true,
                    }));
                }
                "OnePole" => {
                    let tracking = number(node, "KeyTracking", 0.0)?;
                    let key = key.map_or(60.0, f64::from);
                    let hz = (number(node, "Freq", 1000.0)?
                        * 2f64.powf(tracking * (key - 60.0) / 12.0))
                    .clamp(20.0, 20000.0);
                    if tracking != 0.0 {
                        self.unsupported(
                            &at,
                            "OnePole key tracking (keygroup middle key)",
                            tracking,
                        );
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
                    // No file (or an unreadable one) leaves a unit impulse: the
                    // processor stays, so a script can swap an impulse in.
                    let asset = node
                        .attribute("SamplePath")
                        .filter(|p| !p.is_empty())
                        .and_then(|sample| self.asset(&at, sample));
                    self.ir.impulses.push(ir::Impulse {
                        rate: 48000,
                        left: vec![1.0],
                        right: vec![1.0],
                        asset,
                    });
                    let mut shape = crate::reverb::neutral();
                    shape.normalize = number(node, "NormalizePower", 1.0)? != 0.0;
                    if node.has_tag_name("SampledReverb") {
                        shape.sampled_reverb = true;
                        shape.time = number(node, "Time", 1.0)?;
                        shape.damping_low = number(node, "DampingLow", 0.0)?;
                        shape.damping_high = number(node, "DampingHigh", 0.0)?;
                        shape.pre_delay_ms = number(node, "PreDelay", 50.0)?;
                        shape.width = number(node, "Width", 0.0)?;
                    }
                    self.ir.impulse_shapes.resize(self.ir.impulses.len() - 1, None);
                    self.ir.impulse_shapes.push(Some(shape));
                    if shape.damping_low > 0.0 || shape.damping_high > 0.0 {
                        self.ir.unsupported.push(ir::Unsupported {
                            location: at.clone(),
                            feature: "positive SampledReverb damping".into(),
                            value: String::new(),
                            reason: ir::Reason::UnknownLaw,
                        });
                    }
                    out.push(ir::Processor::Convolution {
                        impulse: ir::ImpulseRef(self.ir.impulses.len() - 1),
                        dry: number(node, "Dry", 0.0)?,
                        wet: number(node, "Wet", 1.0)?,
                    });
                }
                "TrackDelay" => {
                    let time = number(node, "DelayTime", 0.0)?;
                    if number(node, "SyncToHost", 0.0)? != 0.0 {
                        self.unsupported(&at, "TrackDelay host sync", time);
                    }
                    if time > 0.0 {
                        out.push(ir::Processor::Delay {
                            time: ir::Time::Seconds(time),
                            feedback: 0.0,
                            mix: 1.0,
                        });
                    }
                }
                "EffectRack" => {
                    // Chains are parallel branches summed at their own gains
                    // (one is just a serial section at that gain). A scripted
                    // bus keeps every chain, bypassed or silent, because its
                    // gain stage is a control the script can raise; a chain's
                    // bypass is that gain at zero.
                    let chains: Vec<_> = node
                        .children()
                        .filter(|n| n.has_tag_name("Chains"))
                        .flat_map(|c| c.children().filter(|c| c.has_tag_name("AuxEffect")))
                        .filter(|c| {
                            live || (number(*c, "Bypass", 0.0).is_ok_and(|b| b == 0.0)
                                && number(*c, "Gain", 1.0).is_ok_and(|g| g != 0.0))
                        })
                        .collect();
                    // A rack sums its active branches. Removing every silent
                    // branch must leave zero, rather than the unprocessed input.
                    if chains.is_empty() {
                        out.push(ir::Processor::Gain(ir::Gain::Linear(0.0)));
                    }
                    for (n, chain) in chains.iter().enumerate() {
                        let off = live && number(*chain, "Bypass", 0.0)? != 0.0;
                        let gain = ir::Gain::Linear(if off {
                            0.0
                        } else {
                            number(*chain, "Gain", 1.0)?
                        });
                        let (inner, entries) = self.inserts(*chain, voice, key)?;
                        if let [_] = chains[..] {
                            out.push(ir::Processor::Gain(gain));
                        } else if live {
                            out.push(ir::Processor::Branch {
                                count: (inner.len() + 1) as u16,
                                gain: ir::Gain::UNITY,
                                first: n == 0,
                                last: n + 1 == chains.len(),
                            });
                            out.push(ir::Processor::Gain(gain));
                        } else {
                            out.push(ir::Processor::Branch {
                                count: inner.len() as u16,
                                gain,
                                first: n == 0,
                                last: n + 1 == chains.len(),
                            });
                        }
                        if live {
                            placed.push((chain.id().get_usize(), out.len() - 1, 1));
                        }
                        let base = out.len();
                        placed.extend(entries.into_iter().map(|(n, f, c)| (n, base + f, c)));
                        out.extend(inner);
                    }
                }
                _ => {
                    out.truncate(slot);
                    placed.push((id, slot, 0));
                    continue;
                }
            }
            let count = out.len() - first;
            if count == 0 && mix {
                out.truncate(slot);
                placed.push((id, slot, 0));
            } else {
                if let (true, Some(ir::Processor::Mix { count: c, .. })) = (mix, out.get_mut(slot))
                {
                    *c = count as u16;
                }
                placed.push((id, first, count));
            }
            self.used.push(node.id());
        }
        Ok((out, placed))
    }

    fn digital_eq(
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
            if get("Enabled", 0.0)? == 0.0 && !live {
                continue;
            }
            let gain = db(get("Gain", 0.0)? * scale);
            let shape = get("Type", 6.0)? as i64;
            // Installed libraries only show types 5 (gain, high-shelf-like
            // frequencies) and 6 (peak); 0 to 4 follow the manual's order.
            unproven |= (0..=4).contains(&shape);
            let kind = match shape {
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
            if flat && !live {
                continue;
            }
            out.push(ir::Processor::Filter(ir::Filter {
                kind,
                cutoff: ir::Frequency::Hertz(get("Freq", 1000.0)?),
                resonance: ir::Resonance::Q(engine_parameters::band_q(
                    |n| node.attribute(n).and_then(|v| v.parse().ok()),
                    band,
                )),
            }));
        }
        let overall = number(node, "OverallGain", 0.0)?;
        if overall != 0.0 || live {
            out.push(ir::Processor::Gain(db(overall)));
        }
        // The manual lists the seven shapes but not their numbering; types 5
        // and 6 are evidenced by the installed banks, 0 to 4 are its order.
        if unproven {
            self.unsupported(
                at,
                "DigitalEq band type numbering (manual order assumed)",
                "",
            );
        }
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
                if let Some(Some(shape)) = instrument.impulse_shapes.get(index) {
                    let (left, right) = crate::reverb::shape(rate, &impulse.left, &impulse.right, shape);
                    (impulse.left, impulse.right) = (left, right);
                }
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
