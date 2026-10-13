//! Universal `color` DSP (wave 4): one drive, character and utility unit.
//!
//! [`Settings`] configures a fixed stage order, each stage optional:
//! gainer, sample hold, decimator, bit quantizer, waveshaper curve, damping
//! low-pass with DC blocker, noise, dry/wet mix, stereo modeller. A native
//! effect is a preset of it (see `sampler_ir::fx::color`). Every stage keeps
//! its own state in [`State`], so stages compose without sharing registers.
//!
//! Stages run over the whole block one after another, which equals the native
//! per-frame order because no stage reads another stage's state.
mod distortion;
mod lofi;
mod saturator;
mod shaping;
mod stereo;
mod tone;

pub use shaping::{Decimator, Rectifier};
pub use stereo::StereoSettings;

use super::{
    ControlRange, ControlRamp, Parameter, Planar, kernels,
    lanes::{Batch, Cells, LaneBlock, VOICES},
    live_knob,
};
use super::control::PreparedParameter;

/// Per-frame Gainer smoothing coefficient `F(1/1800)` (bits 0x3a11a2b4).
const GAIN_K: f32 = f32::from_bits(0x3a11_a2b4);

#[derive(Clone, Copy, Debug)]
pub struct GainerSettings {
    pub gain: Parameter,
    pub dry: f64,
}

#[derive(Clone, Copy, Debug)]
pub enum HoldSettings {
    /// Normalized Kontakt Lo-Fi Frequency.
    Native(Parameter),
    /// UVI BitCrusher target rate in Hz.
    Rate(f64),
}

#[derive(Clone, Copy, Debug)]
pub enum QuantizerSettings {
    /// Normalized Kontakt Lo-Fi Bits.
    Native(Parameter),
    /// UVI BitCrusher bit depth.
    Floor(f64),
}

#[derive(Clone, Copy, Debug)]
pub enum CurveSettings {
    Identity,
    /// Signed Shape -1..1.
    Classic(Parameter),
    /// Signed Shape -1..1 through the Enhanced table shaper.
    Enhanced(Parameter),
    /// Signed Shape -1..1 through the Drums clipper.
    Drums(Parameter),
    /// Normalized Drive.
    Tube(Parameter),
    Transistor(Parameter),
    Rectifier(Rectifier),
    /// UVI BitCrusher normalized Drive.
    Destructor(f64),
}

#[derive(Clone, Copy, Debug)]
pub struct NoiseSettings {
    pub level: Parameter,
    pub color: Parameter,
}

/// A color unit's configuration; control-driven knobs are [`Parameter`]s.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub gainer: Option<GainerSettings>,
    pub hold: Option<HoldSettings>,
    pub decimator: Option<Decimator>,
    pub quantizer: Option<QuantizerSettings>,
    pub curve: CurveSettings,
    /// Normalized damping of the Kontakt Distortion low-pass (plus DC blocker).
    pub damping: Option<Parameter>,
    pub noise: Option<NoiseSettings>,
    /// UVI BitCrusher Cutoff, -1..1 (negative low-pass, positive high-pass).
    pub tone: Option<f32>,
    /// Wet share, 0..=1.
    pub mix: f64,
    pub stereo: Option<StereoSettings>,
}

impl Settings {
    pub const IDENTITY: Self = Self {
        gainer: None,
        hold: None,
        decimator: None,
        quantizer: None,
        curve: CurveSettings::Identity,
        damping: None,
        noise: None,
        tone: None,
        mix: 1.0,
        stereo: None,
    };

    /// A Gainer: `x * (dry + g)`, `g` slewing to `gain`.
    pub fn gainer(gain: Parameter, dry: f64) -> Self {
        Self { gainer: Some(GainerSettings { gain, dry }), ..Self::IDENTITY }
    }

    /// A Stereo Modeller.
    pub fn stereo(stereo: StereoSettings) -> Self {
        Self { stereo: Some(stereo), ..Self::IDENTITY }
    }

    /// A waveshaper curve alone.
    pub fn curve(curve: CurveSettings) -> Self {
        Self { curve, ..Self::IDENTITY }
    }

    /// A fractional decimator alone.
    pub fn decimator(decimator: Decimator) -> Self {
        Self { decimator: Some(decimator), ..Self::IDENTITY }
    }

    pub(super) fn valid(&self) -> bool {
        let shape = |p: &Parameter| {
            p.valid()
                && !matches!(p, Parameter::Expression { .. })
                && p.bounds().iter().all(|v| (-1.0..=1.0).contains(v))
        };
        self.gainer.is_none_or(|g| {
            g.dry.is_finite() && g.gain.valid() && !matches!(g.gain, Parameter::Expression { .. })
        }) && match self.hold {
            None => true,
            Some(HoldSettings::Native(p)) => live_knob(&p),
            Some(HoldSettings::Rate(hz)) => hz.is_finite() && hz >= 1.0,
        } && self.decimator.is_none_or(|d| d.valid())
            && match self.quantizer {
                None => true,
                Some(QuantizerSettings::Native(p)) => live_knob(&p),
                Some(QuantizerSettings::Floor(bits)) => bits.is_finite() && (0.0..=32.0).contains(&bits),
            }
            && match &self.curve {
                CurveSettings::Identity | CurveSettings::Rectifier(_) => true,
                CurveSettings::Classic(p) | CurveSettings::Enhanced(p) | CurveSettings::Drums(p) => shape(p),
                CurveSettings::Tube(p) | CurveSettings::Transistor(p) => live_knob(p),
                CurveSettings::Destructor(d) => d.is_finite() && (0.0..=1.0).contains(d),
            }
            && self.damping.as_ref().is_none_or(live_knob)
            && self.noise.is_none_or(|n| live_knob(&n.level) && live_knob(&n.color))
            && self.tone.is_none_or(|t| t.is_finite() && (-1.0..=1.0).contains(&t))
            && self.mix.is_finite()
            && (0.0..=1.0).contains(&self.mix)
            && self.stereo.is_none_or(StereoSettings::valid)
    }

    /// Whether the unit leaves no tail: no stage keeps history.
    pub fn memoryless(&self) -> bool {
        self.gainer.is_none()
            && self.hold.is_none()
            && self.decimator.is_none()
            && self.damping.is_none()
            && self.noise.is_none()
            && self.tone.is_none()
            && self.stereo.is_none()
            && !matches!(self.curve, CurveSettings::Enhanced(_) | CurveSettings::Drums(_))
    }

    /// Frames of ring storage the pseudo-stereo delay needs.
    pub(super) fn ring_frames(&self) -> usize {
        if self.stereo.is_some_and(|s| s.pseudo) { 1024 } else { 0 }
    }

    pub(super) fn compile(self, rate: u32, bindings: &mut Vec<ControlRange>) -> Color {
        let dc = distortion::distortion_dc(rate as f32);
        let fixed = |p: PreparedParameter| match p {
            PreparedParameter::Constant(v) => Some(v as f32),
            _ => None,
        };
        let curve = match self.curve {
            CurveSettings::Identity => Curve::Identity,
            CurveSettings::Classic(p) => Curve::Classic(p.compile(bindings)),
            CurveSettings::Enhanced(p) => {
                saturator::prepare();
                Curve::Enhanced(p.compile(bindings), saturator::Rates::new(rate))
            }
            CurveSettings::Drums(p) => {
                saturator::prepare();
                Curve::Drums(p.compile(bindings), saturator::Rates::new(rate))
            }
            CurveSettings::Rectifier(mode) => Curve::Rectifier(mode),
            CurveSettings::Destructor(drive) => Curve::Destructor(lofi::Destructor::new(drive)),
            CurveSettings::Tube(p) | CurveSettings::Transistor(p) => {
                let transistor = matches!(self.curve, CurveSettings::Transistor(_));
                let p = p.compile(bindings);
                let shape = |d: f32| {
                    if transistor { distortion::Shape::transistor(d) } else { distortion::Shape::tube(d) }
                };
                match fixed(p) {
                    Some(d) => Curve::Fixed(shape(d)),
                    None => Curve::Driven { drive: p, transistor },
                }
            }
        };
        let damping = self.damping.map(|p| {
            let p = p.compile(bindings);
            match fixed(p) {
                Some(d) => Damping::Fixed(distortion::Damper::new(d, rate, dc)),
                None => Damping::Driven(p),
            }
        });
        Color {
            rate,
            dc,
            gainer: self.gainer.map(|g| (g.gain.compile(bindings), g.dry)),
            hold: self.hold.map(|h| match h {
                HoldSettings::Native(p) => Hold::Native(p.compile(bindings)),
                HoldSettings::Rate(hz) => Hold::Rate((f64::from(rate as f32) / hz) as f32),
            }),
            decimator: self.decimator,
            quantizer: self.quantizer.map(|q| match q {
                QuantizerSettings::Native(p) => Quantizer::Native(p.compile(bindings)),
                QuantizerSettings::Floor(bits) => Quantizer::Floor(bits),
            }),
            curve,
            damping,
            noise: self.noise.map(|n| [n.level.compile(bindings), n.color.compile(bindings)]),
            tone: self.tone.and_then(|cutoff| tone::Tone::new(cutoff, rate)),
            mix: self.mix as f32,
            stereo: self.stereo.map(|s| s.compile(rate, bindings)),
        }
    }
}

enum Hold {
    Native(PreparedParameter),
    /// Frames between captures.
    Rate(f32),
}
enum Quantizer {
    Native(PreparedParameter),
    Floor(f64),
}
enum Curve {
    Identity,
    Classic(PreparedParameter),
    Enhanced(PreparedParameter, saturator::Rates),
    Drums(PreparedParameter, saturator::Rates),
    /// A Distortion curve whose drive is constant.
    Fixed(distortion::Shape),
    Driven { drive: PreparedParameter, transistor: bool },
    Rectifier(Rectifier),
    Destructor(lofi::Destructor),
}
enum Damping {
    Fixed(distortion::Damper),
    Driven(PreparedParameter),
}

/// A prepared color unit.
pub(crate) struct Color {
    rate: u32,
    dc: [f32; 5],
    gainer: Option<(PreparedParameter, f64)>,
    hold: Option<Hold>,
    decimator: Option<Decimator>,
    quantizer: Option<Quantizer>,
    curve: Curve,
    damping: Option<Damping>,
    noise: Option<[PreparedParameter; 2]>,
    tone: Option<tone::Tone>,
    mix: f32,
    stereo: Option<stereo::Stereo>,
}

/// Every stage's history; a stage the unit lacks leaves its field untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    gain: f32,
    gain_ready: bool,
    hold: lofi::HoldState,
    decimator: shaping::DecimatorState,
    damper: [[f64; 5]; 2],
    noise: lofi::NoiseState,
    saturator: saturator::State,
    tone: tone::ToneState,
    stereo: stereo::StereoState,
}

impl State {
    pub(super) fn finite(&self) -> bool {
        let f32s = [self.gain, self.hold.held[0], self.hold.held[1], self.hold.phase,
            self.noise.history[0], self.noise.history[1], self.stereo.width, self.stereo.pan];
        f32s.iter().all(|v| v.is_finite())
            && self.decimator.z.iter().flatten().chain(&self.decimator.delta).chain([&self.decimator.phase]).all(|v| v.is_finite())
            && self.damper.iter().flatten().all(|v| v.is_finite())
            && self.saturator.finite()
            && self.tone.finite()
    }
}

impl Color {
    /// Whether the unit has a voice-lane kernel: only memoryless curves, the
    /// Gainer slew and the non-pseudo Stereo Modeller do.
    pub(super) fn batches(&self) -> bool {
        self.hold.is_none()
            && self.decimator.is_none()
            && self.quantizer.is_none()
            && self.damping.is_none()
            && self.noise.is_none()
            && self.tone.is_none()
            && self.mix == 1.0
            && matches!(self.curve, Curve::Identity | Curve::Classic(_) | Curve::Rectifier(_))
            && self.stereo.as_ref().is_none_or(stereo::Stereo::batches)
    }

    /// The dominant stage's trace name, and every knob as a parameter.
    pub(crate) fn trace(&self) -> (&'static str, Vec<(&'static str, PreparedParameter)>) {
        let constant = PreparedParameter::Constant;
        let mut parameters = Vec::new();
        let name = if self.hold.is_some() || self.quantizer.is_some() || self.noise.is_some() {
            "lofi"
        } else if matches!(self.curve, Curve::Fixed(_) | Curve::Driven { .. }) {
            "kontakt_distortion"
        } else if matches!(self.curve, Curve::Classic(_) | Curve::Enhanced(..) | Curve::Drums(..)) {
            "kontakt_classic_saturation"
        } else if matches!(self.curve, Curve::Rectifier(_)) {
            "rectifier"
        } else if self.decimator.is_some() {
            "decimator"
        } else if self.gainer.is_some() {
            "gainer"
        } else if self.stereo.is_some() {
            "stereo_modeller"
        } else {
            "color"
        };
        if let Some((gain, dry)) = self.gainer {
            parameters.push(("gain", gain));
            parameters.push(("dry", constant(dry)));
        }
        match &self.hold {
            Some(Hold::Native(p)) => parameters.push(("frequency", *p)),
            Some(Hold::Rate(interval)) => parameters.push(("hold_interval", constant(f64::from(*interval)))),
            None => {}
        }
        if let Some(Quantizer::Native(p)) = &self.quantizer {
            parameters.push(("bits", *p));
        }
        if let Some(Quantizer::Floor(bits)) = &self.quantizer {
            parameters.push(("bits", constant(*bits)));
        }
        match &self.curve {
            Curve::Classic(p) | Curve::Enhanced(p, _) | Curve::Drums(p, _) => parameters.push(("shape", *p)),
            Curve::Driven { drive, .. } => parameters.push(("drive", *drive)),
            _ => {}
        }
        match &self.damping {
            Some(Damping::Driven(p)) => parameters.push(("damping", *p)),
            Some(Damping::Fixed(d)) => parameters.push(("damping_pole", constant(d.pole()))),
            None => {}
        }
        if let Some([level, color]) = self.noise {
            parameters.push(("noise", level));
            parameters.push(("color", color));
        }
        if let Some(d) = self.decimator {
            parameters.push(("period", constant(d.period)));
            parameters.push(("blend", constant(d.blend)));
        }
        if let Some(s) = &self.stereo {
            parameters.extend(s.trace_parameters());
        }
        (name, parameters)
    }

    fn damper(&self, damping: &Damping, parameters: &[ControlRamp], at: u64) -> distortion::Damper {
        match damping {
            Damping::Fixed(d) => *d,
            Damping::Driven(p) => {
                distortion::Damper::new(p.value(parameters, at, None) as f32, self.rate, self.dc)
            }
        }
    }

    /// The memoryless curve over `len` frames (and the Distortion curves).
    fn shape(&self, state: &mut State, parameters: &[ControlRamp], block: &mut Planar, len: usize, at: u64) {
        match &self.curve {
            Curve::Identity => {}
            Curve::Rectifier(mode) => {
                for channel in block.iter_mut() {
                    channel[..len].iter_mut().for_each(|v| *v = mode.apply(*v));
                }
            }
            Curve::Classic(shape) => {
                for i in 0..len {
                    let value = shape.value(parameters, at + i as u64, None) as f32;
                    for channel in block.iter_mut() {
                        channel[i] = shaping::saturation(channel[i], value);
                    }
                }
            }
            Curve::Enhanced(shape, rates) => {
                let shape = shape.value(parameters, at, None) as f32;
                saturator::enhanced(shape, *rates, &mut state.saturator, block, len);
            }
            Curve::Drums(shape, rates) => {
                let shape = shape.value(parameters, at, None) as f32;
                saturator::drums(shape, *rates, &mut state.saturator, block, len);
            }
            Curve::Fixed(shape) => curve(shape, block, len),
            Curve::Driven { drive, transistor } => {
                let drive = drive.value(parameters, at, None) as f32;
                let shape = if *transistor {
                    distortion::Shape::transistor(drive)
                } else {
                    distortion::Shape::tube(drive)
                };
                curve(&shape, block, len);
            }
            Curve::Destructor(d) => d.process(block, len),
        }
    }

    pub(super) fn process(
        &self,
        state: &mut State,
        parameters: &[ControlRamp],
        block: &mut Planar,
        len: usize,
        at: u64,
        ring: &mut [[f64; 2]],
    ) {
        let input = (self.mix != 1.0).then_some(*block);
        if let Some((gain, dry)) = self.gainer {
            let [left, right] = &mut *block;
            let mut current = state.gain;
            for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
                let target = gain.value(parameters, at + i as u64, None) as f32;
                if !state.gain_ready {
                    (current, state.gain_ready) = (target, true);
                }
                let (m, next) = kernels::gainer(current, target, dry, GAIN_K);
                *l = kernels::gain(*l, m);
                *r = kernels::gain(*r, m);
                current = next;
            }
            state.gain = current;
        }
        match &self.hold {
            Some(Hold::Native(frequency)) => {
                let frequency = frequency.value(parameters, at, None) as f32;
                if let Some(period) = lofi::native_period(frequency, self.rate) {
                    lofi::hold_native(period, &mut state.hold, block, len);
                }
            }
            Some(Hold::Rate(interval)) => lofi::hold_rate(*interval, &mut state.hold, block, len),
            None => {}
        }
        if let Some(decimator) = &self.decimator {
            decimator.process(&mut state.decimator, block, len);
        }
        match &self.quantizer {
            Some(Quantizer::Native(bits)) => {
                let bits = bits.value(parameters, at, None) as f32;
                if let Some(step) = lofi::native_step(bits) {
                    lofi::quantize_native(step, block, len);
                }
            }
            Some(Quantizer::Floor(bits)) => lofi::quantize_floor(*bits, block, len),
            None => {}
        }
        self.shape(state, parameters, block, len, at);
        if let Some(damping) = &self.damping {
            self.damper(damping, parameters, at).process(&mut state.damper, block, len);
        }
        if let Some(tone) = &self.tone {
            tone.process(&mut state.tone, block, len);
        }
        if let Some([level, color]) = self.noise {
            let [level, color] = [level, color].map(|p| p.value(parameters, at, None) as f32);
            let (active, gain, alpha) = lofi::native_noise(level, color);
            if active {
                lofi::noise((gain, alpha), &mut state.noise, block, len);
            }
        }
        if let Some(input) = input {
            let (wet, dry) = (self.mix, 1.0 - self.mix);
            for (out, original) in block.iter_mut().zip(&input) {
                for (y, x) in out[..len].iter_mut().zip(&original[..len]) {
                    *y = f64::from((*y as f32) * wet + dry * (*x as f32));
                }
            }
        }
        if let Some(stereo) = &self.stereo {
            stereo.process(&mut state.stereo, parameters, block, len, at, ring);
        }
    }

    /// Voice-lane layout of [`Self::process`] for a unit that [`Self::batches`]:
    /// every lane does exactly the scalar arithmetic, voice by voice.
    pub(super) fn process_lanes(
        &self,
        cells: &mut Cells<'_>,
        cell: usize,
        batch: &Batch,
        block: &mut LaneBlock,
        parameters: &[ControlRamp],
        at: u64,
    ) {
        let len = batch.len;
        if let Some((gain, dry)) = self.gainer {
            let mut current = [0.0f32; VOICES];
            let mut ready = [false; VOICES];
            for v in 0..batch.count {
                let state = &cells[v].as_ref().expect("batch voice")[cell].color;
                (current[v], ready[v]) = (state.gain, state.gain_ready);
            }
            for (i, x) in block[..len].iter_mut().enumerate() {
                let target = gain.value(parameters, at + i as u64, None) as f32;
                for v in 0..batch.count {
                    if i >= batch.ends[2 * v] {
                        continue;
                    }
                    if !ready[v] {
                        (current[v], ready[v]) = (target, true);
                    }
                    let (m, next) = kernels::gainer(current[v], target, dry, GAIN_K);
                    x[2 * v] = kernels::gain(x[2 * v], m);
                    x[2 * v + 1] = kernels::gain(x[2 * v + 1], m);
                    current[v] = next;
                }
            }
            for v in 0..batch.count {
                let state = &mut cells[v].as_mut().expect("batch voice")[cell].color;
                (state.gain, state.gain_ready) = (current[v], ready[v]);
            }
        }
        match &self.curve {
            Curve::Rectifier(mode) => {
                for x in &mut block[..len] {
                    x.iter_mut().for_each(|v| *v = mode.apply(*v));
                }
            }
            Curve::Classic(shape) => {
                for (i, x) in block[..len].iter_mut().enumerate() {
                    let value = shape.value(parameters, at + i as u64, None) as f32;
                    x.iter_mut().for_each(|v| *v = shaping::saturation(*v, value));
                }
            }
            _ => {}
        }
        if let Some(stereo) = &self.stereo {
            let mut width = [0.0f32; VOICES];
            let mut pan = [0.0f32; VOICES];
            let mut delta = [0.0f32; VOICES];
            let mut ready = [false; VOICES];
            for v in 0..batch.count {
                let state = &cells[v].as_ref().expect("batch voice")[cell].color.stereo;
                (width[v], pan[v], ready[v]) = (state.width, state.pan, state.ready);
            }
            for (i, x) in block[..len].iter_mut().enumerate() {
                let [target_width, target_pan] = stereo.targets(parameters, at + i as u64);
                for v in 0..batch.count {
                    let end = batch.ends[2 * v];
                    if i >= end {
                        continue;
                    }
                    if !ready[v] {
                        (width[v], pan[v], ready[v]) = (target_width, target_pan, true);
                    }
                    let (l, r) = stereo::matrix(x[2 * v] as f32, x[2 * v + 1] as f32, width[v]);
                    [x[2 * v], x[2 * v + 1]] = stereo::balance(l, r, pan[v]);
                    [width[v], pan[v], delta[v]] = stereo::advance(
                        [width[v], pan[v], delta[v]],
                        [target_width, target_pan],
                        i,
                        end,
                    );
                }
            }
            for v in 0..batch.count {
                let state = &mut cells[v].as_mut().expect("batch voice")[cell].color.stereo;
                (state.width, state.pan, state.ready) = (width[v], pan[v], ready[v]);
            }
        }
    }
}

fn curve(shape: &distortion::Shape, block: &mut Planar, len: usize) {
    for channel in block.iter_mut() {
        for x in &mut channel[..len] {
            *x = f64::from(shape.apply(*x as f32));
        }
    }
}

#[cfg(test)]
mod tests;
