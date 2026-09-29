use anyhow::{Context, Result, bail, ensure};
use ni_file::{NIFile, kontakt::{KontaktChunks, StructuredObject, objects::{Program, FNTableImpl, FileNameListPreK51, GroupList, BParScript, LoopArray}}, nis::schema::{Repository, NISObject}};
use serde::Serialize;
use std::{collections::HashMap, fs::File, io::{Cursor, Read}, path::{Path, PathBuf}};

pub const LIBRARY_ROOT: &str = "/mnt/MAIN_STORAGE/Libraries/Kontakt";

#[derive(Debug, Clone, Serialize)]
pub struct Group {
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub tune: f64,
    pub key_tracking: bool,
    pub reverse: bool,
    pub release_trigger: bool,
    pub muted: bool,
    pub channel: i16,
}

#[derive(Debug, Clone, Serialize)]
pub struct Loop { pub start: usize, pub end: usize, pub until_release: bool, pub crossfade: usize }

#[derive(Debug, Clone, Serialize)]
pub struct Zone {
    pub group: usize,
    pub sample: PathBuf,
    pub available: bool,
    pub low_key: u8, pub high_key: u8, pub root: u8,
    pub low_velocity: u8, pub high_velocity: u8,
    pub start: usize, pub end: i32,
    pub gain: f32, pub pan: f32, pub tune: f64,
    pub loop_range: Option<Loop>,
}

#[derive(Debug, Serialize)]
pub struct Instrument {
    pub path: PathBuf,
    pub name: String,
    pub groups: Vec<Group>,
    pub zones: Vec<Zone>,
    pub warnings: Vec<String>,
    pub missing_samples: Vec<String>,
    #[serde(skip)]
    pub scripts: Vec<String>,
    /// Persistent variable values saved with each script, parallel to `scripts`.
    #[serde(skip)]
    pub script_state: Vec<crate::ksp::Persisted>,
}

/// ni-file is a research parser with panic paths. Keep those off the host thread.
pub fn read(path: &Path) -> Result<Instrument> {
    std::panic::catch_unwind(|| read_inner(path,0))
        .map_err(|_| anyhow::anyhow!("Unsupported or malformed Kontakt structure in {}", path.display()))?
        .with_context(|| format!("Reading {}", path.display()))
}

fn chunks(path: &Path) -> Result<KontaktChunks> {
    ensure!(path.metadata()?.len() <= 128 * 1024 * 1024, "Instrument container exceeds 128 MiB import limit");
    let bytes = match NIFile::read(File::open(path)?).context("NIS/NKS container headers")? {
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
                let key = library_key(path)?;
                let enc = p.encryption_item_with_key(key.as_ref()).context("NIS preset subtree")?;
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

/// Inspect scripts without sample resolution, so compatibility rescans do not reopen sample archives.
pub fn script_inventory(path:&Path)->Result<serde_json::Value>{
    std::panic::catch_unwind(||->Result<_>{
        let c=chunks(path)?;let programs=if let Some(p)=c.find_first(0x28){vec![(0,Program::try_from(p)?)]}else{multi_programs(&c)?.1};let mut report=Vec::new();
        for (program,p) in programs {
            let groups=GroupList::try_from(p.0.find_first(0x33).context("Missing group list")?)?.groups.len();let mut scripts=Vec::new();let mut host=crate::ksp::HostState::default();
            for child in &p.0.children {if child.id==6 {let s=BParScript::try_from(child)?.params()?;if !s.bypass && let Some(source)=s.text.filter(|s|!s.trim().is_empty()){scripts.push(crate::ksp::inspect(&source,groups,&mut host));}}}
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

fn read_inner(path: &Path, index:u32) -> Result<Instrument> {
    let path = path.canonicalize()?;
    let c = chunks(&path).context("Container decoding")?;
    let p = if let Some(p)=c.find_first(0x28) {ensure!(index==0,"NKI has only one instrument");Program::try_from(p)?}else{multi_programs(&c)?.1.into_iter().find(|(id,_)|*id==index).context("Multi program not found")?.1};
    let mut warnings = Vec::new();
    let program = p.params().context("Program parameters")?;
    let table = c.0.iter().find(|c| c.id == 0x4b).map(FNTableImpl::try_from).transpose().context("Sample file table")?.map(|f| f.sample_filetable)
        .or(c.0.iter().find(|c| c.id == 0x3d).map(FileNameListPreK51::try_from).transpose().context("Legacy file table")?.map(|f| f.sample_filetable))
        .context("Missing Kontakt sample file table")?;
    let gl = GroupList::try_from(p.0.find_first(0x33).context("Missing group list")?).context("Group list")?;
    ensure!(gl.groups.len() <= 16384, "Too many groups");
    let mut groups = Vec::new();
    for g in &gl.groups {
        let v = g.params().with_context(|| format!("Group {} version {:x}", groups.len(),g.0.version))?;
        ensure!(v.volume.is_finite() && v.pan.is_finite() && v.tune.is_finite() && v.tune > 0.0, "Invalid group gain/tuning");
        if !v.start_criteria.items.is_empty() { warnings.push(format!("{}: native group start conditions are not implemented", v.name)); }
        if v.soloed {warnings.push("Group solo flags are not applied".into());}
        if v.release_trigger_note_monophonic || v.rls_trig_counter!=0 {warnings.push("Release-trigger monophony/counter behavior is not imported".into());}
        if v.voice_group_index>=0 {warnings.push("Voice-group allocation/choke settings are not imported".into());}
        // Kontakt stores gain and tuning as linear ratios, despite the parser's tune comment.
        groups.push(Group { name: v.name, gain: v.volume, pan: v.pan, tune: v.tune as f64,
            key_tracking: v.key_tracking, reverse: v.reverse, release_trigger: v.release_trigger,
            muted: v.muted, channel: v.midi_channel });
    }
    let mut scripts = Vec::new();
    let mut script_state = Vec::new();
    for c in &p.0.children {
        if c.id == 6 {
            let s = BParScript::try_from(c)?.params().context("Script parameters")?;
            if !s.bypass && let Some(text) = s.text.filter(|s| !s.trim().is_empty()) { scripts.push(text); script_state.push(crate::ksp::saved_persistence(&s.persistent)); }
        }
    }
    if !scripts.is_empty() { warnings.push(format!("{} active KSP script(s): manual group playback only; scripted legato and round robin are not emulated; interface initialization is a preview only", scripts.len())); }
    warnings.push("Kontakt effects and modulation are not imported; playback uses the sampler's envelope".into());
    let parent = path.parent().context("Instrument has no parent")?;
    let root = path.ancestors().find(|p| p.join("Samples").is_dir()).unwrap_or(parent);
    let mut resolver = Resolver::new(root);
    let mut missing_samples = Vec::new();
    let mut paths = HashMap::new();
    let mut unavailable = std::collections::HashSet::new();
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
        let start_mod = i32le(&mut z)?;
        if start_mod!=0 && !warnings.iter().any(|s|s=="Zone sample-start modulation is not imported"){warnings.push("Zone sample-start modulation is not imported".into());}
        let lv = i16le(&mut z)?; let hv = i16le(&mut z)?;
        let lk = i16le(&mut z)?; let hk = i16le(&mut z)?;
        let fades = [i16le(&mut z)?,i16le(&mut z)?,i16le(&mut z)?,i16le(&mut z)?];
        if fades.iter().any(|v| *v != 0) && !warnings.iter().any(|s| s.starts_with("Zone crossfades")) { warnings.push("Zone crossfades are not imported".into()); }
        let root = i16le(&mut z)?;
        let gain = f32le(&mut z)?; let pan = f32le(&mut z)?; let tune = f32le(&mut z)?;
        if so.version >= 0x9a { let mut flags=[0;6]; z.read_exact(&mut flags)?; }
        let file_id = i32le(&mut z)?;
        if let std::collections::hash_map::Entry::Vacant(entry)=paths.entry(file_id) {
            let name=table.get(&(file_id as u32)).context("Zone sample ID is absent from file table")?;
            let resolved=resolver.resolve(parent,name)?;
            if resolved.is_none(){missing_samples.push(name.clone());unavailable.insert(file_id);}
            entry.insert(resolved.unwrap_or_else(||parent.join(name)));
        }
        ensure!([lk,hk,lv,hv,root].iter().all(|v| (0..=127).contains(v)) && lk <= hk && lv <= hv, "Invalid zone mapping");
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
                loop_range = Some(Loop { start: l.loop_start as usize, end: l.loop_start as usize + l.loop_length as usize,
                    until_release: l.mode == 2, crossfade: l.x_fade_length.max(0) as usize });
                break;
            }
        }
        zones.push(Zone { group, sample: paths.get(&file_id).with_context(||format!("Zone {} v{:x}: sample id {} missing from file table ({} entries); public {:?}",zones.len(),so.version,file_id,paths.len(),&so.public_data[..so.public_data.len().min(64)]))?.clone(),
            available: !unavailable.contains(&file_id), low_key: lk as u8, high_key: hk as u8, root: root as u8, low_velocity: lv as u8, high_velocity: hv as u8,
            start: start as usize, end, gain: gain * program.volume, pan: (pan + program.pan).clamp(-1.0,1.0),
            tune: tune as f64 * program.tune as f64 * 2f64.powf(program.transpose as f64 / 12.0), loop_range });
    }
    for (archive,index) in &resolver.archives {warnings.extend(index.issues.iter().map(|issue|format!("{}: {issue}",archive.display())));}
    if c.find_first(3).is_some(){warnings.push("Multi routing, master processing and multi scripts are not restored; parts use manual playback".into());}
    warnings.sort(); warnings.dedup(); missing_samples.sort(); missing_samples.dedup();
    Ok(Instrument { path, name: program.name, groups, zones, warnings, missing_samples, scripts, script_state })
}

pub struct Resolver { root: PathBuf, index: Option<HashMap<String, Vec<PathBuf>>>, archives: HashMap<PathBuf,ni_file::nkr::Archive> }
impl Resolver {
    pub fn new(root: &Path) -> Self { Self { root: root.into(), index: None, archives: HashMap::new() } }
    pub fn resolve(&mut self, parent: &Path, name: &str) -> Result<Option<PathBuf>> {
        let name = name.replace('\\', "/");
        let direct = parent.join(&name);
        if direct.is_file() { return Ok(Some(direct.canonicalize()?)); }
        if let Some((archive, member)) = archive_member(&direct) {
            if !self.archives.contains_key(&archive) {
                self.archives.insert(archive.clone(), ni_file::nkr::Archive::read(File::open(&archive)?).with_context(|| format!("Archive {}", archive.display()))?);
            }
            if let Some(entry) = self.archives[&archive].find(&member).filter(|e| e.valid) {
                return Ok(Some(archive.canonicalize()?.join(&entry.name)));
            }
            return Ok(None);
        }
        let basename = name.rsplit('/').next().unwrap_or(&name).to_lowercase();
        let index = self.index.get_or_insert_with(|| {
            let mut index: HashMap<String, Vec<PathBuf>> = HashMap::new();
            for e in walkdir::WalkDir::new(&self.root).follow_links(false).into_iter().filter_map(Result::ok).filter(|e| e.file_type().is_file()) {
                index.entry(e.file_name().to_string_lossy().to_lowercase()).or_default().push(e.into_path());
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
}

pub fn catalog(root: &Path) -> Result<Vec<PathBuf>> {Ok(presets(root)?.into_iter().filter(|p|!is_multi(p)).collect())}
pub fn is_multi(path:&Path)->bool {path.extension().is_some_and(|x|x.eq_ignore_ascii_case("nkm"))}
pub fn presets(root: &Path) -> Result<Vec<PathBuf>> {
    ensure!(root.is_dir(), "Library folder does not exist: {}", root.display());
    let mut files = Vec::new();
    for e in walkdir::WalkDir::new(root).follow_links(false) {
        let e = e?;
        if e.file_type().is_file() && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("nki") || x.eq_ignore_ascii_case("nkm")) { files.push(e.into_path()); }
    }
    files.sort(); Ok(files)
}
fn u32le(r: &mut impl Read) -> Result<u32> { let mut b=[0;4]; r.read_exact(&mut b)?; Ok(u32::from_le_bytes(b)) }
fn i32le(r: &mut impl Read) -> Result<i32> { Ok(u32le(r)? as i32) }
fn i16le(r: &mut impl Read) -> Result<i16> { let mut b=[0;2]; r.read_exact(&mut b)?; Ok(i16::from_le_bytes(b)) }
fn f32le(r: &mut impl Read) -> Result<f32> { Ok(f32::from_bits(u32le(r)?)) }

/// A virtual archive/member path is never extracted onto the library filesystem.
pub fn archive_member(path: &Path) -> Option<(PathBuf, String)> {
    for parent in path.ancestors().skip(1) {
        if parent.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkx") || e.eq_ignore_ascii_case("nkr")) && parent.is_file() {
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
pub(crate) fn library_key(path: &Path) -> Result<Option<ni_file::nis::LibraryKey>> {
    for parent in path.ancestors().skip(1) {
        for entry in std::fs::read_dir(parent)? {
            let file = entry?.path();
            if !file.extension().is_some_and(|e| e.eq_ignore_ascii_case("nicnt")) { continue; }
            // Product XML precedes artwork. Bound reads even for malformed containers.
            let mut bytes = Vec::new();
            File::open(&file)?.take(64 * 1024).read_to_end(&mut bytes)?;
            fn field<const N: usize>(bytes: &[u8], tag: &[u8]) -> Option<[u8; N]> {
                let start = bytes.windows(tag.len()).position(|w| w == tag)? + tag.len();
                let text = bytes.get(start..start + N * 2)?;
                let mut out = [0; N];
                for (dest, pair) in out.iter_mut().zip(text.chunks_exact(2)) {
                    *dest = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
                }
                (bytes.get(start + N * 2) == Some(&b'<')).then_some(out)
            }
            if let (Some(key), Some(iv)) = (field::<32>(&bytes, b"<JDX>"), field::<16>(&bytes, b"<HU>")) {
                return Ok(Some(ni_file::nis::LibraryKey::new(key, iv)));
            }
        }
        if parent.join("Samples").is_dir() { break; }
    }
    Ok(None)
}

#[cfg(test)]
mod preset_tests {
    #[test]
    fn catalog_separates_presets_from_resources() {
        let root=std::env::temp_dir().join(format!("kontakto-presets-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));std::fs::create_dir(&root).unwrap();
        for name in ["Piano.NKI","Ensemble.NKM","C4.wav","C5.ncw","Resource.nkr","Library.nicnt"]{std::fs::write(root.join(name),[]).unwrap();}
        let all=super::presets(&root).unwrap();assert_eq!(all.len(),2);assert_eq!(all.iter().filter(|p|super::is_multi(p)).count(),1);assert_eq!(super::catalog(&root).unwrap().len(),1);std::fs::remove_dir_all(root).unwrap();
    }
}
