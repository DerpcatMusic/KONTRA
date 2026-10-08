# One-pass group envelope schema construction

Targeted correctness and allocation-bound checks PASS. Quiet load-time and
release RSS acceptance remain open; no RSS saving is claimed for this change.

Direct parent: accepted template
`4ef0d62d8f34e3c14352f9bfa09cc3fdc47710ca`, on onset base `52438eb4`.
Built implementation `327cc62b4e2bd6928d56310ac0f3001e5ff3f9e5`;
final receipt amendment changes documentation only.

## Root cause and port

V1 `0cb7a8a0:src/engine/bank.rs`, `Builder::new`, constructs group settings once
with `groups.iter().map(GroupSettings::from).collect()`. Adapt that one-pass
construction to v2's existing typed, shared engine-parameter schema.

The loader formerly installed each amplitude-envelope owner separately. Each
installation cloned the entire controls, engine bindings and envelope lanes,
then sorted and validated the schema. Production calls occurred before
`bind_behaviors`, so the program table was empty: an earlier hypothesis about
repeatedly scanning Analog's compiled KSP operations was incorrect. It is not
used to explain this change.

The loader now collects the same qualifying amplitude owners and installs their
six native lanes in one batch. The existing full control and address validation
runs once. The singular API delegates to a one-element batch; empty batches
return the plan without heap work. IDs, authored defaults, duplicate rejection,
physical group/slot addresses and existing lookups remain intact. Construction
runs on the control/loader thread, and service reads/writes still allocate
nothing on audio.

W5 explicitly cleared the envelope builder overlap. Changes reach core
`engine_parameters.rs`, `lower.rs`, the new envelope fixture and the existing
test allocation counter. No compiler/VM, mutable Cursor, template, stream pool,
head, ring, cache, native-width metadata or host API changes are included.

## Failing-first measurement and tests

For 128 envelopes on an otherwise empty prepared plan, serial schema
construction requests **31,362,432 bytes**. The batch requests **433,664 bytes**:
98.62% fewer, 72.32× less. The same requested-byte fixture fails on the serial
implementation and passes below a 524,288-byte bound on the batch.

These are cumulative requested bytes, not retained heap or process RSS. The
final schema has the same 768 controls and bindings. No per-preset production
allocation reduction is inferred from this synthetic count alone.

- New sizing/identity/service/validation checks: 2 PASS.
- Core targeted callers: lower 22, controls 10, envelope 7, controller stages 5,
  signal trace 8, batch 2; 54 total PASS.
- KSP parameter/audio tests: 13 PASS.
- Root `cargo test --lib --no-run`: PASS.
- Three production-load/editor/audio probes: PASS.

All jobs used `kontakto-heavy`. The initial no-heap service fixture consumed its
address vector inside the measured closure, causing a test-only free; iteration
was corrected to borrow it. The serial sizing failure remains in its receipt.
No full suite or shared gate was run in this worktree.

## Same-code before/after RSS

The before executable is the already-frozen template build `87ee01fc`; its
source differs from accepted parent `4ef0d62d` only in documentation. Thus it
includes the same accepted template and onset code as the batch parent.

MiB, fresh child processes, debug profile, production loader plus retained
editor, `XDG_CACHE_HOME=/dev/null`, `RUST_MIN_STACK=33554432`, core dumps disabled,
`KONTRA_AUDIT_LOAD=1`, no `PROBE_ALLOCS`. Normal heavy work can overlap, so load,
onset and CPU timing numbers are UNKNOWN for release.

| Preset | Starting RSS before → after | Editor RSS before → after | Editor delta | After explicit trim before → after |
|---|---:|---:|---:|---:|
| conflux | 41.40 → 42.20 | 262.97 → 267.56 | +4.59 | 217.84 → 220.72 |
| pacific | 42.14 → 40.61 | 185.64 → 186.74 | +1.09 | 178.73 → 179.38 |
| analog | 41.66 → 43.04 | 1006.96 → 1008.42 | +1.46 | 950.57 → 951.87 |

All three positive editor deltas are disclosed. The reduction in transient
schema requests does not establish an RSS improvement; allocator retention and
resident executable layout can affect RSS. These runs do not identify the cause
of the small increases. The separate pre-load-offset question from the template
receipt remains open; no further attribution or repeats were requested here.

The measured output peaks and first-audio frames match exactly: Conflux
0.1260089576/64, Pacific 0.0555077009/192, Analog 0.7999154925/192. This is a
one-note smoke check, not full playback equivalence. Stream pool is 25,165,824
bytes and eager heads are zero on both sides; latency-adaptive head/resident
stream diagnostics remain in the numeric receipts.

Receipts: `~/.cache/kontakto-fix-load/rss-owners/envelope-batch-{before,after}.log`,
`envelope-batch-{core-fixtures,ksp-params,root-build}.log`,
`52438-{template,envelope}-rss.json`.
Frozen executables and build records are under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-rss-20261008/`.
Before SHA256 `62554d6c228d35d52b583620a9b0d4ce0620483607cd3347162261e745c6b153`;
after `7e7fc0a21aee96fcacb5f6d25628e65cbdf58933b3969483f99b62161615337b`.
Iterative UI blob unchanged: `7faa02c0d36a3826034d72e8bd8df361c1562457`.

NEXT: route the batching commit for shared quiet load-time acceptance; residual
Analog IR vector capacity is a separately noted source-only follow-up.
