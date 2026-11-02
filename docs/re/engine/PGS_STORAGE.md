# Prepared PGS storage

PGS belongs to the engine and is shared across script slots. Initialization
creates keys while `Engine::new` runs off audio. Integer arrays occupy their
actual declared length (clamped to the existing 1–256 rule); a duplicate keeps
its original length and contents. Initialization admits at most 8,192 keys of
each kind. Above that limit the create action is counted in
`unhandled_actions`, rather than silently reporting support.

After all slot initialization and persistence callbacks, the engine reserves
64 additional integer keys, 64 additional string keys and 16,384 integer
cells. These are explicit per-instrument limits, not dynamically expanding
capacities. Each successful callback creation uses this backing; duplicates do
not consume it. A request beyond a limit leaves state unchanged and increments
the corresponding unhandled action count. `Engine::pgs_budget()` exposes the
limits and actual reserved vector bytes, excluding allocator overhead. Reset
and in-place snapshot recall retain PGS state as before. This admission policy
can refuse callback-created keys even where Kontakt would accept them; callers
must inspect diagnostics when assessing compatibility.

A lookup or creation scans at most 8,256 keys of its kind, with fixed-size key
comparisons of at most 320 bytes. A successful integer creation additionally
zeros at most 256 cells. The action does not perform I/O or take a lock.
This is a bounded work statement, not a deadline guarantee. Reserved virtual
memory is not locked into physical RAM.

## Representative measurement

Captured at `26699a1385` before pre-touching and the equivalent admission-limit refactor.
Rerun the command to measure another revision.

AMD Ryzen 7 7800X3D, Linux x86-64, rustc 1.99.0 (b940084d7
2026-09-28), Cargo release profile, 48 kHz engine; command:

```text
build-slot cargo test -p kontra-host --test script --release --all-features pgs_callback_timing -- --ignored --nocapture
```

A synthetic instrument initialized 420 integer keys of four cells and 420
string keys. Its UI callback creates one additional key of each type (the
first run), then sets the last initialized integer and string. Backing was
578,112 bytes; limits were 484 keys per kind and 18,064 integer cells. The
first callback took 41,031 ns; 10,000 subsequent duplicate-create/edit
callbacks measured p50 13,251 ns, p95 24,421 ns, p99 26,910 ns and maximum
83,531 ns. The 64-frame/48 kHz deadline is 1.333 ms. This is an unpinned,
unisolated wall-clock microbenchmark, including VM and UI dispatch, and is not
a worst-case bound for an 8,192-key instrument or a full audio callback.

The 64-key loss was identified by Thug-Dracula in
[contributor PR #1274](https://github.com/DerpcatMusic/KONTRA/pull/1274)
at `aef4fcb256a4dfa6941c191713cc580ac8e2d11c`. This implementation was written
independently; the contributor's unbounded callback allocation is not copied.
MIDI-file service and editing storage remain separate work in #1281 and #1232.
