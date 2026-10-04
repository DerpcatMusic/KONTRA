//! Feature-owned UVI inventory and private reader authority.
//! Shared identity types, scan policy and persistent index remain in `library`.

use super::{
    DEPTH, Library, Progress, Root, SKIP, Shelf, UviBank, UviPreset, UviSource,
    config_dir, natural, uvi_authority_dir,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Private process-local authority and metadata cache. No access values enter
/// Shelf, project state, Debug output or public diagnostics.
#[derive(Default)]
pub(super) struct UviCatalog {
    pub(super) reader_path: Option<PathBuf>,
    #[cfg(feature = "uvi")]
    pub(super) reader: Option<(PathBuf, u64, Option<std::time::SystemTime>, crate::uvi::cli::ReaderNamespaces)>,
    #[cfg(feature = "uvi")]
    pub(super) banks: BTreeMap<PathBuf, (u64, Option<std::time::SystemTime>, [u8; 16], String, Arc<UviBank>)>,
    #[cfg(feature = "uvi")]
    pub(super) order: std::collections::VecDeque<PathBuf>,
}

impl UviCatalog {
    #[cfg(feature = "uvi")]
    pub(super) fn authority(&mut self) -> Result<&crate::uvi::cli::ReaderNamespaces, &'static str> {
        let path = crate::uvi::access::reader_path(self.reader_path.as_deref())
            .map_err(|_| "No local UVI reader was found. Select a verified reader in settings.")?;
        let meta = std::fs::metadata(&path).map_err(|_| "The configured local UVI reader is unavailable.")?;
        if self.reader.as_ref().is_none_or(|r| r.0 != path || r.1 != meta.len() || r.2 != meta.modified().ok()) {
            let reader = crate::uvi::cli::ReaderNamespaces::open(&path)
                .map_err(|_| "The configured local UVI reader is unsupported.")?;
            self.reader = Some((path, meta.len(), meta.modified().ok(), reader));
            self.banks.clear();
            self.order.clear();
        }
        Ok(&self.reader.as_ref().unwrap().3)
    }

    pub(super) fn inventory(&mut self, path: &Path) -> Result<(String, Arc<UviBank>), &'static str> {
        #[cfg(not(feature = "uvi"))]
        {
            let _ = path;
            Err("UVI inventory requires a build with UVI support.")
        }
        #[cfg(feature = "uvi")]
        {
            if path.to_str().is_none() { return Err("UVI bank paths must be valid UTF-8 for project persistence."); }
            self.authority()?;
            let meta = std::fs::metadata(path).map_err(|_| "The UVI bank is unavailable.")?;
            let bank = crate::uvi::ufs::Ufs::open(path).map_err(|_| "The UVI bank header is unsupported or damaged.")?;
            if let Some((size, modified, uuid, name, cached)) = self.banks.get(path)
                && *size == meta.len() && *modified == meta.modified().ok() && *uuid == bank.header.uuid {
                self.order.retain(|old| old != path);
                self.order.push_back(path.to_owned());
                return Ok((name.clone(), cached.clone()));
            }
            let directory = bank.decode_directory(&self.reader.as_ref().unwrap().3.metadata)
                .map_err(|_| "The UVI bank directory could not be read.")?;
            let mut presets = Vec::new();
            let mut bytes = 0usize;
            for member in directory.files {
                let Some(member) = member.path.filter(|p| p.to_ascii_lowercase().ends_with(".uvip")) else { continue };
                let path_name = Path::new(&member);
                let name = path_name.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                let folder = path_name.parent().unwrap_or(Path::new("")).to_string_lossy().replace('/', " / ");
                let search = format!("{name} {folder} UVI").to_lowercase();
                bytes = bytes.saturating_add(path.as_os_str().len() + member.capacity() + name.capacity() + folder.capacity() + search.capacity() + std::mem::size_of::<UviPreset>() + std::mem::size_of::<Arc<UviPreset>>());
                if presets.len() >= 16_384 || bytes > 8 << 20 {
                    return Err("The UVI program inventory exceeds the catalog limit.");
                }
                presets.push(Arc::new(UviPreset { source: UviSource { bank: path.to_owned(), bank_uuid: bank.header.uuid, member }, name, folder, search }));
            }
            presets.sort_by_cached_key(|p| (natural(&p.source.member), p.source.member.clone()));
            presets.shrink_to_fit();
            bytes += path.as_os_str().len() + bank.header.bank_name.capacity() + std::mem::size_of::<UviBank>();
            let inventory = Arc::new(UviBank { presets, status: String::new(), bytes });
            // Cache only inventory, not open banks or decoded samples. Keep a
            // fixed number of banks even when roots change repeatedly.
            self.banks.remove(path);
            self.order.retain(|old| old != path);
            while self.banks.len() >= 256
                || self.banks.values().map(|b| b.4.bytes).sum::<usize>() + inventory.bytes > 64 << 20 {
                let Some(oldest) = self.order.pop_front() else { break };
                self.banks.remove(&oldest);
            }
            self.banks.insert(path.to_owned(), (meta.len(), meta.modified().ok(), bank.header.uuid, bank.header.bank_name.clone(), inventory.clone()));
            self.order.push_back(path.to_owned());
            Ok((bank.header.bank_name, inventory))
        }
    }

    #[cfg(feature = "uvi")]
    pub(super) fn scan(&mut self, roots: &[Root], shelf: &mut Shelf, progress: &Progress) {
        let mut seen = BTreeSet::new();
        'roots: for root in roots {
            let walk = walkdir::WalkDir::new(&root.path).max_depth(DEPTH + 1).follow_links(false)
                .into_iter().filter_entry(|e| e.depth() == 0 || !e.file_type().is_dir()
                    || !SKIP.contains(&e.file_name().to_string_lossy().to_ascii_lowercase().as_str()));
            for entry in walk {
                if progress.canceled() { return; }
                let Ok(entry) = entry else { continue };
                if !entry.file_type().is_file() || !entry.path().extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")) { continue; }
                let path = std::fs::canonicalize(entry.path()).unwrap_or_else(|_| entry.path().to_owned());
                if !seen.insert(path.clone()) { continue; }
                if seen.len() > 256 { break 'roots; }
                let (name, mut inventory) = match self.inventory(&path) {
                    Ok(found) => found,
                    Err(status) => (path.file_stem().unwrap_or_default().to_string_lossy().into_owned(), Arc::new(UviBank { status: status.into(), ..Default::default() })),
                };
                if shelf.uvi.values().map(|b| b.bytes).sum::<usize>() + inventory.bytes > 64 << 20 {
                    inventory = Arc::new(UviBank { status: "The combined UVI inventory exceeds the catalog limit.".into(), ..Default::default() });
                }
                if progress.canceled() { return; }
                shelf.libraries.push(Library { dir: path.clone(), name, instruments: inventory.presets.len(), ..Default::default() });
                shelf.uvi.insert(path, inventory);
            }
        }
        // Sorting/unique display names do not change bank/member identity.
        let mut prepared = Shelf::new(std::mem::take(&mut shelf.libraries));
        prepared.uvi = std::mem::take(&mut shelf.uvi);
        prepared.snapshots = std::mem::take(&mut shelf.snapshots);
        prepared.per_root = std::mem::take(&mut shelf.per_root);
        *shelf = prepared;
    }

    pub(super) fn inspect(&mut self, source: &UviSource) -> Result<(), &'static str> {
        #[cfg(not(feature = "uvi"))]
        { let _ = source; Err("UVI playback requires a build with UVI support.") }
        #[cfg(feature = "uvi")]
        {
            let config = self.worker_config(source, 48_000)?;
            let library = crate::uvi::library::Library::open(&source.bank, &config.metadata_namespace, config.content_key)
                .map_err(|_| "The selected UVI bank could not be opened.")?;
            let loaded = library.program(&source.member, &config.program_namespace)
                .map_err(|_| "The selected UVI program could not be decoded.")?;
            if !crate::uvi::playback::preflight(&loaded.program).is_empty() {
                return Err("This UVI program requires unsupported playback features.");
            }
            Ok(())
        }
    }

    #[cfg(feature = "uvi")]
    pub(super) fn worker_config(&mut self, source: &UviSource, sample_rate: u32) -> Result<crate::uvi::worker::StartConfig, &'static str> {
        let bank = crate::uvi::ufs::Ufs::open(&source.bank)
            .map_err(|_| "The selected UVI bank could not be opened.")?;
        if bank.header.uuid != source.bank_uuid { return Err("The UVI bank changed. Rescan before selecting its programs."); }
        let reader = self.authority()?;
        let directory = bank.decode_directory(&reader.metadata)
            .map_err(|_| "The selected UVI bank directory could not be read.")?;
        if !directory.files.iter().any(|member| member.path.as_deref() == Some(source.member.as_str())) {
            return Err("The selected UVI program is no longer in this bank. Rescan its library.");
        }
        let state = if directory.files.iter().any(|member| member.mode == 2) {
            let store = uvi_authority_dir(std::env::var_os("KONTRA_UVI_AUTHORITY_DIR"), config_dir())
                .ok_or("The private UVI access store is unavailable.")?;
            crate::uvi::access::ensure_content_state(&source.bank, &bank, &directory, &store)
                .map_err(|error| {
                    crate::diagnostics::event(crate::diagnostics::LogLevel::Error, "uvi", "uvi_local_access_failed",
                        serde_json::json!({"path":source.bank, "member":source.member,
                            "stage":"local_access", "reason":crate::uvi::access::failure_reason(&error)}));
                    "This UVI bank could not be prepared automatically. Open Logs for the cause."
                })?
        } else { None };
        Ok(crate::uvi::worker::StartConfig {
            bank: source.bank.clone(), expected_bank_uuid: Some(source.bank_uuid), member: source.member.clone(),
            metadata_namespace: reader.metadata.clone(), program_namespace: reader.program.clone(),
            content_key: state.as_ref().map(|state| state.key), content_bank: state.and_then(|state| state.bank), sample_rate,
        })
    }
}

/// Cache bindings follow the same configured/environment/discovered reader as playback.
pub(super) fn effective_uvi_reader(settings: &super::Settings) -> Option<PathBuf> {
    crate::uvi::access::reader_path(settings.uvi_reader.as_deref()).ok()
}

impl super::Scanner {
    /// Loader only: authority is transferred directly to the dedicated worker.
    #[cfg(feature = "uvi")]
    pub(crate) fn uvi_worker_config(&self, source: &UviSource, sample_rate: u32) -> Result<crate::uvi::worker::StartConfig, &'static str> {
        let mut catalog = super::lock(&self.uvi);
        catalog.reader_path = self.settings().uvi_reader.clone();
        catalog.worker_config(source, sample_rate)
    }
}
