# Release gate baseline and method

Harness: `tools/kontra-gate/gate.py`, installed entry point `~/.cache/kontra-gate`. Shared-scanner collection only. Source baseline is **16eefff1**, before W8 load and W5/W2/W3 integrations; this is a historical baseline, not evidence against the merged fixes. No release or installation occurred.

Completed run: `/home/derpcat/.cache/kontra-runs/20261008T103425.810199Z-16eefff1d023/summary.md`. The initial summary/manifest/metrics/diff are preserved under `initial-*` before optional adapter extensions. Ledger: `~/.cache/kontra-runs/ledger.tsv`.

76 scanner rows: 19 IDs × v1/v2 × product-cache-disabled cold/OS-repeat. 38 frozen v1 stage/onset probe records. All frozen v1 integrity hashes passed. Per pass v1 admitted 18/19, Original OK 12/19, audible 17/19; v2 admitted 19/19, Original OK 9/19, audible 18/19. Verdict: UI FAIL, DSP UNKNOWN, UVI FAIL, scripting UNKNOWN, beats-v1 FAIL. Unknown blocks release; scalar readback does not certify gestures or native audio families.

Conflux load/RSS: disabled-cache v1 142.26 ms / 69.97 MiB vs v2 17,707.22 ms / 229.20 MiB; OS-repeat v1 141.33 / 69.84 vs v2 4,949.73 / 229.38. OS cache and shared-machine contention are uncontrolled. The frozen v1 scanner explicitly disables parsed-cache lookup under `KONTRA_SCAN_ACTIVE` (`tools/kontra-scan/v1-instrumentation.patch:27`), so the 142 ms figure cannot demonstrate existing parsed-cache reuse. Frozen stage first-audio measurements also varied from an earlier 3,287 ms to 109.77 ms fresh in this run; do not infer a cache hit from timing alone.

RSS uses the identical shared driver in both: isolated worker main-process VmHWM/VmRSS sampled every 100 ms from spawn through exit, combined with the worker-reported peak. All worker threads, loader, UI assets and audio are included. External reader child processes, host/DAW and GPU memory are excluded. A terminal allocation after the last sample/self-report could be missed; this is not a process-tree sum. Earlier audits used different retained/settled lifecycle measurements; those cannot be substituted for whole-worker peaks.

Largest baseline cold RSS deltas among paired admitted IDs:

| Item | v1 MiB | v2 MiB | Delta MiB |
| --- | ---: | ---: | ---: |
| Analog Strings | 563.14 | 1248.45 | +685.31 |
| Areia Full Ensemble | 309.12 | 785.72 | +476.60 |
| Areia 16 Violins | 266.54 | 613.04 | +346.50 |
| Chorus Women | 179.49 | 415.47 | +235.98 |
| Big Screen multi | 80.15 | 280.54 | +200.39 |
| Afflatus 2 Horns KS | 78.22 | 273.11 | +194.89 |
| Conflux | 69.97 | 229.20 | +159.23 |

Next-run corrections: `cold` starts with an empty writable private tmpfs XDG product cache per engine/item; `product-warm` loads again with that cache retained; `os-warm` starts another empty product cache after prior passes. Cache file/byte counts before/after are recorded, content is never retained, and interrupted runs re-prime RAM caches through the shared driver. Frozen v1 cache-enabled workers unset `KONTRA_SCAN_ACTIVE` (no rebuild), which disables specific script-phase observer hooks; those columns are unknown, never zero. Frozen v1 stage/onset probes share fresh/retained writable RAM cache paths. Ties at zero underruns/nonfinite counts pass; ties above optimum fail.

Owner adapters: CPU example + frozen v1 CPU adapter; W2 native Conflux gesture sweep; exported empty CLAP process/flush probe with exact-source v2 artifact receipt. Missing/inaudible/zero-test witnesses remain unknown. Current scanner has audition notes/zone counts, no selected-family witness, so family stays unknown pending shared-scanner extension. W6 `signal-trace.json/svg` are retained per item; per-stage peak/RMS/DC/gain/latency/enabled diffs are supported. Final trace flag/schema/SHA pending.

Completed corrected integration baseline: `/home/derpcat/.cache/kontra-runs/20261008T105451.116229Z-0f62e503e8b7/summary.md`, exact source `0f62e503` (W5/W7, W1/W2/W3 merged; W8 load changes absent). All 114 scanner rows have finished: per condition v1 Original 12/19 and audible 17/19, v2 Original 15/19 and audible 18/19. 38 frozen stage/onset probes completed; the exact native Conflux gesture sweep passes. CPU and CLAP adapters remain UNKNOWN because the frozen CPU adapter/source example and exact-source artifact receipt are absent. Verdict: UI FAIL, DSP UNKNOWN, UVI FAIL, scripting FAIL, beats-v1 FAIL.

Conflux cold/product-warm/OS-warm loads: v1 264.34/136.81/2091.45 ms, v2 5454.23/5223.26/5198.17 ms. Corresponding whole-worker RSS: v1 69.43/69.64/70.95 MiB, v2 339.39/338.93/338.31 MiB. The v1 cold product cache starts with zero files/bytes, then contains two files and 3,489,817 bytes; product-warm reads that retained cache. V2 writes no cache in any condition. OS page-cache warmth and shared-machine contention are uncontrolled; these are observed values, not isolated causal estimates.

Restored-state follow-up: pass host overrides into NKI/program/snapshot translation before initialization, and reuse initialized state only when prepare receives identical overrides. A counted regression verifies one actual initializer call and menu item values 4/-3/0 reaching persistence_changed; callers supplying different values after translation still reinitialize. The gate adds a genuine in-memory scalar host-recall reload with actual/expected initializer counts. Missing restore witnesses yield UNKNOWN timings. Frozen v1 and UVI lack this adapter, so restored-state A/B remains UNKNOWN; whole-worker RSS for this condition includes the seed and restored load.
