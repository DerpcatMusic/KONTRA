//! Native DAHDSR/AHD envelopes. Durations are frames at the prepared sample rate.
use crate::Error;

/// Normalized exponential segment: expm1(curvature * t) / expm1(curvature).
/// Zero is linear. Positive curvature starts slowly; negative starts quickly.
/// This native unit is not a Kontakt or Falcon parameter value.
#[derive(Clone, Copy, Debug, Default)]
pub struct EnvelopeCurve(f64);

impl EnvelopeCurve {
    pub fn exponential(curvature: f64) -> Result<Self, Error> {
        if !curvature.is_finite() || !(-32.0..=32.0).contains(&curvature) {
            return Err(Error::InvalidInput);
        }
        Ok(Self(curvature))
    }

    /// Holds the stage's starting level until the stage ends, then steps.
    pub fn step() -> Self {
        Self(f64::INFINITY)
    }

    /// Normalized position at stage fraction `t` (0..=1).
    pub fn value(self, t: f64) -> f64 {
        if self.0 == 0.0 {
            t
        } else if self.0.is_infinite() {
            if t >= 1.0 { 1.0 } else { 0.0 }
        } else {
            (self.0 * t).exp_m1() / self.0.exp_m1()
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Curve {
    /// Position 0 until the stage ends (a step).
    step: bool,
    curvature: f64,
    denominator: f64,
    multiplier: f64,
    delta: f64,
}

impl Curve {
    fn exprel(x: f64) -> f64 {
        if x.abs() < 1e-5 {
            1.0 + x * (0.5 + x * (1.0 / 6.0 + x / 24.0))
        } else {
            x.exp_m1() / x
        }
    }

    fn new(curve: EnvelopeCurve, duration: u32) -> Self {
        if curve.0.is_infinite() {
            return Self {
                step: true,
                ..Self::default()
            };
        }
        if curve.0 == 0.0 || duration == 0 {
            return Self::default();
        }
        let denominator = Self::exprel(curve.0);
        let step = curve.0 / f64::from(duration);
        Self {
            step: false,
            curvature: curve.0,
            denominator,
            multiplier: step.exp(),
            delta: Self::exprel(step) / denominator / f64::from(duration),
        }
    }

    fn at(self, age: u32, duration: u32) -> f64 {
        let t = f64::from(age) / f64::from(duration);
        t * Self::exprel(self.curvature * t) / self.denominator
    }
}

/// Why this stage envelope is NOT retired in favour of [`Breakpoints`]:
/// `dahdsr_points_match_the_stage_envelope` only measures equality of the generic
/// f64 path (< 2e-6, not bit-exact: closed-form vs recurrence). The `native`
/// Primary path (f32 geometric AHDSR, 32-frame interpolation, verified against
/// the v1 oracle in tests/v1_voice_controls.rs) has no point-list form, and this
/// type also owns choke, release onset, one-shot retirement, stage-control edits
/// and constant-level skipping that `Segment` lacks. Retire only after those
/// move into `Segment` and the Primary law is proved expressible as points.
///
/// Validated immutable envelope parameters. Zero-length stages are skipped.
/// Attack starts at zero, decay at one, release at the next held sample's level.
/// Each ramp reaches its endpoint at the exclusive end of its duration.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    delay: u32,
    attack: u32,
    hold: u32,
    decay: u32,
    sustain: f32,
    release: u32,
    one_shot: bool,
    curves: [Curve; 3],
    native: Option<crate::v1_voice_controls::Primary>,
    controlled: bool,
}

impl Default for Envelope {
    fn default() -> Self {
        Self {
            delay: 0,
            attack: 0,
            hold: 0,
            decay: 0,
            sustain: 1.0,
            release: 0,
            one_shot: false,
            curves: [Curve::default(); 3],
            native: None,
            controlled: false,
        }
    }
}

impl Envelope {
    pub(crate) fn trace_parameters(&self) -> [(&'static str, f64); 10] {
        [
            ("delay_frames", self.delay as f64),
            ("attack_frames", self.attack as f64),
            ("hold_frames", self.hold as f64),
            ("decay_frames", self.decay as f64),
            ("sustain", self.sustain as f64),
            ("release_frames", self.release as f64),
            ("one_shot", f64::from(self.one_shot)),
            ("attack_curvature", self.curves[0].curvature),
            ("decay_curvature", self.curves[1].curvature),
            ("release_curvature", self.curves[2].curvature),
        ]
    }
    pub fn new(
        attack: u32,
        hold: u32,
        decay: u32,
        sustain: f32,
        release: u32,
    ) -> Result<Self, Error> {
        if !sustain.is_finite() || !(0.0..=1.0).contains(&sustain) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            attack,
            hold,
            decay,
            sustain,
            release,
            ..Self::default()
        })
    }

    /// AHD completes independently of gate closure, then retires its source.
    /// Explicit stops, panic and family chokes still apply. Source-loop release
    /// policy is independent of this envelope gate policy.
    pub fn one_shot(attack: u32, hold: u32, decay: u32) -> Self {
        Self {
            attack,
            hold,
            decay,
            sustain: 0.0,
            one_shot: true,
            ..Self::default()
        }
    }

    /// Ends by itself, whatever the gate and the source do.
    pub(crate) fn finite(&self) -> bool {
        self.one_shot && self.hold != u32::MAX
    }

    pub(crate) fn control_value(self, stage: crate::EnvelopeStage) -> f64 {
        use crate::EnvelopeStage as S;
        match stage {
            S::Attack => self.attack as f64,
            S::Hold => self.hold as f64,
            S::Decay => self.decay as f64,
            S::Release => self.release as f64,
            S::Sustain => self.sustain as f64,
            S::AttackCurve => self.curves[0].curvature,
        }
    }
    pub(crate) fn with_control(mut self, stage: crate::EnvelopeStage, value: f64) -> Self {
        self.native = None;
        self.controlled = true;
        match stage {
            crate::EnvelopeStage::Sustain => self.sustain = value.clamp(0., 1.) as f32,
            crate::EnvelopeStage::AttackCurve => {
                self.curves[0] = Curve::new(EnvelopeCurve(value), self.attack)
            }
            _ => self = self.with_stage(stage, value.round().clamp(0., u32::MAX as f64) as u32),
        }
        self
    }

    /// Replace one stage's frames (or Sustain's 0..=1000 level), keeping
    /// the stage's curvature. Script engine parameters; see `script_params`.
    pub(crate) fn with_stage(mut self, stage: crate::EnvelopeStage, value: u32) -> Self {
        // ponytail: live frame-valued setters use v2 until original native knob values reach this owner.
        self.native = None;
        self.controlled = true;
        use crate::EnvelopeStage as S;
        let recurve = |curve: Curve, frames| {
            Curve::new(
                if curve.step {
                    EnvelopeCurve::step()
                } else {
                    EnvelopeCurve(curve.curvature)
                },
                frames,
            )
        };
        match stage {
            S::Attack => {
                self.attack = value;
                self.curves[0] = recurve(self.curves[0], value);
            }
            S::Hold => self.hold = value,
            S::Decay => {
                self.decay = value;
                self.curves[1] = recurve(self.curves[1], value);
            }
            S::Sustain => self.sustain = (value.min(1000) as f32) / 1000.0,
            S::AttackCurve => {
                // Authored curve c = 2v/1e6 - 1 (Barbarian's 700195 is the
                // 10-bit quantised 0.7, c = 0.4); the law is the one
                // sampler-kontakt `ahdsr_curves` decodes for stored curves.
                let c = (f64::from(value.min(1_000_000)) / 500_000.0 - 1.0).clamp(-1.0, 1.0);
                let b =
                    f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
                let k = if c > 0.0 {
                    (b / (1.0 + b)).ln()
                } else {
                    ((1.0 + b) / b).ln()
                };
                self.curves[0] = Curve::new(EnvelopeCurve(k), self.attack);
            }
            S::Release => {
                self.release = value;
                self.curves[2] = recurve(self.curves[2], value);
            }
        }
        self
    }

    pub(crate) fn with_native_primary(mut self, source: &sampler_ir::SourceAhdsr, rate: u32) -> Result<Self, Error> {
        let params = crate::v1_voice_controls::Ahdsr::from(source);
        if !crate::v1_voice_controls::valid_ahdsr(&params) || rate < 10 {
            return Err(Error::InvalidInput);
        }
        self.native = Some(crate::v1_voice_controls::Primary::new(&params, rate as f32, true));
        Ok(self)
    }

    pub fn with_delay(mut self, frames: u32) -> Self {
        self.native = None;
        self.delay = frames;
        self
    }

    /// Coefficients are compiled here, off audio. No per-sample power function.
    pub fn with_curves(
        mut self,
        attack: EnvelopeCurve,
        decay: EnvelopeCurve,
        release: EnvelopeCurve,
    ) -> Self {
        self.native = None;
        self.curves = [
            Curve::new(attack, self.attack),
            Curve::new(decay, self.decay),
            Curve::new(release, self.release),
        ];
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Delay,
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Done,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EnvelopeState {
    shape: Envelope,
    phase: Phase,
    age: u32,
    release_level: f32,
    progress: f64,
    delta: f64,
}

impl EnvelopeState {
    pub(crate) fn admits_v1_controls(&self) -> bool { !self.shape.controlled && !self.shape.one_shot && self.shape.delay == 0 }
    pub(crate) fn set_v1_amplitude(&mut self, primary: crate::v1_voice_controls::Primary, _rate: f32) {
        if !self.shape.controlled { self.shape.native = Some(primary); }
    }

    pub(crate) fn trace_parameters(&self) -> [f64; 10] { self.shape.trace_parameters().map(|(_,v)|v) }
    pub(crate) fn new(shape: Envelope) -> Self {
        let mut state = Self {
            shape,
            phase: Phase::Delay,
            age: 0,
            release_level: 0.0,
            progress: 0.0,
            delta: 0.0,
        };
        state.enter(Phase::Delay);
        state
    }

    fn duration(&self) -> u32 {
        match self.phase {
            Phase::Delay => self.shape.delay,
            Phase::Attack => self.shape.attack,
            Phase::Hold => self.shape.hold,
            Phase::Decay => self.shape.decay,
            Phase::Release => self.shape.release,
            Phase::Sustain | Phase::Done => 0,
        }
    }

    fn curve(&self) -> Curve {
        match self.phase {
            Phase::Attack => self.shape.curves[0],
            Phase::Decay => self.shape.curves[1],
            Phase::Release => self.shape.curves[2],
            _ => Curve::default(),
        }
    }

    fn following(&self) -> Phase {
        match self.phase {
            Phase::Delay => Phase::Attack,
            Phase::Attack => Phase::Hold,
            Phase::Hold => Phase::Decay,
            Phase::Decay if !self.shape.one_shot => Phase::Sustain,
            _ => Phase::Done,
        }
    }

    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        // At most the four finite onset stages; no cyclic zero-time transitions.
        while !matches!(self.phase, Phase::Sustain | Phase::Done) && self.duration() == 0 {
            self.phase = self.following();
        }
        self.age = 0;
        self.progress = 0.0;
        self.delta = self.curve().delta;
    }

    fn level(&self) -> f32 {
        #[cfg(test)]
        LEVEL_READS.set(LEVEL_READS.get() + 1);
        if let Some(native) = &self.shape.native {
            return native.level();
        }
        let e = self.shape;
        let curved = self.curve().curvature != 0.0 || self.curve().step;
        let position = self.progress.clamp(0.0, 1.0);
        match self.phase {
            Phase::Delay | Phase::Done => 0.0,
            Phase::Attack if curved => position as f32,
            Phase::Attack => (f64::from(self.age) / f64::from(e.attack)) as f32,
            Phase::Hold => 1.0,
            Phase::Decay if curved => (1.0 + (f64::from(e.sustain) - 1.0) * position) as f32,
            Phase::Decay => {
                (1.0 + (f64::from(e.sustain) - 1.0) * f64::from(self.age) / f64::from(e.decay))
                    as f32
            }
            Phase::Sustain => e.sustain,
            Phase::Release => {
                let position = if curved {
                    position
                } else {
                    f64::from(self.age) / f64::from(e.release)
                };
                (f64::from(self.release_level) * (1.0 - position)) as f32
            }
        }
    }

    /// Repeated cleanup must never restart an already progressing release.
    pub(crate) fn release(&mut self) {
        if let Some(native) = &mut self.shape.native {
            native.release();
            return;
        }
        if !self.shape.one_shot && !matches!(self.phase, Phase::Release | Phase::Done) {
            self.release_level = self.level();
            self.enter(Phase::Release);
        }
    }

    /// A note released before its storage onset enters release from sustain;
    /// its source and release clock still wait for the first complete window.
    pub(crate) fn release_onset(&mut self) {
        // Storage-onset release starts from sustain under the existing v2 policy.
        self.shape.native = None;
        self.shape.controlled = true;
        if !self.shape.one_shot && !matches!(self.phase, Phase::Release | Phase::Done) {
            self.release_level = self.shape.sustain;
            self.enter(Phase::Release);
        }
    }

    /// Capture any curved stage's next level, then apply a non-extending linear fade.
    pub(crate) fn choke(&mut self, frames: u32) {
        if self.shape.native.is_some() && frames as usize >= self.remaining() {
            return;
        }
        if (self.shape.one_shot || matches!(self.phase, Phase::Release | Phase::Done))
            && frames as usize >= self.remaining()
        {
            return;
        }
        self.release_level = self.level();
        self.shape.native = None;
        self.shape.controlled = true;
        self.shape.release = frames;
        self.shape.curves[2] = Curve::default();
        self.enter(Phase::Release);
    }

    pub(crate) fn releasing(&self) -> bool {
        if let Some(native) = &self.shape.native { return matches!(native.phase(), crate::v1_voice_controls::Phase::Release | crate::v1_voice_controls::Phase::Done); }
        matches!(self.phase, Phase::Release | Phase::Done)
    }

    pub(crate) fn editor_phase(&self)->u8 {
        if let Some(native) = &self.shape.native {
            use crate::v1_voice_controls::Phase as N;
            return match native.phase() { N::Attack=>0, N::Hold=>1, N::Decay=>2, N::Sustain=>3, N::Release=>4, N::Done=>6, N::Flex=>5 };
        }
        match self.phase {Phase::Attack=>0,Phase::Hold=>1,Phase::Decay=>2,Phase::Sustain=>3,Phase::Release=>4,Phase::Delay=>5,Phase::Done=>6}}
    /// The level the next frame starts from.
    pub(crate) fn current(&self) -> f32 {
        self.level()
    }

    pub(crate) fn done(&self) -> bool {
        if let Some(native) = &self.shape.native {
            return native.done();
        }
        self.phase == Phase::Done
    }

    pub(crate) fn remaining(&self) -> usize {
        if let Some(native) = &self.shape.native {
            return native.remaining();
        }
        let frames = match self.phase {
            Phase::Done => 0,
            Phase::Release => u64::from(self.shape.release - self.age),
            _ if self.shape.one_shot => {
                let e = self.shape;
                let later = match self.phase {
                    Phase::Delay => u64::from(e.attack) + u64::from(e.hold) + u64::from(e.decay),
                    Phase::Attack => u64::from(e.hold) + u64::from(e.decay),
                    Phase::Hold => u64::from(e.decay),
                    _ => 0,
                };
                u64::from(self.duration() - self.age) + later
            }
            _ => return usize::MAX,
        };
        usize::try_from(frames).unwrap_or(usize::MAX)
    }

    /// Only return levels that remain constant indefinitely: callers may skip clocks.
    pub(crate) fn constant_level(&self) -> Option<f32> {
        // Native destination interpolation must drain even after the source reaches sustain.
        if self.shape.native.is_some() {
            return None;
        }
        if self.phase == Phase::Sustain {
            return Some(self.shape.sustain);
        }
        if !self.shape.one_shot
            && self.shape.attack == 0
            && self.shape.sustain == 1.0
            && matches!(self.phase, Phase::Hold | Phase::Decay)
        {
            return Some(1.0);
        }
        None
    }

    /// Port v1 voice.rs::Envelope::run(None): advance without discarded levels.
    /// Keep v2's recurrence bits; a 64-frame anchor overwrites earlier steps.
    pub(crate) fn advance(&mut self, mut frames: u32) -> f32 {
        if let Some(native) = &mut self.shape.native {
            native.skip(frames as usize);
            return native.level();
        }
        while frames > 0 && !matches!(self.phase, Phase::Sustain | Phase::Done) {
            let step = frames.min(self.duration() - self.age);
            let curve = self.curve();
            if curve.curvature != 0.0 {
                let target = self.age + step;
                let anchor = target.min(self.duration() - 1) / 64 * 64;
                if anchor > self.age {
                    self.age = anchor;
                    self.progress = curve.at(self.age, self.duration());
                    self.delta = curve.delta
                        * (curve.curvature * (f64::from(self.age) / f64::from(self.duration())))
                            .exp();
                }
                for _ in self.age..target {
                    self.step();
                }
            } else {
                self.age += step;
                if self.age == self.duration() {
                    self.enter(self.following());
                }
            }
            frames -= step;
        }
        self.level()
    }

    pub(crate) fn next(&mut self) -> f32 {
        if let Some(native) = &mut self.shape.native {
            let mut out = [0.];
            native.render(&mut out);
            return out[0];
        }
        let value = self.level();
        if matches!(self.phase, Phase::Sustain | Phase::Done) {
            return value;
        }
        self.step();
        value
    }

    fn step(&mut self) {
        self.age += 1; // Strictly below the u32 duration.
        if self.age == self.duration() {
            self.enter(self.following());
        } else {
            let curve = self.curve();
            if curve.curvature != 0.0 {
                if self.age.is_multiple_of(64) {
                    // Anchor to the source clock, never the host block. Bounded
                    // recurrence spans avoid drift even for u32::MAX durations.
                    self.progress = curve.at(self.age, self.duration());
                    self.delta = curve.delta
                        * (curve.curvature * (f64::from(self.age) / f64::from(self.duration())))
                            .exp();
                } else {
                    self.progress += self.delta;
                    self.delta *= curve.multiplier;
                }
            }
        }
    }
}

/// A point a breakpoint envelope glides to from the previous level (0 at the
/// start) over `frames`, along `curve`.
#[derive(Clone, Copy, Debug)]
pub struct Breakpoint {
    pub frames: u32,
    pub level: f32,
    pub curve: EnvelopeCurve,
}

/// The multi-segment envelope: any point list is an envelope. It holds at
/// `points[sustain]` while gated; a release glides from the current level
/// through the points after `sustain` (jumping there if it is not reached yet),
/// then holds the last. While gated, reaching `loop_range.1` returns to
/// `loop_range.0` (a flex loop). AHDSR, DAHDSR, DBD and Flex are builders or
/// direct point lists, see [`Breakpoints::dahdsr`].
#[derive(Clone, Debug, Default)]
pub struct Breakpoints {
    pub points: Vec<Breakpoint>,
    pub sustain: Option<usize>,
    /// Inclusive (start, end) point indices; the cycle must span at least one frame.
    pub loop_range: Option<(usize, usize)>,
}

/// Optional second decay of a decay-break-decay shape: decay to `level`, then
/// over `frames` along `curve` to the sustain level.
#[derive(Clone, Copy, Debug)]
pub struct Break {
    pub level: f32,
    pub frames: u32,
    pub curve: EnvelopeCurve,
}

/// Every stage of the superset envelope; stages of zero frames vanish, so
/// ADSR is `delay = hold = 0`, AHD has `sustain = 0` with no gate hold in the
/// caller, DBD sets `brk`.
#[derive(Clone, Copy, Debug)]
pub struct Dahdsr {
    pub delay: u32,
    pub attack: u32,
    pub attack_curve: EnvelopeCurve,
    pub hold: u32,
    pub decay: u32,
    pub decay_curve: EnvelopeCurve,
    pub brk: Option<Break>,
    pub sustain: f32,
    pub release: u32,
    pub release_curve: EnvelopeCurve,
}

impl Breakpoints {
    /// Point list for a [`Dahdsr`]; the sustain index is the last decay point.
    pub fn dahdsr(d: &Dahdsr) -> Self {
        let linear = EnvelopeCurve::default();
        let mut points = Vec::with_capacity(7);
        if d.delay > 0 {
            points.push(Breakpoint { frames: d.delay, level: 0.0, curve: linear });
        }
        points.push(Breakpoint { frames: d.attack, level: 1.0, curve: d.attack_curve });
        if d.hold > 0 {
            points.push(Breakpoint { frames: d.hold, level: 1.0, curve: linear });
        }
        match d.brk {
            Some(b) => {
                points.push(Breakpoint { frames: d.decay, level: b.level, curve: d.decay_curve });
                points.push(Breakpoint { frames: b.frames, level: d.sustain, curve: b.curve });
            }
            None => points.push(Breakpoint { frames: d.decay, level: d.sustain, curve: d.decay_curve }),
        }
        let sustain = Some(points.len() - 1);
        points.push(Breakpoint { frames: d.release, level: 0.0, curve: d.release_curve });
        Self { points, sustain, loop_range: None }
    }

    /// Whether a prepared program may run this envelope: finite levels, valid
    /// indices, and a loop that consumes time (no zero-length spin).
    pub fn valid(&self) -> bool {
        let n = self.points.len();
        n <= u32::MAX as usize - 1
            && self.sustain.is_none_or(|s| s < n)
            && self.points.iter().all(|p| p.level.is_finite())
            && self.loop_range.is_none_or(|(a, b)| {
                a <= b
                    && b < n
                    && self.points[a..=b].iter().map(|p| u64::from(p.frames)).sum::<u64>() > 0
            })
    }
}

/// A voice's position in one breakpoint envelope.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Segment {
    /// The point being glided to; past the end, the last level holds.
    index: u32,
    age: u32,
    from: f32,
    level: f32,
    released: bool,
}

impl Segment {
    pub(crate) fn advance(&mut self, envelope: &Breakpoints, mut frames: u32) -> f32 {
        while let Some(point) = envelope.points.get(self.index as usize) {
            let left = point.frames - self.age;
            if frames < left {
                self.age += frames;
                let t = f64::from(self.age) / f64::from(point.frames);
                self.level = self.from + (point.level - self.from) * point.curve.value(t) as f32;
                return self.level;
            }
            frames -= left;
            self.age = point.frames;
            self.level = point.level;
            if !self.released && envelope.sustain == Some(self.index as usize) {
                return self.level;
            }
            match envelope.loop_range {
                Some((start, end)) if !self.released && self.index as usize == end => {
                    self.index = start as u32;
                }
                _ => self.index += 1,
            }
            self.age = 0;
            self.from = self.level;
        }
        self.level
    }

    pub(crate) fn release(&mut self, envelope: &Breakpoints) {
        if self.released {
            return;
        }
        self.released = true;
        if let Some(sustain) = envelope.sustain
            && self.index as usize <= sustain
        {
            self.index = sustain as u32 + 1;
            self.age = 0;
            self.from = self.level;
        }
    }

    /// Restart from the first point at the current level (retrigger); legato
    /// is simply not calling this.
    pub fn retrigger(&mut self) {
        *self = Self { from: self.level, level: self.level, ..Self::default() };
    }
}

#[cfg(test)]
thread_local! { static LEVEL_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod tests {
    use super::*;

    fn native_shape(ahd_only: bool) -> Envelope {
        Envelope::new(96, 32, 96, 0.3, 96).unwrap().with_native_primary(&sampler_ir::SourceAhdsr {
            group: 0, slot: 0, attack_ms: 2., attack_curve: -0.5,
            hold_ms: 1., decay_ms: 2., sustain: 0.3, release_ms: 2.,
            ahd_only, native_amplitude: true,
        }, 48000).unwrap()
    }

    #[test]
    fn native_retirement_bound_counts_the_last_interpolated_point() {
        for ahd in [false, true] {
            for offset in [0, 1, 7, 31, 32, 33, 87, 150] {
                let mut state = EnvelopeState::new(native_shape(ahd));
                state.advance(offset);
                if !ahd { state.release(); }
                let initial = state.remaining();
                assert!(initial < 1024);
                for remaining in (1..=initial).rev() {
                    assert!(!state.done(), "ahd={ahd} offset={offset} initial={initial} remaining={remaining}");
                    assert_eq!(state.remaining(), remaining, "ahd={ahd} offset={offset}");
                    state.next();
                }
                assert!(state.done());
                assert_eq!(state.remaining(), 0);
            }
        }
    }

    #[test]
    fn native_setters_fall_back_and_chokes_keep_the_current_level() {
        let shape = native_shape(false);
        for stage in crate::engine_parameters::ENVELOPE_STAGES {
            assert!(shape.with_stage(stage, 17).native.is_none());
            assert!(!EnvelopeState::new(shape.with_stage(stage, 17)).admits_v1_controls());
            assert!(shape.with_control(stage, 0.5).native.is_none());
            assert!(!EnvelopeState::new(shape.with_control(stage, 0.5)).admits_v1_controls());
        }
        let mut state = EnvelopeState::new(shape);
        state.advance(63);
        let current = state.current();
        state.choke(7);
        assert_eq!(state.next().to_bits(), current.to_bits());
        assert_eq!(state.remaining(), 6);
        state.choke(100);
        assert_eq!(state.remaining(), 6);
        state.advance(6);
        assert!(state.done());
        let mut releasing = EnvelopeState::new(shape);
        releasing.advance(80);
        releasing.release();
        let mut unchanged = releasing;
        releasing.choke(releasing.remaining() as u32 + 1);
        for _ in 0..128 { assert_eq!(releasing.next().to_bits(), unchanged.next().to_bits()); }
    }


    #[test]
    fn step_curves_survive_zero_duration_stage_edits() {
        let step = EnvelopeCurve::step();
        let mut shape = Envelope::new(2, 0, 2, 0.25, 2)
            .unwrap()
            .with_curves(step, step, step);
        for stage in [
            crate::EnvelopeStage::Attack,
            crate::EnvelopeStage::Decay,
            crate::EnvelopeStage::Release,
        ] {
            shape = shape.with_stage(stage, 0).with_stage(stage, 4);
        }
        assert!(shape.curves.iter().all(|curve| curve.step));
    }

    #[test]
    fn skipped_curves_do_not_compute_discarded_frame_levels() {
        let c = EnvelopeCurve::exponential(-4.).unwrap();
        let shape = Envelope::new(48000, 0, 48000, 0.3, 48000)
            .unwrap()
            .with_curves(c, c, c);
        let mut state = EnvelopeState::new(shape);
        LEVEL_READS.set(0);
        std::hint::black_box(state.advance(32));
        assert_eq!(
            LEVEL_READS.get(),
            1,
            "only the requested endpoint needs a level"
        );
    }

    #[test]
    fn skipped_envelopes_keep_scalar_output_and_state_bits() {
        for curvature in [-32., -4., -1e-320, 0., 1e-320, 4., 32., f64::INFINITY] {
            let c = if curvature.is_infinite() {
                EnvelopeCurve::step()
            } else {
                EnvelopeCurve::exponential(curvature).unwrap()
            };
            for frames in [0, 1, 7, 31, 32, 63, 64, 65, 127, 256, 513] {
                for off in [0, 17, 140, 260, 800] {
                    for one_shot in [false, true] {
                        let mut shape = Envelope::new(137, 5, 149, 0.25, 173)
                            .unwrap()
                            .with_delay(3)
                            .with_curves(c, c, c);
                        shape.one_shot = one_shot;
                        let mut scalar = EnvelopeState::new(shape);
                        let mut skip = scalar;
                        for at in (0..1200).step_by(frames.max(1) as usize) {
                            if at >= off {
                                scalar.release();
                                skip.release();
                            }
                            for _ in 0..frames {
                                scalar.next();
                            }
                            assert_eq!(skip.advance(frames).to_bits(), scalar.current().to_bits());
                            assert_eq!(skip.phase, scalar.phase);
                            assert_eq!(skip.age, scalar.age);
                            assert_eq!(skip.progress.to_bits(), scalar.progress.to_bits());
                            assert_eq!(skip.delta.to_bits(), scalar.delta.to_bits());
                            assert_eq!(
                                skip.release_level.to_bits(),
                                scalar.release_level.to_bits()
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn long_curves_keep_finite_monotone_endpoints_without_counter_overflow() {
        for k in [-32., -1e-320, 1e-320, 32.] {
            let curve = EnvelopeCurve::exponential(k).unwrap();
            let shape = Envelope::new(u32::MAX, 0, 0, 1., u32::MAX)
                .unwrap()
                .with_curves(curve, curve, curve);
            let mut state = EnvelopeState::new(shape);
            // Independently initialize a far-advanced valid clock/state. Iterating
            // four billion samples is unnecessary to exercise the final transitions.
            state.age = u32::MAX - 127;
            let t = f64::from(state.age) / f64::from(u32::MAX);
            state.progress = if k.abs() < 1e-8 {
                t
            } else {
                (k * t).exp_m1() / k.exp_m1()
            };
            state.delta = if k.abs() < 1e-8 {
                1. / f64::from(u32::MAX)
            } else {
                (k / f64::from(u32::MAX)).exp_m1() * (k * t).exp() / k.exp_m1()
            };
            let mut previous = 0.;
            for _ in 0..128 {
                let level = state.next();
                assert!(level.is_finite() && level >= previous && level <= 1.);
                previous = level;
            }
            assert_eq!(state.phase, Phase::Sustain);
            assert_eq!(state.next(), 1.);
            state.release();
            state.age = u32::MAX - 1;
            state.progress = 1.;
            assert_eq!(state.next(), 0.);
            assert!(state.done());
            assert_eq!(state.remaining(), 0);
            state.choke(100);
            state.release();
            assert!(state.done());
        }
    }

    fn bp_levels(b: &Breakpoints, release_at: Option<u32>, frames: u32) -> Vec<f32> {
        let mut seg = Segment::default();
        (1..=frames)
            .map(|n| {
                if Some(n) == release_at {
                    seg.release(b);
                }
                seg.advance(b, 1)
            })
            .collect()
    }

    fn dahdsr(delay: u32, c: EnvelopeCurve) -> Dahdsr {
        Dahdsr {
            delay,
            attack: 40,
            attack_curve: c,
            hold: 8,
            decay: 50,
            decay_curve: c,
            brk: None,
            sustain: 0.3,
            release: 60,
            release_curve: c,
        }
    }

    /// The point-list superset reproduces the verified stage envelope.
    #[test]
    fn dahdsr_points_match_the_stage_envelope() {
        for k in [0.0, -3.0, 2.5] {
            let c = EnvelopeCurve::exponential(k).unwrap();
            for delay in [0, 7] {
                let d = dahdsr(delay, c);
                let b = Breakpoints::dahdsr(&d);
                assert!(b.valid());
                let shape = Envelope::new(40, 8, 50, 0.3, 60)
                    .unwrap()
                    .with_delay(delay)
                    .with_curves(c, c, c);
                let mut stage = EnvelopeState::new(shape);
                let mut seg = Segment::default();
                for n in 1..=320u32 {
                    if n == 200 {
                        stage.release();
                        seg.release(&b);
                    }
                    let want = stage.advance(1);
                    let got = seg.advance(&b, 1);
                    assert!((want - got).abs() < 2e-6, "k={k} delay={delay} n={n} {want} {got}");
                }
            }
        }
    }

    #[test]
    fn break_adds_a_second_decay_before_sustain() {
        let mut d = dahdsr(0, EnvelopeCurve::default());
        d.attack = 0;
        d.hold = 0;
        d.decay = 10;
        d.brk = Some(Break { level: 0.8, frames: 20, curve: EnvelopeCurve::default() });
        let b = Breakpoints::dahdsr(&d);
        let l = bp_levels(&b, None, 40);
        assert!((l[9] - 0.8).abs() < 1e-6, "break level {}", l[9]);
        assert!((l[19] - 0.55).abs() < 1e-6);
        assert!((l[29] - 0.3).abs() < 1e-6);
        assert_eq!(l[39], 0.3);
    }

    #[test]
    fn gated_loop_cycles_until_release() {
        let p = |frames, level| Breakpoint { frames, level, curve: EnvelopeCurve::default() };
        let b = Breakpoints {
            points: vec![p(4, 1.0), p(4, 0.0), p(4, 0.0)],
            sustain: None,
            loop_range: Some((0, 1)),
        };
        let l = bp_levels(&b, Some(17), 24);
        assert_eq!(l[3], 1.0);
        assert_eq!(l[7], 0.0);
        assert_eq!(l[11], 1.0, "second pass peaks again");
        assert_eq!(l[15], 0.0);
        // Released at 17: continues past the loop end and stays at the last point's level.
        assert_eq!(l[23], 0.0);
        let spin = Breakpoints { points: vec![p(0, 1.0)], sustain: None, loop_range: Some((0, 0)) };
        assert!(!spin.valid(), "a zero-length loop would spin");
    }

    #[test]
    fn retrigger_restarts_from_the_current_level() {
        let d = dahdsr(0, EnvelopeCurve::default());
        let b = Breakpoints::dahdsr(&d);
        let mut seg = Segment::default();
        let held = seg.advance(&b, 20);
        assert!(held > 0.4 && held < 0.6);
        seg.retrigger();
        let first = seg.advance(&b, 1);
        assert!(first > held && first < held + 0.05, "{held} -> {first}");
        let end = seg.advance(&b, 40);
        assert_eq!(end, 1.0);
    }
}
