//! Programs and samples inside an installed UFS bank. Namespaces use the native
//! format tables; recovered content access and decoded bytes stay in memory.

use crate::{
    AccessError,
    access::{self, Namespaces},
    crypto,
    ufs::{Directory, Member, Protection, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};

/// Recently opened banks, in memory only. Each load walked the bank's whole
/// record chain and re-derived its content key (Augmented Orchestra: 0.5 s
/// warm, 13 s cold), so every preset switch within a bank paid it again.
struct Recent {
    ufs: Arc<Ufs>,
    directory: Arc<Directory>,
    paths: Arc<HashMap<String, Option<usize>>>,
    content_key: Option<u64>,
}
static RECENT: Mutex<Vec<Recent>> = Mutex::new(Vec::new());
/// Large banks hold ~100k member records; keep the last few only.
const RECENT_BANKS: usize = 2;

fn recent_in<T>(
    cache: &Mutex<Vec<Recent>>,
    ufs: &Ufs,
    f: impl FnOnce(&mut Recent) -> T,
) -> Option<T> {
    let start = std::env::var_os("KONTRA_AUDIT_UFS_DIRECTORY").map(|_| std::time::Instant::now());
    let mut recent = cache.lock().unwrap_or_else(|e| e.into_inner());
    let wait_ns = start.map_or(0, |start| {
        start.elapsed().as_nanos().min(u64::MAX as u128) as u64
    });
    let work_start = start.map(|_| std::time::Instant::now());
    let result = recent.iter_mut().find(|r| r.ufs.same_bank(ufs)).map(f);
    if let Some(start) = work_start {
        eprintln!(
            "BANK_NAMESPACE_CACHE {}",
            serde_json::json!({"schema":1,"hit":result.is_some(),"lock_wait_ns":wait_ns,"locked_work_ns":start.elapsed().as_nanos().min(u64::MAX as u128) as u64,"cached_banks":recent.len()})
        );
    }
    result
}

/// An open bank. Deliberately not Debug: it holds access values.
#[derive(Clone)]
pub struct Bank {
    ufs: Arc<Ufs>,
    directory: Arc<Directory>,
    content_key: Option<u64>,
    program_namespace: Vec<u8>,
    /// Duplicate paths remain ambiguous, as in v1's ResourceIndex.
    paths: Arc<HashMap<String, Option<usize>>>,
}

/// Decode clear and protected programs through the native PasswordV2 path.
pub(crate) fn program_text(bytes: &[u8]) -> Result<String, AccessError> {
    let namespaces = Namespaces::native();
    crypto::decode_program_bytes(bytes, &namespaces.program)
        .map_err(|e| AccessError::Program(access::failure_reason(&e)))
}

fn program_paths(directory: &Directory) -> Vec<String> {
    let mut programs: Vec<String> = directory
        .files
        .iter()
        .filter_map(|m| m.path.clone())
        .filter(|p| p.to_ascii_lowercase().ends_with(".uvip"))
        .collect();
    programs.sort();
    programs
}

impl Bank {
    /// List program paths using directory metadata only, without preparing or reading payloads.
    pub fn catalog(path: &Path) -> Result<Vec<String>, AccessError> {
        Self::catalog_status(path).map(|(programs, _, _)| programs)
    }

    /// Directory-only catalog, declared access requirements and bank UUID; no payload reads.
    pub fn catalog_status(
        path: &Path,
    ) -> Result<(Vec<String>, Option<String>, [u8; 16]), AccessError> {
        let bank_error = |e| AccessError::Bank(access::failure_reason(&e));
        let ufs = Ufs::open(path).map_err(bank_error)?;
        let directory = ufs
            .decode_directory(&Namespaces::native().metadata)
            .map_err(bank_error)?;
        let programs = program_paths(&directory);
        let protected = directory
            .files
            .iter()
            .filter(|m| {
                m.path
                    .as_ref()
                    .is_some_and(|p| p.to_ascii_lowercase().ends_with(".uvip"))
                    && m.mode == Protection::Content
            })
            .count();
        let unknown = directory
            .files
            .iter()
            .filter(|m| {
                m.path
                    .as_ref()
                    .is_some_and(|p| p.to_ascii_lowercase().ends_with(".uvip"))
                    && matches!(m.mode, Protection::Unknown(_))
            })
            .count();
        let mut reasons = Vec::new();
        if protected > 0 {
            reasons.push(format!(
                "{protected} of {} presets need content access before they can load.",
                programs.len()
            ));
        }
        if unknown > 0 {
            reasons.push(format!(
                "{unknown} of {} presets use an unsupported protection mode.",
                programs.len()
            ));
        }
        Ok((
            programs,
            (!reasons.is_empty()).then(|| reasons.join(" ")),
            ufs.header.uuid,
        ))
    }

    /// Open a bank for programs, scripts and samples, preparing content access.
    pub fn open(path: &Path) -> Result<Self, AccessError> {
        Self::open_in(path, &RECENT)
    }

    fn open_in(path: &Path, cache: &Mutex<Vec<Recent>>) -> Result<Self, AccessError> {
        let mut bank = Self::open_metadata_in(path, cache)?;
        let content_error = |e| AccessError::Content(access::failure_reason(&e));
        let span = sampler_kontakt::audit::Span::new("uvi_content_setup");
        let cached = recent_in(cache, &bank.ufs, |r| r.content_key).flatten();
        // Only banks with encrypted members need a content state prepared.
        let content_key = if cached.is_some() {
            cached
        } else if bank
            .directory
            .files
            .iter()
            .any(|m| m.mode == Protection::Content)
        {
            Some(
                access::recover_content_key(path, &bank.ufs, &bank.directory)
                    .map_err(content_error)?,
            )
        } else {
            None
        };
        drop(span);
        recent_in(cache, &bank.ufs, |r| r.content_key = content_key);
        bank.content_key = content_key;
        Ok(bank)
    }

    /// Open directory metadata without preparing content access. Content-protected
    /// members remain unavailable; clear and metadata members can be read.
    pub fn open_metadata(path: &Path) -> Result<Self, AccessError> {
        Self::open_metadata_in(path, &RECENT)
    }

    fn open_metadata_in(path: &Path, cache: &Mutex<Vec<Recent>>) -> Result<Self, AccessError> {
        let bank_error = |e| AccessError::Bank(access::failure_reason(&e));
        let span = sampler_kontakt::audit::Span::new("uvi_ufs_header");
        let ufs = Ufs::open(path).map_err(bank_error)?;
        drop(span);
        let span = sampler_kontakt::audit::Span::new("uvi_directory_namespace");
        let namespaces = Namespaces::native();
        let program_namespace = namespaces.program;
        if let Some((ufs, directory, paths)) = recent_in(cache, &ufs, |r| {
            (r.ufs.clone(), r.directory.clone(), r.paths.clone())
        }) {
            return Ok(Self {
                ufs,
                directory,
                content_key: None,
                program_namespace,
                paths,
            });
        }
        let directory = ufs
            .decode_directory(&namespaces.metadata)
            .map_err(bank_error)?;
        drop(span);
        let map_start =
            std::env::var_os("KONTRA_AUDIT_UFS_DIRECTORY").map(|_| std::time::Instant::now());
        let mut paths = HashMap::with_capacity(directory.files.len());
        for (index, member) in directory.files.iter().enumerate() {
            if let Some(path) = &member.path {
                paths
                    .entry(path.to_ascii_lowercase())
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some(index));
            }
        }
        if let Some(start) = map_start {
            eprintln!(
                "BANK_NAMESPACE_INDEX {}",
                serde_json::json!({"schema":1,"casefold_map_ms":start.elapsed().as_secs_f64()*1000.,"path_entries":paths.len(),"ambiguous_paths":paths.values().filter(|index|index.is_none()).count(),"path_string_capacity_bytes":paths.keys().map(String::capacity).sum::<usize>(),"bucket_capacity":paths.capacity()})
            );
        }
        // Share the immutable namespace with all reopened readers. Cloning
        // every member/footer/name and the lookup map made a cache hit O(bank
        // size), and duplicated the namespace again on its first admission.
        let directory = Arc::new(directory);
        let paths = Arc::new(paths);
        let ufs = Arc::new(ufs);
        let mut recent = cache.lock().unwrap_or_else(|e| e.into_inner());
        recent.retain(|r| !r.ufs.same_bank(&ufs));
        recent.insert(
            0,
            Recent {
                ufs: ufs.clone(),
                directory: directory.clone(),
                paths: paths.clone(),
                content_key: None,
            },
        );
        recent.truncate(RECENT_BANKS);
        drop(recent);
        Ok(Self {
            ufs,
            directory,
            content_key: None,
            program_namespace,
            paths,
        })
    }

    /// Program member paths (`*.uvip`), sorted.
    pub fn programs(&self) -> Vec<String> {
        program_paths(&self.directory)
    }

    fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.ufs
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    /// Every member path in the bank, in directory order.
    pub fn members(&self) -> Vec<String> {
        self.directory
            .files
            .iter()
            .filter_map(|m| m.path.clone())
            .collect()
    }

    /// Every Lua occurrence has an admission outcome, including unresolved
    /// paths and failed reads. Only exact UTF-8 source is executable. Unused
    /// failures remain optional; a requested failure reaches `require`.
    pub fn scripts(&self) -> crate::script::Scripts {
        use crate::script::{ScriptOccurrence, ScriptReadError as E};
        let mut scripts = crate::script::Scripts::default();
        for (member_index, member) in self.directory.files.iter().enumerate() {
            if !member.name.to_ascii_lowercase().ends_with(".lua")
                && !member.path.as_ref().is_some_and(|p| p.to_ascii_lowercase().ends_with(".lua"))
            {
                continue;
            }
            let read = || {
                let path = member.path.as_ref().ok_or(E::UnresolvedPath)?;
                if self.paths.get(&path.to_ascii_lowercase()) == Some(&None) {
                    return Err(E::Ambiguous);
                }
                match member.mode {
                    Protection::Unknown(mode) => return Err(E::UnsupportedProtection(mode)),
                    Protection::Content if self.content_key.is_none() => return Err(E::AccessRequired),
                    _ => {},
                }
                // The existing UFS whole-resource limit also applies to Lua.
                if member.size > crate::ufs::MAX_MEMBER_SIZE { return Err(E::Limit); }
                if !member.offset.checked_add(member.size)
                    .is_some_and(|end| end <= self.ufs.header.physical_size)
                { return Err(E::InvalidRange); }
                let bytes = self.read(member).map_err(|_| E::Read)?;
                String::from_utf8(bytes).map_err(|e| E::InvalidUtf8 {
                    valid_up_to: e.utf8_error().valid_up_to(),
                    error_len: e.utf8_error().error_len(),
                })
            };
            let result = read();
            scripts.record(ScriptOccurrence {
                member_index,
                record_offset: member.record_offset,
                path: member.path.clone(),
                bytes: member.size,
                outcome: result.as_ref().map(|_| ()).map_err(|e| *e),
            }, result.ok());
        }
        scripts
    }

    /// The same selected-bank image authority used by playback and inspection.
    /// Clones share immutable bank metadata; image dimension caching belongs
    /// to this script host rather than a process-global source namespace.
    pub(crate) fn bind_script_images(
        &self,
        program: &str,
        files: crate::script::Scripts,
    ) -> crate::script::Scripts {
        let bank = self.clone();
        let program = program.to_owned();
        files.with_images(move |path| bank.ui_resource_result(&program, path).ok().flatten())
    }

    pub(crate) fn bind_script_samples(&self, program:&str, files:crate::script::Scripts)->crate::script::Scripts {
        let bank=self.clone();let program=program.to_owned();
        files.with_samples(Arc::new(move|path,explicit|bank.sample_resource(&program,path,explicit)))
    }

    /// The bytes of the member at bank-root `path` (a script, say).
    pub fn file(&self, path: &str) -> Result<Vec<u8>, String> {
        let read = || self.read(resolve(&self.directory, path)?);
        read().map_err(|e| access::failure_reason(&e))
    }

    /// Decode the program at member `name` to its clear XML and its path.
    pub fn program(&self, name: &str) -> Result<(String, String), AccessError> {
        self.program_inner(name)
            .map_err(|e| AccessError::Program(access::failure_reason(&e)))
    }

    fn program_inner(&self, name: &str) -> Result<(String, String)> {
        let member = self.resolve(name)?;
        ensure!(
            member.size <= crypto::PROGRAM_XML_LIMIT as u64,
            "UVI program exceeds 32 MiB"
        );
        let bytes = self.read(member)?;
        let text = crypto::decode_program_bytes(&bytes, &self.program_namespace)?;
        let path = member
            .path
            .clone()
            .context("UVI program has no directory path")?;
        Ok((text, path))
    }

    /// Read a UI asset relative to its preset or the bank resource root. UVI
    /// scripts commonly name Resources/... from a shared Scripts folder.
    /// Ambiguous suffixes and references to another bank are never accepted.
    pub fn ui_resource(&self, program: &str, path: &str) -> Result<Vec<u8>, String> {
        self.ui_resource_result(program, path)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "UI resource missing".into())
    }

    /// Same bank/script-origin authority as ui_resource, without erasing failure categories.
    pub fn ui_resource_result(
        &self,
        program: &str,
        path: &str,
    ) -> std::result::Result<Option<Vec<u8>>, crate::ResourceError> {
        use crate::ResourceError as E;
        crate::resources::validate_path(path)?;
        let path = path.replace('\\', "/");
        let (program, path) = resource_base(program, &path, &self.ufs.header.bank_name)
            .map_err(|_| E::InvalidPath)?;
        let candidates = ui_candidates(program, path).map_err(|_| E::InvalidPath)?;
        let mut member = None;
        for candidate in &candidates {
            if let Some(index) = self.paths.get(&candidate.to_ascii_lowercase()) {
                member = Some(&self.directory.files[index.ok_or(E::Ambiguous)?]);
                break;
            }
        }
        let member = if let Some(member) = member {
            member
        } else {
            let normalized = normalize(path).map_err(|_| E::InvalidPath)?;
            let suffix = format!("/{}", normalized.to_ascii_lowercase());
            let Some(index) = ui_suffix_index(&self.paths, &suffix)? else {
                return Ok(None);
            };
            &self.directory.files[index]
        };
        if member.size > 32 << 20 {
            return Err(E::Limit);
        };
        self.read(member).map(Some).map_err(|e| {
            if e.downcast_ref::<std::io::Error>().is_some() {
                E::Read
            } else {
                E::Corrupt
            }
        })
    }

    /// Decode a bank-local audio resource relative to `program_path`. A starred
    /// filename is a bundle of mono channels. Returns raw encoded file bytes.
    pub fn resource(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>, AccessError> {
        self.resource_inner(program_path, path)
            .map_err(|e| AccessError::Resource(access::failure_reason(&e)))
    }

    /// Read one ordinary sample resource with a preflight bound on encoded
    /// bytes. Resolution is identical to `resource`; no member is read until
    /// every resolved member fits the aggregate budget. The caller may spend a
    /// smaller remaining budget across several resources, up to 128 MiB.
    /// This does not decode PCM or change the existing `resource` API.
    pub fn resource_bounded(
        &self,
        program_path: &str,
        path: &str,
        byte_limit: u64,
    ) -> Result<Vec<Vec<u8>>, AccessError> {
        let read = || -> Result<Vec<Vec<u8>>> {
            let path = path.replace('\\', "/");
            let (program_path, path) =
                resource_base(program_path, &path, &self.ufs.header.bank_name)?;
            let members = resources(program_path, path, |path| self.resolve(path))?;
            read_bounded_resource_parts(&members, byte_limit, |member| member.size,
                |member| self.read(member))
        };
        read().map_err(|e| AccessError::Resource(access::failure_reason(&e)))
    }

    /// Initial resident read through the ordinary resource resolver, bounded
    /// before any member bytes are read. No decoder/access revision changes.
    pub(crate) fn initial_resident_resource(&self,program_path:&str,path:&str)->Result<Vec<Vec<u8>>,AccessError> {
        self.resource_bounded(program_path,path,128<<20)
    }

    pub(crate) fn initial_resident_sample(&self,program_path:&str,path:&str,inherit:bool)
        ->Result<crate::audio::InitialDecoded,String> {
        let parts=self.initial_resident_resource(program_path,path).map_err(|e|e.to_string())?;
        let decoded=crate::audio::decode_initial(&parts,inherit)?;
        let bank=self.clone();let program_path=program_path.to_owned();let path=path.to_owned();
        sampler_kontakt::install_resident_source_reload(&decoded.source,move || {
            let parts=bank.initial_resident_resource(&program_path,&path).map_err(|_|sampler_core::Error::NotReady)?;
            crate::audio::decode_initial(&parts,false).map(|d|d.source).map_err(|_|sampler_core::Error::NotReady)
        }).map_err(|e|e.to_string())?;
        Ok(decoded)
    }

    /// Decode a resource or mono-channel bundle without translating the program.
    pub fn decode_resource(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<sampler_kontakt::Decoded, AccessError> {
        crate::audio::decode(&self.resource(program_path, path)?)
            .map(|(audio, _)| audio)
            .map_err(AccessError::Audio)
    }

    /// A bank-local audio resource for streaming: read in pieces and decrypted
    /// in memory as it is read, never written out. Checked by opening it once.
    pub fn stream_source(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<std::sync::Arc<dyn sampler_kontakt::AssetSource>, String> {
        self.stream_inner(program_path, path)
            .map_err(|e| access::failure_reason(&e))
    }

    pub(crate) fn sample_resource(&self, program_path:&str,path:crate::script::SamplePath<'_>,geometry_override:bool) -> Result<crate::script::SampleResource,String> {
        let started = std::time::Instant::now();
        let resolve = |candidate: &str| {
            // Dynamic paths have explicit lookup roots; a bare name must never
            // select a different directory elsewhere in the bank. Both hits
            // and misses use the admitted map, without rescanning every member.
            dynamic_sample_index(&self.paths, candidate).map(|i| &self.directory.files[i])
        };
        let (members, candidates) = sample_members_counted(program_path, path, &self.ufs.header.bank_name, resolve)
            .map_err(|e|access::failure_reason(&e))?;
        let parts = members.into_iter().map(|member| {
            let(offset,size,key)=self.ufs.locate(member,self.directory.metadata_key,self.content_key)?;
            Ok(crate::stream::Origin::Member{ufs:self.ufs.clone(),offset,size,key})
        }).collect::<Result<Vec<_>>>().map_err(|e|access::failure_reason(&e))?;
        let lookup_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        let mut resource = crate::stream::sample_resource(parts, geometry_override)?;
        resource.timings.path_candidates = candidates;
        resource.timings.lookup_ns = lookup_ns;
        Ok(resource)
    }

    fn resource_origins(&self,program_path:&str,path:&str)->Result<Vec<crate::stream::Origin>> {
        let path=path.replace('\\',"/");
        let(program_path,path)=resource_base(program_path,&path,&self.ufs.header.bank_name)?;
        resources(program_path,path,|path|self.resolve(path))?.into_iter().map(|member|{
            let(offset,size,key)=self.ufs.locate(member,self.directory.metadata_key,self.content_key)?;
            Ok(crate::stream::Origin::Member{ufs:self.ufs.clone(),offset,size,key})
        }).collect()
    }

    pub(crate) fn initial_stream_source(&self,program_path:&str,path:&str,inherit:bool)
        ->Result<crate::stream::InitialSource,String> {
        let parts=self.resource_origins(program_path,path).map_err(|e|access::failure_reason(&e))?;
        crate::stream::initial_source(parts,inherit)
    }

    fn stream_inner(
        &self,
        program_path: &str,
        path: &str,
    ) -> Result<std::sync::Arc<dyn sampler_kontakt::AssetSource>> {
        let parts=self.resource_origins(program_path,path)?;
        crate::stream::source(parts).map_err(anyhow::Error::msg)
    }

    fn resource_inner(&self, program_path: &str, path: &str) -> Result<Vec<Vec<u8>>> {
        let path = path.replace('\\', "/");
        let (program_path, path) = resource_base(program_path, &path, &self.ufs.header.bank_name)?;
        resources(program_path, path, |path| self.resolve(path))?
            .iter()
            .map(|member| self.read(member))
            .collect()
    }

    fn resolve(&self, path: &str) -> Result<&Member> {
        let path = normalize(path)?;
        if let Some(index) = self.paths.get(&path.to_ascii_lowercase()) {
            return index
                .map(|i| &self.directory.files[i])
                .context("Ambiguous UVI member path");
        }
        resolve(&self.directory, &path)
    }
}

/// Namespace keys are folded once when the bank is admitted. Missing-resource
/// probes must not allocate a fresh lowercase string for every bank member.
fn ui_suffix_index(
    paths: &HashMap<String, Option<usize>>,
    suffix: &str,
) -> std::result::Result<Option<usize>, crate::ResourceError> {
    let mut matches = paths.iter().filter(|(path, _)| path.ends_with(suffix));
    let Some((_, index)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(crate::ResourceError::Ambiguous);
    }
    index.map(Some).ok_or(crate::ResourceError::Ambiguous)
}

/// Preserve the actual module directory first, then search its package roots.
/// Candidate order never turns an ambiguous exact identity into a suffix hit.
fn ui_candidates(program: &str, path: &str) -> Result<Vec<String>> {
    let base = program.rsplit_once('/').map_or("", |(base, _)| base);
    let rooted = normalize(path)?;
    let mut out = Vec::new();
    if !path.starts_with('/') {
        out.push(normalize(&format!("{base}/{path}"))?);
    }
    if !out.contains(&rooted) {
        out.push(rooted.clone());
    }
    let parts: Vec<_> = rooted.split('/').collect();
    if let Some(scripts) = parts.iter().position(|p| p.eq_ignore_ascii_case("Scripts")) {
        // A module can live below nested package folders. The bank index is
        // authoritative; no filesystem or another bank participates.
        for start in scripts + 1..parts.len() {
            let candidate = parts[start..].join("/");
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    Ok(out)
}

/// Falcon's `$Bank.ufs/` volume is rooted in this bank, not the preset folder.
/// Never satisfy a resource explicitly bound to a different bank.
fn resource_base<'a>(program: &'a str, path: &'a str, bank: &str) -> Result<(&'a str, &'a str)> {
    let Some(volume) = path.strip_prefix('$') else {
        return Ok((program, path));
    };
    let (volume, relative) = volume
        .split_once('/')
        .context("UVI bank volume has no resource path")?;
    ensure!(
        volume.eq_ignore_ascii_case(bank) || volume.eq_ignore_ascii_case(&format!("{bank}.ufs")),
        "UVI resource belongs to a different bank"
    );
    Ok(("", relative))
}

fn normalize(path: &str) -> Result<String> {
    ensure!(
        !path.contains(['\0', ':']) && !path.starts_with('$'),
        "Unresolved UVI volume path"
    );
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(
                    parts.pop().is_some(),
                    "UVI resource path ascends above bank root"
                );
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Exact paths take priority; a bare member name is usable only if unique.
/// Some banks retain WAV references after replacing their members with FLAC.
/// An alternate lossless format must have the same directory and sample stem.
fn resolve<'a>(directory: &'a Directory, path: &str) -> Result<&'a Member> {
    resolve_at(directory, path, true)
}

fn resolve_at<'a>(directory: &'a Directory, path: &str, allow_bare_name: bool) -> Result<&'a Member> {
    let path = normalize(path)?;
    let exact: Vec<_> = directory
        .files
        .iter()
        .filter(|m| {
            m.path
                .as_ref()
                .is_some_and(|p| p.eq_ignore_ascii_case(&path))
        })
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    ensure!(exact.is_empty(), "Ambiguous UVI member path");
    let bare = !path.contains('/');
    if bare && allow_bare_name {
        let named: Vec<_> = directory
            .files
            .iter()
            .filter(|m| m.name.eq_ignore_ascii_case(&path))
            .collect();
        if named.len() == 1 {
            return Ok(named[0]);
        }
        ensure!(named.is_empty(), "Ambiguous UVI member name");
    }
    if let Some(stem) = audio_stem(&path) {
        let mut alternatives = directory.files.iter().filter(|member| {
            let name = if bare && allow_bare_name {
                Some(member.name.as_str())
            } else {
                member.path.as_deref()
            };
            name.and_then(audio_stem).is_some_and(|candidate|candidate.eq_ignore_ascii_case(stem))
        });
        if let Some(member) = alternatives.next() {
            ensure!(alternatives.next().is_none(), "Ambiguous UVI audio format");
            return Ok(member);
        }
    }
    Err(MissingResource.into())
}

#[derive(Debug)]
struct MissingResource;
impl std::fmt::Display for MissingResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UVI resource path is absent from this bank")
    }
}
impl std::error::Error for MissingResource {}

fn dynamic_sample_index(paths: &HashMap<String, Option<usize>>, path: &str) -> Result<usize> {
    let path = path.to_ascii_lowercase();
    if let Some(index) = paths.get(&path) { return index.context("Ambiguous UVI member path"); }
    if let Some(stem) = audio_stem(&path) {
        let mut found = None;
        for extension in ["wav", "aif", "aiff", "flac"] {
            if let Some(index) = paths.get(&format!("{stem}.{extension}")) {
                let index = index.context("Ambiguous UVI member path")?;
                ensure!(found.replace(index).is_none(), "Ambiguous UVI audio format");
            }
        }
        if let Some(index) = found { return Ok(index); }
    }
    Err(MissingResource.into())
}

/// The available native read contexts, in order, inside this bank only.
/// Explicit roots and volumes never become a relative fallback search. Unknown
/// multi/original-load contexts are not guessed from unrelated folders.
fn sample_candidates(program: &str, request: crate::script::SamplePath<'_>, bank: &str) -> Result<Vec<String>> {
    let path = request.path.replace('\\', "/");
    ensure!(!path.is_empty() && path.len() <= 4096 && !path.contains(['\0', ':']), "Invalid UVI sample path");
    if path.starts_with('$') {
        let (_, path) = resource_base(program, &path, bank)?;
        return Ok(vec![normalize(path)?]);
    }
    if path.starts_with('/') { return Ok(vec![normalize(&path)?]); }
    let base = program.rsplit_once('/').map_or("", |(base, _)| base);
    let mut out = Vec::new();
    for root in [Some(""), request.script_directory, Some(base), Some("Samples")].into_iter().flatten() {
        // A relative candidate can escape one context while still resolving
        // inside another. No candidate can escape the admitted bank root.
        if let Ok(candidate) = normalize(&format!("{root}/{path}")) {
            ensure!(candidate.len() <= 4096, "UVI sample candidate exceeds path capacity");
            if !out.contains(&candidate) { out.push(candidate); }
        }
    }
    ensure!(!out.is_empty(), "UVI sample path ascends above all available roots");
    Ok(out)
}

#[cfg(test)]
fn sample_members<'a>(program: &str, request: crate::script::SamplePath<'_>, bank: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>) -> Result<Vec<&'a Member>> {
    sample_members_counted(program,request,bank,resolve).map(|(members,_)|members)
}
fn sample_members_counted<'a>(program: &str, request: crate::script::SamplePath<'_>, bank: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>) -> Result<(Vec<&'a Member>,u32)> {
    for (index,path) in sample_candidates(program, request, bank)?.into_iter().enumerate() {
        match resources("", &format!("/{path}"), &resolve) {
            Ok(members) => return Ok((members,index as u32+1)),
            Err(e) if e.downcast_ref::<MissingResource>().is_some() => {},
            Err(e) => return Err(e),
        }
    }
    Err(MissingResource.into())
}

fn audio_stem(path: &str) -> Option<&str> {
    let (stem, extension) = path.rsplit_once('.')?;
    ["wav", "aif", "aiff", "flac"]
        .iter()
        .any(|format| extension.eq_ignore_ascii_case(format))
        .then_some(stem)
}

fn resource<'a>(
    program_path: &str,
    path: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>,
) -> Result<&'a Member> {
    let base = program_path.rsplit_once('/').map_or("", |(base, _)| base);
    let path = if path.starts_with('/') {
        normalize(path)?
    } else {
        normalize(&format!("{base}/{path}"))?
    };
    resolve(&path)
}

/// A stricter off-audio budget, separate from the unchanged 512 MiB member
/// limit of the ordinary reader. All admission work precedes the first read.
fn read_bounded_resource_parts<T>(
    members: &[T],
    byte_limit: u64,
    size: impl Fn(&T) -> u64,
    mut read: impl FnMut(&T) -> Result<Vec<u8>>,
) -> Result<Vec<Vec<u8>>> {
    const MAX_RESOURCE_BYTES: u64 = 128 << 20;
    ensure!(byte_limit <= MAX_RESOURCE_BYTES,
        "bounded UVI resource budget exceeds 128 MiB");
    ensure!(!members.is_empty() && members.len() <= 12,
        "bounded UVI resource needs 1 through 12 members");
    // Sum before checking the budget: overflow is a refusal, never a wrapped
    // value that might admit an over-budget read.
    let total = members.iter().try_fold(0u64, |total, member| {
        total.checked_add(size(member)).context("bounded UVI resource size overflow")
    })?;
    ensure!(members.iter().all(|member| size(member) <= crate::ufs::MAX_MEMBER_SIZE),
        "UVI member exceeds ordinary resource limit");
    ensure!(total <= byte_limit, "bounded UVI resource exceeds byte budget");
    members.iter().map(|member| {
        let bytes = read(member)?;
        ensure!(bytes.len() as u64 == size(member),
            "bounded UVI resource returned a different byte count");
        Ok(bytes)
    }).collect()
}

/// A starred filename is an ordered list of synchronized mono channels.
fn resources<'a>(
    program_path: &str,
    path: &str,
    resolve: impl Fn(&str) -> Result<&'a Member>,
) -> Result<Vec<&'a Member>> {
    if let Some((base, names)) = path.split_once('*') {
        ensure!(
            base.ends_with(['/', '\\']),
            "Invalid UVI channel bundle prefix"
        );
        let names: Vec<_> = names.split('*').collect();
        ensure!(
            !names.is_empty() && names.len() <= 12,
            "UVI channel bundle exceeds 12 channels"
        );
        names
            .into_iter()
            .map(|name| {
                ensure!(
                    !name.is_empty() && !name.contains(['/', '\\']),
                    "Invalid UVI channel bundle member"
                );
                resource(program_path, &format!("{base}{name}"), &resolve)
            })
            .collect()
    } else {
        Ok(vec![resource(program_path, path, resolve)?])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_resources_refuse_before_reading_members() {
        use std::cell::Cell;
        let calls = Cell::new(0usize);
        for (sizes, limit) in [
            (vec![4u64, 5], 8),
            (vec![1u64; 13], 128),
            (vec![crate::ufs::MAX_MEMBER_SIZE + 1], 128 << 20),
            (vec![0u64], (128 << 20) + 1),
            (Vec::new(), 128),
        ] {
            let result = read_bounded_resource_parts(&sizes, limit, |size| *size, |_| {
                calls.set(calls.get() + 1);
                Ok(Vec::new())
            });
            assert!(result.is_err());
            assert_eq!(calls.get(), 0, "preflight refusal must precede any read");
        }
    }

    #[test]
    fn bounded_resources_size_overflow_refuses_before_read() {
        let sizes = [u64::MAX, 1];
        let error = read_bounded_resource_parts(&sizes, 128 << 20, |size| *size,
            |_| panic!("overflow must not invoke the read closure")).unwrap_err();
        assert_eq!(error.to_string(), "bounded UVI resource size overflow");
    }

    #[test]
    fn bounded_resources_128_mib_boundary_is_admitted_without_allocating_payload() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let limit = 128u64 << 20;
        let error = read_bounded_resource_parts(&[limit], limit, |size| *size, |_| {
            calls.set(calls.get() + 1);
            anyhow::bail!("read reached after exact preflight boundary")
        }).unwrap_err();
        assert_eq!(calls.get(), 1);
        assert_eq!(error.to_string(), "read reached after exact preflight boundary");
        let over = read_bounded_resource_parts(&[limit + 1], limit, |size| *size,
            |_| panic!("one byte beyond 128 MiB must not read"));
        assert!(over.is_err());
    }

    #[test]
    fn bounded_resources_exact_budget_preserves_member_order() {
        let members = [(3u64, 7u8), (5, 11)];
        let got = read_bounded_resource_parts(&members, 8, |(size, _)| *size,
            |(size, fill)| Ok(vec![*fill; *size as usize])).unwrap();
        assert_eq!(got, [vec![7; 3], vec![11; 5]]);
        assert!(read_bounded_resource_parts(&members, 7, |(size, _)| *size,
            |_| panic!("one byte over budget must not read")).is_err());
    }

    #[test]
    fn bounded_resource_resolution_matches_normal_resource_order() {
        let member = |path: &str, size| Member {
            record_offset: 0, name: path.rsplit('/').next().unwrap().into(), parent: None,
            path: Some(path.into()), size, offset: 0, mode: Protection::Clear, footer: Vec::new(),
        };
        let directory = Directory {
            files: vec![member("Samples/left.flac", 3), member("Samples/right.flac", 5)],
            directories: Vec::new(), records: Vec::new(), warnings: Vec::new(), metadata_key: 0,
        };
        let path = "$Fixture.ufs/Samples/*left.wav*right.wav";
        let (program, relative) = resource_base("Presets/p.uvip", path, "Fixture").unwrap();
        let ordinary = resources(program, relative, |p| resolve(&directory, p)).unwrap();
        let bounded = resources(program, relative, |p| resolve(&directory, p)).unwrap();
        assert!(ordinary.iter().zip(&bounded).all(|(a,b)| std::ptr::eq(*a,*b)));
        let encode = |m: &&Member| Ok(vec![m.size as u8; m.size as usize]);
        let expected = ordinary.iter().map(encode).collect::<Result<Vec<_>>>().unwrap();
        let got = read_bounded_resource_parts(&bounded, 8, |m| m.size, encode).unwrap();
        assert_eq!(got, expected);
    }

    #[test]
    fn lua_inventory_retains_failed_occurrences_without_rewriting_source_or_requiring_unused_modules() {
        use crate::script::{Files, ScriptReadError as E};
        // Only authored bytes are read. The synthetic namespace deliberately
        // includes duplicate, unlinked, protected, oversized and invalid entries.
        let valid = "return 'é'\r\n";
        let (bytes, _) = clear_bank_fixture_with_xml(valid.as_bytes());
        let path = std::env::temp_dir().join(format!("kontra-lua-outcomes-{}.ufs", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = Bank::open_metadata(&path).unwrap();
        let template = bank.directory.files[0].clone();
        let mut files = Vec::new();
        for (index, (name, mode)) in [
            ("good.lua", Protection::Clear), ("encoding.lua", Protection::Clear),
            ("protected.lua", Protection::Content), ("unknown.lua", Protection::Unknown(7)),
            ("unlinked.lua", Protection::Clear), ("duplicate.lua", Protection::Clear),
            ("Duplicate.lua", Protection::Clear), ("large.lua", Protection::Clear),
            ("range.lua", Protection::Clear),
        ].into_iter().enumerate() {
            let mut m = template.clone();
            m.name = name.into(); m.path = Some(name.into()); m.mode = mode;
            m.record_offset += index as u64;
            if name == "encoding.lua" { m.offset += valid.find('é').unwrap() as u64 + 1; m.size = 1; }
            if name == "unlinked.lua" { m.path = None; }
            if name == "large.lua" { m.size = (512 << 20) + 1; }
            if name == "range.lua" { m.offset = u64::MAX; }
            files.push(m);
        }
        Arc::make_mut(&mut bank.directory).files = files;
        bank.paths = Arc::new(bank.directory.files.iter().enumerate()
            .filter_map(|(i, m)| Some((m.path.as_ref()?.to_ascii_lowercase(), Some(i)))).collect());
        Arc::make_mut(&mut bank.paths).insert("duplicate.lua".into(), None);
        let scripts = bank.scripts();
        assert_eq!(scripts.occurrences().len(), 9);
        assert_eq!(scripts.names(), ["good.lua"]);
        assert_eq!(scripts.script_result("good").unwrap().unwrap().as_bytes(), valid.as_bytes());
        let outcomes: Vec<_> = scripts.occurrences().iter().map(|o| o.outcome).collect();
        assert_eq!(outcomes, [Ok(()), Err(E::InvalidUtf8 { valid_up_to: 0, error_len: Some(1) }),
            Err(E::AccessRequired), Err(E::UnsupportedProtection(7)), Err(E::UnresolvedPath),
            Err(E::Ambiguous), Err(E::Ambiguous), Err(E::Limit), Err(E::InvalidRange)]);
        for occurrence in scripts.occurrences() {
            assert_eq!(occurrence.record_offset, template.record_offset + occurrence.member_index as u64);
        }
        let xml = "<Program><EventProcessors><ScriptProcessor><script>assert(require('good')=='é')</script></ScriptProcessor></EventProcessors></Program>";
        let host = crate::script::ScriptHost::new(xml, scripts.clone(), Default::default()).unwrap();
        assert!(host.fault_counts().init.is_empty(), "unused unavailable members remain optional");
        assert_eq!(scripts.script_result("encoding"), Err(E::InvalidUtf8 { valid_up_to: 0, error_len: Some(1) }));
        let xml = "<Program><EventProcessors><ScriptProcessor><script>require('encoding')</script></ScriptProcessor></EventProcessors></Program>";
        let host = crate::script::ScriptHost::new(xml, scripts, Default::default()).unwrap();
        assert!(host.findings().iter().any(|f| f.value.contains("invalid UTF-8")), "{:?}", host.findings());
        std::fs::write(&path, [0; 320]).unwrap();
        assert_eq!(bank.scripts().occurrences()[0].outcome, Err(E::Read));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn standalone_passwordv2_program_text_uses_native_namespace() {
        use base64::Engine as _;

        let namespace = Namespaces::native().program;
        let mut password = b"native standalone fixture password\0".to_vec();
        let mut program = b"<Program Name=\"Native standalone fixture\"/>".to_vec();
        program.resize(program.len().div_ceil(8) * 8, 0);
        crypto::transform(
            &mut program,
            crypto::key_from_string(&password[..password.len() - 1]),
            0,
        );
        crypto::transform(&mut password, crypto::key_from_string(&namespace), 0);
        let wrapper = format!(
            "<UVI4><Program PasswordV2=\"{}\">{}</Program></UVI4>",
            base64::engine::general_purpose::STANDARD.encode(password),
            base64::engine::general_purpose::STANDARD.encode(program),
        );
        assert_eq!(
            program_text(wrapper.as_bytes()).unwrap(),
            "<Program Name=\"Native standalone fixture\"/>"
        );
    }

    fn clear_bank_fixture() -> (Vec<u8>, Vec<u8>) {
        clear_bank_fixture_with_xml(b"<UVI4><Program Name=\"Our clear fixture\"/></UVI4>")
    }

    fn clear_bank_fixture_with_xml(xml: &[u8]) -> (Vec<u8>, Vec<u8>) {
        fn append(bytes: &mut Vec<u8>, payload: &[u8]) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend((payload.len() as u64).to_le_bytes());
            bytes.extend(payload);
            pointer
        }
        fn point(bytes: &mut [u8], at: u64, value: u64) {
            bytes[at as usize..at as usize + 8].copy_from_slice(&value.to_le_bytes());
        }
        let mut bytes = vec![0; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..56].copy_from_slice(b"Authored");
        // +32 stays opaque, not an asserted physical size.
        point(&mut bytes, 32, 123);
        let mut folder = vec![0; 272];
        folder[..4].copy_from_slice(&0x2fba3632u32.to_le_bytes());
        folder[4..8].copy_from_slice(b"Root");
        let root = append(&mut bytes, &folder);
        point(&mut bytes, 40, root);
        let mut file = vec![0; 289];
        file[..4].copy_from_slice(&0x675850e4u32.to_le_bytes());
        file[4..15].copy_from_slice(b"preset.uvip");
        let member = append(&mut bytes, &file);
        let mut descriptor = vec![0; 34];
        descriptor[..4].copy_from_slice(&0x1847b398u32.to_le_bytes());
        let tree = append(&mut bytes, &descriptor);
        point(&mut bytes, root + 260, tree);
        let mut leaf = vec![0; 288];
        leaf[..4].copy_from_slice(&0x3ca86aafu32.to_le_bytes());
        leaf[4..8].copy_from_slice(&1u32.to_le_bytes());
        leaf[8..19].copy_from_slice(b"preset.uvip");
        point(&mut leaf, 264, member);
        leaf[272..288].fill(255);
        let table = append(&mut bytes, &leaf);
        for offset in [4, 12, 20] {
            point(&mut bytes, tree + offset, table);
        }
        let payload = append(&mut bytes, xml);
        point(&mut bytes, member + 260, xml.len() as u64);
        point(&mut bytes, member + 268, payload);
        (bytes, xml.to_vec())
    }

    fn census_bank_fixture() -> (Vec<u8>, Vec<u8>) {
        let (mut bytes, xml) = clear_bank_fixture();
        let mut asset = vec![0; 289];
        asset[..4].copy_from_slice(&0x675850e4u32.to_le_bytes());
        asset[4..13].copy_from_slice(b"asset.bin");
        asset[276] = 2;
        bytes.extend((asset.len() as u64).to_le_bytes());
        bytes.extend(asset);
        bytes[304] = 1;
        let key = crypto::metadata_key(&Namespaces::native().metadata, "Authored");
        let mut at = 320;
        while at < bytes.len() {
            let length = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap()) as usize;
            let pointer = at + 8;
            let tag = u32::from_le_bytes(bytes[pointer..pointer + 4].try_into().unwrap());
            if matches!(tag, 0x2fba3632 | 0x675850e4) {
                crypto::transform(
                    &mut bytes[pointer + 4..pointer + 260],
                    key,
                    (pointer + 4) as u64,
                );
                if tag == 0x675850e4 && bytes[pointer + 276] == 0 {
                    bytes[pointer + 276] = 1;
                    let size =
                        u64::from_le_bytes(bytes[pointer + 260..pointer + 268].try_into().unwrap())
                            as usize;
                    let offset =
                        u64::from_le_bytes(bytes[pointer + 268..pointer + 276].try_into().unwrap())
                            as usize;
                    crypto::transform_blocks(&mut bytes[offset..offset + size], key, offset as u64);
                }
            } else if tag == 0x3ca86aaf {
                let count = u32::from_le_bytes(bytes[pointer + 4..pointer + 8].try_into().unwrap())
                    as usize;
                for child in 0..count {
                    let start = pointer + 8 + child * 264;
                    crypto::transform(&mut bytes[start..start + 256], key, start as u64);
                }
            }
            at += length + 8;
        }
        (bytes, xml)
    }

    #[test]
    fn census_does_not_prepare_unrelated_content() {
        let (bytes, xml) = census_bank_fixture();
        let path =
            std::env::temp_dir().join(format!("kontra-census-bank-{}.ufs", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut bank =
            Bank::open_metadata(&path).expect("census must not prepare unrelated content");
        assert!(bank.content_key.is_none());
        assert_eq!(bank.program("preset.uvip").unwrap().0.as_bytes(), xml);
        assert!(crate::translate_program(&bank, "preset.uvip").is_ok());
        assert!(
            bank.read(&bank.directory.files[1]).is_err(),
            "content access still requires a supplied key"
        );
        Arc::make_mut(&mut bank.directory).files[0].mode = Protection::Content;
        assert!(
            bank.program("preset.uvip").is_err(),
            "metadata census must refuse content-protected programs"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decoded_program_translation_matches_bank_read_and_promotes_script_writes() {
        let xml = br#"<UVI4><Program Name="P"><ControlSignalSources><ConstantModulation Name="Macro 1" Value="0.25"/></ControlSignalSources><EventProcessors><ScriptProcessor><script>function onController(e) Program.modulations['Macro 1']:setParameter('Value', 1) end</script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
        let (bytes, _) = clear_bank_fixture_with_xml(xml);
        let path = std::env::temp_dir().join(format!(
            "kontra-decoded-program-{}-{}.ufs",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, bytes).unwrap();
        let bank = Bank::open(&path).unwrap();
        let decoded = bank.program("preset.uvip").unwrap().0;
        let reread = crate::translate_program(&bank, "preset.uvip").unwrap();
        let reused = crate::translate_decoded_program(&bank, "preset.uvip", decoded).unwrap();
        assert_eq!(
            serde_json::to_value(&reread).unwrap(),
            serde_json::to_value(&reused).unwrap()
        );
        assert!(
            reused
                .uvi_script_sources
                .as_ref()
                .unwrap()
                .xml
                .contains("ScriptLive=")
        );
        std::fs::remove_file(path).unwrap();
    }

    /// Reopening reuses the recent bank's directory; a rewritten file never does.
    #[test]
    fn recent_bank_metadata_is_reused_only_for_the_same_file() {
        // Other bank tests may evict the global two-bank LRU between opens.
        // Exercise the identical cache implementation with private ownership.
        let cache = Mutex::new(Vec::new());
        let path = std::env::temp_dir().join(format!(
            "kontra-recent-bank-{}-{}.ufs",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let text = |bank: &Bank| bank.program("preset.uvip").unwrap().0;
        let (first, first_xml) =
            clear_bank_fixture_with_xml(b"<UVI4><Program Name=\"One\"/></UVI4>");
        std::fs::write(&path, first).unwrap();
        let first = Bank::open_in(&path, &cache).unwrap();
        assert_eq!(text(&first).as_bytes(), first_xml);
        let reopened = Bank::open_in(&path, &cache).unwrap();
        assert!(
            Arc::ptr_eq(&first.directory, &reopened.directory),
            "warm readers share the whole immutable namespace"
        );
        assert!(
            Arc::ptr_eq(&first.paths, &reopened.paths),
            "warm readers share the immutable lookup map"
        );
        assert!(
            recent_in(&cache, &reopened.ufs, |_| ()).is_some(),
            "reopen hits the recent bank"
        );
        assert_eq!(text(&reopened).as_bytes(), first_xml);
        let (second, second_xml) =
            clear_bank_fixture_with_xml(b"<UVI4><Program Name=\"Second program\"/></UVI4>");
        std::fs::write(&path, second).unwrap();
        let rewritten = Bank::open_in(&path, &cache).unwrap();
        assert_eq!(text(&rewritten).as_bytes(), second_xml);
        assert!(
            !Arc::ptr_eq(&first.directory, &rewritten.directory),
            "rewritten bank cannot reuse stale members"
        );
        assert!(
            !Arc::ptr_eq(&first.paths, &rewritten.paths),
            "rewritten bank cannot reuse stale paths"
        );
        std::fs::remove_file(path).unwrap();
    }

    /// Numeric-only evidence for an installed bank; no program/sample payload
    /// is decoded. The legacy column reconstructs the removed deep clones and
    /// is not an archived pre-change executable measurement.
    #[test]
    #[ignore = "requires KONTRA_PROFILE_BANK, owned bank metadata and an uncontended timing window"]
    fn owned_bank_metadata_cache_profile() {
        let path = std::env::var_os("KONTRA_PROFILE_BANK").expect("owned bank path");
        let path = Path::new(&path);
        let cold_start = std::time::Instant::now();
        let first = Bank::open_metadata(path).unwrap();
        let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.;
        let mut warm_ms = Vec::new();
        let mut legacy_clone_ms = Vec::new();
        for _ in 0..5 {
            let start = std::time::Instant::now();
            let reopened = Bank::open_metadata(path).unwrap();
            warm_ms.push(start.elapsed().as_secs_f64() * 1000.);
            assert!(Arc::ptr_eq(&first.directory, &reopened.directory));
            assert!(Arc::ptr_eq(&first.paths, &reopened.paths));
            let start = std::time::Instant::now();
            let directory = std::hint::black_box(reopened.directory.as_ref().clone());
            let paths = std::hint::black_box(reopened.paths.as_ref().clone());
            legacy_clone_ms.push(start.elapsed().as_secs_f64() * 1000.);
            assert_eq!(directory.files.len(), first.directory.files.len());
            assert_eq!(paths.len(), first.paths.len());
        }
        let mut ui_path_extensions = std::collections::BTreeMap::new();
        for path in first.paths.keys() {
            if let Some((_, extension)) = path.rsplit_once('.')
                && matches!(
                    extension,
                    "png"
                        | "jpg"
                        | "jpeg"
                        | "webp"
                        | "svg"
                        | "ttf"
                        | "otf"
                        | "bmp"
                        | "gif"
                        | "nui"
                        | "lua"
                        | "txt"
                )
            {
                *ui_path_extensions.entry(extension).or_insert(0usize) += 1;
            }
        }
        let mut header_window_predictions = Vec::new();
        for window in [512u64, 4096, 8192, 65536] {
            let mut previous = None;
            let mut fills = 0u64;
            let mut bytes = 0u64;
            for record in &first.directory.records {
                let length = (first.ufs.header.physical_size - record.offset).min(12);
                let last = (record.offset + length - 1) / window;
                for page in record.offset / window..=last {
                    if previous != Some(page) {
                        fills += 1;
                        bytes += window.min(first.ufs.header.physical_size - page * window);
                        previous = Some(page);
                    }
                }
            }
            header_window_predictions.push(serde_json::json!({"window_bytes":window,"predicted_fills":fills,"predicted_requested_bytes":bytes}));
        }
        println!(
            "BANK_METADATA_PROFILE {}",
            serde_json::json!({
                "schema":1, "scope":"metadata-only; elapsed wall time; legacy column is reconstructed deep-clone cost",
                "file_bytes":std::fs::metadata(path).unwrap().len(),
                "members":first.directory.files.len(), "records":first.directory.records.len(), "lookup_paths":first.paths.len(),
                "cold_ms":cold_ms,"warm_ms":warm_ms,"legacy_clone_ms":legacy_clone_ms,
                "ui_path_extensions":ui_path_extensions,
                "header_window_predictions":header_window_predictions,"prediction_scope":"One aligned lookahead window, header-offset sequence only; no bank bytes read by prediction and no OS cache assumption",
                "namespace_shared":true,"lookup_map_shared":true,"decoded_payloads":0
            })
        );
    }

    #[test]
    fn clear_bank_and_program_load_with_native_namespaces() {
        let (bytes, xml) = clear_bank_fixture();
        let path =
            std::env::temp_dir().join(format!("kontra-clear-bank-{}.ufs", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut bank = Bank::open(&path).unwrap();
        assert_eq!(bank.program_namespace.len(), 39);
        assert_eq!(bank.programs(), ["preset.uvip"]);
        assert_eq!(Bank::catalog(&path).unwrap(), bank.programs());
        assert_eq!(
            bank.program("preset.uvip").unwrap(),
            (
                std::str::from_utf8(&xml).unwrap().to_owned(),
                "preset.uvip".to_owned()
            )
        );
        assert!(bank.directory.warnings.is_empty());
        use crate::ResourceError as E;
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "preset.uvip")
                .unwrap(),
            Some(xml.to_vec())
        );
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "missing.png")
                .unwrap(),
            None
        );
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "$Other.ufs/preset.uvip"),
            Err(E::InvalidPath)
        );
        let index = bank.paths["preset.uvip"].unwrap();
        Arc::make_mut(&mut bank.paths).insert("preset.uvip".into(), None);
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "preset.uvip"),
            Err(E::Ambiguous)
        );
        Arc::make_mut(&mut bank.paths).insert("preset.uvip".into(), Some(index));
        let member = &mut Arc::make_mut(&mut bank.directory).files[index];
        let size = member.size;
        member.size = (32 << 20) + 1;
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "preset.uvip"),
            Err(E::Limit)
        );
        Arc::make_mut(&mut bank.directory).files[index].size = size;
        let offset = bank.directory.files[index].offset;
        Arc::make_mut(&mut bank.directory).files[index].offset = u64::MAX;
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "preset.uvip"),
            Err(E::Corrupt)
        );
        Arc::make_mut(&mut bank.directory).files[index].offset = offset;
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            bank.ui_resource_result("preset.uvip", "preset.uvip"),
            Err(E::Read)
        );
    }

    #[test]
    fn audio_format_fallback_preserves_the_sample_and_rejects_ambiguity() {
        let mut directory = Directory {
            files: ["Samples/tone-C1.flac", "Other/tone-C1.flac"]
                .into_iter()
                .map(|path| Member {
                    record_offset: 0,
                    name: path.rsplit('/').next().unwrap().into(),
                    parent: None,
                    path: Some(path.into()),
                    size: 0,
                    offset: 0,
                    mode: Protection::Clear,
                    footer: Vec::new(),
                })
                .collect(),
            directories: Vec::new(),
            records: Vec::new(),
            warnings: Vec::new(),
            metadata_key: 0,
        };
        assert_eq!(
            resolve(&directory, "Samples/tone-C1.wav")
                .unwrap()
                .path
                .as_deref(),
            Some("Samples/tone-C1.flac")
        );
        assert!(resolve(&directory, "Samples/tone-C2.wav").is_err());
        assert!(resolve(&directory, "Absent/tone-C1.wav").is_err());
        assert!(resolve(&directory, "Samples/tone-C1.uvip").is_err());
        assert!(resolve(&directory, "tone-C1.wav").is_err());
        let mut member = directory.files[0].clone();
        member.path = Some("Samples/tone-C1.aif".into());
        member.name = "tone-C1.aif".into();
        directory.files.push(member.clone());
        assert!(resolve(&directory, "Samples/tone-C1.wav").is_err());
        member.path = Some("Samples/tone-C1.wav".into());
        member.name = "tone-C1.wav".into();
        directory.files.push(member);
        assert_eq!(
            resolve(&directory, "Samples/tone-C1.wav")
                .unwrap()
                .path
                .as_deref(),
            Some("Samples/tone-C1.wav")
        );
    }

    #[test]
    fn explicit_bank_volumes_are_rooted_and_cannot_cross_banks() {
        let (program, path) = resource_base(
            "Presets/Category/p.uvip",
            "$Authored.ufs/Scripts/./../IRs/space.aif",
            "Authored",
        )
        .unwrap();
        assert_eq!(program, "");
        assert_eq!(normalize(path).unwrap(), "IRs/space.aif");
        assert!(resource_base("Presets/p.uvip", "$Other.ufs/IRs/space.aif", "Authored").is_err());
        assert!(resource_base("Presets/p.uvip", "$Authored.ufs", "Authored").is_err());
        let (_, path) =
            resource_base("Presets/p.uvip", "$Authored.ufs/../escape.aif", "Authored").unwrap();
        assert!(normalize(path).is_err());
    }

    #[test]
    fn sample_contexts_preserve_raw_relative_order_and_explicit_authority() {
        use crate::script::SamplePath;
        let request = |path| SamplePath { path, script_directory: Some("Scripts/Engine") };
        assert_eq!(sample_candidates("Presets/Category/p.uvip", request("../Samples/音-C4.wav"), "Fixture").unwrap(),
            ["Scripts/Samples/音-C4.wav", "Presets/Samples/音-C4.wav", "Samples/音-C4.wav"]);
        assert_eq!(sample_candidates("Presets/p.uvip", request("tone.wav"), "Fixture").unwrap(),
            ["tone.wav", "Scripts/Engine/tone.wav", "Presets/tone.wav", "Samples/tone.wav"]);
        for path in ["/Samples/tone.wav", "$fIxTuRe.UFS/Samples/tone.wav", "$Fixture/Samples/tone.wav"] {
            assert_eq!(sample_candidates("Presets/p.uvip", request(path), "Fixture").unwrap(), ["Samples/tone.wav"]);
        }
        for path in ["$Other.ufs/Samples/tone.wav", "$Fixture.ufs/../escape.wav", "/../escape.wav", "C:/tone.wav", "a\0b.wav", "../../../../escape.wav"] {
            assert!(sample_candidates("Presets/p.uvip", request(path), "Fixture").is_err(), "{path:?}");
        }
    }

    #[test]
    fn sample_context_fallback_advances_only_absence_and_never_mixes_channel_roots() {
        use crate::script::SamplePath;
        use std::cell::RefCell;
        let member = Member { record_offset: 0, name: "tone.wav".into(), parent: None,
            path: Some("Samples/tone.wav".into()), size: 0, offset: 0, mode: Protection::Clear, footer: Vec::new() };
        let visited = RefCell::new(Vec::new());
        let request = SamplePath { path: "tone.wav", script_directory: Some("Scripts") };
        let result = sample_members("Presets/p.uvip", request, "Fixture", |path| {
            visited.borrow_mut().push(path.to_owned());
            if path=="Samples/tone.wav" { Ok(&member) } else { Err(MissingResource.into()) }
        }).unwrap();
        assert!(std::ptr::eq(result[0], &member));
        assert_eq!(*visited.borrow(), ["tone.wav", "Scripts/tone.wav", "Presets/tone.wav", "Samples/tone.wav"]);
        for error in ["Ambiguous UVI member path", "Permission denied", "corrupt source", "source identity changed"] {
            visited.borrow_mut().clear();
            let found = sample_members("Presets/p.uvip", request, "Fixture", |path| {
                visited.borrow_mut().push(path.to_owned());
                anyhow::bail!("{error}")
            });
            assert_eq!(found.unwrap_err().to_string(), error);
            assert_eq!(visited.borrow().len(), 1);
        }
        let bundle = SamplePath { path: "./*left.wav*right.wav", script_directory: Some("Scripts") };
        assert!(sample_members("Presets/p.uvip", bundle, "Fixture", |path| {
            if path=="Scripts/left.wav" || path=="Samples/right.wav" { Ok(&member) } else { Err(MissingResource.into()) }
        }).unwrap_err().downcast_ref::<MissingResource>().is_some());
    }

    #[test]
    fn dynamic_sample_paths_do_not_use_bare_names_and_casefold_format_stems() {
        let member = |path: &str| Member { record_offset: 0, name: path.rsplit('/').next().unwrap().into(), parent: None,
            path: Some(path.into()), size: 0, offset: 0, mode: Protection::Clear, footer: Vec::new() };
        let mut directory = Directory { files: vec![member("Samples/音-C4.flac"), member("Other/tone.wav")],
            directories: Vec::new(), records: Vec::new(), warnings: Vec::new(), metadata_key: 0 };
        assert!(resolve_at(&directory, "tone.wav", false).unwrap_err().downcast_ref::<MissingResource>().is_some());
        assert_eq!(resolve_at(&directory, "samples/音-c4.WAV", false).unwrap().path.as_deref(), Some("Samples/音-C4.flac"));
        directory.files.push(member("samples/音-c4.FLAC"));
        assert!(resolve_at(&directory, "Samples/音-C4.wav", false).unwrap_err().to_string().contains("Ambiguous"));
    }

    #[test]
    fn ui_candidates_keep_module_and_package_roots_in_order() {
        assert_eq!(
            ui_candidates(
                "Presets/Strings/p.uvip",
                "/Scripts/_ui/../Resources/knob.png"
            )
            .unwrap(),
            [
                "Scripts/Resources/knob.png",
                "Resources/knob.png",
                "knob.png"
            ]
        );
        assert_eq!(
            ui_candidates("Presets/Strings/p.uvip", "Resources/knob.png").unwrap(),
            ["Presets/Strings/Resources/knob.png", "Resources/knob.png"]
        );
        assert!(ui_candidates("Presets/p.uvip", "/Scripts/../../escape.png").is_err());
    }
    #[test]
    fn folded_suffix_index_preserves_unique_paths_and_case_collisions() {
        let mut paths: HashMap<String, Option<usize>> = [
            ("one/resources/pic.png".into(), Some(0)),
            ("two/resources/knob.png".into(), Some(1)),
        ]
        .into();
        assert_eq!(ui_suffix_index(&paths, "/resources/pic.png"), Ok(Some(0)));
        assert_eq!(ui_suffix_index(&paths, "/resources/absent.png"), Ok(None));
        paths.insert("three/resources/pic.png".into(), Some(2));
        assert_eq!(
            ui_suffix_index(&paths, "/resources/pic.png"),
            Err(crate::ResourceError::Ambiguous)
        );
        paths.remove("three/resources/pic.png");
        paths.insert("one/resources/pic.png".into(), None);
        assert_eq!(
            ui_suffix_index(&paths, "/resources/pic.png"),
            Err(crate::ResourceError::Ambiguous)
        );
    }
}
