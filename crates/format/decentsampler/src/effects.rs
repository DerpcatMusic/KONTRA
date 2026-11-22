//! DecentSampler `<effects>` translation into [`kontra_ir::Effect`].

use kontra_format::Notes;
use kontra_format::math::db_to_gain;
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

/// Translates all `<effect>` children of an `<effects>` node.
pub(crate) fn translate_effects(node: Node<'_, '_>, notes: &mut Notes) -> Vec<ir::Effect> {
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
        out.push(translate_effect(child, notes));
    }
    out
}

fn translate_effect(node: Node<'_, '_>, notes: &mut Notes) -> ir::Effect {
    let mut bypass = attr_bool(node, "enabled").is_some_and(|e| !e);
    let fx_type = node.attribute("type").unwrap_or("").trim();

    let (kind, output, dry) = match fx_type.to_ascii_lowercase().as_str() {
        "lowpass" | "lowpass_4pl" | "lowpass_1pl" | "highpass" | "bandpass" | "notch" => {
            let filter = build_filter(fx_type, node);
            (ir::EffectKind::Filter(filter), 1.0, 0.0)
        }
        "reverb" => {
            let room = attr_f32(node, "roomSize").unwrap_or(0.5).clamp(0.0, 1.0);
            let damping = attr_f32(node, "damping").unwrap_or(0.5).clamp(0.0, 1.0);
            let wet = attr_f32(node, "wetLevel").unwrap_or(0.2).clamp(0.0, 1.0);
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
            let time = match node.attribute("delayTimeFormat").map(str::trim) {
                None | Some("seconds") => ir::DelayTime::Ms(
                    attr_f32(node, "delayTime").unwrap_or(0.5).max(0.001) * 1000.0,
                ),
                Some(mode) => {
                    notes.add(ir::Impact::Audible, "unsupported delay timing mode", mode);
                    bypass = true;
                    ir::DelayTime::Ms(500.0)
                }
            };
            let fb = attr_f32(node, "feedback").unwrap_or(0.2).clamp(0.0, 0.99);
            let spread_ms = attr_f32(node, "stereoOffset").unwrap_or(0.0) * 1000.0;
            let wet = attr_f32(node, "wetLevel").unwrap_or(0.5).clamp(0.0, 1.0);
            let dry = attr_f32(node, "dryLevel").map_or(1.0 - wet, |d| d.clamp(0.0, 1.0));
            let delay = ir::Delay {
                time,
                depth_ms: 0.0,
                second_head: false,
                input: ir::IDENTITY,
                feedback: [[fb, 0.0], [0.0, fb]],
                output: ir::IDENTITY,
                damping_hz: f32::INFINITY,
                low_cut_hz: 0.0,
                steep_low_cut: false,
                body: &[],
                triangle_s: None,
                shape: &[],
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
                voices: &CHORUS_VOICES,
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
            (ir::EffectKind::Ensemble(ensemble), 1.0, 0.0)
        }
        "gain" => {
            let gain = attr_f32(node, "level").map_or(1.0, |level| {
                if node
                    .attribute("levelUnit")
                    .is_some_and(|u| u.trim().eq_ignore_ascii_case("linear"))
                {
                    level
                } else {
                    db_to_gain(level)
                }
            });
            (ir::EffectKind::Gain(gain), 1.0, 0.0)
        }
        other => {
            notes.add(ir::Impact::Audible, &format!("effect {other}"), "");
            (ir::EffectKind::Other, 1.0, 0.0)
        }
    };

    ir::Effect {
        kind,
        bypass,
        output,
        dry,
    }
}

fn build_filter(fx_type: &str, node: Node<'_, '_>) -> ir::Filter {
    let (pass, stages, default_hz) = match fx_type.to_ascii_lowercase().as_str() {
        "lowpass_1pl" => (ir::Pass::Low, 0, 22_000.0),
        "lowpass_4pl" => (ir::Pass::Low, 2, 22_000.0),
        "highpass" => (ir::Pass::High, 1, 20.0),
        "bandpass" => (ir::Pass::Band, 1, 1_000.0),
        "notch" => (ir::Pass::Notch, 1, 1_000.0),
        _ => (ir::Pass::Low, 1, 22_000.0),
    };
    let hz = attr_f32(node, "frequency")
        .unwrap_or(default_hz)
        .clamp(20.0, 22_000.0);
    let q = attr_f32(node, "resonance").unwrap_or(0.707).max(0.1);
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
