# Pinned v1 CPU adapter

Product source and Cargo.lock are exactly v1 `0cb7a8a0`. The example adapts the original CPU audit engine seam and copies the CLI bench-stream/load path for equivalence validation. It writes metadata only; decoded PCM stays in RAM. The note/block sequence is copied from v2 `73e6089b:tools/cpu-audit-common.rs`.

Build through `~/.cache/kontakto-heavy env CARGO_PROFILE_RELEASE_STRIP=none CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --locked --release --example cpu_audit_v1 --no-default-features --features library-access`. No target-cpu override is used. Keep parsed-instrument cache disabled with `XDG_CACHE_HOME=/dev/null`.

Audit: `cpu_audit_v1 ORIGINAL_NKI 32|64|256 piano|strings|fx`.
Equivalence: `cpu_audit_v1 bench-stream ORIGINAL_NKI NOTES SECONDS`; compare with the untouched frozen CLI using the same arguments.
Profiles use CPU_AUDIT_READY/CPU_AUDIT_FINISHED markers around the paced sequence, excluding load and teardown. Leaf samples are estimates of exclusive CPU cost, not exact stage timers or proof of p99 attribution.

Frozen path: `~/.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1`; provenance is in BUILD.json next to it. This adapter does not implement product changes or replace anything under `~/.cache/kontra-v1`.

## Frozen CLI equivalence

Three alternating pairs per instrument, 64 notes/s for 10s. All runs returned 0. Preload, mean/peak voice counts and underruns match; all results remain in `~/.cache/kontakto-fix-cpu/v1-adapter-equivalence-2/summary.json`. Whole child-process CPU includes loading. The first unforced-cache load is retained.

| Instrument | Repeat | Frozen process CPU s | Adapter process CPU s | Frozen render/worker % | Adapter render/worker % |
|---|---:|---:|---:|---:|---:|
| piano | 1 | 0.876986 | 0.745532 | 1.8/2.9 | 1.8/2.8 |
| piano | 2 | 0.745259 | 0.816555 | 1.7/2.7 | 1.8/2.8 |
| piano | 3 | 0.755305 | 0.757445 | 1.8/2.8 | 1.8/2.8 |
| fx | 1 | 4.104267 | 3.403594 | 9.5/4.5 | 9.0/4.3 |
| fx | 2 | 3.459957 | 3.462545 | 9.2/4.3 | 9.3/4.4 |
| fx | 3 | 3.429828 | 3.434116 | 9.1/4.3 | 9.0/4.3 |

Process CPU medians: piano 0.755305 → 0.757445s (+0.28%); FX 3.459957 → 3.434116s (-0.75%). Observed three-run CPU ranges overlap and median differences are below 5%, the declared validation bound. This validates the adapter benchmark path; it does not certify v2 parity. The first attempt could not launch because /usr/bin/time was absent; its failure receipt is retained, and the completed retry measures child CPU with wait4.

The profile-only epoch marker is written before pacing, outside timed/counting sections, only when CPU_AUDIT_READY is set. It allows realtime-clock perf samples to select the same 0.25–1.0s steady-note window without inferring it from file timestamps. No user stack or PCM is sampled.

## Streaming challenge schedules (pending validation)

The probe-only `audit/w9-v1-stream-schedules-20261008` branch adds shared
`tools/cpu-audit-schedules.rs`, byte-identical to W9's v2 trial. The original
piano/strings/fx event schedules remain unchanged. New scenario arguments:
`fast-repeat`, `legato`, `cold-jump`, each run on Areia Full at block32.
Fast-repeat is two8s phases at quarter140 BPM with half-length gates and
alternating velocities64/127: first key60, then keys36/48/60/72.
Legato uses literal eighth=180 BPM (333⅓ms per note), 24 chromatic notes48–71,
with30ms overlap. Cold jump sends eight keys spread0–127 in the first block
without the idle phase. Both adapters use the same block dispatch convention;
original sample timestamps remain available for the gate to reuse.
The frozen original adapter remains untouched. New build, schedule tests and
CLI-equivalence evidence are pending; no new measurements are claimed here.

The shared schedule helper is byte-identical. The common runner differs only
by `main`→`audit_main`, required by the existing bench-stream CLI shim. Freeze
receipts store both the actual adapter-file digest and the digest normalized
for that function name. The initial duplicate-main compile failure is retained
in budget128-v1-adapter-check.log; corrected schedule tests4/4 PASS. Required
root no-run and the new frozen executable remain pending.

The library-access-only root no-run attempt found unchanged v1 tests referring
to plugin allocation helpers while that feature was disabled. Schedule tests
already passed; the root no-run retry uses the normal default plugin features.
This is a validation configuration correction; no v1 product files are edited.

Corrected validation is green: shared schedule checks4/4 PASS and default-feature
root `cargo test --locked --no-run` PASS. Product source and Cargo.lock still
match pinned v1; optimized non-test executable freeze follows this commit.
