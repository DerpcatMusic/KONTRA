# Frozen 0.3.326 load comparison

This probe compares the exact W0 CLAP artifact at source
`608a20a1687a501e161d80af72280639c122fbc3` with the untouched frozen v1
CLAP. It changes only the existing audit host and Python collector. Normal
host behavior remains unchanged unless `KONTRA_LOAD_HOST=1` is set.

Preparation, without plugin or instrument loads:

```sh
python3 tools/kontra-gate/load_host.py \
  ~/.cache/kontakto-w0/alpha-followup/frozen-artifacts.json \
  ~/.cache/kontakto-w0/alpha-followup/frozen-cli.json \
  ~/.cache/kontakto-fix-load/rss-owners/live-clap-load-326 \
  /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-load-030326-608a20a1
```

After W9's direct handoff, the launcher creates the W8 quiet request, waits for
the activity observer to report QUIET, grants that request, and invokes the
same command with `--run` through `kontakto-heavy`, with
`KONTRA_QUIET_OWNER=1` and `KONTRA_GATE_REQUIRE_QUIET=1`. The output directory
must be new. The request owner is `W8 release 0.3.326 load cells`.
Frozen-v1 checksums and all three W0 artifact hashes are checked before use.
The launcher removes its own request and grant on completion or failure and
hands the window directly to W6.

Six cells use native Kontakt presets: Conflux v1 then v2, Pacific v2 then v1,
and Analog v1 then v2. Native state export/readback, program 0, key 60,
velocity 100, CC1/CC11 127, 48 kHz, block 64, and a two-second audition are
shared. Product caches are disabled; the OS page cache is uncontrolled.
Native state and raw diagnostics stay in tmpfs. Retained receipts contain
numeric counters, hashes and redacted diagnostics.

The host samples real process VmRSS, VmHWM and VmSwap before state load,
at readiness, and after audition, before state save. The editor stays closed;
editor-open RSS remains UNKNOWN pending W13's separate evidence. Plugin
sample-memory counters do not substitute for RSS. VmHWM includes plugin
initialization before state load.

Both versions use the same external `load_finished` readiness flag, polled
by Python every 5 ms and the host every 1 ms. Plugin-reported elapsed time
and load stages remain separate. First-audio wall time starts at state load
and includes the existing fixed 100 ms post-ready warmup. First-audio frame
starts at audition and uses a finite amplitude threshold of 1e-7. These are
host-protocol measurements, not a claim of immediate first-note scheduling.

Every cell retains the existing unit/process/load-average/disk-I/O activity
timeline. Contended, silent, malformed, failed or unverifiable cells remain
UNKNOWN. No performance verdict is implied by preparation or self-checks.

Checks: C++ host `--self-check`, `check-live-host.py`, `check-load-host.py`,
Python byte compilation, and root `cargo test --locked --lib --no-run`.

## Recovered attribution, 2026-10-09

The 11:34Z app restart interrupted the conversation, not the worker. The existing
three-cell run finished with exit 0. Every cell was QUIET, finite, audible and
verified by native-state readback. The unit was inactive with MainPID 0 when
recovered. W8 wrote `FINAL.json` and `RELEASE.json`, removed its request and
grant, and sent the direct completion handoff to W6 (then W13).

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-load-attribution-326-20261009/`.
These are diagnostic runs with `KONTRA_AUDIT_LOAD=1`, not replacement parity
numbers. They use the same frozen 608a20a1 plugin/CLI and host digests above.

| Preset | Ready ms | First audio wall ms | Ready RSS MiB | After audio RSS MiB |
|---|---:|---:|---:|---:|
| Conflux | 1411.52 | 1512.48 | 138.43 | 138.67 |
| Pacific Legato | 1355.64 | 1463.74 | 133.69 | 135.15 |
| Analog Strings | 9626.30 | 9734.05 | 836.24 | 846.32 |

All three have zero swap. The host retains only the aggregate plugin `prepare`
stage (1371.16, 1338.85 and 9551.03 ms respectively). The detailed numeric
`AUDIT` lines were read from stderr but discarded by `live_host.observe`;
the raw capture was then removed from tmpfs. The original receipts remain
unchanged. No substage attribution can be recovered from their hashes.

The collector now retains `load_audit.records` on load probes, preserving ordered
records rather than summing nested spans or collapsing repeated script slots.
Only fixed public stage/context names, explicitly listed numeric fields and
boolean flags survive. Raw stderr, authored text and unknown keys remain out
of the numeric receipt. Each line and result count is bounded to 4096;
`dropped_records` makes truncation visible. The regression failed before the
collector change and passed after it, including malformed, nonfinite, private
text, oversized-number and overflow cases. Load-host and contention checks
also pass; normal FIFO root `cargo test --no-run` passed.

The existing six-cell uninstrumented quiet comparison still establishes a
load deficit on Conflux and Pacific and a readiness RSS deficit on Analog.
It does not establish editor-open RSS parity. The newer integration already
contains immutable-chain sharing; the frozen 326 artifact predates that code.
Further product edits must use the captured substages and current-source
measurements, rather than reinstalling an already integrated optimization.

NEXT: bounded numeric stage attribution, then current-source cold/warm load
and editor-open RSS comparison against frozen v1.

## Current-source follow-up, 2026-10-09

The repaired collector's normal-FIFO frozen326 diagnostic captured 33/21/33
ordered records for Conflux/Pacific/Analog, with zero dropped records. All three
were CONTENDED; these times cannot establish performance parity. Native state
readback passed and nonfinite output was zero. Numeric receipt:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-load-attribution-326-numeric-20261009/FINAL.json`.

Analog has 95,624 region templates. RSS rose from 255,080 KiB after sample
resolution to 757,044 KiB at sample preload, then 833,044 KiB during runtime
allocation. Eager preload admitted zero heads and took 0.97 ms. The major
increase therefore precedes runtime storage and is not eager sample data.
Current integration already contains immutable chain sharing, so repeating
that patch would not address a current-source deficit.

The remaining resolver allocation was concrete: `archive_member_where` built
a normalized member string for every ancestor, including ordinary loose-file
directories. The shared helper now uses v1 `0cb7a8a0:src/import.rs` directly:
check archive suffix and file existence before constructing the member.
Resolution, decoding, streaming-source lookup, numeric-cache admission and
frame lookup all reach this helper. Existing validation and errors remain.
The isolated source test failed first at 687 allocation bytes and now passes
at 147/161 bytes, exactly the corresponding path lengths, including a loose
file inside a directory named `directory.nkx`. Three existing sample tests and
optimized default-feature root `cargo test --profile ci --lib --no-run` pass.

The contention observer also recognizes an idle shell/Python runner above a
normal wrapper waiter, only when every child is already classified as waiting.
Cargo, an unknown worker, a working sibling and observed cgroup CPU/I/O still
count as contention. The real W13 crash wrapper can queue normally without
being mistaken for a quiet-window intruder. Its failing-first fixture passes.

These changes are on `v2/w8-load-attribution-resume`: collector/receipt
`4c0a72a8`, resolver regression `892f2aaf`, v1 port `cf79b2cf`, observer
`f9d69ce6`, numeric allocation evidence `6a1e031c`. No KSP service, UI
publication, authored picture or callback-CPU implementation was changed.
All new run artifacts live under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-load-attribution-resume/`.

The allocation-only comparison was parked after four cells: three frozen-v1
cold cells (all CONTENDED) and current pre-port Conflux (QUIET). The current
Conflux retained CPU-editor harness was 200.29 MiB versus frozen v1 114.61 MiB.
This is a retained-harness comparison, not live CLAP editor-window evidence.
Current-source Analog subsequently measured 372.73 MiB in that same harness;
the frozen-v1 cold observation was 1064.35 MiB. These newer source observations
must not be relabeled as the old frozen326 plugin measurements.

A separate direct v1 worker heap-trim port (`9000c604`) compiled and received
one decisive before/after check. It was rejected and reverted (`f0c00ea2`), so
the final product code is identical to the checked resolver source at
`6a1e031c`. GNU-only `malloc_trim` ran in the completed-load branch off audio.
It reduced pre-editor RSS but gave mixed retained-editor RSS:

| Preset | Pre-editor before/after MiB | Retained editor before/after MiB |
|---|---:|---:|
| Conflux | 115.27 / 112.56 | 198.51 / 204.45 |
| Analog Strings | 351.35 / 332.16 | 372.73 / 361.46 |

Both before cells were QUIET; both after cells were CONTENDED. No timing gain
or regression is established. All six cold/warm candidate cells were finite
and audible, with zero underrun, nonfinite, script-overrun, Lua-fault,
stream-failed and fault-program counters. Analog recorded one silent internal
note in each pass; audibility alone does not certify every authored note.
Both second loads explicitly
reported `sample_header_cache.hit=true`. Receipt: `HEAP-FINAL.json`.
The post-harness `rss_after_trim_mb` field combines dropping the harness and
trimming; it does not isolate freed allocator pages while an editor is live.
`HEAP-FINAL.json` labels `rss_done_mb` as `ready_rss_mb`; that legacy label
means worker completion after at least 375 audition blocks, not publication
readiness. The editor delta compares this pre-editor sample with RSS sampled
inside `src/ui/tests.rs:2311` while the 1180×760 CPU harness is retained.

Conflux's observed retained-editor RSS exceeds the frozen-v1 observation;
the v1 cell was contended, so a paired deficit is not established. The increase
from pre-editor to retained editor is a concrete lead for W3/W13; W8 has not
changed their renderer or publication path. Timing parity and fresh all-14
shared-scanner acceptance remain open.

## Recovered shared-scanner shard, 12:39Z restart

The restored source passed default-feature optimized root no-run again, then
the canonical shared scanner built with `--profile ci --features shots`.
Scanner source is `f0c00ea2`; its final product code equals `6a1e031c`.
Frozen v1 hashes passed before use. The common collector reused owned copies
of the shared numeric note plans; no new corpus collector was introduced.
Both versions loaded and produced audible output on all three matched plans,
with zero underruns and zero load/runtime fault records.

| Preset | v2 load ms | v1 load ms | v2/v1 worker peak MiB | v2/v1 activity |
|---|---:|---:|---:|---|
| Conflux | 443.96 | 269.57 | 196.98 / 70.01 | CONTENDED / CONTENDED |
| Pacific Legato | 232.73 | 76.25 | 107.12 / 53.49 | CONTENDED / CONTENDED |
| Analog Strings | 1442.38 | 4227.41 | 468.07 / 564.78 | QUIET / CONTENDED |

These are numeric scanner observations, not admitted timing comparisons.
Frozen v1 exposes no first-audio field in this older adapter, and v2 uses the
optimized CI profile without ThinLTO while frozen v1 is release. Scanner peak
is not retained-editor RSS. Analog's observed worker peak is lower than v1;
Conflux and Pacific are higher in this run; paired RSS parity remains UNKNOWN.
Source-port causality is established by
the allocation regression, not these cross-version whole-worker figures.

The current scanner reports Original OK for Conflux/Analog. Pacific paints but
reports one missing image (`lookup-not-found`); frozen v1 also reports missing
images on Conflux/Pacific. These rows do not certify full authored fidelity.
W3 received the exact stage-probe invocation and retained-editor memory lead; the remaining
Pacific lookup must be resolved by library membership. Receipt:
`SCAN-FINAL.json`; binary/profile provenance: `SCAN-BUILD.json`.

`RECOVERY-RELEASE.json` confirms the attribution, stage-probe and scanner units
are inactive with MainPID 0, and no quiet request or grant remains. W6 received
the direct completion handoff again after restart. This recovery submitted no
new heavy job. The rejected heap-trim candidate remains reverted.

Observed v2 minus v1 scanner deltas are +174.39 ms / +126.97 MiB for Conflux,
+156.48 ms / +53.63 MiB for Pacific and -2785.03 ms / -96.71 MiB for Analog.
These are unadmitted observations; all paired performance verdicts are UNKNOWN.

Parked parity note:
1. Symptom: current Conflux retained-editor RSS remains about 199 MiB versus
   frozen v1 about 115 MiB; no fresh every-preset timing pass exists.
2. Best hypothesis: the large pre-editor-to-editor increase warrants retained
   renderer/resource allocation attribution; worker heap trimming was mixed.
3. Next step: W3/W13 measure the live retained editor, then W8 reruns the original
   14 cold/warm onset/RSS cells on the combined build in an admitted quiet window.

NEXT: retained-editor allocation attribution and combined-build 14-preset parity.
