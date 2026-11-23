---
status: implemented
project: kontra
owner: antigravity
created: 2026-11-23
updated: 2026-11-23
---

# Shared Format Intermediary Layer & Kontakt/UVI/Library Reliability

## Problem & Goal

- **Problem:**
  1. `kontra-format` (`crates/format/common`) currently provides only `Notes`, `moved`, `builder`, `math`, `timing`, and a seek-based audio header prober — there is no unified `Format` trait, registry, `ReadAt` abstraction, shared cross-platform path resolver, or structured DRM/protection contract (`06-FORMATS.md`).
  2. `kontra-format-uvi` duplicates 416 lines of WAV/FLAC header probing (`crates/format/uvi/src/header.rs`) because `kontra-format::header` lacked positioned `ReadAt` support, leaving UVI unable to probe RF64/AIFF/OGG members.
  3. Valid non-DRM-blocked Kontakt and UVI libraries fail to load or lose samples/resources on Linux due to case-sensitive lookups (`samples.rs`, `resources.rs::under`, `access.rs`), rigid library-root heuristics (failing when a library's sample folder is not literally `Samples` or when a nested subfolder has its own `Samples`), Windows/macOS path prefixes, moved `.nkx` archive remapping gaps, UVI alternate-extension resolution omitting `program_dir`/`Samples`, standalone `.uvip` files on disk being rejected, and `kontra-library` skipping symlinked libraries and `.nksn`-only folders.
  4. DRM-protected containers (e.g., PACE/iLok-locked UFS banks or encrypted Kontakt presets missing `.nicnt` access data) surface as opaque parse errors instead of structured `ProtectionStatus::Protected` diagnostics ready for a future UI lock badge.
- **Goal:**
  1. Implement the shared format intermediary layer in `kontra-format` (`Format` trait, `Registry`, `FormatId`, `Confidence`, `ProtectionStatus`, `ProtectionSystem`, `FormatError`, `ReadAt`, `paths`, and `ReadAt`-backed `header` probing).
  2. Consolidate all audio header probing into `kontra-format::header` and delete `crates/format/uvi/src/header.rs`.
  3. Expand Kontakt, UVI, and `kontra-library` discovery/loading reliability on Linux while reporting DRM-protected containers cleanly via `ProtectionStatus::Protected`.
- **Plan Reference:** `plugins/kontra/docs/architecture-v3/06-FORMATS.md` (§1 The contract) and `plugins/kontra/docs/architecture-v3/13-IR-VNEXT.md`.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** All format loading, sniffing, scanning, and path resolution happen strictly off the audio thread at load/scan time; no changes touch DSP or audio-thread paths.
- **Strict DRM Boundary:** Never implement DRM bypasses, key cracking/extraction, or circumvention of PACE/iLok/SINE/Opus/IK licensing checks. Only parse open, unencrypted, or legitimately self-contained container/metadata structures and report DRM-protected payloads honestly via `ProtectionStatus::Protected`.
- **Determinism & Compatibility:** Existing `kontra-format`, `kontra-format-kontakt`, `kontra-format-uvi`, and `kontra-library` public APIs stay backward-compatible for sibling crates (`kontra-host`, `kontra-plugin`, `kontra-format-decentsampler`, `kontra-format-sfz`, `kontra-format-sf2`, `kontra-format-native`). `Notes::add_missing` preserves its `"samples not found"` + `"\nfirst: "` diagnostic format consumed by `kontra-host::relink`.
- **Workspace Lints:** Zero `unsafe`, `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`, or unchecked indexing in production code (`-D warnings`).
- **Mirror Boundaries:** Zero references to private monorepo paths or secrets inside `plugins/kontra`.
- **Lease Isolation:** Never modify files leased by parallel lanes (`crates/model/ir`, `crates/format/common/src/builder.rs`, `crates/format/kontakt/src/timed.rs`, `crates/format/kontakt/src/translate.rs`, `crates/format/uvi/src/program.rs`, `crates/format/uvi/src/program/{routing,values}.rs`, `crates/format/decentsampler`, `crates/app/ui*`).

### Non-Goals
- Refactoring `ir::ModKind` -> `ir::ModRoute` (owned by Lane L1).
- Changing `fx_idx_amp_split_point` or DSP/engine behavior (owned by PR #120 / L2-L3).
- Building UI widgets/icons for the lock badge (only exposing the structured `ProtectionStatus` data contract).
- Touching `crates/format/decentsampler` or `vendor/*`.

## API / Data / File Contract

### `kontra-format` (`crates/format/common`)
- `FormatId`: `#[non_exhaustive]` enum (`Kontakt`, `Uvi`, `Sfz`, `Sf2`, `DecentSampler`, `Native`), `Display`, `as_str()`, `Serialize`/`Deserialize`.
- `Confidence`: ordered enum (`None < Extension < Header < Exact`).
- `ProtectionSystem`: `#[non_exhaustive]` enum (`PaceIlok`, `NativeAccess`, `Unknown(String)`), `Display`, `Serialize`/`Deserialize`.
- `ProtectionStatus`: enum (`Open` (default), `Protected { system: ProtectionSystem, detail: String }`), `Serialize`/`Deserialize`.
- `FormatError`: structured `thiserror::Error` with `Protected { path, status }`, `Io`, `NotFound`, `Unsupported`, `Malformed`, plus `protection(&self) -> Option<&ProtectionStatus>`.
- `EntryRef { path: PathBuf, member: Option<String> }`, `ScanEntry`, `ScanSink`, `ScanCollector`, `LoadOptions`, `LoadCx<'a>`.
- `pub trait Format: Send + Sync + 'static`: `id(&self) -> FormatId`, `sniff(&self, path, head) -> Confidence`, `protection(&self, path) -> Result<ProtectionStatus, FormatError>`, `scan(&self, root, out) -> Result<(), FormatError>`, `load(&self, entry, cx) -> Result<ir::Instrument, FormatError>`.
- `Registry`: registers `Box<dyn Format>`, sniffs by highest `Confidence`, probes protection, scans roots, and dispatches `load`.
- `io`: `ReadAt` trait (`read_at`, `read_exact_at`), impls for `File` and `[u8]`/`&[u8]`/`Vec<u8>`, `SliceReader<R>`, `TransformedReader<R>` (with optional `&ir::Keystream`).
- `paths`: `normalize_separators`, `normalize_virtual_path`, `lexical_clean`, `strip_foreign_root`, `DirCache`, `resolve_case_insensitive`, `resolve_case_insensitive_bounded`, `split_container_path`, `resolve_container_path`, `suffix_match_components`, and improved longest-prefix + case-aware `moved` / `moved_with_case`.
- `header`: `probe_read_at(&dyn ReadAt, u64) -> io::Result<AudioHeader>`, `pub fn probe_region`, `probe_bytes`, `probe_header`; `AudioLoop::to_ir_loop` converts repeat count accurately to `ir::Loop::count` (total passes).

### `kontra-format-kontakt`
- `pub struct Kontakt` implementing `kontra_format::Format` (`Kontakt::ID = FormatId::Kontakt`).
- `inspect_protection(path: &Path) -> Result<ProtectionStatus, Error>` and `Error::protection(&self) -> Option<&ProtectionStatus>`.
- Linux case-insensitive sample/archive/resource resolution (`DirCache`), foreign-OS prefix stripping, moved `.nkx` archive remapping, component-boundary suffix disambiguation, improved `find_root` / `.nicnt` discovery.

### `kontra-format-uvi`
- Delete `src/header.rs`; probe all UFS members via `kontra_format::header::probe_read_at` + `TransformedReader`.
- `pub struct Uvi` implementing `kontra_format::Format` (`Uvi::ID = FormatId::Uvi`).
- `inspect_protection(path: &Path) -> Result<ProtectionStatus, Error>` and `Error::protection(&self) -> Option<&ProtectionStatus>` distinguishing PACE/iLok-protected banks from corrupt/XML/codec failures.
- Fixed `Bank::resolve` (alternate extensions across all candidate roots, `$` prefix handling, trailing-component disambiguation), normalized `Bank::audio_under`, and standalone on-disk `.uvip` loading.

### `kontra-library`
- Add `pub protection: ProtectionStatus` to `Library` (default `Open`) and `Issue` (default `Open`); bump `Cache` `VERSION` to `2` to cache protection metadata and use atomic-counter temp filenames.
- Safe symlink following with canonical-directory cycle detection during root detection and library walks.
- Discover `.nksn`-only snapshot folders and expose `BankListing` / `scan_with_protection` (while keeping `scan` backward-compatible).

## Verification Matrix

| Check | Exact Command | Pass Criterion |
|---|---|---|
| Format & library unit + integration tests | `cd plugins/kontra && build-slot cargo nextest run -p kontra-format -p kontra-format-kontakt -p kontra-format-uvi -p kontra-library --no-fail-fast` | Exit 0, zero failures |
| Dependent format & host tests | `cd plugins/kontra && build-slot cargo nextest run -p kontra-format-sfz -p kontra-format-sf2 -p kontra-format-native -p kontra-host --no-fail-fast` | Exit 0, zero failures |
| Clippy (excluding pre-existing decentsampler lint if present) | `cd plugins/kontra && build-slot cargo clippy -p kontra-format -p kontra-format-kontakt -p kontra-format-uvi -p kontra-library -p kontra-host -p kontra-plugin --all-targets --no-deps -- -D warnings` | Exit 0, zero warnings |
| Formatting | `cd plugins/kontra && cargo fmt --check` | Exit 0 |
| Real library verification | `cd plugins/kontra && build-slot cargo run -p kontra-host --example library -- /data/main/Libraries/Kontakt /data/main/Libraries/UVI` + real-library tests | All installed Kontakt & UVI libraries discovered and representative patches load cleanly |

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Implement `kontra-format` shared intermediary (`format.rs`, `io.rs`, `paths.rs`, `header.rs`, `lib.rs`) | `plugins/kontra/crates/format/common/Cargo.toml`, `src/{lib,format,io,paths,header}.rs` | — | `cargo nextest run -p kontra-format` |
| 2 | Audit & expand `kontra-format-kontakt` (`Format` impl, `ProtectionStatus`, Linux case/root/moved resolution) | `plugins/kontra/crates/format/kontakt/{Cargo.toml,src/{lib,access,container,impulse,resources,samples,snapshot}.rs,tests/**}` | 1 | `cargo nextest run -p kontra-format-kontakt` |
| 3 | Audit & expand `kontra-format-uvi` (delete `header.rs`, `Format` impl, `ProtectionStatus`, path resolution, standalone `.uvip`) | `plugins/kontra/crates/format/uvi/{Cargo.toml,src/{lib,bank,header,keys}.rs}` | 1 | `cargo nextest run -p kontra-format-uvi` |
| 4 | Audit & expand `kontra-library` (symlinks with cycle guard, `ProtectionStatus` on `Library`/`Issue`/`Cache`, `.nksn` discovery) | `plugins/kontra/crates/app/library/**` | 1 | `cargo nextest run -p kontra-library` |

## Rollout & Affected Dependents

- **Affected Projects:** `kontra` only (`plugins/kontra`).
- **Mirror Impact:** Subtree mirror (`plugins/kontra` -> `DerpcatMusic/KONTRA`); all changes stay strictly within `plugins/kontra` with zero monorepo leaks.
- **Versioning & Changelog:** Internal architecture & reliability PR (`kontra: ...`); no version bump required.
