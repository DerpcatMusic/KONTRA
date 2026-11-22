//! Core `.dspreset` XML translation into [`kontra_ir::Instrument`] using the
//! `kontra-format` SDK (`SamplePool`, `InstrumentBuilder`, `GroupBuilder`, `ZoneBuilder`).

use std::collections::HashMap;
use std::io;
use std::path::Path;

use kontra_format::Notes;
use kontra_format::builder::{GroupBuilder, InstrumentBuilder, SamplePool, ZoneBuilder};
use kontra_format::header::AudioHeader;
use kontra_format::math::{clamp_midi, gain_to_db, parse_midi_note};
use kontra_ir as ir;
use roxmltree::Node;

use crate::effects::{attr_bool, attr_f32, translate_effects};
use crate::modulators::{TargetMap, translate_midi, translate_modulators};

/// Shared inheritable settings (`<groups>` -> `<group>` -> `<sample>`).
#[derive(Clone, Debug, PartialEq)]
struct Inherited {
    volume_db: f32,
    pan: f32,
    tuning: f64,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    amp_vel_track: f32,
    pitch_key_track: f64,
    seq_mode: Option<SeqMode>,
    seq_length: Option<u8>,
    seq_position: Option<u8>,
    trigger: ir::Trigger,
    silencing_mode: SilencingMode,
    tags: Vec<String>,
    silenced_by_tags: Vec<String>,
}

impl Default for Inherited {
    fn default() -> Self {
        Self {
            volume_db: 0.0,
            pan: 0.0,
            tuning: 0.0,
            attack: 0.0,
            decay: 0.0,
            sustain: 1.0,
            release: 0.01,
            amp_vel_track: 1.0,
            pitch_key_track: 1.0,
            seq_mode: None,
            seq_length: None,
            seq_position: None,
            trigger: ir::Trigger::Attack,
            silencing_mode: SilencingMode::Fast,
            tags: Vec::new(),
            silenced_by_tags: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeqMode {
    Always,
    RoundRobin,
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SilencingMode {
    Fast,
    Normal,
}

/// Maps DecentSampler tag names to [`ir::VoiceGroup`] indices.
#[derive(Default)]
struct TagRegistry {
    index: HashMap<String, usize>,
    voice_groups: Vec<ir::VoiceGroup>,
}

impl TagRegistry {
    fn intern(&mut self, tag: &str) -> usize {
        if let Some(&idx) = self.index.get(tag) {
            return idx;
        }
        let idx = self.voice_groups.len();
        self.voice_groups.push(ir::VoiceGroup::DEFAULT);
        self.index.insert(tag.to_owned(), idx);
        idx
    }

    fn set_polyphony(&mut self, tag: &str, voices: u16) {
        let idx = self.intern(tag);
        if let Some(vg) = self.voice_groups.get_mut(idx) {
            vg.voices = voices;
        }
    }

    fn apply_silencing_mode(&mut self, idx: usize, mode: SilencingMode) {
        if let Some(vg) = self.voice_groups.get_mut(idx) {
            match mode {
                SilencingMode::Normal => vg.release = true,
                SilencingMode::Fast => {
                    vg.release = false;
                    vg.fade_ms = 10.0;
                }
            }
        }
    }
}

pub(crate) fn translate(
    root: Node<'_, '_>,
    base_dir: &Path,
    probe: &dyn Fn(&Path) -> io::Result<AudioHeader>,
) -> ir::Instrument {
    let mut notes = Notes::default();
    let mut pool = SamplePool::with_prober(probe);
    let mut tags = TagRegistry::default();
    let mut builder = InstrumentBuilder::new("");

    let mut groups_nodes = Vec::new();
    let mut effects_nodes = Vec::new();
    let mut modulators_nodes = Vec::new();
    let mut midi_nodes = Vec::new();

    for child in root.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "tags" => parse_tags_section(child, &mut tags, &mut notes),
            "groups" => groups_nodes.push(child),
            "effects" => effects_nodes.push(child),
            "modulators" => modulators_nodes.push(child),
            "midi" => midi_nodes.push(child),
            "ui" => notes.add(ir::Impact::Cosmetic, "element <ui>", "visual layout"),
            other => notes.add(ir::Impact::Audible, &format!("element <{other}>"), ""),
        }
    }

    // Global effects
    for fx_node in effects_nodes {
        for effect in translate_effects(fx_node, &mut notes) {
            builder = builder.add_insert(effect);
        }
    }

    // Groups & samples
    let mut xml_group_map: Vec<Vec<usize>> = Vec::new();
    let single_groups = groups_nodes.len() == 1;
    for groups_node in groups_nodes {
        let mut global_defaults = Inherited::default();
        apply_inherited(&mut global_defaults, groups_node, &mut notes);
        if single_groups {
            builder = builder.output(ir::Output {
                volume_db: std::mem::take(&mut global_defaults.volume_db),
                ..ir::Output::UNITY
            });
        }
        if let Some(global_tuning) = attr_f64(groups_node, "globalTuning") {
            global_defaults.tuning += global_tuning;
        }

        let group_children: Vec<Node<'_, '_>> =
            groups_node.children().filter(Node::is_element).collect();
        let inferred_group_seq_len = group_children
            .iter()
            .filter(|n| n.has_tag_name("group"))
            .filter_map(|n| attr_u8(*n, "seqPosition"))
            .max()
            .unwrap_or(1);

        for child in group_children {
            if !child.has_tag_name("group") {
                notes.add(
                    ir::Impact::Audible,
                    &format!("element <{}>", child.tag_name().name()),
                    "inside <groups>",
                );
                continue;
            }
            let emitted = translate_group(
                child,
                &global_defaults,
                inferred_group_seq_len,
                base_dir,
                &mut pool,
                &mut tags,
                &mut builder,
                &mut notes,
            );
            xml_group_map.push(emitted);
        }
    }

    // Modulators & MIDI CC bindings
    let mut target_map = TargetMap {
        group_indices: &xml_group_map,
        tag_ids: &tags.index,
        promoted_filters: HashMap::new(),
    };
    {
        // Temporarily move out global inserts to avoid double-borrowing `builder`.
        let mut global_inserts = std::mem::take(builder.inserts_mut());
        for mod_node in modulators_nodes {
            translate_modulators(
                mod_node,
                builder.groups_mut(),
                &mut global_inserts,
                &mut target_map,
                &mut notes,
            );
        }
        for midi_node in midi_nodes {
            translate_midi(
                midi_node,
                builder.groups_mut(),
                &mut global_inserts,
                &mut target_map,
                &mut notes,
            );
        }
        *builder.inserts_mut() = global_inserts;
    }

    for vg in tags.voice_groups {
        builder = builder.add_voice_group(vg);
    }

    builder.samples(pool.into_samples()).build(notes)
}

fn parse_tags_section(node: Node<'_, '_>, tags: &mut TagRegistry, notes: &mut Notes) {
    for child in node.children().filter(Node::is_element) {
        if !child.has_tag_name("tag") {
            notes.add(
                ir::Impact::Audible,
                &format!("element <{}>", child.tag_name().name()),
                "inside <tags>",
            );
            continue;
        }
        if let Some(name) = child
            .attribute("name")
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(poly) = child
                .attribute("polyphony")
                .and_then(|s| s.trim().parse::<u16>().ok())
                .filter(|&v| v > 0)
            {
                tags.set_polyphony(name, poly);
            } else {
                tags.intern(name);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn translate_group(
    group_node: Node<'_, '_>,
    global_defaults: &Inherited,
    inferred_group_seq_len: u8,
    base_dir: &Path,
    pool: &mut SamplePool<'_>,
    tags: &mut TagRegistry,
    builder: &mut InstrumentBuilder,
    notes: &mut Notes,
) -> Vec<usize> {
    if attr_bool(group_node, "enabled") == Some(false) {
        return Vec::new();
    }

    let mut group_cfg = global_defaults.clone();
    apply_inherited(&mut group_cfg, group_node, notes);
    let group_seq_length = group_cfg.seq_length;
    if group_cfg.seq_length.is_none() && group_cfg.seq_position.is_some() {
        group_cfg.seq_length = Some(inferred_group_seq_len);
    }
    let group_name = group_node.attribute("name").unwrap_or("").to_owned();

    let mut group_effects = Vec::new();
    let mut sample_nodes = Vec::new();
    for child in group_node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "sample" => sample_nodes.push(child),
            "effects" => group_effects.extend(translate_effects(child, notes)),
            other => notes.add(
                ir::Impact::Audible,
                &format!("element <{other}>"),
                "inside <group>",
            ),
        }
    }

    let inferred_sample_seq_len = sample_nodes
        .iter()
        .filter_map(|n| attr_u8(*n, "seqPosition"))
        .max()
        .unwrap_or(1);

    let mut primary_zones: Vec<ir::Zone> = Vec::new();
    let mut split_groups: Vec<(Inherited, ir::Zone)> = Vec::new();

    for sample_node in sample_nodes {
        let Some(raw_path) = sample_node.attribute("path").map(str::trim) else {
            notes.add(ir::Impact::Broken, "sample without path", &group_name);
            continue;
        };
        let norm_path = raw_path.replace('\\', "/");
        let full_path = base_dir.join(&norm_path);
        let (sample_idx, hdr) = pool.add(norm_path, &full_path, notes);
        let total_frames = hdr.map_or(0, |h| h.frames);
        let embedded_loops = hdr.map(|h| h.loops.clone()).unwrap_or_default();

        let lo_note = sample_node
            .attribute("loNote")
            .and_then(parse_midi_note)
            .unwrap_or(0);
        let hi_note = sample_node
            .attribute("hiNote")
            .and_then(parse_midi_note)
            .unwrap_or(127);
        let root_note = sample_node
            .attribute("rootNote")
            .and_then(parse_midi_note)
            .unwrap_or(if lo_note == hi_note { lo_note } else { 60 });
        let lo_vel = sample_node
            .attribute("loVel")
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map_or(1, clamp_midi);
        let hi_vel = sample_node
            .attribute("hiVel")
            .and_then(|s| s.trim().parse::<i32>().ok())
            .map_or(127, clamp_midi);

        let start = attr_u64(sample_node, "start")
            .unwrap_or(0)
            .min(total_frames);
        let end = attr_u64(sample_node, "end")
            .unwrap_or(total_frames)
            .min(total_frames);

        let zone_vol = sample_node
            .attribute("volume")
            .and_then(parse_volume_db)
            .unwrap_or(0.0);
        let zone_pan = attr_f32(sample_node, "pan").map_or(0.0, |p| (p / 100.0).clamp(-1.0, 1.0));
        let zone_tune = attr_f64(sample_node, "tuning").unwrap_or(0.0);

        let mut zone_builder = ZoneBuilder::new(sample_idx, total_frames)
            .keys(lo_note.min(hi_note)..=lo_note.max(hi_note))
            .velocities(lo_vel.min(hi_vel)..=lo_vel.max(hi_vel))
            .root(root_note)
            .gain_db(zone_vol)
            .pan(zone_pan)
            .tune(zone_tune)
            .range(start, end);

        if attr_bool(sample_node, "loopEnabled").unwrap_or(false) {
            let crossfade = attr_u64(sample_node, "loopCrossfade").unwrap_or(0);
            let explicit_start = attr_u64(sample_node, "loopStart");
            let explicit_end = attr_u64(sample_node, "loopEnd");
            if explicit_start.is_none() && explicit_end.is_none() && !embedded_loops.is_empty() {
                let loops = embedded_loops
                    .iter()
                    .map(|l| {
                        let mut ir_loop = l.to_ir_loop(false);
                        ir_loop.crossfade = crossfade;
                        ir_loop
                    })
                    .collect();
                zone_builder = zone_builder.loops(loops);
            } else {
                let l_start = explicit_start.unwrap_or(start).min(total_frames);
                let l_end = explicit_end.unwrap_or(end).min(total_frames);
                if l_end > l_start {
                    zone_builder = zone_builder.add_loop(ir::Loop {
                        start: l_start,
                        end: l_end,
                        crossfade,
                        ..ir::Loop::default()
                    });
                }
            }
        }

        let zone = zone_builder.build();

        // Check if this sample overrides any group-level IR properties.
        let mut sample_group_cfg = group_cfg.clone();
        let has_override = apply_sample_group_overrides(&mut sample_group_cfg, sample_node, notes);
        if attr_u8(sample_node, "seqPosition").is_some()
            && attr_u8(sample_node, "seqLength").is_none()
            && group_seq_length.is_none()
        {
            sample_group_cfg.seq_length = Some(inferred_sample_seq_len);
        }
        if has_override && sample_group_cfg != group_cfg {
            split_groups.push((sample_group_cfg, zone));
        } else {
            primary_zones.push(zone);
        }
    }

    let mut emitted_indices = Vec::new();
    if !primary_zones.is_empty() || split_groups.is_empty() {
        let ir_group = build_ir_group(
            &group_name,
            &group_cfg,
            &group_effects,
            primary_zones,
            tags,
            notes,
        );
        emitted_indices.push(builder.push_group(ir_group));
    }

    for (cfg, zone) in split_groups {
        let ir_group = build_ir_group(&group_name, &cfg, &group_effects, vec![zone], tags, notes);
        emitted_indices.push(builder.push_group(ir_group));
    }

    emitted_indices
}

fn build_ir_group(
    name: &str,
    cfg: &Inherited,
    effects: &[ir::Effect],
    zones: Vec<ir::Zone>,
    tags: &mut TagRegistry,
    notes: &mut Notes,
) -> ir::Group {
    let mut gb = GroupBuilder::new(name)
        .gain_db(cfg.volume_db)
        .pan(cfg.pan)
        .tune(cfg.tuning)
        .keytrack(cfg.pitch_key_track)
        .adsr(cfg.attack, cfg.decay, cfg.sustain, cfg.release)
        .with_velocity_tracking(cfg.amp_vel_track)
        .trigger(cfg.trigger);

    for effect in effects.iter().cloned() {
        gb = gb.add_insert(effect);
    }

    // Round-robin / random sequence start rule.
    let effective_seq_mode = cfg
        .seq_mode
        .or_else(|| cfg.seq_position.map(|_| SeqMode::RoundRobin));
    if let Some(mode) = effective_seq_mode
        && mode != SeqMode::Always
    {
        let length = cfg.seq_length.unwrap_or(1);
        if length > 1 {
            let pos = cfg.seq_position.unwrap_or(1).saturating_sub(1);
            gb = gb.add_start_rule(
                ir::Join::And,
                ir::StartRule::RoundRobin {
                    position: pos,
                    length,
                    random: mode == SeqMode::Random,
                },
            );
        }
    }

    // Voice-group tags and choke rules.
    if let Some(first_tag) = cfg.tags.first() {
        let vg_idx = tags.intern(first_tag);
        tags.apply_silencing_mode(vg_idx, cfg.silencing_mode);
        gb = gb.voice_group(Some(vg_idx));
        if cfg.tags.len() > 1 {
            notes.add(ir::Impact::Audible, "multiple tags", cfg.tags.join(","));
        }
    }
    if let Some(first_choke) = cfg.silenced_by_tags.first() {
        let choke_idx = tags.intern(first_choke);
        gb = gb.off_by(Some(choke_idx));
        if cfg.silenced_by_tags.len() > 1 {
            notes.add(
                ir::Impact::Audible,
                "multiple silencedByTags",
                cfg.silenced_by_tags.join(","),
            );
        }
    }

    for zone in zones {
        gb = gb.add_zone(zone);
    }

    gb.build()
}

/// Applies `<groups>` or `<group>` attributes onto `cfg`.
fn apply_inherited(cfg: &mut Inherited, node: Node<'_, '_>, notes: &mut Notes) {
    if let Some(raw) = node.attribute("volume") {
        if let Some(db) = parse_volume_db(raw) {
            cfg.volume_db += db;
        } else {
            notes.add(ir::Impact::Audible, "invalid volume", raw);
        }
    }
    if let Some(pan) = attr_f32(node, "pan") {
        cfg.pan = (pan / 100.0).clamp(-1.0, 1.0);
    }
    let group_tuning = if node.has_tag_name("group") {
        attr_f64(node, "groupTuning").or_else(|| attr_f64(node, "tuning"))
    } else {
        attr_f64(node, "tuning")
    };
    if let Some(tuning) = group_tuning {
        cfg.tuning += tuning;
    }
    apply_sample_group_overrides(cfg, node, notes);
}

/// Applies attributes that live on [`ir::Group`] (valid on `<groups>`, `<group>`, or `<sample>`).
/// Returns `true` if any such attribute was present on `node`.
fn apply_sample_group_overrides(
    cfg: &mut Inherited,
    node: Node<'_, '_>,
    notes: &mut Notes,
) -> bool {
    let mut touched = false;
    if let Some(v) = attr_f32(node, "attack") {
        cfg.attack = v.max(0.0);
        touched = true;
    }
    if let Some(v) = attr_f32(node, "decay") {
        cfg.decay = v.max(0.0);
        touched = true;
    }
    if let Some(v) = attr_f32(node, "sustain") {
        cfg.sustain = v.clamp(0.0, 1.0);
        touched = true;
    }
    if let Some(v) = attr_f32(node, "release") {
        cfg.release = v.max(0.0);
        touched = true;
    }
    if let Some(v) = attr_f32(node, "ampVelTrack") {
        cfg.amp_vel_track = v.clamp(-1.0, 1.0);
        touched = true;
    }
    if let Some(v) = attr_f64(node, "pitchKeyTrack") {
        cfg.pitch_key_track = v;
        touched = true;
    }
    if let Some(raw) = node.attribute("seqMode") {
        touched = true;
        match raw.trim().to_ascii_lowercase().as_str() {
            "always" => cfg.seq_mode = Some(SeqMode::Always),
            "round_robin" => cfg.seq_mode = Some(SeqMode::RoundRobin),
            "random" | "true_random" => cfg.seq_mode = Some(SeqMode::Random),
            other => notes.add(ir::Impact::Audible, "seqMode", other),
        }
    }
    if let Some(v) = attr_u8(node, "seqLength") {
        cfg.seq_length = Some(v);
        touched = true;
    }
    if let Some(v) = attr_u8(node, "seqPosition") {
        cfg.seq_position = Some(v);
        touched = true;
    }
    if let Some(raw) = node.attribute("trigger") {
        touched = true;
        match raw.trim().to_ascii_lowercase().as_str() {
            "attack" => cfg.trigger = ir::Trigger::Attack,
            "release" => cfg.trigger = ir::Trigger::Release,
            "first" => cfg.trigger = ir::Trigger::First,
            "legato" => cfg.trigger = ir::Trigger::Legato,
            other => notes.add(ir::Impact::Audible, "trigger", other),
        }
    }
    if let Some(raw) = node.attribute("silencingMode") {
        touched = true;
        match raw.trim().to_ascii_lowercase().as_str() {
            "normal" => cfg.silencing_mode = SilencingMode::Normal,
            "fast" => cfg.silencing_mode = SilencingMode::Fast,
            other => notes.add(ir::Impact::Audible, "silencingMode", other),
        }
    }
    if let Some(raw) = node.attribute("tags") {
        touched = true;
        cfg.tags = split_tags(raw);
    }
    if let Some(raw) = node.attribute("silencedByTags") {
        touched = true;
        cfg.silenced_by_tags = split_tags(raw);
    }
    touched
}

/// Parses DecentSampler volume: `"0dB"`, `"-6 dB"`, or linear gain (`"0.5"` -> -6.02 dB).
pub(crate) fn parse_volume_db(s: &str) -> Option<f32> {
    let trimmed = s.trim();
    if let Some(prefix) = trimmed
        .strip_suffix("dB")
        .or_else(|| trimmed.strip_suffix("db"))
        .or_else(|| trimmed.strip_suffix("DB"))
        .or_else(|| trimmed.strip_suffix("Db"))
    {
        return prefix.trim().parse::<f32>().ok().filter(|v| v.is_finite());
    }
    let val = trimmed.parse::<f32>().ok().filter(|v| v.is_finite())?;
    if val < 0.0 {
        Some(val)
    } else if val == 0.0 {
        Some(-144.0)
    } else {
        Some(gain_to_db(val))
    }
}

fn split_tags(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn attr_f64(node: Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn attr_u8(node: Node<'_, '_>, name: &str) -> Option<u8> {
    node.attribute(name)?.trim().parse::<u8>().ok()
}

fn attr_u64(node: Node<'_, '_>, name: &str) -> Option<u64> {
    node.attribute(name)?.trim().parse::<u64>().ok()
}
