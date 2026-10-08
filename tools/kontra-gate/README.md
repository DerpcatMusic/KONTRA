# KONTRA gate and run ledger

Install the entry point once (no dependencies):

```sh
ln -s /home/derpcat/.t3/worktrees/KONTAKTO/fix-load/tools/kontra-gate/gate.py ~/.cache/kontra-gate
~/.cache/kontra-gate <integrate-sha>
```

The entry point calls `kontakto-heavy` separately for the build and for each shared-scanner/probe shard. **Do not wrap the entire command in another heavy call**: holding an outer slot prevents fairness between shards. Scans use the shared driver with a 235-second budget. Resume an interrupted run with `--resume <run-dir>`; the requested SHA must match. Exit 2 means the release gate is FAIL or UNKNOWN, not that the collector failed. Operational exceptions leave a failed manifest and ledger row.

Each run lives in `~/.cache/kontra-runs/<UTC>-<sha>/`: fixed 19-ID quick15-plus-witness manifest, clean exact-source scanner copy and hashes, integrity-checked immutable v1 references, cold and OS-warm shared scanner rows/JSON, v1 fresh/existing-user-cache stage/onset probes, per-item UI audit and redacted plugin diagnostics, `metrics.json`, `summary.md`, `diff.md`. The ledger appends once under a file lock. The optional `--source` selects an existing owned clean checkout at the exact SHA; otherwise an owned detached `gate-<sha>` checkout is created. These are scanner builds with release optimization (`ci`, no cross-crate LTO), **never plugin release builds or installations**.

V1 is never rebuilt or changed. It comes exclusively from `~/.cache/kontra-v1`; every run checks its SHA256SUMS. Both frozen W8 probes are used. No sample, PCM, authored identifier, script or resource content is exported. Manifest paths identify the test inputs; logs retain numbers and hashes, including hashed text/field keys. Raw logs/crash/session evidence stays in private tmpfs and is removed after conversion. Network reports are disabled. Shared scanner render snapshots are not requested.

The scanner exercises production load/audio/Original-render paths in an isolated process, **not a live plugin host**. Its scalar binding check does not certify gestures. Native family parity, CPU percentiles, complete FX/filter/modulator slot disposition, host perf view, settings parity and full-corpus scripting/UVI coverage remain UNKNOWN until their owners supply same-run evidence. UNKNOWN blocks release; no missing value becomes zero. Strictly smaller latency/RSS/CPU passes the beats-v1 comparison; equal zero underruns is valid. OS page cache is uncontrolled; the repeat is labelled `os-warm`, never claimed as a product warm cache. The v1 probes use a read-only sandbox for existing-user-cache measurement.

W6 signal-graph exporter: emit JSON under `KONTRA_REPORT_DIR` with a filename containing `signal-graph`, `signal_graph` or `engine-trace`. Per-item `plugin-diagnostics.json` preserves redacted trace JSON; numeric `level_db`, `rms_db`, `peak_db`, `gain_db`, `gain`, `latency`, `latency_frames`, `bypass` and `bypassed` fields remain readable. Authored text is hashed. `diff.md` compares corresponding per-stage fields; `signal_graph_trace` is a placeholder metric until an exporter is observed. Raw chart text cannot bypass the no-authored-name rule; its content hash is retained.

Checks:

```sh
python3 tools/kontra-gate/check.py
python3 tools/kontra-scan/check.py
```
