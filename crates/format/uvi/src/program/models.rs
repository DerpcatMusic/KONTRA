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

use std::f64::consts::LOG10_2;

use kontra_format::Notes;
use kontra_ir as ir;
use kontra_law::common::DB_GAIN;
use kontra_law::uvi::WAVESHAPER_DRIVE;
use roxmltree::Node;

use super::{flag, num};

mod delay;
mod reverb;
mod shaper;
mod sweep;

pub use delay::DUAL_DELAY;
pub use reverb::SPARK_VERB;
pub use shaper::{DIODE_CLIPPER, WAVE_SHAPER};
pub use sweep::{FLANGER, PHASOR};

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

/// One saved attribute of a [`UviModule`]: its XML name, the value it
/// reads when absent, whether it reads as a 0/1 flag, and whether a
/// script may move it.
#[derive(Clone, Copy, Debug)]
pub struct Attr {
    pub name: &'static str,
    pub default: f32,
    pub flag: bool,
    pub scripted: bool,
}

impl Attr {
    /// A number, `default` when absent.
    #[must_use]
    pub const fn num(name: &'static str, default: f32) -> Self {
        Self {
            name,
            default,
            flag: false,
            scripted: true,
        }
    }

    /// A 0/1 flag, off when absent.
    #[must_use]
    pub const fn flag(name: &'static str) -> Self {
        Self {
            name,
            default: 0.0,
            flag: true,
            scripted: true,
        }
    }

    /// Kept at its saved value: no script name maps to it.
    #[must_use]
    pub const fn held(self) -> Self {
        Self {
            scripted: false,
            ..self
        }
    }
}

/// A module whose saved effect sets nothing the builder drops.
fn nothing(_: Node, _: &mut Notes) -> Result<(), String> {
    Ok(())
}

/// Declarative single-encoding descriptor for one UVI insert effect.
pub struct UviModule {
    /// Stable serializable [`ir::ModelId`] in the UVI band (`64..128`).
    pub id: ir::ModelId,
    /// XML element tag name (e.g. `"ThreeBandShelves"`).
    pub tag: &'static str,
    /// The saved attributes in knob index order (`0..attrs.len()`).
    /// Empty for dynamically addressed multi-band effects (`DigitalEq`).
    pub attrs: &'static [Attr],
    /// Reports at load what the saved effect sets that the builder does
    /// not carry; scripts moving a knob never reach it.
    pub report: fn(Node, &mut Notes) -> Result<(), String>,
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

    /// The saved values of [`Self::attrs`] at `node`, in knob order.
    ///
    /// # Errors
    /// When a saved number does not parse.
    pub fn saved(&self, node: Node) -> Result<Vec<f32>, String> {
        (self.attrs.iter())
            .map(|a| {
                Ok(if a.flag {
                    f32::from(u8::from(flag(node, a.name)))
                } else {
                    num(node, a.name, f64::from(a.default))? as f32
                })
            })
            .collect()
    }

    /// The effect saved at `node` as its module builds it (bypass off),
    /// its load-only diagnostics in `notes`; `None` for a tag no module
    /// carries or values its builder rejects.
    ///
    /// # Errors
    /// When a saved number does not parse.
    pub fn load(node: Node, notes: &mut Notes) -> Result<Option<ir::Effect>, String> {
        let Some(m) = by_tag(node.tag_name().name()).filter(|m| !m.attrs.is_empty()) else {
            return Ok(None);
        };
        (m.report)(node, notes)?;
        Ok(m.build_fresh(&m.saved(node)?))
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
/// The DualDelay engine's longest line, seconds.
const DUAL_DELAY_MAX_S: f64 = 5.0;

/// ThreeBandShelves: UVI's one biquad ([ref] → [ref]) is the
/// mid gain times a first-order low shelf at FreqLowMid and a first-order
/// high shelf at FreqMidHigh, each by its band's gain over the mid's
/// (bilinear, tan-prewarped, [ref]). A cookbook shelf at slope 0.5 is
/// that first-order shelf exactly.
///
/// Knobs: `[FreqLowMid, FreqMidHigh, GainLow, GainMid, GainHigh]`.
pub static THREE_BAND_SHELVES: UviModule = UviModule {
    id: ids::THREE_BAND_SHELVES,
    tag: "ThreeBandShelves",
    attrs: &[
        Attr::num("FreqLowMid", 200.0),
        Attr::num("FreqMidHigh", 4000.0),
        Attr::num("GainLow", 0.0),
        Attr::num("GainMid", 0.0),
        Attr::num("GainHigh", 0.0),
    ],
    report: nothing,
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
    report: nothing,
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

/// UVI's Maximizer (process [ref], prepare [ref], parameter
/// ids [ref]..2e4). Per block it takes the louder channel in dB,
/// floored at Threshold (or through a quadratic knee `(x−T+k)²/4k` within
/// ±Knee), and turns it into a gain of `Ceiling − level` dB after:
///
/// - a slew limit: the level falls at most `10000/SlewRate` dB/s (prepare
///   `10000/(rate·SlewRate)` per frame, no limit at 0); rises are free;
/// - two one-pole falls blended by ReleaseBlend: one to a
///   third in Release ms (`e^(ln(1/3)/frames)`, ln(1/3) at [ref]),
///   one 100× slower (`e^(−0.010986/frames)`); a rise uses Attack, which
///   CeilingMode 0 (Brickwall, the default) forces to 0;
/// - the largest level over the last `2·⌊Lookahead·rate/2000⌋` frames
///, applied to the audio delayed by as many frames.
///
/// CeilingMode 2 (Hard) also clamps the output at Ceiling (`10^(C/20)`).
/// Threshold-to-Ceiling is input gain into a limiter at Ceiling.
///
/// Knobs: `[Threshold, Ceiling, SlewRate, CeilingMode, Knee, Attack, Release, ReleaseBlend, Lookahead]`.
pub static MAXIMIZER: UviModule = UviModule {
    id: ids::MAXIMIZER,
    tag: "Maximizer",
    attrs: &[
        Attr::num("Threshold", 0.0),
        Attr::num("Ceiling", -0.1),
        Attr::num("SlewRate", 0.0),
        Attr::num("CeilingMode", 0.0),
        Attr::num("Knee", 0.0),
        Attr::num("Attack", 0.0),
        Attr::num("Release", 10.0),
        Attr::num("ReleaseBlend", 0.0),
        Attr::num("Lookahead", 0.0),
    ],
    report: |node, notes| {
        // ponytail: TruePeak (Off in every corpus program) adds a 6-frame
        // oversampled detector; it is reported, not modelled.
        if num(node, "TruePeak", 0.0)? != 0.0 {
            notes.add(ir::Impact::Audible, "maximizer TruePeak", "");
        }
        Ok(())
    },
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

/// XpanderFilter (bus/ladder): `[Freq, Q, Drive, Mode, Algorithm, Fat]`.
pub static XPANDER_FILTER: UviModule = UviModule {
    id: ids::XPANDER_FILTER,
    tag: "XpanderFilter",
    attrs: &[
        Attr::num("Freq", 20_000.0),
        Attr::num("Q", 0.0),
        Attr::num("Drive", 0.0),
        Attr::num("Mode", 0.0),
        Attr::num("Algorithm", 0.0),
        Attr::num("Fat", 1.0),
    ],
    report: nothing,
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

/// Drive: `[DriveAmount, Mode]`. ponytail, unverified (no UVI render
/// oracle): it takes the WaveShaper's drive law and `tanh`, normalised to
/// unity at full scale, so a hard drive adds colour, not level; a Mode
/// other than 0 is reported and not carried.
pub static DRIVE: UviModule = UviModule {
    id: ids::DRIVE,
    tag: "Drive",
    attrs: &[Attr::num("DriveAmount", 0.0), Attr::num("Mode", 0.0)],
    report: |node, notes| {
        if num(node, "Mode", 0.0)? != 0.0 {
            notes.add(ir::Impact::Audible, "Drive mode", "");
        }
        Ok(())
    },
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

/// Convolver: `[Wet, Dry]` (preserves `current.kind` impulse index).
pub static CONVOLVER: UviModule = UviModule {
    id: ids::CONVOLVER,
    tag: "Convolver",
    attrs: &[Attr::num("Wet", 1.0), Attr::num("Dry", 0.0)],
    report: nothing,
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
    attrs: &[
        Attr::num("Wet", 1.0),
        Attr::num("Dry", 0.0),
        // How the response was prepared: a script may not move it, or the
        // wet scale would leave the response's level.
        Attr::flag("NormalizePower").held(),
    ],
    report: nothing,
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

/// [`UviModule::load`] for a test: the effect at `node`, which must load.
#[cfg(test)]
pub(super) fn load_one(node: Node, notes: &mut Notes) -> Result<ir::Effect, String> {
    UviModule::load(node, notes)?.ok_or_else(|| format!("{node:?} not carried"))
}

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
    if let Some(m) = by_tag(node.tag_name().name()).filter(|m| !m.attrs.is_empty()) {
        let saved = m.saved(node).ok()?;
        let map = (m.attrs.iter().enumerate())
            .filter(|(_, a)| a.scripted)
            .filter_map(|(i, a)| Some((a.name.to_owned(), u8::try_from(i).ok()?)))
            .collect();
        return Some((m.knob_model(saved.len(), &saved), map));
    }
    match node.tag_name().name() {
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
