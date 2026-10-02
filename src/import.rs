use anyhow::{Context, Result, bail, ensure};
use ni_file::{
    NIFile,
    kontakt::{
        KontaktChunks, StructuredObject,
        objects::{BParScript, FNTableImpl, FileNameListPreK51, GroupList, LoopArray, Program},
    },
    nis::schema::{NISObject, Repository},
};
use serde::Serialize;
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::File,
    io::{Cursor, Read, Seek},
    path::{Path, PathBuf},
};

/// The developer's library folder: the command-line tools' default, and a
/// last place a first run looks. The app's libraries come from its settings.
pub const LIBRARY_ROOT: &str = "/path/to/Kontakt-Libraries";

pub use crate::modulation::{Ahdsr, FlexEnvelope, FlexPoint, ModAssignment, ModEnvelope, ModSource, ModTarget, Modulator, ShaperCurve};

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct Group {
    pub name: String,
    /// Linear amplitude ratio.
    pub gain: f32,
    pub pan: f32,
    /// Linear pitch ratio.
    pub tune: f64,
    pub key_tracking: bool,
    pub reverse: bool,
    pub release_trigger: bool,
    /// Release-trigger counter start `T` in ms (Source module): the counter
    /// counts down from it while the key is held; 0 disables it.
    pub release_counter_ms: i32,
    pub muted: bool,
    pub channel: i16,
    pub soloed: bool,
    /// Volume AHDSR envelope (first internal AHDSR modulating volume).
    pub volume_env: Option<Ahdsr>,
    /// Flex volume envelope (first internal flex envelope modulating volume);
    /// playback multiplies it with `volume_env`.
    pub flex_env: Option<FlexEnvelope>,
    /// External modulation assignments, one per target.
    pub mods: Vec<ModAssignment>,
    /// Internal and external modulators in KSP `find_mod` order.
    pub modulators: Vec<Modulator>,
    /// Internal AHDSRs driving module parameters (filter cutoff, EQ gain).
    pub envelopes: Vec<ModEnvelope>,
    /// Group insert effects (only filters and EQs play).
    pub fx: crate::fx::Chain,
    /// Kontakt voice group (choke/voice-limit group) index, if assigned.
    pub voice_group: Option<u32>,
    /// Raw interpolation quality setting; 0 in every local preset.
    pub interp_quality: i32,
}

impl Default for Group {
    fn default() -> Self {
        Self { name: String::new(), gain: 1.0, pan: 0.0, tune: 1.0, key_tracking: true, reverse: false,
            release_trigger: false, release_counter_ms: 0, muted: false, channel: -1, soloed: false, volume_env: None, flex_env: None, mods: Vec::new(), modulators: Vec::new(), envelopes: Vec::new(), fx: Default::default(), voice_group: None, interp_quality: 0 }
    }
}

/// Kontakt voice-group (and program) polyphony limit.
#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize)]
pub struct VoiceLimit {
    pub max_voices: u32,
    /// 0 any, 1 oldest, 2 newest, 3 highest, 4 lowest.
    pub kill_mode: i16,
    pub prefer_released: bool,
    pub fade_ms: u32,
    /// Voice groups sharing a non-negative exclusion group choke each other.
    pub exclusion_group: i32,
}

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Loop { pub start: usize, pub end: usize, pub until_release: bool, pub crossfade: usize }

#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize)]
pub struct Zone {
    pub group: usize,
    pub sample: PathBuf,
    pub available: bool,
    pub low_key: u8, pub high_key: u8, pub root: u8,
    pub low_velocity: u8, pub high_velocity: u8,
    /// Crossfade width in velocity steps above `low_velocity`.
    pub fade_low_velocity: u8,
    /// Crossfade width in velocity steps below `high_velocity`.
    pub fade_high_velocity: u8,
    /// Crossfade width in keys above `low_key`.
    pub fade_low_key: u8,
    /// Crossfade width in keys below `high_key`.
    pub fade_high_key: u8,
    pub start: usize, pub end: i32,
    /// Sample-start modulation range in frames (driven by `ModTarget::SampleStart`);
    /// `None` when the zone stores -1.
    pub start_mod: Option<u32>,
    pub gain: f32, pub pan: f32, pub tune: f64,
    pub loop_range: Option<Loop>,
}

impl Default for Zone {
    fn default() -> Self {
        Self { group: 0, sample: PathBuf::new(), available: true, low_key: 0, high_key: 127, root: 60,
            low_velocity: 0, high_velocity: 127, start: 0, end: 0, gain: 1.0, pan: 0.0, tune: 1.0, loop_range: None,
            start_mod: None, fade_low_velocity: 0, fade_high_velocity: 0, fade_low_key: 0, fade_high_key: 0 }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Instrument {
    pub path: PathBuf,
    pub name: String,
    pub groups: Vec<Group>,
    pub zones: Vec<Zone>,
    pub warnings: Vec<String>,
    pub missing_samples: Vec<String>,
    #[serde(skip)]
    pub scripts: Vec<String>,
    pub fx: crate::fx::ProgramFx,
    /// Program-wide polyphony limit, when stored.
    pub voice_limit: Option<VoiceLimit>,
    pub voice_groups: Vec<Option<VoiceLimit>>,
    /// Persistent variable values saved with each script, parallel to `scripts`.
    #[serde(skip)]
    pub script_state: Vec<crate::ksp::Persisted>,
    /// Kontakt's own figures saved in the program: total sample bytes and
    /// per-instrument DFD preload override (0 = the global default).
    pub kontakt_sample_bytes: f64,
    pub kontakt_preload: i32,
    /// Files and directories used by resolution, captured when parsed.
    #[serde(skip)]
    pub dependencies: Vec<crate::cache::Dependency>,
}

/// ni-file is a research parser with panic paths. Keep those off the host thread.
pub fn read(path: &Path) -> Result<Instrument> {
    std::panic::catch_unwind(|| read_inner(path,0))
        .map_err(|_| anyhow::anyhow!("Unsupported or malformed Kontakt structure in {}", path.display()))?
        .with_context(|| format!("Reading {}", path.display()))
}

fn chunks(path: &Path) -> Result<KontaktChunks> {
    ensure!(path.metadata()?.len() <= 128 * 1024 * 1024, "Instrument container exceeds 128 MiB import limit");
    let mut file = File::open(path)?;
    let mut header = [0; 16];
    file.read_exact(&mut header)
        .context("Truncated instrument header")?;
    ensure!(
        header.iter().any(|&b| b != 0),
        "Instrument header contains only zero bytes; check for an incomplete/damaged copy or filesystem read failure"
    );
    file.rewind()?;
    let bytes = match NIFile::read(file).context("NIS/NKS container headers")? {
        NIFile::NKSContainer(n) => n.decompressed_preset()?,
        NIFile::NISoundContainer(n) => nis_payload(n,path,0)?,
        _ => bail!("Unsupported instrument container; choose an NKI or NKM preset"),
    };
    ensure!(bytes.len() <= 256 * 1024 * 1024, "Expanded instrument exceeds 256 MiB limit");
    Ok(KontaktChunks::read(Cursor::new(bytes))?)
}
fn nis_payload(n:ni_file::nis::ItemContainer,path:&Path,depth:usize)->Result<Vec<u8>> {
    ensure!(depth<4,"Too many nested NIS wrappers");
    if let Some(data)=n.find_data(&ni_file::nis::ItemType::AppSpecific) {
        let app=ni_file::nis::AppSpecificProperties::try_from(data)?;
        return nis_payload(app.subtree_item.item()?,path,depth+1);
    }
    Ok(match Repository::from(n).infer_schema() {
            NISObject::BNISoundPreset(p) => {
                let key = if p.is_encrypted()? { crate::access::library_key(path)? } else { None };
                let enc = p.encryption_item_with_key(key.as_deref()).context("NIS preset subtree")?;
                ni_file::nis::schema::PresetChunkItem::from(enc.subtree.item()?).properties()?.0
            },
            _ => bail!("Unsupported NIS preset structure"),
    })
}

/// A multi contains embedded programs sharing one file table, not paths to sample files.
#[derive(Debug,Serialize)]
pub struct Multi {pub name:String,pub parts:Vec<MultiPart>}
#[derive(Debug,Serialize)]
pub struct MultiPart {pub program:u32,pub name:String}
fn multi_programs(c:&KontaktChunks)->Result<(String,Vec<(u32,Program)>)> {
    use ni_file::kontakt::objects::{Bank,SlotList,ProgramList};
    let bank=Bank::try_from(c.find_first(3).context("No Kontakt multi bank")?)?;
    let name=bank.params()?.name;
    let slots=SlotList::try_from(bank.0.find_first(0x37).context("Multi has no slot list")?)?;
    let mut programs=Vec::new();
    for (slot,pc) in slots.slots {
        let list=ProgramList::try_from(pc.0.find_first(0x36).context("Multi slot has no programs")?)?;
        ensure!(list.programs.len()==1,"Instrument banks with program switching are not supported");
        for p in list.programs {programs.push((slot as u32,p));}
    }
    programs.sort_by_key(|(slot,_)|*slot);ensure!(!programs.is_empty(),"Multi contains no instruments");Ok((name,programs))
}
pub fn read_multi(path:&Path)->Result<Multi> {
    std::panic::catch_unwind(||->Result<_>{let (name,programs)=multi_programs(&chunks(path)?)?;let parts=programs.into_iter().map(|(program,p)|Ok(MultiPart{program,name:p.params()?.name})).collect::<Result<Vec<_>>>()?;Ok(Multi{name,parts})})
        .map_err(|_|anyhow::anyhow!("Malformed Kontakt multi"))?
}
pub fn read_program(path:&Path,program:u32)->Result<Instrument> {
    std::panic::catch_unwind(||read_inner(path,program)).map_err(|_|anyhow::anyhow!("Malformed Kontakt program"))?
}

/// Apply a Kontakt snapshot to its explicitly supplied base NKI. Snapshots do
/// not contain a sample mapping. Compact saved group/source/modulation state
/// is not yet imported; the returned instrument reports that limitation.
pub fn read_snapshot(base: &Path, snapshot: &Path) -> Result<Instrument> {
    std::panic::catch_unwind(|| read_snapshot_inner(base, snapshot))
        .map_err(|_| anyhow::anyhow!("Malformed Kontakt snapshot or base instrument"))?
}

fn read_snapshot_inner(base: &Path, snapshot: &Path) -> Result<Instrument> {
    use ni_file::kontakt::objects::{Snapshot, snapshot_instrument_name};
    let snapshot_chunks = chunks(snapshot).context("Snapshot container")?;
    let name = snapshot_instrument_name(
        snapshot_chunks
            .find_first(0x51)
            .context("Snapshot metadata missing")?,
    )?;
    let saved = Snapshot::try_from(
        snapshot_chunks
            .find_first(0x4f)
            .context("Snapshot state missing")?,
    )?;
    let base_chunks = chunks(base).context("Base instrument container")?;
    let program = Program::try_from(
        base_chunks
            .find_first(0x28)
            .context("Snapshot requires a base NKI")?,
    )?;
    ensure!(
        name == program.params()?.name,
        "Snapshot requires base instrument {name:?}"
    );
    let mut instrument = read(base)?;
    ensure!(
        saved.group_count as usize == instrument.groups.len(),
        "Snapshot/base group counts differ"
    );
    let native_groups = GroupList::try_from(
        program
            .0
            .find_first(0x33)
            .context("Base group list missing")?,
    )?;
    ensure!(
        native_groups.groups.len() == instrument.groups.len(),
        "Base group mapping differs"
    );
    let snapshot_groups = saved.group_snapshots().context("Compact snapshot groups")?;
    let mut states = Vec::new();
    let mut warnings = Vec::new();
    for ((id, saved), mut native) in snapshot_groups.into_iter().zip(native_groups.groups) {
        let id = id as usize;
        ensure!(
            native.source_state()?[..7] == saved.source_data[..7],
            "Snapshot group {id}: source mode differs from base"
        );
        let native_fx = native.insert_fx()?;
        snapshot_slot_shape(&native_fx, &saved.fx)
            .with_context(|| format!("Snapshot group {id} effects"))?;
        for (chunk_id, count, slots) in [(0x3b, 16, &saved.internal), (0x3c, 32, &saved.external)] {
            let original = native
                .0
                .find_first(chunk_id)
                .context("Base modulation array missing")?;
            let original = ni_file::kontakt::objects::BParamArrayBParFX8::read(
                Cursor::new(&original.data),
                count,
            )?;
            snapshot_slot_shape(&original, slots)
                .with_context(|| format!("Snapshot group {id} modulation"))?;
        }
        for chunk in saved.modulation_chunks()? {
            let original = native
                .0
                .children
                .iter_mut()
                .find(|c| c.id == chunk.id)
                .context("Base modulation array missing")?;
            *original = chunk;
        }
        let modulation = crate::modulation::read_group(&native)
            .with_context(|| format!("Snapshot group {id} modulation parameters"))?;
        let fx = crate::fx::Chain::from_array(&saved.fx)
            .with_context(|| format!("Snapshot group {id} effect parameters"))?;
        warnings.extend(modulation.warnings);
        warnings.extend(crate::engine::filter::unsupported(&fx));
        let group = &mut instrument.groups[id];
        group.volume_env = modulation.volume_env;
        group.flex_env = modulation.flex_env;
        group.mods = modulation.mods;
        group.modulators = modulation.modulators;
        group.envelopes = modulation.envelopes;
        group.fx = fx;
    }
    let mut slots = 0;
    for (slot, chunk) in program.0.children.iter().filter(|c| c.id == 6).enumerate() {
        slots += 1;
        let params = BParScript::try_from(chunk)?.params()?;
        ensure!(
            slot < saved.persistent.len(),
            "Base instrument has more snapshot script slots"
        );
        if !params.bypass && script_source(base, slot, &params, &mut warnings).is_some() {
            states.push(crate::ksp::saved_persistence(&saved.persistent[slot]));
        }
    }
    ensure!(
        saved.persistent[slots..].iter().all(Vec::is_empty),
        "Snapshot has state for absent base script slots"
    );
    ensure!(
        states.len() == instrument.scripts.len(),
        "Snapshot/base active script slots differ"
    );
    ensure!(
        instrument.script_state.len() == states.len(),
        "Base script persistence slots differ"
    );
    // Reuse the regular effect importer after the entire snapshot has parsed.
    let effects = Program(StructuredObject {
        version: program.0.version,
        public_data: Vec::new(),
        private_data: Vec::new(),
        children: saved.effect_children,
    });
    let mut fx = crate::fx::ProgramFx::read(&effects).context("Snapshot effects")?;
    let files = other_files(&snapshot_chunks)?;
    // Reject an invalid rooted path before applying any saved state/effects.
    for name in files.values() { snapshot_rooted_path(name)?; }
    fx.name_impulses(&files);
    let parent = base.parent().context("Base instrument has no parent")?;
    let root = base
        .ancestors()
        .find(|p| p.join("Samples").is_dir())
        .unwrap_or(parent);
    let mut resolver = Resolver::new(root);
    let container = resource_container(&base_chunks)?;
    let mut dependencies = vec![snapshot.to_path_buf()];
    fx.load_impulses(|name, max_frames| {
        // Snapshot filename segment 0x0b anchors the saved path at the base
        // library. The generic filename table retains it as a leading slash.
        let rooted = snapshot_rooted_path(name)?;
        let (at, relative) = rooted.as_deref().map_or((parent, Path::new(name)), |n| (root, n));
        let ir = match (
            resolver.resolve(at, &relative.to_string_lossy())?,
            &container,
            name.find("Resources/"),
        ) {
            (Some(ir), ..) => Some(ir),
            (None, Some(nkr), Some(at)) => {
                resolver.resolve(parent, &format!("{nkr}/{}", &name[at..]))?
            }
            _ => None,
        }
        .context("file missing or its archive member is unreadable")?;
        dependencies.push(ir.clone());
        crate::audio::decode(&ir, max_frames)
    });
    warnings.extend(fx.warnings());
    warnings.push("Snapshot: unknown group public/source fields and trailing selection flags are retained but not applied; base scalar/source settings remain in use".into());
    warnings.push("Snapshot: group IDs require the supplied base NKI's original group arrangement; a reordered foreign base with the same name/count cannot be detected".into());
    for (base, saved) in instrument.script_state.iter_mut().zip(states) {
        base.extend(saved);
    }
    fx.main = std::mem::take(&mut instrument.fx.main);
    instrument.fx = fx;
    instrument.warnings.extend(warnings);
    instrument.warnings.sort();
    instrument.warnings.dedup();
    instrument.name = snapshot
        .file_stem()
        .context("Snapshot has no name")?
        .to_string_lossy()
        .into_owned();
    instrument
        .dependencies
        .extend(crate::cache::dependencies(dependencies));
    Ok(instrument)
}

fn snapshot_slot_shape(
    base: &ni_file::kontakt::objects::BParamArrayBParFX8,
    saved: &ni_file::kontakt::objects::BParamArrayBParFX8,
) -> Result<()> {
    use ni_file::kontakt::objects::{ExternalMod, InternalMod};
    let identity = |chunk: &ni_file::kontakt::Chunk| -> Result<_> {
        let (name, targets) = match chunk.id {
            0x0d => {
                let p = InternalMod::try_from(chunk)?.params()?;
                (p.name, p.targets)
            }
            0x0c => {
                let p = ExternalMod::try_from(chunk)?.params()?;
                (p.name, p.targets)
            }
            _ => return Ok(None),
        };
        Ok(Some((
            name,
            targets
                .into_iter()
                .map(|t| (t.param, t.slot, t.name))
                .collect::<Vec<_>>(),
        )))
    };
    ensure!(base.items.len() == saved.items.len(), "Slot counts differ");
    for (base, saved) in base.items.iter().zip(&saved.items) {
        ensure!(
            base.as_ref().map(|c| c.id) == saved.as_ref().map(|c| c.id),
            "Slot occupancy/types differ"
        );
        if let (Some(base), Some(saved)) = (base, saved) {
            ensure!(
                identity(base)? == identity(saved)?,
                "Modulator/target identities differ"
            );
        }
    }
    Ok(())
}

fn snapshot_rooted_path(name: &str) -> Result<Option<PathBuf>> {
    let Some(relative) = name.strip_prefix('/') else {
        return Ok(None);
    };
    let relative = PathBuf::from(relative.replace('\\', "/"));
    let mut depth = 0usize;
    for component in relative.components() {
        use std::path::Component;
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => bail!("Snapshot filename escapes its library root"),
        }
    }
    Ok(Some(relative))
}

/// [`read_program`], shared: parts and plugin instances in one process that
/// load the same program hold one parsed copy while any of them lives.
pub fn shared_program(path: &Path, program: u32) -> Result<std::sync::Arc<Instrument>> {
    use std::sync::{Arc, Mutex, Weak};
    type Parsed = std::collections::HashMap<(PathBuf, u32), Weak<Instrument>>;
    static PARSED: Mutex<Option<Parsed>> = Mutex::new(None);
    let key = (std::fs::canonicalize(path).unwrap_or_else(|_| path.into()), program);
    let lock = || PARSED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = lock().get_or_insert_default().get(&key).and_then(Weak::upgrade) {
        if i.missing_samples.is_empty() && crate::cache::current(&i.dependencies) {
            return Ok(i);
        }
    }
    let instrument = Arc::new(read_program(path, program)?);
    let mut parsed = lock();
    let parsed = parsed.get_or_insert_default();
    parsed.retain(|_, i| i.strong_count() > 0);
    parsed.insert(key, Arc::downgrade(&instrument));
    Ok(instrument)
}

/// Effect racks of every program (slot, effects), with IR file names but no audio decoded.
pub fn read_fx(path: &Path) -> Result<Vec<(u32, crate::fx::ProgramFx)>> {
    std::panic::catch_unwind(|| -> Result<_> {
        let c = chunks(path)?;
        let programs = match c.find_first(0x28) {
            Some(p) => vec![(0, Program::try_from(p)?)],
            None => multi_programs(&c)?.1,
        };
        let files = other_files(&c)?;
        programs
            .into_iter()
            .map(|(slot, p)| {
                let mut fx = crate::fx::ProgramFx::read(&p)?;
                fx.name_impulses(&files);
                Ok((slot, fx))
            })
            .collect()
    })
    .map_err(|_| anyhow::anyhow!("Malformed Kontakt effects"))?
}

/// The preset's non-sample file table (IRs, the preset itself).
fn other_files(c: &KontaktChunks) -> Result<HashMap<u32, String>> {
    Ok(c.0.iter().find(|c| c.id == 0x4b).map(FNTableImpl::try_from).transpose()?.map(|t| t.other_filetable).unwrap_or_default())
}

/// The library resource container (`.nkr`) that `.../Resources/...` paths live in.
fn resource_container(c: &KontaktChunks) -> Result<Option<String>> {
    let table = c.0.iter().find(|c| c.id == 0x4b).map(FNTableImpl::try_from).transpose()?;
    Ok(table.and_then(|t| t.special_filetable.into_values().find(|f| f.to_lowercase().ends_with(".nkr"))))
}

/// The source of script slot `slot` (0-based among the instrument's slots):
/// the `Resources/scripts` file it links to when the library has it (Kontakt
/// reloads linked scripts), else the text saved in the slot. Why a linked
/// slot has no text is a warning, never a generic script error.
fn script_source(path: &Path, slot: usize, s: &ni_file::kontakt::objects::BParScriptParams, warnings: &mut Vec<String>) -> Option<String> {
    let saved = s.text.as_deref().map(|t| script_text(t.as_bytes())).filter(|t| !t.trim().is_empty());
    let Some(link) = s.textfile_name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else { return saved };
    let slot = slot + 1;
    let linked = crate::resources::linked_script(path, link);
    match linked {
        Ok(Some(bytes)) => return Some(script_text(&bytes)).filter(|t| !t.trim().is_empty()).or(saved),
        Ok(None) if saved.is_some() => {}
        Ok(None) => warnings.push(format!("Script slot {slot}: linked script Resources/scripts/{} not found", link.rsplit(['/', '\\']).next().unwrap_or(link))),
        Err(e) => warnings.push(format!("Script slot {slot}: linked script {link} is unreadable: {e}")),
    }
    saved
}

/// Script text as Kontakt may store it: UTF-8 (with or without a byte order
/// mark), UTF-16LE, or Windows-1252, whose high half Latin-1 approximates.
fn script_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let utf16 = bytes.starts_with(b"\xFF\xFE") || (bytes.len() >= 4 && bytes[0] != 0 && bytes[1] == 0 && bytes[3] == 0);
    if utf16 {
        let units: Vec<u16> = bytes.strip_prefix(b"\xFF\xFE").unwrap_or(bytes).chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| bytes.iter().map(|&b| char::from(b)).collect())
}

/// Inspect scripts without sample resolution, so compatibility rescans do not reopen sample archives.
pub fn script_inventory(path:&Path)->Result<serde_json::Value>{
    std::panic::catch_unwind(||->Result<_>{
        let c=chunks(path)?;let programs=if let Some(p)=c.find_first(0x28){vec![(0,Program::try_from(p)?)]}else{multi_programs(&c)?.1};let mut report=Vec::new();
        for (program,p) in programs {
            let groups=GroupList::try_from(p.0.find_first(0x33).context("Missing group list")?)?.groups.len();let mut scripts=Vec::new();let mut host=crate::ksp::HostState::default();
            let mut warnings=Vec::new();
            for (slot,child) in p.0.children.iter().filter(|c|c.id==6).enumerate() {let s=BParScript::try_from(child)?.params()?;if !s.bypass && let Some(source)=script_source(path,slot,&s,&mut warnings){scripts.push(crate::ksp::inspect(&source,groups,&mut host));}}
            if !warnings.is_empty() {scripts.push(serde_json::json!({"warnings":warnings}));}
            report.push(serde_json::json!({"program":program,"scripts":scripts}));
        }
        Ok(serde_json::json!(report))
    }).map_err(|_|anyhow::anyhow!("Malformed script inventory"))?
}

/// Inventory unconsumed program/group chunks without treating their presence as playback support.
pub fn source_inventory(path:&Path)->Result<serde_json::Value>{
    std::panic::catch_unwind(||->Result<_>{
        fn visit(c:&ni_file::kontakt::Chunk,scope:&str,depth:usize,budget:&mut usize,out:&mut std::collections::BTreeMap<String,serde_json::Value>)->Result<()> {
            ensure!(depth<32 && *budget>0,"Source inventory limit");*budget-=1;
            let key=format!("{scope}/0x{:02x}",c.id);
            let row=out.entry(key.clone()).or_insert_with(||serde_json::json!({"count":0,"bytes":0}));
            row["count"]=serde_json::json!(row["count"].as_u64().unwrap()+1);row["bytes"]=serde_json::json!(row["bytes"].as_u64().unwrap()+c.data.len() as u64);
            let result=(||->Result<()>{
                if c.id==0x38 {
                    let criteria=ni_file::kontakt::objects::StartCriteriaList::try_from(c)?;
                    for criterion in criteria.items {let mode=format!("mode-{}",criterion.mode);let row=out.get_mut(&key).unwrap();let count=row[&mode].as_u64().unwrap_or(0);row[mode]=serde_json::json!(count+1);}
                }else if matches!(c.id,0x3a..=0x3c) {
                    let object=StructuredObject::try_from(c)?;
                    ensure!(matches!(object.version,0x10|0x12),"Unsupported parameter array version 0x{:x}",object.version);
                    let mut r=Cursor::new(&object.public_data);let slots=match c.id {0x3a=>8,0x3b=>16,_=>32};
                    for _ in 0..slots {let mut present=[0];r.read_exact(&mut present)?;ensure!(present[0]<=1,"Invalid parameter slot flag");if present[0]==1 {visit(&ni_file::kontakt::Chunk::read(&mut r)?,&key,depth+1,budget,out)?;}}
                }else if c.data.first()==Some(&1) {
                    let object=StructuredObject::try_from(c)?;
                    for child in &object.children {visit(child,&key,depth+1,budget,out)?;}
                }
                Ok(())
            })();
            if let Err(e)=result {out.get_mut(&key).unwrap()["inspection_error"]=serde_json::json!(format!("{e:#}"));}
            Ok(())
        }
        let c=chunks(path)?;let programs=if let Some(p)=c.find_first(0x28){vec![(0,Program::try_from(p)?)]}else{multi_programs(&c)?.1};
        let mut out=std::collections::BTreeMap::new();let mut budget=250_000;
        for (slot,p) in programs {
            let scope=format!("program-{slot}");
            for child in &p.0.children {if !matches!(child.id,0x33|0x34){visit(child,&scope,0,&mut budget,&mut out)?;}}
            let groups=GroupList::try_from(p.0.find_first(0x33).context("Missing group list")?)?;
            for g in groups.groups {for child in &g.0.children {visit(child,&format!("{scope}/group"),0,&mut budget,&mut out)?;}}
        }
        Ok(serde_json::json!({"chunks":out,"scope":"Program/group child chunks; opaque private data and zone internals are not decoded by this inventory", "presence_is_not_activation":true}))
    }).map_err(|_|anyhow::anyhow!("Malformed source inventory"))?
}

/// Program `index` of `path`, from the on-disk cache when it is current.
fn read_inner(path: &Path, index: u32) -> Result<Instrument> {
    if crate::creator::is_native(path) { return crate::creator::read_native(path); }
    let path = path.canonicalize()?;
    let dir = crate::cache::dir();
    if let Some(i) = dir.as_deref().and_then(|dir| crate::cache::load(dir, &path, index)) {
        return Ok(i);
    }
    let instrument = parse(path, index)?;
    if let Some(dir) = &dir { crate::cache::store(dir, &instrument.path, index, &instrument); }
    Ok(instrument)
}

fn parse(path: PathBuf, index: u32) -> Result<Instrument> {
    let c = chunks(&path).context("Container decoding")?;
    let p = if let Some(p)=c.find_first(0x28) {ensure!(index==0,"NKI has only one instrument");Program::try_from(p)?}else{multi_programs(&c)?.1.into_iter().find(|(id,_)|*id==index).context("Multi program not found")?.1};
    let mut warnings = Vec::new();
    let program = p.params().context("Program parameters")?;
    let table = c.0.iter().find(|c| c.id == 0x4b).map(FNTableImpl::try_from).transpose().context("Sample file table")?.map(|f| f.sample_filetable)
        .or(c.0.iter().find(|c| c.id == 0x3d).map(FileNameListPreK51::try_from).transpose().context("Legacy file table")?.map(|f| f.sample_filetable))
        .context("Missing Kontakt sample file table")?;
    let gl = GroupList::try_from(p.0.find_first(0x33).context("Missing group list")?).context("Group list")?;
    ensure!(gl.groups.len() <= crate::engine::MAX_GROUPS, "Too many groups (Kontakt allows {})", crate::engine::MAX_GROUPS);
    let mut groups = Vec::new();
    for g in &gl.groups {
        let v = g.params().with_context(|| format!("Group {} version {:x}", groups.len(),g.0.version))?;
        ensure!(v.volume.is_finite() && v.pan.is_finite() && v.tune.is_finite() && v.tune > 0.0, "Invalid group gain/tuning");
        if !v.start_criteria.items.is_empty() { warnings.push(format!("{}: native group start conditions are not implemented", v.name)); }
        if v.release_trigger_note_monophonic {warnings.push("Release-trigger note monophony is not imported".into());}
        let modulation = match crate::modulation::read_group(g) {
            Ok(modulation) => modulation,
            Err(e) => {
                warnings.push(format!("{}: modulation not imported: {e:#}", v.name));
                Default::default()
            }
        };
        warnings.extend(modulation.warnings);
        let fx = match g.insert_fx().map_err(anyhow::Error::from).and_then(|a| crate::fx::Chain::from_array(&a)) {
            Ok(fx) => fx,
            Err(e) => {
                warnings.push(format!("{}: group effects not imported: {e:#}", v.name));
                Default::default()
            }
        };
        warnings.extend(crate::engine::filter::unsupported(&fx));
        // Gain and tuning are linear ratios (see audits/MODULATION.md).
        groups.push(Group {
            name: v.name,
            gain: v.volume,
            pan: v.pan,
            tune: v.tune as f64,
            key_tracking: v.key_tracking,
            reverse: v.reverse,
            release_trigger: v.release_trigger,
            release_counter_ms: v.rls_trig_counter,
            muted: v.muted,
            channel: v.midi_channel,
            soloed: v.soloed,
            volume_env: modulation.volume_env,
            flex_env: modulation.flex_env,
            mods: modulation.mods,
            modulators: modulation.modulators,
            envelopes: modulation.envelopes,
            fx,
            voice_group: u32::try_from(v.voice_group_index).ok(),
            interp_quality: v.interp_quality,
        });
    }
    let mut scripts = Vec::new();
    let mut script_state = Vec::new();
    for (slot, c) in p.0.children.iter().filter(|c| c.id == 6).enumerate() {
        let s = BParScript::try_from(c)?.params().context("Script parameters")?;
        if !s.bypass && let Some(text) = script_source(&path, slot, &s, &mut warnings) { scripts.push(text); script_state.push(crate::ksp::saved_persistence(&s.persistent)); }
    }
    warnings.push("Modulation: the first volume AHDSR and flex envelopes shape each voice, internal pitch AHDSRs drive voice pitch, and velocity, key, CC, pitch bend and aftertouch drive supported volume, pitch, sample-start, envelope-time and group-effect targets; LFOs, additional volume envelopes, flexible pitch envelopes, external inversion, unsupported effect targets and other modulator parameters are not applied".into());
    let parent = path.parent().context("Instrument has no parent")?;
    let root = path.ancestors().find(|p| p.join("Samples").is_dir()).unwrap_or(parent);
    let mut resolver = Resolver::new(root);
    let mut dependency_paths = vec![path.clone(), root.to_path_buf()];
    // Include absent resource paths too: installing them invalidates the entry.
    if let Some(container) = resource_container(&c)? {
        dependency_paths.push(parent.join(container.replace('\\', "/")));
    }
    dependency_paths.extend(library_metadata(&path));
    let mut missing_samples = Vec::new();
    let (mut names, mut order, mut zone_ids) = (HashMap::new(), Vec::new(), Vec::new());
    let data = &p.0.find_first(0x34).context("Missing zone list")?.data;
    let mut r = Cursor::new(data);
    let count = u32le(&mut r)? as usize;
    ensure!(count <= 1_000_000 && count <= data.len() / 8, "Invalid zone count");
    let mut zones = Vec::with_capacity(count);
    for _ in 0..count {
        // Group ownership precedes each structured zone.
        let group = u32le(&mut r)? as usize;
        ensure!(group < groups.len(), "Zone refers to nonexistent group {group}");
        let so = StructuredObject::read(&mut r).with_context(||format!("Zone {} structure", zones.len()))?;
        let mut z = Cursor::new(&so.public_data);
        let start = i32le(&mut z)?;
        let end = i32le(&mut z)?;
        // -1 is the only negative value stored locally: no range.
        let start_mod = match i32le(&mut z)? {
            -1 => None,
            frames => Some(u32::try_from(frames).context("Invalid sample-start modulation range")?),
        };
        let lv = i16le(&mut z)?; let hv = i16le(&mut z)?;
        let lk = i16le(&mut z)?; let hk = i16le(&mut z)?;
        // Crossfade widths: low/high velocity, then low/high key.
        let fades = [i16le(&mut z)?,i16le(&mut z)?,i16le(&mut z)?,i16le(&mut z)?];
        let root = i16le(&mut z)?;
        let gain = f32le(&mut z)?; let pan = f32le(&mut z)?; let tune = f32le(&mut z)?;
        // Six bytes, 00 01 ff ff ff ff in every local v0x9a zone; meaning unknown.
        if so.version >= 0x9a { let mut unknown=[0;6]; z.read_exact(&mut unknown)?; }
        let file_id = i32le(&mut z)?;
        if let std::collections::hash_map::Entry::Vacant(entry)=names.entry(file_id) {
            entry.insert(table.get(&(file_id as u32)).context("Zone sample ID is absent from file table")?.as_str());
            order.push(file_id);
        }
        zone_ids.push(file_id);
        ensure!([lk,hk,lv,hv,root].iter().chain(&fades).all(|v| (0..=127).contains(v)) && lk <= hk && lv <= hv, "Invalid zone mapping");
        let [fade_low_velocity, fade_high_velocity, fade_low_key, fade_high_key] = fades.map(|v| v as u8);
        ensure!(start >= 0 && end <= 0 && gain.is_finite() && pan.is_finite() && tune.is_finite() && tune > 0.0, "Invalid zone {} v{:x}: start {start}, end {end}, gain {gain}, pan {pan}, tune {tune}",zones.len(),so.version);
        let loops = so.find_first(0x39).map(LoopArray::try_from).transpose().with_context(|| format!("Zone {} loops",zones.len()))?;
        let mut loop_range = None;
        if let Some(loops) = loops {
            if loops.items.iter().filter(|l| l.mode != 0).count() > 1 {warnings.push("Multiple loops are parsed; playback currently uses the first supported loop".into());}
            for l in loops.items.into_iter().filter(|l| l.mode != 0) {
                if l.alternating_loop || l.loop_count != 0 || (l.loop_tuning - 1.0).abs() > 0.001 {
                    warnings.push("An unsupported alternating/counted/tuned loop was skipped".into()); continue;
                }
                ensure!(l.loop_start >= 0 && l.loop_length > 0, "Invalid sample loop");
                // Only mode 1 occurs locally; mode 2 as "until release" is unverified.
                loop_range = Some(Loop { start: l.loop_start as usize, end: l.loop_start as usize + l.loop_length as usize,
                    until_release: l.mode == 2, crossfade: l.x_fade_length.max(0) as usize });
                break;
            }
        }
        // Samples resolve in one batch after the zone list.
        zones.push(Zone { group, sample: PathBuf::new(), available: true, low_key: lk as u8, high_key: hk as u8, root: root as u8, low_velocity: lv as u8, high_velocity: hv as u8,
            fade_low_velocity, fade_high_velocity, fade_low_key, fade_high_key, start_mod,
            start: start as usize, end, gain: gain * program.volume, pan: (pan + program.pan).clamp(-1.0,1.0),
            tune: tune as f64 * program.tune as f64 * 2f64.powf(program.transpose as f64 / 12.0), loop_range });
    }
    let resolved = resolver.resolve_all(parent, &order.iter().map(|id| names[id]).collect::<Vec<_>>())?;
    let mut paths = HashMap::new();
    for (id, resolved) in order.iter().zip(resolved) {
        if resolved.is_none() { missing_samples.push(names[id].to_string()); }
        paths.insert(*id, resolved.ok_or_else(|| parent.join(names[id])));
    }
    for (zone, id) in zones.iter_mut().zip(&zone_ids) {
        let (Ok(path) | Err(path)) = &paths[id];
        (zone.sample, zone.available) = (path.clone(), paths[id].is_ok());
    }
    for (archive,(index,_)) in &resolver.archives {warnings.extend(index.issues.iter().map(|issue|format!("{}: {issue}",archive.display())));}
    if c.find_first(3).is_some(){warnings.push("Multi routing, master processing and multi scripts are not restored; parts use manual playback".into());}
    let fx = match crate::fx::ProgramFx::read(&p) {
        Ok(mut fx) => {
            fx.name_impulses(&other_files(&c)?);
            let container = resource_container(&c)?;
            fx.load_impulses(|name, max_frames| {
                // Kontakt maps any `<dir>/Resources/...` path into the resource container.
                let ir = match (resolver.resolve(parent, name)?, &container, name.find("Resources/")) {
                    (Some(ir), ..) => Some(ir),
                    (None, Some(nkr), Some(at)) => resolver.resolve(parent, &format!("{nkr}/{}", &name[at..]))?,
                    _ => None,
                };
                let ir = ir.context("file missing or its archive member is unreadable")?;
                dependency_paths.push(ir.clone());
                crate::audio::decode(&ir, max_frames)
            });
            warnings.extend(fx.warnings());
            fx
        }
        Err(e) => { warnings.push(format!("Effects were not imported: {e:#}")); Default::default() }
    };
    if !resolver.undownloaded.is_empty() {
        let mounts = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        warnings.push(zero_read_warning(resolver.undownloaded.len(), mount_of(&mounts, &resolver.root.canonicalize().unwrap_or_else(|_| resolver.root.clone()))));
    }
    warnings.sort(); warnings.dedup(); missing_samples.sort(); missing_samples.dedup();
    let (voice_limit, voice_groups) = match p.0.find_first(0x32).map(|c| voice_groups(&c.data)).transpose() {
        Ok(v) => v.map_or((None, Vec::new()), |(limit, groups)| (Some(limit), groups)),
        Err(e) => { warnings.push(format!("Voice groups ignored: {e:#}")); (None, Vec::new()) }
    };
    dependency_paths.extend(zones.iter().map(|z| z.sample.clone()));
    dependency_paths.extend(resolver.dependencies.iter().cloned());
    // Virtual members depend on their container, once per archive, not on
    // thousands of nonexistent filesystem paths underneath it.
    let dependency_paths = dependency_paths.into_iter().map(|p| {
        archive_member_where(&p, |a| resolver.archives.contains_key(a.as_os_str()))
            .map_or(p.clone(), |(archive, _)| archive)
    });
    let dependencies = crate::cache::dependencies(dependency_paths);
    Ok(Instrument {
        path,
        name: program.name,
        groups,
        zones,
        warnings,
        missing_samples,
        scripts,
        voice_limit,
        voice_groups,
        fx,
        script_state,
        kontakt_sample_bytes: program.num_bytes_samples_total,
        kontakt_preload: program.dfd_channel_preload_size,
        dependencies,
    })
}

/// (mount point, fs type, source) of the deepest mount containing `path`, from mountinfo text.
fn mount_of(mountinfo: &str, path: &Path) -> Option<(PathBuf, String, String)> {
    let unescape = |s: &str| s.replace("\\040", " ").replace("\\011", "\t").replace("\\134", "\\");
    mountinfo.lines().filter_map(|line| {
        let (left, right) = line.split_once(" - ")?;
        let point = PathBuf::from(unescape(left.split(' ').nth(4)?));
        let mut right = right.split(' ');
        Some((point, right.next()?.to_string(), unescape(right.next()?)))
    }).filter(|(point, ..)| path.starts_with(point)).max_by_key(|(point, ..)| point.as_os_str().len())
}

/// The in-kernel `ntfs` driver (Linux 7.x) rejects the $DATA extents Windows stores in
/// extension MFT records of heavily fragmented files ("Failed to load full runlist" in
/// dmesg) and reads everything past the base extent as zeros. ntfs3 reads them. See audits/NTFS.md.
fn zero_read_warning(samples: usize, mount: Option<(PathBuf, String, String)>) -> String {
    match mount {
        Some((point, fs, source)) if fs == "ntfs" => format!(
            "{samples} samples read back as zeros. The files are likely intact: the kernel `ntfs` driver on {} drops the extents of large fragmented files (dmesg: \"Failed to load full runlist\"). Remount with ntfs3: sudo umount '{1}' && sudo mount -t ntfs3 -o uid=$(id -u),gid=$(id -g),umask=0002,noatime {source} '{1}' (or set the type to ntfs3 in /etc/fstab)",
            source, point.display()),
        _ => format!("{samples} samples read back as zeros from disk: the library files are incomplete (an interrupted download leaves the rest zero-filled). Re-download the affected archives"),
    }
}

/// VoiceGroups chunk (0x32, v0x60): program limit, a 128-bit presence mask, then one
/// limit per present voice group. Layout verified against local Afflatus presets.
fn voice_groups(data: &[u8]) -> Result<(VoiceLimit, Vec<Option<VoiceLimit>>)> {
    fn limit(r: &mut Cursor<&[u8]>) -> Result<VoiceLimit> {
        let mut flag = [0; 3];
        r.read_exact(&mut flag)?;
        ensure!(flag == [0, 0x60, 0], "Unsupported voice limit version");
        let chars = u32le(r)? as usize;
        ensure!(chars <= 1024, "Voice group name too long");
        r.set_position(r.position() + 2 * chars as u64);
        let kill_mode = i16le(r)?;
        let mut prefer = [0];
        r.read_exact(&mut prefer)?;
        Ok(VoiceLimit { kill_mode, prefer_released: prefer[0] != 0, max_voices: i32le(r)?.max(1) as u32,
            fade_ms: i32le(r)?.max(0) as u32, exclusion_group: i32le(r)? })
    }
    let mut r = Cursor::new(data);
    let program = limit(&mut r)?;
    let mut mask = [0; 16];
    r.read_exact(&mut mask)?;
    let groups = (0..128).map(|i| (mask[i / 8] & (1 << (i % 8)) != 0).then(|| limit(&mut r)).transpose()).collect::<Result<Vec<_>>>()?;
    Ok((program, groups))
}

pub struct Resolver {
    root: PathBuf,
    dependencies: std::collections::HashSet<PathBuf>,
    index: Option<HashMap<String, Vec<PathBuf>>>,
    /// Lazily indexed archives with an open handle for member headers.
    archives: HashMap<OsString, (ni_file::nkr::Archive, File)>,
    /// Canonical path of each archive: canonicalizing per member costs a
    /// path walk per component, which dominated large imports.
    canonical: HashMap<OsString, PathBuf>,
    /// These maps key paths by their bytes: hashing a `Path` walks its
    /// components, one hasher write each, and every sample looks them up.
    is_file: HashMap<OsString, bool>,
    /// Archive members whose header reads back as zeros: an interrupted download, or
    /// the kernel `ntfs` driver dropping the extents of a fragmented archive.
    pub undownloaded: std::collections::HashSet<PathBuf>,
}
impl Resolver {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.into(),
            dependencies: Default::default(),
            index: None,
            archives: HashMap::new(),
            canonical: HashMap::new(),
            is_file: HashMap::new(),
            undownloaded: Default::default(),
        }
    }
    pub fn resolve(&mut self, parent: &Path, name: &str) -> Result<Option<PathBuf>> {
        let name = name.replace('\\', "/");
        let direct = parent.join(&name);
        self.dependencies.insert(direct.clone());
        // Archive members first: a path inside an archive file is never a file itself.
        if let Some((archive, member)) = self.archive_member(&direct) {
            if !self.archives.contains_key(archive.as_os_str()) {
                let mut file = File::open(&archive)?;
                let index = ni_file::nkr::Archive::read_index(&mut file).with_context(|| format!("Archive {}", archive.display()))?;
                self.archives.insert(archive.clone().into(), (index, file));
            }
            let (index, file) = &self.archives[archive.as_os_str()];
            let entry = index.member(crate::audio::FileAt { file, pos: 0 }, &member)?;
            return self.member_path(&direct, &archive, entry);
        }
        if direct.is_file() { return Ok(Some(direct.canonicalize()?)); }
        let basename = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
        let index = self.index.get_or_insert_with(|| {
            let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for e in walkdir::WalkDir::new(&self.root).follow_links(false).into_iter().filter_map(Result::ok) {
                if e.file_type().is_dir() { self.dependencies.insert(e.into_path()); }
                else if e.file_type().is_file() { index.entry(e.file_name().to_string_lossy().to_lowercase()).or_default().push(e.into_path()); }
            }
            index
        });
        let Some(matches) = index.get(&basename) else { return Ok(None); };
        if matches.len() == 1 { return Ok(matches.first().cloned()); }
        // Match the longest directory suffix; never silently pick a duplicate mic/sample.
        let parts: Vec<_> = name.split('/').filter(|s| !s.is_empty() && *s != "..").collect();
        for n in (2..=parts.len()).rev() {
            let suffix = parts[parts.len()-n..].join("/").to_lowercase();
            let candidates: Vec<_> = matches.iter().filter(|p| p.to_string_lossy().to_lowercase().ends_with(&suffix)).collect();
            if candidates.len() == 1 { return Ok(Some(candidates[0].clone())); }
        }
        bail!("Ambiguous sample {name}: {} matching files", matches.len())
    }
    /// [`Resolver::resolve`] for every name, validating archive member
    /// headers in parallel first: each is a random read, which a cold disk
    /// serves far faster several at a time.
    pub fn resolve_all(&mut self, parent: &Path, names: &[&str]) -> Result<Vec<Option<PathBuf>>> {
        let (mut resolved, mut jobs, mut loose) = (vec![None; names.len()], Vec::new(), Vec::new());
        for (i, name) in names.iter().enumerate() {
            let direct = parent.join(name.replace('\\', "/"));
            let Some((archive, member)) = self.archive_member(&direct) else { loose.push(i); continue };
            if !self.archives.contains_key(archive.as_os_str()) {
                let mut file = File::open(&archive)?;
                let index = ni_file::nkr::Archive::read_index(&mut file).with_context(|| format!("Archive {}", archive.display()))?;
                self.archives.insert(archive.clone().into(), (index, file));
            }
            jobs.push((i, direct, archive, member));
        }
        let archives = &self.archives;
        let checked = crate::engine::parallel(jobs, |_: &mut (), (i, direct, archive, member)| {
            let (index, file) = &archives[archive.as_os_str()];
            let entry = index.member(crate::audio::FileAt { file, pos: 0 }, &member);
            (i, direct, archive, entry)
        });
        for (i, direct, archive, entry) in checked {
            resolved[i] = self.member_path(&direct, &archive, entry?)?;
        }
        for i in loose {
            resolved[i] = self.resolve(parent, names[i])?;
        }
        Ok(resolved)
    }

    /// The path of an archive member `direct` names, once its header is read.
    fn member_path(&mut self, direct: &Path, archive: &Path, entry: Option<ni_file::nkr::Entry>) -> Result<Option<PathBuf>> {
        match entry {
            Some(entry) if entry.valid => {
                if !self.canonical.contains_key(archive.as_os_str()) {
                    self.canonical.insert(archive.into(), archive.canonicalize()?);
                }
                Ok(Some(self.canonical[archive.as_os_str()].join(&entry.name)))
            }
            Some(entry) if entry.issue == Some("Zero-filled NKX member header") => { self.undownloaded.insert(direct.to_path_buf()); Ok(None) }
            _ => Ok(None),
        }
    }

    /// [`archive_member`], remembering which archive paths are files: one
    /// stat per archive instead of one per member.
    fn archive_member(&mut self, path: &Path) -> Option<(PathBuf, String)> {
        archive_member_where(path, |parent| match self.is_file.get(parent.as_os_str()) {
            Some(&is_file) => is_file,
            None => *self.is_file.entry(parent.into()).or_insert(parent.is_file()),
        })
    }
}

pub fn catalog(root: &Path) -> Result<Vec<PathBuf>> {Ok(presets(root)?.into_iter().filter(|p|!is_multi(p)).collect())}
/// The extension of a rack KONTRA saved itself; see `plugin::SavedMulti`.
pub const SAVED_MULTI: &str = "kontra-multi";
/// The extension racks had before the rename; still opened.
pub const OLD_SAVED_MULTI: &str = "kontakto-multi";
pub fn is_saved_multi(path:&Path)->bool {path.extension().is_some_and(|x|x.eq_ignore_ascii_case(SAVED_MULTI) || x.eq_ignore_ascii_case(OLD_SAVED_MULTI))}
pub fn is_multi(path:&Path)->bool {is_saved_multi(path) || path.extension().is_some_and(|x|x.eq_ignore_ascii_case("nkm"))}
pub fn presets(root: &Path) -> Result<Vec<PathBuf>> {
    if root.is_file() && (root.extension().is_some_and(|x| x.eq_ignore_ascii_case("nki")) || is_multi(root)) {
        return Ok(vec![root.to_owned()]);
    }
    ensure!(root.is_dir(), "Library folder does not exist: {}", root.display());
    let mut files = Vec::new();
    for e in walkdir::WalkDir::new(root).follow_links(false) {
        let e = e?;
        if e.file_type().is_file() && (crate::creator::is_instrument(e.path()) || is_multi(e.path())) { files.push(e.into_path()); }
    }
    files.sort(); Ok(files)
}
fn u32le(r: &mut impl Read) -> Result<u32> { let mut b=[0;4]; r.read_exact(&mut b)?; Ok(u32::from_le_bytes(b)) }
fn i32le(r: &mut impl Read) -> Result<i32> { Ok(u32le(r)? as i32) }
fn i16le(r: &mut impl Read) -> Result<i16> { let mut b=[0;2]; r.read_exact(&mut b)?; Ok(i16::from_le_bytes(b)) }
fn f32le(r: &mut impl Read) -> Result<f32> { Ok(f32::from_bits(u32le(r)?)) }

/// A virtual archive/member path is never extracted onto the library filesystem.
pub fn archive_member(path: &Path) -> Option<(PathBuf, String)> {
    archive_member_where(path, Path::is_file)
}

/// [`archive_member`], asking `is_file` of each archive-named ancestor.
pub fn archive_member_where(path: &Path, mut is_file: impl FnMut(&Path) -> bool) -> Option<(PathBuf, String)> {
    for parent in path.ancestors().skip(1) {
        if parent.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr")) && is_file(parent) {
            return Some((parent.to_path_buf(),path.strip_prefix(parent).ok()?.to_string_lossy().replace('\\',"/")));
        }
    }
    None
}

impl Instrument {
    pub fn first_playable_group(&self)->Option<usize> {
        (0..self.groups.len()).find(|g| !self.groups[*g].muted && !self.groups[*g].release_trigger && {
            let mut zones=self.zones.iter().filter(|z|z.group==*g).peekable();
            zones.peek().is_some() && zones.all(|z|z.available)
        })
    }
}

/// Read only the access fields supplied with this library; never log or persist them.
pub(crate) fn library_metadata(path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for parent in path.ancestors().skip(1) {
        paths.push(parent.into());
        if let Ok(entries) = std::fs::read_dir(parent) {
            paths.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
                    }),
            );
        }
        if parent.join("Samples").is_dir() {
            break;
        }
    }
    paths.sort();
    paths
}

#[cfg(test)]
mod preset_tests {
    #[test]
    fn rooted_snapshot_paths_stay_inside_the_library() {
        use super::snapshot_rooted_path;
        assert!(snapshot_rooted_path("../Samples/relative.ncw").unwrap().is_none());
        assert_eq!(snapshot_rooted_path("/Samples/../Resources/authored.ncw").unwrap(), Some("Samples/../Resources/authored.ncw".into()));
        for path in ["/../outside.ncw", "/Samples/../../outside.ncw", "/Samples\\..\\..\\outside.ncw", "//machine/absolute"] {
            assert!(snapshot_rooted_path(path).is_err(), "{path}");
        }
    }

    #[test]
    fn zero_filled_presets_fail_before_entering_the_container_parser() {
        let path =
            std::env::temp_dir().join(format!("kontra-zero-preset-{}.nki", std::process::id()));
        std::fs::write(&path, [0; 32]).unwrap();
        let err = super::chunks(&path).err().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(err.to_string().contains("only zero bytes"), "{err:#}");
    }

    #[test]
    fn catalog_separates_presets_from_resources() {
        let root=std::env::temp_dir().join(format!("kontakto-presets-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&root).unwrap();
        for name in ["Piano.NKI","Ensemble.NKM","C4.wav","C5.ncw","Resource.nkr","Library.nicnt"]{std::fs::write(root.join(name),[]).unwrap();}
        assert_eq!(super::presets(&root.join("Piano.NKI")).unwrap(), [root.join("Piano.NKI")]);assert!(super::presets(&root.join("C4.wav")).is_err());let all=super::presets(&root).unwrap();assert_eq!(all.len(),2);assert_eq!(all.iter().filter(|p|super::is_multi(p)).count(),1);assert_eq!(super::catalog(&root).unwrap().len(),1);std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn zero_reads_on_kernel_ntfs_name_the_ntfs3_remount() {
        let info = "22 1 0:21 / / rw - btrfs /dev/nvme1n1p2 rw\n\
                    99 22 259:7 / /test-volumes/MAIN\\040STORAGE rw,noatime - ntfs /dev/nvme0n1p1 rw,errors=continue\n\
                    98 22 259:8 / /test-volumes/MAIN rw - ntfs3 /dev/sda1 rw\n";
        let lib = std::path::Path::new("/test-volumes/MAIN STORAGE/Libraries/Kontakt/Areia");
        let m = super::mount_of(info, lib).unwrap();
        assert_eq!((m.0.to_str().unwrap(), m.1.as_str(), m.2.as_str()), ("/test-volumes/MAIN STORAGE", "ntfs", "/dev/nvme0n1p1"));
        let w = super::zero_read_warning(3, Some(m));
        assert!(w.contains("mount -t ntfs3") && w.contains("/dev/nvme0n1p1 '/test-volumes/MAIN STORAGE'"), "{w}");
        assert_eq!(super::mount_of(info, std::path::Path::new("/test-volumes/MAIN/x")).unwrap().1, "ntfs3");
        assert!(!super::zero_read_warning(3, super::mount_of(info, std::path::Path::new("/home/x"))).contains("ntfs3"));
    }
    /// A zero-filled read (the NTFS runlist failure) is a clean error, not a panic.
    #[test]
    fn zero_filled_container_is_an_error() {
        assert!(ni_file::NIFile::read(std::io::Cursor::new(vec![0u8; 4096])).is_err());
    }
    #[test]
    fn script_text_decodes_every_kontakt_encoding() {
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("on init\nend on".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(super::script_text(&utf16), "on init\nend on");
        let bare: Vec<u8> = "on init".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(super::script_text(&bare), "on init");
        assert_eq!(super::script_text(b"\xEF\xBB\xBFon init"), "on init");
        assert_eq!(super::script_text(b"{ caf\xE9 }"), "{ caf\u{e9} }");
    }
    #[test]
    fn linked_scripts_resolve_windows_paths_without_case() {
        let root = std::env::temp_dir().join(format!("kontakto-linked-{}", std::process::id()));
        let dir = root.join("resources").join("Scripts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::write(dir.join("Main.txt"), b"on init\nend on").unwrap();
        let nki = root.join("Instruments").join("A.nki");
        let found = crate::resources::linked_script(&nki, "C:\\Libs\\X\\Resources\\scripts\\Main.txt").unwrap();
        assert_eq!(found.as_deref(), Some(&b"on init\nend on"[..]));
        assert_eq!(crate::resources::linked_script(&nki, "Missing.txt").unwrap(), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
