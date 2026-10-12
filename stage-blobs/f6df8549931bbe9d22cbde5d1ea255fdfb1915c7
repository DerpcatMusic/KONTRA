# Runtime allocation owners

Probe: current W8 source, counting global allocator plus glibc backtrace; all loader threads. Rows show retained requested bytes for allocation sites >=4096 bytes, at the end of load/audio before editor construction. This is allocation attribution, not a release timing or RSS comparison. Raw receipts and symbolized owner lists are in ~/.cache/kontakto-fix-load/rss-owners/.

| Owner / allocation call site | Conflux bytes | Pacific bytes |
|---|---:|---:|
| NoteParams[notes] | 68,681,728 | 68,681,728 |
| Arena<Voice> slots | 17,170,432 | 17,707,008 |
| ScriptBank texts clone | 10,243,142 | 0 |
| PerformanceState states | 9,437,760 | 9,437,760 |
| DspState ProcessorState cells | 8,257,536 | 14,192,640 |
| ScriptBank named-text clone | 5,590,620 | 1,527,960 |
| Arena<Note> slots | 4,456,448 | 4,456,448 |
| Arena<Family> slots | 3,670,016 | 3,670,016 |
| VoiceModState envelopes | 3,014,656 | 0 |
| Arena<ExpressionOwner> slots | 2,883,584 | 2,883,584 |

Runtime retained large-allocation total: Conflux 161,931,746; Pacific 144,153,200 bytes. Small allocations and allocator overhead are excluded. Untouched capacity can contribute requested bytes without the same resident RSS.

Conflux script_ui_prepare: compiled instruction storage 41,513,568 retained bytes; immutable Program text literals 1,321,166; Interface IR 417,792. All large retained script-UI sites total 43,328,910 bytes. The lowering emit/grow path cumulatively allocated 118,571,008 bytes, with 1,572,864 peak bytes at that growth call site; these transient reallocations are separate from retained Program instructions.

All root probe tests passed with RUST_MIN_STACK=33554432. The allocator recursion/allocation-free bookkeeping self-check passed separately. One earlier run used the default debug stack and failed during the later editor audit after capture; only the successful repeats are the before receipts. Subsequent probes disable core dumps.

## NoteParams pages

Port from v1 `0cb7a8a0:src/ksp/runtime.rs` (`EVENT_CAPACITY = 4096`). The production loader starts with 4096 note parameters, keeping the existing 16384-note ceiling. Requested payload falls from 68,681,728 to 17,170,432 bytes (49.125 MiB saved). A 128-entry page table preserves note indices; the existing growth worker allocates additional pages at 75% pressure, doubling to the ceiling subject to available memory. Audio adopts only pointers and returns the empty transfer for control-thread destruction. Note admission is bounded by installed pages. Other runtime arenas are unchanged in this commit.

| Preset | Load RSS before / after (MiB) | Editor RSS before / after (MiB) | Trimmed RSS before / after (MiB) | HWM before / after (MiB) |
|---|---:|---:|---:|---:|
| conflux | 300.98 / 252.17 | 378.40 / 331.72 | 342.32 / 294.73 | 377.59 / 330.88 |
| pacific | 263.00 / 213.97 | 286.90 / 148.08 | 279.88 / 142.34 | 287.53 / 236.30 |
| analog | 1099.47 / 1049.71 | 1132.56 / 1082.95 | 1116.65 / 1067.57 | 1133.26 / 1083.45 |

These are debug production-path, editor-open process RSS measurements, not release timing cells. Normal heavy builds overlapped the runs. Allocator layout/reclamation can change editor RSS beyond the exact payload reduction; the high-water and load RSS comparisons also decrease in all three presets. Streaming readiness changes the Analog first-audio frame between runs; this is not an onset acceptance claim.

Reproduction: build through `~/.cache/kontakto-heavy cargo test --lib --no-run`, then run the resulting test binary with `plugin::tests::probe_load --ignored --exact --nocapture --test-threads=1`, `PROBE_PATH` set to each preset, `RUST_MIN_STACK=33554432`, `XDG_CACHE_HOME=/dev/null`, `KONTRA_AUDIT_LOAD=1`, and core dumps disabled. Omit `PROBE_ALLOCS` for RSS; setting it records numeric allocation/free stacks (>=4096 bytes, bounded to 65536 events). Raw logs and numeric JSON receipts: `~/.cache/kontakto-fix-load/rss-owners/note-{before,after}-*`.

Validation: `cargo test -p sampler-core --test growth` (two tests); root lib `--no-run`; `sound::v2::tests::loader_starts_note_parameters_at_v1_event_capacity`; all six preset probes. The growth fixture compares live-note output against a full-capacity runtime and rejects any allocation or free during render/page adoption.
