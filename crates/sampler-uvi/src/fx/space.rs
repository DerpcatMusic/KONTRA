//! UVI `space` effects onto sampler_ir::fx::space.
//!
//! The IR players (`Convolver`, `SampledReverb`) load a library impulse
//! response whose controls are applied by `sampler_core::shape_impulse`
//! (measured against the official processor by the frozen v1 engine).
//! `SparkVerb`, `Diffusion` and the legacy `PlainReverb` / `GateReverb` /
//! `FilteredReverb` / `DelayedReverb` node (`Topology::Jezar`) run their
//! native laws. `Bloom` maps onto the generic network and says so
//! (`Reason::UnknownLaw`). `IReverb`, `Opal`, `Simple Reverb` and
//! `Predelay Verb` have no known parameters and stay unmapped.

use super::super::{Translation, number};
use roxmltree::Node;
use sampler_ir as ir;
use sampler_ir::fx::space::{UviReverb, Config, DecayBands, Network, SparkVerb, Topology};

/// The first attribute of `names` present on `node`. The factory XML
/// prefixes some names with the element's own ("SampledReverbTime",
/// "ReverbDecayTime"); the scripting catalog does not.
fn pick(node: Node, names: &[&str], default: f64) -> Result<f64, String> {
    match names.iter().find(|n| node.attribute(**n).is_some()) {
        Some(name) => number(node, name, default),
        None => Ok(default),
    }
}

/// A tempo-synced time is stored as beats (4 is 1/1, 0.25 is 1/16). There is no
/// host tempo at translation time, so it is read at 120 BPM.
const SYNC_SECONDS_PER_BEAT: f64 = 0.5;

fn flag(node: Node, name: &str) -> Result<bool, String> {
    Ok(number(node, name, 0.0)? != 0.0)
}

/// Damping stored 0..1 (0 open, 1 dark) as a feedback lowpass: 18 kHz down to 900 Hz.
fn damping_hz(x: f64) -> f64 {
    18_000.0 * 0.05f64.powf(x.clamp(0.0, 1.0))
}

/// Delay-time modulation that gives `cents` of peak pitch deviation at `hz`.
fn modulation_seconds(cents: f64, hz: f64) -> f64 {
    ((2f64.powf(cents / 1200.0) - 1.0) / (std::f64::consts::TAU * hz)).clamp(0.0, 0.0015)
}

impl Translation {
    /// SparkVerb onto the native engine (`sampler_core` `dsp/space/sparkverb.rs`).
    fn spark_verb(&mut self, node: Node, at: &str, synced: bool) -> Result<ir::Processor, String> {
        let n = |name: &str, default: f64| number(node, name, default);
        let on = |name: &str, default: f64| -> Result<bool, String> { Ok(n(name, default)? != 0.0) };
        let predelay_ms = n("PreDelay", 0.0)?;
        if synced {
            self.unsupported(at, "SparkVerb PreDelay host sync", predelay_ms);
        }
        // Absent in Workstation 4.0.9, whose engine fixes them at these values.
        for (name, fixed) in [("UseOldInputMatrix", 1.0), ("UseOldOutputMatrix", 1.0), ("UseFilterInFeedback", 1.0), ("EarlyShape", 0.0)] {
            let v = n(name, fixed)?;
            if v != fixed {
                self.unsupported(at, &format!("SparkVerb {name}"), v);
            }
        }
        let (decay, room, low, high) = (n("DecayTime", 1.0)?, n("RoomSize", 20.0)?, n("DecayLow", 1.0)?, n("DecayHigh", 1.0)?);
        let (low_cut, high_cut, freeze) = (on("LowCut", 0.0)?, on("HiCut", 0.0)?, on("Freeze", 0.0)?);
        let spark = SparkVerb {
            quality: n("Quality", 3.0)?.clamp(1.0, 5.0) as u8,
            room_m: room as f32,
            shape: n("Shape", 0.0)? as f32,
            decay: decay as f32,
            decay_low: low as f32,
            decay_high: high as f32,
            freq_low: n("FreqLow", 250.0)? as f32,
            freq_high: n("FreqHigh", 12_000.0)? as f32,
            low_cut,
            high_cut,
            mod_cents: n("ModDepth", 4.0)? as f32,
            mod_hz: n("ModRate", 1.0)? as f32,
            mode: n("Mode", 1.0)?.clamp(0.0, 2.0) as u8,
            freeze,
            diffuser: on("DiffusionOnOff", 0.0)?.then_some((n("Diffusion", 0.618)? as f32, n("DiffusionStart", 5.0)? as f32)),
            width: n("Width", 1.0)? as f32,
            rolloff_hz: n("Rolloff", 20_000.0)? as f32,
            predelay_seconds: (predelay_ms * 0.001).clamp(0.0, 0.1) as f32,
        };
        // Tail (0x141321310): 2·RT60 of the slowest band plus the predelay.
        let slowest = 1f64.max(if low_cut { 1.0 } else { low }).max(if high_cut { 1.0 } else { high });
        let network = Network {
            topology: Topology::SparkVerb,
            decay_seconds: if freeze { 60.0 } else { (0.1 * room * decay * slowest).clamp(0.01, 60.0) },
            predelay_seconds: (predelay_ms * 0.001).clamp(0.0, 0.1),
            width: f64::from(spark.width).clamp(0.0, 1.0),
            spark: Some(spark),
            ..Network::DEFAULT
        };
        let mix = n("Mix", 0.5)?.clamp(0.0, 1.0);
        let (dry, wet) = if n("MixMode", 1.0)? != 0.0 { ((1.0 - mix).sqrt(), mix.sqrt()) } else { (1.0 - mix, mix) };
        Ok(ir::Processor::Space(Config { address: None, network: Some(network), impulse: None, dry, wet }))
    }

    /// `Convolver` and `SampledReverb`: an impulse response from the library.
    pub(crate) fn impulse_player(&mut self, node: Node, at: &str) -> Result<ir::Processor, String> {
        // No file (or an unreadable one) leaves a unit impulse: the
        // processor stays, so a script can swap an impulse in.
        let asset = node
            .attribute("SamplePath")
            .filter(|p| !p.is_empty())
            .and_then(|sample| self.asset(at, sample));
        self.ir.impulses.push(ir::Impulse { rate: 48000, left: vec![1.0], right: vec![1.0], asset });
        let sampled = node.has_tag_name("SampledReverb");
        let mut shape = sampler_core::neutral_impulse_shape();
        shape.normalize = number(node, "NormalizePower", 1.0)? != 0.0;
        // The factory XML stores a SampledReverb's mix in percent.
        let (mut dry, mut wet) = (number(node, "Dry", 0.0)?, number(node, "Wet", 1.0)?);
        if sampled {
            shape.sampled_reverb = true;
            shape.time = pick(node, &["SampledReverbTime", "Time"], 1.0)?;
            shape.damping_low = pick(node, &["SampledReverbLFDamping", "DampingLow"], 0.0)?;
            shape.damping_high = pick(node, &["SampledReverbHFDamping", "DampingHigh"], 0.0)?;
            shape.pre_delay_ms = pick(node, &["SampledReverbPreDelay", "PreDelay"], 50.0)?;
            shape.width = pick(node, &["SampledReverbWidth", "Width"], 0.0)?;
            if node.attribute("SampledReverbDry").is_some() {
                dry = number(node, "SampledReverbDry", 0.0)? / 100.0;
            }
            if node.attribute("SampledReverbWet").is_some() {
                wet = number(node, "SampledReverbWet", 100.0)? / 100.0;
            }
        }
        self.ir.impulse_shapes.resize(self.ir.impulses.len() - 1, None);
        self.ir.impulse_shapes.push(Some(shape));
        Ok(ir::Processor::Space(Config::convolution(
            ir::ImpulseRef(self.ir.impulses.len() - 1),
            dry,
            wet,
        )))
    }

    /// The algorithmic reverbs, onto a generic network. `None` for an
    /// element this module does not own or that cannot be modelled.
    pub(crate) fn algorithmic_reverb(&mut self, node: Node, at: &str) -> Result<Option<ir::Processor>, String> {
        let kind = node.tag_name().name();
        let generic = |this: &mut Self| {
            this.ir.unsupported.push(ir::Unsupported {
                location: at.into(),
                feature: format!("{kind}: generic network, algorithm and control laws unrecovered"),
                value: String::new(),
                reason: ir::Reason::UnknownLaw,
            });
        };
        let beats = flag(node, "SyncToHost")?;
        let time = |name: &[&str], default: f64| -> Result<f64, String> {
            Ok(pick(node, name, default)? * if beats { SYNC_SECONDS_PER_BEAT } else { 1.0 })
        };
        let mut network = Network::DEFAULT;
        let mix: f64;
        if kind == "SparkVerb" {
            return self.spark_verb(node, at, beats).map(Some);
        }
        match kind {
            "PlainReverb" | "GateReverb" | "FilteredReverb" | "DelayedReverb" => {
                // One node, 0x141312050 (defaults Mix .5, HighDamp 0, PreDelay
                // .05 s, GateTime .4 s, DecayTime 2); Plain has no predelay.
                network.topology = Topology::Jezar;
                let cutoff = if kind == "FilteredReverb" { pick(node, &["Cutoff", "ReverbFilter"], 0.0)? } else { 0.0 };
                network.uvi_reverb = Some(UviReverb {
                    decay_time: pick(node, &["ReverbDecayTime", "DecayTime"], 2.0)?.max(0.0) as f32,
                    high_damp: pick(node, &["ReverbHighDamp", "HighDamp"], 0.0)?.clamp(0.0, 1.0) as f32,
                    cutoff: cutoff.clamp(-1.0, 1.0) as f32,
                });
                // Tail length only: the longest comb (1640/44100 s) losing
                // -log10(fb) decades per pass, fb = 0.7 + 0.28·√(0.1·t).
                let fb = 0.7 + 0.28 * (0.1 * f64::from(network.uvi_reverb.unwrap().decay_time)).sqrt();
                network.decay_seconds = (3.0 * 1640.0 / 44_100.0 / -fb.min(0.999).log10()).clamp(0.01, 60.0);
                mix = pick(node, &["WetMix", "Mix"], 0.5)?;
                if kind != "PlainReverb" {
                    network.predelay_seconds = time(&["ReverbPreDelay", "PreDelay"], 0.05)?.clamp(0.0, 2.0);
                }
                if kind == "GateReverb" {
                    network.gate_seconds = Some(time(&["ReverbGateTime", "GateTime"], 0.4)?.clamp(0.001, 1.0));
                }
                if kind == "DelayedReverb" {
                    network.echo_feedback = pick(node, &["Feedback"], 0.0)?.clamp(0.0, 0.95);
                    if network.echo_feedback != 0.0 {
                        self.unsupported(at, "DelayedReverb Feedback (no feedback in the native predelay 0x1413165f0; generic echo)", network.echo_feedback);
                    }
                }
                for name in ["BuzzDelay", "BuzzFeedback"] {
                    if let Some(v) = node.attribute(name) {
                        self.unsupported(at, &format!("{kind} {name}"), v);
                    }
                }
                if beats {
                    self.ir.unsupported.push(ir::Unsupported {
                        location: at.into(),
                        feature: format!("{kind} host sync read as beats at 120 BPM"),
                        value: String::new(),
                        reason: ir::Reason::UnknownLaw,
                    });
                }
                return Ok(Some(ir::Processor::Space(Config { network: Some(network), impulse: None, dry: 1.0 - mix, wet: mix })));
            }
            "Bloom" => {
                network.topology = Topology::Fdn;
                network.decay_seconds = number(node, "DecayTime", 1.0)?.max(0.01);
                network.size = (number(node, "RoomSize", 20.0)? / 20.0).clamp(0.05, 1.5);
                network.predelay_seconds = time(&["PreDelay"], 0.0)? * if beats { 1.0 } else { 0.001 };
                network.width = number(node, "Width", 1.0)?.clamp(0.0, 1.0);
                network.diffusion = 0.75 * number(node, "Diffusion", 0.618)?.clamp(0.0, 1.0);
                let (low, high) = (number(node, "DecayLow", 1.0)?, number(node, "DecayHigh", 1.0)?);
                if low != 1.0 || high != 1.0 {
                    network.bands = Some(DecayBands {
                        low,
                        high,
                        low_hz: number(node, "FreqLow", 250.0)?,
                        high_hz: number(node, "FreqHigh", 12_000.0)?,
                    });
                }
                let rate = number(node, "ModRate", 1.0)?;
                network.modulation_hz = rate;
                network.modulation_seconds = modulation_seconds(number(node, "ModDepth", 4.0)?, rate);
                network.input_cutoff_hz = number(node, "Rolloff", 20_000.0)?;
                let mix = number(node, "Mix", 0.5)?;
                let gain = 10f64.powf(number(node, "ReverbGain", 0.0)? / 20.0);
                for (name, off) in [
                    ("Freeze", 0.0),
                    ("LowCut", 0.0),
                    ("Shape", 0.0),
                    ("EarlyShape", 0.0),
                    ("Rise", 0.5),
                    ("StructuralMode", 0.0),
                    ("TapoutMode", 1.0),
                    ("Mode", 1.0),
                ] {
                    let v = number(node, name, off)?;
                    if v != off {
                        self.unsupported(at, &format!("{kind} {name}"), v);
                    }
                }
                generic(self);
                return Ok(Some(ir::Processor::Space(Config {
                    address: None,
                    network: Some(network),
                    impulse: None,
                    dry: 1.0 - mix,
                    wet: mix * gain,
                })));
            }
            "Diffusion" => {
                // Constructor 0x14130fc60: Diffusion +0x38, DiffusionStart +0x3c,
                // Mix +0x40; equal-power mix 0x141311900.
                network.topology = Topology::Diffuser;
                network.diffusion_start_ms = number(node, "DiffusionStart", 5.0)?.clamp(0.1, 50.0);
                network.diffusion = number(node, "Diffusion", 0.618)?.clamp(0.0, 1.0);
                let mix = number(node, "Mix", 1.0)?.clamp(0.0, 1.0);
                return Ok(Some(ir::Processor::Space(Config { network: Some(network), impulse: None, dry: (1.0 - mix).sqrt(), wet: mix.sqrt() })));
            }
            _ => return Ok(None),
        }
        generic(self);
        Ok(Some(ir::Processor::Space(Config {
            address: None,
            network: Some(network),
            impulse: None,
            dry: 1.0 - mix,
            wet: mix,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modulation_depth_gives_the_asked_pitch_deviation() {
        let a = modulation_seconds(4.0, 1.0);
        let cents = 1200.0 * (1.0 + std::f64::consts::TAU * 1.0 * a).log2();
        assert!((cents - 4.0).abs() < 1e-9);
        assert_eq!(modulation_seconds(0.0, 1.0), 0.0);
    }

    #[test]
    fn damping_spans_open_to_dark() {
        assert_eq!(damping_hz(0.0), 18_000.0);
        assert!((damping_hz(1.0) - 900.0).abs() < 1e-9);
    }
}
