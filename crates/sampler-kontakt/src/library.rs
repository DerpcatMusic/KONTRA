//! Real Kontakt instruments (NKI, plain or encrypted) to the semantic IR,
//! using the vendored `ni-file` decoders the v1 importer is built on. The
//! translator states only what it decodes; everything else it finds is listed
//! in [`ir::Instrument::unsupported`] with its source location.

use crate::{LoadError, Samples};
use ni_file::{
    NIFile,
    kontakt::{
        KontaktChunks, StructuredObject,
        objects::{
            BParScript, ExternalModArray32, FNTableImpl, FileNameListPreK51, Group, GroupList,
            InternalModArray16, LoopArray, ModSource, Modulator, Program,
        },
    },
    nis::schema::{NISObject, PresetChunkItem, Repository},
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
    let chunks = chunks(&path)?;
    let invalid = |reason: &str| LoadError::Invalid {
        path: path.clone(),
        reason: reason.into(),
    };
    let decode = |what, error| LoadError::decode(&path, what, error);
    let program = Program::try_from(
        chunks
            .find_first(PROGRAM)
            .ok_or_else(|| invalid("not a single-instrument preset"))?,
    )
    .map_err(|e| decode("program", e))?;
    let params = program
        .params()
        .map_err(|e| decode("program parameters", e))?;
    let table = match chunks.find_first(FILE_TABLE) {
        Some(chunk) => {
            FNTableImpl::try_from(chunk)
                .map_err(|e| decode("sample file table", e))?
                .sample_filetable
        }
        None => {
            let chunk = chunks
                .find_first(LEGACY_FILE_TABLE)
                .ok_or_else(|| invalid("missing sample file table"))?;
            FileNameListPreK51::try_from(chunk)
                .map_err(|e| decode("legacy file table", e))?
                .sample_filetable
        }
    };
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
    };
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
                if !script.persistent.is_empty() {
                    out.unsupported(
                        &location,
                        "saved persistent values",
                        script.persistent.len(),
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
                    state: Vec::new(),
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
    out.ir.unsupported.dedup();
    out.ir.validate().map_err(|e| invalid(&e.to_string()))?;
    Ok(Kontakt {
        instrument: out.ir,
        locations: out.locations,
        samples,
    })
}

/// The Kontakt chunk stream inside an NKS or NIS (Kontakt 5+) container.
fn chunks(path: &Path) -> Result<KontaktChunks, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    let mut file = std::fs::File::open(path).map_err(|e| LoadError::io(path, e))?;
    if file.metadata().map_err(|e| LoadError::io(path, e))?.len() > 128 << 20 {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "instrument exceeds 128 MiB".into(),
        });
    }
    let bytes = match NIFile::read(&mut file).map_err(|e| decode("container", e))? {
        NIFile::NKSContainer(nks) => nks
            .decompressed_preset()
            .map_err(|e| decode("NKS preset", e))?,
        NIFile::NISoundContainer(nis) => nis_payload(nis, path, 0)?,
        _ => {
            return Err(LoadError::Invalid {
                path: path.into(),
                reason: "not an instrument container".into(),
            });
        }
    };
    KontaktChunks::read(Cursor::new(bytes)).map_err(|e| decode("Kontakt chunks", e))
}

fn nis_payload(
    container: ni_file::nis::ItemContainer,
    path: &Path,
    depth: usize,
) -> Result<Vec<u8>, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    if depth > 3 {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "too many nested NIS wrappers".into(),
        });
    }
    if let Some(data) = container.find_data(&ni_file::nis::ItemType::AppSpecific) {
        let app = ni_file::nis::AppSpecificProperties::try_from(data)
            .map_err(|e| decode("NIS app wrapper", e))?;
        return nis_payload(
            app.subtree_item
                .item()
                .map_err(|e| decode("NIS subtree", e))?,
            path,
            depth + 1,
        );
    }
    let NISObject::BNISoundPreset(preset) = Repository::from(container).infer_schema() else {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "unsupported NIS preset structure".into(),
        });
    };
    let key = match preset.is_encrypted().map_err(|e| decode("NIS preset", e))? {
        true => Some(
            crate::library_key(path).map_err(|reason| LoadError::Access {
                path: path.into(),
                reason,
            })?,
        ),
        false => None,
    };
    let item = preset
        .encryption_item_with_key(key.as_deref())
        .map_err(|e| decode("NIS preset subtree", e))?;
    let chunk = PresetChunkItem::from(
        item.subtree
            .item()
            .map_err(|e| decode("NIS preset subtree", e))?,
    );
    Ok(chunk
        .properties()
        .map_err(|e| decode("NIS preset chunk", e))?
        .0)
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
}

struct Translation {
    ir: ir::Instrument,
    assets: HashMap<PathBuf, ir::AssetRef>,
    locations: Vec<PathBuf>,
}

impl Translation {
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

    /// A group's settings, or `None` for a muted group.
    fn group(&mut self, index: usize, group: &Group) -> Result<Option<GroupInfo>, ni_file::Error> {
        let v = group.params()?;
        let at = format!("group {index} {:?}", v.name);
        if v.muted {
            return Ok(None);
        }
        let not_modeled = ir::Reason::NotModeled;
        if v.release_trigger && v.rls_trig_counter != 0 {
            self.unsupported(
                &at,
                "release trigger counter decay",
                v.rls_trig_counter,
                not_modeled,
            );
        }
        if v.release_trigger && v.release_trigger_note_monophonic {
            self.unsupported(&at, "monophonic release trigger", true, not_modeled);
        }
        if v.voice_group_index >= 0 {
            self.unsupported(&at, "voice group", v.voice_group_index, not_modeled);
        }
        if v.midi_channel >= 0 {
            self.unsupported(&at, "MIDI channel filter", v.midi_channel, not_modeled);
        }
        if !v.start_criteria.items.is_empty() {
            let modes: Vec<_> = v
                .start_criteria
                .items
                .iter()
                .map(|c| (c.mode, c.next_criteria, c.cycle_class))
                .collect();
            self.unsupported(
                &at,
                "group start options (mode, next, cycle class)",
                format!("{modes:?}"),
                not_modeled,
            );
        }
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
        if let Ok(fx) = group.insert_fx().and_then(|fx| fx.fx_items()) {
            for (slot, fx) in fx.iter().enumerate() {
                if let Ok(params) = fx.params()
                    && !params.bypass
                {
                    let kind = fx.effect().map_or(0, |c| c.id);
                    self.unsupported(
                        &format!("{at} insert slot {slot}"),
                        "insert effect (serialization type)",
                        format!("{kind:#x}"),
                        not_modeled,
                    );
                }
            }
        }
        let mut envelope = None;
        if let Some(chunk) = group.0.find_first(INTERNAL_MODS) {
            for (slot, modulator) in InternalModArray16::try_from(chunk)?.slots()? {
                let params = modulator.params()?;
                let targets: Vec<_> = params.targets.iter().map(|t| t.param.as_str()).collect();
                let volume = targets == ["volume"]
                    && params.targets[0].intensity == 1.0
                    && !params.targets[0].invert;
                match params.modulator {
                    Modulator::Ahdsr(env) if volume && envelope.is_none() => {
                        if env.unknown_flag != 0 {
                            self.unsupported(
                                &at,
                                "AHD-only envelope mode",
                                env.unknown_flag,
                                not_modeled,
                            );
                        }
                        // Kontakt's stages are exponential (decay and release
                        // fall to 3/43 in their stage time; the attack bends
                        // with its curve). The IR states the authored times;
                        // the stage law is reported, not guessed.
                        self.unsupported(
                            &at,
                            "AHDSR stage law",
                            format!("attack curve {}", env.attack_curve),
                            not_modeled,
                        );
                        let ms = |ms: f32| ir::Time::Milliseconds(f64::from(ms.max(0.0)));
                        self.ir.modulators.push(ir::Modulator {
                            scope: ir::Scope::Voice,
                            source: ir::ModulationSource::Envelope(ir::Envelope {
                                attack: ms(env.attack_ms),
                                hold: ms(env.hold_ms),
                                decay: ms(env.decay_ms),
                                sustain: f64::from(env.sustain.clamp(0.0, 1.0)),
                                release: ms(env.release_ms),
                                ..Default::default()
                            }),
                        });
                        envelope = Some(ir::ModulatorRef(self.ir.modulators.len() - 1));
                    }
                    other => {
                        let kind = match other {
                            Modulator::Ahdsr(_) => "internal AHDSR modulation".into(),
                            Modulator::Flex(_) => "internal flex envelope modulation".into(),
                            Modulator::Lfo(_) => "internal LFO modulation".into(),
                            Modulator::Other { chunk_id } => {
                                format!("internal modulator chunk {chunk_id:#x}")
                            }
                        };
                        self.unsupported(
                            &format!("{at} modulator slot {slot}"),
                            &kind,
                            format!("{:?} -> {targets:?}", params.name),
                            not_modeled,
                        );
                    }
                }
            }
        }
        let mut velocity = ir::VelocityResponse::None;
        if let Some(chunk) = group.0.find_first(EXTERNAL_MODS) {
            for (slot, modulation) in ExternalModArray32::try_from(chunk)?.slots()? {
                let params = modulation.params()?;
                let targets: Vec<_> = params
                    .targets
                    .iter()
                    .map(|t| (t.param.as_str(), t.intensity))
                    .collect();
                if let (ModSource::Velocity, [("volume", intensity)]) =
                    (&params.source, targets.as_slice())
                    && velocity == ir::VelocityResponse::None
                {
                    // Kontakt's velocity-to-volume law is not decoded; the
                    // IR's linear response stands in for any nonzero intensity.
                    if *intensity != 0.0 {
                        velocity = ir::VelocityResponse::Linear;
                    }
                    if *intensity != 0.0 && *intensity != 1.0 {
                        self.unsupported(
                            &at,
                            "velocity to volume intensity",
                            intensity,
                            not_modeled,
                        );
                    }
                    continue;
                }
                let at = format!("{at} external modulation slot {slot}");
                self.unsupported(
                    &at,
                    &format!("external modulation from {:?}", params.source),
                    format!("{:?} -> {targets:?}", params.name),
                    not_modeled,
                );
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
        }))
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
        if z.fades != [0; 4] {
            self.unsupported(
                &at,
                "crossfades (low/high velocity, low/high key)",
                format!("{:?}", z.fades),
                not_modeled,
            );
        }
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
            trigger: if group.release {
                ir::Trigger::KeyRelease
            } else {
                ir::Trigger::Attack
            },
            pitch: if group.tracking {
                ir::KeyTracking::Tracked { root: z.root }
            } else {
                ir::KeyTracking::Fixed
            },
            tune: ir::Pitch::Semitones(semitones),
            gain: ir::Gain::Linear(f64::from(z.gain * program.volume)),
            velocity: group.velocity,
            pan: ir::Pan {
                position: f64::from((z.pan + program.pan).clamp(-1.0, 1.0)),
                law: ir::PanLaw::Balance,
            },
            playback: ir::Playback {
                start: z.start,
                end,
                reverse: group.reverse,
                looping,
            },
            amplitude: group.envelope,
            ..ir::Zone::new(asset)
        });
    }
}

/// One serialized zone: its group, mapping and sample reference.
struct RawZone {
    group: usize,
    start: u64,
    end: i32,
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
        let (start, end, _start_mod) = (i32le(z)?, i32le(z)?, i32le(z)?);
        let mut ranges = [0i16; 9];
        for value in &mut ranges {
            *value = i16le(z)?;
        }
        let (gain, pan, tune) = (f32le(z)?, f32le(z)?, f32le(z)?);
        if so.version >= 0x9a {
            z.read_exact(&mut [0; 6])?;
        }
        Ok((start, end, ranges, gain, pan, tune, i32le(z)?))
    };
    let (start, end, ranges, gain, pan, tune, file) = read(&mut z).map_err(|e| e.to_string())?;
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
                if p.is_dir() { stack.push(p) } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) { files.push(p) }
            }
        }
        files.sort();
        let mut seen = std::collections::BTreeMap::<String, (usize, String)>::new();
        for f in &files {
            let Ok(chunks) = chunks(f) else { continue };
            let Some(program) = chunks.find_first(PROGRAM) else { continue };
            let Ok(program) = Program::try_from(program) else { continue };
            let Some(gl) = program.0.find_first(GROUP_LIST) else { continue };
            let Ok(groups) = GroupList::try_from(gl) else { continue };
            for g in &groups.groups {
                if let Some(chunk) = g.0.find_first(INTERNAL_MODS) {
                    let Ok(arr) = InternalModArray16::try_from(chunk) else { continue };
                    let Ok(slots) = arr.slots() else { continue };
                    for (slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p.targets.iter().map(|t| format!("{}@{:?} i={} inv={} lag={} fl={:#x} sh={}", t.param, t.slot, t.intensity, t.invert, t.lag_ms, t.unknown_flags, t.shaper.as_ref().is_some_and(|s| s.enabled))).collect();
                        let src = match &p.modulator {
                            Modulator::Lfo(l) => format!("LFO v{:#x} wf={} init={:?} r0={:?} r1={:?} tf={} tv={:?} add={:?}", l.version, l.waveform, l.initial_values, l.records[0], l.records[1], l.trailing_flag, l.trailing_values, l.additional_flag),
                            Modulator::Ahdsr(e) => format!("AHDSR a={} h={} d={} s={} r={} c={} f={}", e.attack_ms, e.hold_ms, e.decay_ms, e.sustain, e.release_ms, e.attack_curve, e.unknown_flag),
                            Modulator::Flex(e) => format!("FLEX {:?} sus={}", e.points, e.sustain),
                            Modulator::Other { chunk_id } => format!("OTHER {chunk_id:#x}"),
                        };
                        let key = format!("INT {} flags={:?} {src} -> {t:?}", p.name, p.unknown_flags);
                        let e = seen.entry(key).or_insert((0, format!("{} slot {slot}", f.display())));
                        e.0 += 1;
                    }
                }
                if let Some(chunk) = g.0.find_first(EXTERNAL_MODS) {
                    let Ok(arr) = ExternalModArray32::try_from(chunk) else { continue };
                    let Ok(slots) = arr.slots() else { continue };
                    for (_slot, m) in slots {
                        let Ok(p) = m.params() else { continue };
                        let t: Vec<_> = p.targets.iter().map(|t| format!("{}@{:?} i={} inv={} lag={} fl={:#x} sh={}", t.param, t.slot, t.intensity, t.invert, t.lag_ms, t.unknown_flags, t.shaper.as_ref().is_some_and(|s| s.enabled))).collect();
                        let key = format!("EXT {} {:?} -> {t:?}", p.name, p.source);
                        let e = seen.entry(key).or_insert((0, f.display().to_string()));
                        e.0 += 1;
                    }
                }
            }
        }
        for (k, (n, f)) in &seen { println!("{n:6} {k}\n         e.g. {f}"); }
        println!("{} files", files.len());
    }
}
