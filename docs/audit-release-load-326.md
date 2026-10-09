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

## 2026-10-09 measured receipt

All six cells are QUIET, audible, have verified native selection readback and zero
process swap. Frozen v1 checksum verification and every 326 artifact digest
passed. Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-load-030326-608a20a1/`.
No product build or install was performed.

| Preset | v1 ready ms | 326 ready ms | v1 first audio ms | 326 first audio ms | v1/326 ready RSS MiB | v1/326 after-audio RSS MiB |
|---|---:|---:|---:|---:|---:|---:|
| Conflux | 186.84 | 442.08 | 285.99 | 543.14 | 398.88 / 138.54 | 416.91 / 138.75 |
| Pacific 10 Cellos – Legato Sustains | 78.67 | 1238.61 | 179.30 | 1344.45 | 341.75 / 133.40 | 460.27 / 134.78 |
| ANALOG STRINGS | 3438.75 | 2750.16 | 3539.10 | 2856.68 | 581.91 / 836.40 | 1222.75 / 846.52 |

This single trial is an exported-CLAP, editor-closed observation with the fixed
100 ms post-ready warmup included. It establishes neither editor-open RSS nor
native sound parity. V1 reported 1/1/2 underruns for Conflux/Pacific/Analog;
326 reported zero. The readiness and memory axes are independent of those
underruns. V1 plugin initialization already held about 249–251 MiB before state
load; 326 about 21 MiB. Full process RSS is reported, not sample counters.

The first attempt retained two completed Conflux cells and stopped on an audit
bug: `native.state` was reused while the state exporter requires create-new.
Each preset now uses a unique state filename. A provenance-checked `--resume`
retained those two observations and ran only the four remaining cells. The
original and resumed driver digests are recorded. A mock exporter enforcing
create-new proves unique filenames and no repeated auditions during resume.
An unrelated KURV build delayed the resumed pregrant; it ended before those
four cells began. No measurement overlapped it; all six activity timelines are
QUIET. Both owned quiet requests/grants were removed, and the final unit has
MainPID 0. The next quiet owner is W5, then W6 and W13.

NEXT: exact shipping 0.3.344 Kontakt load/audio sweep after timed owners clear.
