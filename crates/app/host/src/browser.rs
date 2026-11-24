//! The browser's side on the control thread: the player's library roots,
//! scanned on a thread of their own into lane F's [`Catalog`], and what the
//! player keeps between sessions ([`Prefs`]: roots, stars, recent). A first
//! run starts from v2's roots, read and never written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use std::time::{Duration, Instant};

use kontra_library::{Cache, Catalog, Library, Prefs, Progress, Root, discover, index};

/// How often [`Browser::poll`] checks, off thread, whether the roots or
/// libraries changed on disk ([`index::stale`]).
const WATCH: Duration = Duration::from_secs(30);

/// Whether `path` is a Kontakt instrument file.
fn nki(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
}

/// Where v3 keeps its own settings, beside v2's `kontra`.
const FOLDER: &str = "kontra-v3";

/// What the scan thread does before the catalog scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Find {
    /// Scan the roots already known.
    Nothing,
    /// Look up registrations and defaults, and walk home and the volumes
    /// briefly ([`discover::quick`]).
    Quick,
    /// Walk the whole machine ([`discover::machine`]).
    Machine,
}

/// What a scan thread sends back.
struct Scanned {
    catalog: Catalog,
    /// The roots [`Find::Quick`] found.
    auto: Option<Vec<Root>>,
    /// The roots [`Find::Machine`] found.
    found: Option<Vec<Root>>,
    /// Libraries found in a new folder, as (old folder, new folder).
    moved: Vec<(PathBuf, PathBuf)>,
}

/// A running scan: how far it is, what it finds first, and its result.
struct Running {
    progress: Arc<Progress>,
    find: Find,
    result: mpsc::Receiver<Scanned>,
}

/// One scanned folder as the editor lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderStatus {
    pub path: PathBuf,
    /// Found by discovery, not added by the player: removing it hides it.
    pub auto: bool,
    /// "3 libraries found", "not found"…; empty until a scan has run.
    pub summary: String,
    /// What to do about it (the Flatpak override, mount permissions).
    pub hint: Option<String>,
    /// The folder could not be read, or held no library.
    pub problem: bool,
}

pub struct Browser {
    pub prefs: Prefs,
    pub catalog: Catalog,
    /// Moves whenever the catalog or the prefs change.
    pub generation: u64,
    scan: Option<Running>,
    /// Discovery runs at all ([`Browser::discover`]); never in tests.
    discovering: bool,
    /// The next scan looks up registrations and defaults first.
    quick_due: bool,
    /// What the last quick discovery found.
    auto: Vec<Root>,
    /// The Flatpak app KONTRA runs in, for the override hint.
    flatpak: Option<String>,
    prefs_path: Option<PathBuf>,
    cache_path: Option<PathBuf>,
    /// The library index ([`index`]): the catalog as last known, loaded
    /// before any scan.
    index_path: Option<PathBuf>,
    /// A running check of whether anything changed on disk, and when the
    /// last one started.
    watch: (Option<mpsc::Receiver<bool>>, Instant),
    /// The library [`Browser::locate`] looks for.
    locating: Option<String>,
    /// What the player should hear about, for the report.
    pub notices: Vec<String>,
    /// The player's presets folder ([`crate::presets`]), listed after each
    /// library's own presets.
    pub user: Option<PathBuf>,
}

impl Browser {
    /// The player's prefs from `config` (v2's roots on a first run), with a
    /// scan started; `None` folders keep nothing on disk.
    #[must_use]
    pub fn open(config: Option<&Path>, cache: Option<&Path>) -> Self {
        let prefs_path = config.map(|c| c.join(FOLDER).join("prefs.json"));
        let prefs = prefs_path
            .as_deref()
            .and_then(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok())
            .unwrap_or_else(|| Prefs {
                roots: config.map(v2_roots).unwrap_or_default(),
                ..Prefs::default()
            });
        let index_path = config.map(|c| c.join(FOLDER).join("library-index.json"));
        let libraries = index_path.as_deref().map(index::load).unwrap_or_default();
        let mut browser = Self {
            prefs,
            catalog: Catalog {
                libraries,
                ..Catalog::default()
            },
            generation: 1,
            scan: None,
            discovering: false,
            quick_due: false,
            auto: Vec::new(),
            flatpak: discover::flatpak_app(),
            prefs_path,
            cache_path: cache.map(|c| c.join(FOLDER).join("bank-cache.json")),
            index_path,
            watch: (None, Instant::now()),
            locating: None,
            notices: Vec::new(),
            user: None,
        };
        browser.rescan();
        browser
    }

    /// Finds library folders on its own from now on: registrations,
    /// defaults and a brief walk before the next scan and each
    /// [`Browser::refresh`].
    pub fn discover(&mut self) {
        self.discovering = true;
        self.quick_due = true;
        self.rescan();
    }

    /// The Rescan button: discovers again, when discovering, and scans.
    pub fn refresh(&mut self) {
        self.quick_due = self.discovering;
        self.rescan();
    }

    /// Walks the whole machine for libraries, keeps what it finds as auto
    /// roots, and scans. [`Browser::cancel`] stops it.
    pub fn scan_machine(&mut self) {
        self.start(Find::Machine);
    }

    /// Stops a running scan; the catalog stays as it was.
    pub fn cancel(&mut self) {
        if let Some(running) = self.scan.take() {
            running.progress.cancel.store(true, Ordering::Relaxed);
            self.generation += 1;
        }
    }

    /// A running scan's progress: folders listed, libraries found, and
    /// whether it walks the whole machine.
    #[must_use]
    pub fn progress(&self) -> Option<(usize, usize, bool)> {
        let r = self.scan.as_ref()?;
        Some((
            r.progress.folders.load(Ordering::Relaxed),
            r.progress.found.load(Ordering::Relaxed),
            r.find == Find::Machine,
        ))
    }

    /// Every root scanned: the player's, then the auto ones not hidden or
    /// covered.
    #[must_use]
    pub fn roots(&self) -> Vec<Root> {
        let mut auto = self.auto.clone();
        auto.extend(self.prefs.found.iter().cloned());
        discover::merge(&self.prefs.roots, &auto, &self.prefs.hidden)
    }

    /// Every root and how its last scan went.
    #[must_use]
    pub fn folders(&self) -> Vec<FolderStatus> {
        (self.roots().into_iter())
            .map(|root| {
                let status = (self.catalog.roots.iter()).find(|s| s.path == root.path);
                FolderStatus {
                    summary: status
                        .map(kontra_library::RootStatus::summary)
                        .unwrap_or_default(),
                    hint: status.and_then(|s| s.hint(self.flatpak.as_deref())),
                    problem: status.is_some_and(|s| s.error.is_some() || s.libraries == 0),
                    auto: root.auto,
                    path: root.path,
                }
            })
            .collect()
    }

    /// Scans the roots again; a scan still running is stopped.
    pub fn rescan(&mut self) {
        let find = if std::mem::take(&mut self.quick_due) {
            Find::Quick
        } else {
            Find::Nothing
        };
        self.start(find);
    }

    fn start(&mut self, find: Find) {
        if let Some(running) = self.scan.take() {
            running.progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(Progress::default());
        let (send, receive) = mpsc::channel();
        let (user, known, hidden) = (
            self.prefs.roots.clone(),
            self.auto.clone(),
            self.prefs.hidden.clone(),
        );
        let found = self.prefs.found.clone();
        let cache_path = self.cache_path.clone();
        let index_path = self.index_path.clone();
        let previous = self.catalog.libraries.clone();
        let scanning = Arc::clone(&progress);
        let spawned = std::thread::Builder::new()
            .name("kontra-scan".into())
            .spawn(move || {
                let (mut auto, mut found_now) = (None, None);
                let mut all = known;
                match find {
                    Find::Nothing => all.extend(found),
                    Find::Quick => {
                        let quick = discover::quick(&scanning);
                        all = quick.clone();
                        all.extend(found);
                        auto = Some(quick);
                    }
                    Find::Machine => {
                        let machine = discover::machine(&scanning);
                        if scanning.cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        all.extend(machine.iter().cloned());
                        found_now = Some(machine);
                    }
                }
                // The catalog scan counts its own folders and libraries.
                scanning.folders.store(0, Ordering::Relaxed);
                scanning.found.store(0, Ordering::Relaxed);
                let roots = discover::merge(&user, &all, &hidden);
                let mut cache = cache_path.as_deref().map(Cache::load).unwrap_or_default();
                cache.invalidate_protected(kontra_format_uvi::has_cached_key);
                let inspect = |b: &Path| {
                    let sf = |e: &std::ffi::OsStr| {
                        e.eq_ignore_ascii_case("sf2") || e.eq_ignore_ascii_case("sf3")
                    };
                    if b.extension().is_some_and(sf) {
                        kontra_format_sf2::list_presets(b)
                            .map(|list| kontra_library::BankListing {
                                presets: list
                                    .into_iter()
                                    .map(|p| {
                                        let trimmed = p.name.trim();
                                        let safe = if trimmed.is_empty() {
                                            "Preset".to_string()
                                        } else {
                                            trimmed.replace(['/', '\\'], "-")
                                        };
                                        format!("{:03}:{:03} {safe}.sf2p", p.bank, p.program)
                                    })
                                    .collect(),
                                protection: kontra_library::ProtectionStatus::Open,
                            })
                            .map_err(|e| kontra_library::BankError::failed(e.to_string()))
                    } else {
                        kontra_format_uvi::inspect_programs(b)
                            .map(|(presets, protection)| kontra_library::BankListing {
                                presets,
                                protection,
                            })
                            .map_err(|e| match e.protection() {
                                Some(status) => {
                                    kontra_library::BankError::protected(status.clone())
                                }
                                None => kontra_library::BankError::failed(e.to_string()),
                            })
                    }
                };
                if let Some(mut catalog) = kontra_library::scan_incremental(
                    &roots, &scanning, &mut cache, &inspect, &previous,
                ) {
                    let moved = index::reconcile(&previous, &mut catalog, &roots);
                    if let Some(path) = &cache_path {
                        let _saved = path.parent().map(std::fs::create_dir_all);
                        let _saved = cache.save(path);
                    }
                    if let Some(path) = &index_path {
                        let _saved = index::save(path, &catalog.libraries);
                    }
                    let _gone = send.send(Scanned {
                        catalog,
                        auto,
                        found: found_now,
                        moved,
                    });
                }
            });
        if spawned.is_ok() {
            self.scan = Some(Running {
                progress,
                find,
                result: receive,
            });
        }
    }

    /// Takes a finished scan; true when the catalog changed. A scan that
    /// found what was already listed moves nothing, so an idle editor
    /// draws no frame for it.
    pub fn poll(&mut self) -> bool {
        self.watch_disk();
        let Some(scanned) = self.scan.as_ref().and_then(|r| r.result.try_recv().ok()) else {
            return false;
        };
        self.scan = None;
        self.watch.1 = Instant::now();
        for (from, to) in scanned.moved {
            self.relink(from, to);
        }
        if let Some(id) = self.locating.take()
            && let Some(l) = scanned.catalog.by_id(&id).filter(|l| l.offline.is_some())
        {
            self.notices.push(format!(
                "{} is not in that folder (it holds another library, or none); it is still missing",
                l.name
            ));
        }
        if let Some(auto) = scanned.auto {
            self.auto = auto;
        }
        if let Some(found) = scanned.found {
            self.prefs.found = found;
            self.changed();
        }
        let mut catalog = scanned.catalog;
        self.with_user(&mut catalog);
        if catalog == self.catalog {
            return false;
        }
        self.catalog = catalog;
        self.generation += 1;
        true
    }

    /// Lists the player's presets from `folder` from now on.
    pub fn set_user(&mut self, folder: PathBuf) {
        self.user = Some(folder);
        self.user_changed();
    }

    /// Lists the player's presets again, after one was saved, renamed or
    /// deleted.
    pub fn user_changed(&mut self) {
        let mut catalog = std::mem::take(&mut self.catalog);
        self.with_user(&mut catalog);
        self.catalog = catalog;
        self.generation += 1;
    }

    /// Each library's user presets after its own, then Kontakt's user
    /// snapshots of its instruments.
    fn with_user(&self, catalog: &mut Catalog) {
        let kontakt = self.kontakt();
        for l in &mut catalog.libraries {
            let ours = |p: &PathBuf| {
                [&self.user, &kontakt]
                    .iter()
                    .any(|f| f.as_ref().is_some_and(|f| p.starts_with(f)))
            };
            l.presets.retain(|p| !ours(p));
            if let Some(user) = &self.user {
                l.presets.extend(crate::presets::list(user, &l.name));
            }
            if let Some(kontakt) = &kontakt {
                let instruments = (l.presets.iter()).filter(|p| nki(p)).map(PathBuf::as_path);
                let snapshots = kontra_library::presets::snapshots(kontakt, instruments);
                l.presets.extend(snapshots);
            }
        }
    }

    /// Kontakt's user snapshot folder: the player's choice, else Kontakt's
    /// own once the player's presets are listed (never in tests).
    #[must_use]
    pub fn kontakt(&self) -> Option<PathBuf> {
        let own = || {
            self.user
                .as_ref()
                .and_then(|_| kontra_library::presets::kontakt_folder())
        };
        self.prefs.kontakt_snapshots.clone().or_else(own)
    }

    /// Every instrument of the catalog, by file ([`crate::rack::Rack::know`]).
    #[must_use]
    pub fn instruments(&self) -> Vec<PathBuf> {
        (self
            .catalog
            .libraries
            .iter()
            .flat_map(|l| l.presets.iter().filter(|p| nki(p))))
        .cloned()
        .collect()
    }

    /// Whether a scan is running.
    #[must_use]
    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Preset `preset` of library `library`, as a path to load.
    #[must_use]
    pub fn preset(&self, library: usize, preset: usize) -> Option<PathBuf> {
        self.catalog
            .libraries
            .get(library)?
            .presets
            .get(preset)
            .cloned()
    }

    /// The player loaded `path`: first among the recent. If the library holding
    /// `path` was marked protected, triggers a rescan so unlocked banks clear
    /// their stale protection badge.
    pub fn loaded(&mut self, path: &Path) {
        let was_protected = self
            .find(path)
            .and_then(|(l, _)| self.catalog.libraries.get(l))
            .is_some_and(|l| l.protection.is_protected());
        self.prefs.loaded(path);
        self.changed();
        if was_protected {
            self.rescan();
        }
    }

    pub fn star(&mut self, library: usize, on: bool) {
        if let Some(dir) = self.catalog.libraries.get(library).map(|l| l.dir.clone()) {
            self.prefs.star(&dir, on);
            self.changed();
        }
    }

    /// Stars or unstars preset `preset` of library `library`.
    pub fn star_preset(&mut self, library: usize, preset: usize, on: bool) {
        if let Some(path) = self.preset(library, preset) {
            self.prefs.star(&path, on);
            self.changed();
        }
    }

    /// Remembers that the library folder `from` is now at `to`.
    pub fn relink(&mut self, from: PathBuf, to: PathBuf) {
        let follow = |p: &mut PathBuf| {
            if let Ok(rest) = p.strip_prefix(&from) {
                *p = to.join(rest);
            }
        };
        self.prefs.recent.iter_mut().for_each(follow);
        self.prefs.starred.iter_mut().for_each(follow);
        self.prefs.moved.retain(|(f, _)| *f != from);
        self.prefs.moved.push((from, to));
        self.changed();
    }

    /// Every library's identity and folder, offline ones too
    /// ([`crate::rack::Rack::know`]).
    #[must_use]
    pub fn identities(&self) -> Vec<(String, PathBuf, bool)> {
        (self.catalog.libraries.iter())
            .map(|l| (l.id.clone(), l.dir.clone(), l.offline.is_none()))
            .collect()
    }

    /// The player says offline library `library` is now in `folder`: it is
    /// scanned (as a root, unless one covers it), and the library found
    /// there by its identity is relocated ([`index::reconcile`]). A folder
    /// holding another library changes nothing, and says so.
    pub fn locate(&mut self, library: usize, folder: PathBuf) {
        let Some(l) = self.catalog.libraries.get(library) else {
            return;
        };
        self.locating = Some(l.id.clone());
        // The folder is the library itself when its first preset is there.
        let single = (l.presets.first())
            .and_then(|p| p.strip_prefix(&l.dir).ok())
            .is_some_and(|rel| folder.join(rel).exists());
        if self.roots().iter().any(|r| folder.starts_with(&r.path)) {
            self.rescan();
            return;
        }
        self.prefs.hidden.retain(|h| *h != folder);
        self.prefs.add_root(Root {
            path: folder,
            single,
            auto: false,
        });
        self.changed();
        self.rescan();
    }

    /// Every [`WATCH`], when no scan runs, checks on a thread of its own
    /// whether a root or library changed (a drive mounted, a library
    /// added or moved), and scans again when one did.
    fn watch_disk(&mut self) {
        if let Some(changed) = self.watch.0.as_ref().and_then(|r| r.try_recv().ok()) {
            self.watch.0 = None;
            if changed && self.scan.is_none() {
                self.rescan();
            }
        }
        if self.scan.is_some() || self.watch.0.is_some() || self.watch.1.elapsed() < WATCH {
            return;
        }
        self.watch.1 = Instant::now();
        // Only what the check reads: no preset lists.
        let seen = Catalog {
            libraries: (self.catalog.libraries.iter())
                .map(|l| Library {
                    dir: l.dir.clone(),
                    offline: l.offline.clone(),
                    stamps: l.stamps.clone(),
                    ..Library::default()
                })
                .collect(),
            issues: Vec::new(),
            roots: self.catalog.roots.clone(),
        };
        let (send, receive) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("kontra-watch".into())
            .spawn(move || {
                let _gone = send.send(index::stale(&seen));
            });
        if spawned.is_ok() {
            self.watch.0 = Some(receive);
        }
    }

    /// Adds a folder the player dropped, and scans again.
    pub fn add_root(&mut self, path: PathBuf) {
        self.prefs.hidden.retain(|h| *h != path);
        self.prefs.add_root(Root {
            path,
            single: false,
            auto: false,
        });
        self.changed();
        self.rescan();
    }

    /// Where new parts listen and play ([`Prefs::next_channel`]).
    pub fn new_parts(&mut self, next_channel: bool, own_output: bool) {
        (self.prefs.next_channel, self.prefs.own_output) = (next_channel, own_output);
        self.changed();
    }

    /// Stops scanning folder `n` of [`Browser::roots`], and scans again: the
    /// player's own is removed, an auto one hidden for good.
    pub fn remove_root(&mut self, n: usize) {
        let Some(root) = self.roots().into_iter().nth(n) else {
            return;
        };
        if root.auto {
            self.prefs.found.retain(|r| r.path != root.path);
            self.prefs.hidden.push(root.path);
        } else {
            self.prefs.roots.retain(|r| r.path != root.path);
        }
        self.changed();
        self.rescan();
    }

    /// Reads Kontakt's user snapshots from `folder` from now on (saved
    /// with the prefs), and lists them again.
    pub fn set_kontakt(&mut self, folder: PathBuf) {
        self.prefs.kontakt_snapshots = Some(folder);
        self.changed();
        self.user_changed();
    }

    /// Recent instruments as (library, preset) in the catalog.
    #[must_use]
    pub fn recent(&self) -> Vec<(usize, usize)> {
        (self.prefs.recent.iter())
            .filter_map(|path| self.find(path))
            .collect()
    }

    /// Where the catalog lists `path`: (library, preset).
    #[must_use]
    pub fn find(&self, path: &Path) -> Option<(usize, usize)> {
        (self.catalog.libraries.iter().enumerate())
            .find_map(|(library, l)| Some((library, l.presets.iter().position(|p| p == path)?)))
    }

    /// The editor's colour tokens (`theme.json` beside the prefs, token →
    /// `#rrggbb`); empty when none are saved. A file of their own, so another
    /// instance saving its prefs never writes back older colours.
    #[must_use]
    pub fn theme(&self) -> BTreeMap<String, String> {
        self.theme_path()
            .and_then(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok())
            .unwrap_or_default()
    }

    /// Saves the editor's colour tokens ([`Self::theme`]).
    pub fn set_theme(&self, tokens: &BTreeMap<String, String>) {
        if let Some(path) = self.theme_path() {
            save(&path, tokens);
        }
    }

    fn theme_path(&self) -> Option<PathBuf> {
        Some(self.prefs_path.as_ref()?.with_file_name("theme.json"))
    }

    fn changed(&mut self) {
        self.generation += 1;
        if let Some(path) = &self.prefs_path {
            save(path, &self.prefs);
        }
    }
}

/// `value` as JSON at `path`: written whole to a unique temporary file, then
/// renamed over the old file.
fn save(path: &Path, value: &impl serde::Serialize) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let Ok(json) = serde_json::to_vec_pretty(value) else {
        return;
    };
    let _saved = path.parent().map(std::fs::create_dir_all);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("prefs.json");
    let tmp = path.with_file_name(format!("{file_name}.tmp.{}.{seq}", std::process::id()));
    if std::fs::write(&tmp, json).is_err() || std::fs::rename(&tmp, path).is_err() {
        let _removed = std::fs::remove_file(&tmp);
    }
}

/// The roots v2 scans (`<config>/kontra/settings.json`), read only.
fn v2_roots(config: &Path) -> Vec<Root> {
    #[derive(serde::Deserialize)]
    struct V2 {
        #[serde(default)]
        roots: Vec<Root>,
    }
    std::fs::read(config.join("kontra").join("settings.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<V2>(&b).ok())
        .map(|v| v.roots)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_run_starts_from_v2s_roots_and_keeps_its_own_prefs() {
        let dir = std::env::temp_dir().join(format!("kontra-browser-{}", std::process::id()));
        let v2 = dir.join("kontra");
        std::fs::create_dir_all(&v2).unwrap();
        std::fs::write(
            v2.join("settings.json"),
            r#"{"version": 2, "roots": [{"path": "/nowhere/Kontakt", "single": false}]}"#,
        )
        .unwrap();
        let mut b = Browser::open(Some(&dir), None);
        assert_eq!(b.prefs.roots.len(), 1);
        b.add_root("/nowhere/UVI".into());
        let again = Browser::open(Some(&dir), None);
        assert_eq!(again.prefs.roots.len(), 2, "v3 keeps its own prefs");
        let v2_after = std::fs::read_to_string(v2.join("settings.json")).unwrap();
        assert!(!v2_after.contains("UVI"), "v2's settings are never written");
        b.remove_root(0);
        let roots = Browser::open(Some(&dir), None).prefs.roots;
        assert_eq!(roots.len(), 1, "a removed folder stays removed");
        assert_eq!(roots[0].path, Path::new("/nowhere/UVI"));
        // Kontakt's snapshot folder, once chosen, is the one read from then on.
        b.set_kontakt("/nowhere/User Content".into());
        let again = Browser::open(Some(&dir), None);
        assert_eq!(
            again.kontakt().as_deref(),
            Some(Path::new("/nowhere/User Content"))
        );
        // The colours: saved apart, and not undone by another prefs save.
        assert!(again.theme().is_empty(), "no colours saved yet");
        let tokens = BTreeMap::from([("accent".to_owned(), "#ff8000".to_owned())]);
        again.set_theme(&tokens);
        b.set_kontakt("/nowhere/Other".into());
        assert_eq!(Browser::open(Some(&dir), None).theme(), tokens);
        drop((b, again));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A root a machine scan found is listed as auto with how its scan went;
    /// removing it hides it for good, and adding it back unhides it.
    #[test]
    fn auto_roots_show_their_status_and_removing_one_hides_it() {
        let dir = std::env::temp_dir().join(format!("kontra-browser-auto-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prefs = dir.join(FOLDER);
        std::fs::create_dir_all(&prefs).unwrap();
        let gone = dir.join("unmounted");
        let json = serde_json::json!({ "found": [{ "path": gone }] });
        std::fs::write(prefs.join("prefs.json"), json.to_string()).unwrap();
        let mut b = Browser::open(Some(&dir), None);
        for _ in 0..5000 {
            if !b.poll() && !b.scanning() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let folders = b.folders();
        assert_eq!(folders.len(), 1);
        assert!(folders[0].auto && folders[0].problem);
        assert_eq!(folders[0].summary, "not found");
        b.remove_root(0);
        let again = Browser::open(Some(&dir), None);
        assert!(again.roots().is_empty(), "a hidden auto root stays hidden");
        assert_eq!(again.prefs.hidden, std::slice::from_ref(&gone));
        b.add_root(gone.clone());
        assert!(b.prefs.hidden.is_empty(), "adding a folder unhides it");
        assert!(!b.roots()[0].auto);
        // The browsers' background index writes can still be landing in the
        // temp dir; cleanup is best effort, not what this test checks.
        drop((b, again));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scan that finds what is already listed changes nothing the editor
    /// shows (the plugin's idle-tick test raced the first scan).
    #[test]
    fn a_scan_that_finds_nothing_new_moves_no_generation() {
        let mut b = Browser::open(None, None);
        let before = b.generation;
        for _ in 0..5000 {
            if !b.poll() && !b.scanning() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(!b.scanning(), "the scan finished");
        assert_eq!(b.generation, before);
    }
}
