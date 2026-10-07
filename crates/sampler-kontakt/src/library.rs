//! Real Kontakt instruments (NKI, plain or encrypted) to the semantic IR,
//! using the vendored `ni-file` decoders the v1 importer is built on. The
//! translator states only what it decodes; everything else it finds is listed
//! in [`ir::Instrument::unsupported`] with its source location.

use crate::{LoadError, Samples};
use ni_file::kontakt::{
    StructuredObject,
    objects::{
        BParScript, ExternalModArray32, FNTableImpl, FileNameListPreK51, Group, GroupList,
        InternalModArray16, LoopArray, ModSource, Modulator, Program,
    },
};
use sampler_ir as ir;
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

const PROGRAM: u16 = 0x28;
const GROUP_LIST: u16 = 0x33;
const ZONE_LIST: u16 = 0x34;
const LOOPS: u16 = 0x39;
const INTERNAL_MODS: u16 = 0x3b;
const EXTERNAL_MODS: u16 = 0x3c;
const FILE_TABLE: u16 = 0x4b;
const LEGACY_FILE_TABLE: u16 = 0x3d;
const SCRIPT: u16 = 6;

/// A translated instrument and the resolver of the samples its assets name.
pub struct Kontakt {
    pub instrument: ir::Instrument,
    /// Each asset's resolved location, in asset order.
    pub locations: Vec<PathBuf>,
    pub samples: Samples,
}

/// Translate the NKI at `path`. Zones whose sample is missing are left out
/// and reported; a malformed container or zone table is an error.
pub fn read(path: &Path) -> Result<Kontakt, LoadError> {
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    let chunks = crate::read_chunks(&path).map_err(|e| e.at(crate::Stage::Container))?;
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let program = Program::try_from(
        chunks
            .find_first(PROGRAM)
            .ok_or_else(|| invalid("not a single-instrument preset"))?,
    )
    .map_err(|e| decode("program", e))?;
    let (table, others) = match chunks.find_first(FILE_TABLE) {
        Some(chunk) => {
            let t = FNTableImpl::try_from(chunk).map_err(|e| decode("sample file table", e))?;
            (t.sample_filetable, t.other_filetable)
        }
        None => {
            let chunk = chunks
                .find_first(LEGACY_FILE_TABLE)
                .ok_or_else(|| invalid("missing sample file table"))?;
            let t =
                FileNameListPreK51::try_from(chunk).map_err(|e| decode("legacy file table", e))?;
            (t.sample_filetable, t.other_filetable)
        }
    };
    translate(path, program, table, others).map_err(|e| e.at(crate::Stage::Translate))
}

/// Translate program `index` (0-based, in slot order) of the multi at `path`;
/// the programs of a `.nkm` are the instruments of a rack.
pub fn read_program(path: &Path, index: usize) -> Result<Kontakt, LoadError> {
    use ni_file::kontakt::objects::Bank;
    let path = path.canonicalize().map_err(|e| LoadError::io(path, e))?;
    let chunks = crate::read_chunks(&path).map_err(|e| e.at(crate::Stage::Container))?;
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let bank = Bank::try_from(
        chunks
            .find_first(3)
            .ok_or_else(|| invalid("missing multi bank"))?,
    )
    .map_err(|e| decode("multi bank", e))?;
    let mut slots: Vec<_> = bank
        .slot_list()
        .map_err(|e| decode("multi slots", e))?
        .slots
        .into_iter()
        .collect();
    slots.sort_by_key(|(slot, _)| *slot);
    let mut programs = Vec::new();
    for (_, container) in slots {
        programs.extend(
            container
                .program_list()
                .map_err(|e| decode("multi programs", e))?
                .programs,
        );
    }
    let program = programs
        .into_iter()
        .nth(index)
        .ok_or_else(|| invalid("the multi has no such program"))?;
    let (table, others) = match chunks
        .filename_tables()
        .map_err(|e| decode("multi file table", e))?
    {
        Some(t) => (t.sample_filetable, t.other_filetable),
        None => (
            chunks
                .filename_table()
                .ok_or_else(|| invalid("missing multi sample table"))?
                .map_err(|e| decode("multi file table", e))?,
            Default::default(),
        ),
    };
    translate(path, program, table, others).map_err(|e| e.at(crate::Stage::Translate))
}

fn translate(
    path: PathBuf,
    program: Program,
    table: HashMap<u32, String>,
    others: HashMap<u32, String>,
) -> Result<Kontakt, LoadError> {
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error).at(crate::Stage::Parse);
    let params = program
        .params()
        .map_err(|e| decode("program parameters", e))?;
    let mut out = Translation {
        ir: ir::Instrument {
            name: params.name.clone(),
            source: ir::SourceFormat::Kontakt {
                version: program.version(),
            },
            ..Default::default()
        },
        assets: HashMap::new(),
        locations: Vec::new(),
        start_criteria: Vec::new(),
        voice_groups: Vec::new(),
    };
    if let Some(chunk) = program.0.find_first(VOICE_GROUPS) {
        out.voice_groups(&chunk.data)
            .map_err(|e| decode("voice groups", e))?;
    }
    let groups = GroupList::try_from(
        program
            .0
            .find_first(GROUP_LIST)
            .ok_or_else(|| invalid("missing group list"))?,
    )
    .map_err(|e| decode("group list", e))?;
    let mut translated = Vec::new();
    for (index, group) in groups.groups.iter().enumerate() {
        translated.push(out.group(index, group).map_err(|e| decode("group", e))?);
    }
    for (slot, chunk) in program
        .0
        .children
        .iter()
        .filter(|c| c.id == SCRIPT)
        .enumerate()
    {
        let script = BParScript::try_from(chunk)
            .and_then(|s| s.params())
            .map_err(|e| decode("script", e))?;
        let location = format!("script slot {slot}");
        match script.text {
            _ if script.bypass => {}
            Some(text) if !text.trim().is_empty() => {
                let state = saved(&script.persistent);
                if state.len() < script.persistent.len() {
                    out.unsupported(
                        &location,
                        "saved persistent text arrays",
                        script.persistent.len() - state.len(),
                        ir::Reason::NotModeled,
                    );
                }
                out.ir.behaviors.push(ir::Behavior {
                    name: script
                        .description
                        .filter(|d| !d.is_empty())
                        .unwrap_or(location),
                    language: ir::Language::Ksp,
                    source: text,
                    slot: Some(slot.min(usize::from(u8::MAX)) as u8),
                    state,
                    requires: Vec::new(),
                });
            }
            // Neither text nor a linked file: an empty slot.
            _ => {
                if let Some(name) = script.textfile_name.filter(|name| !name.is_empty()) {
                    out.unsupported(
                        &location,
                        "linked script file",
                        name,
                        ir::Reason::NotModeled,
                    );
                }
            }
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("instrument has no folder"))?;
    let root = path
        .ancestors()
        .find(|p| p.join("Samples").is_dir())
        .unwrap_or(parent);
    let mut samples = Samples::new(root);
    let racks = crate::effects::program_racks(&program);
    {
        // Convolution impulse responses are named by the other-files table.
        let mut load = |index: i32| -> Result<crate::effects::Decoded, String> {
            let name = u32::try_from(index)
                .ok()
                .and_then(|i| others.get(&i))
                .ok_or_else(|| format!("index {index} is not in the file table"))?;
            let at = samples
                .resolve(parent, name)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("{name} was not found"))?;
            let decoded = samples.decode(&at).map_err(|e| e.to_string())?;
            Ok((decoded.rate, decoded.frames))
        };
        for (at, (slot, feature, value, reason)) in
            crate::effects::instrument_buses(&mut out.ir, &racks, &mut load)
        {
            out.unsupported(&format!("{at} slot {slot}"), &feature, value, reason);
        }
    }
    // Bus racks are not modelled yet.
    for (at, slots) in racks.into_iter().filter(|(at, _)| at.starts_with("bus")) {
        out.effects(&at, slots);
    }
    let mut resolved = HashMap::new();
    let data = &program
        .0
        .find_first(ZONE_LIST)
        .ok_or_else(|| invalid("missing zone list"))?
        .data;
    let mut r = Cursor::new(data.as_slice());
    let count = u32le(&mut r).map_err(|e| LoadError::io(&path, e))? as usize;
    if count > data.len() / 8 {
        return Err(invalid("zone count exceeds the zone list"));
    }
    for index in 0..count {
        let zone =
            raw_zone(&mut r).map_err(|reason| invalid(&format!("zone {index}: {reason}")))?;
        let Some(group) = translated.get(zone.group).ok_or_else(|| {
            invalid(&format!(
                "zone {index} refers to missing group {}",
                zone.group
            ))
        })?
        else {
            continue; // Muted group: never sounds.
        };
        let name = table.get(&(zone.file as u32)).ok_or_else(|| {
            invalid(&format!(
                "zone {index} sample is absent from the file table"
            ))
        })?;
        let location = match resolved.get(name) {
            Some(location) => location,
            None => resolved
                .entry(name.clone())
                .or_insert(samples.resolve(parent, name)?),
        };
        let Some(location) = location else {
            out.unsupported(
                &format!("zone {index}"),
                "missing sample",
                name,
                ir::Reason::InvalidValue,
            );
            continue;
        };
        // Kontakt stores the end relative to the end of the sample.
        let end = match zone.end {
            0 => None,
            end => Some(
                samples
                    .frames(location)?
                    .saturating_sub(u64::from(end.unsigned_abs())),
            ),
        };
        out.zone(index, zone, end, group, &params, location.clone());
    }
    crate::keyswitch::translate(&mut out.ir, &out.start_criteria);
    out.ir.unsupported.dedup();
    out.ir.validate().map_err(|e| invalid(&e.to_string()))?;
    Ok(Kontakt {
        instrument: out.ir,
        locations: out.locations,
        samples,
    })
}

/// Group settings every zone of the group inherits.
struct GroupInfo {
    index: usize,
    group: ir::GroupRef,
    release: bool,
    tracking: bool,
    reverse: bool,
    envelope: Option<ir::ModulatorRef>,
    velocity: ir::VelocityResponse,
    /// Modulation routes every zone of the group carries.
    routes: Vec<ir::RouteRef>,
    /// Its insert rack as a voice chain.
    chain: Option<ir::ChainRef>,
}

struct Translation {
    ir: ir::Instrument,
    assets: HashMap<PathBuf, ir::AssetRef>,
    locations: Vec<PathBuf>,
    start_criteria: Vec<(
        String,
        ir::GroupRef,
        Vec<ni_file::kontakt::objects::StartCriteriaParams>,
    )>,
    /// Kontakt voice group index -> `ir.voice_limits` index.
    voice_groups: Vec<Option<usize>>,
}

const VOICE_GROUPS: u16 = 0x32;

/// One `BVoiceLimit` (version 0x60): name, kill mode (Any, Oldest, Newest,
/// Highest, Lowest), prefer released, max voices, fade ms, exclusion group.
fn voice_limit(data: &mut &[u8]) -> Result<(ir::VoiceLimit, i32), ni_file::Error> {
    fn take<const N: usize>(data: &mut &[u8]) -> Result<[u8; N], ni_file::Error> {
        let (head, rest) = data
            .split_first_chunk::<N>()
            .ok_or_else(|| ni_file::Error::Generic("truncated voice limit".into()))?;
        *data = rest;
        Ok(*head)
    }
    let header = take::<3>(data)?;
    if header != [0, 0x60, 0] {
        return Err(ni_file::Error::Generic(format!(
            "voice limit header {header:02x?}"
        )));
    }
    let chars = u32::from_le_bytes(take(data)?) as usize;
    if data.len() < chars * 2 {
        return Err(ni_file::Error::Generic("truncated voice limit name".into()));
    }
    *data = &data[chars * 2..];
    let kill = match i16::from_le_bytes(take(data)?) {
        0 => ir::Kill::Any,
        1 => ir::Kill::Oldest,
        2 => ir::Kill::Newest,
        3 => ir::Kill::Highest,
        4 => ir::Kill::Lowest,
        other => return Err(ni_file::Error::Generic(format!("voice kill mode {other}"))),
    };
    let prefer_released = take::<1>(data)?[0] != 0;
    let voices = i32::from_le_bytes(take(data)?).max(1) as u32;
    let fade = i32::from_le_bytes(take(data)?).max(0);
    let exclusion = i32::from_le_bytes(take(data)?);
    Ok((
        ir::VoiceLimit {
            voices,
            kill,
            prefer_released,
            fade: ir::Time::Milliseconds(f64::from(fade)),
        },
        exclusion,
    ))
}

/// Kontakt AHDSR stage laws as native `expm1(k·t)/expm1(k)` curves (decoded
/// from Kontakt's engine in v1, `src/engine/ahdsr.rs`, control rate rate/32):
/// decay and release fall geometrically to 3/43 of `1.075` above a `0.075`
/// floor, which is exactly k = ln(3/43); the attack runs a geometric segment
/// with base b = e^((1 − |c|)·ln 500000 − ln 20000) for authored curve c in
/// −1..1: k = ln(b / (1 + b)) for c > 0 (fast start), ln((1 + b) / b) else.
fn ahdsr_curves(curve: f32) -> (ir::Curve, ir::Curve) {
    let c = f64::from(curve.clamp(-1.0, 1.0));
    // The engine rounds the base to f32 before its power.
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let attack = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    (
        ir::Curve::Exponential(attack),
        ir::Curve::Exponential((3.0f64 / 43.0).ln()),
    )
}

impl Translation {
    /// The `VoiceGroups` chunk: the instrument's voice limit, a 128-bit set of
    /// defined voice groups, then one voice limit per defined group.
    fn voice_groups(&mut self, mut data: &[u8]) -> Result<(), ni_file::Error> {
        let (instrument, _) = voice_limit(&mut data)?;
        self.ir.voice_limit = Some(instrument);
        let (defined, mut data) = data
            .split_first_chunk::<16>()
            .ok_or_else(|| ni_file::Error::Generic("truncated voice groups".into()))?;
        self.voice_groups = vec![None; 128];
        for g in 0..128 {
            if defined[g / 8] & (1 << (g % 8)) != 0 {
                let (limit, exclusion) = voice_limit(&mut data)?;
                if exclusion >= 0 {
                    self.unsupported(
                        &format!("voice group {g}"),
                        "voice group exclusion group",
                        exclusion,
                        ir::Reason::NotModeled,
                    );
                }
                self.ir.voice_limits.push(limit);
                self.voice_groups[g] = Some(self.ir.voice_limits.len() - 1);
            }
        }
        if !data.is_empty() {
            return Err(ni_file::Error::Generic(format!(
                "{} bytes after the voice groups",
                data.len()
            )));
        }
        Ok(())
    }
    fn unsupported(
        &mut self,
        location: &str,
        feature: &str,
        value: impl std::fmt::Display,
        reason: ir::Reason,
    ) {
        self.ir.unsupported.push(ir::Unsupported {
            location: location.into(),
            feature: feature.into(),
            value: value.to_string(),
            reason,
        });
    }

    /// Report a rack's active effects (none are modelled yet).
    fn effects(&mut self, at: &str, slots: Vec<crate::effects::Slot>) {
        for fx in slots.iter().filter(|fx| !fx.bypass) {
            self.unsupported(
                &format!("{at} slot {}", fx.slot),
                "effect",
                format!(
                    "{} v{:#x} {:?} wet {} dry {}",
                    crate::effects::module_name(fx.module),
                    fx.version,
                    fx.params(),
                    fx.output_gain,
                    fx.dry_level
                ),
                ir::Reason::NotModeled,
            );
        }
    }

    /// A group's settings, or `None` for a muted group.
    fn group(&mut self, index: usize, group: &Group) -> Result<Option<GroupInfo>, ni_file::Error> {
        let v = group.params()?;
        let at = format!("group {index} {:?}", v.name);
        if v.muted {
            return Ok(None);
        }
        let not_modeled = ir::Reason::NotModeled;
        // 1-based (0 = none): Una Corda's groups use 1 and 2 with voice
        // groups 0 and 1 defined, Afflatus 2 Horns KS 1..=8 with 0..=7.
        let voice_group = usize::try_from(v.voice_group_index - 1).ok();
        let voice_limit = voice_group.and_then(|g| {
            let limit = self.voice_groups.get(g).copied().flatten();
            if limit.is_none() {
                self.unsupported(&at, "undefined voice group", g, ir::Reason::InvalidValue);
            }
            limit
        });
        if v.midi_channel >= 0 {
            self.unsupported(&at, "MIDI channel filter", v.midi_channel, not_modeled);
        }
        // Start options become articulations or reports in `keyswitch::translate`.
        self.start_criteria.push((
            at.clone(),
            ir::GroupRef(self.ir.groups.len()),
            v.start_criteria.items.clone(),
        ));
        match group.source_identity() {
            // v1 plays every mode but wavetable (9) as a sampler; so does this.
            Ok(source) if source.mode == 9 => {
                self.unsupported(&at, "wavetable source", source.mode, not_modeled)
            }
            Ok(source) if source.mode != 0 => self.unsupported(
                &at,
                "source mode (played as a sampler)",
                source.mode,
                not_modeled,
            ),
            Ok(_) => {}
            Err(error) => self.unsupported(&at, "source module", error, ir::Reason::Unknown),
        }
        let mut chain = None;
        let mut filter_slots = Vec::new();
        if let Ok(array) = group.insert_fx() {
            let c =
                crate::effects::chain(&crate::effects::rack(&array), crate::effects::Scope::Voice);
            let processors = c.processors;
            filter_slots = c.filter_slots;
            for (slot, feature, value, reason) in c.notes {
                self.unsupported(&format!("{at} insert slot {slot}"), &feature, value, reason);
            }
            if !processors.is_empty() {
                self.ir.chains.push(ir::Chain {
                    scope: ir::Scope::Voice,
                    pre_amplitude: processors,
                    post_amplitude: Vec::new(),
                });
                chain = Some(ir::ChainRef(self.ir.chains.len() - 1));
            }
        }
        let mut envelope = None;
        let mut flex_release = None;
        let mut routes = Vec::new();
        if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
            for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
                let params = modulator.params()?;
                let at = format!("{at} modulator slot {slot}");
                // [router UI, bypass, retrigger, unknown]
                if params.unknown_flags[1] != 0 || params.targets.is_empty() {
                    continue;
                }
                let retrigger = params.unknown_flags[2] != 0;
                let volume = matches!(params.targets.as_slice(), [t]
                    if t.param == "volume" && t.intensity == 1.0 && !t.invert
                        && t.slot.is_none() && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled));
                let source = match params.modulator {
                    Modulator::Ahdsr(env) => {
                        let ms = |ms: f32| ir::Time::Milliseconds(f64::from(ms.max(0.0)));
                        let (attack_shape, fall) = ahdsr_curves(env.attack_curve);
                        ir::ModulationSource::Envelope(ir::Envelope {
                            attack: ms(env.attack_ms),
                            hold: ms(env.hold_ms),
                            decay: ms(env.decay_ms),
                            sustain: f64::from(env.sustain.clamp(0.0, 1.0)),
                            release: ms(env.release_ms),
                            attack_shape,
                            decay_shape: fall,
                            release_shape: fall,
                            // The AHD-only switch (v1: `ahd_only = flag != 0`).
                            one_shot: env.unknown_flag != 0,
                            ..Default::default()
                        })
                    }
                    Modulator::Lfo(lfo) => match self.lfo(&at, &lfo, retrigger) {
                        Some(lfo) => ir::ModulationSource::Lfo(lfo),
                        None => continue,
                    },
                    Modulator::Flex(flex) => {
                        // Point times are deltas from the previous point and
                        // levels linear gain (audits/MODULATION.md, medium).
                        // Segment curve s (0..1, 0.5 linear), c = s - 0.5:
                        // measured on Kontakt 8 flex envelopes, k = BOW_K * |c|
                        // with positive c starting fast and negative slowly
                        // (docs/architecture-v2/KONTAKT_REFERENCE.md s.10).
                        let sustain = flex.sustain as usize;
                        if volume {
                            // Held until the release segments end.
                            let after: f32 = flex
                                .points
                                .iter()
                                .skip(sustain + 1)
                                .map(|p| p.time_ms.max(0.0))
                                .sum();
                            flex_release = Some(flex_release.unwrap_or(0.0f32).max(after));
                        }
                        ir::ModulationSource::Breakpoints(ir::Breakpoints {
                            points: flex
                                .points
                                .iter()
                                .map(|p| ir::Breakpoint {
                                    time: ir::Time::Milliseconds(f64::from(p.time_ms.max(0.0))),
                                    level: f64::from(p.level.clamp(0.0, 1.0)),
                                    shape: match f64::from(p.curve) - 0.5 {
                                        c if c.abs() < 1e-4 => ir::Curve::Linear,
                                        c => ir::Curve::Exponential(-BOW_K * c),
                                    },
                                })
                                .collect(),
                            sustain: Some(sustain),
                        })
                    }
                    Modulator::Other { chunk_id } => {
                        self.unsupported(
                            &at,
                            "internal modulator chunk",
                            format!("{chunk_id:#x} {:?}", params.name),
                            ir::Reason::Unknown,
                        );
                        continue;
                    }
                };
                let envelope_source = matches!(source, ir::ModulationSource::Envelope(_));
                self.ir.modulators.push(ir::Modulator {
                    scope: ir::Scope::Voice,
                    source,
                });
                let modulator = ir::ModulatorRef(self.ir.modulators.len() - 1);
                if envelope_source && volume && envelope.is_none() {
                    envelope = Some(modulator);
                    continue;
                }
                for target in &params.targets {
                    routes.extend(self.route(
                        &at,
                        modulator,
                        envelope_source,
                        target,
                        chain.zip(Some(&filter_slots[..])),
                    ));
                }
            }
        }
        if let (None, Some(release_ms)) = (envelope, flex_release) {
            // A flex volume envelope without an AHDSR plays against a gate
            // that holds through the flex release, then ends the voice.
            self.ir.modulators.push(ir::Modulator {
                scope: ir::Scope::Voice,
                source: ir::ModulationSource::Envelope(ir::Envelope {
                    release: ir::Time::Milliseconds(f64::from(release_ms)),
                    release_shape: ir::Curve::Step,
                    ..Default::default()
                }),
            });
            envelope = Some(ir::ModulatorRef(self.ir.modulators.len() - 1));
        }
        let mut velocity = ir::VelocityResponse::None;
        if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
            for (slot, modulation) in ExternalModArray32::try_from(chunk)?.slots()? {
                let params = modulation.params()?;
                let at = format!("{at} external modulation slot {slot}");
                let plain_volume = |t: &ni_file::kontakt::objects::ModTarget| {
                    t.param == "volume"
                        && t.slot.is_none()
                        && !t.invert
                        && t.lag_ms == 0
                        && !t.shaper.as_ref().is_some_and(|s| s.enabled)
                };
                if let (ModSource::Velocity, [t]) = (&params.source, params.targets.as_slice())
                    && plain_volume(t)
                    && t.intensity == 1.0
                    && velocity == ir::VelocityResponse::None
                {
                    // gain × velocity: the attenuate law at full intensity,
                    // kept on the voice so no per-voice modulation is needed.
                    velocity = ir::VelocityResponse::Linear;
                    continue;
                }
                let source = match params.source {
                    ModSource::Velocity => ir::ModulationSource::Velocity,
                    ModSource::KeyPosition => ir::ModulationSource::Key,
                    ModSource::MidiCc(cc) if cc < 128 => ir::ModulationSource::Controller(cc),
                    ModSource::PitchBend => ir::ModulationSource::PitchBend,
                    ModSource::MonoAftertouch => ir::ModulationSource::ChannelPressure,
                    ModSource::PolyAftertouch => ir::ModulationSource::PolyPressure,
                    ModSource::Constant => ir::ModulationSource::Constant,
                    // Kontakt manual (Source module, T): counts down from T ms
                    // at note-on and holds its value at note-off.
                    ModSource::ReleaseTriggerCounter if v.rls_trig_counter > 0 => {
                        ir::ModulationSource::ReleaseCounter(ir::Time::Milliseconds(f64::from(
                            v.rls_trig_counter,
                        )))
                    }
                    // KSP set_event_par_arr($EVENT_PAR_MOD_VALUE_ID, v, id).
                    ModSource::Script(id) if id <= 1000 => ir::ModulationSource::Script(id as u16),
                    ModSource::RandomUnipolar => ir::ModulationSource::Random,
                    ModSource::Unassigned => continue,
                    other => {
                        let reason = match other {
                            ModSource::RandomBipolar => ir::Reason::UnknownLaw,
                            _ => not_modeled,
                        };
                        let targets: Vec<_> =
                            params.targets.iter().map(|t| t.param.as_str()).collect();
                        self.unsupported(
                            &at,
                            &format!("external modulation from {other:?}"),
                            format!("{:?} -> {targets:?}", params.name),
                            reason,
                        );
                        continue;
                    }
                };
                // Bend is bipolar: shapers read (bend + 1) / 2 (Pacific's PB
                // shapers sit at 0.81 at rest and 1 at full bend). Bend to
                // pitch stays the note's native expression bend in lowering.
                let bipolar = source.bipolar();
                self.ir.modulators.push(ir::Modulator {
                    scope: ir::Scope::Voice,
                    source,
                });
                let modulator = ir::ModulatorRef(self.ir.modulators.len() - 1);
                for target in &params.targets {
                    routes.extend(self.route(
                        &at,
                        modulator,
                        !bipolar,
                        target,
                        chain.zip(Some(&filter_slots[..])),
                    ));
                }
            }
        }
        if !v.volume.is_finite() || !v.pan.is_finite() || !(v.tune.is_finite() && v.tune > 0.0) {
            return Err(ni_file::Error::Generic(format!(
                "{at}: invalid gain, pan or tune"
            )));
        }
        self.ir.groups.push(ir::Group {
            name: v.name,
            gain: ir::Gain::Linear(f64::from(v.volume)),
            pan: ir::Pan {
                position: f64::from(v.pan.clamp(-1.0, 1.0)),
                law: ir::PanLaw::Balance,
            },
            tune: ir::Pitch::Ratio(f64::from(v.tune)),
            voice_limit,
            monophonic_release: v.release_trigger && v.release_trigger_note_monophonic,
            ..Default::default()
        });
        Ok(Some(GroupInfo {
            index,
            group: ir::GroupRef(self.ir.groups.len() - 1),
            release: v.release_trigger,
            tracking: v.key_tracking,
            reverse: v.reverse,
            envelope,
            velocity,
            routes,
            chain,
        }))
    }

    /// One Kontakt modulation target as an IR route, or a report entry.
    /// Laws (decoded from Kontakt's engine, control rate = rate / 32):
    /// volume factor 1 − i(1 − u); pitch 12·i semitones; playPos start + i·u
    /// of the zone's start-modulation range; lag reaches 99% in `lag_ms`.
    fn route(
        &mut self,
        at: &str,
        source: ir::ModulatorRef,
        unipolar: bool,
        target: &ni_file::kontakt::objects::ModTarget,
        filters: Option<(ir::ChainRef, &[(usize, usize)])>,
    ) -> Option<ir::RouteRef> {
        let i = f64::from(target.intensity);
        let report = |this: &mut Self, feature: &str, reason| {
            this.unsupported(
                at,
                feature,
                format!("{} ({}) intensity {i}", target.param, target.name),
                reason,
            );
            None
        };
        // Filter cutoff of an insert slot: octaves, linear in the modulator,
        // 10 octaves per 100 % (KONTAKT_REFERENCE.md section 17).
        let cutoff = match (target.slot, target.param.as_str()) {
            (None, _) => None,
            (Some(slot), "filterCutoff") => filters.and_then(|(chain, slots)| {
                slots
                    .iter()
                    .find(|(s, _)| *s == usize::from(slot))
                    .map(|&(_, index)| ir::Target::Processor {
                        chain,
                        index,
                        parameter: ir::ProcessorParameter::Cutoff,
                    })
            }),
            _ => None,
        };
        if target.slot.is_some() && cutoff.is_none() {
            return report(
                self,
                "modulation of a module parameter",
                ir::Reason::NotModeled,
            );
        }
        // Flag 0x02 marks a signed (bipolar) target scaling; how a unipolar
        // source maps onto it is not established.
        if unipolar && target.unknown_flags & 0x02 != 0 {
            return report(self, "signed modulation target", ir::Reason::UnknownLaw);
        }
        let (route_target, depth) = match target.param.as_str() {
            _ if cutoff.is_some() => (
                cutoff.unwrap_or(ir::Target::Amplitude),
                ir::Depth::Pitch(ir::Pitch::Semitones(120.0 * i)),
            ),
            "volume" => (ir::Target::Amplitude, ir::Depth::Normalized(i)),
            "pitch" => (
                ir::Target::Pitch,
                ir::Depth::Pitch(ir::Pitch::Semitones(12.0 * i)),
            ),
            "playPos" => (ir::Target::SampleStart, ir::Depth::Normalized(i)),
            "pan" => return report(self, "pan modulation", ir::Reason::UnknownLaw),
            _ => return report(self, "modulation target", ir::Reason::NotModeled),
        };
        // The invert flag does not act through an enabled shaper: Vista Full
        // Strings stores identical shaped crossfade copies (mic `cl`/`dc`,
        // `BALANCE_COMP`) that differ only in the flag and must play alike,
        // and no inversion order makes both play the same.
        let shaper = target.shaper.as_ref().filter(|s| s.enabled);
        let invert = target.invert && shaper.is_none();
        let shape = match shaper {
            None => None,
            Some(shaper) => {
                use ni_file::kontakt::objects::ShaperCurve;
                let points: Vec<(f64, f64)> = match &shaper.curve {
                    ShaperCurve::Table(table) if table.len() > 1 => {
                        let last = (table.len() - 1) as f64;
                        table
                            .iter()
                            .enumerate()
                            .map(|(n, y)| (n as f64 / last, f64::from(*y)))
                            .collect()
                    }
                    ShaperCurve::Breakpoints(points) if !points.is_empty() => curved_shaper(points),
                    _ => return report(self, "empty modulation shaper", ir::Reason::UnknownLaw),
                };
                self.ir.shapes.push(ir::Shape { points });
                Some(ir::ShapeRef(self.ir.shapes.len() - 1))
            }
        };
        self.ir.routes.push(ir::Route {
            source,
            target: route_target,
            depth,
            invert,
            shape,
            smoothing: ir::Time::Milliseconds(f64::from(target.lag_ms)),
            scale: None,
        });
        Some(ir::RouteRef(self.ir.routes.len() - 1))
    }

    /// Kontakt LFO: waveform ids sine 0, rectangle 1, triangle 2, sawtooth 3,
    /// random 4, Multi 5. Rate is Hz when the sync note value is −1, else a
    /// cycle of note value × count beats; initial values are fade-in ms,
    /// rate/count, pulse width and start phase in cycles.
    fn lfo(
        &mut self,
        at: &str,
        lfo: &ni_file::kontakt::objects::Lfo,
        retrigger: bool,
    ) -> Option<ir::Lfo> {
        let [fade_ms, rate, width, phase] = lfo.initial_values.map(f64::from);
        let shape = match (lfo.waveform, lfo.trailing_values) {
            (0, _) => ir::LfoShape::Sine,
            (1, _) if width == 0.5 => ir::LfoShape::Square,
            (2, _) => ir::LfoShape::Triangle,
            // Multi with exactly one wave and normalization on is that wave.
            (5, Some(weights))
                if lfo.records[0].flag
                    && weights.iter().filter(|w| **w != 0.0).count() == 1
                    && weights[3] == 0.0
                    && weights[4] == 0.0
                    && (weights[1] == 0.0 || width == 0.5)
                    && weights.iter().all(|w| *w >= 0.0) =>
            {
                match weights.iter().position(|w| *w != 0.0) {
                    Some(0) => ir::LfoShape::Sine,
                    Some(1) => ir::LfoShape::Square,
                    _ => ir::LfoShape::Triangle,
                }
            }
            (waveform, weights) => {
                self.unsupported(
                    at,
                    "LFO waveform (id, multi weights, pulse width)",
                    format!("{waveform} {weights:?} {width}"),
                    ir::Reason::UnknownLaw,
                );
                return None;
            }
        };
        let note = f64::from(lfo.records[0].values[0]);
        let rate = if note == -1.0 {
            ir::Frequency::Hertz(rate)
        } else {
            ir::Frequency::Beats(note * rate)
        };
        if !matches!(rate, ir::Frequency::Hertz(r) | ir::Frequency::Beats(r) if r.is_finite() && r > 0.0)
            || !fade_ms.is_finite()
            || !phase.is_finite()
        {
            self.unsupported(
                at,
                "LFO rate",
                format!("{rate:?}"),
                ir::Reason::InvalidValue,
            );
            return None;
        }
        Some(ir::Lfo {
            shape,
            rate,
            delay: ir::Time::ZERO,
            fade_in: ir::Time::Milliseconds(fade_ms.max(0.0)),
            phase: phase.rem_euclid(1.0),
            retrigger,
        })
    }

    fn zone(
        &mut self,
        index: usize,
        z: RawZone,
        end: Option<u64>,
        group: &GroupInfo,
        program: &ni_file::kontakt::objects::ProgramPublicParams,
        location: PathBuf,
    ) {
        let at = format!("zone {index} (group {})", group.index);
        let not_modeled = ir::Reason::NotModeled;
        let asset = match self.assets.get(&location) {
            Some(&asset) => asset,
            None => {
                let encoding = match location
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .as_deref()
                {
                    Some("ncw") => ir::Encoding::Ncw,
                    Some("wav") => ir::Encoding::Wav,
                    Some("aif" | "aiff") => ir::Encoding::Aiff,
                    _ => ir::Encoding::Unknown,
                };
                self.ir.assets.push(ir::Asset {
                    location: ir::AssetLocation::Path(location.to_string_lossy().into_owned()),
                    encoding,
                    root_key: None,
                    loops: Vec::new(),
                });
                self.locations.push(location.clone());
                let asset = ir::AssetRef(self.ir.assets.len() - 1);
                self.assets.insert(location, asset);
                asset
            }
        };
        if end.is_some_and(|end| end <= z.start) {
            self.unsupported(
                &at,
                "empty sample range (start, end)",
                format!("{}, {end:?}", z.start),
                ir::Reason::InvalidValue,
            );
            return;
        }
        let mut looping = ir::Looping::None;
        for (slot, l) in z.loops.iter().enumerate().filter(|(_, l)| l.mode != 0) {
            if looping != ir::Looping::None {
                self.unsupported(
                    &at,
                    "additional loop",
                    format!("slot {slot}: {l:?}"),
                    not_modeled,
                );
                continue;
            }
            if l.loop_count != 0
                || (l.loop_tuning - 1.0).abs() > 0.001
                || l.loop_start < 0
                || l.loop_length <= 0
            {
                self.unsupported(
                    &at,
                    "counted, tuned or invalid loop",
                    format!("{l:?}"),
                    not_modeled,
                );
                continue;
            }
            let range = ir::LoopRange {
                start: l.loop_start as u64,
                end: l.loop_start as u64 + l.loop_length as u64,
                crossfade: ir::Span::Frames(l.x_fade_length.max(0) as u64),
                alternating: l.alternating_loop,
            };
            // Mode 1 is the only mode in local libraries; 2 as "until release" is unverified.
            looping = match l.mode {
                1 => ir::Looping::Continuous(range),
                2 => ir::Looping::UntilRelease(range),
                mode => {
                    self.unsupported(&at, "loop mode", mode, ir::Reason::Unknown);
                    continue;
                }
            };
        }
        let semitones =
            12.0 * f64::from(z.tune * program.tune).log2() + f64::from(program.transpose);
        self.ir.zones.push(ir::Zone {
            group: Some(group.group),
            keys: ir::KeyRange {
                low: z.keys[0],
                high: z.keys[1],
            },
            velocities: ir::VelocityRange {
                low: z.velocities[0].max(1),
                high: z.velocities[1].max(1),
            },
            // Kontakt's system pedal script holds the note-off under sustain,
            // so release-trigger groups fire when the note actually releases:
            // at pedal-up for a sustained key (at key-up with
            // NO_SYS_SCRIPT_PEDAL, which sampler-ksp maps to script sustain).
            trigger: if group.release {
                ir::Trigger::GateRelease
            } else {
                ir::Trigger::Attack
            },
            pitch: if group.tracking {
                ir::KeyTracking::Tracked { root: z.root }
            } else {
                ir::KeyTracking::Fixed
            },
            tune: ir::Pitch::Semitones(semitones),
            gain: ir::Gain::Linear(output_gain(z.gain, program.volume)),
            velocity: group.velocity,
            // Mapping Editor crossfades: widths in key and velocity steps
            // inside the zone; the gain law is measured (Kontakt 8,
            // KONTAKT_REFERENCE.md s.11). Field order low velocity, high
            // velocity, low key, high key (v1 importer); the velocity pair is
            // confirmed by ANALOG STRINGS' zone 1..40 fading out (f1 = 21) and
            // Afflatus zones fading in (f0), no library here uses the key pair.
            fades: {
                let step = |w: i16| w.clamp(0, 127) as u8;
                ir::Fades {
                    velocity_in: step(z.fades[0]),
                    velocity_out: step(z.fades[1]),
                    key_in: step(z.fades[2]),
                    key_out: step(z.fades[3]),
                }
            },
            pan: ir::Pan {
                position: f64::from((z.pan + program.pan).clamp(-1.0, 1.0)),
                law: ir::PanLaw::Balance,
            },
            playback: ir::Playback {
                start: z.start,
                end,
                reverse: group.reverse,
                looping,
                start_range: z.start_mod,
            },
            amplitude: group.envelope,
            routes: group.routes.clone(),
            chain: group.chain,
            ..ir::Zone::new(asset)
        });
    }
}

/// One serialized zone: its group, mapping and sample reference.
struct RawZone {
    group: usize,
    start: u64,
    end: i32,
    /// Frames past `start` that full-depth playPos modulation reaches.
    start_mod: u64,
    velocities: [u8; 2],
    keys: [u8; 2],
    fades: [i16; 4],
    root: u8,
    gain: f32,
    pan: f32,
    tune: f32,
    file: i32,
    loops: Vec<ni_file::kontakt::objects::Loop>,
}

/// Layout from the v1 importer: the owning group, then a structured zone whose
/// public data is start, end, start-modulation range, velocity and key ranges,
/// four crossfade widths, root, gain, pan, tune and (v0x9a+) six unknown bytes
/// before the file ID.
fn raw_zone(r: &mut Cursor<&[u8]>) -> Result<RawZone, String> {
    let group = u32le(r).map_err(|e| e.to_string())? as usize;
    let so = StructuredObject::read(&mut *r).map_err(|e| e.to_string())?;
    let mut z = Cursor::new(so.public_data.as_slice());
    let read = |z: &mut Cursor<&[u8]>| -> std::io::Result<_> {
        // The third field is the sample-start modulation range, used only by playPos modulation.
        let (start, end, start_mod) = (i32le(z)?, i32le(z)?, i32le(z)?);
        let mut ranges = [0i16; 9];
        for value in &mut ranges {
            *value = i16le(z)?;
        }
        let (gain, pan, tune) = (f32le(z)?, f32le(z)?, f32le(z)?);
        if so.version >= 0x9a {
            z.read_exact(&mut [0; 6])?;
        }
        Ok((start, end, start_mod, ranges, gain, pan, tune, i32le(z)?))
    };
    let (start, end, start_mod, ranges, gain, pan, tune, file) =
        read(&mut z).map_err(|e| e.to_string())?;
    let [lv, hv, lk, hk, f0, f1, f2, f3, root] = ranges;
    let midi = |value: i16, what| {
        u8::try_from(value)
            .ok()
            .filter(|v| *v < 128)
            .ok_or(format!("{what} {value} is outside 0..=127"))
    };
    let (lk, hk, lv, hv) = (
        midi(lk, "low key")?,
        midi(hk, "high key")?,
        midi(lv, "low velocity")?,
        midi(hv, "high velocity")?,
    );
    if lk > hk || lv > hv {
        return Err(format!(
            "inverted range: keys {lk}..={hk}, velocities {lv}..={hv}"
        ));
    }
    if start < 0
        || end > 0
        || !gain.is_finite()
        || !pan.is_finite()
        || !(tune.is_finite() && tune > 0.0)
    {
        return Err(format!(
            "start {start}, end {end}, gain {gain}, pan {pan}, tune {tune}"
        ));
    }
    let loops = match so.find_first(LOOPS) {
        Some(chunk) => LoopArray::try_from(chunk).map_err(|e| e.to_string())?.items,
        None => Vec::new(),
    };
    Ok(RawZone {
        group,
        start: start as u64,
        end,
        start_mod: start_mod.max(0) as u64,
        velocities: [lv, hv],
        keys: [lk, hk],
        fades: [f0, f1, f2, f3],
        root: midi(root, "root")?,
        gain,
        pan,
        tune,
        file,
        loops,
    })
}

fn u32le(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
fn i32le(r: &mut impl Read) -> std::io::Result<i32> {
    Ok(u32le(r)? as i32)
}
fn i16le(r: &mut impl Read) -> std::io::Result<i16> {
    let mut b = [0; 2];
    r.read_exact(&mut b)?;
    Ok(i16::from_le_bytes(b))
}
fn f32le(r: &mut impl Read) -> std::io::Result<f32> {
    Ok(f32::from_bits(u32le(r)?))
}

#[cfg(test)]
mod survey {
    use super::*;
    #[test]
    #[ignore]
    fn survey_modulation() {
        let root = std::env::var("KONTRA_KONTAKT_LIBRARIES").unwrap();
        let mut stack = vec![PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) {
                    files.push(p)
                }
            }
        }
        files.sort();
        let mut seen = std::collections::BTreeMap::<String, (usize, String)>::new();
        for f in &files {
            let Ok(chunks) = crate::read_chunks(f) else {
                continue;
            };
            let Some(program) = chunks.find_first(PROGRAM) else {
                continue;
            };
            let Ok(program) = Program::try_from(program) else {
                continue;
            };
            let Some(gl) = program.0.find_first(GROUP_LIST) else {
                continue;
            };
            let Ok(groups) = GroupList::try_from(gl) else {
                continue;
            };
            for g in &groups.groups {
                if let Some(chunk) = g.0.find_first(INTERNAL_MODS) {
                    let Ok(arr) = InternalModArray16::try_from(chunk) else {
                        continue;
                    };
                    let Ok(slots) = arr.slots() else { continue };
                    for (slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p
                            .targets
                            .iter()
                            .map(|t| {
                                format!(
                                    "{}@{:?} i={} inv={} lag={} fl={:#x} sh={}",
                                    t.param,
                                    t.slot,
                                    t.intensity,
                                    t.invert,
                                    t.lag_ms,
                                    t.unknown_flags,
                                    t.shaper.as_ref().is_some_and(|s| s.enabled)
                                )
                            })
                            .collect();
                        let src = match &p.modulator {
                            Modulator::Lfo(l) => format!(
                                "LFO v{:#x} wf={} init={:?} r0={:?} r1={:?} tf={} tv={:?} add={:?}",
                                l.version,
                                l.waveform,
                                l.initial_values,
                                l.records[0],
                                l.records[1],
                                l.trailing_flag,
                                l.trailing_values,
                                l.additional_flag
                            ),
                            Modulator::Ahdsr(e) => format!(
                                "AHDSR a={} h={} d={} s={} r={} c={} f={}",
                                e.attack_ms,
                                e.hold_ms,
                                e.decay_ms,
                                e.sustain,
                                e.release_ms,
                                e.attack_curve,
                                e.unknown_flag
                            ),
                            Modulator::Flex(e) => format!("FLEX {:?} sus={}", e.points, e.sustain),
                            Modulator::Other { chunk_id } => format!("OTHER {chunk_id:#x}"),
                        };
                        let key =
                            format!("INT {} flags={:?} {src} -> {t:?}", p.name, p.unknown_flags);
                        let e = seen
                            .entry(key)
                            .or_insert((0, format!("{} slot {slot}", f.display())));
                        e.0 += 1;
                    }
                }
                if let Some(chunk) = g.0.find_first(EXTERNAL_MODS) {
                    let Ok(arr) = ExternalModArray32::try_from(chunk) else {
                        continue;
                    };
                    let Ok(slots) = arr.slots() else { continue };
                    for (_slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p
                            .targets
                            .iter()
                            .map(|t| {
                                format!(
                                    "{}@{:?} i={} inv={} lag={} fl={:#x} sh={}",
                                    t.param,
                                    t.slot,
                                    t.intensity,
                                    t.invert,
                                    t.lag_ms,
                                    t.unknown_flags,
                                    t.shaper.as_ref().is_some_and(|s| s.enabled)
                                )
                            })
                            .collect();
                        let key = format!("EXT {} {:?} -> {t:?}", p.name, p.source);
                        let e = seen.entry(key).or_insert((0, f.display().to_string()));
                        e.0 += 1;
                    }
                }
            }
        }
        for (k, (n, f)) in &seen {
            println!("{n:6} {k}\n         e.g. {f}");
        }
        println!("{} files", files.len());
    }
}

/// A script slot's saved persistent values, from Kontakt's `"<name> <value>"`
/// entries: `$` integers, `~` reals, `@` strings. Arrays (`%`, `?`, `!`) are
/// left out.
fn saved(entries: &[String]) -> Vec<(String, ir::Saved)> {
    entries
        .iter()
        .filter_map(|entry| {
            let (name, rest) = entry.split_once(' ').unwrap_or((entry, ""));
            let value = match name.as_bytes().first()? {
                b'$' => ir::Saved::Int(rest.trim().parse().ok()?),
                b'~' => ir::Saved::Real(rest.trim().parse().ok()?),
                b'@' => ir::Saved::Text(rest.to_owned()),
                b'%' => ir::Saved::Ints(
                    rest.split_whitespace()
                        .map(|n| n.parse().ok())
                        .collect::<Option<_>>()?,
                ),
                b'?' => ir::Saved::Reals(
                    rest.split_whitespace()
                        .map(|n| n.parse().ok())
                        .collect::<Option<_>>()?,
                ),
                _ => return None,
            };
            Some((name.to_owned(), value))
        })
        .collect()
}

#[cfg(test)]
mod saved_tests {
    #[test]
    fn saved_values_keep_their_types_and_arrays() {
        let entries = [
            "$level 17",
            "~mix 0.5",
            "@label two words",
            "%table 1 2 3",
            "$bad x",
        ]
        .map(String::from);
        let saved = super::saved(&entries);
        assert_eq!(
            saved,
            [
                ("$level".to_owned(), sampler_ir::Saved::Int(17)),
                ("~mix".to_owned(), sampler_ir::Saved::Real(0.5)),
                (
                    "@label".to_owned(),
                    sampler_ir::Saved::Text("two words".into())
                ),
                ("%table".to_owned(), sampler_ir::Saved::Ints(vec![1, 2, 3])),
            ]
        );
    }
}

#[cfg(test)]
mod modulation {
    use super::*;
    use ni_file::kontakt::objects::{Lfo, LfoRecord, ModTarget};

    #[test]
    fn ahdsr_stage_laws_are_native_exponential_curves() {
        let curve = |k: ir::Curve, t: f64| match k {
            ir::Curve::Exponential(k) => (k * t).exp_m1() / k.exp_m1(),
            ir::Curve::Linear | ir::Curve::Step => t,
        };
        let (attack, fall) = ahdsr_curves(0.5);
        // Decay: 1.075·(3/43)^t − 0.075 falls from 1 to 0.
        for t in [0.25, 0.5, 0.9] {
            let kontakt = 1.075 * (3.0f64 / 43.0).powf(t) - 0.075;
            assert!((1.0 - curve(fall, t) - kontakt).abs() < 2e-3, "{t}");
        }
        // Positive curve: geometric from 1 + b down to b, read as start − state.
        let b = f64::from(((0.5 * 500_000f64.ln() - 20_000f64.ln()).exp()) as f32);
        for t in [0.1, 0.5] {
            let kontakt = (1.0 + b) * (1.0 - (b / (1.0 + b)).powf(t));
            assert!((curve(attack, t) - kontakt).abs() < 1e-9, "{t}");
        }
        // Negative curves start slowly.
        let (attack, _) = ahdsr_curves(-0.5);
        assert!(curve(attack, 0.5) < 0.5);
    }

    fn translation() -> Translation {
        Translation {
            ir: ir::Instrument::default(),
            assets: HashMap::new(),
            locations: Vec::new(),
            start_criteria: Vec::new(),
            voice_groups: Vec::new(),
        }
    }

    fn target(param: &str, intensity: f32) -> ModTarget {
        ModTarget {
            param: param.into(),
            intensity,
            lag_ms: 0,
            name: String::new(),
            slot: None,
            invert: false,
            shaper: None,
            unknown_i16: -1,
            unknown_flags: 0x10,
        }
    }

    /// KONTAKT_REFERENCE.md section 17: with a 2 s decay to sustain 0.25 the
    /// envelope minus sustain falls 0.51, 0.23, 0.085, 0.02 at 0.25 s steps
    /// from the 2 s peak (1 - 0.25 = 0.75 of travel, so ratios .67/.30/.11/.026).
    #[test]
    fn ahdsr_decay_is_exponential_like_the_measurement() {
        let (attack, ir::Curve::Exponential(k)) = ahdsr_curves(0.0) else {
            panic!("exponential decay")
        };
        let remaining = |t: f64| 1.0 - ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        for (t, measured) in [(0.125, 0.67), (0.375, 0.30), (0.625, 0.11), (0.875, 0.026)] {
            assert!(
                (remaining(t) - measured).abs() < 0.04,
                "t {t}: {} vs {measured}",
                remaining(t)
            );
        }
        // Curve 0 attack is near-linear: env 0.12/0.35/0.59/0.84 at 1/8, 3/8, 5/8, 7/8.
        let ir::Curve::Exponential(a) = attack else {
            panic!("attack curve")
        };
        let rise = |t: f64| ((a * t).exp() - 1.0) / (a.exp() - 1.0);
        for (t, measured) in [(0.125, 0.12), (0.375, 0.35), (0.625, 0.59), (0.875, 0.84)] {
            assert!((rise(t) - measured).abs() < 0.04, "t {t}: {}", rise(t));
        }
    }

    #[test]
    fn filter_cutoff_modulation_is_ten_octaves_per_full_amount() {
        let mut t = translation();
        let source = ir::ModulatorRef(0);
        let cutoff = ModTarget {
            slot: Some(2),
            ..target("filterCutoff", 0.444)
        };
        let chain = ir::ChainRef(3);
        t.route("g", source, true, &cutoff, Some((chain, &[(2, 1)])))
            .unwrap();
        assert_eq!(
            t.ir.routes[0].target,
            ir::Target::Processor {
                chain,
                index: 1,
                parameter: ir::ProcessorParameter::Cutoff
            }
        );
        let ir::Depth::Pitch(p) = t.ir.routes[0].depth else {
            panic!("pitch depth")
        };
        assert!((p.semitones() / 12.0 - 4.44 * 1.0).abs() < 1e-6);
        // A slot with no translated filter stays reported.
        assert!(
            t.route("g", source, true, &cutoff, Some((chain, &[])))
                .is_none()
        );
    }

    #[test]
    fn targets_translate_with_kontakt_laws_or_are_reported() {
        let mut t = translation();
        let source = ir::ModulatorRef(0);
        let lagged = ModTarget {
            lag_ms: 40,
            invert: true,
            ..target("pitch", 0.5)
        };
        t.route("g", source, true, &lagged, None).unwrap();
        assert_eq!(
            t.ir.routes[0],
            ir::Route {
                source,
                target: ir::Target::Pitch,
                depth: ir::Depth::Pitch(ir::Pitch::Semitones(6.0)),
                invert: true,
                shape: None,
                smoothing: ir::Time::Milliseconds(40.0),
                scale: None,
            }
        );
        t.route("g", source, true, &target("volume", 0.25), None)
            .unwrap();
        assert_eq!(t.ir.routes[1].depth, ir::Depth::Normalized(0.25));
        t.route("g", source, true, &target("playPos", 1.0), None)
            .unwrap();
        assert_eq!(t.ir.routes[2].target, ir::Target::SampleStart);
        for unknown in [
            target("pan", 1.0),
            target("cutoff", 1.0),
            ModTarget {
                unknown_flags: 0x12,
                ..target("pitch", 1.0)
            },
            ModTarget {
                slot: Some(0),
                ..target("cutoff", 1.0)
            },
        ] {
            assert!(t.route("g", source, true, &unknown, None).is_none());
        }
        assert_eq!(t.ir.routes.len(), 3);
        let reasons: Vec<_> = t.ir.unsupported.iter().map(|u| u.reason).collect();
        assert_eq!(
            reasons,
            [
                ir::Reason::UnknownLaw,
                ir::Reason::NotModeled,
                ir::Reason::UnknownLaw,
                ir::Reason::NotModeled
            ]
        );
    }

    #[test]
    fn lfos_translate_known_waves_and_report_the_rest() {
        let lfo = |waveform, weights| Lfo {
            structured: false,
            version: 0x72,
            waveform,
            initial_values: [10.0, 4.0, 0.5, 0.25],
            records: [
                LfoRecord {
                    flag: true,
                    values: [-1.0, 0.0, 0.0],
                },
                LfoRecord {
                    flag: false,
                    values: [0.0; 3],
                },
            ],
            trailing_flag: false,
            trailing_values: weights,
            additional_flag: None,
        };
        let mut t = translation();
        let sine = t.lfo("g", &lfo(0, None), true).unwrap();
        assert_eq!(sine.shape, ir::LfoShape::Sine);
        assert_eq!(sine.rate, ir::Frequency::Hertz(4.0));
        assert_eq!(sine.fade_in, ir::Time::Milliseconds(10.0));
        assert_eq!(sine.phase, 0.25);
        let multi = t
            .lfo("g", &lfo(5, Some([0.0, 0.0, 0.7, 0.0, 0.0])), false)
            .unwrap();
        assert_eq!(multi.shape, ir::LfoShape::Triangle);
        assert!(!multi.retrigger);
        let mut synced = lfo(1, None);
        synced.records[0].values[0] = 0.25;
        assert_eq!(
            t.lfo("g", &synced, true).unwrap().rate,
            ir::Frequency::Beats(1.0)
        );
        assert!(t.lfo("g", &lfo(3, None), true).is_none());
        assert!(
            t.lfo("g", &lfo(5, Some([0.5, 0.0, 0.5, 0.0, 0.0])), true)
                .is_none()
        );
        assert_eq!(t.ir.unsupported.len(), 2);
    }
}

#[cfg(test)]
mod multi_tests {
    #[test]
    fn a_multi_program_translates() {
        let Ok(root) = std::env::var("KONTRA_KONTAKT_LIBRARIES") else {
            return;
        };
        let path = std::path::Path::new(&root)
            .join("Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm");
        if !path.exists() {
            return;
        }
        let k = super::read_program(&path, 0).expect("first program");
        assert!(!k.instrument.groups.is_empty());
        assert!(super::read_program(&path, 999).is_err());
    }
}

/// Kontakt's master volume at its displayed 0.00 dB is 0.5 linear against
/// file level (-6.021 dB bit-exact on a bare noise instrument at velocity 127;
/// +6.00 dB is unity; KONTAKT_REFERENCE.md). Applied with the zone gain, so it
/// is exact for linear signal paths (ponytail: nonlinear inserts see the
/// halved level; a master-scope stage if one ever matters).
pub const MASTER_UNITY: f64 = 0.5;

fn output_gain(zone: f32, program: f32) -> f64 {
    f64::from(zone * program) * MASTER_UNITY
}

#[cfg(test)]
mod master_tests {
    #[test]
    fn neutral_zone_is_six_db_below_file_level() {
        let db = 20.0 * super::output_gain(1.0, 1.0).log10();
        assert!((db + 6.0206).abs() < 1e-3, "{db}");
    }
}

/// Exponential bow constant for a shaper or flex-envelope segment:
/// `k = BOW_K * |curvature|`.
/// Measured, not in the manual: fitted to Kontakt 8 renders of Vista 3 Cellos
/// group 37 swept over CC100 (0..96), 0.08 dB RMS residual with K = 18; the
/// Una Corda Cotton "Depth" shaper GUI reading (curvature 0.118, mid value
/// 0.29) implies K about 15. docs/architecture-v2/KONTAKT_REFERENCE.md.
const BOW_K: f64 = 17.0;
const BOW_STEPS: usize = 16;

/// A breakpoint shaper as plain points: each segment's stored curvature
/// (positive bows below the chord, negative above) expands into `BOW_STEPS`
/// linear pieces of `y0 + (y1 - y0) f(t)`, with `e(t) = (e^kt - 1)/(e^k - 1)`
/// and `f = e(t)` when the curvature's sign matches the segment's rise (a
/// rising positive or falling negative segment), else `1 - e(1 - t)`.
fn curved_shaper(points: &[ni_file::kontakt::objects::Breakpoint]) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for (n, p) in points.iter().enumerate() {
        let (x0, y0, c) = (f64::from(p.x), f64::from(p.y), f64::from(p.curve));
        out.push((x0, y0));
        let Some(next) = points.get(n + 1) else {
            continue;
        };
        let (x1, y1) = (f64::from(next.x), f64::from(next.y));
        let k = BOW_K * c.abs();
        if k < 1e-6 || x1 <= x0 {
            continue;
        }
        let e = |t: f64| ((k * t).exp() - 1.0) / (k.exp() - 1.0);
        let same = (c > 0.0) == (y1 > y0);
        for i in 1..BOW_STEPS {
            let t = i as f64 / BOW_STEPS as f64;
            let f = if same { e(t) } else { 1.0 - e(1.0 - t) };
            out.push((x0 + (x1 - x0) * t, y0 + (y1 - y0) * f));
        }
    }
    out
}

#[cfg(test)]
mod curved_shaper_tests {
    use super::curved_shaper;
    use ni_file::kontakt::objects::Breakpoint;

    fn at(points: &[(f64, f64)], x: f64) -> f64 {
        let i = points.iter().position(|p| p.0 >= x).unwrap();
        if i == 0 {
            return points[0].1;
        }
        let (a, b) = (points[i - 1], points[i]);
        a.1 + (b.1 - a.1) * (x - a.0) / (b.0 - a.0)
    }

    fn seg(y0: f32, y1: f32, c: f32) -> Vec<(f64, f64)> {
        curved_shaper(&[
            Breakpoint {
                x: 0.0,
                y: y0,
                curve: c,
            },
            Breakpoint {
                x: 1.0,
                y: y1,
                curve: 0.0,
            },
        ])
    }

    #[test]
    fn a_positive_rising_segment_bows_below_the_chord_like_unas_depth_shaper() {
        // Una Corda Cotton "Depth": curvature 0.118, GUI mid value 0.29-0.30.
        let p = seg(0.0, 1.0, 0.11824325);
        assert!((at(&p, 0.5) - 0.28).abs() < 0.03, "{}", at(&p, 0.5));
    }

    #[test]
    fn curvature_sign_is_geometric_not_directional() {
        // Vista CC100 shaper: a falling segment with negative curvature
        // bows above the chord, and mirrors the rising positive one.
        let up = seg(0.0, 1.0, 0.15);
        let down = seg(1.0, 0.0, -0.15);
        assert!(at(&down, 0.5) > 0.5);
        assert!((at(&up, 0.5) + at(&down, 0.5) - 1.0).abs() < 1e-9);
        let flat = seg(0.0, 1.0, 0.0);
        assert_eq!(flat.len(), 2);
    }
}

#[cfg(test)]
mod census {
    #[test]
    #[ignore]
    fn mic_census() {
        let root = std::env::var("KONTRA_KONTAKT_LIBRARIES").unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let mut files = Vec::new();
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p)
                } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) {
                    files.push(p)
                }
            }
        }
        files.sort();
        for f in &files {
            let Ok(k) = super::read(f) else { continue };
            let i = &k.instrument;
            let mut outs = std::collections::BTreeSet::new();
            for g in &i.groups {
                outs.insert(format!("{:?}", g.output));
            }
            let names: std::collections::BTreeSet<_> =
                i.groups.iter().map(|g| g.name.as_str()).collect();
            let buses: Vec<_> = i.buses.iter().map(|b| b.name.as_str()).collect();
            println!(
                "CENSUS\t{}\tgroups={}\tdistinct_names={}\touts={}\tbuses={:?}\tnames={:?}",
                f.display(),
                i.groups.len(),
                names.len(),
                outs.len(),
                buses,
                names.iter().take(12).collect::<Vec<_>>()
            );
        }
    }
}
