//! Single-encoding UVI effect modules (`ModelId::UVI_BASE + 0..13`).
//!
//! Every script-addressable UVI insert defines ONE pure [`UviModule`] whose
//! `build_effect` function is called both at XML load time AND at runtime when
//! a Lua script invokes `setParameter` (`Action::EffectKnob`). This eliminates
//! the frozen piecewise-linear Lua lookup tables and ensures coupled parameters
//! (such as `ThreeBandShelves` `GainMid`/`GainLow`/`GainHigh`, `SparkVerb`
//! `RoomSize`/`DecayTime`/`Mix`/`MixMode`, `DigitalEq` `GainScale`/`Gain{n}`,
//! `Maximizer` `Threshold`/`Ceiling`, `WaveShaper` `Amount`/`Mix`/`OutputGain`,
//! `DualDelay` `Feedback`/`FeedbackRatio`/`Rotation`/`Mix`, etc.) always stay
//! exact regardless of the order a script moves them.

use std::f32::consts::FRAC_PI_2;
use std::f64::consts::{FRAC_PI_4, LOG10_2};

use kontra_ir as ir;
use kontra_law::common::{DB_GAIN, SQUARE_ROOT};
use kontra_law::uvi::{CLIPPER_ASYMMETRY, WAVESHAPER_DRIVE};
use roxmltree::Node;

use super::{flag, num};

/// Standard `Knob(0..MAX)` table for [`ir::KnobModel`].
const KNOBS: [ir::SlotKnob; ir::KnobModel::MAX] = [
    ir::SlotKnob::Knob(0),
    ir::SlotKnob::Knob(1),
    ir::SlotKnob::Knob(2),
    ir::SlotKnob::Knob(3),
    ir::SlotKnob::Knob(4),
    ir::SlotKnob::Knob(5),
    ir::SlotKnob::Knob(6),
    ir::SlotKnob::Knob(7),
    ir::SlotKnob::Knob(8),
    ir::SlotKnob::Knob(9),
    ir::SlotKnob::Knob(10),
    ir::SlotKnob::Knob(11),
    ir::SlotKnob::Knob(12),
    ir::SlotKnob::Knob(13),
    ir::SlotKnob::Knob(14),
    ir::SlotKnob::Knob(15),
    ir::SlotKnob::Knob(16),
    ir::SlotKnob::Knob(17),
    ir::SlotKnob::Knob(18),
    ir::SlotKnob::Knob(19),
    ir::SlotKnob::Knob(20),
    ir::SlotKnob::Knob(21),
    ir::SlotKnob::Knob(22),
    ir::SlotKnob::Knob(23),
];

/// Declarative single-encoding descriptor for one UVI insert effect.
pub struct UviModule {
    /// Stable serializable [`ir::ModelId`] in the UVI band (`64..128`).
    pub id: ir::ModelId,
    /// XML element tag name (e.g. `"ThreeBandShelves"`).
    pub tag: &'static str,
    /// Static attribute names in knob index order (`0..attrs.len()`).
    /// Empty for dynamically addressed multi-band effects (`DigitalEq`).
    pub attrs: &'static [&'static str],
    /// Single pure builder shared by load-time translation and audio-thread
    /// script updates. Must never allocate.
    pub build_effect: fn(&[f32], &ir::Effect) -> Option<ir::Effect>,
}

impl UviModule {
    /// Registers this module in `kontra-ir`'s wait-free model table (idempotent).
    #[inline]
    pub fn register(&'static self) {
        ir::register_model(self.id, self);
    }

    /// Evaluates [`Self::build_effect`] against a clean default effect shell.
    #[inline]
    #[must_use]
    pub fn build_fresh(&self, params: &[f32]) -> Option<ir::Effect> {
        let base = ir::Effect {
            kind: ir::EffectKind::Other,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        (self.build_effect)(params, &base)
    }

    /// Builds a serializable [`ir::KnobModel`] bound to [`Self::id`] with
    /// `knob_count` addressable knobs and `saved` positions.
    #[must_use]
    pub fn knob_model(&'static self, knob_count: usize, saved: &[f32]) -> ir::KnobModel {
        self.register();
        let knobs = KNOBS
            .get(..knob_count.min(ir::KnobModel::MAX))
            .unwrap_or_default();
        ir::KnobModel {
            id: self.id,
            knobs: knobs.into(),
            saved: saved.into(),
        }
    }
}

impl ir::EffectModel for UviModule {
    #[inline]
    fn build(&self, params: &[f32]) -> Option<ir::EffectKind> {
        self.build_fresh(params).map(|e| e.kind)
    }

    #[inline]
    fn build_effect(&self, params: &[f32], current: &ir::Effect) -> Option<ir::Effect> {
        (self.build_effect)(params, current)
    }
}

/// Model IDs for all UVI effect modules (`ModelId::UVI_BASE + 0..64`).
pub mod ids {
    use kontra_ir::ModelId;

    const B: u16 = ModelId::UVI_BASE;

    pub const THREE_BAND_SHELVES: ModelId = ModelId(B);
    pub const DIGITAL_EQ: ModelId = ModelId(B + 1);
    pub const SPARK_VERB: ModelId = ModelId(B + 2);
    pub const MAXIMIZER: ModelId = ModelId(B + 3);
    pub const WAVE_SHAPER: ModelId = ModelId(B + 4);
    pub const FLANGER: ModelId = ModelId(B + 5);
    pub const PHASOR: ModelId = ModelId(B + 6);
    pub const DIODE_CLIPPER: ModelId = ModelId(B + 7);
    pub const XPANDER_FILTER: ModelId = ModelId(B + 8);
    pub const DRIVE: ModelId = ModelId(B + 9);
    pub const DUAL_DELAY: ModelId = ModelId(B + 10);
    pub const CONVOLVER: ModelId = ModelId(B + 11);
    pub const SAMPLED_REVERB: ModelId = ModelId(B + 12);
}

/// SampledReverb un-normalized wet scaling (−20 dB).
pub(super) const RAW_WET: f32 = 0.1;

/// Octaves of the 20 Hz .. 20 kHz filter frequency range.
const FREQ_OCTAVES: f32 = 9.965_784;
const SPEED_OF_SOUND: f64 = 340.0;
const DUAL_DELAY_MAX_S: f64 = 5.0;

/// ThreeBandShelves: `[FreqLowMid, FreqMidHigh, GainLow, GainMid, GainHigh]`.
pub static THREE_BAND_SHELVES: UviModule = UviModule {
    id: ids::THREE_BAND_SHELVES,
    tag: "ThreeBandShelves",
    attrs: &[
        "FreqLowMid",
        "FreqMidHigh",
        "GainLow",
        "GainMid",
        "GainHigh",
    ],
    build_effect: |p, current| {
        let [f_low, f_high, g_low, g_mid, g_high] = *p else {
            return None;
        };
        let bands = [
            ir::Bell {
                shape: ir::Shape::LowShelf,
                hz: f_low,
                q: 0.5,
                gain_db: g_low - g_mid,
            },
            ir::Bell {
                shape: ir::Shape::HighShelf,
                hz: f_high,
                q: 0.5,
                gain_db: g_high - g_mid,
            },
            ir::Bell::FLAT,
            ir::Bell::FLAT,
        ];
        Some(ir::Effect {
            kind: ir::EffectKind::Eq(bands),
            bypass: current.bypass,
            output: DB_GAIN.eval_unclamped(g_mid),
            dry: 0.0,
        })
    },
};

/// DigitalEq (one 4-band chunk):
/// `[GainScale, f0, g0, bw0, f1, g1, bw1, f2, g2, bw2, f3, g3, bw3, s0, s1, s2, s3]`
/// where `s_i`: `0 = flat, 1 = LowShelf, 2 = HighShelf, 3 = Peak`.
pub static DIGITAL_EQ: UviModule = UviModule {
    id: ids::DIGITAL_EQ,
    tag: "DigitalEq",
    attrs: &[],
    build_effect: |p, current| {
        let [
            scale,
            f0,
            g0,
            bw0,
            f1,
            g1,
            bw1,
            f2,
            g2,
            bw2,
            f3,
            g3,
            bw3,
            s0,
            s1,
            s2,
            s3,
        ] = *p
        else {
            return None;
        };
        let make = |hz: f32, gain: f32, bw: f32, shape_code: f32| -> ir::Bell {
            let code = shape_code.round() as i32;
            let shape = match code {
                1 => ir::Shape::LowShelf,
                2 => ir::Shape::HighShelf,
                3 => ir::Shape::Peak,
                _ => return ir::Bell::FLAT,
            };
            let q = if shape == ir::Shape::Peak {
                let w = bw.max(1e-3);
                1.0 / ((w / 2.0).exp2() - (-w / 2.0).exp2())
            } else {
                1.0
            };
            ir::Bell {
                shape,
                hz: hz.clamp(20.0, 20_000.0),
                q,
                gain_db: gain * scale,
            }
        };
        let bands = [
            make(f0, g0, bw0, s0),
            make(f1, g1, bw1, s1),
            make(f2, g2, bw2, s2),
            make(f3, g3, bw3, s3),
        ];
        Some(ir::Effect {
            kind: ir::EffectKind::Eq(bands),
            bypass: current.bypass,
            output: 1.0,
            dry: 0.0,
        })
    },
};

/// SparkVerb: 21 parameters matching `reverb::spark_verb`.
pub static SPARK_VERB: UviModule = UviModule {
    id: ids::SPARK_VERB,
    tag: "SparkVerb",
    attrs: &[
        "RoomSize",
        "DecayTime",
        "PreDelay",
        "Rolloff",
        "Width",
        "Mix",
        "MixMode",
        "Diffusion",
        "ModRate",
        "ModDepth",
        "Quality",
        "Shape",
        "DiffusionOnOff",
        "DiffusionStart",
        "DecayLow",
        "DecayHigh",
        "FreqLow",
        "FreqHigh",
        "LowCut",
        "HiCut",
        "Freeze",
    ],
    build_effect: |p, current| {
        let [
            room,
            decay,
            predelay,
            rolloff,
            width,
            mix,
            mix_mode,
            diffusion,
            mod_rate,
            mod_depth,
            quality,
            shape,
            diff_on,
            diff_start,
            decay_low,
            decay_high,
            freq_low,
            freq_high,
            low_cut,
            hi_cut,
            freeze,
        ] = *p
        else {
            return None;
        };
        let room = f64::from(room).max(1.0);
        let seconds = f64::from(decay).max(0.01) * room / 10.0;
        let crossing = room / SPEED_OF_SOUND;
        let hz = f64::from(mod_rate).max(0.0) / 16.0 / crossing;
        let swing_ms = if hz > 0.0 {
            let ratio = f64::from(mod_depth).max(0.0) * std::f64::consts::LN_2 / 1200.0;
            1000.0 * ratio / (std::f64::consts::TAU * hz)
        } else {
            0.0
        };
        let mix = f64::from(mix).clamp(0.0, 1.0);
        let (wet, dry) = if (mix_mode - 1.0).abs() < 0.5 {
            (SQUARE_ROOT.eval_f64(mix), SQUARE_ROOT.eval_f64(1.0 - mix))
        } else {
            (mix, 1.0 - mix)
        };
        let law = ir::HadamardNetwork {
            lines: 1 << (f64::from(quality).clamp(1.0, 4.0) as u8),
            shape,
            diffusers: diff_on > 0.5,
            diffuser_ms: diff_start.max(0.1),
            decay_low,
            decay_high,
            low_hz: freq_low,
            high_hz: freq_high,
            low_cut: low_cut > 0.5,
            high_cut: hi_cut > 0.5,
            freeze: freeze > 0.5,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Reverb(ir::Reverb {
                seconds: seconds as f32,
                size: crossing as f32,
                network: Some(ir::Network::Hadamard(law)),
                damping: 0.0,
                diffusion: diffusion.clamp(0.0, 1.0),
                predelay_ms: predelay.max(0.0),
                high_cut_hz: rolloff,
                low_shelf_db: 0.0,
                stereo: width.clamp(0.0, 1.0),
                modulation_ms: swing_ms as f32,
                modulation_hz: hz as f32,
            }),
            bypass: current.bypass,
            output: wet as f32,
            dry: dry as f32,
        })
    },
};

/// Maximizer: `[Threshold, Ceiling, SlewRate, CeilingMode, Knee, Attack, Release, ReleaseBlend, Lookahead]`.
pub static MAXIMIZER: UviModule = UviModule {
    id: ids::MAXIMIZER,
    tag: "Maximizer",
    attrs: &[
        "Threshold",
        "Ceiling",
        "SlewRate",
        "CeilingMode",
        "Knee",
        "Attack",
        "Release",
        "ReleaseBlend",
        "Lookahead",
    ],
    build_effect: |p, current| {
        let [
            threshold,
            ceiling,
            slew,
            mode,
            knee,
            attack,
            release,
            blend,
            lookahead,
        ] = *p
        else {
            return None;
        };
        let slew = f64::from(slew);
        let kind = ir::EffectKind::Limiter(ir::Limiter {
            input_db: ceiling - threshold,
            ceiling_db: ceiling,
            release_ms: if slew > 0.0 {
                (slew * 20.0 * LOG10_2 / 10.0) as f32
            } else {
                0.0
            },
            knee_db: knee.max(0.0),
            attack_ms: if mode == 0.0 { 0.0 } else { attack.max(0.0) },
            smooth_ms: release.max(0.0),
            slow: blend.clamp(0.0, 1.0),
            lookahead_ms: lookahead.max(0.0),
            clip: (mode - 2.0).abs() < 0.5,
        });
        Some(ir::Effect {
            kind,
            bypass: current.bypass,
            output: 1.0,
            dry: 0.0,
        })
    },
};

/// WaveShaper: `[InputGain, Amount, Mix, OutputGain, Mode, PreFreq, PostFreq]`.
pub static WAVE_SHAPER: UviModule = UviModule {
    id: ids::WAVE_SHAPER,
    tag: "WaveShaper",
    attrs: &[
        "InputGain",
        "Amount",
        "Mix",
        "OutputGain",
        "Mode",
        "PreFreq",
        "PostFreq",
    ],
    build_effect: |p, current| {
        let [input_db, amount, mix, output_db, mode, pre_hz, post_hz] = *p else {
            return None;
        };
        if mode.fract() != 0.0 || !(0.0..=3.0).contains(&mode) {
            return None;
        }
        let input = DB_GAIN.eval_unclamped(input_db);
        let out_gain = DB_GAIN.eval_unclamped(output_db);
        let d = WAVESHAPER_DRIVE.eval(amount);
        let (curve, scale) = match mode as u8 {
            0 => (ir::DriveCurve::Sine(d * FRAC_PI_2 * input), 1.0),
            1 => (ir::DriveCurve::Fold(d * FRAC_PI_2 * input), 1.0 / FRAC_PI_2),
            2 => (ir::DriveCurve::Tanh(d * input), 1.0 / (2.0 * d).tanh()),
            3 => (ir::DriveCurve::Atan(d * input), 1.0 / (2.0 * d).atan()),
            _ => return None,
        };
        let mix = mix.clamp(0.0, 1.0);
        let shaper = ir::Shaper {
            pre_lowpass_hz: Some(pre_hz),
            curve,
            lowpass_hz: None,
            highpass_hz: Some(post_hz),
            sections: ir::Sections::NONE,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Shaper(shaper),
            bypass: current.bypass,
            output: mix * out_gain * scale,
            dry: 1.0 - mix,
        })
    },
};

/// Flanger: `[DelayTime, Depth, Feedback, Speed, Mix, SyncToHost]`.
pub static FLANGER: UviModule = UviModule {
    id: ids::FLANGER,
    tag: "Flanger",
    attrs: &[
        "DelayTime",
        "Depth",
        "Feedback",
        "Speed",
        "Mix",
        "SyncToHost",
    ],
    build_effect: |p, current| {
        let [time, depth, feedback, speed, mix, synced] = *p else {
            return None;
        };
        let depth = depth.clamp(0.0, 1.0);
        let time = time.clamp(0.0, 1.0);
        let speed = speed.max(1e-3);
        let rate = if synced > 0.5 {
            ir::Rate::Beats(speed)
        } else {
            ir::Rate::Hz(speed)
        };
        let delay = ir::Delay {
            direct: 0.0,
            time: ir::DelayTime::Ms(20.0 * (time + depth / 2.0)),
            depth_ms: 10.0 * depth,
            second_head: false,
            input: ir::IDENTITY,
            feedback: [[feedback, 0.0], [0.0, feedback]],
            output: ir::IDENTITY,
            damping_hz: f32::INFINITY,
            low_cut_hz: 0.0,
            steep_low_cut: false,
            body: std::borrow::Cow::Borrowed(&[]),
            triangle_s: None,
            shape: std::borrow::Cow::Borrowed(&[]),
            noise: [0.0; 2],
            high_shelf: None,
            spread_ms: 0.0,
            rate,
            phase: 0.0,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(delay),
            bypass: current.bypass,
            output: mix.clamp(0.0, 1.0),
            dry: 1.0,
        })
    },
};

/// Phasor: `[MinFreq, MaxFreq, Feedback, Speed, Spread, Depth, Order, LFOSHape, SyncToHost]`.
pub static PHASOR: UviModule = UviModule {
    id: ids::PHASOR,
    tag: "Phasor",
    attrs: &[
        "MinFreq",
        "MaxFreq",
        "Feedback",
        "Speed",
        "Spread",
        "Depth",
        "Order",
        "LFOSHape",
        "SyncToHost",
    ],
    build_effect: |p, current| {
        let [
            low,
            high,
            feedback,
            speed,
            spread,
            depth,
            order,
            shape,
            synced,
        ] = *p
        else {
            return None;
        };
        let order = order.round().clamp(1.0, 12.0);
        let low = low.max(1.0);
        let high = high.max(1.0);
        let speed = speed.max(1e-3);
        let rate = if synced > 0.5 {
            ir::Rate::Beats(speed)
        } else {
            ir::Rate::Hz(speed.min(10.0))
        };
        let hz = order / std::f32::consts::PI * (low * high).sqrt();
        let phaser = ir::Phaser {
            stages: 2 * order as u8,
            triangle: shape == 0.0,
            hz,
            spread_hz: hz,
            exponent: 0.5 * (high / low).ln(),
            feedback,
            rate,
            phase: spread.clamp(0.0, 1.0) / 4.0,
        };
        let depth = depth.clamp(0.0, 1.0);
        let norm = (1.0 + depth * depth).sqrt();
        Some(ir::Effect {
            kind: ir::EffectKind::Phaser(phaser),
            bypass: current.bypass,
            output: depth / norm,
            dry: 1.0 / norm,
        })
    },
};

/// DiodeClipper: `[OutputGain, Drive, Tone, HighPass, Asymmetry]`.
pub static DIODE_CLIPPER: UviModule = UviModule {
    id: ids::DIODE_CLIPPER,
    tag: "DiodeClipper",
    attrs: &["OutputGain", "Drive", "Tone", "HighPass", "Asymmetry"],
    build_effect: |p, current| {
        let [out_db, drive_db, tone_hz, coupling_hz, asymmetry] = *p else {
            return None;
        };
        let clipper = ir::Clipper {
            drive: DB_GAIN.eval_unclamped(drive_db),
            limit: 4.5,
            tone_hz,
            coupling_hz,
            saturation: 5.5462e-6,
            slope: 22.075_055,
            asymmetry: CLIPPER_ASYMMETRY.eval_unclamped(asymmetry),
            dc_hz: 1.0,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Clipper(clipper),
            bypass: current.bypass,
            output: DB_GAIN.eval_unclamped(out_db),
            dry: 0.0,
        })
    },
};

/// XpanderFilter (bus/ladder): `[Freq, Q, Drive, Mode, Algorithm, Fat]`.
pub static XPANDER_FILTER: UviModule = UviModule {
    id: ids::XPANDER_FILTER,
    tag: "XpanderFilter",
    attrs: &["Freq", "Q", "Drive", "Mode", "Algorithm", "Fat"],
    build_effect: |p, current| {
        let [hz, q, drive_db, mode, algorithm, fat] = *p else {
            return None;
        };
        let q = q.clamp(0.0, 1.0);
        let drive_db = drive_db.clamp(-20.0, 20.0);
        let taps = (mode.round().clamp(0.0, 11.0) as u8) + 1;
        let ladder = if (algorithm - 1.0).abs() < 0.5 {
            ir::Ladder::ZeroDelay(taps)
        } else {
            ir::Ladder::Explicit {
                taps,
                fat: fat.clamp(0.0, 1.0),
            }
        };
        let filter = ir::Filter {
            pass: if (mode - 1.0).abs() < 0.5 {
                ir::Pass::High
            } else {
                ir::Pass::Low
            },
            hz: hz.clamp(20.0, 20_000.0),
            stages: 0,
            ladder: Some(ladder),
            damping: [2.0 * (1.0 - q); 3],
            gain: DB_GAIN.eval_unclamped(drive_db),
            octaves: FREQ_OCTAVES,
            q_octaves: 0.0,
        };
        Some(ir::Effect {
            kind: ir::EffectKind::Filter(filter),
            bypass: current.bypass,
            output: 1.0,
            dry: 0.0,
        })
    },
};

/// Drive: `[DriveAmount, Mode]`.
pub static DRIVE: UviModule = UviModule {
    id: ids::DRIVE,
    tag: "Drive",
    attrs: &["DriveAmount", "Mode"],
    build_effect: |p, current| {
        let [amount, mode] = *p else {
            return None;
        };
        if mode != 0.0 {
            return None;
        }
        let d = WAVESHAPER_DRIVE.eval(amount.clamp(0.0, 1.0));
        let shaper = ir::Shaper {
            pre_lowpass_hz: None,
            curve: ir::DriveCurve::Tanh(d),
            lowpass_hz: None,
            highpass_hz: None,
            sections: ir::Sections::NONE,
        };
        let scale = 1.0 / d.tanh().max(f32::EPSILON);
        Some(ir::Effect {
            kind: ir::EffectKind::Shaper(shaper),
            bypass: current.bypass,
            output: scale,
            dry: 0.0,
        })
    },
};

/// DualDelay: `[DelayTime, HighCut, LowCut, ModRate, Mix, Feedback, FeedbackRatio,
/// InputWidth, InputRotation, Rotation, OutputWidth, OutputRotation, SyncToHost]`.
pub static DUAL_DELAY: UviModule = UviModule {
    id: ids::DUAL_DELAY,
    tag: "DualDelay",
    attrs: &[
        "DelayTime",
        "HighCut",
        "LowCut",
        "ModRate",
        "Mix",
        "Feedback",
        "FeedbackRatio",
        "InputWidth",
        "InputRotation",
        "Rotation",
        "OutputWidth",
        "OutputRotation",
        "SyncToHost",
    ],
    build_effect: |p, current| {
        let [
            time,
            high_cut,
            low_cut,
            mod_rate,
            mix,
            feedback,
            ratio,
            in_width,
            in_rot,
            rot_deg,
            out_width,
            out_rot,
            synced,
        ] = *p
        else {
            return None;
        };
        let rot = |rad: f32| -> ir::Matrix {
            let (s, c) = rad.sin_cos();
            [[c, -s], [s, c]]
        };
        let wid = |w: f32| -> ir::Matrix {
            let (same, other) = ((1.0 + w) / 2.0, (1.0 - w) / 2.0);
            [[same, other], [other, same]]
        };
        let mul = |a: ir::Matrix, b: ir::Matrix| -> ir::Matrix {
            let [[b00, b01], [b10, b11]] = b;
            a.map(|[x, y]| [x * b00 + y * b10, x * b01 + y * b11])
        };
        let ratio = ratio.clamp(-1.0, 1.0);
        let [fb0, fb1] = if ratio <= 0.0 {
            [(1.0 + ratio) * feedback, feedback]
        } else {
            [feedback, (1.0 - ratio) * feedback]
        };
        let quarter = FRAC_PI_4 as f32;
        let input = mul(rot(in_rot * quarter), wid(in_width));
        let output = mul(wid(out_width), rot(out_rot * quarter));
        let fb_rot = rot(rot_deg.to_radians());
        let scale = |[top, bottom]: ir::Matrix| [top.map(|x| x * fb0), bottom.map(|x| x * fb1)];
        let time = f64::from(time).max(0.0);
        let mix = mix.clamp(0.0, 1.0);
        Some(ir::Effect {
            kind: ir::EffectKind::Delay(ir::Delay {
                direct: 0.0,
                time: if synced > 0.5 {
                    ir::DelayTime::Beats(time as f32)
                } else {
                    ir::DelayTime::Ms((time.min(DUAL_DELAY_MAX_S) * 1000.0) as f32)
                },
                depth_ms: 0.0,
                second_head: false,
                input: scale(input),
                feedback: scale(fb_rot),
                output,
                damping_hz: high_cut,
                low_cut_hz: low_cut.max(0.0),
                steep_low_cut: false,
                body: std::borrow::Cow::Borrowed(&[]),
                triangle_s: None,
                shape: std::borrow::Cow::Borrowed(&[]),
                noise: [0.0; 2],
                high_shelf: None,
                spread_ms: 0.0,
                rate: ir::Rate::Hz(mod_rate),
                phase: 0.0,
            }),
            bypass: current.bypass,
            output: SQUARE_ROOT.eval(mix),
            dry: SQUARE_ROOT.eval(1.0 - mix),
        })
    },
};

/// Convolver: `[Wet, Dry]` (preserves `current.kind` impulse index).
pub static CONVOLVER: UviModule = UviModule {
    id: ids::CONVOLVER,
    tag: "Convolver",
    attrs: &["Wet", "Dry"],
    build_effect: |p, current| {
        let [wet, dry] = *p else {
            return None;
        };
        let output = if matches!(current.kind, ir::EffectKind::OtherWet) {
            1.0
        } else {
            wet
        };
        Some(ir::Effect {
            kind: current.kind.clone(),
            bypass: current.bypass,
            output,
            dry,
        })
    },
};

/// SampledReverb: `[Wet, Dry, NormalizePower]` (preserves `current.kind` impulse index);
/// NormalizePower holds its saved value ([`decode_model`] maps no script name to it).
pub static SAMPLED_REVERB: UviModule = UviModule {
    id: ids::SAMPLED_REVERB,
    tag: "SampledReverb",
    attrs: &["Wet", "Dry", "NormalizePower"],
    build_effect: |p, current| {
        let [wet, dry, normalized] = *p else {
            return None;
        };
        let scale = if normalized > 0.5 { 1.0 } else { RAW_WET };
        let output = if matches!(current.kind, ir::EffectKind::OtherWet) {
            1.0
        } else {
            wet * scale
        };
        Some(ir::Effect {
            kind: current.kind.clone(),
            bypass: current.bypass,
            output,
            dry,
        })
    },
};

/// All UVI effect modules.
pub static ALL: &[&UviModule] = &[
    &THREE_BAND_SHELVES,
    &DIGITAL_EQ,
    &SPARK_VERB,
    &MAXIMIZER,
    &WAVE_SHAPER,
    &FLANGER,
    &PHASOR,
    &DIODE_CLIPPER,
    &XPANDER_FILTER,
    &DRIVE,
    &DUAL_DELAY,
    &CONVOLVER,
    &SAMPLED_REVERB,
];

/// Looks up a UVI module by its XML tag name.
#[must_use]
pub fn by_tag(tag: &str) -> Option<&'static UviModule> {
    ALL.iter().copied().find(|m| m.tag == tag)
}

/// Registers every UVI effect module in `kontra-ir`'s wait-free model table.
pub fn register_all() {
    for module in ALL {
        module.register();
    }
}

/// Decodes a UVI XML insert node into `(ir::KnobModel, Vec<(String, u8)>)`
/// where each entry in the map links an XML attribute name to its knob index.
///
/// For `DigitalEq`, `eq_chunk` selects the 4-band EQ slice (`0..4`).
pub(super) fn decode_model(
    node: Node,
    eq_chunk: usize,
) -> Option<(ir::KnobModel, Vec<(String, u8)>)> {
    let val = |attr: &str, default: f64| num(node, attr, default).unwrap_or(default) as f32;
    let bool_val = |attr: &str| f32::from(u8::from(flag(node, attr)));
    let static_map = |m: &'static UviModule| -> Vec<(String, u8)> {
        m.attrs
            .iter()
            .enumerate()
            .filter_map(|(i, &name)| Some((name.to_owned(), u8::try_from(i).ok()?)))
            .collect()
    };

    match node.tag_name().name() {
        "ThreeBandShelves" => {
            let saved = [
                val("FreqLowMid", 200.0),
                val("FreqMidHigh", 4000.0),
                val("GainLow", 0.0),
                val("GainMid", 0.0),
                val("GainHigh", 0.0),
            ];
            Some((
                THREE_BAND_SHELVES.knob_model(saved.len(), &saved),
                static_map(&THREE_BAND_SHELVES),
            ))
        }
        "DigitalEq" => {
            let scale = val("GainScale", 1.0);
            let mut saved = [0.0f32; 17];
            saved[0] = scale;
            let mut map = Vec::new();
            let mut band = 0usize;
            let mut chunk_has_bands = false;
            for n in 1..=16u8 {
                if !flag(node, &format!("Enabled{n}")) {
                    continue;
                }
                let kind = num(node, &format!("Type{n}"), 6.0).unwrap_or(6.0) as i32;
                let shape_code = match kind {
                    4 => 1.0f32,
                    5 => 2.0f32,
                    6 => 3.0f32,
                    _ => continue,
                };
                if band / 4 == eq_chunk {
                    let b = band % 4;
                    chunk_has_bands = true;
                    let base = 1 + 3 * b;
                    if let Some(s) = saved.get_mut(base) {
                        *s = val(&format!("Freq{n}"), 1000.0);
                    }
                    if let Some(s) = saved.get_mut(base + 1) {
                        *s = val(&format!("Gain{n}"), 0.0);
                    }
                    if let Some(s) = saved.get_mut(base + 2) {
                        *s = val(&format!("Bandwidth{n}"), 1.0);
                    }
                    if let Some(s) = saved.get_mut(13 + b) {
                        *s = shape_code;
                    }
                    let idx = u8::try_from(base).ok()?;
                    map.push((format!("Freq{n}"), idx));
                    map.push((format!("Gain{n}"), idx + 1));
                    if kind == 6 {
                        map.push((format!("Bandwidth{n}"), idx + 2));
                    }
                }
                band += 1;
            }
            if !chunk_has_bands {
                return None;
            }
            map.push(("GainScale".to_owned(), 0));
            Some((DIGITAL_EQ.knob_model(13, &saved), map))
        }
        "SparkVerb" => {
            let saved = [
                val("RoomSize", 20.0),
                val("DecayTime", 1.0),
                val("PreDelay", 0.0),
                val("Rolloff", 20_000.0),
                val("Width", 1.0),
                val("Mix", 1.0),
                val("MixMode", 0.0),
                val("Diffusion", 0.618),
                val("ModRate", 1.0),
                val("ModDepth", 4.0),
                val("Quality", 3.0),
                val("Shape", 0.0),
                bool_val("DiffusionOnOff"),
                val("DiffusionStart", 5.0),
                val("DecayLow", 1.0),
                val("DecayHigh", 1.0),
                val("FreqLow", 250.0),
                val("FreqHigh", 12_000.0),
                bool_val("LowCut"),
                bool_val("HiCut"),
                bool_val("Freeze"),
            ];
            Some((
                SPARK_VERB.knob_model(saved.len(), &saved),
                static_map(&SPARK_VERB),
            ))
        }
        "Maximizer" => {
            let saved = [
                val("Threshold", 0.0),
                val("Ceiling", -0.1),
                val("SlewRate", 0.0),
                val("CeilingMode", 0.0),
                val("Knee", 0.0),
                val("Attack", 0.0),
                val("Release", 10.0),
                val("ReleaseBlend", 0.0),
                val("Lookahead", 0.0),
            ];
            Some((
                MAXIMIZER.knob_model(saved.len(), &saved),
                static_map(&MAXIMIZER),
            ))
        }
        "WaveShaper" => {
            let mode = num(node, "Mode", 0.0).unwrap_or(0.0);
            if mode.fract() != 0.0 || !(0.0..=3.0).contains(&mode) {
                return None;
            }
            let saved = [
                val("InputGain", 0.0),
                val("Amount", 0.0),
                val("Mix", 1.0),
                val("OutputGain", 0.0),
                mode as f32,
                val("PreFreq", 20_000.0),
                val("PostFreq", 20.0),
            ];
            Some((
                WAVE_SHAPER.knob_model(saved.len(), &saved),
                static_map(&WAVE_SHAPER),
            ))
        }
        "Flanger" => {
            let saved = [
                val("DelayTime", 0.0),
                val("Depth", 0.5),
                val("Feedback", 0.0),
                val("Speed", 1.0),
                val("Mix", 1.0),
                bool_val("SyncToHost"),
            ];
            Some((
                FLANGER.knob_model(saved.len(), &saved),
                static_map(&FLANGER),
            ))
        }
        "Phasor" => {
            let saved = [
                val("MinFreq", 200.0),
                val("MaxFreq", 3000.0),
                val("Feedback", 0.0),
                val("Speed", 1.0),
                val("Spread", 1.0),
                val("Depth", 1.0),
                val("Order", 4.0),
                val("LFOSHape", 0.0),
                bool_val("SyncToHost"),
            ];
            Some((PHASOR.knob_model(saved.len(), &saved), static_map(&PHASOR)))
        }
        "DiodeClipper" => {
            let saved = [
                val("OutputGain", 0.0),
                val("Drive", 0.0),
                val("Tone", 20_000.0),
                val("HighPass", 1.0),
                val("Asymmetry", 0.0),
            ];
            Some((
                DIODE_CLIPPER.knob_model(saved.len(), &saved),
                static_map(&DIODE_CLIPPER),
            ))
        }
        "XpanderFilter" if super::filters::on_bus(node) => {
            let saved = [
                val("Freq", 20_000.0),
                val("Q", 0.0),
                val("Drive", 0.0),
                val("Mode", 0.0),
                val("Algorithm", 0.0),
                val("Fat", 1.0),
            ];
            Some((
                XPANDER_FILTER.knob_model(saved.len(), &saved),
                static_map(&XPANDER_FILTER),
            ))
        }
        "Drive" if num(node, "Mode", 0.0).unwrap_or(0.0) == 0.0 => {
            let saved = [val("DriveAmount", 0.0), 0.0];
            Some((DRIVE.knob_model(saved.len(), &saved), static_map(&DRIVE)))
        }
        "DualDelay" => {
            let saved = [
                val("DelayTime", 0.5),
                val("HighCut", 20_000.0),
                val("LowCut", 0.0),
                val("ModRate", 1.0),
                val("Mix", 1.0),
                val("Feedback", 0.3),
                val("FeedbackRatio", 0.0),
                val("InputWidth", 1.0),
                val("InputRotation", 0.0),
                val("Rotation", 0.0),
                val("OutputWidth", 1.0),
                val("OutputRotation", 0.0),
                bool_val("SyncToHost"),
            ];
            Some((
                DUAL_DELAY.knob_model(saved.len(), &saved),
                static_map(&DUAL_DELAY),
            ))
        }
        "Convolver" => {
            let saved = [val("Wet", 1.0), val("Dry", 0.0)];
            Some((
                CONVOLVER.knob_model(saved.len(), &saved),
                static_map(&CONVOLVER),
            ))
        }
        "SampledReverb" => {
            let saved = [val("Wet", 1.0), val("Dry", 0.0), bool_val("NormalizePower")];
            // NormalizePower is how the response was prepared: a script may
            // not move it, or the wet scale would leave the response's level.
            let mut map = static_map(&SAMPLED_REVERB);
            map.retain(|(name, _)| name != "NormalizePower");
            Some((SAMPLED_REVERB.knob_model(saved.len(), &saved), map))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn all_uvi_modules_have_unique_ids_in_uvi_band_and_register() {
        register_all();
        let mut seen = HashSet::new();
        for m in ALL {
            assert!(
                seen.insert(m.id),
                "duplicate UVI ModelId {:?} on {}",
                m.id,
                m.tag
            );
            assert!(
                m.id.0 >= ir::ModelId::UVI_BASE && m.id.0 < 128,
                "UVI module {} ID {:?} outside UVI band",
                m.tag,
                m.id
            );
            assert!(
                ir::model(m.id).is_some(),
                "UVI module {} not registered",
                m.tag
            );
            assert_eq!(by_tag(m.tag).map(|x| x.id), Some(m.id));
        }
        assert_eq!(ALL.len(), 13);
    }

    /// A script reaches Wet and Dry, never NormalizePower: the response was
    /// prepared at load with or without it, and the wet scale must agree.
    #[test]
    fn sampled_reverb_normalize_power_is_not_script_movable() {
        let xml = r#"<SampledReverb Wet="0.5" Dry="0.25" NormalizePower="0"/>"#;
        let doc = roxmltree::Document::parse(xml).unwrap();
        let (model, map) = decode_model(doc.root_element(), 0).unwrap();
        let names: Vec<_> = map.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Wet", "Dry"]);
        assert_eq!(model.saved.get(2), Some(&0.0));
    }
}
