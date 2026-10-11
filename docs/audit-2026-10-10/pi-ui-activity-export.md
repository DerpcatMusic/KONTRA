# Pi takeover: versioned UI activity host readback

Status: source checkpoint, not runtime-verified or performance-accepted.

## Change

Adds a separate CLAP diagnostic symbol:

```cpp
bool __kontra_clap_ui_activity(const clap_plugin_t* plugin,
                              uint64_t version, uint64_t* out, size_t count);
```

Version 1 requires exactly 20 aligned, writable `uint64_t` elements. The order is
`plugin::ui_activity::FIELDS`. The instance must be live and created by the same
library. Call on the host main thread before destruction. Null pointers,
unsupported versions, wrong lengths and disabled instrumentation return false
without changing output. Instrumentation is enabled by
`KONTRA_NATIVE_UI_TIMING=1` at instance construction.

The implementation reads only fixed per-instance atomic counters. It does not
walk the rack/IR, call Lua, collect the heap, retain native assets or take a
lock. Cost is O(20), with a 160-byte result and no heap allocation. Counter
loads are independent Relaxed reads, not one coherent readiness snapshot.

The existing `__kontra_clap_perf` signature/layout/implementation is unchanged.
The presented host preserves the legacy four-phase path. Its opt-in 11-phase
cycle path requires the new export and writes numeric version/count arrays at
each of the existing ten samples. The summarizer rejects absent, wrong-version,
wrong-length, non-u64 and decreasing counters, then reports named per-phase
deltas. It does not derive an authored-renderer-ready verdict.

## Dependencies and limits

- Counter producers: original W1 source `120b1883`; clean prerequisite cherry
  on this branch is `a052695a`. Do not cherry-pick both equivalents.
- Cycle lifecycle/input prerequisite: W13 source `44952fae` and its ancestry.
- W1 idle-counter assertion `4598e486` remains a separate source assertion;
  this change does not replace it.
- W3 Native59 ownership/readiness/presentation contract is still a proposal.
  UI20 is not a substitute for it. A caller can read all-zero UI20 counts from
  an enabled instance before any editor opens; that proves no asset readiness.
- No cycle runner is added to the legacy `observe()` command. Real host
  admission still needs the renderer producers, generation-correlated
  presentation, a gate driver and exact frozen artifact provenance.
- Disabled export and missing export must never be counted as ready.

## Required verification by the integration owner

No cargo/rustc/build/test/clippy, host, capture or performance run was launched
by this implementation worker. `git diff --check` and Python AST parsing passed.

Run once in the integration owner's serialized validation slot:

1. Root tests filtered to `plugin::host_probe::tests::ui_activity_export` with
   CLAP enabled; also `plugin::ui_activity::tests`.
2. W1 `4598e486` idle scalar/readback assertion, after its UI prerequisites.
3. `python3 tools/kontra-gate/check-editor-cycles.py` and the existing legacy RSS
   self-check. The new missing/invalid/reset cases were accepted by the old
   summarizer; the new summarizer must reject them.
4. Compile the presented-host with the existing CLAP/X11 build recipe and run
   its input transport fixture. No package installation is needed.
5. In an admitted live fixture, disable opt-in and confirm cycle mode refuses
   it; enable opt-in and confirm 110 valid UI20 observations and named deltas.
   Keep expensive pixel/native-state readback before phase gates.

Actual Conflux CPU/RSS acceptance, close/reopen ownership, GPU completion and
v1 performance parity remain unverified.
