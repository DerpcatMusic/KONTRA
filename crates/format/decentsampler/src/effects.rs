//! DecentSampler `<effects>` translation into [`kontra_ir::Effect`].

use std::path::Path;

use kontra_format::Notes;
use kontra_format::builder::SamplePool;
use kontra_format::math::{db_to_gain, gain_to_db};
use kontra_ir as ir;
use roxmltree::Node;

/// Default modulation octave span for DecentSampler filters (20 Hz .. ~20 kHz).
pub(crate) const FILTER_OCTAVES: f32 = 10.0;

/// Fixed 7-voice ensemble shape used for DecentSampler `chorus` effects.
static CHORUS_VOICES: ir::EnsembleVoices = ir::EnsembleVoices {
    delay: [0.004, 0.004, 0.008, 0.008, 0.012, 0.012, 0.015],
    rate: [1.0, 1.0, 1.3, 1.3, 1.6, 1.6, 0.8],
    start: [0.0, 0.5, 0.25, 0.75, 0.1, 0.6, 0.35],
    bend: [0.15; ir::ENSEMBLE_VOICES],
    wobble_rate: [5.0; ir::ENSEMBLE_VOICES],
    wobble: [0.0, 0.0, 0.0, 0.0, 0.08, 0.08, 0.0],
    smooth_hz: [4_000.0; ir::ENSEMBLE_VOICES],
    filter: [ir::Section {
        hz: 6_000.0,
        num: [1.0, 0.0, 0.0],
        den: [1.0, 1.414, 1.0],
    }; ir::ENSEMBLE_VOICES],
};

/// What an effect may load: its IR files become samples and impulses.
pub(crate) struct Assets<'r, 'p> {
    pub base_dir: &'r Path,
    pub pool: SamplePool<'p>,
    pub impulses: Vec<ir::Impulse>,
}

/// Translates all `<effect>` children of an `<effects>` node.
pub(crate) fn translate_effects(
    node: Node<'_, '_>,
    assets: &mut Assets<'_, '_>,
    notes: &mut Notes,
) -> Vec<ir::Effect> {
    let mut out = Vec::new();
    for child in node.children().filter(Node::is_element) {
        if !child.has_tag_name("effect") {
            notes.add(
                ir::Impact::Audible,
                &format!("element <{}>", child.tag_name().name()),
                "inside <effects>",
            );
            continue;
        }
        out.push(translate_effect(child, assets, notes));
    }
    out
}

/// The tempo-synced delay times, in quarter-note beats, a `musical_time`
/// control sends as 0..=20: 1/64T, 1/64, 1/32T, 1/64D, 1/32, 1/16T, 1/32D,
/// 1/16, 1/8T, 1/16D, 1/8, 1/4T, 1/8D, 1/4, 1/2T, 1/4D, 1/2, 1T, 1/2D, 1,
/// 1D (decentsamples.com, "How to add tempo-synced delay", 2024-02-08).
const MUSICAL_BEATS: [f32; 21] = [
    1.0 / 24.0,
    1.0 / 16.0,
    1.0 / 12.0,
    3.0 / 32.0,
    1.0 / 8.0,
    1.0 / 6.0,
    3.0 / 16.0,
    1.0 / 4.0,
    1.0 / 3.0,
    3.0 / 8.0,
    1.0 / 2.0,
    2.0 / 3.0,
    3.0 / 4.0,
    1.0,
    4.0 / 3.0,
    3.0 / 2.0,
    2.0,
    8.0 / 3.0,
    3.0,
    4.0,
    6.0,
];

fn translate_effect(
    node: Node<'_, '_>,
    assets: &mut Assets<'_, '_>,
    notes: &mut Notes,
) -> ir::Effect {
    let bypass = attr_bool(node, "enabled").is_some_and(|e| !e);
    let fx_type = node
        .attribute("type")
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let unit = |kind| (kind, 1.0, 0.0);
    // A wet path mixed against the input: `mix` of it, `1 − mix` dry.
    let mixed = |kind, default: f32| {
        let mix = attr_f32(node, "mix").unwrap_or(default).clamp(0.0, 1.0);
        (kind, mix, 1.0 - mix)
    };

    let (kind, output, dry) = match fx_type.as_str() {
        "lowpass" | "lowpass_4pl" | "lowpass_1pl" | "highpass" | "bandpass" | "notch" => {
            unit(ir::EffectKind::Filter(build_filter(&fx_type, node)))
        }
        "peak" | "low_shelf" | "high_shelf" => {
            let band = build_bell(&fx_type, node, notes);
            unit(ir::EffectKind::Eq([
                band,
                ir::Bell::FLAT,
                ir::Bell::FLAT,
                ir::Bell::FLAT,
            ]))
        }
        "reverb" => {
            let room = attr_f32(node, "roomSize").unwrap_or(0.7).clamp(0.0, 1.0);
            let damping = attr_f32(node, "damping").unwrap_or(0.3).clamp(0.0, 1.0);
            let wet = attr_f32(node, "wetLevel").unwrap_or(0.0).clamp(0.0, 1.0);
            let dry = attr_f32(node, "dryLevel").map_or(1.0 - wet, |d| d.clamp(0.0, 1.0));
            let reverb = ir::Reverb {
                seconds: 0.4 + room * 5.6,
                size: 0.5 + room,
                network: None,
                damping,
                diffusion: 0.7,
                predelay_ms: 0.0,
                high_cut_hz: 12_000.0,
                low_shelf_db: 0.0,
                stereo: 1.0,
                modulation_ms: 0.5,
                modulation_hz: 0.7,
            };
            (ir::EffectKind::Reverb(reverb), wet, dry)
        }
        "delay" => {
            let time = attr_f32(node, "delayTime").unwrap_or(0.7);
            let time = match node.attribute("delayTimeFormat").map(str::trim) {
                Some("musical_time") => {
                    let step = time.round().clamp(0.0, 20.0) as usize;
                    ir::DelayTime::Beats(MUSICAL_BEATS.get(step).copied().unwrap_or(1.0 / 24.0))
                }
                None | Some("seconds") => ir::DelayTime::Ms(time.clamp(0.001, 20.0) * 1000.0),
                Some(mode) => {
                    notes.add(ir::Impact::Audible, "unsupported delay timing mode", mode);
                    ir::DelayTime::Ms(time.clamp(0.001, 20.0) * 1000.0)
                }
            };
            let fb = attr_f32(node, "feedback").unwrap_or(0.2).clamp(0.0, 0.99);
            // Half the offset off the left line, half onto the right.
            let spread_ms = attr_f32(node, "stereoOffset").unwrap_or(0.0) * 500.0;
            let cutoff = attr_f32(node, "feedbackCutoff").unwrap_or(22_000.0);
            let wet = attr_f32(node, "wetLevel").unwrap_or(0.5).clamp(0.0, 1.0);
            let dry = attr_f32(node, "dryLevel").map_or(1.0 - wet, |d| d.clamp(0.0, 1.0));
            let delay = ir::Delay {
                time,
                depth_ms: 0.0,
                second_head: false,
                input: ir::IDENTITY,
                feedback: [[fb, 0.0], [0.0, fb]],
                output: ir::IDENTITY,
                // 22 kHz is the documented "off".
                damping_hz: if cutoff >= 22_000.0 {
                    f32::INFINITY
                } else {
                    cutoff.max(20.0)
                },
                low_cut_hz: 0.0,
                steep_low_cut: false,
                body: std::borrow::Cow::Borrowed(&[]),
                triangle_s: None,
                shape: std::borrow::Cow::Borrowed(&[]),
                noise: [0.0, 0.0],
                high_shelf: None,
                spread_ms,
                rate: ir::Rate::Hz(0.0),
                phase: 0.0,
                direct: 0.0,
            };
            (ir::EffectKind::Delay(delay), wet, dry)
        }
        "chorus" => {
            let rate_hz = attr_f32(node, "modRate").unwrap_or(0.2).max(0.01);
            let depth = attr_f32(node, "modDepth").unwrap_or(0.2).clamp(0.0, 1.0);
            let mix = attr_f32(node, "mix").unwrap_or(0.5).clamp(0.0, 1.0);
            let ensemble = ir::Ensemble {
                voices: ir::Shared::Borrowed(&CHORUS_VOICES),
                span: 1.0,
                hz: rate_hz,
                sweep: depth * 0.005,
                levels: [1.0, 1.0, 0.7, 0.7, 0.0, 0.0, 0.0],
                angles: [-0.2, 0.2, -0.15, 0.15, 0.0, 0.0, 0.0],
                scatter: [0.5, -0.2, -0.2, 0.3, -0.2, 0.3, 0.3],
                feedback: 0.25,
                polarity: ir::Polarity::Normal,
                input: 1.0,
                wet: mix,
                direct: 1.0 - mix,
                shelf: ir::Section {
                    hz: 30.0,
                    num: [0.5, 1.0, 1.0],
                    den: [1.0, 1.414, 1.0],
                },
            };
            unit(ir::EffectKind::Ensemble(ensemble))
        }
        // JUCE's phaser, whose parameters these are: six first-order
        // allpasses at `20·1000^p`, `p` the centre's log position swung by
        // `depth/2·sin` — the centre times `1000^(depth/2·sin)`.
        "phaser" => {
            let depth = attr_f32(node, "modDepth").unwrap_or(0.2).clamp(0.0, 1.0);
            let hz = attr_f32(node, "centerFrequency")
                .unwrap_or(400.0)
                .clamp(20.0, 20_000.0);
            let phaser = ir::Phaser {
                stages: 6,
                triangle: false,
                hz,
                spread_hz: hz,
                exponent: depth * PHASER_EXPONENT,
                feedback: attr_f32(node, "feedback").unwrap_or(0.7).clamp(-0.99, 0.99),
                rate: ir::Rate::Hz(attr_f32(node, "modRate").unwrap_or(0.2).clamp(0.0, 10.0)),
                phase: 0.0,
            };
            mixed(ir::EffectKind::Phaser(phaser), 0.5)
        }
        "convolution" => match impulse(node, assets, notes) {
            Some(impulse) => mixed(ir::EffectKind::Convolution { impulse }, 0.5),
            None => mixed(ir::EffectKind::OtherWet, 0.5),
        },
        "pitch_shift" => {
            let semis = attr_f32(node, "pitchShift")
                .unwrap_or(0.0)
                .clamp(-24.0, 24.0);
            let speed = (semis / 12.0).exp2();
            // ponytail: a 50 ms grain, DecentSampler's own is not documented.
            let shifter = ir::Shifter {
                time: ir::DelayTime::Ms(50.0),
                right: 1.0,
                speed: [speed, speed],
                feedback: 0.0,
                cross: 0.0,
            };
            mixed(ir::EffectKind::Shifter(shifter), 0.5)
        }
        // A triangle fold at ±threshold: `asin(sin(d·x))` with `d =
        // π·drive/(2·threshold)`, scaled back by `2·threshold/π`.
        "wave_folder" => {
            let drive = attr_f32(node, "drive").unwrap_or(1.0).clamp(1.0, 100.0);
            let threshold = attr_f32(node, "threshold")
                .unwrap_or(0.25)
                .clamp(0.001, 10.0);
            let output = threshold * std::f32::consts::FRAC_2_PI;
            (shaper(ir::DriveCurve::Fold(drive / output)), output, 0.0)
        }
        "wave_shaper" => {
            let drive = attr_f32(node, "drive").unwrap_or(1.0).clamp(0.0, 1000.0);
            let output = attr_f32(node, "outputLevel").unwrap_or(0.1).clamp(0.0, 8.0);
            if node.attribute("driveBoost").is_some() {
                notes.add(
                    ir::Impact::Audible,
                    "wave_shaper driveBoost",
                    "curve carried as tanh(drive·x)",
                );
            }
            (shaper(ir::DriveCurve::Tanh(drive.max(0.001))), output, 0.0)
        }
        "bit_crusher" => {
            let bits = attr_f32(node, "bitDepth").unwrap_or(24.0).clamp(1.0, 24.0);
            let factor = attr_f32(node, "sampleRateReduction")
                .unwrap_or(1.0)
                .clamp(1.0, 32.0);
            let crush = ir::Crush {
                // The engine holds `1/rate + 1` frames.
                rate: if factor > 1.0 {
                    1.0 / (factor - 1.0)
                } else {
                    1.0
                },
                bits,
                noise: 0.0,
                noise_tone: 1.0,
                converter: None,
            };
            mixed(ir::EffectKind::Crush(crush), 1.0)
        }
        // Input gain before the detector is the threshold lowered by it and
        // the make-up raised by it.
        "compressor" => {
            let input_db = attr_f32(node, "inputGain")
                .unwrap_or(0.0)
                .clamp(-24.0, 24.0);
            if attr_bool(node, "autoBypass") == Some(true) {
                notes.add(
                    ir::Impact::Cosmetic,
                    "compressor autoBypass",
                    "runs always; below threshold it is unity anyway",
                );
            }
            let compressor = ir::Compressor {
                threshold_db: attr_f32(node, "threshold")
                    .unwrap_or(-12.0)
                    .clamp(-60.0, 0.0)
                    - input_db,
                ratio: attr_f32(node, "ratio").unwrap_or(4.0).clamp(1.0, 20.0),
                knee_db: 0.0,
                attack_ms: attr_f32(node, "attack").unwrap_or(5.0).clamp(0.1, 200.0),
                release_ms: attr_f32(node, "release")
                    .unwrap_or(100.0)
                    .clamp(5.0, 2000.0),
                makeup_db: attr_f32(node, "outputGain")
                    .unwrap_or(0.0)
                    .clamp(-24.0, 24.0)
                    + input_db,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            };
            unit(ir::EffectKind::Compressor(compressor))
        }
        "gain" => {
            let linear = node
                .attribute("levelUnit")
                .is_some_and(|u| u.trim().eq_ignore_ascii_case("linear"));
            let level = node.attribute("level").map(str::trim);
            let gain = match level {
                None if linear => 0.5,
                None => db_to_gain(-6.0),
                Some(l) => match l.strip_suffix("dB").or_else(|| l.strip_suffix("db")) {
                    Some(db) => db.trim().parse().map_or(1.0, db_to_gain),
                    None => match l.parse::<f32>() {
                        Ok(v) if linear => v.clamp(0.0, 8.0),
                        Ok(v) => db_to_gain(v.clamp(-100.0, 24.0)),
                        Err(_) => 1.0,
                    },
                },
            };
            unit(ir::EffectKind::Gain(gain))
        }
        other => {
            notes.add(ir::Impact::Audible, &format!("effect {other}"), "");
            unit(ir::EffectKind::Other)
        }
    };

    ir::Effect {
        kind,
        bypass,
        output,
        dry,
    }
}

/// `ln(1000)/2`: the phaser's sweep exponent per unit of `modDepth`.
pub(crate) const PHASER_EXPONENT: f32 = 3.453_877_6;

fn shaper(curve: ir::DriveCurve) -> ir::EffectKind {
    ir::EffectKind::Shaper(ir::Shaper {
        pre_lowpass_hz: None,
        curve,
        lowpass_hz: None,
        highpass_hz: None,
        sections: ir::Sections::NONE,
    })
}

/// The `irFile` as a sample and an impulse; `None` (reported) when absent.
fn impulse(node: Node<'_, '_>, assets: &mut Assets<'_, '_>, notes: &mut Notes) -> Option<u16> {
    let Some(raw) = node
        .attribute("irFile")
        .map(str::trim)
        .filter(|p| !p.is_empty())
    else {
        notes.add(ir::Impact::Audible, "convolution without irFile", "");
        return None;
    };
    let path = raw.replace('\\', "/");
    let full = assets.base_dir.join(&path);
    let (sample, header) = assets.pool.add(path, &full, notes);
    header?;
    assets.impulses.push(ir::Impulse {
        sample,
        ..ir::Impulse::PLAIN
    });
    u16::try_from(assets.impulses.len() - 1).ok()
}

fn build_filter(fx_type: &str, node: Node<'_, '_>) -> ir::Filter {
    // `lowpass_4pl` is the legacy name of the 2-pole `lowpass`.
    let (pass, stages, default_hz) = match fx_type {
        "lowpass_1pl" => (ir::Pass::Low, 0, 22_000.0),
        "highpass" => (ir::Pass::High, 1, 22_000.0),
        "bandpass" => (ir::Pass::Band, 1, 22_000.0),
        "notch" => (ir::Pass::Notch, 1, 10_000.0),
        _ => (ir::Pass::Low, 1, 22_000.0),
    };
    let hz = attr_f32(node, "frequency")
        .unwrap_or(default_hz)
        .clamp(20.0, 22_000.0);
    let q = if pass == ir::Pass::Notch {
        attr_q(node).unwrap_or(0.7).clamp(0.01, 18.0)
    } else {
        attr_f32(node, "resonance").unwrap_or(0.7).clamp(0.01, 5.0)
    };
    let damping = 1.0 / q;
    ir::Filter {
        pass,
        hz,
        stages,
        ladder: None,
        damping: [damping; 3],
        gain: if pass == ir::Pass::Band { damping } else { 1.0 },
        octaves: FILTER_OCTAVES,
        q_octaves: 4.0,
    }
}

/// A `peak`, `low_shelf` or `high_shelf` band.
fn build_bell(fx_type: &str, node: Node<'_, '_>, notes: &mut Notes) -> ir::Bell {
    let (shape, default_hz) = match fx_type {
        "low_shelf" => (ir::Shape::LowShelf, 200.0),
        "high_shelf" => (ir::Shape::HighShelf, 5_000.0),
        _ => (ir::Shape::Peak, 10_000.0),
    };
    let hz = attr_f32(node, "frequency")
        .unwrap_or(default_hz)
        .clamp(20.0, 22_000.0);
    let q = attr_q(node).unwrap_or(0.7).clamp(0.01, 18.0);
    let gain_db = attr_f32(node, "gain").map_or(0.0, |g| filter_gain_db(node, g));
    let q = if shape == ir::Shape::Peak {
        q
    } else {
        let slope = shelf_slope(q, gain_db);
        if slope > 1.001 {
            notes.add(
                ir::Impact::Audible,
                "shelf q above 0.707",
                "its corner bump is not carried",
            );
        }
        slope
    };
    ir::Bell {
        shape,
        hz,
        q,
        gain_db,
    }
}

/// A peak or shelf `gain` in dB: linear unless `gainUnit="decibels"`.
fn filter_gain_db(node: Node<'_, '_>, gain: f32) -> f32 {
    if gain_in_db(node) {
        gain.clamp(-80.0, 20.0)
    } else {
        gain_to_db(gain.clamp(0.0001, 10.0))
    }
}

/// The effect's `gain` is written in dB (`gainUnit="decibels"`).
pub(crate) fn gain_in_db(node: Node<'_, '_>) -> bool {
    node.attribute("gainUnit")
        .is_some_and(|u| u.trim().eq_ignore_ascii_case("decibels"))
}

/// A shelf's cookbook Q as the slope `S` the engine's shelf takes:
/// `1/Q² = (A + 1/A)(1/S − 1) + 2`, `A = 10^(dB/40)`.
fn shelf_slope(q: f32, gain_db: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    1.0 / ((1.0 / (q * q) - 2.0) / (a + 1.0 / a) + 1.0).max(1e-6)
}

/// The `Q` attribute, written `q` in the guide's own examples.
fn attr_q(node: Node<'_, '_>) -> Option<f32> {
    attr_f32(node, "Q").or_else(|| attr_f32(node, "q"))
}

pub(crate) fn attr_f32(node: Node<'_, '_>, name: &str) -> Option<f32> {
    node.attribute(name)?
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
}

pub(crate) fn attr_bool(node: Node<'_, '_>, name: &str) -> Option<bool> {
    let v = node.attribute(name)?.trim();
    if v.eq_ignore_ascii_case("true") || v == "1" {
        Some(true)
    } else if v.eq_ignore_ascii_case("false") || v == "0" {
        Some(false)
    } else {
        None
    }
}
