# Family/expression ownership and segmented rendering

2026-10-05. This clean-sheet implementation extends the experiment at `95faf11`.
It closes a runnable portion of V2-03/04/12/19, not their full product gates.
The core remains dependency-free and forbids unsafe code. It builds/tests without
the old application or VM. Historical KSP integration tests consume it only as a
development dependency and are not the new scripting implementation.

## Executable contracts

The [ownership module](../../crates/sampler-core/src/ownership.rs) adds separate
runtime/slot/generation identities for families and expression owners. A logical
note owns zero or more families; each family owns admitted source voices. An open
family accepts layers. Sealing stops admission and retains existing/delayed voices;
the last source completion retires a sealed family. Stopping a voice affects only
that voice; stopping a family cancels all its sources and queued starts without
releasing the note or sibling families. Note release revokes its families.

Expression sharing is explicit at child creation: linked, snapshot, or independent,
separately from release linkage. Roots always receive distinct owners. Detachment
copies a shared expression transactionally, or fails without changing the link.
It detaches the selected note only, not every descendant. Host/channel reuse does
not select an old expression owner. Adapters must retain the correct per-note handle;
this slice does not yet implement their routing policy.

Notes, families, voices, expressions and scheduled commands have separate prepared
limits. Failed admission rolls back newly acquired ownership. Continuation pins,
child provenance and pending terminal acceptance still prevent note recycling.
Expression storage retires with its last note. Generations never wrap; exhausted
slots are quarantined. Explicit family creation is required for coordinated layers;
`start(note, ...)` is the convenience operation for a single-source family.

`Input` now preserves group alongside port/channel/original key. Pressure and timbre
retain exact `u32` values; canonical gain, pan and pitch use `f64`. Only gain and
stereo balance currently affect PCM. Pitch, pressure and timbre storage is **not**
a completed modulation engine. Balance has center unity and no smoothing in this
fixture; production panning and modulation policies still need implementation.

Protocol reference: MIDI Association/AMEI, M2-104-UM v1.1.2, dated 2023-10-27,
published 2023-11-10, sections 2.1.2 and 7.4. Used only for group addressing and
resolution requirements, not copied code or reference audio. Official
[specification entry](https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification)
and [AMEI published PDF](https://amei-music.github.io/midi2.0-docs/amei-pdf/M2-104-UM_v1-1-2_UMP_and_MIDI_2-0_Protocol_Specification.pdf).
No specification file is redistributed. `Protocol::Midi2` and group/precision
storage do **not** constitute UMP decoding, MIDI-CI or device support.

## Rendering and bounded work

The old prototype scanned reserved voice capacity for every frame. Rendering now
splits a block at scheduled event boundaries, then processes contiguous frames for
each voice. Slot order preserves summation order. Exclusive-end and empty-block
semantics remain tested. Finite-source overflow still silences/counts affected frames.

Release/panic cleanup scans families, voices and commands once per domain after
propagating gate state. A family/voice retirement updates ownership counts directly.
Arena admission is still a bounded linear scan; ancestry propagation and terminal
retirement can still be quadratic. The renderer scans inactive capacity once per
event segment. Dense event traffic, admission, streaming and DSP are not represented
by the microbenchmark below. These ceilings remain optimization work, not claims
of optimal complexity or proven hard realtime deadlines.

## Validation

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo +1.92.0 test --locked --offline -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline --release -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo clippy --locked --offline -p sampler-core --all-targets -- -D warnings
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --profile ci
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --release --test v2_ksp
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --offline --release -p sampler-core --example render_bench
```

Core: 11 unit tests plus one independent realtime integration test. The latter
counts allocations and frees through first use, saturation, expression detachment,
rendering, cancellation, panic and terminal retry for 100 cycles. The library forbids
unsafe code; the shared test-only allocator wrapper locally permits forwarding to
`System`. The tests also cover stale/foreign/quarantined family/expression handles,
family versus voice targeting, failed-admission rollback and 4,000 deterministic
mixed operations checked against reachable ownership counts.

Existing partition tests cover 44.1/48 kHz, regular blocks from 16 through 1024,
one-frame reference, irregular partitions, zero blocks and canceled starts. The
root suite remains a check against accidental checkout breakage, not a 1.x parity
gate for v2. All listed tests pass: core debug/release on Rust 1.99.0 and core debug on the
1.92.0 minimum; core clippy passes with warnings denied. Root CI passes 601 library,
2 CLI, 89 playback and 2 historical integration tests (34 pre-existing ignores).
The two historical integration tests also pass in the shipping release profile.
Root compilation still reports existing legacy warnings. `cargo tree -p sampler-core`
shows no dependencies. No DAW, cross-platform, sanitizer or competitor validation
was performed in this slice.

## Initial performance observation

Runnable [resident PCM benchmark](../../crates/sampler-core/examples/render_bench.rs):
64-frame blocks at 48 kHz, shared constant resident stereo source, unity gain,
100 warmup blocks and 2,000 timed callbacks. Linux x86-64, Ryzen 7 7800X3D,
Rust 1.99.0, workspace shipping release profile. One before/after run, not CPU-pinned;
background compilation and scheduler noise were not controlled. Timings include
clock measurement overhead. No claims about competing samplers follow from this.

| Active / capacity | Before p50 / p99 / max (µs) | After p50 / p99 / max (µs) |
| --- | --- | --- |
| 16 / 64 | 2.210 / 5.030 / 35.571 | 0.390 / 0.570 / 0.990 |
| 16 / 4096 | 133.102 / 284.466 / 966.378 | 2.420 / 4.450 / 22.791 |
| 256 / 256 | 25.340 / 36.971 / 109.942 | 5.340 / 9.160 / 25.340 |

All three output checksums agree before/after; neither run observed a callback
exceeding the 1.333 ms block deadline. Baseline was the pre-change core at `95faf11`
with the same benchmark body (before adding required family/expression/group fields).
This supports removing the per-frame empty-slot scan. It does not establish release
performance budgets or superiority to HISE, SINE, Kontakt, Falcon or KODA Sampler.
Those comparisons require matched features, source assets, quality and workload,
recorded product versions, representative hardware and repeatable measurements.

## Remaining foundation work

Unified timestamped external events/expression/continuations, physical/effective
gates and pedals, new scripting execution, instrument compilation, production DSP,
plan/asset lifetime, streaming, coherent state and actual MIDI 2.0/host adapters
remain open. PCM note release currently stops sources immediately; envelopes and
release tails are not implemented by this fixture. The source attachments and their conformance statuses are unchanged.
