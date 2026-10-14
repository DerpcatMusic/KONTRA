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
                "ThreeBandShelves" => self.three_band_shelves(node, &mut out)?,
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
                    out.push(ir::Processor::Color(ir::fx::color::Config::rectifier(rectifier)));
                    out.push(ir::Processor::Gain(db(number(node, "OutputGain", 0.0)?)));
                }
                "BitCrusher" => {
                    // Kernel 0x1412d1770; the public fields are the kernel's
                    // members unchanged (ctor defaults 22050, 16, 0, 1; setters
                    // 0x141041710.., Cutoff 0x1412d1730 -> tilt filter).
                    let hertz = number(node, "EffectiveSampleRate", 22050.0)?;
                    let bits = number(node, "BitSize", 16.0)?;
                    let drive = number(node, "Drive", 0.0)?;
                    let cutoff = number(node, "Cutoff", 0.0)?;
                    let mix = number(node, "Mix", 1.0)?;
                    if !(hertz >= 1.0
                        && (1.0..=32.0).contains(&bits)
                        && (0.0..=1.0).contains(&drive)
                        && (-1.0..=1.0).contains(&cutoff)
                        && (0.0..=1.0).contains(&mix))
                    {
                        self.unsupported(&at, "BitCrusher value outside its public range", "");
                        out.truncate(slot);
                        placed.push((id, slot, 0));
                        continue;
                    }
                    out.push(ir::Processor::Color(ir::fx::color::Config::bit_crusher(
                        hertz,
                        bits,
                        drive,
                        cutoff as f32,
                        mix,
                    )));
                }
                "FormantCrusher" => {
                    out.push(ir::Processor::Color(ir::fx::color::Config::formant_crusher(
                        number(node, "Morph", 0.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "Q", 10.0)?.clamp(10.0, 20.0) as f32,
                        number(node, "Formant", 0.0)?.clamp(-1.0, 1.0) as f32,
                        number(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "Bite", 0.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "FilterA", 0.0)?.clamp(0.0, 10.0) as u8,
                        number(node, "FilterB", 2.0)?.clamp(0.0, 10.0) as u8,
                    )));
                }
                "DiodeClipper" => {
                    use ir::fx::color::{Config, Curve, Diode};
                    let diode = Diode {
                        drive: number(node, "Drive", 0.0)?.clamp(0.0, 30.0) as f32,
                        tone: number(node, "Tone", 20_000.0)?.clamp(100.0, 20_000.0) as f32,
                        high_pass: number(node, "HighPass", 1.0)?.clamp(1.0, 20_000.0) as f32,
                        asymmetry: number(node, "Asymmetry", 0.0)?.clamp(0.0, 1.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Diode(diode), ..Config::IDENTITY }));
                }
                "ToneStack" => {
                    use ir::fx::color::{Config, Curve, ToneStack};
                    let unit = |name, default| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(0.0, 1.0) as f32) };
                    let stack = ToneStack {
                        bass: unit("Bass", 0.5)?,
                        mid: unit("Mid", 0.5)?,
                        treble: unit("Treble", 0.5)?,
                        gain: unit("Gain", 0.3)?,
                        model: number(node, "Model", 0.0)?.clamp(0.0, 25.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::ToneStack(stack), ..Config::IDENTITY }));
                }
                "Exciter" => {
                    use ir::fx::color::{Config, Curve, Exciter};
                    let exciter = Exciter {
                        mode: number(node, "Mode", 1.0)?.clamp(0.0, 1.0) as u8,
                        input_gain: number(node, "InputGain", 0.0)?.clamp(-30.0, 20.0) as f32,
                        amount: number(node, "Amount", 0.0)?.clamp(0.0, 15.0) as f32,
                        mix: number(node, "Mix", 100.0)?.clamp(0.0, 100.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                        oversampling: number(node, "Oversampling", 0.0)?.clamp(0.0, 4.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Exciter(exciter), ..Config::IDENTITY }));
                }
                "Widener" => {
                    use ir::fx::color::{Config, Widener};
                    let widener = Widener {
                        delay: number(node, "Delay", 0.0)?.clamp(-15.0, 15.0) as f32,
                        width: number(node, "Width", 0.0)?.clamp(-1.0, 1.0) as f32,
                        shelf_hertz: number(node, "HighShelfFreqSide", 10_000.0)?.clamp(2000.0, 20_000.0) as f32,
                        shelf_db: number(node, "HighSelfGainSide", 0.0)?.clamp(-15.0, 15.0) as f32,
                    };
                    out.push(ir::Processor::Color(Config { widener: Some(widener), ..Config::IDENTITY }));
                }
                "GarageFuzz" | "SmoothOverdrive" | "StraightFuzz" => {
                    // Native drive engines (color chain Curve::Drive with the HIIR stage).
                    use ir::fx::color::{Config, Curve, Drive, DriveKind};
                    let drive = Drive {
                        kind: match node.tag_name().name() {
                            "GarageFuzz" => DriveKind::Garage,
                            "SmoothOverdrive" => DriveKind::Smooth,
                            _ => DriveKind::Straight,
                        },
                        input_gain: number(node, "InputGain", 0.0)?.clamp(-30.0, 20.0) as f32,
                        overdrive: number(node, "Overdrive", 0.0)?.clamp(0.0, 10.0) as f32,
                        trash: number(node, "Trash", 0.0)?.clamp(0.0, 1.0) as f32,
                        mix: number(node, "Mix", 100.0)?.clamp(0.0, 100.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                        oversampling: number(node, "Oversampling", 0.0)?.clamp(0.0, 4.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Drive(drive), ..Config::IDENTITY }));
                }
                k if crate::fx::dynamics::handles(k) => {
                    crate::fx::dynamics::translate(self, node, &at, &mut out)?;
                }
                tag if super::fx::filter::handles(tag) => {
                    match super::fx::filter::translate(node, key, |name, default| {
                        number(node, name, default)
                    })? {
                        Ok(done) => {
                            out.extend(done.processors);
                            for (feature, value) in done.unsupported {
                                self.unsupported(&at, feature, value);
                            }
                        }
                        Err(gap) => {
                            self.unsupported(&at, &format!("{} ({})", gap.name, gap.reason), "");
                            out.truncate(slot);
                            placed.push((id, slot, 0));
                            continue;
                        }
                    }
                }
                "Convolver" | "SampledReverb" if !voice => {
                    let processor = self.impulse_player(node, &at)?;
                    out.push(processor);
                }
                "PlainReverb" | "GateReverb" | "FilteredReverb" | "DelayedReverb" | "SparkVerb" | "Bloom" | "Diffusion"
                    if !voice =>
                {
                    out.extend(self.algorithmic_reverb(node, &at)?);
                }
                "TrackDelay" | "FxDelay" | "FatDelay" | "SimpleDelay" | "StereoDelay" | "PingPongDelay" | "DualDelay"
                | "DualDelayX" | "TapeEcho" | "Chorus" | "Flanger" | "AnalogChorus" | "AnalogFlanger"
                | "WhiteChorus" | "Ensemble" | "Phaser" | "CrossPhaser" | "Phasor" | "PhasorFilter" => {
                    if let Some(config) = self.timemod(node, &at)? {
                        out.push(ir::Processor::TimeMod(config));
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
                other => {
                    if let Some(why) = crate::fx::color::unproved(other) {
                        self.unsupported(&at, other, why);
                    }
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
            self.used.insert(node.id());
        }
        Ok((out, placed))
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
                    let (left, right) = sampler_core::shape_impulse(rate, &impulse.left, &impulse.right, shape);
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
