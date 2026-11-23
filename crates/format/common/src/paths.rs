//! Cross-platform path normalization, Linux case-insensitive resolution, and
//! container-member path splitting shared across format translators.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Converts Windows (`\`) and Kontakt (`|`) path separators to `/`, collapsing
/// duplicate slashes and trimming surrounding whitespace.
#[must_use]
pub fn normalize_separators(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_slash = false;
    for ch in raw.trim().chars() {
        let ch = if ch == '\\' || ch == '|' { '/' } else { ch };
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    out
}

/// Normalizes a virtual archive/container path (`bank.ufs`, `.nkx`, `.nkr`),
/// converting `\` and `|` to `/` and collapsing `.`, `..`, and empty segments.
///
/// Leading `..` components above the container root are dropped rather than
/// escaping outside the container.
#[must_use]
pub fn normalize_virtual_path(path: &str) -> String {
    let normalized = path.replace(['\\', '|'], "/");
    let mut parts: Vec<&str> = Vec::new();
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Resolves `.` and `..` components lexically without touching the filesystem.
#[must_use]
pub fn lexical_clean(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Strips a foreign OS absolute prefix from a normalized `/`-separated path:
/// - Windows drive letters (`C:/Samples/a.wav` -> `Samples/a.wav`)
/// - Windows UNC prefixes (`//server/share/Samples/a.wav` -> `Samples/a.wav`)
/// - macOS volume prefixes (`/Volumes/Drive/Samples/a.wav` -> `Samples/a.wav`)
/// - Classic macOS colon paths (`Macintosh HD:Samples:a.wav` -> `Samples/a.wav`)
/// - Leading `/` on POSIX absolute paths (`/Samples/a.wav` -> `Samples/a.wav`)
///
/// Returns `None` when `normalized` is already a plain relative path.
#[must_use]
pub fn strip_foreign_root(normalized: &str) -> Option<String> {
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
    {
        let rest = normalized
            .get(2..)
            .unwrap_or_default()
            .trim_start_matches('/');
        return Some(rest.to_owned());
    }
    if let Some(rest) = normalized.strip_prefix("//") {
        let mut parts = rest.splitn(3, '/');
        let _server = parts.next();
        let _share = parts.next();
        if let Some(tail) = parts.next() {
            return Some(tail.trim_start_matches('/').to_owned());
        }
    }
    if let Some(rest) = normalized
        .strip_prefix("/Volumes/")
        .or_else(|| normalized.strip_prefix("/volumes/"))
        && let Some((_, tail)) = rest.split_once('/')
    {
        return Some(tail.trim_start_matches('/').to_owned());
    }
    if !normalized.contains('/') && normalized.contains(':') {
        let mut parts = normalized.split(':').filter(|s| !s.is_empty());
        let _volume = parts.next();
        let tail: Vec<&str> = parts.collect();
        if !tail.is_empty() {
            return Some(tail.join("/"));
        }
    }
    normalized
        .strip_prefix('/')
        .map(|rest| rest.trim_start_matches('/').to_owned())
}

/// Per-directory entry cache for repeated case-insensitive lookups on Linux.
///
/// Avoids calling `read_dir` thousands of times when loading large instruments
/// with thousands of sample references sharing a handful of directories.
#[derive(Debug, Default)]
pub struct DirCache {
    dirs: HashMap<PathBuf, Vec<(String, PathBuf)>>,
}

impl DirCache {
    /// Creates an empty directory cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Finds a child of `dir` matching `name` ASCII case-insensitively.
    ///
    /// Tries the exact path first (zero `read_dir` calls when casing matches)
    /// and caches directory entries on the first case miss.
    #[must_use]
    pub fn find_child(&mut self, dir: &Path, name: &str) -> Option<PathBuf> {
        let exact = dir.join(name);
        if exact.exists() {
            return Some(exact);
        }
        let entries = self.dirs.entry(dir.to_path_buf()).or_insert_with(|| {
            let mut list: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| {
                    let path = entry.path();
                    let lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
                    (lower, path)
                })
                .collect();
            list.sort_by(|a, b| a.1.cmp(&b.1));
            list
        });
        let want = name.to_ascii_lowercase();
        entries
            .iter()
            .find(|(lower, _)| *lower == want)
            .map(|(_, path)| path.clone())
    }
}

/// Resolves a relative path `rel` under `base` case-insensitively on Linux/Unix.
///
/// Rejects `..` components so the result stays strictly inside `base`.
#[must_use]
pub fn resolve_case_insensitive(
    base: &Path,
    rel: &str,
    cache: Option<&mut DirCache>,
) -> Option<PathBuf> {
    resolve_case_insensitive_bounded(base, base, rel, cache)
}

/// Resolves `rel` starting from `base` case-insensitively while preventing any
/// `..` traversal from escaping above `boundary`.
#[must_use]
pub fn resolve_case_insensitive_bounded(
    base: &Path,
    boundary: &Path,
    rel: &str,
    mut cache: Option<&mut DirCache>,
) -> Option<PathBuf> {
    let normalized = normalize_separators(rel);
    let trimmed = normalized.trim_start_matches('/');
    if trimmed.is_empty() {
        return base.exists().then(|| base.to_path_buf());
    }

    // Fast path: check lexically joined exact path first.
    let direct = lexical_clean(&base.join(trimmed));
    if direct.starts_with(boundary) && direct.exists() {
        return Some(direct);
    }

    let mut cur = base.to_path_buf();
    for segment in trimmed.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                if cur == boundary || !cur.pop() || !cur.starts_with(boundary) {
                    return None;
                }
            }
            part => {
                cur = match cache.as_deref_mut() {
                    Some(dir_cache) => dir_cache.find_child(&cur, part)?,
                    None => find_child_uncached(&cur, part)?,
                };
            }
        }
    }
    cur.starts_with(boundary).then_some(cur)
}

fn find_child_uncached(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.exists() {
        return Some(exact);
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    entries
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case(name)))
}

/// Splits a virtual container-member path (e.g., `/lib/Samples/Pack.nkx/c4.ncw`
/// or `/lib/Bank.ufs/Presets/Lead.uvip`) into `(container_path, member_path)`
/// when an ancestor has one of `extensions` (ASCII case-insensitive) and either
/// already exists as a file or passes `is_known`.
#[must_use]
pub fn split_container_path(
    path: &Path,
    extensions: &[&str],
    is_known: impl Fn(&Path) -> bool,
) -> Option<(PathBuf, String)> {
    path.ancestors().skip(1).find_map(|parent| {
        let matches_ext = parent.extension().is_some_and(|ext| {
            extensions
                .iter()
                .any(|wanted| ext.eq_ignore_ascii_case(wanted))
        });
        if !matches_ext || !(is_known(parent) || parent.is_file()) {
            return None;
        }
        let member = path
            .strip_prefix(parent)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        Some((parent.to_path_buf(), member))
    })
}

/// Splits a relative container reference like `Samples/Pack.nkx/sub/c4.ncw`
/// into `(container_rel, member_rel)` purely by extension, then resolves the
/// container path case-insensitively on disk under `base` (bounded by `boundary`).
#[must_use]
pub fn resolve_container_path(
    base: &Path,
    boundary: &Path,
    rel: &str,
    extensions: &[&str],
    cache: Option<&mut DirCache>,
) -> Option<(PathBuf, String)> {
    let normalized = normalize_separators(rel);
    let trimmed = normalized.trim_start_matches('/');
    let (container_rel, member) = split_container_reference(trimmed, extensions)?;
    let container = resolve_case_insensitive_bounded(base, boundary, container_rel, cache)?;
    container
        .is_file()
        .then_some((container, normalize_virtual_path(member)))
}

/// Splits a `/`-separated string `a/b/pack.nkx/dir/member.ncw` at the first
/// segment whose extension is in `extensions`.
#[must_use]
pub fn split_container_reference<'a>(
    normalized: &'a str,
    extensions: &[&str],
) -> Option<(&'a str, &'a str)> {
    normalized.match_indices('/').find_map(|(at, _)| {
        let prefix = normalized.get(..at)?;
        let rest = normalized.get(at + 1..)?;
        if rest.is_empty() {
            return None;
        }
        let matches_ext = Path::new(prefix).extension().is_some_and(|ext| {
            extensions
                .iter()
                .any(|wanted| ext.eq_ignore_ascii_case(wanted))
        });
        matches_ext.then_some((prefix, rest))
    })
}

/// Checks whether `candidate` ends with the exact trailing path components of
/// `suffix_parts` (ASCII case-insensitive, respecting `/` component boundaries).
///
/// Unlike raw string `ends_with`, `"close/c4.wav"` matches `"/lib/Close/C4.wav"`
/// and does **not** falsely match `"/lib/NotClose/C4.wav"`.
#[must_use]
pub fn suffix_match_components(candidate: &Path, suffix_parts: &[&str]) -> bool {
    if suffix_parts.is_empty() {
        return false;
    }
    let cand_parts: Vec<String> = candidate
        .components()
        .filter_map(|c| match c {
            Component::Normal(os) => Some(os.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    if cand_parts.len() < suffix_parts.len() {
        return false;
    }
    let start = cand_parts.len() - suffix_parts.len();
    let Some(tail) = cand_parts.get(start..) else {
        return false;
    };
    tail.iter()
        .zip(suffix_parts)
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_separators_and_virtual_paths() {
        assert_eq!(
            normalize_separators(r"  Samples\Strings||Violin\\C4.wav "),
            "Samples/Strings/Violin/C4.wav"
        );
        assert_eq!(
            normalize_virtual_path("Presets/./../Samples\\Strings|C4.flac"),
            "Samples/Strings/C4.flac"
        );
        assert_eq!(normalize_virtual_path("../../a/b"), "a/b");
    }

    #[test]
    fn strips_windows_unc_and_macos_foreign_roots() {
        assert_eq!(
            strip_foreign_root("C:/Libraries/Areia/Samples/C4.wav").as_deref(),
            Some("Libraries/Areia/Samples/C4.wav")
        );
        assert_eq!(
            strip_foreign_root("//NAS/Audio/Samples/C4.wav").as_deref(),
            Some("Samples/C4.wav")
        );
        assert_eq!(
            strip_foreign_root("/Volumes/SSD/Samples/C4.wav").as_deref(),
            Some("Samples/C4.wav")
        );
        assert_eq!(
            strip_foreign_root("Macintosh HD:Samples:C4.wav").as_deref(),
            Some("Samples/C4.wav")
        );
        assert_eq!(strip_foreign_root("Samples/C4.wav"), None);
    }

    #[test]
    fn resolves_case_insensitive_paths_and_containers_on_linux() {
        let root = std::env::temp_dir().join(format!("kontra-paths-ci-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Samples/Strings")).unwrap();
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::write(root.join("Samples/Strings/Violin_C4.WAV"), b"riff").unwrap();
        std::fs::write(root.join("Samples/PACK_01.NKX"), b"nkx").unwrap();

        let mut cache = DirCache::new();
        let found = resolve_case_insensitive_bounded(
            &root.join("Instruments"),
            &root,
            r"..\samples\strings\violin_c4.wav",
            Some(&mut cache),
        );
        assert_eq!(found, Some(root.join("Samples/Strings/Violin_C4.WAV")));

        assert_eq!(
            resolve_case_insensitive_bounded(
                &root.join("Instruments"),
                &root,
                "../../etc/passwd",
                Some(&mut cache),
            ),
            None
        );

        let (archive, member) = resolve_container_path(
            &root.join("Instruments"),
            &root,
            r"..\samples\pack_01.nkx\Violin\C4.ncw",
            &["nkx", "nkr"],
            Some(&mut cache),
        )
        .unwrap();
        assert_eq!(archive, root.join("Samples/PACK_01.NKX"));
        assert_eq!(member, "Violin/C4.ncw");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn suffix_match_respects_component_boundaries() {
        assert!(suffix_match_components(
            Path::new("/lib/Samples/Close/C4.wav"),
            &["close", "c4.wav"]
        ));
        assert!(!suffix_match_components(
            Path::new("/lib/Samples/NotClose/C4.wav"),
            &["close", "c4.wav"]
        ));
    }
}
