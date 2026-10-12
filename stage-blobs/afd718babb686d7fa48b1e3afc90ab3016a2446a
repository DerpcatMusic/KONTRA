# W13 loaded native host verdict

**FAIL**, candidate `735535a8` (ci; no release/install), frozen plugin 0.3.152; gate audition plans `3f49a9a1`. Native CLAP, 48 kHz, 64 frames, six-second real-time auditions, editor closed. Request removed at 21:29:46 UTC; no host remains.

14 pairs / 28 cells are **MEASURED/QUIET**. V2 wall p50 wins 10/14, p99 wins 8/14, both win 8/14. Stream underruns: v1 **119**, v2 **0**; process deadline misses: v1 **23**, v2 **2346**. Scheduling wake misses are recorded separately.

| Gate index / program | v1 wall p50/p99 µs | v2 wall p50/p99 µs | v2 CPU verdict |
|---|---:|---:|---|
| 0 / 0 | 16.541 / 87.752 | 10.230 / 112.742 | WORSE |
| 1 / 0 | 175.324 / 331.236 | 73.961 / 261.155 | BETTER |
| 3 / 0 | 401.438 / 841.635 | 381.767 / 674.073 | BETTER |
| 4 / 0 | 213.454 / 377.707 | 92.662 / 209.074 | BETTER |
| 5 / 0 | 188.414 / 338.267 | 63.141 / 133.203 | BETTER |
| 6 / 0 | 259.435 / 360.826 | 211.374 / 423.798 | WORSE |
| 7 / 0 | 53.411 / 134.623 | 6.030 / 17.901 | BETTER |
| 8 / 0 | 56.571 / 117.602 | 1632.920 / 2389.214 | WORSE |
| 9 / 0 | 188.684 / 283.295 | 45.411 / 127.992 | BETTER |
| 10 / 0 | 10.120 / 32.841 | 10.890 / 72.201 | WORSE |
| 15 / 0 | 224.294 / 484.779 | 324.036 / 585.721 | WORSE |
| 16 / 0 | 9.940 / 31.601 | 10.770 / 70.771 | WORSE |
| 17 / 1 | 74.741 / 167.703 | 20.990 / 97.632 | BETTER |
| 18 / 0 | 193.324 / 321.406 | 7.580 / 25.031 | BETTER |

Gate index 8 accounts for **2321** v2 deadline misses. Peaks are nearly identical (0.0945477411 / 0.0946570486); route its CPU regression to W2/W9. No attribution rerun is needed for this verdict.

Evidence: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w13-live-host-735535a8/summary.json`; per-cell `metrics.json`, `activity.json[l]`, hashed `plugin-diagnostics.json`, and `bin/BUILD.json` bind plugin/CLI/host hashes and the actual source. Each measured cell retains wall/thread CPU p50/p99, dispatch counts, peak/nonfinite checks, sampler underruns, process/wake deadlines, and process logical/physical stream I/O. V2 perf-view snapshots read the same Shared counters and Watch smoothing; this is a numeric headless model, not a rendered DAW frame. Frozen v1 has no numeric perf-view export: **UNKNOWN**, as approved. OS page-cache state was not controlled; process I/O is not the same as the sampler disk meter.

Open: gate index 2 has no audition plan. Frozen 0.3.152 explicitly failed the first UVI load; this v2 artifact also lacks the UVI feature. Further UVI cells were parked rather than repeating known absence. NKM index 17/program 0 is silent in **both** versions; neither is certified. Loaded VST3, 32/256-frame cells and live GUI frame remain UNKNOWN. These receipts do not certify the newer integration head.

Probe correction: a temporary positional prefix failed because frozen 0.3.152's Selection differs from 0cb7a8a0. Its keyed derive is verified at d097c363 and used successfully. The invalid attempt and a W12 scanner-fixture intrusion are excluded; no measured v2 cell was repeated. Native states and authored logs stayed in tmpfs; no PCM persisted.

Validation: native C++ self-check; Python schedule, invalid/silent/contended receipts, artifact provenance, private first-run prefs and failed-load handling checks. Cargo targeted perf/state tests and `--no-run` passed through kontakto-heavy outside the quiet window. Host additions are separate from editor ownership commit fba565c9.

Resumed native readback follow-up (2026-10-08 22:38 UTC): native `state.load` success alone does not prove the intended selection. The admission regression reproduced red on unverified readback. The host now saves CLAP state on the main thread after audition, outside measured process calls/stream deltas, into private tmpfs. The driver compares native envelope identity and keyed Selection path/program/MIDI/output/gain/aux/part order; default, wrong-source/program/routing/gain, malformed and unavailable states cannot be MEASURED. Additional keyed defaults/order changes are tolerated. This excludes script widget/custom-state recall. Historical 735535a8 receipts lack this new readback proof and remain specific to their original artifact/protocol.

Native C++ compiled through `kontakto-heavy` with `-O2 -std=c++17 -Wall -Wextra -Werror`; its percentile/event/I/O/bounded-save self-check passed. All six Python self-check groups passed, including red→green version-2 private settings at the same `kontra/settings.json` path. The probe accepts hash-bound `ci` and authorized `release` artifacts, records thread-setting/environment policy, and does not build/install a plugin. No new timed cells or production ABI changes are claimed.

NEXT: exact released artifact live-host cells after W0 exits the wrapper, in the W13→W9 quiet order.
