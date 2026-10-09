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
