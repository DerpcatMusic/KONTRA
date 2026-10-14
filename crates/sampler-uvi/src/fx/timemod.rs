//! UVI delay, chorus, flanger, phaser and ensemble inserts onto
//! `sampler_ir::fx::timemod`.
//!
//! TrackDelay, DualDelay(X) and the legacy Simple/Stereo/PingPong delays run
//! their native laws. The others' laws are
//! unproved, so each of those mappings is a documented-parameter, generic
//! topology. Each one reports `UnknownLaw` for
//! the effect and `NotModeled` for each control it cannot honour, so the
//! inspector shows "generic", never "exact".
use crate::{Translation, number};
use roxmltree::Node;
use sampler_ir::{
    self as ir,
    fx::timemod::{
        Allpass, Config, DelayTime, DualDelay, Interpolation, Lfo, LfoShape, Line, Native, Order, Rate, SweepLaw, UviBuzzDelay, UviDiffuseDelay, UviLegacyDelay, UviTapeEcho, UviVelvetDelay, UviLegacyKind,
    },
};

const SINE: LfoShape = LfoShape::Sine;

fn lfo(hz: f64) -> Lfo {
    Lfo { shape: SINE, rate: Rate::Hz(hz.clamp(0., 200.) as f32), phase: 0.25 }
}

fn mod_config(line: Line, speed: f64, mix: f64) -> Config {
    // Chorus / flanger family: the unprocessed signal always passes.
    Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(speed), dry: 1., wet: mix }
}

fn delay_config(line: Line, mix: f64) -> Config {
    Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(0.), dry: 1. - mix, wet: mix }
}

fn feedback(line: &mut Line, fb: f64) {
    let fb = fb.clamp(0., 0.99);
    line.feedback.matrix = [[fb, 0.], [0., fb]];
}

/// UVI TrackDelay (ctor 0x141339b70, process 0x14133a2e0): a pure delay of
/// `ceil(seconds·rate)` frames replacing the input, at most 5 s; synced, the
/// time is `60/tempo·beats` (0x1411c8250). `None` at zero delay.
fn track_delay(time: f64, synced: bool) -> Option<Config> {
    if !(time > 0.) {
        return None;
    }
    let time = if synced { DelayTime::Beats(time) } else { DelayTime::Ms(time.min(5.) * 1000.) };
    let mut line = Line::single(time, 5000.);
    line.interpolation = Interpolation::Ceil;
    Some(Config { dry: 0., wet: 1., ..delay_config(line, 1.) })
}

impl Translation {
    /// DualDelay / DualDelayX onto the native kernel (`sampler_core`
    /// `dsp/timemod/dual.rs`). DualDelayX's extra stages are reported when on.
    fn dual_delay(&mut self, node: Node, at: &str, tag: &str, synced: bool) -> Result<Config, String> {
        let n = |name: &str, default: f64| -> Result<f32, String> { Ok(number(node, name, default)? as f32) };
        let x = tag == "DualDelayX";
        let modulation = !x || number(node, "Modulation", 1.)? != 0.;
        let filters = !x || number(node, "Filtering", 1.)? != 0.;
        let peak = number(node, "PeakGain", 0.)?;
        if filters && peak != 0. {
            self.unsupported(at, &format!("{tag} PeakGain (peaking bandwidth law 0x141645d00 unproved)"), peak);
        }
        if number(node, "Interpolation", 1.)? > 2. {
            self.unsupported(at, &format!("{tag} Interpolation"), number(node, "Interpolation", 1.)?);
        }
        if x {
            for (name, why) in [
                ("Crusher", "crusher table 0x141f8f908 and quantizer unproved"),
                ("Dispersion", "dispersion allpass 0x14164ccd0 unproved"),
                ("Diffusion", "diffusion network 0x14165b160 unproved"),
                ("TapeSaturation", "tape curve 0x1419f245c unproved"),
                ("Reflection", "reflected rotation 0x14165df70 unproved"),
            ] {
                if number(node, name, 0.)? != 0. {
                    self.unsupported(at, &format!("{tag} {name} ({why})"), 1);
                }
            }
            if number(node, "DuckerBypass", 1.)? == 0. {
                self.unsupported(at, &format!("{tag} Ducker (envelope 0x14165e350 unproved)"), 1);
            }
        }
        let d = DualDelay {
            time: n("DelayTime", 0.125)?.clamp(0., 5.),
            synced,
            delay_ratio: n("DelayRatio", 0.)?.clamp(-1., 1.),
            feedback: n("Feedback", 0.3)?.clamp(0., 1.),
            feedback_ratio: n("FeedbackRatio", 0.)?.clamp(-1., 1.),
            filters,
            low_cut_hz: n("LowCut", 20.)?.max(1.),
            high_cut_hz: n("HighCut", 20_000.)?.max(1.),
            rotation: n("Rotation", 0.)?,
            input: [n("InputWidth", 1.)?, n("InputRotation", 0.)?],
            output: [n("OutputWidth", 1.)?, n("OutputRotation", 0.)?],
            mod_cents: if modulation { n("ModDepth", 0.)?.max(0.) } else { 0. },
            mod_hz: n("ModRate", 1.)?.max(0.01),
            mod_offset: n("ModChannelOffset", 1.)?,
        };
        let mut line = Line::single(DelayTime::Ms(0.), 5000.);
        line.native = Native::UviDualDelay(d);
        line.interpolation = match number(node, "Interpolation", 1.)? as i64 {
            0 => Interpolation::Floor,
            1 => Interpolation::Linear,
            _ => Interpolation::Lagrange,
        };
        // Equal-power mix: sqrt(1 - Mix) dry, sqrt(Mix) wet.
        let mix = number(node, "Mix", 0.5)?.clamp(0., 1.);
        Ok(Config { dry: (1. - mix).sqrt(), wet: mix.sqrt(), ..delay_config(line, mix) })
    }

    fn generic_law(&mut self, at: &str, feature: &str, value: impl std::fmt::Display) {
        self.ir.unsupported.push(ir::Unsupported {
            location: at.into(),
            feature: format!("{feature}: generic time/modulation law"),
            value: value.to_string(),
            reason: ir::Reason::UnknownLaw,
        });
    }

    /// Controls the generic topology does not honour, reported when set away
    /// from their neutral value.
    fn not_modeled(&mut self, at: &str, node: Node, tag: &str, controls: &[(&str, f64)]) -> Result<(), String> {
        for (name, neutral) in controls {
            let value = number(node, name, *neutral)?;
            if (value - neutral).abs() > 1e-9 {
                self.unsupported(at, &format!("{tag} {name}"), value);
            }
        }
        Ok(())
    }

    /// Translate one UVI time/modulation insert; `None` when its controls
    /// cannot form a valid configuration.
    pub(crate) fn timemod(&mut self, node: Node, at: &str) -> Result<Option<Config>, String> {
        let tag = node.tag_name().name();
        let mix = number(node, "Mix", 1.)?.clamp(0., 1.);
        let synced = number(node, "SyncToHost", 0.)? != 0.;
        if tag == "TrackDelay" {
            return Ok(track_delay(number(node, "DelayTime", 0.)?, synced));
        }
        if tag == "DualDelay" || tag == "DualDelayX" {
            return self.dual_delay(node, at, tag, synced).map(Some);
        }
        if tag == "TapeEcho" {
            // Native laws: `sampler_core` `dsp/timemod/tape.rs`.
            let n = |name, default: f64, lo: f64, hi: f64| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(lo, hi) as f32) };
            let mode = number(node, "Mode", 1.)?.clamp(1., 12.) as u8;
            let hi = if synced { 128. } else { 0.5 };
            let mut line = Line::single(DelayTime::Ms(0.), 1000.);
            line.native = Native::UviTapeEcho(UviTapeEcho {
                speed: n("Speed", 2., 1., 3.)?,
                feedback: n("Feedback", 0.5, 0., 2.)?,
                mode,
                bass_db: n("Bass", 0., -10., 10.)?,
                treble_db: n("Treble", 0., -10., 10.)?,
                delays: [n("Delay1", 0.165, 0.05, hi)?, n("Delay2", 0.324, 0.05, hi)?, n("Delay3", 0.487, 0.05, hi)?],
                synced,
                noise: n("NoiseLevel", 0.0001, 0., 0.001)?,
                tape_stop: number(node, "TapeStop", 0.)? != 0.,
            });
            if mode >= 5 {
                self.unsupported(at, "TapeEcho reverb (modes 5-12: an impulse response embedded in the UVI binary, not shipped)", mode);
            }
            let mix = number(node, "Mix", 0.5)?.clamp(0., 1.);
            return Ok(Some(Config { dry: (1. - mix).sqrt(), wet: mix.sqrt(), ..delay_config(line, mix) }));
        }
        if tag == "DiffuseDelayNested" {
            // Native laws: `sampler_core` `dsp/timemod/velvet.rs`.
            let n = |name, default: f64, lo: f64, hi: f64| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(lo, hi) as f32) };
            let mut line = Line::single(DelayTime::Ms(0.), 1000.);
            line.native = Native::UviVelvetDelay(UviVelvetDelay {
                time: n("DelayTime", 0.05, 0.001, if synced { 128. } else { 1. })?,
                synced,
                spread: n("TimeSpread", 1., 0.1, 1.)?,
                diffusion: n("Diffusion", 0.1, 0., 0.9)?,
                seed: number(node, "Seed", 0.)?.clamp(0., 100.) as u32,
                shape: number(node, "Shape", 0.)?.clamp(0., 6.) as u8,
                curve: n("ShapeCurve", 0., -1., 1.)?,
            });
            return Ok(Some(delay_config(line, mix)));
        }
        if tag == "Buzz" {
            // Native laws: `sampler_core` `dsp/timemod/buzz.rs`.
            let n = |name, default: f64, lo: f64, hi: f64| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(lo, hi) as f32) };
            let ms = n("Delay", 200., if synced { 0. } else { 40. }, if synced { 128. } else { 2000. })?;
            let mut line = Line::single(DelayTime::Ms(0.), 2000.);
            line.native = Native::UviBuzzDelay(UviBuzzDelay {
                time: if synced { ms } else { ms * 0.001 },
                synced,
                feedback: n("Feedback", 0.2, 0., 1.)?,
                trash: n("Trash", 0.25, 0., 1.)?,
                spread: n("Spread", 1., 0., 2.)?,
                glide: n("InterpolationTime", 0.1, 0.01, 1.)?,
                mode: number(node, "DistortionMode", 0.)?.clamp(0., 2.) as u8,
            });
            let (dry, wet) = (number(node, "Dry", 0.5)?.clamp(0., 1.), number(node, "Wet", 0.5)?.clamp(0., 1.));
            return Ok(Some(Config { dry, wet, ..delay_config(line, wet) }));
        }
        if tag == "DiffuseDelay" {
            // Native laws: `sampler_core` `dsp/timemod/diffuse.rs`.
            let n = |name, default: f64, lo: f64, hi: f64| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(lo, hi) as f32) };
            let mut line = Line::single(DelayTime::Ms(0.), 1000.);
            line.native = Native::UviDiffuseDelay(UviDiffuseDelay {
                time: n("DelayTime", 0.125, 0.01, if synced { 128. } else { 1. })?,
                synced,
                spread: n("TimeSpread", 1., 0., 1.)?,
                diffusion: n("Diffusion", 0.5, 0., 1.)?,
                feedback: n("Feedback", 0., 0., 1.)?,
                damp: n("Damp", 0., 0., 1.)?,
                crosstalk: n("CrossTalk", 0., -1., 1.)?,
                randomness: n("Randomness", 0., 0., 0.5)?,
                mod_rate: n("ModRate", 1., 0.1, 10.)?,
                mod_depth: n("ModDepth", 0., 0., 20.)?,
            });
            let mix = number(node, "Mix", 0.5)?.clamp(0., 1.);
            return Ok(Some(Config { dry: (1. - mix).sqrt(), wet: mix.sqrt(), ..delay_config(line, mix) }));
        }
        if let Some(kind) = match tag {
            "SimpleDelay" => Some(UviLegacyKind::Simple),
            "StereoDelay" => Some(UviLegacyKind::Stereo),
            "PingPongDelay" => Some(UviLegacyKind::PingPong),
            _ => None,
        } {
            // Native laws: `sampler_core` `dsp/timemod/uvi_legacy.rs`.
            let time = |name, default| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(0., if synced { 128. } else { 10. }) as f32) };
            let time = if kind == UviLegacyKind::Stereo { [time("LeftDelayTime", 0.8)?, time("RightDelayTime", 0.1)?] } else { [time("DelayTime", 0.1)?; 2] };
            let mut line = Line::single(DelayTime::Ms(0.), 10_000.);
            line.native = Native::UviLegacyDelay(UviLegacyDelay {
                kind,
                time,
                synced,
                feedback: number(node, "Feedback", 0.)?.clamp(0., 1.) as f32,
                cutoff: number(node, "Cutoff", 0.)?.clamp(-1., 1.) as f32,
                spread: number(node, "Spread", -1.)?.clamp(-1., 1.) as f32,
            });
            if kind != UviLegacyKind::PingPong {
                self.not_modeled(at, node, tag, &[("Spread", -1.)])?;
            }
            self.not_modeled(at, node, tag, &[("Diffusion", 1.)])?;
            return Ok(Some(delay_config(line, mix)));
        }
        if synced {
            self.unsupported(at, &format!("{tag} host sync"), "free time used");
        }
        let config = match tag {
            "FxDelay" | "FatDelay" => {
                // FxDelay 0x141249980 (in-loop drive, auto-pan) and FatDelay
                // 0x141242d10 (four lines at d, (1+D)d, (2-D)d, 2d): their
                // drive and mixing laws are unproved, so one generic line.
                let t = number(node, "DelayTime", 0.1)?.clamp(0.001, 10.);
                let mut line = Line::single(DelayTime::Ms(t * 1000.), t * 1000.);
                feedback(&mut line, number(node, "Feedback", 0.)?);
                self.not_modeled(at, node, tag, &[("Cutoff", 0.), ("Diffusion", 1.)])?;
                if tag == "FxDelay" {
                    self.not_modeled(at, node, tag, &[("BypassDrive", 0.)])?;
                }
                delay_config(line, mix)
            }
            "Chorus" | "Flanger" | "AnalogChorus" | "AnalogFlanger" | "WhiteChorus" | "Ensemble" => {
                let (base, depth_ms, speed, voices, fb_amount, mix) = match tag {
                    "Chorus" => (number(node, "DelayTime", 0.01)?.clamp(0., 0.02) * 1000., number(node, "Depth", 0.38)? * 8., number(node, "Speed", 0.8)?, 1, 0., mix),
                    "Flanger" => (number(node, "DelayTime", 0.2)? * 10., number(node, "Depth", 0.08)? * 20., number(node, "Speed", 0.8)?, 1, number(node, "Feedback", 0.)?, mix),
                    "AnalogChorus" => (8., number(node, "Width", 4.)? / 6. * 3., number(node, "Speed", 2.)?, 1, 0., 0.5),
                    "AnalogFlanger" => (2., number(node, "Width", 4.)? / 6. * 1.5, number(node, "Speed", 2.)?, 1, number(node, "Intensity", 3.)? / 6. * 0.8, 0.5),
                    "WhiteChorus" => (12., number(node, "Depth", 5.)?.clamp(1., 40.) * 0.25, number(node, "Speed", 0.2)?, number(node, "NumVoices", 4.)? as u8, 0., mix),
                    _ => (10., 2.5, 0.6, 3, 0., mix),
                };
                let mut line = Line::single(DelayTime::Ms(base.max(0.5)), 45.);
                line.taps[0].depth_ms = depth_ms.clamp(0., 20.);
                line.interpolation = Interpolation::Hermite;
                line.voices = voices.clamp(1, 8);
                line.voice_phase = if voices > 1 { 1. } else { 0. };
                line.voice_spread = if voices > 1 { 0.8 } else { 0. };
                feedback(&mut line, fb_amount);
                if tag == "AnalogChorus" || tag == "AnalogFlanger" {
                    self.not_modeled(at, node, tag, &[("TransientEnhancement", 2.)])?;
                }
                if tag == "WhiteChorus" {
                    self.not_modeled(at, node, tag, &[("Crossover", 20.), ("Edge", 0.), ("LowGain", 1.), ("Tone", 22050.), ("Trim", 0.), ("Mode", 0.)])?;
                }
                if tag == "Ensemble" {
                    self.not_modeled(at, node, tag, &[("Mode", 0.), ("Tone", 6000.)])?;
                }
                mod_config(line, speed, mix)
            }
            "Phaser" | "CrossPhaser" | "Phasor" | "PhasorFilter" => {
                let depth = number(node, "Depth", 0.8)?.clamp(0., 1.);
                let stages = match tag {
                    "Phasor" | "PhasorFilter" => number(node, "Order", 3.)?.clamp(1., 12.) as u8,
                    _ => 4,
                };
                let (min_hz, max_hz) = match tag {
                    "PhasorFilter" => {
                        let f = number(node, "Freq", 1000.)?.clamp(20., 20000.);
                        (f, f)
                    }
                    "Phasor" => (number(node, "MinFreq", 200.)?, number(node, "MaxFreq", 3000.)?),
                    _ => (number(node, "MinFreq", 400.)?, number(node, "MaxFreq", 800.)?),
                };
                let (min_hz, max_hz) = (min_hz.clamp(20., 20000.), max_hz.clamp(20., 20000.));
                self.not_modeled(at, node, tag, &[("Spread", if tag == "Phasor" { 1. } else { 0. }), ("KeyTracking", 0.), ("CutoffMode", 0.), ("LFOSHape", 0.)])?;
                Config {
                    line: None,
                    allpass: Some(Allpass {
                        stages,
                        law: SweepLaw::Sweep { min_hz: min_hz.min(max_hz) as f32, max_hz: max_hz.max(min_hz) as f32, depth: depth as f32 },
                        feedback: number(node, "Feedback", 0.7)?.clamp(-0.98, 0.98) as f32,
                    }),
                    order: Order::LineThenAllpass,
                    lfo: lfo(number(node, "Speed", 0.8)?),
                    // A phaser sums dry and allpass; the filter variant is the allpass alone.
                    dry: if tag == "PhasorFilter" { 0. } else { 0.5 },
                    wet: if tag == "PhasorFilter" { 1. } else { 0.5 },
                }
            }
            _ => return Ok(None),
        };
        match config.check() {
            Ok(()) => {
                self.generic_law(at, tag, "documented parameters only");
                Ok(Some(config))
            }
            Err(field) => {
                self.unsupported(at, &format!("{tag} {field}"), "out of range");
                Ok(None)
            }
        }
    }
}
