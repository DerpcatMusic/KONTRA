//! Where the player's libraries are, what they are called and what they
//! hold: the browser's data, found by naming a folder and nothing else.
//!
//! A root is a folder of libraries, searched at any depth, or one folder
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

pub mod discover;
pub mod index;
pub mod kontakt;
pub mod presets;
pub mod selector;

mod cache;
mod folder;
mod name;
mod prefs;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::PoisonError;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub use cache::Cache;
pub use folder::{Folder, natural};
pub use index::{identity, stale};
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
    /// Found by [`discover`], not added by the player; never saved.
    #[serde(skip)]
    pub auto: bool,
}

/// Which player a library was made for.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Format {
    #[default]
    Kontakt,
    Uvi,
}

/// One library found.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Library {
    /// Who it is, whatever folder it sits in ([`identity`]).
    pub id: String,
    /// Why it is not where the index last saw it ("not found", "can't read:
    /// permission denied"…); its presets stay listed.
    pub offline: Option<String>,
    /// Folders it was found in before, oldest first.
    pub moved_from: Vec<PathBuf>,
    /// Every folder its preset walk listed (or the bank file), with its
    /// modification time: unchanged, the next scan does not walk it again.
    pub stamps: Vec<(PathBuf, u128)>,
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
    /// The I/O error behind it, when there was one: tells "permission
    /// denied" from "not found".
    pub kind: Option<std::io::ErrorKind>,
}

/// How one root's scan went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootStatus {
    pub path: PathBuf,
    pub auto: bool,
    /// Libraries catalogued from it (not counting ones another root listed).
    pub libraries: usize,
    /// Why the root itself could not be read.
    pub error: Option<Issue>,
    /// Its modification time when scanned ([`stale`]).
    pub stamp: Option<u128>,
    /// Files listed under it this scan (an unchanged library is not listed).
    pub files: usize,
    /// Presets catalogued from it, by format, largest first.
    pub recognized: Vec<(&'static str, usize)>,
    /// What was passed over, by reason: "other file types" (not an
    /// instrument, bank, sample or picture), "unreadable", "protected",
    /// "parse error", "duplicate" (a library another root listed).
    pub skipped: Vec<(&'static str, usize)>,
}

impl RootStatus {
    /// One line: "3 libraries found · presets 40 (UVI 38, SFZ 2) · files
    /// listed 120 · skipped: other file types 70, protected 1", or "can't
    /// read: permission denied"…
    #[must_use]
    pub fn summary(&self) -> String {
        use std::io::ErrorKind;
        let mut line = match &self.error {
            Some(e) => {
                return match e.kind {
                    Some(ErrorKind::PermissionDenied) => "can't read: permission denied".into(),
                    Some(ErrorKind::NotFound) => "not found".into(),
                    _ => format!("can't read: {}", e.reason),
                };
            }
            None if self.libraries == 0 => "0 libraries \u{2014} nothing matched".to_owned(),
            None if self.libraries == 1 => "1 library found".to_owned(),
            None => format!("{} libraries found", self.libraries),
        };
        let list = |counts: &[(&str, usize)]| {
            let parts: Vec<String> = counts.iter().map(|(k, n)| format!("{k} {n}")).collect();
            parts.join(", ")
        };
        let presets: usize = self.recognized.iter().map(|(_, n)| n).sum();
        if presets > 0 {
            line += &format!(" \u{b7} presets {presets} ({})", list(&self.recognized));
        }
        if self.files > 0 {
            line += &format!(" \u{b7} files listed {}", self.files);
        }
        if !self.skipped.is_empty() {
            line += &format!(" \u{b7} skipped: {}", list(&self.skipped));
        }
        line
    }

    /// What the player can do about an unreadable root: the Flatpak
    /// override when sandboxed (`flatpak` is the app id), else a hint.
    #[must_use]
    pub fn hint(&self, flatpak: Option<&str>) -> Option<String> {
        use std::io::ErrorKind;
        let kind = self.error.as_ref()?.kind?;
        if !matches!(kind, ErrorKind::PermissionDenied | ErrorKind::NotFound) {
            return None;
        }
        if let Some(app) = flatpak {
            return Some(discover::flatpak_hint(&self.path, app));
        }
        (kind == ErrorKind::PermissionDenied).then(|| {
            "Check the folder's permissions; an NTFS/exFAT drive needs mounting with \
             uid=<you> (or the permissions option)."
                .into()
        })
    }
}

/// Every library a scan found, sorted by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub libraries: Vec<Library>,
    pub issues: Vec<Issue>,
    /// Each root scanned, in order, and how it went.
    pub roots: Vec<RootStatus>,
}

impl Catalog {
    /// The library known as `id`.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Library> {
        self.libraries.iter().find(|l| l.id == id)
    }

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
    /// Files listed, and of those, files of other types.
    pub files: AtomicUsize,
    pub other: AtomicUsize,
    /// The root being walked now.
    pub root: std::sync::Mutex<PathBuf>,
    /// Each library as it is catalogued, for the browser to show before the
    /// scan ends; the browser takes them.
    pub partial: std::sync::Mutex<Vec<Library>>,
    /// Held while an instrument loads ([`Progress::pause`]).
    paused: (std::sync::Mutex<bool>, std::sync::Condvar),
}

impl Progress {
    /// Whether the scan is canceled. While it is paused, waits first.
    fn canceled(&self) -> bool {
        let (paused, resume) = &self.paused;
        let held = paused.lock().unwrap_or_else(PoisonError::into_inner);
        let _held = resume
            .wait_while(held, |p| *p && !self.cancel.load(Ordering::Relaxed))
            .unwrap_or_else(PoisonError::into_inner);
        self.cancel.load(Ordering::Relaxed)
    }

    /// Holds the scan at its next check while `on`, so an instrument load
    /// has the disk to itself (one HDD serves both badly).
    pub fn pause(&self, on: bool) {
        *self.paused.0.lock().unwrap_or_else(PoisonError::into_inner) = on;
        self.paused.1.notify_all();
    }

    /// Stops the scan at its next check, paused or not.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.pause(false);
    }

    /// The libraries catalogued since the last call.
    pub fn take_partial(&self) -> Vec<Library> {
        std::mem::take(&mut self.partial.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn catalogued(&self, library: &Library) {
        (self.partial.lock().unwrap_or_else(PoisonError::into_inner)).push(library.clone());
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

/// Files a library keeps beside its instruments that are not instruments
/// (pictures, docs, monolith parts), not counted as other types.
const COMPANIONS: [&str; 11] = [
    "png", "jpg", "jpeg", "nkr", "nicnt", "nkx", "nkc", "pdf", "txt", "html", "nkp",
];

/// A folder with this many audio files and no presets holds samples only.
pub(crate) const SAMPLE_ONLY: usize = 64;
/// Threads listing one root. A root sits on one drive, and on a spinning
/// one more parallel listings cost more seeks than they save.
// ponytail: fixed for every drive. Telling an HDD from an SSD takes the
// seek-penalty query per OS (Windows IOCTL, Linux sysfs, macOS IOKit);
// add it if first scans of SSD roots get slow.
const ROOT_THREADS: usize = 2;
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
    scan_incremental(roots, progress, cache, inspect, &[])
}

/// Like [`scan_with_inspector`], but a library of `previous` whose stamped
/// folders are all unchanged is taken as it was: neither its folders nor
/// its presets are listed again, only stat-ed.
// ponytail: the folders above libraries are still listed. Stamp them too if
// a drive of loose vendor folders stays slow.
pub fn scan_incremental(
    roots: &[Root],
    progress: &Progress,
    cache: &mut Cache,
    inspect: BankInspector<'_>,
    previous: &[Library],
) -> Option<Catalog> {
    let mut issues = Vec::new();
    let mut libraries = Vec::new();
    let mut seen = BTreeSet::new();
    let mut statuses = Vec::new();
    for root in roots {
        (*progress.root.lock().unwrap_or_else(PoisonError::into_inner)).clone_from(&root.path);
        let before = (issues.len(), libraries.len());
        let counted = |p: &Progress| {
            (
                p.files.load(Ordering::Relaxed),
                p.other.load(Ordering::Relaxed),
            )
        };
        let (files, other) = counted(progress);
        let mut duplicates = 0;
        let founds = detect(root, progress, &mut issues, previous);
        // Every library found under this root, nested ones included: a library
        // inside another is catalogued as itself, so the outer one's walk stops
        // at it rather than listing its presets a second time
        // ([`visit_within`]).
        let dirs: BTreeSet<PathBuf> = founds.iter().map(|f| path_identity(&f.dir)).collect();
        for found in founds {
            let key = path_identity(&found.dir);
            if !seen.insert(key) {
                duplicates += 1;
                continue;
            }
            let same = previous.iter().find(|l| {
                l.dir == found.dir && !l.stamps.is_empty() && index::unchanged(&l.stamps)
            });
            let library = match same {
                Some(same) => Some(Library {
                    offline: None,
                    ..same.clone()
                }),
                None => catalog(found, &dirs, progress, cache, inspect, &mut issues),
            };
            if progress.canceled() {
                return None;
            }
            if let Some(library) = library {
                progress.catalogued(&library);
                libraries.push(library);
            }
        }
        let error = (issues.iter().skip(before.0))
            .find(|i| i.path == root.path && i.kind.is_some())
            .cloned();
        let (files_now, other_now) = counted(progress);
        let mine = issues.get(before.0..).unwrap_or_default();
        let others = |keep: fn(&Issue) -> bool| {
            mine.iter()
                .filter(|i| Some(*i) != error.as_ref() && keep(i))
                .count()
        };
        statuses.push(RootStatus {
            path: root.path.clone(),
            auto: root.auto,
            libraries: libraries.len() - before.1,
            stamp: index::mtime(&root.path),
            files: files_now - files,
            recognized: recognized(libraries.get(before.1..).unwrap_or_default()),
            skipped: [
                ("other file types", other_now - other),
                (
                    "unreadable",
                    others(|i| i.kind.is_some() && !i.protection.is_protected()),
                ),
                ("protected", others(|i| i.protection.is_protected())),
                (
                    "parse error",
                    others(|i| i.kind.is_none() && !i.protection.is_protected()),
                ),
                ("duplicate", duplicates),
            ]
            .into_iter()
            .filter(|(_, n)| *n > 0)
            .collect(),
            error,
        });
    }
    cache.forget_unseen();
    Some(Catalog {
        libraries: unique(libraries),
        issues,
        roots: statuses,
    })
}

/// The presets of `libraries` by format, largest first.
fn recognized(libraries: &[Library]) -> Vec<(&'static str, usize)> {
    let mut counts: Vec<(&'static str, usize)> = Vec::new();
    for preset in libraries.iter().flat_map(|l| &l.presets) {
        let format = match ext(preset).as_str() {
            "nki" | "nkm" | "nksn" => "Kontakt",
            "uvip" => "UVI",
            "sfz" => "SFZ",
            "sf2p" => "SoundFont",
            "dspreset" => "DecentSampler",
            "kontra" => "KONTRA",
            _ => "other",
        };
        match counts.iter_mut().find(|(f, _)| *f == format) {
            Some((_, n)) => *n += 1,
            None => counts.push((format, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counts
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

pub(crate) fn path_identity(path: &Path) -> PathBuf {
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
    /// Files, and files of no type the scan knows.
    files: usize,
    other: usize,
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
            issues.push(io_issue(dir, &e));
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
                    issues.push(io_issue(&path, &e).prefixed("broken symlink"));
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
        out.files += 1;
        match ext(&path).as_str() {
            "nicnt" => out.nicnt = out.nicnt.take().or(Some(path)),
            "nkx" | "nkc" | "nkr" => out.monolith = true,
            "ufs" | "sf2" | "sf3" => {
                out.presets += 1;
                out.banks.push(path);
            }
            "nki" | "nkm" | "nksn" | "uvip" | "sfz" | "dspreset" | "kontra" => out.presets += 1,
            "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => out.audio += 1,
            e if !COMPANIONS.contains(&e) => out.other += 1,
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
        kind: None,
    }
}

fn io_issue(path: &Path, e: &std::io::Error) -> Issue {
    Issue {
        kind: Some(e.kind()),
        ..issue(path, e)
    }
}

impl Issue {
    fn prefixed(mut self, what: &str) -> Self {
        self.reason = format!("{what}: {}", self.reason);
        self
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
        kind: None,
    }
}

/// The library folders and loose banks in `root`.
fn detect(
    root: &Root,
    progress: &Progress,
    issues: &mut Vec<Issue>,
    previous: &[Library],
) -> Vec<Found> {
    let dir = root.path.clone();
    match std::fs::metadata(&dir) {
        Err(e) => {
            issues.push(io_issue(&dir, &e));
            Vec::new()
        }
        Ok(m) if m.is_file() && matches!(ext(&dir).as_str(), "ufs" | "sf2" | "sf3") => {
            vec![Found {
                dir,
                nicnt: None,
                vendor: None,
            }]
        }
        Ok(m) if !m.is_dir() => {
            issues.push(issue(&dir, "not a library folder or UFS/SF2 bank"));
            Vec::new()
        }
        Ok(_) if root.single => {
            let nicnt = list(&dir, issues).nicnt;
            vec![Found {
                dir,
                nicnt,
                vendor: None,
            }]
        }
        Ok(_) => search(dir, progress, issues, previous),
    }
}

/// One folder to visit in [`search`].
struct Visit {
    dir: PathBuf,
    depth: usize,
    /// The first folder below the root that is not a library: the vendor.
    vendor: Option<String>,
    /// `dir` already sits inside a library.
    within: bool,
    /// It was reached through a symlink.
    through_symlink: bool,
}

/// What [`search`]'s threads share.
#[derive(Default)]
struct Search {
    /// Per `(canonical_dir, within)`: whether it was only reached through a
    /// symlink. A real path revisits a folder first reached through an alias.
    visited: HashMap<(PathBuf, bool), bool>,
    /// Per canonical library: the best route to it, `(through_symlink,
    /// depth)` smallest first.
    found: HashMap<PathBuf, ((bool, usize), Found)>,
    issues: Vec<Issue>,
}

impl Search {
    fn found(
        &mut self,
        canonical: PathBuf,
        rank: (bool, usize),
        candidate: Found,
        progress: &Progress,
    ) {
        match self.found.get_mut(&canonical) {
            Some((best, slot)) => {
                if rank < *best {
                    *best = rank;
                    *slot = candidate;
                }
            }
            None => {
                progress.found.fetch_add(1, Ordering::Relaxed);
                self.found.insert(canonical, (rank, candidate));
            }
        }
    }
}

/// Every library folder and loose bank under `root`, at any depth, found on
/// all cores ([`discover::par_walk`]). The search skips sample folders and
/// folders of many audio files without presets. Inside a library, only a
/// `.nicnt` makes a folder a library of its own. Symlinks are followed, and
/// canonical paths guard against cycles.
fn search(
    root: PathBuf,
    progress: &Progress,
    issues: &mut Vec<Issue>,
    previous: &[Library],
) -> Vec<Found> {
    let shared = std::sync::Mutex::new(Search::default());
    let lock = || {
        shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    };
    let start = Visit {
        dir: root,
        depth: 0,
        vendor: None,
        within: false,
        through_symlink: false,
    };
    discover::par_walk(vec![start], ROOT_THREADS, |v: Visit| {
        if progress.canceled() {
            return Vec::new();
        }
        let canonical = std::fs::canonicalize(&v.dir).unwrap_or_else(|_| v.dir.clone());
        let is_symlink = v.through_symlink
            || std::fs::symlink_metadata(&v.dir)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
        {
            let mut s = lock();
            match s.visited.get_mut(&(canonical.clone(), v.within)) {
                Some(prev) if is_symlink || !*prev => return Vec::new(),
                Some(prev) => *prev = false,
                None => {
                    s.visited.insert((canonical.clone(), v.within), is_symlink);
                }
            }
        }
        // A library the index knows, unchanged with every library inside
        // it, is found from its stamps without listing it again.
        if !v.within
            && let Some(known) = unchanged_libraries(&v.dir, previous)
        {
            let mut s = lock();
            for l in known {
                let rel = l.dir.strip_prefix(&v.dir).unwrap_or(Path::new(""));
                let key = if rel.as_os_str().is_empty() {
                    canonical.clone()
                } else {
                    canonical.join(rel)
                };
                let vendor = (l.dir == v.dir).then(|| v.vendor.clone()).flatten();
                let candidate = Found {
                    dir: l.dir.clone(),
                    nicnt: None,
                    vendor,
                };
                s.found(key, (is_symlink, v.depth), candidate, progress);
            }
            return Vec::new();
        }
        progress.folders.fetch_add(1, Ordering::Relaxed);
        let mut found_issues = Vec::new();
        let l = list(&v.dir, &mut found_issues);
        // Loose banks at the root are libraries of their own, not the root's presets.
        let presets = l.presets - if v.depth == 0 { l.banks.len() } else { 0 };
        let strong =
            l.nicnt.is_some() || l.instruments || (presets > 0 && (l.samples || l.monolith));
        // Below the root, a folder of presets; at the root, stray presets must
        // not hide the libraries beside them. Inside a library, only a
        // `.nicnt` makes a folder a library of its own.
        let library = if v.within {
            l.nicnt.is_some()
        } else {
            strong || (v.depth > 0 && l.presets > 0)
        };
        {
            let mut s = lock();
            s.issues.append(&mut found_issues);
            if library {
                let candidate = Found {
                    dir: v.dir.clone(),
                    nicnt: l.nicnt.clone(),
                    vendor: v.vendor.clone(),
                };
                s.found(canonical, (is_symlink, v.depth), candidate, progress);
            } else if v.depth == 0 {
                for bank in &l.banks {
                    let alias = std::fs::symlink_metadata(bank)
                        .map(|m| m.file_type().is_symlink())
                        .unwrap_or(false);
                    let candidate = Found {
                        dir: bank.clone(),
                        nicnt: None,
                        vendor: None,
                    };
                    s.found(path_identity(bank), (alias, 0), candidate, progress);
                }
            }
        }
        if l.audio >= SAMPLE_ONLY && l.presets == 0 {
            return Vec::new();
        }
        let inside = v.within || library;
        // A library's own files are counted by its catalog walk.
        if !inside {
            progress.files.fetch_add(l.files, Ordering::Relaxed);
            progress.other.fetch_add(l.other, Ordering::Relaxed);
        }
        // Below the root, the first folder that is not a library is its vendor.
        let vendor = if library {
            None
        } else {
            (v.vendor.clone()).or_else(|| (v.depth > 0).then(|| clean_name(&file_name(&v.dir)).0))
        };
        // Inside a library, its own folders (samples, docs, data) hold no
        // libraries; above one, only sample folders are skipped, so a drive's
        // `Documents` or `Data` folder is still searched.
        let skip = |name: &str| {
            if inside {
                SKIP.contains(&name)
            } else {
                name == "samples"
            }
        };
        (l.folders.into_iter().rev())
            .filter(|f| !skip(file_name(f).to_lowercase().as_str()))
            .map(|dir| Visit {
                dir,
                depth: v.depth + 1,
                vendor: vendor.clone(),
                within: inside,
                through_symlink: is_symlink,
            })
            .collect()
    });
    let s = shared
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut found_issues = s.issues;
    found_issues.sort_by(|a, b| a.path.cmp(&b.path));
    issues.append(&mut found_issues);
    let mut out: Vec<Found> = s.found.into_values().map(|(_, f)| f).collect();
    out.sort_by(|a, b| a.dir.cmp(&b.dir));
    out
}

/// The libraries of `previous` in `dir`, when `dir` is one of them and
/// none changed since its stamps ([`index::unchanged`]).
fn unchanged_libraries<'a>(dir: &Path, previous: &'a [Library]) -> Option<Vec<&'a Library>> {
    let inside: Vec<&Library> = (previous.iter())
        .filter(|l| l.dir.starts_with(dir))
        .collect();
    let known = inside.iter().any(|l| l.dir == dir)
        && (inside.iter()).all(|l| !l.stamps.is_empty() && index::unchanged(&l.stamps));
    known.then_some(inside)
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
    let mut stamps = Vec::new();
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
        stamps.extend(index::mtime(&found.dir).map(|t| (found.dir.clone(), t)));
    } else {
        let mut state = Walk {
            nested,
            progress,
            cache,
            inspect,
            issues,
            protection: &mut protection,
            stamps: &mut stamps,
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
    let product = (!name.is_empty()).then(|| format!("{name}|{company}"));
    let vendor = [company, bracket, found.vendor.unwrap_or_default()]
        .into_iter()
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let uvi_bank = (bank && ext(&found.dir) == "ufs")
        || (presets.is_empty() && art.iter().any(|p| ext(p) == "ufs"));
    let uvi = presets.iter().filter(|p| ext(p) == "uvip").count();
    let id = identity(&found.dir, product.as_deref(), &presets);
    Some(Library {
        id,
        offline: None,
        moved_from: Vec::new(),
        stamps,
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
    stamps: &'a mut Vec<(PathBuf, u128)>,
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
        state
            .stamps
            .extend(index::mtime(&dir).map(|t| (dir.clone(), t)));
        state.progress.folders.fetch_add(1, Ordering::Relaxed);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                state.issues.push(io_issue(&dir, &e));
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
                        (state.issues).push(io_issue(&path, &e).prefixed("broken symlink"));
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
            state.progress.files.fetch_add(1, Ordering::Relaxed);
            match ext(&path).as_str() {
                "nki" | "nkm" | "nksn" | "uvip" | "sfz" | "dspreset" | "kontra" | "ufs" | "sf2"
                | "sf3" => {
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
                "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => {}
                e if !COMPANIONS.contains(&e) => {
                    state.progress.other.fetch_add(1, Ordering::Relaxed);
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
            "sf2" | "sf3" => banks.push((path, false)),
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
pub(crate) fn unique(mut libraries: Vec<Library>) -> Vec<Library> {
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
