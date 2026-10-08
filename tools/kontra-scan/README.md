# Shared KONTRA scanner

Optimized release builds; the installed README records the frozen source commits, binary digests and build date.

- `kontra-scan-v2`: integrate/core-v2 HEAD base `7e82b152b46b31e8b9fd85c2ac01d01dad669ddd` plus scanner-only instrumentation.
- `kontra-scan-v1`: pinned v1 base `0cb7a8a0` plus scanner-only instrumentation; no product fixes. V1 parsed/source and header caches are disabled for scanner workers to prevent decrypted records being written.
- Both binaries invoke ONE adjacent stdlib Python CLI, `kontra_scan.py`; keep it beside them. Python 3 and Linux /proc are required.

## Usage

```
/home/derpcat/.cache/kontakto-heavy /home/derpcat/.cache/kontra-scan/bin/kontra-scan-v2 \
  --list /home/derpcat/.cache/kontra-scan/v2-items.tsv --start 0 --count 25 \
  --out /home/derpcat/.cache/kontra-scan/results/v2
```

Use `kontakt-items.tsv` for v1 (834: 781 NKI + 53 NKM), `uvi-items.tsv` for v2 UVI (660), or `v2-items.tsv` for both. These frozen lists put the mandatory Conflux NKI first. The original shared corpus list also works. `--list '/path/**/*.nki'` accepts a quoted recursive glob. Start is ZERO-based; count is manifest rows, each multi is one row with every embedded program measured.

Each invocation defaults to 235 seconds and releases its heavy slot; hard allowed budget <=240 seconds. Default worker timeout is 90 seconds. A new item starts only when its full timeout fits in the remaining shard budget. Exit 75 means items remain in the requested slice; rerun the SAME arguments to resume, then advance start. Each shard needs ONE heavy call; never hold a heavy slot around a loop of shards. One heavy job per agent. Do not set build target or slot env vars.

Cache identity includes binary SHA-256, item path, container byte size and mtime, the frozen UVI sidecar digest and the per-ID note plan; the installed corpus/resources must stay immutable during a sweep. Use a fresh --out after changing resources. `out/cache/*.json` is metrics only. `out/results.tsv` is regenerated from this binary's cache. Do not treat partial output as a complete corpus. `--shots` retains only screenshots of OUR renderer in out/items; use it for a small selected gallery, keeping the total below 50 MB. No samples, decrypted scripts, source resource bytes, key material or authored text/property values are written.

## TSV meanings

`path, library, loads, ui, controls_bound, plays_note, load_ms, peak_rss_mb, reason` (TAB-separated).

- loads yes = importer and initial playable bank/plan construction returned successfully. For v1 this is its initial streaming bank (`Bank::load_bare`); v2 uses the production `V2Loader` streamed plan. This is load admission, separate from a complete working GUI or native sound parity. A worker timeout/exception is loads no with reason and stage; it is a bounded observation, not proof the instrument can never load. An empty/missing sample mapping can still be admitted; consult JSON and plays_note.
- ui judges ONLY Original bitmap authored view; vector never runs. Precedence: budget-hit > error > blank > missing-images > no-ui > original-ok. original-ok means the authored view built/rendered and its requested pictures resolved; it DOES NOT certify every widget, typography, gesture or callback. Detailed counters live in JSON. v1 uses its production whole-editor Original layout; blank detection is authored visibility, not shell-pixel uniformity. V2 paints authored components and records pixel uniformity/white fraction. Geometric flags are candidates, not native-host-verified defects.
- controls_bound = visible interactive runtime bindings / visible interactive widgets. V1 IDs route directly into its script runtime; v2 scalar IDs must read back from the installed Core. Array/service-backed widgets need additional semantics and can be unbound here.
- plays_note = yes if any measured multi program emits finite audio above 1e-5; silent if admitted but not audible for one declared/covered or explicit safe fallback note in ~0.5 seconds with CC1=100, CC11=127; no if loading failed or every program has no safe audition key. This is an audition, not DSP/reference validation.
- load_ms = import/script/sample-bank construction wall time (v1 excludes UI render; v2 loader includes asset metadata resolution), separate from process_ms and UI renders. v2 currently reports zero for failed loads; failure duration is in process_ms. Peak RSS covers the isolated worker, including assets/UI/audio.

An installed official UVI 4.0.9 reader is selected if KONTRA_UVI_READER is unset; an explicit environment selection wins. UVI IDs in items.tsv are bank.ufs::program; the adapter uses the production bank.ufs/program virtual path.

## Outputs and checks

Canonical sweep directories: ../results/v1 and ../results/v2. Publishing script `tools/kontra-scan/summary.py` writes ../results/v1.tsv, v2.tsv, summary.md, taxonomy.json and v1-loads-v2-doesnt.tsv. Shared scanner check: `python3 tools/kontra-scan/check.py`. Rust pixel/source scanner check is `scan_metrics::tests::scanner_metrics_skip_source_text_and_detect_uniform_render` with the shots feature.

## Binary digests

- kontra-scan-v1 SHA-256 `870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`
- kontra-scan-v2 SHA-256 `d4534838916e008d32a6e0763541a8bd9285651d77f130bad18fe926d0975f5a`
- kontra-scan-v1-uvi SHA-256 `34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1`

## Rebuild from the shared source branch

The self-contained v2 scanner source branch is `tools/kontra-scan`, based on `7e82b152b46b31e8b9fd85c2ac01d01dad669ddd`. Resolve its exact commit with `git rev-parse origin/tools/kontra-scan` after fetching; the installed README beside the binaries records the frozen scanner commit SHA. W0 can merge this branch into future integration HEADs. Production additions are gated by `shots` (an audit/CPU-render feature); default plugin builds do not include the scanner adapter.

```sh
git fetch origin tools/kontra-scan
git worktree add /path/to/scan-build origin/tools/kontra-scan
cd /path/to/scan-build
/home/derpcat/.cache/kontakto-heavy cargo build --release --example kontra_scan --features shots
# Copy that worktree's release/examples/kontra_scan and tools/kontra-scan/kontra_scan.py beside one another.
```

The pinned-v1 adapter remains on the separately based `audit/ui-census-v1-scanner-20261008` worktree; it is not a product change to merge into v2. The common CLI and metrics sources are the same.

## Extension schema (single shared sweep)

The first nine columns remain unchanged. Additional columns are generated by `kontra_scan.py:COLUMNS`; detailed per-program/per-wire-slot evidence is in each cached JSON. `unknown` means the observed path did not expose the metric, never an inferred zero.

- Raw slots partition into decode_failed, bypassed, inline_nonempty, linked_only and empty. Production disposition independently uses the loader's untrimmed linked-name rule. Raw `wire_slot` remains distinct from v1's compacted `runtime_slot`; owner/program index stay in metadata. Final v2 runtime-preparation attempts alone enter slot success totals; import-harvest and dynamic-rack attempts remain separate.
- V1 `compile_admitted` uses the existing production compiler result, `compile_clean` additionally requires zero `Program.errors`. Those errors are disabled non-init callback blocks, not evidence of preprocessor-inactive regions. V2 records lex/preprocess/parse/sema/init/persistence_changed/lower failures. Inactive-region classification stays unknown unless independently established.
- Init and persistence callbacks have independent observed statuses/completion/faults. V1 statuses follow actual VM yields, spawn/drop and compiler disabling. V2 completion follows the actual evaluator result before warning suppression. A persistence fault can coexist with successful loader admission. Diagnostics contain phase, fixed category/kind, static builtin if available and numeric position; no message, identifier, source or payload is serialized. V1 retained load-fault records are separate from terminal callback statuses. V2 note-time Fault/FuelExhausted records are drained with `flush_behaviors_at`, separate from load faults.
- Raw saved-entry histograms contain only `$ ~ % ? @ ! empty other`. A bounded framing check distinguishes absent/decoded/malformed/unknown tables rather than trusting `params()`'s empty-Vec fallback. Admitted histograms come from the production loader's state. Admission does not prove declaration-aware restoration; no names or values leave memory.
- Lua init/runtime faults and their first diagnostics are observed separately in the scripted worker. The first exported diagnostic is a fixed category plus digest, never authored text. MUI tree budget produces `ui=budget-hit`; actual paint, image lookup/decode, custom fonts, picture strips/frames/margins and widget-kind counts are retained. Baseline v2 has no custom-font service, so font success is zero when observed; asset failures report lookup/decode/font service categories. Bound controls are scalar readback, not gesture validation.
- Background colour/fraction is measured on the v2 Original authored page (one RGBA-unit pixel tolerance). Without a declared colour, modal colour is explicitly inferred. V1 whole-editor pixels cannot establish authored-page coverage and remain unknown.

## Identical audition notes

`~/.cache/kontra-scan/notes/<sha256(item-ID)>.json` stores only per-program MIDI key/velocity, an optional numeric keyswitch and a fixed selection-source enum. `KONTRA_SCAN_NOTE_PLAN` passes it to either worker; no instrument data is stored. V2 goes first and establishes the plan. Prefer declared white keys intersecting active sample-zone coverage at velocity 64, then zone coverage nearest middle C, then a fallback. Explicit invalid/control/keyswitch keys are avoided. Columns retain `note_picked`, `pick_source` (native_declared / zone_coverage / fallback), valid keys, conflicts and native preference. A white key is an authored declaration, not proof of audible PCM. `audition_status=audition-mismatch` prohibits an audio regression claim. Loaded/silent rows can still have uncertain range declarations.

Audited v2 7e82b152 lacks the newer root keyboard snapshot accessor. Scanner-only observation at the existing inert setKeyColour/resetKeyColour stub preserves return behavior, and treats conflicting writes conservatively as unknown. Pinned Kontakt v1 uses its existing KSP key declarations. This is observation, not a product keyboard fix.

## V1 UVI sidecar

Pinned v1 Kontakt stays at 0cb7a8a0. It has no UVI implementation. `::` IDs route through adjacent `kontra-scan-v1-uvi`, supplied by the UVI auditor: branch `audit/uvi-v1-scanner-20261008`, source `026bdbb49f29a5ad752b3470a5f6f64a20a8957d`, product base `4bffbb18`, optimized release. SHA-256 `34a61e82ca6f09afc0eab692a1bf0b6765edbd7d063fccca94d95e699a93b9c1`. Its snapshots use conservative processor-local merging: #00FFFFFF valid, #00000000 invalid, other colours highlights, absent/conflicting unknown. This later v1 UVI baseline is explicitly separate from pinned Kontakt. Both use the same shared driver and note-plan contract; use `v2-items.tsv` to census both corpora with either version.

### Rebuild pinned Kontakt v1 from this bundle

```sh
python3 tools/kontra-scan/prepare-v1.py /path/to/scan-v1
cd /path/to/scan-v1
/home/derpcat/.cache/kontakto-heavy cargo build --release --example kontra_scan --features shots
```

The patch contains only feature-gated v1 adapter hooks. The preparer copies the same driver/metrics from this source bundle and disables the v2-only host check; v1's actual VM-yield test remains enabled. UVI sidecar source/rebuild provenance is the separate auditor branch above.

The scanner always sends a safe fallback note when load-time zone coverage is empty (note callbacks can enable zones), avoiding declared invalid/control/keyswitch keys. `sample_zone_count` is the retained instrument IR mapping count (v1: loaded bank mappings), or unknown if the adapter exposes no retained instrument. Raw decoded mappings remain separately in JSON as decoded_zone_count. Runtime script gating/purge is not inferred from these counts; sample residency can include reserved cache memory and does not prove usable PCM. Entirely invalid keyboards have no safe audition key: plays_note=no and an explicit reason, rather than silently skipping MIDI while claiming a silent note.

Before a rebuild, run `python3 tools/kontra-scan/generate-symbols.py` (v2 source worktree) and `--check` to verify the whitelist matches compiler UI/keyboard/persistence tables plus the pinned public spec. Both adapters use the generated file. The small vendor-extension set is explicit and carries unknown semantics.

`bound_typed` counts visible text/array targets validated against installed KSP models separately from the frozen scalar/ID criterion; it does not certify live typed edits. UVI typed targets and phantom-free counts stay unknown on baselines lacking the necessary origin/readback accessor. Fallback auditions are flagged and excluded from parity. The shared numeric note plan retains the pre-audition declared keyswitch when available. `v1ok-v2missing.tsv` reports authored-UI regressions; `results/v2/symbol-aggregates.tsv` includes coverage, per-ID/program-owner incidence, NKI/NKM splits, initialized widget kinds and fixed saved sigils.

## Load onset (section J)

first_audio_ms is monotonic wall time from the first production program import to the first observed finite, exactly nonzero output block using the shared audition plan. It is unknown when no such output is observed. The audible audition still uses its separate 1e-5 threshold. ui_first_frame_ms is elapsed time to actual CPU paint completion, before pixel hashing/PNG writing. The lexical metadata prepass and worker spawn are outside this clock. Original painting and audition run concurrently in the isolated worker; timing is a scanner observation, not a native plugin scheduling benchmark. Later programs of a multi share the item clock.

load_ms is unchanged: pinned v1 includes script import/init and the deferred initial sample-bank preload; it is NOT first sound. cache_state describes the product cache condition, not the per-item metrics cache or OS page cache. Frozen pinned v1 scanner disables parsed/header cache reads and writes; frozen v2 7e82 has no product metadata cache, so both are cold under this scanner. OS page cache is uncontrolled. New product cache implementations must expose their actual cache condition before their warm/cold acceptance results can use this column.

Decoded source-slot disposition is independent of saved-table integrity. Only actual record/parameter errors count as decode_failed; an unknown saved-table format keeps its successfully decoded bypass/inline/link/empty category and an incomplete raw histogram.

The UVI J sidecar uses production Worker::start after the metadata/assets prepass, includes required pre-audition native snapshots, and paints concurrently with audio observation. Its load_ms retains the earlier legacy origin. The shared worker environment forces KONTRA_UVI_STATIC_PCM_CACHE=0, disabling persistent decoded PCM caching; cache_state=cold describes this product condition, not OS cache. Historical1c198e60/d565 sidecar remains frozen separately.
