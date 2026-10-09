# KONTRA gate and run ledger

Install the entry point once (no dependencies):

```sh
ln -s /home/derpcat/.t3/worktrees/KONTAKTO/fix-load/tools/kontra-gate/gate.py ~/.cache/kontra-gate
~/.cache/kontra-gate <integrate-sha>
# Wait for other heavy units/processes before timed work:
~/.cache/kontra-gate <integrate-sha> --require-quiet
```

The entry point calls `kontakto-heavy` separately for the build and for each shared-scanner/probe shard. **Do not wrap the entire command in another heavy call**: holding an outer slot prevents fairness between shards. Scans use the shared driver with a 235-second budget. Resume an interrupted run with `--resume <run-dir>`; the requested SHA must match. Exit 2 means the release gate is FAIL or UNKNOWN, not that the collector failed. Operational exceptions leave a failed manifest and ledger row.

Timed scanner, onset-probe and CPU/host workers retain `activity.jsonl` and `activity.json` beside their evidence. Samples at worker boundaries and every second record UTC/monotonic timestamps, active/activating user `kontakto-*` and census units, scanner/build/heavy processes (PID and fixed executable kind only), load averages and cumulative disk I/O counters. The current process family and its unit are recorded as owned and excluded from contention. Any external matching activity during a cell marks it **CONTENDED**; paired load/onset/RSS, CPU, underrun and deadline-miss comparisons retain the numbers but count as UNKNOWN for release. Missing or failed activity observations also stay UNKNOWN. Load averages and disk counters provide context; no invented utilization threshold certifies quiet. Periodic sampling may miss activity shorter than one second.

`--require-quiet` waits outside heavy slots. Every timed worker checks again after admission; a race releases the slot and retries after quiet, and contention beginning during a worker still invalidates its result. Quiet mode does not stop or pause other units; they must become inactive. Cached unobserved/contended cells are remeasured in quiet mode. Gate scanner cache identities include the activity protocol, leaving historical records intact. The new `harness-contention-v1` snapshot preserves earlier harness copies; the manifest records the observer hash and quiet requirement. This instrumentation does not turn historical measurements into quiet evidence.

Each run lives in `~/.cache/kontra-runs/<UTC>-<sha>/`: fixed 22-ID quick15-plus-witness manifest (including Coline MW, Diamond Crackling and Antartide bounded UVI loads), clean exact-source scanner copy and hashes, integrity-checked immutable v1 references, cold, product-warm and OS-warm shared scanner rows/JSON, v1 fresh/existing-user-cache stage/onset probes, per-item UI audit and redacted plugin diagnostics, `metrics.json`, `summary.md`, `diff.md`. The ledger appends once under a file lock. The optional `--source` selects an existing owned clean checkout at the exact SHA; otherwise an owned detached `gate-<sha>` checkout is created. These are scanner builds with release optimization (`ci`, no cross-crate LTO), **never plugin release builds or installations**.

V1 is never rebuilt or changed. Scanner, plugin and onset probes come from `~/.cache/kontra-v1`; every run checks its SHA256SUMS. The separately frozen W9 CPU adapter is verified as described below. Both frozen W8 probes are used. No sample, PCM, authored identifier, script or resource content is exported. Manifest paths identify the test inputs; logs retain numbers and hashes, including hashed text/field keys. Raw logs/crash/session evidence stays in private tmpfs and is removed after conversion. Network reports are disabled. Shared scanner render snapshots are not requested.

The scanner exercises production load/audio/Original-render paths in an isolated process, **not a live plugin host**. Its scalar binding check does not certify gestures. Native family parity, CPU percentiles, complete FX/filter/modulator slot disposition, host perf view, settings parity and full-corpus scripting/UVI coverage remain UNKNOWN until their owners supply same-run evidence. UNKNOWN blocks release; no missing value becomes zero. Strictly smaller latency/RSS/CPU passes the beats-v1 comparison; equal zero underruns/nonfinite counts are valid. New runs use a fresh empty writable private tmpfs product cache for each engine/item, then retain it for the `product-warm` second load. `os-warm` uses another empty product cache. OS page cache is uncontrolled. Per-worker before/after cache file and byte counts prove the condition; no cache payload is exported. Resume re-primes a lost RAM cache through the same shared collector. The frozen v1 scanner disables caches under KONTRA_SCAN_ACTIVE, so gate cache-enabled workers unset that variable; the affected v1 script-phase observer fields are explicitly unknown. Frozen stage probes bind only their private RAM cache writable. Legacy baseline measurements retain their original disabled-cache semantics.

W6 signal-graph exporter: emit `signal-trace.json` and `signal-trace.svg` under `KONTRA_REPORT_DIR`; alternate `signal-graph`, `signal_graph` or `engine-trace` JSON filenames are also recognized as redacted evidence. Per-item `plugin-diagnostics.json` preserves redacted trace JSON; numeric `level_db`, `rms_db`, `peak_db`, `gain_db`, `gain`, `latency`, `latency_frames`, `bypass` and `bypassed` fields remain readable. Authored text is hashed. `diff.md` compares corresponding per-stage fields; `signal_graph_trace` is a placeholder metric until an exporter is observed. The exact W6 numeric/fixed prepared graph JSON and SVG are retained per item (its exporter guarantees no PCM, authored names or text); other raw log/chart text remains hashed. Exporter flags pass through the gate environment unchanged. Final flag/schema/SHA from W6 is pending.

Checks:

```sh
python3 tools/kontra-gate/check.py
python3 tools/kontra-gate/check-contention.py
python3 tools/kontra-scan/check.py
```

Owner adapters run after scanner/probes when present. They may also be run without repeating the scanner:

```sh
~/.cache/kontra-gate <sha> --resume <run-dir> --source <owned-clean-exact-sha-checkout> --adapter cpu
# --adapter gestures, host or all
```

CPU consumes immutable `~/.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1` and builds the exact-source `cpu_audit` example. The binary SHA256 must be `b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab`; adjacent `BUILD.json` must match that digest, v1 source `0cb7a8a0`, adapter commit `42a0ae9103b1a1cc31a93b3c08e5b86462ccfcad` and equivalence PASS. Its receipt is retained in `cpu.json`. Both CPU workers run with `XDG_CACHE_HOME=/dev/null`, using `PATH BLOCK piano|strings|fx` at 32/64/256 frames. Raw output stays in RAM. Cells remain unknown when the adapter is missing, fails integrity or is inaudible; UVI and multi support is absent from the existing v1 CPU adapter. Gesture coverage is measured per item and condition by the Original editor probe below. Family stays unknown: the current scanner exposes MIDI audition picks and zone counts, not selected family/RR identity.

Host consumes the existing CPU audit CLAP host probe and an existing exact-source v2 artifact, never builds or installs a plugin release. Set `KONTRA_GATE_CLAP_INCLUDE` to the SDK include path and `KONTRA_GATE_V2_CLAP_RECEIPT` to a JSON file with `path`, `source_sha` and `sha256`. The source/hash must match the gate. Both installed v1 and supplied v2 process/flush cells must complete; they measure an empty exported plugin, not loaded-library host CPU. Initial baseline manifest/metrics/summary/diff are preserved before adapter extensions.

W12 fidelity contract for integration: `fidelity.slot_verdict` reads each dropped
slot object's enabled/bypassed counts and fails either nonzero count. These are
absolute native-parity verdicts, not comparisons against frozen v1's absent
observer columns. Map scanner `family_match` MATCH→PASS, MISMATCH→FAIL,
UNKNOWN→UNKNOWN, preserve typed `family_match_reason` and
`family_script_driven_count`. Include these four measured verdicts in DSP parity.
The full authored rows remain in each program's `dsp_slots` receipt. The independent
Tier 1 evaluator is `tools/kontra-scan/native_family.py`; native captures for
SCRIPT_DRIVEN cells can be compared by `fidelity.family_match` (minimum 32 takes).
No host launching is required or authorized by these tools.

Widget receipts now address each item and cache condition independently. The ignored
`ui::widget_gate::original_widget_gestures` test loads through the production plugin
loader, forces Original, drives pointer/keyboard input through the full editor, reads
engine/Lua values, serializes the host Part in RAM, and reloads it. The adapter runs
all programs named by the same-run scanner receipt, one heavy invocation per program.
Missing, crashed, timed-out, empty or partially enumerated probes cannot pass. A legacy
Conflux Vector/readback witness cannot certify another cell.

Presented-window coverage is separate from the headless editor. `presented-host.cpp`
opens the exported CLAP GUI in a mapped X11 parent, processes audio on a separate
thread, waits for the loaded-part counter, and reads the visible window with
`XGetImage`. Its binary RGB stdout must stay in a pipe/RAM, never a disk file.
On Hyprland/Xwayland, those pixels can be black despite a mapped window; the driver
uses `grim` on the compositor bounds of the probe-owned window as its pixel witness.
It refuses a window outside the host process family. Pillow/NumPy, `grim` and
`hyprctl` are required only for this live capture, not the numeric gate collector.
Compile using the official CLAP headers with `-lX11 -lXtst -ldl -pthread`, through
`kontakto-heavy`. `presented.py PLUGIN STATE_AUTHOR_CLI HOST ITEM OUT --source-sha
FULL_SHA --ram-dir PRIVATE_DEV_SHM_DIR` separately submits the native state export
and GUI host through the wrapper; do not wrap the Python driver. It retains before/
last PNGs and state only in the caller's private `/dev/shm` directory for the render
owner. Remove that directory after handoff. It never builds or installs a plugin.
The host forwards actual ConfigureNotify sizes through CLAP GUI `set_size`, accepts
bounded GUI resize requests, and arms the opt-in native timing capture with a safe
two-pixel primary drag on the resize grip. Idle captures intentionally have no
native timing record. Timing reports arrive at capture finish/close, not per frame.

The numeric receipt detects large pure-black rectangles, translated copies of a
varied chrome template, and repeated edge appearance/disappearance over N frames.
Only selected static chrome tiles are asserted; authored animated tiles remain
findings. Black artwork needs a fresh reference before being called an occlusion.
Screenshot intervals are capture times, not display FPS. Existing opt-in native
callback timing is collected separately; shared-device GPU counters do not prove
GPU completion or attribute memory to the plugin. Contended timings stay UNKNOWN.
Retain numeric `metrics.json` under `RUN/presented/...`, then run `--adapter
presented` to incorporate exact-source, item/condition-bound findings. Missing,
partial and mismatched receipts remain UNKNOWN; detected artifacts fail the UI
axis. A clean static capture does not certify every interaction. Checks:
`python3 tools/kontra-gate/check-presented.py`.

`gestures/<condition>/<item-sha256>/metrics.json` contains passed/total counts,
per-program observations and typed reasons: parameter-unchanged, navigation-only,
occluded-or-outside-viewport, save-reload-mismatch, save-reload-load-failed,
script-or-render-fault, probe-budget,
probe-timeout, probe-crash-or-no-receipt, invalid-receipt. Target identity is hashed;
resources, parameter values and serialized host state are never exported. Native
menus use real popup item clicks. Passive meters/panels/images and disabled controls
have no edit obligation. Navigation-only targets remain explicit failures until their
view-state obligation has its own witness. These are headless production editor
receipts; they do not certify OS/DAW capture, IME or native Kontakt calibration.

`live_host.py GATE V2_CLAP V2_CLI LIVE_HOST OUTPUT` measures loaded libraries through `vendor/moose-clap/tests/live_performance.cpp`. Build that native host outside the quiet window, then run the driver through `kontakto-heavy` as the quiet owner after other builds finish. Frozen v1 is run without rebuilding. The supplied plugin's adjacent `BUILD.json` must identify its full source SHA, `ci` or `release` profile, path, and plugin/CLI/host hashes; alpha release artifacts are now authorized through W0.

The native host takes `PLUGIN STATE BLOCK SECONDS READY_FLAG EVENT_TSV EXPECTED_PARTS READBACK_STATE`. It saves CLAP state on the main thread after audition, outside CPU/streaming measurements. The driver verifies saved native Selection source/program, MIDI/output/gain/aux and part order against the authored state. Raw readback stays in private tmpfs; receipts retain only its hash and verdict. Missing/mismatched readback, silence, incomplete events or contention means UNKNOWN. This identity/routing proof does not certify scripted widget/custom-state recall or VST3. `check-live-host.py` and the native host's `--self-check` cover receipt admission and bounded streams.
