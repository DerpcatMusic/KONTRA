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
    /// Lowpass and highpass in series on the recirculated signal, before the matrix.
    pub cut: Option<Cut>,
}

impl Feedback {
    pub const OFF: Self = Self { matrix: [[0.; 2]; 2], source: 0, lowpass: 0., highpass: 0., drive: 0., cut: None };
}

/// A highpass and a lowpass in series, in Hz; `None` bypasses a stage. Realised as TPT
/// state-variable sections (sampler-core `dsp::tpt_svf`).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    pub highpass_hz: Option<f32>,
    pub lowpass_hz: Option<f32>,
}

/// Log sweep `lo (hi / lo)^v` over a 0..1 control. Kontakt Replika LOWCUT uses 10 Hz to 1 kHz
/// (`10^(2v+1)`, decompile `141abe6a0`).
pub fn log_sweep_hz(v: f32, lo: f32, hi: f32) -> f32 {
    lo * (hi / lo).powf(v)
}

/// Linear sweep `lo + (hi - lo) v` over a 0..1 control. Kontakt Replika HIGHCUT uses 200 Hz to
/// 20 kHz (`200 + 19800 v`, decompile `141abe820`).
pub fn linear_sweep_hz(v: f32, lo: f32, hi: f32) -> f32 {
    lo + (hi - lo) * v
}

/// Kontakt 8 Replika (`NI::DSP::FXFDelayLight`) controls the core runs (setter indices in
/// `docs/re/engine/k-replika-binding.md`). Each is a stored value in 0..=1 (the wrapper's clamp). The
/// reset values of the other setters are the core's (sampler-core `dsp::replika`).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplikaControls {
    /// Index 0 and 1: input level A and B (reset 0.762405 = unity).
    pub input_a: f32,
    pub input_b: f32,
    /// Index 2 (LOWCUT, highpass 10 Hz to 1 kHz on a log law; off at 0) and 3 (HIGHCUT, lowpass 200 Hz to 20 kHz; off at 1).
    pub lowcut: f32,
    pub highcut: f32,
    /// Index 4 (dry/wet balance; reset 1.0 = wet only), 5 (FEEDBACK, head 2 and 3 gain 1.2 x value).
    pub dry_wet: f32,
    pub feedback: f32,
    /// Index 8 (TIME, mode 3: 2000 ms x value), 15 (delay offset, -600 frames per unit at 48 kHz).
    pub time: f32,
    pub delay_offset: f32,
    /// Index 13 and 14 (dry and wet pan pairs; reset 0.5), 16 (WIDTH, side scale; reset 0.5).
    pub pan_dry: f32,
    pub pan_wet: f32,
    pub width: f32,
    /// Index 29 (SATURATION, amount 0..1; 0 = off).
    pub saturation: f32,
    /// Index 52 (tap gain; implicit 1.0 at reset).
    pub tap_gain: f32,
    /// Index 17 (ducking sense: threshold 1 - v on |mid|), 18 (ducking amount: target -60 v dB), 19 (ducking
    /// release: 200 + 7300 v ms). Reset 0 for all three (no duck). The saved-field tie is INFERRED.
    pub ducking_sense: f32,
    pub ducking_amount: f32,
    pub ducking_release: f32,
    /// Index 28 (path select: path k = min(4, trunc(5 v)); k = 1 is head mode 1). Reset 0 (path 0). The saved-field
    /// tie is INFERRED (sampler-kontakt maps the saved TYPE here, k-replika-gaps.md piece 3c part 5b).
    pub path: f32,
}

impl Default for ReplikaControls {
    /// The reset values of `1408ddbc0` (`144f38814` = 0.762405 for inputs, `144f38764` = 0.3 for index 15),
    /// with TIME at 0.02 and HIGHCUT off, the harness choices of the native vectors.
    fn default() -> Self {
        Self {
            input_a: 0.762_405,
            input_b: 0.762_405,
            lowcut: 0.,
            highcut: 1.,
            dry_wet: 1.,
            feedback: 0.,
            time: 0.02,
            delay_offset: 0.3,
            pan_dry: 0.5,
            pan_wet: 0.5,
            width: 0.5,
            saturation: 0.,
            tap_gain: 1.,
            ducking_sense: 0.,
            ducking_amount: 0.,
            ducking_release: 0.,
            path: 0.,
        }
    }
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
    /// Kontakt Wow / Flutter (0x6A): a generic tape-transport pitch wobble
    /// (`sampler_core::dsp::timemod::wowflutter`). Its laws are not verified
    /// against Kontakt; the mapping reports them as unknown.
    WowFlutter(WowFlutter),
    /// Kontakt 8 Replika Delay core (`NI::DSP::FXFDelayLight`, `k-replika-binding.md`): runs the controls
    /// in sampler-core `dsp::replika`. Natives ignore the generic line.
    Replika(ReplikaControls),
}

/// Wow / Flutter controls after the Kontakt control laws (`wow_flutter_laws` in
/// sampler-kontakt): wow `4 w²` and flutter `0.5 f²` depth multipliers, speed
/// `0.75 s + 0.05` rate multiplier, stereo `s²`. The mix is the line's wet gain (`sqrt(m)`).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WowFlutter {
    /// Depth multiplier, `0..=4`.
    pub wow: f32,
    /// Depth multiplier, `0..=0.5`.
    pub flutter: f32,
    /// Rate multiplier, `0.05..=0.8`, applied to both modulators' base rates.
    pub speed: f32,
    /// Right channel's modulation phase offset, in squared-control units `0..=1`.
    pub stereo: f32,
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
    /// Amount `a` (0..=1) of the in-loop saturator on the line input (Replika SATURATION):
    /// `(1 - a) x input + a x saturate(input)`, 0 = off. Generic lines only; natives ignore it.
    pub saturation: f32,
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
            saturation: 0.,
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
    /// The script slot whose `ENGINE_PAR_*` writes reach [`Config::lanes`].
    #[cfg_attr(feature = "cache", serde(default))]
    pub address: Option<crate::SlotAddress>,
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
        Self { address: None, line: Some(line), allpass: None, order: Order::LineThenAllpass, lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. }, dry, wet }
    }

    /// Kontakt filter type 13 (Phaser filter): four fixed allpass stages from the
    /// normalized cutoff and resonance, half dry, half wet. With `address`, its
    /// cutoff and resonance are lanes (`ENGINE_PAR_CUTOFF`/`_RESONANCE`, modulation).
    pub fn phaser_filter(cutoff: f32, resonance: f32, address: Option<crate::SlotAddress>) -> Self {
        Self {
            address,
            line: None,
            allpass: Some(Allpass { stages: 4, law: SweepLaw::Filter { cutoff }, feedback: (f64::from(resonance) * 0.85) as f32 }),
            order: Order::LineThenAllpass,
            lfo: Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. },
            dry: 0.5,
            wet: 0.5,
        }
    }

    /// Live knobs of a script-addressed Kontakt legacy Delay, Chorus, Flanger
    /// or Phaser, each holding its stored value (fx_legacy_delay_reverb_mod.tsv);
    /// [`Config::set`] applies the native setter. A synced time or speed has
    /// another law and no lane.
    pub fn lanes(&self) -> Vec<super::Lane> {
        use super::{Lane, Law, tables};
        use crate::ProcessorParameter as P;
        if self.address.is_none() {
            return Vec::new();
        }
        if self.uvi() {
            return self.uvi_lanes();
        }
        let speed = match self.lfo.rate {
            Rate::Hz(hz) => Some(f64::from(hz)),
            _ => None,
        };
        let speed = |name| speed.map(|hz| Lane::new(name, P::Speed, hz, Law::Exp { a: 0.05, r: 160. }));
        let phase = |name| Lane::new(name, P::Phase, f64::from(self.lfo.phase) * 4., Law::Identity);
        let mut lanes = Vec::new();
        match (self.line.map(|l| l.native), self.allpass) {
            (Some(Native::LegacyDelay { time, unit, feedback, damping, pan }), _) => {
                if unit <= f32::from_bits(0x3089705f) {
                    lanes.push(Lane::new("ENGINE_PAR_DL_TIME", P::Time, f64::from(time), Law::Table(&tables::DL_TIME)));
                }
                lanes.push(Lane::new("ENGINE_PAR_DL_FEEDBACK", P::Feedback, f64::from(feedback), Law::Identity));
                lanes.push(Lane::new("ENGINE_PAR_DL_DAMPING", P::Damping, f64::from(damping), Law::Identity));
                lanes.push(Lane::new("ENGINE_PAR_DL_PAN", P::Pan, f64::from(pan), Law::Identity));
            }
            (Some(Native::LegacyModDelay { second_head: true, excursion, .. }), _) => {
                lanes.push(Lane::new("ENGINE_PAR_CH_DEPTH", P::Depth, excursion / 75., Law::Identity));
                lanes.extend(speed("ENGINE_PAR_CH_SPEED"));
                lanes.push(phase("ENGINE_PAR_CH_PHASE"));
            }
            (Some(Native::LegacyModDelay { base_ms, excursion, feedback, .. }), _) => {
                lanes.push(Lane::new("ENGINE_PAR_FL_DEPTH", P::Depth, excursion / 50., Law::Identity));
                let stored = f64::from(feedback) / f64::from(f32::from_bits(0x3f733333));
                lanes.push(Lane::new("ENGINE_PAR_FL_FEEDBACK", P::Feedback, stored, Law::Identity));
                lanes.extend(speed("ENGINE_PAR_FL_SPEED"));
                lanes.push(phase("ENGINE_PAR_FL_PHASE"));
                lanes.push(Lane::new("ENGINE_PAR_FL_COLOR", P::Colour, f64::from(base_ms), Law::Linear { a: 0., b: 5. }));
            }
            (None, Some(Allpass { law: SweepLaw::Legacy { depth }, feedback, .. })) => {
                lanes.push(Lane::new("ENGINE_PAR_PH_DEPTH", P::Depth, f64::from(depth), Law::Identity));
                lanes.push(Lane::new("ENGINE_PAR_PH_FEEDBACK", P::Feedback, f64::from(feedback) / 0.85, Law::Identity));
                lanes.extend(speed("ENGINE_PAR_PH_SPEED"));
                lanes.push(phase("ENGINE_PAR_PH_PHASE"));
            }
            (None, Some(Allpass { law: SweepLaw::Filter { cutoff }, feedback, .. })) => {
                lanes.push(Lane::new("ENGINE_PAR_CUTOFF", P::Cutoff, f64::from(cutoff), Law::Identity));
                lanes.push(Lane::new("ENGINE_PAR_RESONANCE", P::Resonance, f64::from(feedback) / 0.85, Law::Identity));
            }
            _ => {}
        }
        lanes
    }

    /// Write the stored value of lane `role` through the native setter
    /// (Delay 0x1409645ae, Chorus 0x140963970, Flanger 0x1409637f0, Phaser
    /// 0x140962c75: phase * 0.25 cycles, Flanger feedback * 0.95, Phaser * 0.85).
    pub fn set(&mut self, role: crate::ProcessorParameter, stored: f64) {
        use crate::ProcessorParameter as P;
        let v = stored as f32;
        if self.uvi() {
            return self.uvi_set(role, stored);
        }
        match role {
            P::Speed => self.lfo.rate = Rate::Hz(v),
            // ponytail: takes effect at the next LFO reset; the native re-phases at once (0x140960040).
            P::Phase => self.lfo.phase = v * 0.25,
            _ => {}
        }
        match &mut self.allpass {
            Some(Allpass { law: SweepLaw::Legacy { depth }, feedback, .. }) => match role {
                P::Depth => *depth = v,
                P::Feedback => *feedback = (stored * 0.85) as f32,
                _ => {}
            },
            Some(Allpass { law: SweepLaw::Filter { cutoff }, feedback, .. }) => match role {
                P::Cutoff => *cutoff = v.clamp(0., 1.),
                P::Resonance => *feedback = (stored.clamp(0., 1.) * 0.85) as f32,
                _ => {}
            },
            _ => {}
        }
        let Some(line) = &mut self.line else { return };
        match (&mut line.native, role) {
            (Native::LegacyDelay { time, .. }, P::Time) => {
                *time = v;
                line.taps[0].time = DelayTime::Ms(stored);
            }
            (Native::LegacyDelay { feedback, .. }, P::Feedback) => *feedback = v,
            (Native::LegacyDelay { damping, .. }, P::Damping) => *damping = v,
            (Native::LegacyDelay { pan, .. }, P::Pan) => *pan = v,
            (Native::LegacyModDelay { second_head, excursion, .. }, P::Depth) => {
                *excursion = stored * if *second_head { 75. } else { 50. };
            }
            (Native::LegacyModDelay { feedback, .. }, P::Feedback) => *feedback = v * f32::from_bits(0x3f733333),
            (Native::LegacyModDelay { base_ms, .. }, P::Colour) => {
                *base_ms = v;
                line.taps[0].time = DelayTime::Ms(stored);
            }
            _ => {}
        }
    }

    /// A UVI unit: one of its native kernels, or a line or allpass on a UVI
    /// table LFO (Chorus, Flanger, Phaser, Cross Phaser, Phasor).
    fn uvi(&self) -> bool {
        matches!(self.lfo.shape, LfoShape::Uvi { .. })
            || self.line.is_some_and(|l| {
                matches!(
                    l.native,
                    Native::UviDualDelay(_)
                        | Native::UviLegacyDelay(_)
                        | Native::UviDiffuseDelay(_)
                        | Native::UviTapeEcho(_)
                        | Native::UviVelvetDelay(_)
                        | Native::UviBuzzDelay(_)
                )
            })
    }

    /// A UVI unit's live knobs, each its own field: the unit's wet and dry
    /// gains, its LFO rate in Hz, then DualDelay's time, feedback and loop
    /// cuts, or a plain line's base time and feedback, or an allpass feedback.
    /// A script write elsewhere is a new configuration, not a lane.
    fn uvi_lanes(&self) -> Vec<super::Lane> {
        use super::uvi_lane;
        use crate::ProcessorParameter as P;
        let mut lanes = vec![uvi_lane(P::Gain, self.wet, 0., 2.), uvi_lane(P::Response, self.dry, 0., 1.)];
        if let (LfoShape::Uvi { .. }, Rate::Hz(hz)) = (self.lfo.shape, self.lfo.rate) {
            lanes.push(uvi_lane(P::Speed, f64::from(hz), 0., 200.));
        }
        match self.line.map(|l| (l.native, l)) {
            Some((Native::UviDualDelay(d), _)) => lanes.extend([
                uvi_lane(P::Time, f64::from(d.time), 0., 5.),
                uvi_lane(P::Feedback, f64::from(d.feedback), 0., 1.),
                uvi_lane(P::Cutoff, f64::from(d.high_cut_hz), 1., 20_000.),
                uvi_lane(P::Frequency, f64::from(d.low_cut_hz), 1., 20_000.),
            ]),
            Some((Native::None, l)) => {
                if l.tap_count == 1
                    && let DelayTime::Ms(ms) = l.taps[0].time
                {
                    lanes.push(uvi_lane(P::Time, ms, 0., l.max_ms));
                }
                let [[a, b], [c, d]] = l.feedback.matrix;
                if a == d && b == 0. && c == 0. {
                    lanes.push(uvi_lane(P::Feedback, a, -1., 1.));
                }
            }
            _ => {}
        }
        if let (None, Some(a)) = (self.line, self.allpass) {
            lanes.push(uvi_lane(P::Feedback, f64::from(a.feedback), -1., 1.));
        }
        lanes.into_iter().flatten().collect()
    }

    /// Write lane `role` of a UVI unit (see [`Config::uvi_lanes`]).
    fn uvi_set(&mut self, role: crate::ProcessorParameter, stored: f64) {
        use crate::ProcessorParameter as P;
        let v = stored as f32;
        match role {
            P::Gain => return self.wet = stored,
            P::Response => return self.dry = stored,
            P::Speed => return self.lfo.rate = Rate::Hz(v),
            _ => {}
        }
        if let (None, Some(a), P::Feedback) = (self.line, &mut self.allpass, role) {
            return a.feedback = v;
        }
        let Some(line) = &mut self.line else { return };
        match (&mut line.native, role) {
            (Native::UviDualDelay(d), P::Time) => d.time = v,
            (Native::UviDualDelay(d), P::Feedback) => d.feedback = v,
            (Native::UviDualDelay(d), P::Cutoff) => d.high_cut_hz = v,
            (Native::UviDualDelay(d), P::Frequency) => d.low_cut_hz = v,
            (Native::None, P::Time) if line.tap_count == 1 => line.taps[0].time = DelayTime::Ms(stored),
            (Native::None, P::Feedback) => line.feedback.matrix = [[stored, 0.], [0., stored]],
            _ => {}
        }
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
            if !(l.saturation.is_finite() && (0. ..=1.).contains(&l.saturation)) {
                return Err("saturation");
            }
            if !(0.1..=10.).contains(&l.right_scale) {
                return Err("right_scale");
            }
            if let Native::Replika(r) = l.native {
                let unit_interval = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                let all = [r.input_a, r.input_b, r.lowcut, r.highcut, r.dry_wet, r.feedback, r.time, r.delay_offset, r.pan_dry, r.pan_wet, r.width, r.saturation, r.tap_gain, r.ducking_sense, r.ducking_amount, r.ducking_release];
                if !all.iter().all(|&v| unit_interval(v)) {
                    return Err("replika");
                }
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
            if let Native::WowFlutter(w) = l.native {
                let unit = |v: f32| v.is_finite() && (0. ..=1.).contains(&v);
                if !([w.wow, w.flutter, w.speed, w.stereo].into_iter().all(unit)) {
                    return Err("wow flutter");
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
            if l.feedback.cut.is_some_and(|c| [c.highpass_hz, c.lowpass_hz].into_iter().flatten().any(|hz| !(hz.is_finite() && hz > 0.))) {
                return Err("feedback cut");
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
