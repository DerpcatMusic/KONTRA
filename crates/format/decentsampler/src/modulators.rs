//! DecentSampler `<modulators>` (`<lfo>`, `<envelope>`) and `<midi>` (`<cc>`)
//! bindings translated into [`kontra_ir::Mod`].

use std::collections::HashMap;

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

use crate::effects::{FILTER_OCTAVES, attr_f32};

/// Mapping context from DecentSampler XML positions/tags to emitted IR groups.
pub(crate) struct TargetMap<'a> {
    /// Emitted IR group indices for each XML `<group>` index.
    pub group_indices: &'a [Vec<usize>],
    /// Destination slots, indexed by IR group, for each promoted instrument insert.
    pub promoted_filters: HashMap<u8, Vec<u8>>,
    /// Tag name -> `voice_group` index for `level="tag"` bindings.
    pub tag_ids: &'a HashMap<String, usize>,
}

/// Translates a `<modulators>` section (`<lfo>` and `<envelope>` elements).
pub(crate) fn translate_modulators(
    node: Node<'_, '_>,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    targets: &mut TargetMap<'_>,
    notes: &mut Notes,
) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "lfo" => apply_lfo(child, groups, global_inserts, targets, notes),
            "envelope" => apply_envelope(child, groups, global_inserts, targets, notes),
            other => notes.add(ir::Impact::Audible, &format!("modulator <{other}>"), ""),
        }
    }
}

/// Translates a `<midi>` section (`<cc>` bindings).
pub(crate) fn translate_midi(
    node: Node<'_, '_>,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    targets: &mut TargetMap<'_>,
    notes: &mut Notes,
) {
    for child in node.children().filter(Node::is_element) {
        if !child.has_tag_name("cc") {
            notes.add(
                ir::Impact::Audible,
                &format!("midi element <{}>", child.tag_name().name()),
                "",
            );
            continue;
        }
        let Some(cc) = child
            .attribute("number")
            .and_then(|s| s.trim().parse::<u8>().ok())
            .filter(|&n| n <= 127)
        else {
            notes.add(
                ir::Impact::Audible,
                "invalid midi cc number",
                child.attribute("number").unwrap_or(""),
            );
            continue;
        };
        let source = ir::Source::Controller(cc);
        let label = format!("cc{cc}");
        for binding in child.children().filter(Node::is_element) {
            if !binding.has_tag_name("binding") {
                continue;
            }
            apply_binding(
                binding,
                BindingSource::Direct(source),
                1.0,
                5.0,
                &label,
                groups,
                global_inserts,
                targets,
                notes,
            );
        }
    }
}

enum BindingSource {
    Direct(ir::Source),
    Lfo(ir::Lfo),
    Envelope(ir::Breakpoints),
}

fn apply_lfo(
    node: Node<'_, '_>,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    targets: &mut TargetMap<'_>,
    notes: &mut Notes,
) {
    let freq = attr_f32(node, "frequency").unwrap_or(1.0).max(0.001);
    let mod_amount = attr_f32(node, "modAmount").unwrap_or(1.0);
    let phase = attr_f32(node, "phase").unwrap_or(0.0);
    let fade_ms = attr_f32(node, "fadeIn").unwrap_or(0.0).max(0.0) * 1000.0;

    let wave = match node
        .attribute("waveform")
        .unwrap_or("sine")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "sine" => ir::Wave::Sine,
        "triangle" => ir::Wave::Triangle,
        "square" => ir::Wave::Square { width: 0.5 },
        "saw" | "sawtooth" => ir::Wave::Saw,
        "ramp" => ir::Wave::Ramp,
        "random" | "sample_and_hold" => ir::Wave::Random,
        other => {
            notes.add(ir::Impact::Audible, "lfo waveform", other);
            ir::Wave::Sine
        }
    };

    let clock = match node
        .attribute("scope")
        .unwrap_or("voice")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "voice" => ir::Clock::Voice,
        "global" | "instrument" => ir::Clock::Free,
        other => {
            notes.add(ir::Impact::Audible, "lfo scope", other);
            ir::Clock::Voice
        }
    };

    let lfo = ir::Lfo {
        wave,
        rate: ir::Rate::Hz(freq),
        phase,
        fade_ms,
        clock,
    };

    for binding in node.children().filter(Node::is_element) {
        if !binding.has_tag_name("binding") {
            notes.add(
                ir::Impact::Audible,
                &format!("LFO element <{}>", binding.tag_name().name()),
                "",
            );
            continue;
        }
        apply_binding(
            binding,
            BindingSource::Lfo(lfo),
            mod_amount,
            0.0,
            "lfo",
            groups,
            global_inserts,
            targets,
            notes,
        );
    }
}

fn apply_envelope(
    node: Node<'_, '_>,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    targets: &mut TargetMap<'_>,
    notes: &mut Notes,
) {
    let attack = attr_f32(node, "attack").unwrap_or(0.01).max(0.0);
    let decay = attr_f32(node, "decay").unwrap_or(0.1).max(0.0);
    let sustain = attr_f32(node, "sustain").unwrap_or(1.0).clamp(0.0, 1.0);
    let release = attr_f32(node, "release").unwrap_or(0.1).max(0.0);
    let mod_amount = attr_f32(node, "modAmount").unwrap_or(1.0);

    let breakpoints = ir::Breakpoints {
        start: 0.0,
        points: vec![
            ir::Breakpoint {
                ms: attack * 1000.0,
                level: 1.0,
                curve: 0.0,
            },
            ir::Breakpoint {
                ms: 0.0,
                level: 1.0,
                curve: 0.0,
            },
            ir::Breakpoint {
                ms: decay * 1000.0,
                level: sustain,
                curve: 0.0,
            },
            ir::Breakpoint {
                ms: release * 1000.0,
                level: 0.0,
                curve: 0.0,
            },
        ],
        sustain: Some(2),
        loop_from: None,
        stages: true,
    };

    for binding in node.children().filter(Node::is_element) {
        if !binding.has_tag_name("binding") {
            continue;
        }
        apply_binding(
            binding,
            BindingSource::Envelope(breakpoints.clone()),
            mod_amount,
            0.0,
            "envelope",
            groups,
            global_inserts,
            targets,
            notes,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_binding(
    binding: Node<'_, '_>,
    binding_source: BindingSource,
    mod_amount: f32,
    tau_ms: f32,
    label: &str,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    targets: &mut TargetMap<'_>,
    notes: &mut Notes,
) {
    let param = binding.attribute("parameter").unwrap_or("").trim();
    if let Some(translation) = binding.attribute("translation")
        && !translation.trim().eq_ignore_ascii_case("linear")
    {
        notes.add(ir::Impact::Audible, "binding translation", translation);
    }

    let level = binding
        .attribute("level")
        .unwrap_or("instrument")
        .trim()
        .to_ascii_lowercase();
    let target_indices = resolve_target_groups(&level, binding, groups, targets, notes);
    if target_indices.is_empty() {
        return;
    }

    let out_min = attr_f32(binding, "translationOutputMin");
    let out_max = attr_f32(binding, "translationOutputMax");
    let effect_index = binding
        .attribute("effectIndex")
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    let normalized_param = param.to_ascii_uppercase();
    // If an instrument-level filter is modulated, promote it to per-voice group inserts
    // so ModKind::Cutoff can modulate it on every voice.
    let slots = if normalized_param == "FX_FILTER_FREQUENCY" {
        let Some(slot) = ensure_voice_filter(
            &target_indices,
            effect_index,
            &level,
            groups,
            global_inserts,
            &mut targets.promoted_filters,
            notes,
        ) else {
            notes.add(
                ir::Impact::Audible,
                "filter cutoff binding without filter",
                format!("{label} -> slot {effect_index}"),
            );
            return;
        };
        slot
    } else {
        vec![effect_index; target_indices.len()]
    };

    let mut unsupported = false;
    for (&g_idx, resolved_insert) in target_indices.iter().zip(slots) {
        let Some(group) = groups.get_mut(g_idx) else {
            continue;
        };
        let source = match &binding_source {
            BindingSource::Direct(src) => *src,
            BindingSource::Lfo(lfo) => {
                let idx = u8::try_from(group.lfos.len()).unwrap_or(u8::MAX);
                group.lfos.push(*lfo);
                ir::Source::Lfo(idx)
            }
            BindingSource::Envelope(bp) => {
                let idx = u8::try_from(group.envelopes.len()).unwrap_or(u8::MAX);
                group.envelopes.push(bp.clone());
                ir::Source::Envelope(idx)
            }
        };

        let kind = match normalized_param.as_str() {
            "AMP_VOLUME" | "GROUP_VOLUME" => {
                let span = match (out_min, out_max) {
                    (Some(lo), Some(hi)) => (hi - lo) * mod_amount,
                    _ => mod_amount,
                };
                let curve = match source {
                    ir::Source::Controller(_) => ir::Curve::Depth { power: 1 },
                    _ => ir::Curve::Signed,
                };
                ir::ModKind::Volume {
                    source,
                    intensity: span.clamp(-1.0, 1.0),
                    curve,
                    tau_ms,
                }
            }
            "FX_FILTER_FREQUENCY"
                if binding.attribute("modBehavior") == Some("add")
                    && let (Some(lo), Some(hi)) = (out_min, out_max) =>
            {
                ir::ModKind::CutoffHz {
                    source,
                    insert: resolved_insert,
                    min_hz: lo * mod_amount,
                    max_hz: hi * mod_amount,
                }
            }
            "FX_FILTER_FREQUENCY" => {
                let octaves = group
                    .inserts
                    .get(usize::from(resolved_insert))
                    .and_then(|s| s.as_ref())
                    .and_then(|e| match e.kind {
                        ir::EffectKind::Filter(f) if f.octaves > 0.0 => Some(f.octaves),
                        _ => None,
                    })
                    .unwrap_or(FILTER_OCTAVES);
                let depth = match (out_min, out_max) {
                    (Some(lo), Some(hi)) if lo > 0.0 && hi > 0.0 => {
                        ((hi / lo).log2() / octaves) * mod_amount
                    }
                    _ => mod_amount,
                };
                ir::ModKind::Cutoff {
                    source,
                    insert: resolved_insert,
                    depth: depth.clamp(-1.0, 1.0),
                }
            }
            "PITCH" | "GROUP_TUNING" | "GLOBAL_TUNING" => {
                let semitones = match (out_min, out_max) {
                    (Some(lo), Some(hi)) => {
                        if matches!(source, ir::Source::Controller(_)) {
                            group.tune += f64::from(lo * mod_amount);
                        }
                        (hi - lo) * mod_amount
                    }
                    _ => mod_amount,
                };
                ir::ModKind::Pitch {
                    source,
                    semitones,
                    depth: 1.0,
                    tau_ms,
                }
            }
            "PAN" | "GROUP_PAN" => {
                let amount = match (out_min, out_max) {
                    (Some(lo), Some(hi)) => ((hi - lo) / 100.0) * mod_amount,
                    _ => mod_amount,
                };
                ir::ModKind::Pan {
                    source,
                    amount: amount.clamp(-1.0, 1.0),
                }
            }
            _ => {
                unsupported = true;
                break;
            }
        };

        group.mods.push(ir::Mod {
            name: format!("{label}:{param}"),
            slot: 0,
            target: 0,
            bypass: false,
            kind,
        });
    }

    if unsupported {
        notes.add(ir::Impact::Audible, &format!("binding {param}"), label);
    }
}

fn resolve_target_groups(
    level: &str,
    binding: Node<'_, '_>,
    groups: &[ir::Group],
    targets: &TargetMap<'_>,
    notes: &mut Notes,
) -> Vec<usize> {
    match level {
        "instrument" | "" => (0..groups.len()).collect(),
        "group" => {
            let pos = binding
                .attribute("position")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            targets.group_indices.get(pos).cloned().unwrap_or_default()
        }
        "tag" => {
            let tag = binding.attribute("tags").unwrap_or("").trim();
            let Some(&vg_idx) = targets.tag_ids.get(tag) else {
                return Vec::new();
            };
            groups
                .iter()
                .enumerate()
                .filter_map(|(i, g)| (g.voice_group == Some(vg_idx)).then_some(i))
                .collect()
        }
        other => {
            notes.add(ir::Impact::Audible, "binding level", other);
            Vec::new()
        }
    }
}

/// Ensures target groups have a filter insert to receive [`ir::ModKind::Cutoff`].
/// If targeting an instrument-level filter, promotes that filter into each target
/// group's insert rack and bypasses the global copy so it is not applied twice.
fn ensure_voice_filter(
    target_indices: &[usize],
    effect_index: u8,
    level: &str,
    groups: &mut [ir::Group],
    global_inserts: &mut [Option<ir::Effect>],
    promoted_filters: &mut HashMap<u8, Vec<u8>>,
    notes: &mut Notes,
) -> Option<Vec<u8>> {
    let idx = usize::from(effect_index);
    if !matches!(level, "instrument" | "") {
        return target_indices
            .iter()
            .all(|&g_idx| {
                groups
                    .get(g_idx)
                    .and_then(|group| group.inserts.get(idx))
                    .and_then(Option::as_ref)
                    .is_some_and(|e| matches!(e.kind, ir::EffectKind::Filter(_)))
            })
            .then(|| vec![effect_index; target_indices.len()]);
    }

    let resolve_slots = |slots: &[u8]| {
        target_indices
            .iter()
            .map(|&g_idx| slots.get(g_idx).copied())
            .collect::<Option<Vec<_>>>()
    };
    if let Some(slots) = promoted_filters.get(&effect_index) {
        return resolve_slots(slots);
    }

    let reordered = global_inserts
        .get(..idx)
        .into_iter()
        .flatten()
        .flatten()
        .any(|fx| !fx.bypass);

    let global_fx = global_inserts.get_mut(idx)?.as_mut()?;
    if !matches!(global_fx.kind, ir::EffectKind::Filter(_)) {
        return None;
    }

    // Instrument bindings always target every group. Validate all destinations
    // before mutating either rack, so failure leaves the original filter intact.
    let slots: Vec<u8> = groups
        .iter()
        .map(|group| u8::try_from(group.inserts.len()))
        .collect::<Result<_, _>>()
        .ok()?;
    let resolved = resolve_slots(&slots)?;
    let promoted = *global_fx;
    global_fx.bypass = true;
    for group in groups {
        group.inserts.push(Some(promoted));
    }
    if reordered {
        notes.add(
            ir::Impact::Audible,
            "promoted global filter reordered before preceding global effects",
            format!("slot {effect_index}"),
        );
    }
    promoted_filters.insert(effect_index, slots);
    Some(resolved)
}
