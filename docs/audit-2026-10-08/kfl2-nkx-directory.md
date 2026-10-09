# KFL2 NKX directory rejection

The Linux tester on `0.3.344-nightly.20261009.g7cd326ee5b67` cannot load Kontakt Factory Library 2 → Orchestral / 1 - Woodwinds / Clarinet Bb, Oboe or Piccolo. Translation fails in `KFL2O_02.nkx` directory indexing with `Duplicate or excessive NKX member`. The supplied triage identifies the case-folded full-path collision branch, rather than the million-member limit.

`vendor/ni-file/src/nkr/archive.rs` previously rejected an entire directory index when a member name repeated after lowercasing. The failing-first synthetic compatibility fixture reproduces that exact error. V1 `0cb7a8a0` has the same rejecting reader, so there is no working v1 policy to port.

## Reader policy

- Preserve each distinct exact-case full path; prefer that spelling when resolving a member.
- Identical paths and a non-exact case-insensitive fallback use directory-order last-wins.
- Store additional variants only for collisions; ordinary indexes retain the existing folded map.
- Keep version, component, truncation, cycle/depth and per-directory limits. Count every file record, including repeats, toward the global 1,000,000-member limit.
- Validate lazy and eager member headers by the existing path. Payload decoding, library access and activation remain outside this change.

The `members()` iterator includes both case variants. Resource-container enumeration, the format survey and header-artwork enumeration use it. Header artwork no longer inserts a validated alias into the folded map, which would overwrite its fallback selection. Resource reads and encryption checks are unchanged.

## Evidence and limits

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-kfl2-nkx-344/`.

- RED: `red.log`, exit 101, exact `Duplicate or excessive NKX member`; test commit `be23aabe`.
- GREEN: `green.log`, ni-file library 34 passed / 53 ignored; compatibility 33 passed / 1 ignored (the explicit local-corpus probe).
- Fixtures cover kinds 0/2/4, identical references, case-distinct synthetic payloads under lazy/eager lookup, recurring case alternation, separate directories with the same basename, the declared directory limit, and repeated records at the global member limit.
- Mounted inventory: 270 NKX files, all under `/mnt/MAIN_STORAGE/Libraries/Kontakt`, no enumeration errors. KFL2 is absent. `inventory.json` and `nkx-paths.txt` freeze coverage.
- Actual pre-fix and candidate production `read_index` probes both index 270/270 files; zero local duplicate/member-limit failures, unchanged member counts and issue counts. See `baseline-scan.json` and `candidate-scan.json`. These probes read directories only, without member payloads or keys.
- Root compile check is recorded in `root-no-run.log`; it uses this isolated 0.3.326-derived worktree, not the frozen 0.3.355 release.

Reproduce the synthetic check through `~/.cache/kontakto-heavy cargo test --manifest-path vendor/ni-file/Cargo.toml --lib --test compatibility`. For the directory-only corpus probe, set `KONTRA_NKX_PATHS` to the receipt's `nkx-paths.txt` and run the same compatibility target with `local_nkx_directory_corpus_probe -- --exact --ignored --nocapture`.

Native duplicate tie-break behavior and actual KFL2 playback remain unverified without the tester's archive or a native receipt. This establishes directory-reader acceptance; it does not establish full KFL2 instrument or DRM compatibility.

P2 deferred from the same tester bundle: NOIRE prepare ~5 s and Kemence/Stradivari slow loads (`tester-logs-NwxC/TRIAGE.md`, row 12). Pacific/Conflux first-audio regressions and Analog readiness RSS remain separate open load targets.
