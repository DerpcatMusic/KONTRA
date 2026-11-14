//! Where the player's libraries are, what they are called and what they
//! hold: the browser's data, found by naming a folder and nothing else.
//!
//! A root is a folder of libraries, searched a few levels down, or one folder
//! (or `.ufs` bank) that is a library. A folder is a library when it holds a
//! `.nicnt` (registered), an `Instruments` or `Multis` folder, presets beside
//! a `Samples` folder or a monolith (`.nkx`/`.nkc`/`.nkr`), or, below the
//! root, presets of its own. Loose `.ufs` banks at a root are libraries of
//! their own. The search stops at a library, skips sample folders, and gives
//! up on a folder of many audio files with no presets. A library shows only
//! when it holds a preset.
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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub use cache::Cache;
pub use folder::{Folder, natural};
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

/// Something under a root that could not be read, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub path: PathBuf,
    pub reason: String,
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
    let mut issues = Vec::new();
    let mut libraries = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let founds = detect(root, progress, &mut issues);
        // Every library found under this root, nested ones included: a library
        // inside another is catalogued as itself, so the outer one's walk stops
        // at it rather than listing its presets a second time
        // ([`visit_within`]).
        let dirs: BTreeSet<PathBuf> = founds.iter().map(|f| f.dir.clone()).collect();
        for found in founds {
            let key = std::fs::canonicalize(&found.dir).unwrap_or_else(|_| found.dir.clone());
            if !seen.insert(key) {
                continue;
            }
            let library = catalog(found, &dirs, progress, cache, programs, &mut issues);
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
        // Symbolic links are not followed: a loop would never end.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if name.starts_with('.') || kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            match name.as_str() {
                "instruments" | "multis" => out.instruments = true,
                "samples" => out.samples = true,
                _ => {}
            }
            out.folders.push(path);
            continue;
        }
        match ext(&path).as_str() {
            "nicnt" => out.nicnt = out.nicnt.take().or(Some(path)),
            "nkx" | "nkc" | "nkr" => out.monolith = true,
            "ufs" => {
                out.presets += 1;
                out.banks.push(path);
            }
            "nki" | "nkm" | "uvip" => out.presets += 1,
            "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => out.audio += 1,
            _ => {}
        }
    }
    out.folders.sort();
    out.banks.sort();
    out
}

fn issue(path: &Path, reason: impl ToString) -> Issue {
    Issue {
        path: path.to_path_buf(),
        reason: reason.to_string(),
    }
}

/// The library folders and loose banks in `root`.
fn detect(root: &Root, progress: &Progress, issues: &mut Vec<Issue>) -> Vec<Found> {
    let dir = root.path.clone();
    let mut out = Vec::new();
    match std::fs::metadata(&dir) {
        Err(e) => issues.push(issue(&dir, e)),
        Ok(m) if m.is_file() && ext(&dir) == "ufs" => out.push(Found {
            dir,
            nicnt: None,
            vendor: None,
        }),
        Ok(m) if !m.is_dir() => issues.push(issue(&dir, "not a library folder or UFS bank")),
        Ok(_) if root.single => {
            let nicnt = list(&dir, issues).nicnt;
            out.push(Found {
                dir,
                nicnt,
                vendor: None,
            });
        }
        Ok(_) => visit(&dir, 0, None, &mut out, progress, issues),
    }
    out
}

fn visit(
    dir: &Path,
    depth: usize,
    vendor: Option<String>,
    out: &mut Vec<Found>,
    progress: &Progress,
    issues: &mut Vec<Issue>,
) {
    visit_within(dir, depth, vendor, false, out, progress, issues);
}

/// [`visit`], `within` saying whether `dir` already sits inside a library.
///
/// A library is not the end of the search. A bundle one level down can be a
/// library of its own — Action Strings 2 v1.2.0 is a `.nicnt` folder holding a
/// second `.nicnt` library, and AudioBro's LASS bundle holds three — and
/// stopping at the outer one made the inner ones unreachable: the browser
/// showed `Action Strings 2` once, with both copies' presets in it, where
/// Kontakt lists two libraries.
///
/// Inside a library none of that is true of its own subfolders. Below the root,
/// a folder of presets is a library (that is how a folder added without a
/// `.nicnt` is recognised at all); inside a library, the same rule would turn
/// every preset folder into a library — Evolve R2's `Instruments/4 Tonality and
/// FX` holds presets beside a `Samples` folder, so it would become a library
/// and take its instruments out of Evolve R2. So inside a library only a
/// `.nicnt` makes a folder a library of its own: that is the file Kontakt
/// registers a product by, and it is what identifies Action Strings 2's inner
/// library and LASS's three. The parent's walk prunes it ([`catalog`]), so a
/// preset is listed under exactly one library.
fn visit_within(
    dir: &Path,
    depth: usize,
    vendor: Option<String>,
    within: bool,
    out: &mut Vec<Found>,
    progress: &Progress,
    issues: &mut Vec<Issue>,
) {
    if progress.canceled() {
        return;
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
        progress.found.fetch_add(1, Ordering::Relaxed);
        out.push(Found {
            dir: dir.into(),
            nicnt: l.nicnt,
            // A nested library is named from its own folder, not the outer
            // one's: it is a different publisher entry in Kontakt's list.
            vendor: vendor.clone(),
        });
    } else if depth == 0 {
        for bank in l.banks {
            progress.found.fetch_add(1, Ordering::Relaxed);
            out.push(Found {
                dir: bank,
                nicnt: None,
                vendor: vendor.clone(),
            });
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
/// `programs`) and its banner candidates, from one walk that skips samples.
fn catalog(
    found: Found,
    nested: &BTreeSet<PathBuf>,
    progress: &Progress,
    cache: &mut Cache,
    programs: Programs<'_>,
    issues: &mut Vec<Issue>,
) -> Option<Library> {
    let mut presets = Vec::new();
    let mut art = Vec::new();
    let bank = found.dir.is_file();
    if bank {
        bank_presets(&found.dir, cache, programs, issues, &mut presets);
        art.extend(
            ["png", "jpg", "jpeg"]
                .iter()
                .map(|e| found.dir.with_extension(e))
                .filter(|p| p.is_file()),
        );
        art.push(found.dir.clone());
    } else {
        let mut state = Walk {
            nested,
            progress,
            cache,
            programs,
            issues,
        };
        walk(&found.dir, &mut state, &mut presets, &mut art);
        art.sort_by_cached_key(|p| (art_rank(p), p.clone()));
    }
    let multis = presets.iter().filter(|p| ext(p) == "nkm").count();
    if presets.is_empty() {
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
    let uvi = presets.iter().filter(|p| ext(p) == "uvip").count();
    Some(Library {
        name: if name.is_empty() { folder_name } else { name },
        vendor,
        format: if uvi * 2 > presets.len() {
            Format::Uvi
        } else {
            Format::Kontakt
        },
        registered: found.nicnt.is_some(),
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
    programs: Programs<'a>,
    issues: &'a mut Vec<Issue>,
}

/// The presets and banner candidates under `dir`, skipping sample folders and
/// the folders of libraries nested inside it (`state.nested`): those are
/// libraries of their own and their presets are listed under them.
fn walk(dir: &Path, state: &mut Walk<'_>, presets: &mut Vec<PathBuf>, art: &mut Vec<PathBuf>) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if state.progress.canceled() {
            return;
        }
        state.progress.folders.fetch_add(1, Ordering::Relaxed);
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) => {
                state.issues.push(issue(&dir, e));
                continue;
            }
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if name != "samples" && !state.nested.contains(&path) {
                    stack.push(path);
                }
                continue;
            }
            match ext(&path).as_str() {
                "nki" | "nkm" | "nksn" | "uvip" => presets.push(path),
                "ufs" => {
                    bank_presets(
                        &path,
                        &mut *state.cache,
                        state.programs,
                        &mut *state.issues,
                        presets,
                    );
                    art.push(path);
                }
                "png" | "jpg" | "jpeg" | "nicnt" | "nkr" => art.push(path),
                _ => {}
            }
        }
    }
}

fn bank_presets(
    bank: &Path,
    cache: &mut Cache,
    programs: Programs<'_>,
    issues: &mut Vec<Issue>,
    presets: &mut Vec<PathBuf>,
) {
    match cache.programs(bank, programs) {
        Ok(members) => presets.extend(members.iter().map(|m| bank.join(m))),
        Err(reason) => issues.push(issue(bank, reason)),
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
        // A bank chooses its own cover: after pictures named for the part,
        // before loose ones that may be anything.
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
