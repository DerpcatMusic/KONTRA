# Compact immutable region playback templates

Status: **READY for integration**, accepted by the coordinator after reviewing
the pre-load offset and the per-region payload reduction. Targeted correctness
PASS; Analog improves substantially. No quiet timing or release claim.

Base: `52438eb49b3d9daa45e88eeead48f610c9dfaa95`.
Built implementation: `87ee01fc87261ccf0fcf422ebe81762b7cd106b3` (direct parent is
that base). The final receipt amendment changes documentation only.

## Change and ownership

Port the compact immutable playback geometry from
`v1 0cb7a8a0:src/engine/map.rs` (`PlayMap`), adapting to v2's validated geometry,
finite counts, tuning and eight physical loop slots. `PreparedRegion` stores a
104-byte `CursorTemplate` instead of a 640-byte mutable `Cursor`. Ordinary and
normalized single-loop regions need no side entry. Each region that requires
full multi-loop state has one 512-byte entry in a sparse, plan-owned boxed table.

Activation copies the existing full mutable cursor by value, including its loop
state. It starts with `cold_hold: false`, as on the accepted onset base. It never
allocates or shares mutable release state. Mutable cursor layout, methods and
stream traversal remain intact. Source validation and release-loop validation
still use the existing cursor behavior.

The narrow caller changes are in `prepare.rs` and `prepare/selection.rs`.
W9 cleared the ownership overlap; their ring admission and traversal helpers
remain separate. There are no compiler, VM, stream pool, head, ring or reader
cache changes in this trial.

## Bytes

The new prepared region is 400 bytes; the former region is 936 bytes, consistent
with the earlier 89,504,064-byte Analog owner and 95,624 regions. The immutable
payload saving is `regions × 536 − multi_loop_regions × 512` bytes, excluding
the one added boxed-slice handle and temporary vector growth.

| Preset | Regions | Full multi-loop entries | Immutable payload saved, bytes |
|---|---:|---:|---:|
| conflux | 1985 | 1 | 1,063,448 |
| pacific | 6136 | 346 | 3,111,744 |
| analog | 95624 | 0 | 51,254,464 |

Requested bytes are not RSS: allocator retention and resident executable pages
also contribute. Even a full multi-loop region saves 24 bytes of final payload.

## Same-base RSS

MiB, fresh child processes, production loader + editor audit, same debug
profile, `XDG_CACHE_HOME=/dev/null`, `RUST_MIN_STACK=33554432`, core dumps disabled.
`PROBE_ALLOCS` is unset. Run through the normal heavy wrapper after W0's quiet
request was removed. Other heavy jobs can run; timings are UNKNOWN for release.

| Preset | Starting RSS before → after | Editor RSS before → after | Editor delta | After explicit trim before → after |
|---|---:|---:|---:|---:|
| conflux | 37.27 → 41.40 | 261.39 → 262.97 | +1.59 | 219.47 → 217.84 |
| pacific | 37.06 → 42.14 | 182.46 → 185.64 | +3.18 | 175.81 → 178.73 |
| analog | 37.08 → 41.66 | 1092.77 → 1006.96 | -85.82 | 993.86 → 950.57 |

One reverse-order repeat was retained for the unresolved small-preset increases:

| Preset | Starting RSS before → after | Editor RSS before → after | Editor delta |
|---|---:|---:|---:|
| conflux | 34.78 → 41.30 | 259.88 → 265.64 | +5.76 |
| pacific | 35.79 → 41.23 | 181.45 → 185.21 | +3.76 |

The candidate starts 4–6.5 MiB higher before `Load`. ELF `.text` + `.rodata` grow
only 4,544 bytes; `.data` and `.bss` sizes are unchanged. These measurements do
not identify the owner of the starting-RSS difference and do not justify
subtracting it from the release metric. Both positive editor deltas remain
recorded. The coordinator accepted the template reduction and left the process
RSS difference before Load as a separate open question (possible allocator
arena or thread-stack placement; neither is established). No further repeats
are needed now. The shared release gate still owns per-preset acceptance.

All three measured output peaks and first-audio frame indices are identical
before/after: Conflux 0.1260089576/64, Pacific 0.0555077009/192,
Analog 0.7999154925/192. This is a one-note smoke check, not a full equivalence
gate. Stream pool is 25,165,824 bytes and eager heads are zero on both sides.
Latency-adaptive head-frame diagnostics and small resident stream differences
are retained in the raw receipts; no template saving is credited to W9's trial.

## Targeted verification and receipts

- Round-trip cursor matrix: 1 PASS, both directions, 0–8 slots, finite and
  release-ended loops, offsets, tuning and independent release state.
- Source 10, release 4, release selection 18, plan transfer 6, prepared size 1:
  all PASS. Two-slot and all-eight-slot onset/render fixtures assert no heap work.
- Root `cargo test --lib --no-run`: PASS. All Cargo calls used `kontakto-heavy`.
- Initial fixture compilation failed because the new no-heap trigger closure
  returned `NoteId`; corrected to return `()`, then the targeted set passed.

Numeric receipts and build logs:
`~/.cache/kontakto-fix-load/rss-owners/52438-{before,template}*-rss.json`,
`52438-template-{unit-final,fixtures-final,build}.log`.
Frozen executables and `BUILD.json` files:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-rss-20261008/52438-{before,template}-*`.
Baseline SHA256 `5613933b008f9f9f9ec16f26bc9debc7598257e48bfdc739968ffe81e76ec27e`;
candidate `62554d6c228d35d52b583620a9b0d4ce0620483607cd3347162261e745c6b153`.
Both use iterative UI blob `7faa02c0d36a3826034d72e8bd8df361c1562457`.

NEXT: batch control-thread envelope-schema construction; W5 cleared the builder
ownership overlap. Compiler/VM and streaming acceptance remain their owners'.
