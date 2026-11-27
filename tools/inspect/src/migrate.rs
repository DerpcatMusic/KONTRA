//! Automated Kontakt host session chunk (`hsin`) -> KONTRA v3 OAST state
//! conversion (`kontra-inspect migrate-chunk` / `migrate-batch`).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use kontra_format_kontakt::session::{self, Keyring, KontaktSession, SessionSlot, SessionStatus};
use kontra_host::state::{Saved, SavedId, SavedPart};
use kontra_library::{Catalog, Prefs, index};
use serde::{Deserialize, Serialize};

const KONTRA_PLUGIN_ID: &str = "audio.matari.kontra-v3";

#[derive(Deserialize)]
struct BatchJob {
    id: String,
    chunk: PathBuf,
    out_state: Option<PathBuf>,
}

#[derive(Serialize)]
struct SlotReport {
    slot: u16,
    name: String,
    matched_path: Option<PathBuf>,
    match_reason: String,
    channel: u8,
    output: u8,
    gain: f32,
    pan: f32,
    script_slots: usize,
    persistent_vars: usize,
    sample_hints: Vec<String>,
}

#[derive(Serialize)]
struct ChunkReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    chunk: PathBuf,
    status: &'static str,
    encrypted: bool,
    snpids: Vec<String>,
    matched_library: Option<String>,
    matched_library_dir: Option<PathBuf>,
    multi_script_count: usize,
    slots: Vec<SlotReport>,
    slot_errors: Vec<String>,
    unresolved_slots: usize,
    state_bytes: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    out_state: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

struct LibraryMatcher {
    keyring: Keyring,
    /// `(library_id, library_dir, online)` for `SavedId` generation.
    identities: Vec<(String, PathBuf, bool)>,
    /// All indexed `.nki` files grouped by their parent library directory.
    by_library: HashMap<PathBuf, Vec<NkiEntry>>,
    /// All indexed `.nki` files across every library.
    all_nkis: Vec<NkiEntry>,
    /// Cached base program `(gain, pan)` per `.nki`.
    base_mix_cache: HashMap<PathBuf, Option<(f32, f32)>>,
}

#[derive(Clone)]
struct NkiEntry {
    path: PathBuf,
    stem_lower: String,
    stem_norm: String,
    /// Normalized name with standalone patch-index numbers after ` - ` stripped
    /// (handles library version renumbering like Areia 1.0 -> 1.2).
    stem_unnumbered: String,
}

impl LibraryMatcher {
    fn open(extra_roots: &[PathBuf], index_override: Option<&Path>) -> Self {
        let config_dir = config_dir();
        let prefs_path = config_dir
            .as_deref()
            .map(|c| c.join("kontra-v3").join("prefs.json"));
        let prefs: Prefs = prefs_path
            .as_deref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        let index_path = index_override.map(Path::to_path_buf).or_else(|| {
            config_dir
                .as_deref()
                .map(|c| c.join("kontra-v3").join("library-index.json"))
        });
        let libraries = index_path.as_deref().map(index::load).unwrap_or_default();
        let catalog = Catalog {
            libraries,
            ..Catalog::default()
        };

        let mut roots: Vec<PathBuf> = extra_roots.to_vec();
        for r in &prefs.roots {
            if !roots.contains(&r.path) {
                roots.push(r.path.clone());
            }
        }
        for r in &prefs.found {
            if !roots.contains(&r.path) {
                roots.push(r.path.clone());
            }
        }
        for lib in &catalog.libraries {
            if lib.offline.is_none() && lib.dir.is_dir() && !roots.contains(&lib.dir) {
                roots.push(lib.dir.clone());
            }
        }

        let keyring = Keyring::scan(&roots);

        let mut identities = Vec::new();
        let mut by_library: HashMap<PathBuf, Vec<NkiEntry>> = HashMap::new();
        let mut all_nkis = Vec::new();
        let mut seen_paths: BTreeSet<PathBuf> = BTreeSet::new();

        for lib in &catalog.libraries {
            identities.push((lib.id.clone(), lib.dir.clone(), lib.offline.is_none()));
            if lib.offline.is_some() {
                continue;
            }
            for preset in &lib.presets {
                if !is_nki(preset) || !seen_paths.insert(preset.clone()) {
                    continue;
                }
                if let Some(entry) = make_entry(preset.clone()) {
                    by_library
                        .entry(lib.dir.clone())
                        .or_default()
                        .push(entry.clone());
                    all_nkis.push(entry);
                }
            }
        }

        // Also scan extra roots directly for `.nki` files not yet in library-index.json.
        for root in extra_roots {
            scan_extra_nkis(
                root,
                root,
                0,
                &mut seen_paths,
                &mut by_library,
                &mut all_nkis,
            );
        }

        Self {
            keyring,
            identities,
            by_library,
            all_nkis,
            base_mix_cache: HashMap::new(),
        }
    }

    fn base_mix(&mut self, path: &Path) -> Option<(f32, f32)> {
        if let Some(&cached) = self.base_mix_cache.get(path) {
            return cached;
        }
        let mix = session::base_program_mix(path);
        self.base_mix_cache.insert(path.to_path_buf(), mix);
        mix
    }

    fn resolve_slot(
        &self,
        slot: &SessionSlot,
        matched_dir: Option<&Path>,
    ) -> Option<(PathBuf, String)> {
        let target_lower = slot.name.trim().to_ascii_lowercase();
        let target_norm = normalize_name(&slot.name);
        let target_unnum = normalize_unnumbered(&slot.name);
        if target_lower.is_empty() {
            return None;
        }

        // 1. Inside the SNPID-matched library directory (highest confidence).
        if let Some(dir) = matched_dir {
            if let Some(entries) = self.by_library.get(dir)
                && let Some((path, why)) =
                    match_in_list(entries, &target_lower, &target_norm, &target_unnum, slot)
            {
                return Some((path, format!("snpid-library ({why})")));
            }
            // Fallback: scan directory directly in case index didn't include this library.
            let mut local = Vec::new();
            let mut seen = BTreeSet::new();
            let mut dummy = HashMap::new();
            scan_extra_nkis(dir, dir, 0, &mut seen, &mut dummy, &mut local);
            if let Some((path, why)) =
                match_in_list(&local, &target_lower, &target_norm, &target_unnum, slot)
            {
                return Some((path, format!("snpid-dir-scan ({why})")));
            }
        }

        // 2. Match library directory via sample/resource path hints (great for unencrypted libs).
        let hint_dirs = self.hint_matched_libraries(slot);
        for dir in &hint_dirs {
            if let Some(entries) = self.by_library.get(dir)
                && let Some((path, why)) =
                    match_in_list(entries, &target_lower, &target_norm, &target_unnum, slot)
            {
                return Some((path, format!("path-hint-library ({why})")));
            }
        }

        // 3. Global unique exact stem match across all installed libraries.
        let exact_global: Vec<&NkiEntry> = self
            .all_nkis
            .iter()
            .filter(|e| e.stem_lower == target_lower)
            .collect();
        if exact_global.len() == 1
            && let Some(only) = exact_global.first()
        {
            return Some((only.path.clone(), "global-unique-exact".to_owned()));
        }
        if exact_global.len() > 1
            && let Some(best) = pick_by_hints(&exact_global, slot)
        {
            return Some((best.path.clone(), "global-exact-hint".to_owned()));
        }

        // 4. Global unique normalized match across all installed libraries.
        if !target_norm.is_empty() {
            let norm_global: Vec<&NkiEntry> = self
                .all_nkis
                .iter()
                .filter(|e| e.stem_norm == target_norm)
                .collect();
            if norm_global.len() == 1
                && let Some(only) = norm_global.first()
            {
                return Some((only.path.clone(), "global-unique-normalized".to_owned()));
            }
            if norm_global.len() > 1
                && let Some(best) = pick_by_hints(&norm_global, slot)
            {
                return Some((best.path.clone(), "global-normalized-hint".to_owned()));
            }
        }

        // 5. Global unique renumbered-patch match (e.g. Areia 1.0 -> 1.2 inside a mixed multi).
        if !target_unnum.is_empty() && target_unnum != target_norm {
            let unnum_global: Vec<&NkiEntry> = self
                .all_nkis
                .iter()
                .filter(|e| e.stem_unnumbered == target_unnum)
                .collect();
            if unnum_global.len() == 1
                && let Some(only) = unnum_global.first()
            {
                return Some((only.path.clone(), "global-unique-unnumbered".to_owned()));
            }
        }

        None
    }

    fn hint_matched_libraries(&self, slot: &SessionSlot) -> Vec<PathBuf> {
        let mut hint_tokens: Vec<String> = Vec::new();
        for raw in slot
            .resource_container
            .iter()
            .chain(slot.sample_hints.iter())
        {
            for seg in raw.split(['/', '\\']) {
                let low = seg.trim().to_ascii_lowercase();
                if low.len() >= 3
                    && !matches!(
                        low.as_str(),
                        "samples" | "instruments" | "kontakt" | "libraries" | "resources" | "data"
                    )
                {
                    let stem = low
                        .strip_suffix(".nkr")
                        .or_else(|| low.strip_suffix(".nkx"))
                        .unwrap_or(&low);
                    if !hint_tokens.iter().any(|t| t == stem) {
                        hint_tokens.push(stem.to_owned());
                    }
                }
            }
        }
        let mut scored: Vec<(usize, PathBuf)> = Vec::new();
        for dir in self.by_library.keys() {
            let folder = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let folder_norm = normalize_name(&folder);
            let mut score = 0usize;
            for tok in &hint_tokens {
                let tok_norm = normalize_name(tok);
                if folder == *tok || (!folder_norm.is_empty() && folder_norm == tok_norm) {
                    score += 10;
                } else if !tok_norm.is_empty()
                    && tok_norm.len() >= 4
                    && (folder_norm.contains(&tok_norm) || tok_norm.contains(&folder_norm))
                {
                    score += 4;
                }
            }
            if score > 0 {
                scored.push((score, dir.clone()));
            }
        }
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.into_iter().map(|(_, d)| d).collect()
    }

    fn identify(&self, path: &Path) -> Option<SavedId> {
        let (id, dir, _) = self
            .identities
            .iter()
            .filter(|(id, dir, _)| !id.is_empty() && path.starts_with(dir))
            .max_by_key(|(_, dir, _)| dir.as_os_str().len())?;
        Some(SavedId {
            path: path.to_path_buf(),
            library: id.clone(),
            relative: path.strip_prefix(dir).ok()?.to_path_buf(),
        })
    }

    fn convert_chunk(
        &mut self,
        id: Option<String>,
        chunk_path: &Path,
        out_state: Option<&Path>,
    ) -> ChunkReport {
        let bytes = match std::fs::read(chunk_path) {
            Ok(b) => b,
            Err(e) => return error_report(id, chunk_path, format!("reading chunk: {e}")),
        };
        let decoded = match session::decode_state(&bytes, &self.keyring) {
            Ok(s) => s,
            Err(e) => return error_report(id, chunk_path, e),
        };
        self.build_report(id, chunk_path, out_state, decoded)
    }

    fn build_report(
        &mut self,
        id: Option<String>,
        chunk_path: &Path,
        out_state: Option<&Path>,
        decoded: KontaktSession,
    ) -> ChunkReport {
        let matched_dir = decoded.matched_library.as_ref().map(|m| m.dir.clone());
        let matched_name = decoded
            .matched_library
            .as_ref()
            .and_then(|m| m.name.clone());

        if decoded.status == SessionStatus::MissingLibraryKey {
            return ChunkReport {
                id,
                chunk: chunk_path.to_path_buf(),
                status: "missing_library_key",
                encrypted: decoded.encrypted,
                snpids: decoded.snpids,
                matched_library: matched_name,
                matched_library_dir: matched_dir,
                multi_script_count: 0,
                slots: Vec::new(),
                slot_errors: decoded.slot_errors,
                unresolved_slots: 0,
                state_bytes: 0,
                out_state: None,
                error: None,
            };
        }

        if decoded.status == SessionStatus::Empty {
            let saved = Saved::new(Vec::new());
            let blob = serialize_kontra_oast(&saved);
            let written = write_state_file(out_state, &blob);
            return match written {
                Ok(written_path) => ChunkReport {
                    id,
                    chunk: chunk_path.to_path_buf(),
                    status: "empty",
                    encrypted: decoded.encrypted,
                    snpids: decoded.snpids,
                    matched_library: matched_name,
                    matched_library_dir: matched_dir,
                    multi_script_count: decoded.multi_script_count,
                    slots: Vec::new(),
                    slot_errors: decoded.slot_errors,
                    unresolved_slots: 0,
                    state_bytes: blob.len(),
                    out_state: written_path,
                    error: None,
                },
                Err(e) => error_report(id, chunk_path, e),
            };
        }

        let mut parts = Vec::new();
        let mut slot_reports = Vec::new();
        let mut unresolved = 0usize;
        let mut saved_ids = Vec::new();

        for (dense_idx, slot) in decoded.slots.iter().enumerate() {
            let channel = u8::try_from(slot.channel)
                .ok()
                .filter(|&c| c <= 64)
                .unwrap_or(0);
            let output = u8::try_from(usize::from(slot.output).min(15)).unwrap_or(0);
            let part_idx = usize::from(slot.index);
            let part_idx = if part_idx < 16 && !parts.iter().any(|p: &SavedPart| p.part == part_idx)
            {
                part_idx
            } else {
                dense_idx
            };

            let resolved = self.resolve_slot(slot, matched_dir.as_deref());
            let (gain, pan) = if let Some((ref path, _)) = resolved {
                compute_relative_mix(slot, self.base_mix(path))
            } else {
                (1.0, 0.0)
            };

            let persistent_vars: usize = slot.scripts.iter().map(Vec::len).sum();
            match resolved {
                Some((path, reason)) => {
                    if let Some(sid) = self.identify(&path) {
                        saved_ids.push(sid);
                    }
                    if part_idx < 16 {
                        parts.push(SavedPart {
                            part: part_idx,
                            path: path.clone(),
                            channel,
                            output,
                            gain,
                            pan,
                            solo: false,
                            mute: false,
                            scripts: slot.scripts.clone(),
                            delays: None,
                            slot_types: Vec::new(),
                            slot_responses: Vec::new(),
                            preset: None,
                            modified: false,
                        });
                    }
                    slot_reports.push(SlotReport {
                        slot: slot.index,
                        name: slot.name.clone(),
                        matched_path: Some(path),
                        match_reason: reason,
                        channel,
                        output,
                        gain,
                        pan,
                        script_slots: slot.scripts.len(),
                        persistent_vars,
                        sample_hints: slot.sample_hints.iter().take(3).cloned().collect(),
                    });
                }
                None => {
                    unresolved += 1;
                    slot_reports.push(SlotReport {
                        slot: slot.index,
                        name: slot.name.clone(),
                        matched_path: None,
                        match_reason: "unresolved".to_owned(),
                        channel,
                        output,
                        gain,
                        pan,
                        script_slots: slot.scripts.len(),
                        persistent_vars,
                        sample_hints: slot.sample_hints.iter().take(3).cloned().collect(),
                    });
                }
            }
        }

        let status = if unresolved == 0 && !parts.is_empty() {
            "ok"
        } else if !parts.is_empty() {
            "partial"
        } else {
            "unresolved_instrument"
        };

        let (state_bytes, written_path) = if !parts.is_empty() {
            let saved = Saved {
                ids: saved_ids,
                ..Saved::new(parts)
            };
            let json = saved.write();
            if let Err(err) = Saved::parse(&json) {
                return error_report(
                    id,
                    chunk_path,
                    format!("generated invalid KONTRA state: {err}"),
                );
            }
            let blob = serialize_kontra_oast(&saved);
            match write_state_file(out_state, &blob) {
                Ok(p) => (blob.len(), p),
                Err(e) => return error_report(id, chunk_path, e),
            }
        } else {
            (0, None)
        };

        ChunkReport {
            id,
            chunk: chunk_path.to_path_buf(),
            status,
            encrypted: decoded.encrypted,
            snpids: decoded.snpids,
            matched_library: matched_name,
            matched_library_dir: matched_dir,
            multi_script_count: decoded.multi_script_count,
            slots: slot_reports,
            slot_errors: decoded.slot_errors,
            unresolved_slots: unresolved,
            state_bytes,
            out_state: written_path,
            error: None,
        }
    }
}

fn error_report(id: Option<String>, chunk_path: &Path, message: String) -> ChunkReport {
    ChunkReport {
        id,
        chunk: chunk_path.to_path_buf(),
        status: "error",
        encrypted: false,
        snpids: Vec::new(),
        matched_library: None,
        matched_library_dir: None,
        multi_script_count: 0,
        slots: Vec::new(),
        slot_errors: Vec::new(),
        unresolved_slots: 0,
        state_bytes: 0,
        out_state: None,
        error: Some(message),
    }
}

fn write_state_file(out_state: Option<&Path>, blob: &[u8]) -> Result<Option<PathBuf>, String> {
    let Some(path) = out_state else {
        return Ok(None);
    };
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(path, blob).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(Some(path.to_path_buf()))
}

fn compute_relative_mix(slot: &SessionSlot, base: Option<(f32, f32)>) -> (f32, f32) {
    let Some((base_gain, base_pan)) = base else {
        return (1.0, 0.0);
    };
    let gain = if base_gain > 1e-6 && slot.volume_gain.is_finite() {
        let ratio = slot.volume_gain / base_gain;
        if (ratio - 1.0).abs() < 0.005 {
            1.0
        } else {
            ratio.clamp(0.0, 16.0)
        }
    } else {
        1.0
    };
    let pan = if slot.pan.is_finite() && base_pan.is_finite() {
        let delta = slot.pan - base_pan;
        if delta.abs() < 0.005 {
            0.0
        } else {
            delta.clamp(-1.0, 1.0)
        }
    } else {
        0.0
    };
    (gain, pan)
}

fn match_in_list(
    entries: &[NkiEntry],
    target_lower: &str,
    target_norm: &str,
    target_unnum: &str,
    slot: &SessionSlot,
) -> Option<(PathBuf, &'static str)> {
    let exact: Vec<&NkiEntry> = entries
        .iter()
        .filter(|e| e.stem_lower == target_lower)
        .collect();
    if exact.len() == 1
        && let Some(only) = exact.first()
    {
        return Some((only.path.clone(), "exact"));
    }
    if exact.len() > 1 {
        let best = pick_by_hints(&exact, slot).or_else(|| exact.first().copied())?;
        return Some((best.path.clone(), "exact-hint"));
    }
    if !target_norm.is_empty() {
        let norm: Vec<&NkiEntry> = entries
            .iter()
            .filter(|e| e.stem_norm == target_norm)
            .collect();
        if norm.len() == 1
            && let Some(only) = norm.first()
        {
            return Some((only.path.clone(), "normalized"));
        }
        if norm.len() > 1 {
            let best = pick_by_hints(&norm, slot).or_else(|| norm.first().copied())?;
            return Some((best.path.clone(), "normalized-hint"));
        }
    }
    if !target_unnum.is_empty() {
        let unnum: Vec<&NkiEntry> = entries
            .iter()
            .filter(|e| e.stem_unnumbered == target_unnum)
            .collect();
        if unnum.len() == 1
            && let Some(only) = unnum.first()
        {
            return Some((only.path.clone(), "renumbered"));
        }
        if unnum.len() > 1 {
            let best = pick_by_hints(&unnum, slot).or_else(|| unnum.first().copied())?;
            return Some((best.path.clone(), "renumbered-hint"));
        }
    }
    if !target_norm.is_empty() {
        // Suffix/substring match when unique inside one library (e.g. "01 Areia - 16 Violins" vs "16 Violins").
        let contained: Vec<&NkiEntry> = entries
            .iter()
            .filter(|e| {
                e.stem_norm.ends_with(target_norm)
                    || (target_norm.len() >= 6 && e.stem_norm.contains(target_norm))
            })
            .collect();
        if contained.len() == 1
            && let Some(only) = contained.first()
        {
            return Some((only.path.clone(), "substring"));
        }
    }
    None
}

fn pick_by_hints<'a>(candidates: &[&'a NkiEntry], slot: &SessionSlot) -> Option<&'a NkiEntry> {
    let hint_blob: String = slot
        .resource_container
        .iter()
        .chain(slot.sample_hints.iter())
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let mut best: Option<(usize, &'a NkiEntry)> = None;
    for &entry in candidates {
        let mut score = 0usize;
        for comp in entry.path.components() {
            let s = comp.as_os_str().to_string_lossy().to_ascii_lowercase();
            if s.len() >= 4 && hint_blob.contains(&s) {
                score += s.len();
            }
        }
        if best.is_none_or(|(b, _)| score > b) {
            best = Some((score, entry));
        }
    }
    best.filter(|(s, _)| *s > 0).map(|(_, e)| e)
}

fn is_nki(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
}

fn make_entry(path: PathBuf) -> Option<NkiEntry> {
    let stem = path.file_stem()?.to_string_lossy();
    let stem_lower = stem.trim().to_ascii_lowercase();
    let stem_norm = normalize_name(&stem);
    let stem_unnumbered = normalize_unnumbered(&stem);
    Some(NkiEntry {
        path,
        stem_lower,
        stem_norm,
        stem_unnumbered,
    })
}

/// Normalizes a patch name while also removing a leading 2-digit patch index
/// on each `" - "` segment (e.g. `"Areia - 08 Full Ens - Spiccato Fast"` ->
/// `"areiafullensspiccatofast"`), without touching trailing voice numbers like
/// `"Horn 1"` vs `"Horn 4"`.
fn normalize_unnumbered(name: &str) -> String {
    let mut out = String::new();
    for segment in name.split(" - ") {
        let trimmed = segment.trim();
        let rest = strip_leading_patch_index(trimmed);
        for c in rest.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
            }
        }
    }
    out
}

fn strip_leading_patch_index(s: &str) -> &str {
    let bytes = s.as_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if (1..=3).contains(&digits)
        && let Some(after) = s.get(digits..)
        && after.starts_with([' ', '.', '-', '_'])
    {
        let cleaned = after.trim_start_matches([' ', '.', '-', '_']);
        if cleaned.len() >= 3 {
            return cleaned;
        }
    }
    s
}

fn normalize_name(name: &str) -> String {
    // Strip common leading track/patch numbering like "01. ", "01 - ", "01 ".
    let trimmed = name.trim();
    let stripped = trimmed
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['.', '-', '_', ' ']);
    let source = if stripped.len() >= 3 {
        stripped
    } else {
        trimmed
    };
    source
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn scan_extra_nkis(
    root: &Path,
    lib_dir: &Path,
    depth: usize,
    seen: &mut BTreeSet<PathBuf>,
    by_library: &mut HashMap<PathBuf, Vec<NkiEntry>>,
    all: &mut Vec<NkiEntry>,
) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_file() && is_nki(&path) {
            if seen.insert(path.clone())
                && let Some(entry) = make_entry(path)
            {
                by_library
                    .entry(lib_dir.to_path_buf())
                    .or_default()
                    .push(entry.clone());
                all.push(entry);
            }
        } else if path.is_dir() {
            let skip = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with('.')
                    || n.eq_ignore_ascii_case("samples")
                    || n.eq_ignore_ascii_case("documentation")
                    || n.eq_ignore_ascii_case("resources")
            });
            if !skip {
                let next_lib = if depth == 0 { &path } else { lib_dir };
                scan_extra_nkis(&path, next_lib, depth + 1, seen, by_library, all);
            }
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(xdg));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"))
}

/// FNV-1a-64 hash matching `moose_utils::state::hash_plugin_id`.
fn hash_plugin_id(id: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Serializes `saved` into KONTRA v3's canonical OAST v1 plugin state blob
/// (identical to `KontraParams` saved by moose CLAP and VST3 wrappers).
pub(crate) fn serialize_kontra_oast(saved: &Saved) -> Vec<u8> {
    let json = saved.write();
    let json_bytes = json.as_bytes();
    let str_len = u32::try_from(json_bytes.len()).unwrap_or(u32::MAX);
    let field_len = str_len.saturating_add(4);

    // KeyedPersistWrite format: u32 count, then (u32 key_len, key, u32 field_len, field_payload).
    // String field payload: u32 str_len + UTF-8 bytes.
    let mut persist = Vec::with_capacity(4 + 4 + 4 + 4 + 4 + json_bytes.len());
    persist.extend_from_slice(&1u32.to_le_bytes()); // 1 persist field: "rack"
    persist.extend_from_slice(&4u32.to_le_bytes());
    persist.extend_from_slice(b"rack");
    persist.extend_from_slice(&field_len.to_le_bytes());
    persist.extend_from_slice(&str_len.to_le_bytes());
    persist.extend_from_slice(json_bytes);

    let mut out = Vec::with_capacity(64 + persist.len());
    out.extend_from_slice(b"OAST");
    out.extend_from_slice(&1u32.to_le_bytes()); // STATE_VERSION_V1
    out.extend_from_slice(&hash_plugin_id(KONTRA_PLUGIN_ID).to_le_bytes());
    // 2 base params: Volume (id=0, 0.0 dB), Auto-align (id=1, false = 0.0).
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0.0f64.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0.0f64.to_le_bytes());
    // Empty extra block.
    out.extend_from_slice(&0u64.to_le_bytes());
    // Persist block.
    out.extend_from_slice(&(persist.len() as u64).to_le_bytes());
    out.extend_from_slice(&persist);
    out
}

pub fn run_chunk(args: &mut dyn Iterator<Item = String>) -> Result<()> {
    let mut chunk = None;
    let mut roots = Vec::new();
    let mut index = None;
    let mut out_state = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => roots.push(PathBuf::from(args.next().context("--root needs a path")?)),
            "--index" => index = Some(PathBuf::from(args.next().context("--index needs a path")?)),
            "--out-state" => {
                out_state = Some(PathBuf::from(
                    args.next().context("--out-state needs a path")?,
                ));
            }
            _ if arg.starts_with('-') => bail!("unknown option {arg}"),
            _ if chunk.is_none() => chunk = Some(PathBuf::from(arg)),
            _ => bail!("unexpected argument {arg}"),
        }
    }
    let chunk = chunk.context("usage: kontra-inspect migrate-chunk <chunk.nkm> [--root DIR]... [--index FILE] [--out-state FILE]")?;
    let mut matcher = LibraryMatcher::open(&roots, index.as_deref());
    let report = matcher.convert_chunk(None, &chunk, out_state.as_deref());
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub fn run_batch(args: &mut dyn Iterator<Item = String>) -> Result<()> {
    let mut manifest = None;
    let mut roots = Vec::new();
    let mut index = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => roots.push(PathBuf::from(args.next().context("--root needs a path")?)),
            "--index" => index = Some(PathBuf::from(args.next().context("--index needs a path")?)),
            _ if arg.starts_with('-') => bail!("unknown option {arg}"),
            _ if manifest.is_none() => manifest = Some(PathBuf::from(arg)),
            _ => bail!("unexpected argument {arg}"),
        }
    }
    let manifest = manifest.context(
        "usage: kontra-inspect migrate-batch <manifest.json> [--root DIR]... [--index FILE]",
    )?;
    let text = std::fs::read_to_string(&manifest)
        .with_context(|| format!("reading {}", manifest.display()))?;
    let jobs: Vec<BatchJob> =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", manifest.display()))?;
    let mut matcher = LibraryMatcher::open(&roots, index.as_deref());
    let reports: Vec<ChunkReport> = jobs
        .into_iter()
        .map(|job| matcher.convert_chunk(Some(job.id), &job.chunk, job.out_state.as_deref()))
        .collect();
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oast_state_matches_kontra_v3_hash_and_persist_layout() {
        let saved = Saved::new(vec![SavedPart::new(
            0,
            PathBuf::from("/music/Test.nki"),
            kontra_host::rack::Mix::DEFAULT,
        )]);
        let blob = serialize_kontra_oast(&saved);
        assert_eq!(blob.get(..4), Some(&b"OAST"[..]));
        let version = blob
            .get(4..8)
            .and_then(|b| b.try_into().ok())
            .map(u32::from_le_bytes);
        assert_eq!(version, Some(1));
        let hash = blob
            .get(8..16)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes);
        assert_eq!(hash, Some(0xafec_6cd5_be98_053c));
        // Skip header(16) + count(4) + 2*param(24) + extra_len(8) = 52
        let persist_len = blob
            .get(52..60)
            .and_then(|b| b.try_into().ok())
            .map(u64::from_le_bytes);
        assert_eq!(persist_len, Some((blob.len() - 60) as u64));
        let persist = blob.get(60..).unwrap_or_default();
        // count=1, key_len=4, "rack", field_len, str_len, json
        assert_eq!(persist.get(8..12), Some(&b"rack"[..]));
        let json_bytes = persist.get(20..).unwrap_or_default();
        let parsed = Saved::parse(std::str::from_utf8(json_bytes).unwrap_or_default());
        assert_eq!(parsed, Ok(saved));
    }

    #[test]
    fn normalize_name_handles_numbered_patches() {
        assert_eq!(normalize_name("01. Horn 1"), "horn1");
        assert_eq!(normalize_name("01 Areia - 16 Violins"), "areia16violins");
        assert_eq!(normalize_name("Horn 1"), "horn1");
        assert_eq!(
            normalize_unnumbered("Areia - 08 Full Ens - Spiccato Fast"),
            normalize_unnumbered("Areia - 03 Full Ens - Spiccato Fast")
        );
        assert_ne!(
            normalize_unnumbered("01. Horn 1"),
            normalize_unnumbered("04. Horn 4")
        );
    }
}
