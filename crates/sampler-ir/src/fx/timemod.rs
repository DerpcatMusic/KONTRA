//! `timemod` family: the one configuration every delay and modulation effect
//! (delay, ping-pong, tape echo, chorus, flanger, ensemble, vibrato, phaser,
//! rotary) is expressed in. It describes a modulated multi-tap delay line and
//! an allpass network, either or both, around a shared LFO. Formats only fill
//! this in; the engine lives in `sampler_core::dsp::timemod`.
//!
//! Laws marked "native" are reproduced from the Kontakt binary (addresses in the
//! engine); everything else is a generic law and a format that uses it must
//! report the mapping as generic, not exact.

pub const MAX_TAPS: usize = 4;
pub const MAX_VOICES: usize = 8;
/// UVI Phasor runs `2 * order` stages, order up to 12.
pub const MAX_STAGES: usize = 24;

/// How a fractional delay read interpolates between ring samples.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interpolation {
    /// Round to the nearest frame (tape-less digital delay, integer delays).
    Nearest,
    Linear,
    /// 4-point cubic Hermite (Catmull-Rom).
    Hermite,
    /// 4-point third-order Lagrange.
    Lagrange,
    /// The frame at or before the delay (UVI ring read mode 0, `0x1416485a0`).
    Floor,
    /// The frame at or after the delay (UVI TrackDelay's `ceil(seconds·rate)`).
    Ceil,
}

/// A tap's base delay.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DelayTime {
    Frames(u32),
    Ms(f64),
    /// Quarter-note beats at the host tempo (clamped to the line capacity).
    Beats(f64),
}

/// One read head of the line. Output is `level` times the head's signal,
/// balanced by `pan` (-1 left .. 1 right; 0 is unity on both channels).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tap {
    pub time: DelayTime,
    pub level: f64,
    pub pan: f64,
    /// Peak LFO excursion of the delay time in ms (0 = unmodulated).
    pub depth_ms: f64,
}

impl Tap {
    pub const fn at(time: DelayTime) -> Self {
        Self { time, level: 1., pan: 0., depth_ms: 0. }
    }
}

/// The recirculating path, applied to the summed voices of tap `source`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feedback {
    /// Rows are line input L/R, columns delayed L/R: diagonal is plain
    /// feedback, off-diagonal is cross / ping-pong. Each absolute row sum must
    /// stay below one.
    pub matrix: [[f64; 2]; 2],
    pub source: u8,
    /// One-pole low-pass coefficient in the loop (0 = off, 1 = pass).
    pub lowpass: f64,
    /// One-pole high-pass pole in the loop (0 = off).
    pub highpass: f64,
    /// Soft saturation in the loop (0 = off); unity small-signal gain.
    pub drive: f64,
}

impl Feedback {
    pub const OFF: Self = Self { matrix: [[0.; 2]; 2], source: 0, lowpass: 0., highpass: 0., drive: 0. };
}

/// A native Kontakt delay-line kernel that replaces the generic line
/// recursion. Everything else in [`Line`] is ignored while one is set.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
// ponytail: the voice bank keeps ~1 KB inline so the IR stays `Copy`; the
// engine reads natives by reference. Box it if IR size starts to matter.
#[allow(clippy::large_enum_variant)]
pub enum Native {
    None,
    /// Kontakt Legacy Delay (0x10): stereo ping-pong ring of 2^18 frames,
    /// audio 0x140aa1130, setter 0x1409645ae. `time` is the stored value (ms);
    /// when `unit` > 1e-9 the delay is `unit * 60 / tempo * time * 1000` ms
    /// (getter 0x140a74bf0).
    LegacyDelay { time: f32, unit: f32, feedback: f32, damping: f32, pan: f32 },
    /// Kontakt legacy modulated delay, shared by Legacy Flanger (0x12, audio
    /// 0x140aa2350, setter 0x1409637f0) and Legacy Chorus (0x11, audio
    /// 0x140aa0510, setter 0x140963970): a 4096-frame ring (prepare
    /// 0x140960f00) read with 4-point Lagrange `base + 1 - lfo * excursion`
    /// frames behind the write, where `base = (int)(base_ms * rate * 0.001)`
    /// and the excursion is pulled to `base - 1` when it reaches `base`.
    /// Rate and the right channel's phase come from [`Config::lfo`] (phase
    /// already scaled by 0.25).
    LegacyModDelay {
        base_ms: f32,
        /// LFO excursion in frames before the clamp (Flanger `depth * 50`,
        /// Chorus `depth * 75`).
        excursion: f64,
        /// Head output subtracted from the ring write (Flanger `feedback * 0.95`).
        feedback: f32,
        /// Chorus: a second head at `base >> 1` on its own LFO 0.015 cycles
        /// ahead, summed with the first.
        second_head: bool,
        /// One-pole `y += (head - y) * k` on each head (Chorus 0.95; 0 = off).
        smoothing: f32,
    },
    /// A bank of [`BANK_VOICES`] modulated lines run in place of the single
    /// ring: the voice-bank form of this line (Kontakt Choral is one
    /// configuration of it). The bank owns its dry/wet.
    VoiceBank(VoiceBank),
    /// UVI DualDelay / DualDelayX core (Workstation 4.0.9 kernel
    /// `0x14165c960`); the line's `interpolation` is its read mode and the
    /// unit's dry/wet its equal-power mix.
    UviDualDelay(DualDelay),
    /// UVI legacy SimpleDelay / StereoDelay / PingPongDelay (Workstation
    /// 4.0.9); the unit's dry/wet is the linear mix.
    UviLegacyDelay(UviLegacyDelay),
    /// UVI DiffuseDelay (Workstation 4.0.9 core `0x141657890`); the unit's
    /// dry/wet is the equal-power mix.
    UviDiffuseDelay(UviDiffuseDelay),
    /// UVI TapeEcho (Workstation 4.0.9 core `0x141663e40`) without its
    /// reverb; the unit's dry/wet is the equal-power mix.
    UviTapeEcho(UviTapeEcho),
    /// Kontakt Twin Delay (0x6B; kernel `0x140ab07d0`): wet output only, the
    /// unit's dry is the slot's.
    TwinDelay(TwinDelay),
    /// Kontakt Psyche Delay (0x63; kernel `0x140aadb50`): wet output only.
    PsycheDelay(PsycheDelay),
    /// UVI Velvet Delay (`DiffuseDelayNested`, kernel `0x14165af50`); the
    /// unit's dry/wet is the linear mix.
    UviVelvetDelay(UviVelvetDelay),
    /// UVI analog tape delay (`Buzz`, process `0x14166ab40`); the unit's
    /// dry/wet is `Dry`/`Wet`.
    UviBuzzDelay(UviBuzzDelay),
}

/// UVI Velvet Delay controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UviVelvetDelay {
    /// Seconds, or beats when synced.
    pub time: f32,
    pub synced: bool,
    pub spread: f32,
    pub diffusion: f32,
    pub seed: u32,
    /// 0..=6: ramp, uniform, exp, ramp down, parabolic, inverse, parametric.
    pub shape: u8,
    pub curve: f32,
}

/// UVI `Buzz` analog tape delay controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UviBuzzDelay {
    /// Seconds, or beats when synced.
    pub time: f32,
    pub synced: bool,
    pub feedback: f32,
    pub trash: f32,
    pub spread: f32,
    /// Seconds.
    pub glide: f32,
    /// 0..=2.
    pub mode: u8,
}

/// Kontakt Psyche Delay saved controls (reader `0x140ac5bc0`).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PsycheDelay {
    /// Free: milliseconds (5..2000). Synced: the note count.
    pub time: f32,
    /// Sync unit factor (≤ 0: free).
    pub unit: f32,
    pub feedback: f32,
    pub detune: f32,
    pub pitch: f32,
    /// Stereo offset: the right side's time is `time·(1 - offset/2)`.
    pub offset: f32,
    pub cross: f32,
    pub reverse_stereo: bool,
    pub detune_stereo: bool,
    pub reverse: bool,
}

/// Kontakt Twin Delay saved controls (all normalized `0..=1` unless synced).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TwinDelay {
    /// Left, right: free time `x` (seconds `0.001 + x²·1.999`), or the
    /// synced count when `time_unit` is set.
    pub time: [f32; 2],
    /// Left, right: free predelay `x` (seconds `2x`), or the synced count.
    pub predelay: [f32; 2],
    /// Sync unit factors (≤ 0: free) for time L/R and predelay L/R.
    pub time_unit: [f32; 2],
    pub predelay_unit: [f32; 2],
    pub feedback: [f32; 2],
    pub level: [f32; 2],
    pub width: f32,
    pub cross: f32,
}

/// UVI TapeEcho controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UviTapeEcho {
    pub speed: f32,
    pub feedback: f32,
    /// 1..=12: which heads play (5..=12 also feed the reverb).
    pub mode: u8,
    pub bass_db: f32,
    pub treble_db: f32,
    /// Head delays in seconds, or quarter-note beats when synced.
    pub delays: [f32; 3],
    pub synced: bool,
    pub noise: f32,
    pub tape_stop: bool,
}

/// UVI DiffuseDelay controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UviDiffuseDelay {
    /// Seconds, or quarter-note beats when synced.
    pub time: f32,
    pub synced: bool,
    pub spread: f32,
    pub diffusion: f32,
    pub feedback: f32,
    pub damp: f32,
    pub crosstalk: f32,
    pub randomness: f32,
    pub mod_rate: f32,
    /// Cents.
    pub mod_depth: f32,
}

/// Which UVI legacy delay node.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UviLegacyKind {
    Simple,
    Stereo,
    PingPong,
}

/// UVI legacy delay controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UviLegacyDelay {
    pub kind: UviLegacyKind,
    /// Left and right seconds, or quarter-note beats when synced (Simple and
    /// PingPong use the left).
    pub time: [f32; 2],
    pub synced: bool,
    pub feedback: f32,
    /// Loop filter, `-1..=1`: negative low-pass, positive high-pass, 0 off.
    pub cutoff: f32,
    /// PingPong input split, `-1..=1` (-1 all into the left line).
    pub spread: f32,
}

pub const BANK_VOICES: usize = 7;

/// Voices of a [`Native::VoiceBank`], all in physical units (seconds, Hz) so
/// the engine resolves them at its own rate. Per sample (Kontakt core audio
/// 0x14193be50): voice `j` takes `invert * feedback_j * M_j + input * x`,
/// where `M_j = sum_k y_k * scatter[(k - j) mod 7]` over last outputs `y`;
/// filters it (TPT SVF, k = sqrt 2, `(1 - highpass) lp + highpass hp`) into
/// its ring and reads it back with Catmull-Rom at
/// `rate * (delay_s + (rate / lfo_hz) * excursion * m)` frames; `m` is the
/// smoothed `(2 - (1 - l1) shape)(l1 + 1) / 4 + l2` of a triangle `l1`
/// (phase + `offset`) and a rounded-sine `l2` at `lfo_hz * lfo2_ratio`
/// scaled by `lfo2` (0 = off). The output sums `gain_j * rotation_j *
/// (invert * y_j + in_j)`, low-shelves it and mixes `wet * W + dry * x`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoiceBank {
    pub voices: [BankVoice; BANK_VOICES],
    pub scatter: [f64; BANK_VOICES],
    /// 1 or -1.
    pub invert: f64,
    pub input: f64,
    pub wet: f64,
    pub dry: f64,
    /// Low shelf on the sum: pre-warp corner (Hz) and gain.
    pub shelf_hz: f64,
    pub shelf_gain: f64,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BankVoice {
    pub gain: f64,
    pub feedback: f64,
    /// `[a, b, c, d]`: L gets `a sL + b sR`, R gets `c sL + d sR`.
    pub rotation: [f64; 4],
    pub delay_s: f64,
    /// Half the pitch-ratio excursion, `(ratio - 1) / 2`.
    pub excursion: f64,
    pub lfo_hz: f64,
    pub lfo2_ratio: f64,
    pub lfo2: f64,
    pub offset: f64,
    pub shape: f64,
    pub smoothing: f64,
    pub filter_hz: f64,
    pub highpass: f64,
}

/// UVI DualDelay controls in native units.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualDelay {
    /// Seconds, or quarter-note beats when synced; at most 5 s either way.
    pub time: f32,
    pub synced: bool,
    /// DelayRatio / FeedbackRatio, `-1..=1`: shorten / weaken one side.
    pub delay_ratio: f32,
    pub feedback: f32,
    pub feedback_ratio: f32,
    /// Loop one-poles; off (DualDelayX Filtering off) skips both.
    pub filters: bool,
    pub low_cut_hz: f32,
    pub high_cut_hz: f32,
    /// Loop rotation in degrees.
    pub rotation: f32,
    /// Input and output width and rotation (rotation in eighth turns).
    pub input: [f32; 2],
    pub output: [f32; 2],
    /// Peak delay-time modulation in cents, its rate and the right
    /// channel's phase offset in half cycles.
    pub mod_cents: f32,
    pub mod_hz: f32,
    pub mod_offset: f32,
    /// DualDelayX's extra loop stages (`None`: plain DualDelay).
    pub x: Option<DualX>,
}

/// DualDelayX loop stages; each `Some` is switched on.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualX {
    /// CrusherFreq table index (0..=9), CrusherBits (4..=16).
    pub crusher: Option<(u8, u8)>,
    /// DispersionFreq Hz, DispersionSpread ms.
    pub dispersion: Option<(f32, f32)>,
    /// DiffusionAmount, DiffusionSpread ms.
    pub diffusion: Option<(f32, f32)>,
    /// TapeDrive, TapeWarmth.
    pub tape: Option<(f32, f32)>,
    pub reflection: bool,
}

/// Modulated multi-tap delay line.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub taps: [Tap; MAX_TAPS],
    pub tap_count: u8,
    /// Copies of every tap read at LFO phases spread over `voice_phase`
    /// cycles and panned over `voice_spread` (ensemble / chorus voices).
    pub voices: u8,
    pub voice_phase: f64,
    pub voice_spread: f64,
    /// Voice `v` reads `v * voice_delay_ms` later than the tap (UVI Chorus 4 ms).
    pub voice_delay_ms: f64,
    pub interpolation: Interpolation,
    /// Rows are line-input L/R, columns signal L/R (identity = stereo,
    /// `[[1,1],[0,0]]` style rows feed one side, e.g. ping-pong).
    pub input: [[f64; 2]; 2],
    pub feedback: Feedback,
    /// Ring capacity; tempo-synced and modulated reads clamp to it.
    pub max_ms: f64,
    /// Right-channel delay times are the left ones times this (1 = equal).
    pub right_scale: f64,
    pub native: Native,
}

impl Line {
    pub const IDENTITY_INPUT: [[f64; 2]; 2] = [[1., 0.], [0., 1.]];

    /// A single unmodulated tap.
    pub fn single(time: DelayTime, max_ms: f64) -> Self {
        Self {
            taps: [Tap::at(time); MAX_TAPS],
            tap_count: 1,
            voices: 1,
            voice_phase: 0.,
            voice_spread: 0.,
            voice_delay_ms: 0.,
            interpolation: Interpolation::Linear,
            input: Self::IDENTITY_INPUT,
            feedback: Feedback::OFF,
            max_ms,
            right_scale: 1.,
            native: Native::None,
        }
    }
}

/// How the allpass coefficient is produced.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SweepLaw {
    /// Kontakt Legacy Phaser (native): exponential polynomial of `lfo * depth`.
    Legacy { depth: f32 },
    /// Kontakt Phaser filter (native): static coefficient from normalized cutoff.
    Filter { cutoff: f32 },
    /// Generic: log sweep between two frequencies by `lfo * depth` (0..1),
    /// first-order coefficients refreshed every 32 frames.
    Sweep { min_hz: f32, max_hz: f32, depth: f32 },
    /// UVI Phaser / Cross Phaser (native, 0x1412f0ec0 / 0x1412e4c90): per
    /// sample `w = (lfo + 1) * (wmax - wmin) / 2 + wmin` with `w = 2 f / rate`
    /// (no tan warp), coefficient `(1 - w) / (1 + w)`.
    Linear { min_hz: f32, max_hz: f32 },
    /// UVI Phasor (native, 0x14166c120): per sample the stage delay
    /// `d = hi * (lo / hi)^m`, `m = (lfo + 1) / 2`, `hi = rate / max_hz /
    /// order`, `lo = rate / min_hz / order`; coefficient `(1 - d) / (1 + d)`
    /// in UVI's `y = a u + s` stage form (negated for the shared stage).
    DelayDomain { min_hz: f32, max_hz: f32, order: u8 },
}

/// Cascade of first-order allpass stages (phaser).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Allpass {
    pub stages: u8,
    pub law: SweepLaw,
    /// Last-stage output fed back to the cascade input.
    pub feedback: f32,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LfoShape {
    /// Native leapfrog oscillator (no per-sample trig).
    Sine,
    Triangle,
    Saw,
    Square,
    /// UVI table LFO (uvi-dsp-laws #1, tables 0x14163c3c0): 32-bit phase,
    /// 257-point table read linearly at `phase >> 24`, value read before the
    /// phase advances. `direct` selects the increment `2^32 / rate * hz`
    /// (Flanger, Phaser) over `hz * 256 / rate * 2^24` (Chorus, Phasor).
    Uvi { wave: UviWave, direct: bool },
}

/// UVI LFO tables (0x14283abf8 + 8 * shape).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UviWave {
    Sine,
    Square,
    Triangle,
    RampUp,
    RampDown,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rate {
    Hz(f32),
    /// Native Kontakt synced getter: `1 / ((unit * 60 / tempo) * speed)` Hz,
    /// speed clamped to at least 1, result clamped to 0..=8 Hz and tempo
    /// below 0.1 BPM read as 120.
    Sync { unit: f32, speed: f32 },
    /// UVI host sync: `tempo / 60 / beats` Hz, at most `max_hz`.
    Beats { beats: f32, max_hz: f32 },
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lfo {
    pub shape: LfoShape,
    pub rate: Rate,
    /// Right-channel phase offset in cycles (the left channel starts at zero).
    pub phase: f32,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    LineThenAllpass,
    AllpassThenLine,
}

/// The universal delay / modulation effect.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub line: Option<Line>,
    pub allpass: Option<Allpass>,
    pub order: Order,
    pub lfo: Lfo,
    /// Output is `dry * input + wet * processed`.
    pub dry: f64,
    pub wet: f64,
}

impl Config {
    /// Plain integer-frame stereo delay with a feedback matrix (the original
    /// `Delay` stage).
    pub fn delay(frames: u32, feedback: [[f64; 2]; 2], dry: f64, wet: f64) -> Self {
        let mut line = Line::single(DelayTime::Frames(frames), 0.);
        line.interpolation = Interpolation::Nearest;
        line.feedback.matrix = feedback;
        Self { line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry, wet }
    }

    /// Frames of line the host must keep ringing after the input ends, when
    /// nothing recirculates (0 when it does).
    pub fn tail_ms(&self) -> f64 {
        match self.line {
            Some(l) if l.native != Native::None => 0.,
            Some(l) if l.feedback.matrix == [[0.; 2]; 2] => l.max_ms * l.right_scale.max(1.),
            _ => 0.,
        }
    }

    /// The first out-of-range field, if any.
    pub fn check(&self) -> Result<(), &'static str> {
        let fin = |v: f64, name| if v.is_finite() { Ok(()) } else { Err(name) };
        fin(self.dry, "dry")?;
        fin(self.wet, "wet")?;
        if let Rate::Hz(h) = self.lfo.rate
            && !(h.is_finite() && (0. ..=200.).contains(&h))
        {
            return Err("lfo rate");
        }
        if let Rate::Sync { unit, speed } = self.lfo.rate
            && !(unit.is_finite() && unit > 0. && speed.is_finite() && speed >= 0.)
        {
            return Err("lfo sync");
        }
        if let Rate::Beats { beats, max_hz } = self.lfo.rate
            && !(beats.is_finite() && beats > 0. && max_hz.is_finite() && max_hz > 0. && max_hz <= 200.)
        {
            return Err("lfo beats");
        }
        if !self.lfo.phase.is_finite() {
            return Err("lfo phase");
        }
        if self.line.is_none() && self.allpass.is_none() {
            return Err("no section");
        }
        if let Some(l) = &self.line {
            if !(1..=MAX_TAPS as u8).contains(&l.tap_count) {
                return Err("tap count");
            }
            if !(1..=MAX_VOICES as u8).contains(&l.voices) {
                return Err("voices");
            }
            if !(l.max_ms.is_finite() && l.max_ms >= 0. && l.max_ms <= 20_000.) {
                return Err("max_ms");
            }
            if !(0.1..=10.).contains(&l.right_scale) {
                return Err("right_scale");
            }
            if let Native::LegacyDelay { time, unit, feedback, damping, pan } = l.native {
                let unit_interval = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                if !(time.is_finite() && time >= 0. && unit.is_finite() && unit_interval(feedback) && unit_interval(damping) && unit_interval(pan)) {
                    return Err("legacy delay");
                }
            }
            if let Native::UviDualDelay(d) = l.native {
                let all = [d.time, d.delay_ratio, d.feedback, d.feedback_ratio, d.low_cut_hz, d.high_cut_hz, d.rotation, d.mod_cents, d.mod_hz, d.mod_offset];
                if !(all.iter().chain(&d.input).chain(&d.output).all(|v| v.is_finite())
                    && d.time >= 0.
                    && d.delay_ratio.abs() <= 1.
                    && d.feedback_ratio.abs() <= 1.
                    && (0. ..=1.).contains(&d.feedback)
                    && d.low_cut_hz > 0.
                    && d.high_cut_hz > 0.
                    && d.mod_hz > 0.
                    && d.mod_cents >= 0.)
                {
                    return Err("uvi dual delay");
                }
                if let Some(x) = d.x {
                    let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                    if !(x.crusher.is_none_or(|(i, b)| i <= 9 && (4..=16).contains(&b))
                        && x.dispersion.is_none_or(|(f, s)| ok(f, 0., 20_000.) && ok(s, 1., 100.))
                        && x.diffusion.is_none_or(|(a, s)| ok(a, 0., 1.) && ok(s, 1., 100.))
                        && x.tape.is_none_or(|(d, w)| ok(d, 0., 1.) && ok(w, 0., 1.)))
                    {
                        return Err("uvi dual delay x");
                    }
                }
            }
            if let Native::UviLegacyDelay(d) = l.native {
                let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                if !(d.time.iter().all(|&t| ok(t, 0., 128.)) && ok(d.feedback, 0., 1.) && ok(d.cutoff, -1., 1.) && ok(d.spread, -1., 1.)) {
                    return Err("uvi legacy delay");
                }
            }
            if let Native::UviVelvetDelay(v) = l.native {
                let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                if !(ok(v.time, 0., 128.) && ok(v.spread, 0.1, 1.) && ok(v.diffusion, 0., 1.) && v.shape <= 6 && ok(v.curve, -1., 1.)) {
                    return Err("uvi velvet delay");
                }
            }
            if let Native::UviBuzzDelay(b) = l.native {
                let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                if !(ok(b.time, 0., 128.) && ok(b.feedback, 0., 1.) && ok(b.trash, 0., 1.) && ok(b.spread, 0., 2.) && ok(b.glide, 0.01, 1.) && b.mode <= 2) {
                    return Err("uvi buzz delay");
                }
            }
            if let Native::PsycheDelay(p) = l.native {
                let unit = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                if !([p.feedback, p.detune, p.pitch, p.offset, p.cross].into_iter().all(unit) && p.unit.is_finite() && p.time.is_finite() && (0. ..=2000.).contains(&p.time)) {
                    return Err("psyche delay");
                }
            }
            if let Native::TwinDelay(t) = l.native {
                let all = [t.time, t.predelay, t.time_unit, t.predelay_unit, t.feedback, t.level];
                let unit = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                if !(all.iter().flatten().all(|v| v.is_finite())
                    && t.feedback.iter().chain(&t.level).all(|&v| unit(v))
                    && unit(t.width)
                    && unit(t.cross)
                    && t.time.iter().chain(&t.predelay).all(|&v| (0. ..=64.).contains(&v)))
                {
                    return Err("twin delay");
                }
            }
            if let Native::UviTapeEcho(t) = l.native {
                let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                if !(ok(t.speed, 1., 3.)
                    && ok(t.feedback, 0., 2.)
                    && (1..=12).contains(&t.mode)
                    && ok(t.bass_db, -10., 10.)
                    && ok(t.treble_db, -10., 10.)
                    && t.delays.iter().all(|&d| ok(d, 0., 128.))
                    && ok(t.noise, 0., 0.001))
                {
                    return Err("uvi tape echo");
                }
            }
            if let Native::UviDiffuseDelay(d) = l.native {
                let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
                if !(ok(d.time, 0.001, 128.)
                    && ok(d.spread, 0., 1.)
                    && ok(d.diffusion, 0., 1.)
                    && ok(d.feedback, 0., 1.)
                    && ok(d.damp, 0., 1.)
                    && ok(d.crosstalk, -1., 1.)
                    && ok(d.randomness, 0., 0.5)
                    && ok(d.mod_rate, 0.1, 10.)
                    && ok(d.mod_depth, 0., 20.))
                {
                    return Err("uvi diffuse delay");
                }
            }
            if let Native::LegacyModDelay { base_ms, excursion, feedback, smoothing, .. } = l.native {
                let unit_interval = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                if !(base_ms.is_finite()
                    && (0. ..=5.).contains(&base_ms)
                    && excursion.is_finite()
                    && (0. ..=75.).contains(&excursion)
                    && unit_interval(feedback)
                    && unit_interval(smoothing))
                {
                    return Err("legacy modulated delay");
                }
            }
            if let Native::VoiceBank(bank) = &l.native {
                let v = &bank.voices;
                let finite = v.iter().flat_map(|v| [v.gain, v.feedback, v.delay_s, v.excursion, v.lfo2, v.offset, v.shape, v.smoothing, v.highpass].into_iter().chain(v.rotation))
                    .chain(bank.scatter)
                    .chain([bank.invert, bank.input, bank.wet, bank.dry])
                    .all(f64::is_finite);
                let positive = v.iter().flat_map(|v| [v.lfo_hz, v.lfo2_ratio, v.filter_hz]).chain([bank.shelf_hz, bank.shelf_gain]).all(|x| x.is_finite() && x > 0.);
                let sane = v.iter().all(|v| v.delay_s >= 0. && (0. ..=1.).contains(&v.smoothing) && v.feedback.abs() < 1.);
                if !(finite && positive && sane) {
                    return Err("voice bank");
                }
            }
            if !(l.voice_delay_ms.is_finite() && l.voice_delay_ms >= 0.) {
                return Err("voice delay");
            }
            for v in [l.voice_phase, l.voice_spread, l.feedback.drive] {
                fin(v, "line")?;
            }
            if !(0. ..=1.).contains(&l.feedback.lowpass) || !(0. ..1.).contains(&l.feedback.highpass) {
                return Err("feedback filter");
            }
            if l.feedback.source >= l.tap_count {
                return Err("feedback source");
            }
            if l.feedback.matrix.iter().any(|r| r.iter().any(|v| !v.is_finite()) || r[0].abs() + r[1].abs() >= 1.)
                || l.input.iter().flatten().any(|v| !v.is_finite())
            {
                return Err("feedback matrix");
            }
            for t in &l.taps[..l.tap_count as usize] {
                fin(t.level, "tap level")?;
                fin(t.pan, "tap pan")?;
                if !(t.depth_ms.is_finite() && t.depth_ms >= 0.) {
                    return Err("tap depth");
                }
                let base = match t.time {
                    DelayTime::Frames(0) => return Err("tap time"),
                    DelayTime::Frames(_) | DelayTime::Beats(_) => 0.,
                    DelayTime::Ms(ms) if ms.is_finite() && ms >= 0. => ms,
                    DelayTime::Ms(_) => return Err("tap time"),
                };
                if let DelayTime::Beats(b) = t.time
                    && !(b.is_finite() && b > 0.)
                {
                    return Err("tap beats");
                }
                let spread = f64::from(l.voices - 1) * l.voice_delay_ms;
                if base + t.depth_ms + spread > l.max_ms && !matches!(t.time, DelayTime::Frames(_)) {
                    return Err("tap beyond max_ms");
                }
            }
        }
        if let Some(a) = &self.allpass {
            if !(1..=MAX_STAGES as u8).contains(&a.stages) {
                return Err("stages");
            }
            if !(a.feedback.is_finite() && a.feedback.abs() < 1.) {
                return Err("allpass feedback");
            }
            let ok = match a.law {
                SweepLaw::Legacy { depth } => depth.is_finite() && (0. ..=1.).contains(&depth),
                SweepLaw::Filter { cutoff } => cutoff.is_finite() && (0. ..=1.).contains(&cutoff),
                SweepLaw::Sweep { min_hz, max_hz, depth } => {
                    min_hz.is_finite() && max_hz.is_finite() && min_hz > 0. && max_hz >= min_hz && (0. ..=1.).contains(&depth)
                }
                SweepLaw::Linear { min_hz, max_hz } => min_hz.is_finite() && max_hz.is_finite() && min_hz > 0. && max_hz > 0.,
                SweepLaw::DelayDomain { min_hz, max_hz, order } => {
                    min_hz.is_finite() && max_hz.is_finite() && min_hz > 0. && max_hz > 0. && (1..=12).contains(&order)
                }
            };
            if !ok {
                return Err("allpass law");
            }
        }
        Ok(())
    }
}
