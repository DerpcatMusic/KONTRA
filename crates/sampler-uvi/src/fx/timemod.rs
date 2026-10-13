//! UVI delay, chorus, flanger, phaser and ensemble inserts onto
//! `sampler_ir::fx::timemod`.
//!
//! UVI's native laws for these modules are unproved (`uvi-dsp-laws.md` covers
//! LFO, OnePole, WaveShaper and the crushers only), so every mapping here is a
//! documented-parameter, generic topology. Each one reports `UnknownLaw` for
//! the effect and `NotModeled` for each control it cannot honour, so the
//! inspector shows "generic", never "exact".
use crate::{Translation, number};
use roxmltree::Node;
use sampler_ir::{
    self as ir,
    fx::timemod::{
        Allpass, Config, DelayTime, Interpolation, Lfo, LfoShape, Line, Order, Rate, SweepLaw, Tap,
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

impl Translation {
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
        if synced {
            self.unsupported(at, &format!("{tag} host sync"), "free time used");
        }
        let config = match tag {
            "FxDelay" | "FatDelay" | "SimpleDelay" | "StereoDelay" | "PingPongDelay" => {
                let (left, right) = if tag == "StereoDelay" {
                    (number(node, "LeftDelayTime", 0.8)?, number(node, "RightDelayTime", 0.1)?)
                } else {
                    let t = number(node, "DelayTime", 0.1)?;
                    (t, t)
                };
                let (left, right) = (left.clamp(0.001, 10.), right.clamp(0.001, 10.));
                let mut line = Line::single(DelayTime::Ms(left * 1000.), left.max(right) * 1000.);
                line.right_scale = right / left;
                feedback(&mut line, number(node, "Feedback", 0.)?);
                if tag == "PingPongDelay" {
                    // Spread picks the side of the first echo; width is not modelled.
                    let fb = line.feedback.matrix[0][0];
                    line.feedback.matrix = [[0., fb], [fb, 0.]];
                    line.input = if number(node, "Spread", -1.)? < 0. { [[0.5, 0.5], [0., 0.]] } else { [[0., 0.], [0.5, 0.5]] };
                }
                self.not_modeled(at, node, tag, &[("Cutoff", 0.), ("Diffusion", 1.)])?;
                if tag == "FatDelay" || tag == "FxDelay" {
                    // The documented Speed / Diffusion / drive stages are not modelled.
                    if tag == "FxDelay" {
                        self.not_modeled(at, node, tag, &[("BypassDrive", 0.)])?;
                    }
                }
                delay_config(line, mix)
            }
            "DualDelay" | "DualDelayX" => {
                let t = number(node, "DelayTime", 0.125)?.clamp(0.001, 5.);
                let ratio = number(node, "DelayRatio", 0.)?.clamp(-0.9, 0.9);
                let mut line = Line::single(DelayTime::Ms(t * 1000.), t * 1000. * (1. + ratio.max(0.)));
                line.right_scale = 1. + ratio;
                feedback(&mut line, number(node, "Feedback", 0.3)?);
                let depth = number(node, "ModDepth", 0.)?;
                if depth > 0. {
                    line.taps[0].depth_ms = depth.min(line.max_ms - t * 1000.).max(0.);
                }
                line.interpolation = match number(node, "Interpolation", 1.)? as i64 {
                    0 => Interpolation::Nearest,
                    1 => Interpolation::Linear,
                    _ => Interpolation::Hermite,
                };
                self.not_modeled(
                    at,
                    node,
                    tag,
                    &[
                        ("FeedbackRatio", 0.), ("LowCut", 20.), ("HighCut", 20000.), ("PeakGain", 0.), ("InputWidth", 1.),
                        ("InputRotation", 0.), ("OutputWidth", 1.), ("OutputRotation", 0.), ("Rotation", 0.),
                        ("Diffusion", 0.), ("Dispersion", 0.), ("Crusher", 0.), ("TapeSaturation", 0.), ("Reflection", 0.),
                    ],
                )?;
                Config { lfo: lfo(number(node, "ModRate", 1.)?), ..delay_config(line, mix) }
            }
            "TapeEcho" => {
                let mut line = Line::single(DelayTime::Ms(165.), 500.);
                for (i, (name, default)) in [("Delay1", 0.165), ("Delay2", 0.324), ("Delay3", 0.487)].into_iter().enumerate() {
                    line.taps[i] = Tap::at(DelayTime::Ms(number(node, name, default)?.clamp(0.05, 0.5) * 1000.));
                    line.taps[i].level = 1. / 3.;
                }
                line.tap_count = 3;
                // Feedback runs to 2 on the real unit; the loop is bounded here.
                feedback(&mut line, number(node, "Feedback", 0.5)?);
                self.not_modeled(at, node, tag, &[("Mode", 1.), ("Speed", 2.), ("Bass", 0.), ("Treble", 0.), ("NoiseLevel", 0.), ("TapeStop", 0.), ("ReverbGain", 1.)])?;
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
