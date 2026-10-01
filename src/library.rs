//! Where the Kontakt libraries are and what they are called, found without
//! the player doing anything past naming a folder.
//!
//! The player adds roots, kept in the app's settings (not a project): a
//! folder of libraries, searched a few levels down, or one folder that is a
//! library. A folder is a library when it holds a `.nicnt` (registered), an
//! `Instruments` or `Multis` folder, presets beside a `Samples` folder or a
//! monolith (`.nkx`/`.nkc`/`.nkr`), or, below the root, presets of its own
//! (detected). The search stops at a library, skips sample folders, and gives
//! up on a folder of hundreds of audio files with no presets: a sample tree.
//! A library shows only when it holds a preset.
//!
//! Each is named from its `.nicnt` product, else its folder name cleaned of
//! versions, brackets and underscores; the vendor comes from the product, a
//! bracketed name or the vendor folder it sits in. Scans run on a thread of
//! their own, report progress, and can be canceled.
//!
//! On a first run with no roots, the libraries Kontakt knows about are added
//! (see [`kontakt`]); the player can import them again at any time.

use crate::import;

mod kontakt;
use moose::mui::mui::scene::Image;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

/// A folder the player added.
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
    /// Covers the player chose, by library folder.
    pub covers: BTreeMap<String, Cover>,
    /// The libraries Kontakt knows about were looked for, on the first run.
    pub imported: bool,
    /// Parts show KONTRA's own controls, not the library's original
    /// performance view, unless a part chooses otherwise.
    pub vector_view: bool,
    /// The original performance view's scale; 0 fits the part's width.
    pub view_scale: f32,
}

impl Settings {
    /// `settings.json` in the app's config folder; none under test.
    pub fn path() -> Option<PathBuf> {
        Some(config_dir()?.join("settings.json"))
    }

    pub fn load(path: &Path) -> Option<Self> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    }

    /// Written aside and renamed into place: a crash never leaves half a file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
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

/// The libraries found, looked up by folder or name.
#[derive(Default, Debug)]
pub struct Shelf {
    pub libraries: Vec<Library>,
    /// By folder, as text: looking a path up slices it, never parses it.
    by_dir: HashMap<String, usize>,
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
            .map(|(n, l)| (l.dir.to_string_lossy().trim_end_matches(['/', '\\']).to_owned(), n))
            .collect();
        let by_name = libraries.iter().enumerate().map(|(n, l)| (l.name.clone(), n)).collect();
        Self { libraries, by_dir, by_name, per_root: Vec::new() }
    }

    /// The library `path` is in: the nearest library folder above it.
    /// String slicing, not `Path` parsing: the browser asks for every preset
    /// on each rebuild.
    pub fn of(&self, path: &Path) -> Option<&Library> {
        let mut at = path.to_str()?;
        while let Some(cut) = at.rfind(['/', '\\']) {
            at = &at[..cut];
            if let Some(&n) = self.by_dir.get(at.trim_end_matches(['/', '\\'])) {
                return Some(&self.libraries[n]);
            }
        }
        None
    }

    pub fn named(&self, name: &str) -> Option<&Library> {
        self.by_name.get(name).map(|&n| &self.libraries[n])
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
}

impl Progress {
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
    audio: usize,
    folders: Vec<PathBuf>,
}

fn list(dir: &Path) -> Listing {
    let mut out = Listing::default();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.starts_with('.') {
            continue;
        }
        // Symbolic links are not followed: a loop would never end.
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            match name.as_str() {
                "instruments" | "multis" => out.instruments = true,
                "samples" => out.samples = true,
                _ => {}
            }
            out.folders.push(path);
            continue;
        }
        let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
        match ext {
            "nicnt" => out.nicnt = out.nicnt.take().or(Some(path)),
            "nkx" | "nkc" | "nkr" => out.monolith = true,
            "nki" | "nkm" => out.presets += 1,
            "wav" | "ncw" | "aif" | "aiff" | "flac" | "ogg" => out.audio += 1,
            _ if import::is_multi(&path) => out.presets += 1,
            _ => {}
        }
    }
    out.folders.sort();
    out
}

/// A library folder found under a root.
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
    if root.single {
        let listing = list(&dir);
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
    let l = list(dir);
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

/// Presets in a library folder, its sample folders left unread.
fn presets(dir: &Path, progress: &Progress) -> Vec<PathBuf> {
    let walk = walkdir::WalkDir::new(dir).follow_links(false).into_iter().filter_entry(|e| {
        !(e.depth() > 0 && e.file_type().is_dir() && {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name == "samples" || name.starts_with('.')
        })
    });
    let mut out = Vec::new();
    for e in walk.flatten() {
        if progress.canceled() {
            break;
        }
        if e.file_type().is_dir() {
            progress.folders.fetch_add(1, Ordering::Relaxed);
        }
        let path = e.path();
        if e.file_type().is_file()
            && (path.extension().is_some_and(|x| x.eq_ignore_ascii_case("nki")) || import::is_multi(path))
        {
            out.push(e.into_path());
        }
    }
    out
}

/// Every library in `roots` and its presets; `None` once canceled.
pub fn scan(roots: &[Root], progress: &Progress) -> Option<(Shelf, Vec<PathBuf>)> {
    let mut libraries: Vec<Library> = Vec::new();
    let mut files = BTreeSet::new();
    let mut per_root = Vec::new();
    let mut seen = BTreeSet::new();
    for root in roots {
        let before = libraries.len();
        for c in detect(root, progress) {
            let key = std::fs::canonicalize(&c.dir).unwrap_or(c.dir.clone());
            if !seen.insert(key) {
                continue;
            }
            let found = presets(&c.dir, progress);
            if progress.canceled() {
                return None;
            }
            if found.is_empty() {
                continue;
            }
            let (folder_name, bracket) = clean_name(&file_name(&c.dir));
            let product = c.nicnt.as_deref().and_then(product);
            let (name, company) = product.unwrap_or_default();
            let vendor = [company, bracket, c.vendor.unwrap_or_default()]
                .into_iter()
                .find(|v| !v.is_empty())
                .unwrap_or_default();
            let multis = found.iter().filter(|p| import::is_multi(p)).count();
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
        per_root.push(libraries.len() - before);
    }
    let mut shelf = Shelf::new(libraries);
    shelf.per_root = per_root;
    Some((shelf, files.into_iter().collect()))
}

/// Everything a finished scan hands the editor.
pub struct Scanned {
    pub shelf: Arc<Shelf>,
    pub files: Arc<Vec<PathBuf>>,
    pub artwork: HashMap<String, Arc<Image>>,
    /// The roots an import from Kontakt added, when the scan made one.
    pub imported: Option<Vec<Root>>,
}

/// The app's libraries: the settings naming them, the scan finding them,
/// and each library's size on disk, measured when first asked.
pub struct Scanner {
    settings: RwLock<Option<Arc<Settings>>>,
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
            settings: RwLock::default(),
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
    /// The settings, read from disk on first use.
    pub fn settings(&self) -> Arc<Settings> {
        if let Some(s) = self.settings.read().unwrap_or_else(PoisonError::into_inner).as_ref() {
            return s.clone();
        }
        let mut slot = self.settings.write().unwrap_or_else(PoisonError::into_inner);
        slot.get_or_insert_with(|| {
            Arc::new(Settings::path().and_then(|p| Settings::load(&p)).unwrap_or_default())
        })
        .clone()
    }

    /// Change the settings and save them.
    pub fn edit(&self, change: impl FnOnce(&mut Settings)) {
        let mut next = (*self.settings()).clone();
        change(&mut next);
        if let Some(path) = Settings::path() {
            let _ = next.save(&path);
        }
        *self.settings.write().unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(next));
        self.stamp.fetch_add(1, Ordering::Relaxed);
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

    /// Add the libraries Kontakt knows about that are not here yet, and scan.
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
        self.wanted.load(Ordering::Acquire)
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
        self.stamp.load(Ordering::Relaxed).wrapping_mul(31).wrapping_add(folders / 50)
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
        let import = self.import.swap(false, Ordering::Relaxed) || first;
        // Multis saved with no library folder to keep them in.
        let multis = data_dir().map(|d| d.join("Multis")).filter(|d| d.is_dir());
        let done = self.done.clone();
        let nothing = roots.is_empty() && multis.is_none() && !import;
        let work = {
            let (progress, done, stamp) = (progress.clone(), done.clone(), self.stamp.clone());
            move || {
                let imported = import.then(|| kontakt::roots(&roots));
                roots.extend(imported.iter().flatten().cloned());
                if let Some(multis) = multis {
                    roots.push(Root { path: multis.to_string_lossy().into_owned(), single: true });
                }
                let scanned = scan(&roots, &progress).map(|(mut shelf, files)| {
                    let artwork = crate::artwork::scan(&shelf.libraries);
                    for library in &mut shelf.libraries {
                        if !artwork.contains_key(&library.name) && !progress.canceled() {
                            library.hue = crate::artwork::own_hue(&library.dir);
                        }
                    }
                    let per_root = std::mem::take(&mut shelf.per_root);
                    let mut shelf = Shelf::new(shelf.libraries);
                    shelf.per_root = per_root;
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

    /// `dir`'s size on disk once measured; the first ask measures it on a
    /// thread of its own.
    pub fn size(&self, dir: &Path) -> Option<u64> {
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

    fn found(root: &Path, single: bool) -> Vec<(String, String, bool, usize)> {
        let roots = [Root { path: root.to_string_lossy().into(), single }];
        let (shelf, _) = scan(&roots, &Progress::default()).unwrap();
        shelf.libraries.iter().map(|l| (l.name.clone(), l.vendor.clone(), l.registered, l.instruments)).collect()
    }

    const NICNT: &str =
        "\u{0}\u{1}<ProductHints><Product><Name>Areia</Name><Company>Audio Imperia</Company></Product></ProductHints>";

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
    fn roots_and_covers_survive_a_restart() {
        let dir = tree("settings", &[]);
        let path = dir.join("nested/settings.json");
        let mut s = Settings::default();
        s.roots.push(Root { path: "/libs".into(), single: false });
        s.roots.push(Root { path: "/one".into(), single: true });
        s.covers.insert("/libs/Areia".into(), Cover::Generated);
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), Some(s));
        assert_eq!(Settings::load(&dir.join("missing.json")), None);
        let _ = std::fs::remove_dir_all(dir);
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
