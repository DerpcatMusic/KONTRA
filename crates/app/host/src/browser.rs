//! The browser's side on the control thread: the player's library roots,
//! scanned on a thread of their own into lane F's [`Catalog`], and what the
//! player keeps between sessions ([`Prefs`]: roots, stars, recent). A first
//! run starts from v2's roots, read and never written.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use kontra_library::{Cache, Catalog, Prefs, Progress, Root};

/// Where v3 keeps its own settings, beside v2's `kontra`.
const FOLDER: &str = "kontra-v3";

pub struct Browser {
    pub prefs: Prefs,
    pub catalog: Catalog,
    /// Moves whenever the catalog or the prefs change.
    pub generation: u64,
    scan: Option<(Arc<Progress>, mpsc::Receiver<Catalog>)>,
    prefs_path: Option<PathBuf>,
    cache_path: Option<PathBuf>,
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
        let mut browser = Self {
            prefs,
            catalog: Catalog::default(),
            generation: 1,
            scan: None,
            prefs_path,
            cache_path: cache.map(|c| c.join(FOLDER).join("library-index.json")),
        };
        browser.rescan();
        browser
    }

    /// Scans the roots again; a scan still running is stopped.
    pub fn rescan(&mut self) {
        if let Some((progress, _)) = self.scan.take() {
            progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(Progress::default());
        let (send, receive) = mpsc::channel();
        let (roots, cache_path) = (self.prefs.roots.clone(), self.cache_path.clone());
        let scanning = Arc::clone(&progress);
        let spawned = std::thread::Builder::new()
            .name("kontra-scan".into())
            .spawn(move || {
                let mut cache = cache_path.as_deref().map(Cache::load).unwrap_or_default();
                let programs = |b: &Path| kontra_format_uvi::programs(b).map_err(|e| e.to_string());
                if let Some(catalog) =
                    kontra_library::scan(&roots, &scanning, &mut cache, &programs)
                {
                    if let Some(path) = &cache_path {
                        let _saved = path.parent().map(std::fs::create_dir_all);
                        let _saved = cache.save(path);
                    }
                    let _gone = send.send(catalog);
                }
            });
        if spawned.is_ok() {
            self.scan = Some((progress, receive));
        }
    }

    /// Takes a finished scan; true when the catalog changed.
    pub fn poll(&mut self) -> bool {
        let Some(catalog) = self.scan.as_ref().and_then(|(_, r)| r.try_recv().ok()) else {
            return false;
        };
        self.scan = None;
        self.catalog = catalog;
        self.generation += 1;
        true
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

    /// The player loaded `path`: first among the recent.
    pub fn loaded(&mut self, path: &Path) {
        self.prefs.loaded(path);
        self.changed();
    }

    pub fn star(&mut self, library: usize, on: bool) {
        if let Some(dir) = self.catalog.libraries.get(library).map(|l| l.dir.clone()) {
            self.prefs.star(&dir, on);
            self.changed();
        }
    }

    /// Adds a folder the player dropped, and scans again.
    pub fn add_root(&mut self, path: PathBuf) {
        self.prefs.add_root(Root {
            path,
            single: false,
        });
        self.changed();
        self.rescan();
    }

    /// Stops scanning folder `n`, and scans again.
    pub fn remove_root(&mut self, n: usize) {
        if n < self.prefs.roots.len() {
            self.prefs.roots.remove(n);
            self.changed();
            self.rescan();
        }
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

    fn changed(&mut self) {
        self.generation += 1;
        let Some(path) = &self.prefs_path else {
            return;
        };
        let Ok(json) = serde_json::to_vec_pretty(&self.prefs) else {
            return;
        };
        let _saved = path.parent().map(std::fs::create_dir_all);
        // Written whole, then renamed over the old file.
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _saved = std::fs::rename(tmp, path);
        }
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
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
