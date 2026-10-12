# Editor poison recovery and original panic evidence

The 0.3.344 Linux support bundle records 1,784 `PoisonError` window warnings after an Amati Viola partial load. The first panic is absent. Recovering the poisoned outer MUI model alone was insufficient: plugin rack, selection, control, Native bridge and resource cache locks still unwrapped poison on every later frame.

This change reuses the existing support lock helper across those shared states, recovers their contents and clears poison without rebuilding or resetting the rack. Nonblocking audio publication keeps `WouldBlock` and performs no allocation when recovering poison; its recovery notification is deferred to an ordinary off-audio lock. Recovery is diagnosed separately from the original fault.

A process panic observer records the first Rust panic per thread, with bounded/redacted message, source location and thread identity, before unwind poisons a held lock. It chains the previous hook. Diagnostics use only try-locks: if their own lock is held, stderr remains the evidence fallback. The plugin module is pinned before registering a host-retained callback. On Linux, main-executable detection checks loaded address ranges rather than trusting argv or the working directory; this also addresses the observed standalone module-pinning rejection seam.

The native window guard suppresses identical consecutive fault messages, keeps existing state and invalidates the scene and presentation caches. The next successful callback can rebuild and paint without reopening the editor. It does not certify that a panic left partially mutated state logically consistent.

## Focused evidence

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-editor-poison-344/`. `EVIDENCE.json` records exact source, commands and results.

The test-only baseline `a1f9d1df` fails both synthetic plugin poison regressions with `PoisonError`. The candidate passes both: retained rack/selection/program data survives 64 frame accesses; control publication resumes with zero measured heap allocations. An isolated child-process test verifies the original panic record precedes poisoning, the previous hook remains chained, and duplicate first-panic/recovery records are suppressed. Separate tests cover held diagnostic locks, Linux loaded-module identity and native window recovery.

The frozen v1 source `0cb7a8a0:src/plugin.rs` also unwraps the rack/view shared locks. It provides no safer poison policy to port.

## Unresolved original Amati trigger

Symptom: Amati Viola was the last partial load before the poison storm; the poisoning panic was not logged.
Best hypothesis: an earlier unwind poisoned shared state, but no frame or panic payload identifies its origin; no Amati preset is installed locally.
Next step: use the new first-panic record from a tester reproduction to identify and repair the original fault.

## Unresolved early native exits

Symptom: the triage table contains six sub-0.5-second exits after window-ready, all on 0.3.148/0.3.199; the sole 0.3.344 unclean-exit report ends after a directory scan.
Best hypothesis: native window/GPU initialization or user termination; all nine reports lack panic, signal and stack evidence.
Next step: obtain stderr/native crash evidence from a reproduction; the separate `e2c6c47c` GPU-overflow/first-present fix operates after GPU initialization and does not establish repair of these exits.

No native signal/exception handler, Kontakt/Falcon launch, install, host DAW test or full integration gate was added or run for this change.
