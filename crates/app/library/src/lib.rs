//! Where the player's libraries are, what they are called and what they
//! hold: the browser's data, found by naming a folder and nothing else.
//!
//! A root is a folder of libraries, searched a few levels down, or one folder
//! (or `.ufs` bank) that is a library. A folder is a library when it holds a
//! `.nicnt` (registered), an `Instruments` or `Multis` folder, presets beside
//! a `Samples` folder or a monolith (`.nkx`/`.nkc`/`.nkr`), or, below the
//! root, presets of its own. Loose `.ufs` banks at a root are libraries of
//! their own. The search stops at a library, skips sample folders, follows
//! symlinks safely with canonical-directory cycle guards, and gives up on a
//! folder of many audio files with no presets.
//!
//! Each is named from its `.nicnt` product, else its folder name cleaned of
//! versions, brackets and underscores; the vendor comes from the product, a
//! bracketed name or the vendor folder it sits in.
//!
//! The crate reads no instrument: a bank's program list comes from the
//! lister the host passes (`kontra_format_uvi::programs`), and what a scan
//! reads once is kept in a [`Cache`] keyed by file size and time.

pub mod kontakt;
pub mod presets;

mod cache;
mod folder;
mod name;
mod prefs;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub use cache::Cache;
pub use folder::{Folder, natural};
pub use kontra_format::{ProtectionStatus, ProtectionSystem};
pub use name::{clean_name, product};
pub use prefs::Prefs;

/// A folder or `.ufs` bank the player added.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Root {
    pub path: PathBuf,
    /// The folder is one library, rather than holding libraries.
    #[serde(default)]
    pub single: bool,
}

/// Which player a library was made for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Format {
    #[default]
    Kontakt,
    Uvi,
}

/// One library found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    /// Its folder, or the bank file for a loose `.ufs`.
    pub dir: PathBuf,
    /// Its name, unique among the catalog's libraries.
    pub name: String,
    pub vendor: String,
    pub format: Format,
    /// It has a `.nicnt`; otherwise it was recognised from its folders.
    pub registered: bool,
    /// Whether this library is open or guarded by DRM (e.g., PACE/iLok-locked UFS).
    pub protection: ProtectionStatus,
    /// Its instruments and snapshots, sorted naturally by path. A UVI
    /// program is `bank.ufs/Folder/Name.uvip`.
    pub presets: Vec<PathBuf>,
    /// How many multis (`.nkm`) it holds, among `presets`: one fills a
    /// part for each of its slots.
    pub multis: usize,
    /// Pictures that may be its banner, best first: `wallpaper.png`, a
    /// library-browser picture, the `.nicnt`/`.nkr` (which holds pictures),
    /// then any picture named cover, artwork, banner, logo or background,
    /// then a UVI bank (which picks its own), then the rest. A lone bank's
    /// are its `.png`/`.jpg` sidecars, then the bank itself. Decoding and
    /// the size check are the viewer's.
    pub art: Vec<PathBuf>,
}

impl Library {
    /// Its instrument tree, with counts.
    #[must_use]
    pub fn tree(&self) -> Folder {
        Folder::tree(&self.dir, self.presets.iter().map(PathBuf::as_path))
    }
}

/// Something under a root that could not be read (or is DRM-protected), and why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Issue {
    pub path: PathBuf,
    pub reason: String,
    pub protection: ProtectionStatus,
}

/// Every library a scan found, sorted by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub libraries: Vec<Library>,
    pub issues: Vec<Issue>,
}

impl Catalog {
    /// The library `path` is in: the nearest library folder above it.
    #[must_use]
    pub fn of(&self, path: &Path) -> Option<&Library> {
        path.ancestors()
            .skip(1)
            .find_map(|at| self.libraries.iter().find(|l| l.dir == at))
    }
}

/// How far a scan is, and a way to stop it from another thread.
#[derive(Debug, Default)]
pub struct Progress {
    pub folders: AtomicUsize,
    pub found: AtomicUsize,
    pub cancel: AtomicBool,
}

impl Progress {
    fn canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Lists a bank's programs as paths inside it, or says why it can't.
pub type Programs<'a> = &'a dyn Fn(&Path) -> Result<Vec<String>, String>;

/// Result of inspecting a bank (`.ufs` or `.sf2`): its preset member paths
/// together with its structured [`ProtectionStatus`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BankListing {
    pub presets: Vec<String>,
    pub protection: ProtectionStatus,
}

/// Failure inspecting a bank, separating DRM protection from corrupt/I/O errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BankError {
    pub reason: String,
    pub protection: ProtectionStatus,
}

impl BankError {
    #[must_use]
    pub fn failed(reason: impl Into<String>) -> Self {
        let reason = reason.into();
        let protection = infer_protection(&reason);
        Self { reason, protection }
    }

    #[must_use]
    pub fn protected(status: ProtectionStatus) -> Self {
        Self {
            reason: status.to_string(),
            protection: status,
        }
    }
}

/// Lists a bank's programs with structured [`ProtectionStatus`] reporting.
pub type BankInspector<'a> = &'a dyn Fn(&Path) -> Result<BankListing, BankError>;

/// How many levels under a root libraries are looked for.
const DEPTH: usize = 4;
/// A folder with this many audio files and no presets holds samples only.
const SAMPLE_ONLY: usize = 64;
/// Folders a library keeps no presets in, never searched for libraries.
const SKIP: [&str; 6] = [
    "samples",
    "documentation",
    "documents",
    "docs",
    "resources",
    "data",
];

/// Every library in `roots` and its presets; `None` once canceled.
pub fn scan(
    roots: &[Root],
    progress: &Progress,
    cache: &mut Cache,
    programs: Programs<'_>,
) -> Option<Catalog> {
    let inspect = |bank: &Path| match programs(bank) {
        Ok(presets) => Ok(BankListing {
            presets,
            protection: ProtectionStatus::Open,
        }),
        Err(reason) => Err(BankError::failed(reason)),
    };
    scan_with_inspector(roots, progress, cache, &inspect)
}

/// Like [`scan`], but receives structured [`ProtectionStatus`] per bank from
/// `inspect` so DRM-protected banks carry an explicit protection badge state.
pub fn scan_with_inspector(
    roots: &[Root],
    progress: &Progress,
    cache: &mut Cache,
    inspect: BankInspector<'_>,
) -> Option<Catalog> {
    let mut issues = Vec::new();
    let mut libraries = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let founds = detect(root, progress, &mut issues);
        // Every library found under this root, nested ones included: a library
        // inside another is catalogued as itself, so the outer one's walk stops
        // at it rather than listing its presets a second time
        // ([`visit_within`]).
        let dirs: BTreeSet<PathBuf> = founds.iter().map(|f| path_identity(&f.dir)).collect();
        for found in founds {
            let key = path_identity(&found.dir);
            if !seen.insert(key) {
                continue;
            }
            let library = catalog(found, &dirs, progress, cache, inspect, &mut issues);
            if progress.canceled() {
                return None;
            }
            libraries.extend(library);
        }
    }
    cache.forget_unseen();
    Some(Catalog {
        libraries: unique(libraries),
        issues,
    })
}

/// Infers a structured [`ProtectionStatus`] from a plain string error returned
/// by legacy [`Programs`] callbacks.
fn infer_protection(reason: &str) -> ProtectionStatus {
    let lower = reason.to_ascii_lowercase();
    let diagnostic = lower
        .rsplit(": ")
        .find(|part| part.starts_with("protected ("));
    if diagnostic.is_some_and(|part| {
        matches!(
            part.split_once(')').map(|(kind, _)| kind),
            Some(
                "protected (pace / ilok"
                    | "protected (pace/ilok"
                    | "protected (pace"
                    | "protected (ilok"
            )
        )
    }) || lower.contains("requires pace/ilok license")
    {
        ProtectionStatus::protected(ProtectionSystem::PaceIlok, reason)
    } else if lower.contains("native access") || lower.contains("no .nicnt access") {
        ProtectionStatus::protected(ProtectionSystem::NativeAccess, reason)
    } else if lower.starts_with("protected (") {
        ProtectionStatus::protected(ProtectionSystem::Unknown("DRM".into()), reason)
    } else {
        ProtectionStatus::Open
    }
}

fn path_identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A library folder or loose bank found under a root.
struct Found {
    dir: PathBuf,
    nicnt: Option<PathBuf>,
    /// The folder it sits in below the root, which names its vendor.
    vendor: Option<String>,
}

/// What a folder holds, from one listing.
#[derive(Default)]
struct Listing {
    nicnt: Option<PathBuf>,
    /// An `Instruments` or `Multis` folder.
    instruments: bool,
    samples: bool,
    monolith: bool,
    presets: usize,
    banks: Vec<PathBuf>,
    audio: usize,
    folders: Vec<PathBuf>,
}

fn ext(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn list(dir: &Path, issues: &mut Vec<Issue>) -> Listing {
    let mut out = Listing::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            issues.push(issue(dir, e));
            return out;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let (is_dir, is_file) = if kind.is_symlink() {
            match std::fs::metadata(&path) {
                Ok(meta) => (meta.is_dir(), meta.is_file()),
                Err(e) => {
                    issues.push(issue(&path, format!("broken symlink: {e}")));
                    continue;
                }
            }
        } else {
            (kind.is_dir(), kind.is_file())
        };
        if is_dir {
            match name.as_str() {
                "instruments" | "multis" => out.instruments = true,
                "samples" => out.samples = true,
                _ => {}
            }
            out.folders.push(path);
            continue;
        }
        if !is_file {
            continue;
        }
        match ext(&path).as_str() {
            "nicnt" => out.nicnt = out.nicnt.take().or(Some(path)),
            "nkx" | "nkc" | "nkr" => out.monolith = true,
            "ufs" | "sf2" => {
                out.presets += 1;
                out.banks.push(path);
            }
            "nki" | "nkm" | "nksn" | "uvip" | "sfz" | "dspreset" | "kontra" => out.presets += 1,
            "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => out.audio += 1,
            _ => {}
        }
    }
    out.folders.sort();
    out.banks.sort();
    out
}

fn issue(path: &Path, reason: impl ToString) -> Issue {
    let reason = reason.to_string();
    let protection = infer_protection(&reason);
    Issue {
        path: path.to_path_buf(),
        reason,
        protection,
    }
}

fn issue_with_protection(
    path: &Path,
    reason: impl ToString,
    protection: ProtectionStatus,
) -> Issue {
    Issue {
        path: path.to_path_buf(),
        reason: reason.to_string(),
        protection,
    }
}

/// The library folders and loose banks in `root`.
fn detect(root: &Root, progress: &Progress, issues: &mut Vec<Issue>) -> Vec<Found> {
    let dir = root.path.clone();
    let mut out = Vec::new();
    match std::fs::metadata(&dir) {
        Err(e) => issues.push(issue(&dir, e)),
        Ok(m) if m.is_file() && matches!(ext(&dir).as_str(), "ufs" | "sf2") => out.push(Found {
            dir,
            nicnt: None,
            vendor: None,
        }),
        Ok(m) if !m.is_dir() => issues.push(issue(&dir, "not a library folder or UFS/SF2 bank")),
        Ok(_) if root.single => {
            let nicnt = list(&dir, issues).nicnt;
            out.push(Found {
                dir,
                nicnt,
                vendor: None,
            });
        }
        Ok(_) => {
            let mut visited = HashMap::new();
            let mut found_libs = HashMap::new();
            visit_within(
                &dir,
                0,
                None,
                false,
                false,
                &mut visited,
                &mut found_libs,
                &mut out,
                progress,
                issues,
            );
        }
    }
    out
}

/// Visits `dir`, `within` saying whether `dir` already sits inside a library.
/// Tracks the shallowest depth and best route independently per
/// `(canonical_dir, within)`. Revisit only when either improves, preserving a
/// shallower traversal budget and allowing real paths to replace aliases.
/// Discovered libraries are deduplicated separately in `found_libs`.
#[allow(clippy::too_many_arguments)]
fn visit_within(
    dir: &Path,
    depth: usize,
    vendor: Option<String>,
    within: bool,
    through_symlink: bool,
    visited: &mut HashMap<(PathBuf, bool), (usize, bool)>,
    found_libs: &mut HashMap<PathBuf, (usize, bool, usize)>,
    out: &mut Vec<Found>,
    progress: &Progress,
    issues: &mut Vec<Issue>,
) {
    if progress.canceled() {
        return;
    }
    let canonical = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let is_symlink = through_symlink
        || std::fs::symlink_metadata(dir)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false);
    let key = (canonical.clone(), within);
    match visited.get_mut(&key) {
        Some((prev_depth, prev_symlink)) => {
            if depth >= *prev_depth && (is_symlink || !*prev_symlink) {
                return;
            }
            *prev_depth = (*prev_depth).min(depth);
            *prev_symlink &= is_symlink;
        }
        None => {
            visited.insert(key, (depth, is_symlink));
        }
    }
    progress.folders.fetch_add(1, Ordering::Relaxed);
    let l = list(dir, issues);
    // Loose banks at the root are libraries of their own, not the root's presets.
    let presets = l.presets - if depth == 0 { l.banks.len() } else { 0 };
    let strong = l.nicnt.is_some() || l.instruments || (presets > 0 && (l.samples || l.monolith));
    // Below the root, a folder of presets; at the root, stray presets must not
    // hide the libraries beside them. Inside a library, only a `.nicnt` makes
    // a folder a library of its own.
    let library = if within {
        l.nicnt.is_some()
    } else {
        strong || (depth > 0 && l.presets > 0)
    };
    if library {
        let candidate = Found {
            dir: dir.into(),
            nicnt: l.nicnt,
            vendor: vendor.clone(),
        };
        match found_libs.get_mut(&canonical) {
            Some((idx, was_symlink, prev_depth)) => {
                if (is_symlink, depth) < (*was_symlink, *prev_depth) {
                    if let Some(slot) = out.get_mut(*idx) {
                        *slot = candidate;
                    }
                    *was_symlink = is_symlink;
                    *prev_depth = depth;
                }
            }
            None => {
                progress.found.fetch_add(1, Ordering::Relaxed);
                let idx = out.len();
                out.push(candidate);
                found_libs.insert(canonical, (idx, is_symlink, depth));
            }
        }
    } else if depth == 0 {
        for bank in l.banks {
            let bank_canon = path_identity(&bank);
            let is_symlink = std::fs::symlink_metadata(&bank)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            let candidate = Found {
                dir: bank,
                nicnt: None,
                vendor: vendor.clone(),
            };
            match found_libs.get_mut(&bank_canon) {
                Some((idx, was_symlink, prev_depth)) => {
                    if (is_symlink, depth) < (*was_symlink, *prev_depth) {
                        if let Some(slot) = out.get_mut(*idx) {
                            *slot = candidate;
                        }
                        *was_symlink = is_symlink;
                        *prev_depth = depth;
                    }
                }
                None => {
                    progress.found.fetch_add(1, Ordering::Relaxed);
                    let idx = out.len();
                    out.push(candidate);
                    found_libs.insert(bank_canon, (idx, is_symlink, depth));
                }
            }
        }
    }
    if depth >= DEPTH || (l.audio >= SAMPLE_ONLY && l.presets == 0) {
        return;
    }
    // Below the root, the first folder that is not a library is its vendor.
    let vendor = if library {
        None
    } else {
        vendor.or_else(|| (depth > 0).then(|| clean_name(&file_name(dir)).0))
    };
    for folder in &l.folders {
        if !SKIP.contains(&file_name(folder).to_lowercase().as_str()) {
            visit_within(
                folder,
                depth + 1,
                vendor.clone(),
                within || library,
                is_symlink,
                visited,
                found_libs,
                out,
                progress,
                issues,
            );
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// What a library folder holds: its presets (banks opened through
/// `inspect`) and its banner candidates, from one walk that skips samples.
fn catalog(
    found: Found,
    nested: &BTreeSet<PathBuf>,
    progress: &Progress,
    cache: &mut Cache,
    inspect: BankInspector<'_>,
    issues: &mut Vec<Issue>,
) -> Option<Library> {
    let mut presets = Vec::new();
    let mut art = Vec::new();
    let mut protection = ProtectionStatus::Open;
    let bank = found.dir.is_file();
    if bank {
        bank_presets(
            &found.dir,
            cache,
            inspect,
            issues,
            &mut presets,
            &mut protection,
        );
        art.extend(
            ["png", "jpg", "jpeg"]
                .iter()
                .map(|e| found.dir.with_extension(e))
                .filter(|p| p.is_file()),
        );
        if ext(&found.dir) == "ufs" {
            art.push(found.dir.clone());
        }
    } else {
        let mut state = Walk {
            nested,
            progress,
            cache,
            inspect,
            issues,
            protection: &mut protection,
        };
        walk(&found.dir, &mut state, &mut presets, &mut art);
        art.sort_by_cached_key(|p| (art_rank(p), p.clone()));
    }
    let multis = presets.iter().filter(|p| ext(p) == "nkm").count();
    if presets.is_empty() && !protection.is_protected() {
        return None;
    }
    presets.sort_by_cached_key(|p| {
        let inside = p.strip_prefix(&found.dir).unwrap_or(p);
        (natural(&inside.to_string_lossy()), p.clone())
    });
    let stem = if bank {
        found
            .dir
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    } else {
        file_name(&found.dir)
    };
    let (folder_name, bracket) = clean_name(&stem);
    let (name, company) = found
        .nicnt
        .as_deref()
        .and_then(|p| cache.product(p))
        .unwrap_or_default();
    let vendor = [company, bracket, found.vendor.unwrap_or_default()]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let uvi_bank = (bank && ext(&found.dir) == "ufs")
        || (presets.is_empty() && art.iter().any(|p| ext(p) == "ufs"));
    let uvi = presets.iter().filter(|p| ext(p) == "uvip").count();
    Some(Library {
        name: if name.is_empty() { folder_name } else { name },
        vendor,
        format: if uvi_bank || uvi * 2 > presets.len() {
            Format::Uvi
        } else {
            Format::Kontakt
        },
        registered: found.nicnt.is_some(),
        protection,
        multis,
        presets,
        art,
        dir: found.dir,
    })
}

/// What one library's walk shares: where to report, how to read a bank, and
/// which folders are libraries of their own.
struct Walk<'a> {
    nested: &'a BTreeSet<PathBuf>,
    progress: &'a Progress,
    cache: &'a mut Cache,
    inspect: BankInspector<'a>,
    issues: &'a mut Vec<Issue>,
    protection: &'a mut ProtectionStatus,
}

/// The presets and banner candidates under `dir`, skipping sample folders and
/// the folders of libraries nested inside it (`state.nested`). Follows
/// symlinks safely with canonical-path cycle tracking, and prefers real regular
/// file paths over symlink aliases when deduplicating canonical targets.
fn walk(dir: &Path, state: &mut Walk<'_>, presets: &mut Vec<PathBuf>, art: &mut Vec<PathBuf>) {
    let mut visited: HashMap<PathBuf, bool> = HashMap::new();
    let mut files: HashMap<PathBuf, (bool, PathBuf)> = HashMap::new();
    let mut artwork: HashMap<PathBuf, (bool, PathBuf)> = HashMap::new();
    let mut stack = vec![(dir.to_path_buf(), false)];
    while let Some((dir, through_symlink)) = stack.pop() {
        if state.progress.canceled() {
            return;
        }
        let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if visited
            .get(&canonical)
            .is_some_and(|&was_symlink| !was_symlink || through_symlink)
        {
            continue;
        }
        visited.insert(canonical, through_symlink);
        state.progress.folders.fetch_add(1, Ordering::Relaxed);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                state.issues.push(issue(&dir, e));
                continue;
            }
        };
        let mut dir_entries: Vec<_> = entries.flatten().collect();
        dir_entries.sort_by_key(std::fs::DirEntry::path);
        for entry in dir_entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            let is_symlink = kind.is_symlink();
            let (is_dir, is_file) = if is_symlink {
                match std::fs::metadata(&path) {
                    Ok(meta) => (meta.is_dir(), meta.is_file()),
                    Err(e) => {
                        state
                            .issues
                            .push(issue(&path, format!("broken symlink: {e}")));
                        continue;
                    }
                }
            } else {
                (kind.is_dir(), kind.is_file())
            };
            if is_dir {
                if name != "samples" && !state.nested.contains(&path_identity(&path)) {
                    stack.push((path, through_symlink || is_symlink));
                }
                continue;
            }
            if !is_file {
                continue;
            }
            match ext(&path).as_str() {
                "nki" | "nkm" | "nksn" | "uvip" | "sfz" | "dspreset" | "kontra" | "ufs" | "sf2" => {
                    let alias = through_symlink || is_symlink;
                    let id = path_identity(&path);
                    match files.get_mut(&id) {
                        Some((was_alias, stored)) if *was_alias && !alias => {
                            *stored = path;
                            *was_alias = false;
                        }
                        Some(_) => {}
                        None => {
                            files.insert(id, (alias, path));
                        }
                    }
                }
                "png" | "jpg" | "jpeg" | "nicnt" | "nkr" => {
                    let alias = through_symlink || is_symlink;
                    let id = path_identity(&path);
                    match artwork.get_mut(&id) {
                        Some((was_alias, stored)) if *was_alias && !alias => {
                            *stored = path;
                            *was_alias = false;
                        }
                        Some(_) => {}
                        None => {
                            artwork.insert(id, (alias, path));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    art.extend(artwork.into_values().map(|(_, path)| path));
    let mut files: Vec<PathBuf> = files.into_values().map(|(_, path)| path).collect();
    files.sort();
    let mut banks = Vec::new();
    for path in files {
        match ext(&path).as_str() {
            "ufs" => banks.push((path, true)),
            "sf2" => banks.push((path, false)),
            _ => presets.push(path),
        }
    }
    for (path, is_ufs) in banks {
        if state.progress.canceled() {
            return;
        }
        bank_presets(
            &path,
            &mut *state.cache,
            state.inspect,
            &mut *state.issues,
            presets,
            &mut *state.protection,
        );
        if is_ufs {
            art.push(path);
        }
    }
}

fn bank_presets(
    bank: &Path,
    cache: &mut Cache,
    inspect: BankInspector<'_>,
    issues: &mut Vec<Issue>,
    presets: &mut Vec<PathBuf>,
    library_protection: &mut ProtectionStatus,
) {
    match cache.bank_listing(bank, inspect) {
        Ok(listing) => {
            presets.extend(
                listing
                    .presets
                    .iter()
                    .map(|m| bank.join(m.replace('\\', "/"))),
            );
            if listing.protection.is_protected() {
                issues.push(issue_with_protection(
                    bank,
                    listing.protection.to_string(),
                    listing.protection.clone(),
                ));
                if library_protection.is_open() {
                    *library_protection = listing.protection;
                }
            }
        }
        Err(err) => {
            if err.protection.is_protected() && library_protection.is_open() {
                *library_protection = err.protection.clone();
            }
            issues.push(issue_with_protection(bank, err.reason, err.protection));
        }
    }
}

/// Where a picture stands among a library's banner candidates; lower first.
fn art_rank(path: &Path) -> usize {
    let name = file_name(path).to_ascii_lowercase();
    match ext(path).as_str() {
        _ if name == "wallpaper.png" => 0,
        _ if name.contains("libbrowser") => 1,
        "nicnt" => 2,
        "nkr" => 3,
        "ufs" => 10,
        _ => {
            5 + ["cover", "artwork", "banner", "logo", "background"]
                .iter()
                .position(|word| name.contains(word))
                .unwrap_or(6)
        }
    }
}

/// `libraries` sorted by name, each name made unique: the vendor in
/// brackets first, then a number.
fn unique(mut libraries: Vec<Library>) -> Vec<Library> {
    libraries.sort_by_cached_key(|l| (natural(&l.name), l.dir.clone()));
    let mut taken = BTreeSet::new();
    for library in &mut libraries {
        let mut name = library.name.clone();
        if taken.contains(&name) && !library.vendor.is_empty() {
            name = format!("{} ({})", library.name, library.vendor);
        }
        let mut n = 2;
        while taken.contains(&name) {
            name = format!("{} {n}", library.name);
            n += 1;
        }
        taken.insert(name.clone());
        library.name = name;
    }
    libraries
}
