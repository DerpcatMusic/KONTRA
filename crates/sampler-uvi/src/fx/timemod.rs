//! UVI delay, chorus, flanger, phaser and ensemble inserts onto
//! `sampler_ir::fx::timemod`.
//!
//! Chorus, Flanger, Phaser, Cross Phaser and Phasor follow native laws read
//! from UVI Workstation 4.0.9 ([`Translation::native_modulation`]); TrackDelay
//! and DualDelay(X) run their native laws too. The other inserts here are
//! documented-parameter, generic topologies: each reports `UnknownLaw` for the
//! effect and `NotModeled` for each control it cannot honour, so the inspector
//! shows "generic", never "exact".
use crate::{Translation, number};
use roxmltree::Node;
use sampler_ir::{
    self as ir,
    fx::timemod::{
        Allpass, Config, DelayTime, DualDelay, Interpolation, Lfo, LfoShape, Line, Native, Order, Rate, SweepLaw,
        Tap, UviWave,
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
        if let Some(native) = self.native_modulation(node, at)? {
            return Ok(native);
        }
        let mix = number(node, "Mix", 1.)?.clamp(0., 1.);
        let synced = number(node, "SyncToHost", 0.)? != 0.;
        if tag == "TrackDelay" {
            return Ok(track_delay(number(node, "DelayTime", 0.)?, synced));
        }
        if tag == "DualDelay" || tag == "DualDelayX" {
            return self.dual_delay(node, at, tag, synced).map(Some);
        }
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
            "AnalogChorus" | "AnalogFlanger" | "WhiteChorus" | "Ensemble" => {
                let (base, depth_ms, speed, voices, fb_amount, mix): (f64, f64, f64, u8, f64, f64) = match tag {
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
            "PhasorFilter" => {
                let depth = number(node, "Depth", 0.8)?.clamp(0., 1.);
                let f = number(node, "Freq", 1000.)?.clamp(20., 20000.) as f32;
                self.not_modeled(at, node, tag, &[("KeyTracking", 0.), ("CutoffMode", 0.)])?;
                Config {
                    line: None,
                    allpass: Some(Allpass {
                        stages: number(node, "Order", 3.)?.clamp(1., 12.) as u8,
                        law: SweepLaw::Sweep { min_hz: f, max_hz: f, depth: depth as f32 },
                        feedback: number(node, "Feedback", 0.7)?.clamp(-0.98, 0.98) as f32,
                    }),
                    order: Order::LineThenAllpass,
                    lfo: lfo(number(node, "Speed", 0.8)?),
                    // The filter variant is the allpass alone.
                    dry: 0.,
                    wet: 1.,
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

/// First present attribute of `names` (modern name, then the legacy alias).
fn number_any(node: Node, names: &[&str], default: f64) -> Result<f64, String> {
    match names.iter().find(|name| node.attribute(**name).is_some()) {
        Some(name) => number(node, name, default),
        None => Ok(default),
    }
}

impl Translation {
    /// UVI modulation inserts with native laws (Workstation 4.0.9). `None`
    /// for tags this path does not own.
    fn native_modulation(&mut self, node: Node, at: &str) -> Result<Option<Option<Config>>, String> {
        let tag = node.tag_name().name();
        let prefix = match tag {
            "Chorus" | "Flanger" | "CrossPhaser" => tag,
            "Phaser" | "Phasor" => "Phaser",
            _ => return Ok(None),
        };
        let legacy = |name: &str| format!("{prefix}{name}");
        let speed = number_any(node, &["Speed", &legacy("Speed")], if tag == "Phasor" { 0.3 } else { 0.8 })?;
        // Legacy "WetMix" reads percent above 1 (0x1412e0070).
        let mix = match node.attribute("Mix") {
            Some(_) => number(node, "Mix", 1.)?,
            None => {
                let wet = number(node, "WetMix", 1.)?;
                if wet > 1. { wet * 0.01 } else { wet }
            }
        }
        .clamp(0., 1.);
        let synced = number(node, "SyncToHost", 0.)? != 0.;
        // Synced Speed is in beats: Hz = tempo / 60 / Speed. The phasers clamp at
        // 10 Hz (0x141cfab90); the Chorus / Flanger bounds are runtime data
        // (0x14258cc08) that was not read.
        let max_hz = if matches!(tag, "Chorus" | "Flanger") { 200. } else { 10. };
        if synced && max_hz > 10. {
            self.unsupported(at, &format!("{tag} synced rate clamp"), "runtime bounds 0x14258cc08 unread");
        }
        let rate = if synced && speed > 0. {
            Rate::Beats { beats: speed as f32, max_hz }
        } else {
            Rate::Hz(speed.clamp(0., 200.) as f32)
        };
        let lfo = |wave, direct, phase| Lfo { shape: LfoShape::Uvi { wave, direct }, rate, phase };
        let config = match tag {
            "Chorus" => {
                // 0x1412e02a0: two voices per channel on triangle LFOs at L 0 / 0.5 and
                // R 0.25 / 0.75 cycles, linear read of `v * 4 ms + DelayTime * 50 ms +
                // (lfo + 1) * Depth * 5 ms` (ring 68 ms), out = x + Mix * (y0 + y1).
                let time = number_any(node, &["DelayTime", "ChorusDelayTime"], 0.01)?.clamp(0., 1.);
                let depth = number_any(node, &["Depth", "ChorusDepth"], 0.38)?.clamp(0., 1.);
                let mut line = Line::single(DelayTime::Ms(time * 50. + depth * 5.), 68.);
                line.taps[0].depth_ms = depth * 5.;
                line.voices = 2;
                line.voice_phase = 1.;
                line.voice_delay_ms = 4.;
                // The voice bank averages its voices; UVI sums them.
                Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(UviWave::Triangle, false, 0.25), dry: 1., wet: 2. * mix }
            }
            "Flanger" => {
                // 0x1412eeec0: one triangle LFO for both channels, Catmull-Rom read of
                // `DelayTime * 20 ms + (lfo + 1) * Depth * 10 ms` (ring 44 ms), ring
                // written `x + Feedback * y`, out = x + Mix * y.
                let time = number_any(node, &["DelayTime", "FlangerDelayTime"], 0.2)?.clamp(0., 1.);
                let depth = number_any(node, &["Depth", "FlangerDepth"], 0.08)?.clamp(0., 1.);
                let mut fb = number_any(node, &["Feedback", "FlangerFeedback"], 0.)?.clamp(0., 1.);
                if fb > 0.999 {
                    self.unsupported(at, "Flanger feedback 1 (the native loop is unbounded)", fb);
                    fb = 0.999;
                }
                let mut line = Line::single(DelayTime::Ms(time * 20. + depth * 10.), 44.);
                line.taps[0].depth_ms = depth * 10.;
                line.interpolation = Interpolation::Hermite;
                line.feedback.matrix = [[fb, 0.], [0., fb]];
                Config { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: lfo(UviWave::Triangle, true, 0.), dry: 1., wet: mix }
            }
            "Phaser" | "CrossPhaser" => {
                // 0x1412f0ec0 / 0x1412e4c90: six allpasses on a linear w sweep, sine
                // table LFO (Cross Phaser: right channel half a cycle on), out = x + Depth * y.
                let min_hz = number_any(node, &["MinFreq", &legacy("MinFreq")], 400.)?.clamp(1., 24_000.);
                let max_hz = number_any(node, &["MaxFreq", &legacy("MaxFreq")], 800.)?.clamp(1., 24_000.);
                let depth = number_any(node, &["Depth", &legacy("Depth")], 0.8)?.clamp(0., 1.);
                let fb = number_any(node, &["Feedback", &legacy("FeedBack")], 0.7)?.clamp(0., 0.999);
                Config {
                    line: None,
                    allpass: Some(Allpass { stages: 6, law: SweepLaw::Linear { min_hz: min_hz as f32, max_hz: max_hz as f32 }, feedback: fb as f32 }),
                    order: Order::LineThenAllpass,
                    lfo: lfo(UviWave::Sine, tag == "Phaser", if tag == "CrossPhaser" { 0.5 } else { 0. }),
                    dry: 1.,
                    wet: depth,
                }
            }
            _ => {
                // Phasor (0x14166c120): 2 * Order allpasses swept in the delay domain,
                // per-channel LFOs Spread / 4 cycles apart, equal-power Depth mix.
                let min_hz = number(node, "MinFreq", 200.)?.clamp(1., 24_000.);
                let max_hz = number(node, "MaxFreq", 3000.)?.clamp(1., 24_000.);
                let depth = number(node, "Depth", 1.)?.clamp(0., 1.);
                let order = number(node, "Order", 3.)?.clamp(1., 12.) as u8;
                let shape = number(node, "LFOSHape", 0.)? as i64;
                let wave = match shape {
                    1 => UviWave::Sine,
                    0 => UviWave::Triangle,
                    _ => {
                        self.unsupported(at, "Phasor random LFO shape (LCG sample-and-hold)", shape);
                        UviWave::Triangle
                    }
                };
                let spread = number(node, "Spread", 1.)?.clamp(0., 1.);
                let norm = (depth * depth + 1.).sqrt();
                Config {
                    line: None,
                    allpass: Some(Allpass {
                        stages: 2 * order,
                        law: SweepLaw::DelayDomain { min_hz: min_hz as f32, max_hz: max_hz as f32, order },
                        feedback: number(node, "Feedback", 0.7)?.clamp(-0.99, 0.99) as f32,
                    }),
                    order: Order::LineThenAllpass,
                    lfo: lfo(wave, false, (spread * 0.25) as f32),
                    dry: 1. / norm,
                    wet: depth / norm,
                }
            }
        };
        Ok(Some(match config.check() {
            Ok(()) => Some(config),
            Err(field) => {
                self.unsupported(at, &format!("{tag} {field}"), "out of range");
                None
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use sampler_ir::{
        Processor,
        fx::timemod::{Config, DelayTime, Interpolation, LfoShape, Rate, SweepLaw, UviWave},
    };

    /// The one program insert's configuration and the unsupported notes.
    fn insert(xml: &str) -> (Config, Vec<String>) {
        let program = format!("<UVI4><Program><Inserts>{xml}</Inserts></Program></UVI4>");
        let ir = crate::translate(&program, std::path::Path::new(".")).unwrap().instrument;
        let config = ir
            .chains
            .iter()
            .flat_map(|c| &c.pre_amplitude)
            .find_map(|p| match p {
                Processor::TimeMod(c) => Some(*c),
                _ => None,
            })
            .expect("a timemod stage");
        (config, ir.unsupported.iter().map(|u| u.feature.clone()).collect())
    }

    #[test]
    fn chorus_is_two_triangle_voices_four_ms_apart() {
        // Native vector (0x1412e02a0): DelayTime 0.2, Depth 0 -> taps at 10 ms and 14 ms.
        let (c, notes) = insert(r#"<Chorus DelayTime="0.2" Depth="0" Speed="0.8" Mix="0.5"/>"#);
        assert!(notes.is_empty(), "{notes:?}");
        let line = c.line.unwrap();
        assert_eq!((line.taps[0].time, line.taps[0].depth_ms), (DelayTime::Ms(10.), 0.));
        assert_eq!((line.voices, line.voice_phase, line.voice_delay_ms, line.max_ms), (2, 1., 4., 68.));
        assert_eq!(line.interpolation, Interpolation::Linear);
        assert_eq!(c.lfo.shape, LfoShape::Uvi { wave: UviWave::Triangle, direct: false });
        assert_eq!((c.lfo.phase, c.dry, c.wet), (0.25, 1., 1.));
        // Legacy names, percent WetMix.
        let (c, _) = insert(r#"<Chorus ChorusDelayTime="1" ChorusDepth="1" ChorusSpeed="2" WetMix="50"/>"#);
        assert_eq!(c.line.unwrap().taps[0].time, DelayTime::Ms(55.));
        assert_eq!((c.lfo.rate, c.wet), (Rate::Hz(2.), 1.));
    }

    #[test]
    fn flanger_phaser_and_phasor_follow_their_native_laws() {
        // Flanger (0x1412eeec0): DelayTime 0.5 -> 10 ms, Depth 0.08 -> 0.8 ms sweep.
        let (c, notes) = insert(r#"<Flanger DelayTime="0.5" Feedback="0.5"/>"#);
        assert!(notes.is_empty(), "{notes:?}");
        let line = c.line.unwrap();
        assert_eq!(line.taps[0].time, DelayTime::Ms(10. + 0.8));
        assert!((line.taps[0].depth_ms - 0.8).abs() < 1e-12);
        assert_eq!((line.feedback.matrix[0][0], line.interpolation), (0.5, Interpolation::Hermite));
        assert_eq!(c.lfo.shape, LfoShape::Uvi { wave: UviWave::Triangle, direct: true });
        // Phaser: six stages, x + Depth * y; Cross Phaser's right LFO half a cycle on.
        let (c, _) = insert(r#"<Phaser Depth="0.6"/>"#);
        let ap = c.allpass.unwrap();
        assert_eq!((ap.stages, ap.law, ap.feedback), (6, SweepLaw::Linear { min_hz: 400., max_hz: 800. }, 0.7));
        assert_eq!((c.dry, c.wet, c.lfo.phase), (1., 0.6, 0.));
        let (c, _) = insert(r#"<CrossPhaser/>"#);
        assert_eq!(c.lfo.phase, 0.5);
        // Phasor: 2 * Order stages, equal-power Depth, Spread / 4 cycles.
        let (c, notes) = insert(r#"<Phasor Order="3" Depth="1" Spread="1"/>"#);
        assert!(notes.is_empty(), "{notes:?}");
        let ap = c.allpass.unwrap();
        assert_eq!((ap.stages, ap.law), (6, SweepLaw::DelayDomain { min_hz: 200., max_hz: 3000., order: 3 }));
        assert!((c.dry - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12 && (c.wet - c.dry).abs() < 1e-12);
        assert_eq!(c.lfo.phase, 0.25);
        let (_, notes) = insert(r#"<Phasor LFOSHape="2"/>"#);
        assert_eq!(notes.len(), 1, "random LFO shapes are named");
    }
}
