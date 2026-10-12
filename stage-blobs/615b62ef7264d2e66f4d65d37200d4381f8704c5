//! Where Kontakt and UVI libraries are and what they are called, found without
//! the player doing anything past naming a folder.
//!
//! The player adds roots, kept in the app's settings (not a project): a
//! folder of libraries, searched a few levels down, or one folder that is a
//! library. A folder is a library when it holds a `.nicnt` (registered), an
//! `Instruments` or `Multis` folder, presets beside a `Samples` folder or a
//! monolith (`.nkx`/`.nkc`/`.nkr`), or, below the root, presets of its own
//! (detected). Loose `.ufs` banks at a root are individual libraries.
//! The search stops at a library, skips sample folders, and gives
//! up on a folder of hundreds of audio files with no presets: a sample tree.
//! A library shows only when it holds a preset.
//!
//! Each is named from its `.nicnt` product, else its folder name cleaned of
//! versions, brackets and underscores; the vendor comes from the product, a
//! bracketed name or the vendor folder it sits in. Scans run on a thread of
//! their own, report progress, and can be canceled.
//!
//! On a first run with no roots, the libraries Kontakt knows about are added
//! (see [`kontakt`]); default Windows UVI bank folders are checked once even
//! with existing roots. The player can find installed libraries again at any time.


mod kontakt;
mod cache;
use moose::mui::mui::scene::Image;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock};

/// A folder or UFS bank the player added.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Root {
    pub path: String,
    /// The folder is one library, rather than holding libraries.
    #[serde(default)]
    pub single: bool,
}

/// A library's cover when it is not its own artwork.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Cover {
    /// The generated cover, even though the library has artwork.
    Generated,
    /// A picture the player chose, copied into the app's data folder.
    Custom { file: String, stamp: u64 },
}

/// What the app keeps between sessions, whatever the project.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Settings {
    pub roots: Vec<Root>,
    /// Display names chosen by the player, keyed by library folder.
    pub names: BTreeMap<String, String>,
    /// Covers the player chose, by library folder.
    pub covers: BTreeMap<String, Cover>,
    /// The libraries Kontakt knows about were looked for, on the first run.
    pub imported: bool,
    /// Default UVI bank locations were checked independently of Kontakt roots.
    pub uvi_imported: bool,
    /// The performance view parts show unless they choose their own.
    pub view_mode: ViewMode,
    /// Saved view override by instrument path; rack overrides take precedence.
    pub instrument_views: BTreeMap<String, ViewMode>,
    /// The performance view's scale; 0 fits the available width and height.
    pub view_scale: f32,
    /// Overall interface zoom, independent of physical display DPI; 0 means 100%.
    pub ui_scale: f64,
    /// Last native window dimensions in host logical points, independent of zoom.
    pub window_size: Option<(u32, u32)>,
    /// How the browser lists the libraries.
    pub sort: Sort,
    /// The player's own order of the libraries, by folder: set by dragging one.
    pub order: Vec<String>,
    /// Libraries pinned above the rest, by folder.
    pub pinned: Vec<String>,
    /// When a preset of each library was last loaded, by folder: seconds
    /// since 1970.
    pub used: BTreeMap<String, u64>,
    /// Folders opened or closed in the browser, by path.
    pub folders: BTreeMap<String, bool>,
    /// The library the browser last showed, by folder, and the row chosen in it.
    pub last_library: String,
    pub last_row: String,
    /// The MIDI input new parts take: `None` the next free channel
    /// (Kontakt's auto-increment), else that port and channel (-1 omni).
    pub new_input: Option<(u8, i16)>,
    /// The output bus new parts play through: `None` routes them as the
    /// rack's Outputs choice says, else that bus, held as if picked by hand.
    pub new_output: Option<u8>,
    /// Voice-rendering threads for parts loaded from now on (`KONTRA_THREADS`
    /// overrides it).
    pub threads: ThreadSetting,
    /// Additional v2 preferences not interpreted by this version.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// How many threads render a part's voices.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThreadSetting {
    /// The audio thread alone.
    #[default]
    Single,
    /// Up to four, never more than the machine has cores.
    Auto,
    /// Exactly this many (counting the audio thread).
    Fixed(u8),
}

/// How a part shows its library's performance view.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ViewMode {
    /// The library's own pictures, as Kontakt draws them.
    #[default]
    Original,
    /// The same controls in the same places, drawn in KONTRA's own look.
    Vectorized,
    /// KONTRA's rebuilt controls.
    Kontra,
}

impl ViewMode {
    pub const ALL: [Self; 3] = [Self::Original, Self::Vectorized, Self::Kontra];

    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Vectorized => "Vectorized",
            Self::Kontra => "KONTRA",
        }
    }
}

/// How the browser lists the libraries. Pinned ones lead whatever the sort.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    /// By name, until the player drags one.
    #[default]
    Name,
    /// The player's own order.
    Custom,
    /// The library a preset was last loaded from first.
    Recent,
    Vendor,
}

impl Sort {
    pub const ALL: [Self; 4] = [Self::Custom, Self::Name, Self::Recent, Self::Vendor];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "A\u{2013}Z",
            Self::Custom => "Custom",
            Self::Recent => "Recently used",
            Self::Vendor => "Vendor",
        }
    }
}

/// A library's presets by folder, as on disk.
#[derive(Debug, Default, PartialEq)]
pub struct Folder {
    pub name: String,
    /// Its path, as text.
    pub path: String,
    /// Its folders by name, then its presets as indices into what it was
    /// built from, in that order.
    pub folders: Vec<Folder>,
    pub presets: Vec<usize>,
    /// Presets in it and every folder under it.
    pub count: usize,
}

impl Folder {
    /// The folders of `presets` under `dir`; one outside it sits at the top.
    pub fn tree<'a>(dir: &Path, presets: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut root = Folder { path: dir.to_string_lossy().into_owned(), ..Folder::default() };
        for (n, preset) in presets.into_iter().enumerate() {
            let inside = preset.parent().and_then(|p| p.strip_prefix(dir).ok());
            let mut at = &mut root;
            for part in inside.into_iter().flat_map(Path::components) {
                let name = part.as_os_str().to_string_lossy();
                // Presets come sorted: their folder is most often the last one made.
                let found = match at.folders.last() {
                    Some(f) if f.name == name => Some(at.folders.len() - 1),
                    _ => at.folders.iter().position(|f| f.name == name),
                };
                let i = found.unwrap_or_else(|| {
                    let path = Path::new(&at.path).join(name.as_ref()).to_string_lossy().into_owned();
                    at.folders.push(Folder { name: name.into_owned(), path, ..Folder::default() });
                    at.folders.len() - 1
                });
                at = &mut at.folders[i];
            }
            at.presets.push(n);
        }
        root.settle();
        root
    }

    /// Folders by name, numbers in them by value; counts summed.
    fn settle(&mut self) {
        self.folders.sort_by_cached_key(|f| natural(&f.name));
        self.count = self.presets.len();
        for f in &mut self.folders {
            f.settle();
            self.count += f.count;
        }
    }
}

/// A library folder name without the vendor noise.
pub fn label(name: &str) -> String {
    name.replace("Performance Samples ", "").replace(" Library", "")
}

/// A sort key reading runs of digits as numbers: "2 Legato" before "10 Shorts".
pub fn natural(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
        let (number, tail) = rest.split_at(digits);
        let words = tail.find(|c: char| c.is_ascii_digit()).unwrap_or(tail.len());
        let (word, tail) = tail.split_at(words);
        out.push((number.parse().unwrap_or(0), word.to_lowercase()));
        rest = tail;
    }
    out
}

#[derive(Serialize, Deserialize)]
struct SettingsFile<T> {
    version: u32,
    #[serde(flatten)]
    settings: T,
}

impl Settings {
    /// Overall UI zoom; legacy/invalid values keep the default.
    pub fn editor_scale(&self) -> f64 {
        if self.ui_scale.is_finite() && self.ui_scale > 0.0 { self.ui_scale.clamp(0.5, 3.0) } else { 1.0 }
    }

    pub fn editor_size(&self) -> (u32, u32) {
        let (w, h) = self.window_size.unwrap_or((1180, 760));
        (w.max(900), h.max(600))
    }

    /// `settings.json` in the app's config folder; none under test.
    pub fn path() -> Option<PathBuf> {
        Some(config_dir()?.join("settings.json"))
    }

    pub fn load(path: &Path) -> Option<Self> {
        let file: SettingsFile<Self> = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (file.version == 2).then_some(file.settings)
    }

    /// Browser text only: resource and source identities keep `Library::name`.
    pub fn library_name(&self, library: &Library) -> String {
        if let Some(name) = self.names.get(library.dir.to_string_lossy().as_ref()).filter(|n| !n.trim().is_empty()) {
            return name.clone();
        }
        let name = label(&library.name);
        if !name.trim().is_empty() { return name; }
        let folder = library.dir.file_name().unwrap_or(library.dir.as_os_str()).to_string_lossy();
        if folder.trim().is_empty() { "Library".into() } else { folder.into_owned() }
    }

    pub fn rename_library(&mut self, dir: &str, name: &str) {
        let name = name.trim();
        if name.is_empty() { self.names.remove(dir); }
        else { self.names.insert(dir.into(), name.into()); }
    }

    /// `libraries` as the browser lists them: pinned ones first, then by the
    /// sort chosen. Libraries the custom order has not seen follow it, by name.
    pub fn arrange<'a>(&self, libraries: impl IntoIterator<Item = &'a Library>) -> Vec<&'a Library> {
        let ranks: BTreeMap<_, _> = self.order.iter().enumerate().map(|(n, p)| (Path::new(p), n)).collect();
        let mut out: Vec<&Library> = libraries.into_iter().collect();
        out.sort_by_cached_key(|l| {
            let dir = l.dir.to_string_lossy();
            let (rank, vendor) = match self.sort {
                Sort::Name => (0, String::new()),
                Sort::Custom => (ranks.get(l.dir.as_path()).copied().unwrap_or(usize::MAX) as u64, String::new()),
                // Newest first; never used last.
                Sort::Recent => (u64::MAX - self.used.get(dir.as_ref()).map_or(0, |&t| t + 1), String::new()),
                Sort::Vendor => (u64::from(l.vendor.is_empty()), l.vendor.to_lowercase()),
            };
            (!self.pinned.iter().any(|p| Path::new(p) == l.dir), rank, vendor, natural(&self.library_name(l)))
        });
        out
    }

    /// Move the library in folder `from` to just before `before` (the end
    /// when `None`), `shown` being every library's folder as listed: the
    /// listing becomes the player's own order. Dropped among the pinned, it
    /// is pinned; among the rest, it is not.
    pub fn reorder(&mut self, shown: &[String], from: &str, before: Option<&str>) {
        let pin = before.is_some_and(|b| self.pinned.iter().any(|p| p == b));
        self.pinned.retain(|p| p != from);
        if pin {
            self.pinned.push(from.to_owned());
        }
        let mut order: Vec<String> = shown.iter().filter(|d| *d != from).cloned().collect();
        let at = before.and_then(|b| order.iter().position(|d| d == b)).unwrap_or(order.len());
        order.insert(at, from.to_owned());
        // Libraries offline now keep their place, at the end.
        order.extend(self.order.iter().filter(|d| !shown.contains(d)).cloned());
        self.order = order;
        self.sort = Sort::Custom;
    }

    /// Written aside and renamed into place: a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&SettingsFile { version: 2, settings: self })?)?;
        std::fs::rename(tmp, path)
    }
}

fn config_dir() -> Option<PathBuf> {
    // Under test every scanner starts from nothing and saves nowhere: tests
    // share a process, and must not share settings.
    if cfg!(test) {
        return None;
    }
    Some(dirs::config_dir()?.join("kontra"))
}

/// The app's data folder: chosen artwork, and multis saved with no library
/// folder to keep them in.
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(test) {
        return Some(std::env::temp_dir().join(format!("kontra-test-{}", std::process::id())).join("data"));
    }
    Some(dirs::data_dir()?.join("kontra"))
}

/// One library found.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Library {
    pub dir: PathBuf,
    /// Its name, unique among the libraries.
    pub name: String,
    pub vendor: String,
    /// It has a `.nicnt`; otherwise it was recognized from its folders.
    pub registered: bool,
    pub instruments: usize,
    pub multis: usize,
    /// The dominant hue of its own pictures, when it has no artwork.
    pub hue: Option<f32>,
}

/// Real snapshot files matched to the exact embedded base instrument name.
#[derive(Debug)]
pub struct Snapshots {
    pub instrument: String,
    pub paths: Vec<PathBuf>,
}

/// The libraries found, looked up by folder or name.
#[derive(Default, Debug)]
pub struct Shelf {
    pub libraries: Vec<Library>,
    pub snapshots: HashMap<PathBuf, Snapshots>,
    pub bank_issues: Vec<BankIssue>,
    /// Unavailable filesystem entries retained for per-root diagnostics.
    pub path_issues: BTreeMap<PathBuf, String>,
    /// Prepared once by the library worker, never scanned during painting.
    /// Native path keys also equate Windows' slash and backslash separators.
    by_dir: HashMap<PathBuf, usize>,
    by_name: HashMap<String, usize>,
    /// Libraries found in each root, in the order of the roots.
    pub per_root: Vec<usize>,
}

impl Shelf {
    /// `libraries` sorted by name, each name made unique.
    pub fn new(mut libraries: Vec<Library>) -> Self {
        libraries.sort_by_cached_key(|l| (l.name.to_lowercase(), l.dir.clone()));
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
        let by_dir = (libraries.iter().enumerate())
            .map(|(n, l)| (l.dir.clone(), n))
            .collect();
        let by_name = libraries.iter().enumerate().map(|(n, l)| (l.name.clone(), n)).collect();
        Self { libraries, by_dir, by_name, per_root: Vec::new(), snapshots: HashMap::new(), bank_issues: Vec::new(), path_issues: BTreeMap::new() }
    }

    /// The library `path` is in: the nearest library folder above it.
    pub fn of(&self, path: &Path) -> Option<&Library> {
        for at in path.parent()?.ancestors() {
            if let Some(&n) = self.by_dir.get(at) {
                return Some(&self.libraries[n]);
            }
        }
        None
    }

    pub fn named(&self, name: &str) -> Option<&Library> {
        self.by_name.get(name).map(|&n| &self.libraries[n])
    }

    /// Count unreadable banks and retain each reported cause for this root.
    pub fn bank_problem(&self, root: &Path) -> Option<String> {
        let mut count = 0;
        let mut causes = Vec::new();
        for issue in &self.bank_issues {
            let matched = issue.locations.iter().filter(|path| path.starts_with(root)).count();
            if matched > 0 {
                count += matched;
                causes.push(issue.message.as_str());
            }
        }
        (count > 0).then(|| format!("{count} UVI {} could not be cataloged: {}", if count == 1 { "bank" } else { "banks" }, causes.join("; ")))
    }

    /// Filesystem and bank failures under a saved root, including partial scans.
    pub fn root_problem(&self, root: &Path) -> Option<String> {
        let mut messages: Vec<String> = self.bank_problem(root).into_iter().collect();
        let issues: Vec<_> = self.path_issues.iter().filter(|(path, _)| path.starts_with(root)).collect();
        if !issues.is_empty() {
            let reasons: BTreeSet<&str> = issues.iter().map(|(_, message)| message.as_str()).collect();
            messages.push(format!("{} library {} skipped: {}", issues.len(), if issues.len() == 1 { "path" } else { "paths" }, reasons.into_iter().collect::<Vec<_>>().join("; ")));
        }
        (!messages.is_empty()).then(|| messages.join("\n"))
    }

    /// One library per folder right under `root`, named for it: what a list
    /// of files with no folders on disk behind them is shelved as.
    #[cfg(test)]
    pub fn under(root: &str, files: &[PathBuf]) -> Self {
        let dirs: BTreeSet<PathBuf> = files
            .iter()
            .filter_map(|f| Some(Path::new(root).join(f.strip_prefix(root).ok()?.components().next()?)))
            .collect();
        Self::new(
            dirs.into_iter()
                .map(|dir| Library {
                    name: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                    dir,
                    ..Library::default()
                })
                .collect(),
        )
    }
}

/// How far a scan is, and a way to stop it.
#[derive(Default, Debug)]
pub struct Progress {
    pub folders: AtomicUsize,
    pub found: AtomicUsize,
    pub cancel: AtomicBool,
    pub running: AtomicBool,
    bank_issues: Mutex<BTreeMap<(bool, String), BTreeSet<PathBuf>>>,
    path_issues: Mutex<BTreeMap<PathBuf, String>>,
}

/// One catalog problem and every bank affected by it.
#[derive(Debug)]
pub struct BankIssue {
    pub unsupported: bool,
    pub message: String,
    pub locations: Vec<PathBuf>,
}

impl Progress {
    fn path_issue(&self, path: &Path, reason: impl ToString) {
        let reason = reason.to_string();
        if lock(&self.path_issues).insert(path.into(), reason.clone()).is_none() {
            crate::diagnostics::resource(path, "library path", &reason);
        }
    }

    fn bank_issue(&self, path: &Path, error: sampler_uvi::AccessError) {
        let unsupported = matches!(error, sampler_uvi::AccessError::Disabled);
        let message = if unsupported { "protected library: not supported".into() } else { error.to_string() };
        lock(&self.bank_issues).entry((unsupported, message)).or_default().insert(path.into());
    }

    fn canceled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// How many levels under a root libraries are looked for.
const DEPTH: usize = 4;
/// A folder with this many audio files and no presets holds samples only.
const SAMPLE_ONLY: usize = 64;
/// Folders a library keeps no presets in, never searched.
const SKIP: [&str; 6] = ["samples", "documentation", "documents", "docs", "resources", "data"];

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

fn list(dir: &Path, progress: &Progress) -> Listing {
    let mut out = Listing::default();
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => { progress.path_issue(dir, e); return out; }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => { progress.path_issue(dir, error); continue; }
        };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') {
            continue;
        }
        // Symbolic links are not followed: a loop would never end.
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) => { progress.path_issue(&path, error); continue; }
        };
        if kind.is_symlink() {
            if let Err(error) = std::fs::metadata(&path) {
                progress.path_issue(&path, format!("Symbolic link cannot be resolved: {error}"));
            }
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
        if !kind.is_file() { continue; }
        let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
        match ext {
            "nicnt" => out.nicnt = out.nicnt.take().or(Some(path)),
            "nkx" | "nkc" | "nkr" => out.monolith = true,
            "ufs" => { out.presets += 1; out.banks.push(path); }
            "nki" | "nkm" | "uvip" | MULTI => out.presets += 1,
            "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => out.audio += 1,
            _ => {}
        }
    }
    out.folders.sort();
    out.banks.sort();
    out
}

/// A library folder or loose UFS bank found under a root.
struct Candidate {
    dir: PathBuf,
    nicnt: Option<PathBuf>,
    /// The folder it sits in below the root, which names its vendor.
    vendor: Option<String>,
}

/// The library folders in `root`.
fn detect(root: &Root, progress: &Progress) -> Vec<Candidate> {
    let dir = PathBuf::from(&root.path);
    let mut out = Vec::new();
    let metadata = match std::fs::metadata(&dir) {
        Ok(metadata) => metadata,
        Err(error) => { progress.path_issue(&dir, error); return out; }
    };
    if metadata.is_file() && (dir.extension().is_some_and(|x| x.eq_ignore_ascii_case("ufs")) || (root.single && is_preset(&dir))) {
        out.push(Candidate { dir, nicnt: None, vendor: None });
    } else if !metadata.is_dir() {
        progress.path_issue(&dir, "Not a library directory or UFS bank");
    } else if root.single {
        let listing = list(&dir, progress);
        out.push(Candidate { dir, nicnt: listing.nicnt, vendor: None });
    } else {
        visit(&dir, 0, None, &mut out, progress);
    }
    out
}

fn visit(dir: &Path, depth: usize, vendor: Option<String>, out: &mut Vec<Candidate>, progress: &Progress) {
    if progress.canceled() {
        return;
    }
    progress.folders.fetch_add(1, Ordering::Relaxed);
    let l = list(dir, progress);
    let library = l.nicnt.is_some()
        || l.instruments
        || (l.presets > 0 && (l.samples || l.monolith))
        // Below the root, a folder of presets: at the root, stray presets
        // must not hide the libraries beside them.
        || (depth > 0 && l.presets > 0);
    if library {
        progress.found.fetch_add(1, Ordering::Relaxed);
        out.push(Candidate { dir: dir.into(), nicnt: l.nicnt, vendor });
        return;
    }
    // Loose banks are libraries themselves; keep searching adjacent folders.
    if depth == 0 {
        for bank in l.banks {
            progress.found.fetch_add(1, Ordering::Relaxed);
            out.push(Candidate { dir: bank, nicnt: None, vendor: vendor.clone() });
        }
    }
    if depth >= DEPTH || (l.audio >= SAMPLE_ONLY && l.presets == 0) {
        return;
    }
    // Below the root, the first folder that is not a library is its vendor;
    // folders under it are bundles of that vendor.
    let vendor = vendor.or_else(|| (depth > 0).then(|| clean_name(&file_name(dir)).0));
    for folder in &l.folders {
        let name = file_name(folder).to_lowercase();
        if !SKIP.contains(&name.as_str()) {
            visit(folder, depth + 1, vendor.clone(), out, progress);
        }
    }
}

/// FNV-1a: the same on every machine and every run.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

fn file_name(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// The product's name and company from a `.nicnt`'s product XML. Only these
/// two fields are read.
pub fn product(nicnt: &Path) -> Option<(String, String)> {
    let mut bytes = Vec::new();
    File::open(nicnt).ok()?.take(64 * 1024).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let field = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let start = text.find(&open)? + open.len();
        let end = start + text[start..].find('<')?;
        let value = text[start..end]
            .replace("&amp;", "&")
            .replace("&apos;", "'")
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        let value = value.trim();
        (!value.is_empty() && value.len() < 120).then(|| value.to_owned())
    };
    Some((field("Name")?, field("Company").unwrap_or_default()))
}

/// A folder name as a library name, and the vendor it names in brackets:
/// "Areia 1.2.0 [Audio Imperia]" is Areia, by Audio Imperia.
pub fn clean_name(folder: &str) -> (String, String) {
    let mut vendor = String::new();
    let mut kept = String::new();
    let (mut depth, mut inner, mut square) = (0, String::new(), false);
    for c in folder.chars() {
        match c {
            '[' | '(' | '{' => {
                if depth == 0 {
                    inner.clear();
                    square = c == '[';
                }
                depth += 1;
                kept.push(' ');
            }
            ']' | ')' | '}' if depth > 0 => {
                depth -= 1;
                let words = words(&inner);
                if depth == 0 && square && !words.is_empty() {
                    vendor = words.join(" ");
                }
            }
            _ if depth > 0 => inner.push(c),
            _ => kept.push(c),
        }
    }
    let name = words(&kept).join(" ");
    let name = name.trim_matches(|c: char| c == '-' || c == ' ').to_owned();
    if name.is_empty() {
        return (folder.trim().to_owned(), vendor);
    }
    (name, vendor)
}

/// The words of a name without underscores, versions or packaging noise.
fn words(text: &str) -> Vec<&str> {
    let version = |w: &str| {
        let w = w.strip_prefix(['v', 'V']).unwrap_or(w);
        !w.is_empty()
            && w.starts_with(|c: char| c.is_ascii_digit())
            && (w.contains('.') || w.len() < 3 && text.contains(['v', 'V']))
            && w.chars().all(|c| c.is_ascii_digit() || c == '.' || c.is_ascii_lowercase() && c < 'g')
    };
    let noise = |w: &str| {
        matches!(
            w.to_lowercase().as_str(),
            "kontakt" | "library" | "nki" | "full" | "win" | "mac" | "osx" | "pc" | "update"
        )
    };
    text.split(|c: char| c.is_whitespace() || c == '_')
        .filter(|w| !w.is_empty() && !version(w) && !noise(w))
        .collect()
}

/// The extension of a rack KONTRA saved (`plugin::SavedMulti`).
pub const MULTI: &str = "kontra-multi";

pub fn is_multi(path: &Path) -> bool {
    path.extension().is_some_and(|x| x.eq_ignore_ascii_case(MULTI))
}

/// What plays as one rack part: a Kontakt instrument or a sample.
pub fn is_instrument(path: &Path) -> bool {
    path.extension().is_some_and(|x| ["nki", "nkm", "nksn", "uvip", "wav"].iter().any(|e| x.eq_ignore_ascii_case(e)))
}

/// What the browser lists and the rack opens: Kontakt instruments and saved racks.
pub fn is_preset(path: &Path) -> bool {
    is_multi(path) || path.extension().is_some_and(|x| ["nki", "nkm", "uvip"].iter().any(|e| x.eq_ignore_ascii_case(e)))
}

/// Presets in a library folder, its sample folders left unread.
fn presets(dir: &Path, progress: &Progress) -> (Vec<PathBuf>, HashMap<PathBuf, Snapshots>) {
    cached_presets(dir, progress, &mut cache::Cache::default())
}

fn cached_presets(dir: &Path, progress: &Progress, cache: &mut cache::Cache) -> (Vec<PathBuf>, HashMap<PathBuf, Snapshots>) {
    let mut trace = crate::diagnostics::LoadTrace::new(dir, 0, None);
    trace.detail("operation", "preset_catalog");
    trace.stage("catalog");
    let walk = walkdir::WalkDir::new(dir).follow_links(false).into_iter().filter_entry(|e| {
        !(e.depth() > 0 && e.file_type().is_dir() && {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name == "samples" || name.starts_with('.')
        })
    });
    let mut out = Vec::new();
    let mut snapshots: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in walk {
        if progress.canceled() {
            break;
        }
        let e = match entry {
            Ok(e) => e,
            Err(e) => {
                progress.path_issue(e.path().unwrap_or(dir), e.io_error().map_or_else(|| e.to_string(), ToString::to_string));
                trace.issue("catalog", "directory_unreadable", e.to_string());
                continue;
            }
        };
        if e.file_type().is_dir() {
            progress.folders.fetch_add(1, Ordering::Relaxed);
        }
        let path = e.path();
        if e.file_type().is_symlink() {
            if let Err(error) = std::fs::metadata(path) {
                progress.path_issue(path, format!("Symbolic link cannot be resolved: {error}"));
            }
            continue;
        }
        if !e.file_type().is_file() { continue; }
        if path.extension().is_some_and(|s| s.eq_ignore_ascii_case("nksn")) {
            if let Some(cache::Metadata::Snapshot(name)) = cache.memo(path, || match crate::sound::v2::snapshot_instrument(path) {
                Ok(name) => Some(cache::Metadata::Snapshot(name)),
                Err(error) => { trace.issue("catalog", "snapshot_metadata_failed", format!("{}: {error:#}", path.display())); None }
            }) { snapshots.entry(name).or_default().push(e.into_path()); }
        } else if is_preset(path) {
            cache.observe(path);
            out.push(e.into_path());
        } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("ufs")) {
            if let Some(cache::Metadata::Bank(members)) = cache.memo(path, || match sampler_uvi::Bank::catalog(path) {
                Ok(members) => Some(cache::Metadata::Bank(members)),
                Err(e) => { progress.bank_issue(path, e); None }
            }) { out.extend(members.into_iter().map(|member| path.join(member))); }
        }
    }
    let mut matched = HashMap::new();
    if !snapshots.is_empty() {
        for paths in snapshots.values_mut() {
            paths.sort_by_cached_key(|p| (natural(&p.strip_prefix(dir).unwrap_or(p).to_string_lossy()), p.clone()));
        }
        let mut bases: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for base in out.iter().filter(|p| p.extension().is_some_and(|s| s.eq_ignore_ascii_case("nki"))) {
            if progress.canceled() { break; }
            if let Some(cache::Metadata::Instrument(name)) = cache.memo(base, || match crate::sound::v2::snapshot_base_name(base) {
                Ok(name) => Some(cache::Metadata::Instrument(name)),
                Err(error) => { trace.issue("catalog", "snapshot_base_metadata_failed", format!("{}: {error:#}", base.display())); None }
            }) { bases.entry(name).or_default().push(base.clone()); }
        }
        for (name, bases) in bases {
            if let Some(paths) = snapshots.get(&name) {
                if bases.len() == 1 {
                    matched.insert(bases[0].clone(), Snapshots { instrument: name, paths: paths.clone() });
                } else {
                    trace.issue("catalog", "snapshot_base_ambiguous", format!("{name}: {} base instruments have the same embedded name; load snapshots explicitly", bases.len()));
                }
            }
        }
    }
    trace.detail("presets", out.len());
    trace.detail("snapshots", snapshots.values().map(Vec::len).sum::<usize>());
    trace.detail("snapshot_bases", matched.len());
    trace.finish(if progress.canceled() { "canceled" } else { "loaded" });
    (out, matched)
}

/// Every library in `roots` and its presets; `None` once canceled.
pub fn scan(roots: &[Root], progress: &Progress) -> Option<(Shelf, Vec<PathBuf>)> {
    cached_scan(roots, progress, &mut cache::Cache::default())
}

fn cached_scan(roots: &[Root], progress: &Progress, cache: &mut cache::Cache) -> Option<(Shelf, Vec<PathBuf>)> {
    let mut libraries: Vec<Library> = Vec::new();
    let mut files = BTreeSet::new();
    let mut snapshots = HashMap::new();
    let mut per_root = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let mut trace = crate::diagnostics::LoadTrace::new(Path::new(&root.path), 0, None);
        trace.detail("operation", "library_discovery");
        trace.stage("discover");
        let before = libraries.len();
        for c in detect(root, progress) {
            let key = std::fs::canonicalize(&c.dir).unwrap_or(c.dir.clone());
            if !seen.insert(key) {
                continue;
            }
            let (found, matched) = cached_presets(&c.dir, progress, cache);
            snapshots.extend(matched);
            if progress.canceled() {
                trace.finish("canceled");
                return None;
            }
            if found.is_empty() {
                continue;
            }
            let name = if c.dir.is_file() && c.dir.extension().is_some_and(|x| x.eq_ignore_ascii_case("ufs")) {
                c.dir.file_stem().unwrap_or_default().to_string_lossy().into_owned()
            } else { file_name(&c.dir) };
            let (folder_name, bracket) = clean_name(&name);
            let product = c.nicnt.as_deref().and_then(|path| cache.memo(path, || {
                let (name, vendor) = product(path).unwrap_or_default();
                Some(cache::Metadata::Product(name, vendor))
            })).and_then(|m| match m { cache::Metadata::Product(name, vendor) => Some((name, vendor)), _ => None });
            let (name, company) = product.unwrap_or_default();
            let vendor = [company, bracket, c.vendor.unwrap_or_default()]
                .into_iter()
                .find(|v| !v.is_empty())
                .unwrap_or_default();
            let multis = found.iter().filter(|p| is_multi(p) || p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkm"))).count();
            libraries.push(Library {
                name: if name.is_empty() { folder_name } else { name },
                vendor,
                registered: c.nicnt.is_some(),
                instruments: found.len() - multis,
                multis,
                dir: c.dir,
                hue: None,
            });
            files.extend(found);
        }
        trace.detail("libraries", libraries.len() - before);
        trace.finish(if progress.canceled() { "canceled" } else { "cataloged" });
        per_root.push(libraries.len() - before);
    }
    let mut shelf = Shelf::new(libraries);
    shelf.per_root = per_root;
    shelf.snapshots = snapshots;
    shelf.path_issues = lock(&progress.path_issues).clone();
    shelf.bank_issues = lock(&progress.bank_issues).iter().map(|((unsupported, message), locations)| BankIssue {
        unsupported: *unsupported, message: message.clone(), locations: locations.iter().cloned().collect(),
    }).collect();
    for issue in &shelf.bank_issues {
        crate::diagnostics::event(crate::diagnostics::LogLevel::Warning, "library", "bank_unreadable", serde_json::json!({
            "path": roots.first().map(|r| &r.path), "stage": "catalog", "unsupported": issue.unsupported,
            "message": issue.message, "locations": issue.locations, "count": issue.locations.len(),
        }));
    }
    Some((shelf, files.into_iter().collect()))
}

/// Everything a finished scan hands the editor.
pub struct Scanned {
    pub shelf: Arc<Shelf>,
    pub files: Arc<Vec<PathBuf>>,
    pub artwork: HashMap<String, Arc<Image>>,
    /// Registered Kontakt and default UVI roots added by this scan.
    pub imported: Option<Vec<Root>>,
}

/// Coalesced preference work, with an explicit flush when a window closes.
enum SaveSettings { Changed, Flush(std::sync::mpsc::Sender<()>) }

struct Preferences {
    state: Arc<RwLock<(Arc<Settings>, u64)>>,
    save: Option<std::sync::mpsc::Sender<SaveSettings>>,
    queued: Arc<AtomicBool>,
    roots_changed: AtomicU64,
}

impl Preferences {
    fn new(path: Option<PathBuf>) -> Arc<Self> {
        let settings = path.as_ref().and_then(|p| Settings::load(p)).unwrap_or_default();
        let state = Arc::new(RwLock::new((Arc::new(settings), 0)));
        let queued = Arc::new(AtomicBool::new(false));
        let save = path.map(|path| {
            let (tx, rx) = std::sync::mpsc::channel();
            let (state, queued) = (state.clone(), queued.clone());
            std::thread::spawn(move || {
                let mut saved = 0;
                let mut persist = || {
                    let (settings, revision) = { let s = state.read().unwrap_or_else(PoisonError::into_inner); (s.0.clone(), s.1) };
                    if revision != saved && settings.save(&path).is_ok() { saved = revision; }
                };
                while let Ok(mut message) = rx.recv() {
                    loop {
                        match message {
                            SaveSettings::Changed => queued.store(false, Ordering::Release),
                            SaveSettings::Flush(reply) => { persist(); let _ = reply.send(()); break; }
                        }
                        match rx.recv_timeout(std::time::Duration::from_millis(250)) {
                            Ok(next) => message = next,
                            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => { persist(); break; }
                            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => { persist(); return; }
                        }
                    }
                }
            });
            tx
        });
        Arc::new(Self { state, save, queued, roots_changed: AtomicU64::new(0) })
    }

    fn shared() -> Arc<Self> {
        if cfg!(test) { return Self::new(None); }
        static GLOBAL: OnceLock<Arc<Preferences>> = OnceLock::new();
        GLOBAL.get_or_init(|| Self::new(Settings::path())).clone()
    }

    fn settings(&self) -> Arc<Settings> { self.state.read().unwrap_or_else(PoisonError::into_inner).0.clone() }

    fn edit(&self, change: impl FnOnce(&mut Settings)) {
        let mut state = self.state.write().unwrap_or_else(PoisonError::into_inner);
        let mut next = (*state.0).clone();
        change(&mut next);
        if next == *state.0 { return; }
        if next.roots != state.0.roots { self.roots_changed.fetch_add(1, Ordering::Release); }
        state.0 = Arc::new(next);
        state.1 = state.1.wrapping_add(1);
        drop(state);
        if let Some(save) = &self.save && !self.queued.swap(true, Ordering::AcqRel) {
            let _ = save.send(SaveSettings::Changed);
        }
    }

    fn flush(&self) {
        if let Some(save) = &self.save {
            let (tx, rx) = std::sync::mpsc::channel();
            if save.send(SaveSettings::Flush(tx)).is_ok() { let _ = rx.recv_timeout(std::time::Duration::from_millis(100)); }
        }
    }
}

/// The app's libraries and scan; preference state is shared across instances.
pub struct Scanner {
    preferences: Arc<Preferences>,
    /// The scan asked for, and the one last started.
    wanted: AtomicU64,
    started: AtomicU64,
    progress: Mutex<Arc<Progress>>,
    /// The next scan first imports the libraries Kontakt knows about.
    import: AtomicBool,
    /// A finished scan and which it was; `None` inside when canceled.
    done: Arc<Mutex<Option<(u64, Option<Scanned>)>>>,
    sizes: Arc<Mutex<HashMap<PathBuf, Option<u64>>>>,
    /// Moves whenever something the editor shows arrives.
    stamp: Arc<AtomicU64>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            preferences: Preferences::shared(),
            wanted: AtomicU64::new(1),
            started: AtomicU64::new(0),
            progress: Mutex::default(),
            import: AtomicBool::new(false),
            done: Arc::default(),
            sizes: Arc::default(),
            stamp: Arc::default(),
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Scanner {
    /// Global settings shared by plugin instances, loaded once per process.
    pub fn settings(&self) -> Arc<Settings> { self.preferences.settings() }

    /// Publish immediately; save a coalesced snapshot off the UI thread.
    pub fn edit(&self, change: impl FnOnce(&mut Settings)) { self.preferences.edit(change); }

    /// Complete pending preference writes when an editor closes.
    pub fn flush_settings(&self) { self.preferences.flush(); }

    /// Called with native host logical dimensions; unchanged frames do no work.
    pub fn remember_window(&self, size: (u32, u32)) {
        if size.0 < 900 || size.1 < 600 || self.settings().window_size == Some(size) { return; }
        self.edit(|s| s.window_size = Some(size));
    }

    /// Add a root, unless it is there already, and scan again.
    pub fn add_root(&self, path: &Path, single: bool) {
        let path = path.to_string_lossy().trim_end_matches('/').to_owned();
        let path = if path.is_empty() { "/".to_owned() } else { path };
        self.edit(|s| match s.roots.iter_mut().find(|r| r.path == path) {
            Some(r) => r.single = single,
            None => s.roots.push(Root { path, single }),
        });
        self.rescan();
    }

    pub fn remove_root(&self, n: usize) {
        self.edit(|s| {
            if n < s.roots.len() {
                s.roots.remove(n);
            }
        });
        self.rescan();
    }

    /// Show `picture` (PNG or JPEG) as the cover of the library in `dir`:
    /// copied into the app's data folder, so it stays when the original moves.
    pub fn set_artwork(&self, dir: &Path, picture: &Path) -> Result<(), String> {
        let ext = picture.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg") {
            return Err("Choose a PNG or JPEG picture".into());
        }
        let size = std::fs::metadata(picture).map_err(|e| e.to_string())?.len();
        if size > 32 << 20 {
            return Err("The picture is over 32 MB".into());
        }
        let folder = data_dir().ok_or("No app data folder")?.join("artwork");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        let file = folder.join(format!("{:016x}-{stamp}.{ext}", fnv(&dir.to_string_lossy())));
        std::fs::copy(picture, &file).map_err(|e| e.to_string())?;
        self.set_cover(dir, Some(Cover::Custom { file: file.to_string_lossy().into_owned(), stamp }));
        Ok(())
    }

    /// The generated cover (`Some(Cover::Generated)`), or back to the
    /// library's own artwork (`None`). A picture chosen before is deleted.
    pub fn set_cover(&self, dir: &Path, cover: Option<Cover>) {
        let key = dir.to_string_lossy().into_owned();
        self.edit(|s| {
            let old = match cover {
                Some(cover) => s.covers.insert(key, cover),
                None => s.covers.remove(&key),
            };
            if let Some(Cover::Custom { file, .. }) = old {
                let _ = std::fs::remove_file(file);
            }
        });
    }

    /// Add registered Kontakt libraries and default UVI bank roots, and scan.
    pub fn import_kontakt(&self) {
        self.import.store(true, Ordering::Relaxed);
        self.rescan();
    }

    /// Ask for a new scan; the one running stops.
    pub fn rescan(&self) {
        self.wanted.fetch_add(1, Ordering::AcqRel);
        lock(&self.progress).cancel.store(true, Ordering::Relaxed);
    }

    /// Stop the scan running; the libraries found before stay.
    pub fn cancel(&self) {
        lock(&self.progress).cancel.store(true, Ordering::Relaxed);
    }

    /// The scan the editor should show.
    pub fn wanted(&self) -> u64 {
        self.wanted.load(Ordering::Acquire).wrapping_add(self.preferences.roots_changed.load(Ordering::Acquire))
    }

    /// Folders looked at and libraries found, while a scan runs.
    pub fn scanning(&self) -> Option<(usize, usize)> {
        let p = lock(&self.progress).clone();
        p.running.load(Ordering::Relaxed).then(|| (p.folders.load(Ordering::Relaxed), p.found.load(Ordering::Relaxed)))
    }

    /// A number that moves whenever a scan or a size makes headway.
    pub fn stamp(&self) -> u64 {
        let p = lock(&self.progress).clone();
        let folders = p.folders.load(Ordering::Relaxed) as u64;
        let preferences = self.preferences.state.read().unwrap_or_else(PoisonError::into_inner).1;
        self.stamp.load(Ordering::Relaxed).wrapping_add(preferences).wrapping_mul(31).wrapping_add(folders / 50)
    }

    /// For the loader: the scan finished since `installed`, once. Starts the
    /// scan wanted when it has not started.
    pub fn poll(&self, installed: u64) -> Option<(u64, Option<Scanned>)> {
        let want = self.wanted();
        if installed == want {
            return None;
        }
        if let Some(done) = lock(&self.done).take_if(|(g, _)| *g <= want) {
            if done.0 == want {
                if let Some(imported) = done.1.as_ref().and_then(|s| s.imported.clone()) {
                    self.edit(|s| {
                        for root in imported {
                            if !s.roots.iter().any(|r| r.path == root.path) {
                                s.roots.push(root);
                            }
                        }
                        s.imported = true;
                        s.uvi_imported = true;
                    });
                }
                return Some(done);
            }
        }
        if self.started.swap(want, Ordering::AcqRel) != want {
            self.start(want);
        }
        None
    }

    fn start(&self, generation: u64) {
        let progress = Arc::new(Progress::default());
        progress.running.store(true, Ordering::Relaxed);
        *lock(&self.progress) = progress.clone();
        let settings = self.settings();
        let mut roots = settings.roots.clone();
        // A first run, with nothing set up yet, starts from what Kontakt knows.
        let first = !settings.imported && roots.is_empty() && Settings::path().is_some();
        let import_kontakt = self.import.swap(false, Ordering::Relaxed) || first;
        let import_uvi = import_kontakt || (!settings.uvi_imported && Settings::path().is_some());
        // Multis saved with no library folder to keep them in.
        let multis = data_dir().map(|d| d.join("Multis")).filter(|d| d.is_dir());
        let done = self.done.clone();
        let nothing = roots.is_empty() && multis.is_none() && !import_kontakt && !import_uvi;
        let work = {
            let (progress, done, stamp) = (progress.clone(), done.clone(), self.stamp.clone());
            move || {
                let imported = (import_kontakt || import_uvi).then(|| {
                    let mut added = if import_kontakt { kontakt::roots(&roots) } else { Vec::new() };
                    let mut have = roots.clone();
                    have.extend(added.iter().cloned());
                    added.extend(kontakt::uvi_roots(&have));
                    added
                });
                roots.extend(imported.iter().flatten().cloned());
                if let Some(multis) = multis {
                    roots.push(Root { path: multis.to_string_lossy().into_owned(), single: true });
                }
                let cache_path = cache::Cache::path();
                let mut cache = cache::Cache::load(cache_path.as_deref());
                let scanned = cached_scan(&roots, &progress, &mut cache).map(|(mut shelf, files)| {
                    if let Err(error) = cache.save(cache_path.as_deref()) {
                        crate::diagnostics::resource(cache_path.as_deref().unwrap_or(Path::new("library index")), "library index", &error.to_string());
                    }
                    let artwork = crate::artwork::scan(&shelf.libraries);
                    for library in &mut shelf.libraries {
                        if !artwork.contains_key(&library.name) && !progress.canceled() {
                            library.hue = crate::artwork::own_hue(&library.dir);
                        }
                    }
                    let per_root = std::mem::take(&mut shelf.per_root);
                    let snapshots = std::mem::take(&mut shelf.snapshots);
                    let bank_issues = std::mem::take(&mut shelf.bank_issues);
                    let path_issues = std::mem::take(&mut shelf.path_issues);
                    let mut shelf = Shelf::new(shelf.libraries);
                    shelf.per_root = per_root;
                    shelf.snapshots = snapshots;
                    shelf.bank_issues = bank_issues;
                    shelf.path_issues = path_issues;
                    Scanned { shelf: Arc::new(shelf), files: Arc::new(files), artwork, imported }
                });
                let scanned = scanned.filter(|_| !progress.canceled());
                // A canceled scan finishing late never replaces a newer one.
                let mut done = lock(&done);
                if done.as_ref().is_none_or(|(g, _)| *g < generation) {
                    *done = Some((generation, scanned));
                }
                drop(done);
                progress.running.store(false, Ordering::Relaxed);
                stamp.fetch_add(1, Ordering::Relaxed);
            }
        };
        // Nothing to look through: done at once, no thread.
        if nothing {
            work();
        } else if std::thread::Builder::new().name("kontra-library-scan".into()).spawn(work).is_err() {
            progress.running.store(false, Ordering::Relaxed);
            *lock(&done) = Some((generation, None));
        }
    }

    /// Cached size only: displaying a library must never walk its sample tree.
    pub fn size(&self, dir: &Path) -> Option<u64> {
        lock(&self.sizes).get(dir).copied().flatten()
    }

    /// Explicit size measurement on its own worker.
    pub fn measure_size(&self, dir: &Path) -> Option<u64> {
        let mut sizes = lock(&self.sizes);
        if let Some(size) = sizes.get(dir) {
            return *size;
        }
        sizes.insert(dir.into(), None);
        let (sizes, stamp, dir) = (self.sizes.clone(), self.stamp.clone(), dir.to_path_buf());
        let _ = std::thread::Builder::new().name("kontra-library-size".into()).spawn(move || {
            let total = walkdir::WalkDir::new(&dir)
                .follow_links(false)
                .into_iter()
                .flatten()
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| e.metadata().ok())
                .map(|m| m.len())
                .sum();
            lock(&sizes).insert(dir, Some(total));
            stamp.fetch_add(1, Ordering::Relaxed);
        });
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn looking_up_library_size_does_not_walk_sample_directories() {
        let scanner = super::Scanner::default();
        assert_eq!(scanner.size(std::path::Path::new("/virtual/library")), None);
        assert!(super::lock(&scanner.sizes).is_empty(), "a browser lookup must never start a sample-directory walk");
    }

    #[test]
    fn library_names_sort_numbers_naturally() {
        let libraries = ["Library 10", "Library 2"].map(|name| super::Library { name: name.into(), dir: name.into(), ..Default::default() });
        let names: Vec<_> = super::Settings::default().arrange(&libraries).iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Library 2", "Library 10"]);
    }

    #[test]
    fn a_save_keeps_settings_this_version_does_not_know() {
        let path = std::env::temp_dir().join(format!("kontra-settings-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"version":2,"future_preference":"retained","ui_scale":1.5}"#).unwrap();
        let settings = super::Settings::load(&path).unwrap();
        settings.save(&path).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(saved["future_preference"], "retained");
        assert_eq!(saved["ui_scale"], 1.5);
    }

    use super::*;

    /// A fresh folder under the temp dir with `files` made in it.
    fn tree(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kontra-library-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in files {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn w10_uvi_import_flag_migrates_v2_settings_and_survives_restart() {
        let path = std::env::temp_dir().join(format!("kontra-uvi-import-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"version":2,"imported":true,"roots":[{"path":"/owned/Kontakt","single":false}]}"#).unwrap();
        let mut settings = Settings::load(&path).unwrap();
        assert!(settings.imported);
        assert!(!settings.uvi_imported, "old Kontakt import cannot suppress new UVI defaults");
        settings.uvi_imported = true;
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(settings));
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(feature = "library-access")]
    fn clear_bank(path: &Path) {
        fn append(bytes: &mut Vec<u8>, payload: &[u8]) -> u64 {
            let pointer = bytes.len() as u64 + 8;
            bytes.extend((payload.len() as u64).to_le_bytes());
            bytes.extend(payload);
            pointer
        }
        fn point(bytes: &mut [u8], at: u64, value: u64) {
            bytes[at as usize..at as usize + 8].copy_from_slice(&value.to_le_bytes());
        }
        let xml = b"<UVI4><Program Name=\"Our clear fixture\"/></UVI4>";
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
        for offset in [4, 12, 20] { point(&mut bytes, tree + offset, table); }
        let payload = append(&mut bytes, xml);
        point(&mut bytes, member + 260, xml.len() as u64);
        point(&mut bytes, member + 268, payload);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    #[cfg(feature = "library-access")]
    fn w10_uvi_catalog_does_not_prepare_unavailable_member_payloads() {
        let root = tree("uvi-catalog-only", &[]);
        let path = root.join("Owned.ufs");
        clear_bank(&path);
        let mut bytes = std::fs::read(&path).unwrap();
        let member = bytes.windows(4).position(|x| x == 0x675850e4u32.to_le_bytes()).unwrap();
        bytes[member + 276] = 2;
        std::fs::write(&path, bytes).unwrap();
        let (shelf, files) = scan(&[Root { path: root.to_string_lossy().into_owned(), single: false }], &Progress::default()).unwrap();
        assert_eq!((shelf.libraries.len(), files.len()), (1, 1), "cataloging requires directory metadata, not content preparation");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(feature = "library-access")]
    fn w10_uvi_flat_bank_folder_is_cataloged_and_cached() {
        let root = tree("uvi-flat-banks", &[]);
        clear_bank(&root.join("Alpha.ufs"));
        clear_bank(&root.join("Beta.UFS"));
        let roots = [Root { path: root.to_string_lossy().into_owned(), single: false }];
        let mut cache = cache::Cache::default();
        let (shelf, files) = cached_scan(&roots, &Progress::default(), &mut cache).unwrap();
        println!("UVI_FLAT libraries={} presets={}", shelf.libraries.len(), files.len());
        assert_eq!((shelf.libraries.len(), files.len()), (2, 2));
        let index = root.join("index.json");
        cache.save(Some(&index)).unwrap();
        let mut cache = cache::Cache::load(Some(&index));
        let (warm, warm_files) = cached_scan(&roots, &Progress::default(), &mut cache).unwrap();
        assert_eq!((warm.libraries.len(), warm_files), (2, files));
        assert_eq!(cache.stats.reads, 0, "unchanged accessible banks retain their catalog");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(feature = "library-access")]
    fn w10_uvi_banks_at_root_and_nested_libraries_are_all_cataloged() {
        let root = tree("uvi-root-banks", &[("Nested/Instruments/Owned.uvip", "<UVI4><Program/></UVI4>")]);
        clear_bank(&root.join("Alpha.ufs"));
        clear_bank(&root.join("Beta.UFS"));
        let (shelf, files) = scan(&[Root { path: root.to_string_lossy().into_owned(), single: false }], &Progress::default()).unwrap();
        assert_eq!(shelf.libraries.len(), 3, "loose banks must not disappear or hide nested libraries");
        assert_eq!(files.len(), 3);
        assert!(files.iter().all(|p| shelf.of(p).is_some()));
        assert!(shelf.libraries.iter().any(|l| l.name == "Alpha"));
        let (shelf, files) = scan(&[Root { path: root.join("Beta.UFS").to_string_lossy().into_owned(), single: true }], &Progress::default()).unwrap();
        assert_eq!((shelf.libraries.len(), files.len()), (1, 1));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "filesystem-only installed UVI discovery receipt; no bank payload access"]
    fn w10_uvi_installed_paths_receipt() {
        let root = Root { path: std::env::var("KONTRA_UVI_DISCOVERY_ROOT").expect("discovery root"), single: false };
        let detected = detect(&root, &Progress::default());
        let banks = walkdir::WalkDir::new(&root.path).into_iter().flatten()
            .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("ufs"))).count();
        println!("UVI_FILESYSTEM libraries={} containers={banks}", detected.len());
        assert!(!detected.is_empty() && banks > 0);
    }

    fn found(root: &Path, single: bool) -> Vec<(String, String, bool, usize)> {
        let roots = [Root { path: root.to_string_lossy().into(), single }];
        let (shelf, _) = scan(&roots, &Progress::default()).unwrap();
        shelf.libraries.iter().map(|l| (l.name.clone(), l.vendor.clone(), l.registered, l.instruments)).collect()
    }

    const NICNT: &str =
        "\u{0}\u{1}<ProductHints><Product><Name>Areia</Name><Company>Audio Imperia</Company></Product></ProductHints>";

    #[test]
    fn snapshot_catalog_keeps_instrument_counts_and_rejects_bad_metadata() {
        let root = tree("snapshot-catalog", &[
            ("Instruments/Piano.nki", "base"),
            ("Snapshots/Piano/Damaged.nksn", "not a snapshot"),
        ]);
        let (shelf, files) = scan(&[Root { path: root.to_string_lossy().into_owned(), single: true }], &Progress::default()).unwrap();
        assert_eq!(files, [root.join("Instruments/Piano.nki")]);
        assert_eq!(shelf.libraries[0].instruments, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incremental_catalog_reuses_product_metadata_and_drops_removed_presets() {
        let root = tree("incremental", &[("Product.nicnt", NICNT), ("Instruments/Piano.nki", "preset")]);
        let roots = [Root { path: root.to_string_lossy().into_owned(), single: true }];
        let index = root.join("index.json");
        let mut cache = cache::Cache::default();
        let (first, files) = cached_scan(&roots, &Progress::default(), &mut cache).unwrap();
        assert_eq!(cache.stats.reads, 1);
        cache.save(Some(&index)).unwrap();
        let mut cache = cache::Cache::load(Some(&index));
        let (second, next) = cached_scan(&roots, &Progress::default(), &mut cache).unwrap();
        assert_eq!(files, next);
        assert_eq!(first.libraries, second.libraries);
        assert_eq!((cache.stats.changed, cache.stats.reads), (0, 0));
        std::fs::remove_file(root.join("Instruments/Piano.nki")).unwrap();
        std::fs::write(root.join("Instruments/Organ.nki"), b"new preset").unwrap();
        let (_, changed) = cached_scan(&roots, &Progress::default(), &mut cache::Cache::load(Some(&index))).unwrap();
        assert_eq!(changed, [root.join("Instruments/Organ.nki")]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn w10_catalog_bank_errors_are_grouped_with_all_locations() {
        let _lease = crate::diagnostics::acquire();
        let root = tree("bank-errors", &[("A/Bad.ufs", "broken"), ("B/Bad.ufs", "broken")]);
        scan(&[Root { path: root.to_string_lossy().into_owned(), single: false }], &Progress::default()).unwrap();
        let snapshot = crate::diagnostics::snapshot();
        let records: Vec<_> = snapshot.events.iter().filter(|e| e.code.as_deref() == Some("bank_unreadable") && e.path.as_ref().is_some_and(|p| Path::new(p).starts_with(&root))).collect();
        assert_eq!(records.len(), 1, "one grouped diagnostic must retain every failed bank location");
        assert_eq!(records[0].details["locations"].as_array().unwrap().len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn w10_uvi_root_problem_counts_banks_and_keeps_distinct_causes() {
        let mut shelf = Shelf::default();
        shelf.bank_issues = vec![
            BankIssue { unsupported: false, message: "Truncated header".into(),
                locations: vec!["/owned/A.ufs".into(), "/owned/B.ufs".into(), "/owned-other/C.ufs".into()] },
            BankIssue { unsupported: false, message: "Invalid directory".into(), locations: vec!["/owned/D.ufs".into()] },
        ];
        assert_eq!(shelf.bank_problem(Path::new("/owned")).as_deref(),
            Some("3 UVI banks could not be cataloged: Truncated header; Invalid directory"));
        assert_eq!(shelf.bank_problem(Path::new("/owned/D.ufs")).as_deref(),
            Some("1 UVI bank could not be cataloged: Invalid directory"));
        assert_eq!(shelf.bank_problem(Path::new("/missing")), None);
    }

    #[test]
    fn unsupported_bank_errors_share_one_record_without_reader_access() {
        let progress = Progress::default();
        progress.bank_issue(Path::new("/virtual/First.ufs"), sampler_uvi::AccessError::Disabled);
        progress.bank_issue(Path::new("/virtual/Second.ufs"), sampler_uvi::AccessError::Disabled);
        progress.bank_issue(Path::new("/virtual/First.ufs"), sampler_uvi::AccessError::Disabled);
        let issues = lock(&progress.bank_issues);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues.keys().next().unwrap(), &(true, "protected library: not supported".into()));
        assert_eq!(issues.values().next().unwrap().len(), 2);
    }

    #[test]
    fn native_content_failures_retain_their_cause_without_unsupported_label() {
        let progress = Progress::default();
        progress.bank_issue(Path::new("/virtual/Corrupt.ufs"), sampler_uvi::AccessError::Content("invalid PNG checksum".into()));
        progress.bank_issue(Path::new("/virtual/Disabled.ufs"), sampler_uvi::AccessError::Disabled);
        let issues = lock(&progress.bank_issues);
        assert_eq!(issues.len(), 2);
        let content = issues.iter().find(|((unsupported, _), _)| !unsupported).unwrap();
        assert!(content.0.1.contains("invalid PNG checksum"));
        assert!(!content.0.1.contains("not supported"));
        assert_eq!(content.1.len(), 1);
    }

    #[test]
    #[ignore = "local metadata-only warm-start receipt; set KONTRA_CATALOG_RECEIPT"]
    fn w14_real_catalog_second_start() {
        let out = PathBuf::from(std::env::var_os("KONTRA_CATALOG_RECEIPT").expect("receipt"));
        let roots: Vec<Root> = ["/mnt/MAIN_STORAGE/Libraries/Kontakt", "/mnt/MAIN_STORAGE/Libraries/UVI"].into_iter()
            .map(|p| Root { path: p.into(), single: false }).collect();
        assert!(roots.iter().all(|r| Path::new(&r.path).is_dir()));
        let rep = std::env::var("KONTRA_CATALOG_REP").unwrap_or_else(|_| "0".into());
        let index = out.join(format!("library-index-v2-{rep}.json"));
        let mode = std::env::var("KONTRA_CATALOG_MODE").expect("before or after");
        let start: usize = std::env::var("KONTRA_CATALOG_START").unwrap().parse().unwrap();
        assert!(matches!(mode.as_str(), "before" | "after") && start < 2);
        let begin = std::time::Instant::now();
        let mut cache = if mode == "before" || start == 0 { cache::Cache::default() } else { cache::Cache::load(Some(&index)) };
        let (shelf, files) = cached_scan(&roots, &Progress::default(), &mut cache).unwrap();
        let ms = begin.elapsed().as_secs_f64() * 1000.;
        if mode == "after" { cache.save(Some(&index)).unwrap(); }
        eprintln!("CATALOG mode={mode} rep={rep} start={start} libraries={} roots={:?} presets={} elapsed_ms={ms:.3} changed={} metadata_reads={} reused={}",
            shelf.libraries.len(), shelf.per_root, files.len(), cache.stats.changed, cache.stats.reads, cache.stats.reused);
        if mode == "after" && start == 1 { assert_eq!((cache.stats.changed, cache.stats.reads), (0, 0)); }

    }


    #[test]
    fn libraries_are_found_with_or_without_a_library_file() {
        let root = tree(
            "parent",
            &[
                ("Areia 1.2.0/Areia.nicnt", NICNT),
                ("Areia 1.2.0/Instruments/Violins.nki", ""),
                ("Areia 1.2.0/Instruments/Violas.nki", ""),
                ("Areia 1.2.0/Samples/a.nkx", ""),
                ("Una Corda Library/Una Corda.nki", ""),
                ("Una Corda Library/Samples/c1.wav", ""),
                // A vendor folder of a product and a bundle of one more.
                ("Spitfire Audio/Olafur Arnalds Chamber Evolutions/Instruments/Evolutions.nki", ""),
                ("Spitfire Audio/Bundle/Tundra/Tundra.nki", ""),
                ("Spitfire Audio/Bundle/Tundra/Tundra.nkr", ""),
                // Samples only: no presets anywhere, not a library.
                ("Loose Samples/Samples/a.wav", ""),
                ("Loose Samples/b.wav", ""),
            ],
        );
        let mut got = found(&root, false);
        got.sort();
        assert_eq!(
            got,
            [
                ("Areia".into(), "Audio Imperia".into(), true, 2),
                ("Olafur Arnalds Chamber Evolutions".into(), "Spitfire Audio".into(), false, 1),
                ("Tundra".into(), "Spitfire Audio".into(), false, 1),
                ("Una Corda".into(), String::new(), false, 1),
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_sample_folder_is_no_library_and_a_single_root_is_one() {
        let root = tree("samples", &[("Loose/Samples/a.wav", ""), ("Loose/b.ncw", "")]);
        assert!(found(&root, false).is_empty());
        let lib = tree("single", &[("Presets/Tape Choir.nki", ""), ("Presets/Samples/a.wav", "")]);
        let got = found(&lib, true);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].3, 1);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(lib);
    }

    #[test]
    fn folder_names_lose_versions_brackets_and_packaging() {
        let cases = [
            ("Areia 1.2.0 [Audio Imperia]", ("Areia", "Audio Imperia")),
            ("Cinematic_Brass_Ensembles_v2.0.3", ("Cinematic Brass Ensembles", "")),
            ("Una Corda Library", ("Una Corda", "")),
            ("Tape Choir (KONTAKT)", ("Tape Choir", "")),
            ("Kinder Piano 1.1", ("Kinder Piano", "")),
            ("Kontakt", ("Kontakt", "")),
        ];
        for (folder, (name, vendor)) in cases {
            assert_eq!(clean_name(folder), (name.to_owned(), vendor.to_owned()), "{folder}");
        }
    }

    #[test]
    fn v2_view_mode_survives_restart_without_legacy_switch_migration() {
        let dir = tree("view-mode", &[]);
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"version":2,"vector_view":true,"vector_backdrop":true}"#).unwrap();
        assert_eq!(Settings::load(&path).unwrap().view_mode, ViewMode::Original);
        let settings = Settings { view_mode: ViewMode::Vectorized, ..Settings::default() };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(settings));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn editor_preferences_are_shared_coalesced_and_survive_restart() {
        let dir = tree("editor-preferences", &[]);
        let path = dir.join("settings.json");
        let preferences = Preferences::new(Some(path.clone()));
        let first = Scanner { preferences: preferences.clone(), ..Scanner::default() };
        let second = Scanner { preferences, ..Scanner::default() };
        let generation = second.wanted();
        first.edit(|s| s.ui_scale = 1.5);
        for width in 1200..1300 { first.remember_window((width, 900)); }
        assert_eq!(second.settings().editor_scale(), 1.5);
        assert_eq!(second.settings().editor_size(), (1299, 900));
        assert_eq!(second.wanted(), generation, "geometry does not rescan libraries");
        first.edit(|s| s.roots.push(Root { path: "/new-library".into(), single: true }));
        assert!(second.wanted() > generation, "shared folder edits wake other instances' scans");
        first.flush_settings();
        let loaded = Settings::load(&path).unwrap();
        assert_eq!(loaded.editor_scale(), 1.5);
        assert_eq!(loaded.editor_size(), (1299, 900));
        assert_eq!(loaded, *second.settings());
        let defaults: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(defaults.editor_scale(), 1.0);
        assert_eq!(defaults.editor_size(), (1180, 760));
        for (scale, expected) in [(f64::NAN, 1.0), (-1.0, 1.0), (0.25, 0.5), (100.0, 3.0)] {
            assert_eq!(Settings { ui_scale: scale, ..Settings::default() }.editor_scale(), expected);
        }
        assert_eq!(Settings { window_size: Some((0, 0)), ..Settings::default() }.editor_size(), (900, 600));
        drop(first); drop(second);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn library_names_are_display_only_and_survive_settings_roundtrip() {
        let libraries = [
            Library { dir: "/libs/Tubular Bell".into(), name: "Performance Samples  Library".into(), ..Default::default() },
            Library { dir: "/libs/Zebra".into(), name: "Zebra".into(), ..Default::default() },
        ];
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.library_name(&libraries[0]), "Tubular Bell");
        settings.rename_library("/libs/Tubular Bell", "  Zzz Library  ");
        assert_eq!(settings.library_name(&libraries[0]), "Zzz Library");
        assert_eq!(settings.arrange(&libraries).iter().map(|l| l.dir.clone()).collect::<Vec<_>>(), [libraries[1].dir.clone(), libraries[0].dir.clone()]);
        let restored: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        assert_eq!(libraries[0].name, "Performance Samples  Library");
        settings.rename_library("/libs/Tubular Bell", " ");
        assert!(settings.names.is_empty());
        assert_eq!(settings.library_name(&libraries[0]), "Tubular Bell");
    }

    #[test]
    fn roots_and_covers_survive_a_restart() {
        let dir = tree("settings", &[]);
        let path = dir.join("nested/settings.json");
        let mut s = Settings::default();
        s.roots.push(Root { path: "/libs".into(), single: false });
        s.roots.push(Root { path: "/one".into(), single: true });
        s.covers.insert("/libs/Areia".into(), Cover::Generated);
        s.order = vec!["/one".into(), "/libs/Areia".into()];
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(s));
        assert_eq!(Settings::load(&dir.join("missing.json")), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_dragged_order_survives_a_restart_and_new_libraries_follow_it() {
        let lib = |name: &str, vendor: &str| Library {
            dir: format!("/libs/{name}").into(),
            name: name.into(),
            vendor: vendor.into(),
            ..Library::default()
        };
        let all = [lib("Areia", "Audio Imperia"), lib("Kinder Piano", "Hollow Sun"), lib("Tundra", "Spitfire")];
        let names = |s: &Settings, libs: &[Library]| -> Vec<String> {
            s.arrange(libs).iter().map(|l| l.name.clone()).collect()
        };
        let mut s = Settings::default();
        assert_eq!(s.sort, Sort::Name);
        assert_eq!(names(&s, &all), ["Areia", "Kinder Piano", "Tundra"]);
        // Tundra dragged to the top: the order becomes the player's own.
        let shown: Vec<String> = s.arrange(&all).iter().map(|l| l.dir.to_string_lossy().into()).collect();
        s.reorder(&shown, "/libs/Tundra", Some("/libs/Areia"));
        assert_eq!(s.sort, Sort::Custom);
        assert_eq!(names(&s, &all), ["Tundra", "Areia", "Kinder Piano"]);

        let dir = tree("order", &[]);
        let path = dir.join("settings.json");
        s.save(&path).unwrap();
        let s = Settings::load(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(names(&s, &all), ["Tundra", "Areia", "Kinder Piano"], "kept across a restart");

        // A library found later follows the order; an offline one keeps its place.
        let mut more = all.to_vec();
        more.push(lib("Alpha", ""));
        assert_eq!(names(&s, &more), ["Tundra", "Areia", "Kinder Piano", "Alpha"]);
        let mut s = s;
        let shown: Vec<String> = s.arrange(&all[..2]).iter().map(|l| l.dir.to_string_lossy().into()).collect();
        s.reorder(&shown, "/libs/Areia", None);
        assert_eq!(s.order, ["/libs/Kinder Piano", "/libs/Areia", "/libs/Tundra"]);

        // Pinned leads; the other sorts. Dropped before a pinned one, it is pinned.
        s.pinned = vec!["/libs/Kinder Piano".into()];
        let shown: Vec<String> = s.arrange(&all).iter().map(|l| l.dir.to_string_lossy().into()).collect();
        s.reorder(&shown, "/libs/Tundra", Some("/libs/Kinder Piano"));
        assert_eq!(names(&s, &all), ["Tundra", "Kinder Piano", "Areia"]);
        s.reorder(&shown, "/libs/Tundra", None);
        assert_eq!(s.pinned, ["/libs/Kinder Piano"], "and dropped among the rest, not");
        s.sort = Sort::Vendor;
        assert_eq!(names(&s, &more), ["Kinder Piano", "Areia", "Tundra", "Alpha"]);
        s.pinned.clear();
        s.sort = Sort::Recent;
        s.used.insert("/libs/Tundra".into(), 10);
        s.used.insert("/libs/Alpha".into(), 20);
        assert_eq!(names(&s, &more), ["Alpha", "Tundra", "Areia", "Kinder Piano"]);
    }

    #[test]
    fn presets_are_shelved_by_their_folders() {
        let files: Vec<PathBuf> = [
            "/lib/Instruments/10 Shorts/Spiccato.nki",
            "/lib/Instruments/2 Legato/Legato.nki",
            "/lib/Instruments/2 Legato/Sub/Slow.nki",
            "/lib/Instruments/All.nki",
            "/lib/Loose.nki",
            "/elsewhere/Odd.nki",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        let root = Folder::tree(Path::new("/lib"), files.iter().map(PathBuf::as_path));
        assert_eq!(root.count, 6);
        assert_eq!(root.presets, [4, 5], "loose presets, and one outside, sit at the top");
        let instruments = &root.folders[0];
        assert_eq!((instruments.name.as_str(), instruments.count), ("Instruments", 4));
        assert_eq!(instruments.presets, [3]);
        let names: Vec<_> = instruments.folders.iter().map(|f| (f.name.as_str(), f.count)).collect();
        assert_eq!(names, [("2 Legato", 2), ("10 Shorts", 1)], "numbers sort by value");
        assert_eq!(instruments.folders[0].path, "/lib/Instruments/2 Legato");
        assert_eq!(instruments.folders[0].folders[0].presets, [2]);
        assert_eq!(Folder::tree(Path::new("/lib"), []).count, 0);
    }

    #[test]
    fn two_libraries_with_one_name_are_told_apart() {
        let lib = |dir: &str, vendor: &str| Library {
            dir: dir.into(),
            name: "Piano".into(),
            vendor: vendor.into(),
            registered: false,
            instruments: 1,
            multis: 0,
            hue: None,
        };
        let shelf = Shelf::new(vec![lib("/a/Piano", "Hollow Sun"), lib("/b/Piano", "Soniccouture")]);
        let names: BTreeSet<_> = shelf.libraries.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(shelf.of(Path::new("/b/Piano/Instruments/Grand.nki")).unwrap().vendor, "Soniccouture");
        assert!(shelf.of(Path::new("/c/Other.nki")).is_none());
    }
}

#[cfg(test)]
mod uvi_bank_tests {
    use super::*;

    /// A UVI bank lists its programs as virtual files. Needs the installed bank.
    #[test]
    fn a_ufs_bank_lists_its_programs() {
        let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(dir) = std::env::split_paths(&roots)
            .map(|r| r.parent().unwrap_or(&r).join("UVI/VWinds - Clarinets"))
            .find(|d| d.is_dir())
        else {
            return;
        };
        let (found, _) = presets(&dir, &Progress::default());
        let program = found.iter().find(|p| p.to_string_lossy().contains(".ufs/")).expect("a bank program");
        assert!(is_instrument(program));
    }
}

#[cfg(test)]
mod thread_setting_tests {
    use super::*;

    #[test]
    fn the_thread_setting_round_trips_and_defaults_to_the_audio_thread() {
        assert_eq!(Settings::default().threads, ThreadSetting::Single);
        let kept = serde_json::to_string(&Settings { threads: ThreadSetting::Fixed(4), ..Default::default() }).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&kept).unwrap().threads, ThreadSetting::Fixed(4));
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap().threads, ThreadSetting::Single);
    }
}

#[cfg(test)]
#[test]
fn v2_settings_reject_legacy_and_unversioned_files() {
    let path = std::env::temp_dir().join(format!("kontra-settings-version-{}.json", std::process::id()));
    for json in [r#"{"view_mode":"Original"}"#, r#"{"version":1}"#, r#"{"version":3}"#] {
        std::fs::write(&path, json).unwrap();
        assert!(Settings::load(&path).is_none(), "only the version-2 document is accepted");
    }
    std::fs::write(&path, r#"{"roots":[{"path":"old-library","single":true}],"ui_scale":1.5,"vector_view":true,"future_preference":"retained"}"#).unwrap();
    let settings = Settings::load(&path).unwrap_or_default();
    assert_eq!(settings, Settings::default(), "legacy fields never seed v2 defaults");
    settings.save(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(saved.get("version").and_then(|v| v.as_u64()), Some(2));
    std::fs::remove_file(path).unwrap();
}
