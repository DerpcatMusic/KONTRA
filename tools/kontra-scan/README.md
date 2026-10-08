# Shared KONTRA scanner

Built 2026-10-08T06:21:15.347955+00:00 in optimized `cargo build --release --example kontra_scan --features shots`.

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

Each invocation defaults to 235 seconds and releases its heavy slot; hard allowed budget <=240 seconds. Default worker timeout is 90 seconds, truncated to the shard remainder. Exit 75 means items remain in the requested slice; rerun the SAME arguments to resume, then advance start. Each shard needs ONE heavy call; never hold a heavy slot around a loop of shards. One heavy job per agent. Do not set build target or slot env vars.

Cache identity includes binary SHA-256, item path, container byte size and mtime; the installed corpus/resources must stay immutable during a sweep. Use a fresh --out after changing resources. `out/cache/*.json` is metrics only. `out/results.tsv` is regenerated from this binary's cache. Do not treat partial output as a complete corpus. `--shots` retains only screenshots of OUR renderer in out/items; use it for a small selected gallery, keeping the total below 50 MB. No samples, decrypted scripts, source resource bytes, key material or authored text/property values are written.

## TSV meanings

`path, library, loads, ui, controls_bound, plays_note, load_ms, peak_rss_mb, reason` (TAB-separated).

- loads yes = importer and initial playable bank/plan construction returned successfully. For v1 this is its initial streaming bank (`Bank::load_bare`); v2 uses the production `V2Loader` streamed plan. This is load admission, separate from a complete working GUI or native sound parity. A worker timeout/exception is loads no with reason and stage; it is a bounded observation, not proof the instrument can never load. An empty/missing sample mapping can still be admitted; consult JSON and plays_note.
- ui judges ONLY Original bitmap authored view; vector never runs. Precedence: error > blank > missing-images > no-ui > original-ok. original-ok means the authored view built/rendered and its requested pictures resolved; it DOES NOT certify every widget, typography, gesture or callback. Detailed counters live in JSON. v1 uses its production whole-editor Original layout; blank detection is authored visibility, not shell-pixel uniformity. V2 paints authored components and records pixel uniformity/white fraction. Geometric flags are candidates, not native-host-verified defects.
- controls_bound = visible interactive runtime bindings / visible interactive widgets. V1 IDs route directly into its script runtime; v2 scalar IDs must read back from the installed Core. Array/service-backed widgets need additional semantics and can be unbound here.
- plays_note = yes if any measured multi program emits finite audio above 1e-5; silent if admitted but not audible for one mapped note in ~0.5 seconds with CC1=100, CC11=127; no if loading failed. This is an audition, not DSP/reference validation.
- load_ms = import/script/sample-bank construction wall time (v1 excludes UI render; v2 loader includes asset metadata resolution), separate from process_ms and UI renders. v2 currently reports zero for failed loads; failure duration is in process_ms. Peak RSS covers the isolated worker, including assets/UI/audio.

An installed official UVI 4.0.9 reader is selected if KONTRA_UVI_READER is unset; an explicit environment selection wins. UVI IDs in items.tsv are bank.ufs::program; the adapter uses the production bank.ufs/program virtual path.

## Outputs and checks

Canonical sweep directories: ../results/v1 and ../results/v2. Publishing script `tools/kontra-scan/summary.py` writes ../results/v1.tsv, v2.tsv, summary.md, taxonomy.json and v1-loads-v2-doesnt.tsv. Shared scanner check: `python3 tools/kontra-scan/check.py`. Rust pixel/source scanner check is `scan_metrics::tests::scanner_metrics_skip_source_text_and_detect_uniform_render` with the shots feature.

## Binary digests

- kontra-scan-v1 SHA-256 `4ab053cde8eb1197591cc3696ef38e99709a6ac52174c56fccc24f5596129caf`
- kontra-scan-v2 SHA-256 `1d28987a6aa6afd089277221b1249561b478a9ae387181ee945e9df6ac4c919a`

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
