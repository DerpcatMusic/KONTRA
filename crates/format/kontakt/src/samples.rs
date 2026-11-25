//! Where a preset's sample names live. Lookups
//! never leave the library root: a sample outside it does not resolve.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use kontra_format::io::ReadAt;
use kontra_format::paths::{
    DirCache, lexical_clean, normalize_separators, resolve_case_insensitive_bounded,
    resolve_container_path, split_container_path, split_container_reference, strip_foreign_root,
    suffix_match_components,
};
use kontra_ir as ir;
use ni_file::nkr::Archive;

const ARCHIVE_EXTS: &[&str] = &["nkx", "nkr"];

/// A sample's bytes: a whole file, or a member of a monolith or archive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Location {
    pub file: PathBuf,
    pub offset: u64,
    /// `None`: to the end of `file`.
    pub size: Option<u64>,
    pub keystream: Option<ir::Keystream>,
}

pub(crate) struct Resolver {
    root: PathBuf,
    /// The instrument's folder and each folder above it inside the root.
    bases: Vec<PathBuf>,
    /// Lower-case member path to its bytes, when the instrument is a monolith.
    monolith: HashMap<String, Location>,
    /// Each archive opened once, with its member index.
    archives: HashMap<PathBuf, Option<(File, Archive)>>,
    /// Per-directory listing cache for fast case-insensitive lookups on Linux.
    dir_cache: DirCache,
    /// Lower-case basename to files under `root`, built on the first miss.
    loose: Option<HashMap<String, Vec<PathBuf>>>,
    /// Streams by archive folder; moved archives can belong to another library.
    keys: HashMap<PathBuf, std::sync::Arc<[u8]>>,
    /// Each name resolved once: a large table names the same file many times.
    resolved: HashMap<String, Option<Location>>,
    /// Files checked to be under the root (each canonicalizes it).
    #[cfg(test)]
    inside_checks: std::cell::Cell<usize>,
}

impl Resolver {
    /// `monolith`: the instrument file's own table, when it is a monolith.
    pub(crate) fn new(
        instrument: &Path,
        monolith: Option<ni_file::file_container::NIFileContainer>,
    ) -> Result<Self, String> {
        // Canonical, so an empty or `./` parent, `..` or a symlink still
        // contains its candidates.
        let instrument = &instrument
            .canonicalize()
            .or_else(|_| std::path::absolute(instrument))
            .map_err(|e| e.to_string())?;
        let parent = instrument.parent().unwrap_or(Path::new("/"));
        let root = crate::resources::root(instrument);
        let root = root.canonicalize().unwrap_or(root);
        let bases = (parent.ancestors())
            .take_while(|a| a.starts_with(&root))
            .map(Path::to_path_buf)
            .collect();
        let mut members = HashMap::new();
        if let Some(container) = monolith {
            for item in container.items {
                let Some(offset) = container
                    .file_section_offset
                    .checked_add(item.file_start_offset)
                else {
                    continue;
                };
                if offset.checked_add(item.file_size).is_none() {
                    continue;
                }
                let location = Location {
                    file: instrument.to_path_buf(),
                    offset,
                    size: Some(item.file_size),
                    keystream: None,
                };
                members.insert(
                    item.filename.replace(['|', '\\'], "/").to_lowercase(),
                    location,
                );
            }
        }
        Ok(Self {
            root,
            bases,
            monolith: members,
            archives: HashMap::new(),
            dir_cache: DirCache::new(),
            loose: None,
            keys: HashMap::new(),
            resolved: HashMap::new(),
            #[cfg(test)]
            inside_checks: std::cell::Cell::new(0),
        })
    }

    /// Whether `file`, symlinks followed, is still under the (canonical) root.
    fn inside(&self, file: &Path) -> bool {
        #[cfg(test)]
        self.inside_checks.set(self.inside_checks.get() + 1);
        file.canonicalize().is_ok_and(|f| f.starts_with(&self.root))
    }

    /// An archive already opened was checked when it was.
    fn can_open_archive(&self, archive: &Path) -> bool {
        self.archives.contains_key(archive) || self.inside(archive)
    }

    /// The bytes of the sample the preset names `name`, if the library has it.
    pub(crate) fn resolve(&mut self, name: &str) -> Result<Option<Location>, String> {
        if let Some(found) = self.resolved.get(name) {
            return Ok(found.clone());
        }
        let found = self.find(name)?;
        self.resolved.insert(name.to_owned(), found.clone());
        Ok(found)
    }

    fn find(&mut self, name: &str) -> Result<Option<Location>, String> {
        let normalized = normalize_separators(name);
        if let Some(found) = self.member(&normalized)? {
            return Ok(Some(found));
        }

        // Direct moved-folder remapping on absolute or Windows-authored paths
        // (supports both loose files and moved `.nkx`/`.nkr` archives).
        if let Some(found) = self.try_moved(Path::new(&normalized))? {
            return Ok(Some(found));
        }

        // Candidate relative forms to test under each library base:
        // the normalized path itself (with any leading `/` stripped so `join`
        // stays inside the base) plus a stripped foreign-OS prefix (`C:/...`,
        // `/Volumes/...`, `Macintosh HD:...`) if present.
        let mut rels: Vec<String> = Vec::with_capacity(2);
        let stripped_leading = normalized.trim_start_matches('/');
        if !stripped_leading.is_empty() {
            rels.push(stripped_leading.to_owned());
        }
        if let Some(foreign) = strip_foreign_root(&normalized)
            && !foreign.is_empty()
            && !rels.iter().any(|r| r == &foreign)
        {
            rels.push(foreign);
        }

        // Relative to the instrument folder, then each folder above it in the library.
        for at in 0..self.bases.len() {
            let Some(base) = self.bases.get(at).cloned() else {
                break;
            };
            for rel in &rels {
                // `..` is normal in Kontakt's tables, but never past the root.
                let candidate = lexical_clean(&base.join(rel));
                if !candidate.starts_with(&self.root) {
                    continue;
                }
                if let Some(found) = self.try_moved(&candidate)? {
                    return Ok(Some(found));
                }
                if let Some((archive, member)) = self.resolve_archive(&base, &candidate, rel) {
                    if self.can_open_archive(&archive)
                        && let Some(found) = self.archive_entry(&archive, &member)?
                    {
                        return Ok(Some(found));
                    }
                } else if candidate.is_file() && self.inside(&candidate) {
                    return Ok(Some(whole(candidate)));
                } else if let Some(resolved) = resolve_case_insensitive_bounded(
                    &base,
                    &self.root,
                    rel,
                    Some(&mut self.dir_cache),
                ) && resolved.is_file()
                    && self.inside(&resolved)
                {
                    return Ok(Some(whole(resolved)));
                }
            }
        }

        // A moved or relocated archive: find the archive by name, keep the member exact.
        if let Some((prefix, member)) = split_container_reference(&normalized, ARCHIVE_EXTS)
            && let Some(archive) = self.by_name(prefix)?
            && self.can_open_archive(&archive)
        {
            return self.archive_entry(&archive, member);
        }
        match self.by_name(&normalized)? {
            Some(file) if self.inside(&file) => Ok(Some(whole(file))),
            _ => Ok(None),
        }
    }

    /// Checks whether `candidate` remaps via `kontra_format::moved_with_case`
    /// to an existing loose file or an `.nkx`/`.nkr` archive member, while
    /// ensuring neither `..` traversal nor symlinks can escape the matched
    /// destination root.
    fn try_moved(&mut self, candidate: &Path) -> Result<Option<Location>, String> {
        let Some(cleaned) = clean_no_escape(candidate) else {
            return Ok(None);
        };
        let Some(moved) = kontra_format::moved_with_case(&cleaned, Some(&mut self.dir_cache))
        else {
            return Ok(None);
        };
        if moved == cleaned
            || moved
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Ok(None);
        }
        let Some((moved_root_raw, moved_root)) = moved_destination_root(&cleaned, &moved) else {
            return Ok(None);
        };
        let in_moved_root = |path: &Path| {
            path.canonicalize()
                .is_ok_and(|canon| canon.starts_with(&moved_root))
        };
        if let Some((archive, member)) =
            split_container_path(&moved, ARCHIVE_EXTS, |p| self.archives.contains_key(p))
            && in_moved_root(&archive)
            && let Some(found) = self.archive_entry(&archive, &member)?
        {
            return Ok(Some(found));
        }
        // Handle moved archive whose path differs in case on Linux
        let moved_str = normalize_separators(&moved.to_string_lossy());
        if let Some((prefix, member)) = split_container_reference(&moved_str, ARCHIVE_EXTS) {
            let prefix_path = Path::new(prefix);
            if prefix_path.is_file()
                && in_moved_root(prefix_path)
                && let Some(found) = self.archive_entry(prefix_path, member)?
            {
                return Ok(Some(found));
            }
            if let Some(rel_prefix) = strip_prefix_ascii_case(prefix_path, &moved_root_raw)
                && let Some(rel_str) = rel_prefix.to_str()
                && let Some(found_archive) = resolve_case_insensitive_bounded(
                    &moved_root_raw,
                    &moved_root_raw,
                    rel_str,
                    Some(&mut self.dir_cache),
                )
                && found_archive.is_file()
                && in_moved_root(&found_archive)
                && let Some(found) = self.archive_entry(&found_archive, member)?
            {
                return Ok(Some(found));
            }
        }
        if moved.is_file() && in_moved_root(&moved) {
            return Ok(Some(whole(moved)));
        }
        Ok(None)
    }

    /// Resolves an archive path either directly or case-insensitively on Linux.
    fn resolve_archive(
        &mut self,
        base: &Path,
        candidate: &Path,
        rel: &str,
    ) -> Option<(PathBuf, String)> {
        if let Some(found) =
            split_container_path(candidate, ARCHIVE_EXTS, |p| self.archives.contains_key(p))
        {
            return Some(found);
        }
        resolve_container_path(
            base,
            &self.root,
            rel,
            ARCHIVE_EXTS,
            Some(&mut self.dir_cache),
        )
    }

    /// A monolith member: the exact path, else a unique match of its basename.
    fn member(&self, name: &str) -> Result<Option<Location>, String> {
        if self.monolith.is_empty() {
            return Ok(None);
        }
        let name = name.to_lowercase();
        if let Some(found) = self.monolith.get(&name) {
            return Ok(Some(found.clone()));
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        let suffix = format!("/{base}");
        let mut hits = self
            .monolith
            .iter()
            .filter(|(path, _)| *path == base || path.ends_with(&suffix));
        match (hits.next(), hits.next()) {
            (Some((_, found)), None) => Ok(Some(found.clone())),
            (Some(_), Some(_)) => Err(format!("several monolith members are named {base}")),
            _ => Ok(None),
        }
    }

    /// The archive at `path` if it is open here: its handle duplicated and
    /// its index, so a second reader neither reopens it nor rereads the index.
    pub(crate) fn opened(&self, path: &Path) -> Option<(File, Archive)> {
        let (file, index) = self.archives.get(&path.canonicalize().ok()?)?.as_ref()?;
        Some((file.try_clone().ok()?, index.clone()))
    }

    /// Takes an archive another reader opened, unless one is open here.
    pub(crate) fn adopt(&mut self, path: &Path, file: &File, index: &Archive) {
        let (Ok(path), Ok(file)) = (path.canonicalize(), file.try_clone()) else {
            return;
        };
        if !path.starts_with(&self.root) {
            return;
        }
        self.archives
            .entry(path)
            .or_insert_with(|| Some((file, index.clone())));
    }

    /// The archive's handle and index, opened on first use.
    fn open(&mut self, archive: &Path) -> &Option<(File, Archive)> {
        if !self.archives.contains_key(archive) {
            let index = File::open(archive).ok().and_then(|f| {
                let index = Archive::read_index(&f).ok()?;
                Some((f, index))
            });
            self.archives.insert(archive.to_path_buf(), index);
        }
        self.archives.get(archive).unwrap_or(&None)
    }

    /// [`Resolver::warm`] for the samples the zones of `zones` name.
    pub(crate) fn warm_zones(
        &mut self,
        zones: &ni_file::kontakt::objects::ZoneList,
        table: &HashMap<u32, String>,
    ) {
        let used: std::collections::BTreeSet<u32> = zones
            .zones()
            .iter()
            .filter_map(|zone| {
                let z = zone.params().ok()?;
                z.sample_present
                    .then(|| u32::try_from(z.filename_id).unwrap_or(u32::MAX))
            })
            .collect();
        self.warm(
            used.iter()
                .filter_map(|id| table.get(id).map(String::as_str)),
        );
    }

    /// Reads the member headers of `names` in parallel, so the lookups that
    /// follow find them in the page cache: on a cold disk each is a random
    /// read, and one at a time they dominate the load.
    fn warm<'a>(&mut self, names: impl Iterator<Item = &'a str>) {
        let phase = kontra_format::timing::span("warm: names");
        let mut named: Vec<(PathBuf, String)> = Vec::new();
        // Each archive checked once (whether it may be opened): a library's
        // thousands of samples sit in a few archives, and each check stats
        // and canonicalizes it.
        let mut checked: HashMap<PathBuf, bool> = HashMap::new();
        for name in names {
            let normalized = normalize_separators(name);
            let rel = normalized.trim_start_matches('/');
            let mut found = None;
            for at in 0..self.bases.len() {
                let Some(base) = self.bases.get(at).cloned() else {
                    break;
                };
                let candidate = lexical_clean(&base.join(rel));
                if !candidate.starts_with(&self.root) {
                    continue;
                }
                let known = split_container_path(&candidate, ARCHIVE_EXTS, |p| {
                    checked.contains_key(p) || self.archives.contains_key(p)
                });
                let Some((archive, member)) =
                    known.or_else(|| self.resolve_archive(&base, &candidate, rel))
                else {
                    continue;
                };
                let open = match checked.get(&archive) {
                    Some(&open) => open,
                    None => *checked
                        .entry(archive.clone())
                        .or_insert(self.can_open_archive(&archive)),
                };
                if open {
                    found = Some((archive, member));
                    break;
                }
            }
            named.extend(found);
        }
        drop(phase);
        let phase = kontra_format::timing::span("warm: archive indexes");
        // Open the archives (each reads its directory) side by side.
        let mut missing: Vec<&PathBuf> = named
            .iter()
            .map(|(archive, _)| archive)
            .filter(|a| !self.archives.contains_key(*a))
            .collect();
        missing.sort();
        missing.dedup();
        let opened: Vec<(PathBuf, Option<(File, Archive)>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = missing
                .iter()
                .map(|archive| {
                    scope.spawn(move || {
                        let index = File::open(archive).ok().and_then(|f| {
                            let index = Archive::read_index(&f).ok()?;
                            Some((f, index))
                        });
                        ((*archive).clone(), index)
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        self.archives.extend(opened);
        drop(phase);
        let _phase = kontra_format::timing::span("warm: headers");
        let mut wanted: Vec<(PathBuf, u64)> = (named.iter())
            .filter_map(|(archive, member)| {
                let (_, index) = self.archives.get(archive)?.as_ref()?;
                Some((archive.clone(), index.find(member)?.header_offset))
            })
            .collect();
        wanted.sort();
        let archives = &self.archives;
        let lanes = 16;
        std::thread::scope(|scope| {
            for lane in 0..lanes {
                let wanted = &wanted;
                scope.spawn(move || {
                    let mut header = [0u8; 31];
                    for (archive, offset) in wanted.iter().skip(lane).step_by(lanes) {
                        if let Some(Some((file, _))) = archives.get(archive) {
                            // Only the page cache matters; a short or failed read is fine.
                            let _ = file.read_at(*offset, &mut header);
                        }
                    }
                });
            }
        });
    }

    fn archive_entry(&mut self, archive: &Path, member: &str) -> Result<Option<Location>, String> {
        let index = self.open(archive);
        let Some((file, index)) = index else {
            return Ok(None);
        };
        let Some(entry) = index.member(file, member).map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        if !entry.valid {
            return Err(format!("damaged archive member {member}"));
        }
        let keystream = match (entry.encoded, entry.key_index) {
            (false, _) | (true, 0xff) => None,
            (true, 0x100) => {
                let stream = match self
                    .keys
                    .entry(archive.parent().unwrap_or(archive).to_path_buf())
                {
                    std::collections::hash_map::Entry::Occupied(key) => key.get().clone(),
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        let key = crate::access::keystream(archive)
                            .map_err(|e| format!("protected archive (Native Access): {e}"))?;
                        slot.insert(key).clone()
                    }
                };
                Some(crate::access::repeat_keystream(stream, entry.offset))
            }
            (true, other) => {
                return Err(format!(
                    "protected archive (Native Access): unsupported archive cipher {other:#x}"
                ));
            }
        };
        Ok(Some(Location {
            file: archive.to_path_buf(),
            offset: entry.offset,
            size: Some(entry.size),
            keystream,
        }))
    }

    /// A file under the root with this name: the only one, else the one whose
    /// folders match the most trailing components of `name` (respecting
    /// path-component boundaries).
    fn by_name(&mut self, name: &str) -> Result<Option<PathBuf>, String> {
        let base = name.rsplit('/').next().unwrap_or(name).to_ascii_lowercase();
        let root = &self.root;
        let loose = self.loose.get_or_insert_with(|| {
            let mut index = HashMap::<String, Vec<PathBuf>>::new();
            walk(root, &mut |file| {
                let key = file
                    .file_name()
                    .map(|n| n.to_string_lossy().to_ascii_lowercase());
                index.entry(key.unwrap_or_default()).or_default().push(file);
            });
            index
        });
        let Some(matches) = loose.get(&base) else {
            return Ok(None);
        };
        if let [only] = matches.as_slice() {
            return Ok(Some(only.clone()));
        }
        let parts: Vec<_> = name
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".." && *s != ".")
            .collect();
        for n in (2..=parts.len()).rev() {
            let Some(suffix) = parts.get(parts.len() - n..) else {
                continue;
            };
            let mut found = matches
                .iter()
                .filter(|p| suffix_match_components(p, suffix));
            if let (Some(only), None) = (found.next(), found.next()) {
                return Ok(Some(only.clone()));
            }
        }
        Err(format!("{} files are named {base}", matches.len()))
    }
}

fn whole(file: PathBuf) -> Location {
    Location {
        file,
        offset: 0,
        size: None,
        keystream: None,
    }
}

/// Cleans `.` and `..` components without allowing `..` to escape past the
/// root, Windows drive prefix, or relative start.
fn clean_no_escape(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::new();
    let mut depth: usize = 0;
    let mut first_normal = true;
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir => {
                out.push(part);
                first_normal = false;
            }
            Component::Normal(seg) => {
                // Treat a leading Windows drive component (`C:`) on Unix as an
                // anchor that `..` must never pop past.
                let is_drive = first_normal
                    && seg.to_str().is_some_and(
                        |s| matches!(s.as_bytes(), [c, b':'] if c.is_ascii_alphabetic()),
                    );
                first_normal = false;
                out.push(seg);
                if !is_drive {
                    depth += 1;
                }
            }
            Component::ParentDir => {
                if depth == 0 {
                    return None;
                }
                out.pop();
                depth -= 1;
            }
        }
    }
    Some(out)
}

/// Discovers the destination root `(raw, canonical)` of the moved-folder rule
/// that remapped `cleaned` to `moved`.
fn moved_destination_root(cleaned: &Path, moved: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut root = None;
    for ancestor in cleaned.ancestors() {
        let Some(mapped) = kontra_format::moved_with_case(ancestor, None) else {
            break;
        };
        if strip_prefix_ascii_case(moved, &mapped).is_some() {
            root = Some(mapped);
        } else {
            break;
        }
    }
    let raw = root?;
    let canon = raw.canonicalize().ok()?;
    Some((raw, canon))
}

fn strip_prefix_ascii_case(path: &Path, prefix: &Path) -> Option<PathBuf> {
    let mut parts = path.components();
    for expected in prefix.components() {
        if !parts
            .next()?
            .as_os_str()
            .eq_ignore_ascii_case(expected.as_os_str())
        {
            return None;
        }
    }
    Some(parts.collect())
}

fn walk(dir: &Path, found: &mut impl FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => walk(&entry.path(), found),
            Ok(kind) if kind.is_file() => found(entry.path()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A library's samples sit in a few archives: warming thousands of
    /// names checks each archive once, not once per name.
    #[test]
    fn warming_many_names_checks_their_archive_once() {
        let dir = std::env::temp_dir().join(format!("kontra-warm-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Samples")).unwrap();
        std::fs::write(dir.join("Samples/pack.nkx"), b"").unwrap();
        let instrument = dir.join("x.nki");
        std::fs::write(&instrument, b"").unwrap();
        let mut resolver = Resolver::new(&instrument, None).unwrap();
        let names: Vec<String> = (0..1000)
            .map(|n| format!("Samples/pack.nkx/{n}.ncw"))
            .collect();
        resolver.warm(names.iter().map(String::as_str));
        assert_eq!(resolver.inside_checks.get(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn resolves_case_insensitive_windows_and_foreign_paths_on_linux() {
        let root =
            std::env::temp_dir().join(format!("kontra-samples-linux-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::create_dir_all(root.join("Samples/Close")).unwrap();
        std::fs::create_dir_all(root.join("Samples/NotClose")).unwrap();
        let inst = root.join("Instruments/Patch.nki");
        std::fs::write(&inst, b"").unwrap();
        let close_wav = root.join("Samples/Close/C4.WAV");
        let not_close_wav = root.join("Samples/NotClose/C4.WAV");
        std::fs::write(&close_wav, b"riff").unwrap();
        std::fs::write(&not_close_wav, b"riff").unwrap();

        let mut resolver = Resolver::new(&inst, None).unwrap();

        // Case-insensitive relative Windows path with backslashes
        let found = resolver
            .resolve(r"..\samples\close\c4.wav")
            .unwrap()
            .unwrap();
        assert_eq!(found.file, close_wav.canonicalize().unwrap());

        // Windows drive-letter absolute path falling back to component-boundary suffix
        let found = resolver
            .resolve(r"D:\OldDrive\MyLib\Samples\Close\c4.wav")
            .unwrap()
            .unwrap();
        assert_eq!(found.file, close_wav.canonicalize().unwrap());

        // Component boundary check: `Close/c4.wav` must NOT falsely match `NotClose/C4.WAV`
        let found = resolver.resolve("Close/c4.wav").unwrap().unwrap();
        assert_eq!(found.file, close_wav.canonicalize().unwrap());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resolves_samples_in_non_standard_samples_folder_name() {
        let root =
            std::env::temp_dir().join(format!("kontra-samples-altroot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::create_dir_all(root.join("Morphology Samples/Drones")).unwrap();
        let inst = root.join("Instruments/Patch.nki");
        std::fs::write(&inst, b"").unwrap();
        let wav = root.join("Morphology Samples/Drones/Pad.WAV");
        std::fs::write(&wav, b"riff").unwrap();

        let mut resolver = Resolver::new(&inst, None).unwrap();
        let found = resolver
            .resolve(r"..\morphology samples\drones\pad.wav")
            .unwrap()
            .unwrap();
        assert_eq!(found.file, wav.canonicalize().unwrap());

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn try_moved_blocks_path_traversal_and_symlink_escapes() {
        let base =
            std::env::temp_dir().join(format!("kontra-samples-traversal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let lib = base.join("Lib");
        let moved_dest = base.join("MovedDest");
        let outside = base.join("Outside");
        let other_moved = base.join("OtherMoved");
        std::fs::create_dir_all(lib.join("Instruments")).unwrap();
        std::fs::create_dir_all(lib.join("Samples")).unwrap();
        std::fs::create_dir_all(moved_dest.join("Samples")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&other_moved).unwrap();

        let inst = lib.join("Instruments/Patch.nki");
        std::fs::write(&inst, b"").unwrap();
        let legit = moved_dest.join("Samples/C4.WAV");
        std::fs::write(&legit, b"riff").unwrap();
        let secret = outside.join("secret.wav");
        std::fs::write(&secret, b"secret").unwrap();
        let other_secret = other_moved.join("other.wav");
        std::fs::write(&other_secret, b"other").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&secret, moved_dest.join("Samples/escape.wav")).unwrap();
        }

        let old_prefix = base.join("OldLocation/MyLib");
        let other_old = base.join("OtherLib");
        kontra_format::set_moved(&[
            (old_prefix.clone(), moved_dest.clone()),
            (PathBuf::from("C:/OldLib"), moved_dest.clone()),
            (other_old, other_moved),
        ]);

        let mut resolver = Resolver::new(&inst, None).unwrap();

        // An archive member is not a filesystem path, so only its ancestor is case-corrected.
        let archive = moved_dest.join("Samples/Library.NKX");
        std::fs::write(&archive, b"not an archive").unwrap();
        let requested = old_prefix.join("samples/library.nkx/member.wav");
        let mapped = kontra_format::moved_with_case(&requested, None).unwrap();
        let (raw_root, canonical_root) = moved_destination_root(&requested, &mapped).unwrap();
        assert_eq!(raw_root, moved_dest);
        assert_eq!(canonical_root, moved_dest.canonicalize().unwrap());
        let archive_prefix = mapped.parent().unwrap();
        let relative = strip_prefix_ascii_case(archive_prefix, &raw_root).unwrap();
        let resolved = resolve_case_insensitive_bounded(
            &raw_root,
            &raw_root,
            relative.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(resolved, archive);

        // Legitimate moved sample resolves cleanly (including case-insensitive lookup).
        let ok = resolver
            .resolve(&format!("{}/samples/c4.wav", old_prefix.display()))
            .unwrap();
        assert_eq!(ok.map(|l| l.file), Some(legit.clone()));

        // Legitimate internal `..` that stays within the moved prefix resolves cleanly.
        let ok_internal = resolver
            .resolve(&format!(
                "{}/Instruments/../Samples/C4.WAV",
                old_prefix.display()
            ))
            .unwrap();
        assert_eq!(ok_internal.map(|l| l.file), Some(legit));

        // Traversal via `..` escaping the moved destination root is rejected.
        let escaped_posix = resolver
            .resolve(&format!(
                "{}/../../Outside/secret.wav",
                old_prefix.display()
            ))
            .unwrap();
        assert_eq!(escaped_posix, None);

        // Windows-style drive prefix traversal escaping the moved root is rejected.
        let escaped_win = resolver
            .resolve(r"C:\OldLib\..\..\Outside\secret.wav")
            .unwrap();
        assert_eq!(escaped_win, None);

        // Relative `..` escaping the library root cannot trigger another library's moved rule.
        let escaped_rel = resolver.resolve("../../OtherLib/other.wav").unwrap();
        assert_eq!(escaped_rel, None);

        // Symlink inside moved destination pointing outside the moved root is rejected.
        #[cfg(unix)]
        {
            let escaped_symlink = resolver
                .resolve(&format!("{}/Samples/escape.wav", old_prefix.display()))
                .unwrap();
            assert_eq!(escaped_symlink, None);
        }

        kontra_format::set_moved(&[]);
        std::fs::remove_dir_all(&base).unwrap();
    }
}
