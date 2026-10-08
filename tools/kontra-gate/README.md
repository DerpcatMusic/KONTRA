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

CPU consumes immutable `~/.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1` and builds the exact-source `cpu_audit` example. The binary SHA256 must be `b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab`; adjacent `BUILD.json` must match that digest, v1 source `0cb7a8a0`, adapter commit `42a0ae9103b1a1cc31a93b3c08e5b86462ccfcad` and equivalence PASS. Its receipt is retained in `cpu.json`. Both CPU workers run with `XDG_CACHE_HOME=/dev/null`, using `PATH BLOCK piano|strings|fx` at 32/64/256 frames. Raw output stays in RAM. Cells remain unknown when the adapter is missing, fails integrity or is inaudible; UVI and multi support is absent from the existing v1 CPU adapter. Gesture invokes W2's exact ignored native Conflux sweep and requires one executed test plus nonzero gesture witnesses; this does not certify the other gate instruments. Family stays unknown: the current scanner exposes MIDI audition picks and zone counts, not selected family/RR identity.

Host consumes the existing CPU audit CLAP host probe and an existing exact-source v2 artifact, never builds or installs a plugin release. Set `KONTRA_GATE_CLAP_INCLUDE` to the SDK include path and `KONTRA_GATE_V2_CLAP_RECEIPT` to a JSON file with `path`, `source_sha` and `sha256`. The source/hash must match the gate. Both installed v1 and supplied v2 process/flush cells must complete; they measure an empty exported plugin, not loaded-library host CPU. Initial baseline manifest/metrics/summary/diff are preserved before adapter extensions.

`restored-state` uses the shared v2 production collector: a seed load captures bound scalar controls in memory, drops the seed, then times a load with those host overrides. No authored values are written to reports. It records actual initializer runs, expected script count, override count and whether the timer excludes the seed. Peak RSS covers the whole worker, including seed and restore. The frozen v1 collectors and UVI do not support this path: they stay UNKNOWN. Older v2 workers without a restore witness also stay UNKNOWN. This condition tests scalar Kontakt host recall, not complete host session serialization.

For a targeted regression measurement, `--items <items.tsv> --conditions restored-state` creates a separately logged diagnostic run with the same collector, hashes and ledger. It does not replace the fixed acceptance set. Resume preserves the original manifest and conditions.

Use `--signal-trace` for a diagnostic run against W6 schema1 (11ab86bd or later). Each isolated worker gets `KONTRA_SIGNAL_TRACE=1` and a fresh report directory. Root and numeric runtime subdirectories retain their exact JSON/SVG; `signal-traces.json` records validity. Stage diffs require complete zero-drop traces, compare frame-weighted RMS/DC and peak maxima, and keep contributions separate from coherent sums. Trace-enabled load/first-audio/RSS scores remain UNKNOWN for performance acceptance.
