# First executable slice

Date: 2026-10-05. Source starting point: `fc0b5f4`, whose runtime is unchanged from
`ac2adc981191347bbdacaee3a29c359464ec712e`. This is experimental M1 groundwork,
not completion of M0/M1 or a production engine switch.

## Implemented

[`sampler-core`](../../crates/sampler-core/src/lib.rs) is a dependency-free workspace
crate with unsafe code forbidden. The root package uses it only as a development
dependency. CLAP/VST3/standalone production playback remains on the existing engine.

- Separate opaque note and voice handles carry runtime identity, slot and generation.
  Stale/cross-runtime handles fail; exhausted generations are quarantined, not wrapped.
- Notes retain original signed external identity independently of transformed key and
  high-resolution velocity. Anonymous identical inputs pair FIFO; exact tuple duplicates
  remain inadmissible until their terminal notification is accepted.
- Bounded note/voice arenas, parent links, linked/detached release, continuation pins,
  immediate cleanup and explicit voice stop. Source completion does not erase a held
  logical note. Detached children retain provenance without following parent release.
- Terminal delivery uses the existing note slot. A refusing sink retains ownership;
  retries stop on the first refusal. No separate terminal queue can overflow after admission.
- A bounded, stably ordered sample-time queue reserves delayed voice starts before
  returning success. Release removes delayed starts and cannot be blocked by queue capacity.
  Failed future scheduling returns an error; it does not silently drop the request.
- Borrowed, validated resident stereo PCM at the prepared sample rate. This is a unity-rate
  fixture path, not a copy of the legacy resampler or a second production sample engine.
- Checked monotonic clock arithmetic; events at an exclusive block end remain pending
  until the next call, including an empty render. Source data/destructors stay outside
  rendering because the runtime borrows the immutable prepared arrays.

The [authored KSP integration probe](../../tests/v2_ksp.rs) uses the existing VM and
`KspEngine` interface. It suppresses the original note, generates a linked sustain
and a detached whole sample, resumes a polyphonic variable after `wait`, and verifies
PCM and original terminal identity. A thread-local allocator counts both allocations
and frees during callbacks. Unsupported probe operations fail explicitly.

The bridge intentionally supports one input root and a finite binding table. It is
test code, not an adapter ready for arbitrary KSP instruments. Continuation pin/unpin
is explicit in the fixture; automatic integration with every VM continuation is still
required. Production code has not gained a runtime-selection switch.

## Prototype contract and capacities

| Resource | Owner | Fixture bound / failure |
| --- | --- | --- |
| Notes | Core audio execution | 8 in the main fixture; 2 in saturation test. Reject admission before accepting ownership when full. |
| Voices | Core audio execution | 8 normally; 1 in saturation test. Failed start leaves no allocated voice/job. |
| Scheduled commands | Core audio execution | 8 normally; 1 or 2 under pressure. Future scheduling returns `Capacity`; immediate release/panic does not use this queue. |
| Terminal delivery | Existing note slot | Remains retained until sink acceptance. No-source notes use the same rule. |
| Continuations | Existing KSP VM plus explicit core pins | A note cannot be reclaimed while pinned. Scheduler owners must unpin canceled/completed work. |
| Sample memory | Preparing caller | Borrowed immutable slices; no callback refcount/destructor. Mismatched rates, empty samples and nonfinite inputs rejected during preparation. |
| KSP bindings | Test bridge | 8 preallocated entries; no production wrap/reuse contract implied. |

These are executable fixture bounds, not tuned product limits. The initial arenas
and queue use bounded linear scans; child propagation/retirement can be quadratic
in note capacity. `ponytail:` comments record those ceilings. Large-pool optimization
requires measurement; no low-CPU or hard-deadline claim follows from these tests.

No-source roots explicitly close their gate before terminal delivery. Rejected
`note_on` calls have not admitted ownership: a future host adapter still needs a
bounded policy for protocol inputs arriving after core admission is exhausted.

## Checks executed

Use explicit worktree-local target directories: a machine-wide Cargo configuration
otherwise redirected the initial baseline attempt to a shared cache. The accepted
baseline results were rerun with `CARGO_TARGET_DIR="$PWD/target"`.

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo +1.92.0 test --locked --offline -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline --release -p sampler-core
CARGO_TARGET_DIR="$PWD/target-core" cargo clippy --locked --offline -p sampler-core --all-targets -- -D warnings
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --profile ci --test v2_ksp
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --profile ci
CARGO_TARGET_DIR="$PWD/target" cargo clippy --locked --offline --profile ci --all-targets
CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --release --test v2_ksp
```

The six core tests pass on Rust 1.99.0 and the declared 1.92.0 minimum,
including the shipping release profile on 1.99.0. New-crate clippy passes with
warnings denied. Both KSP/native integration checks pass in the `ci` and shipping
`release` profiles.
Root-package clippy also completes, with existing legacy warnings; the root package
is not warning-clean.
The root `ci` run passes: 601 library tests, 2 CLI tests, 89 playback tests and 2 new
integration tests; 34 pre-existing tests are ignored. Core tests run separately
because ordinary root `cargo test` does not select all workspace members.

Focused legacy checks also passed: five host-owner tests, host terminal backpressure,
MPE channel reuse, stale runtime/view replacement, async completion and budgeted
persistence refresh with an unchanged source during each copy. These do not cover
the two failure cases below.

PCM assertions cover 16/32/64/128/256/512/1024-frame blocks, a one-frame reference,
irregular partitions and empty calls at 44.1/48 kHz. The KSP fixture runs at 48 kHz.
Expected samples are exact authored constants, not recorded vendor output.

## Conformance scope

| Executed assertion group | Related catalogue requirements | Limits |
| --- | --- | --- |
| Original address, linked/detached children, stale handles, held one-shot, admission refusal | NOTE_IDENTITY_01–05, 07 | No general expression/family model yet; no vendor golden |
| Delayed PCM onset, partition equality, cancellation, end-boundary/empty render | EVENT_SCHEDULING_01/02/04; HOST_LIFECYCLE_AND_OFFLINE_03 | Start/release queue only; not unified transport, CC or arbitrary callback scheduling |
| Root retention through rejected terminal output and later ID reuse | HOST_LIFECYCLE_AND_OFFLINE_01/02 | Mock sink; existing plugin no-owner route still fails its separate probe |
| Continuation and generated KSP note lifetime | KSP_RUNTIME_01–03 | One authored script, not a pinned Kontakt reference measurement |
| Queue pressure, bounded cleanup and zero heap operations | ROBUSTNESS_AND_RESOURCE_BOUNDS_03/08 | Exercised fixture paths; no universal native-helper/lock/I/O audit |
| Clock/generation exhaustion | ROBUSTNESS_AND_RESOURCE_BOUNDS_06 | Forced boundaries in the core; legacy clocks not changed |

These rows report passed assertions within a subset, not full-catalogue pass counts.
The four source attachments and their original statuses remain unchanged.

## Confirmed legacy defects

Two temporary tests were appended to the unmodified baseline plugin test module and
run with `cargo test --locked --offline --profile ci --lib v2_baseline_probe`.
Both failed. The exact probe code is preserved in [probes/legacy.rs](probes/legacy.rs);
`src/plugin.rs` was restored byte-for-byte afterward. The probes are deliberately
outside the passing test suite and are **not fixed or counted as passes**.

1. **Unmatched terminal retry:** fill a one-event host output buffer; send an exact
   note on an unmatched port; clear output and process another block. The rejection
   counter increments, but no NOTE_END retry arrives. Failure:
   `missing retry for rejected no-owner NOTE_END`.
2. **Torn persistence:** set every element of a 128-element persistent array to 1;
   copy 64 values; set every element to 2; finish the copy. The result's first element
   is 1 and last element is 2, a state that never existed at a callback boundary.
   Failure: `torn persistent array: first=1, last=2`.

To reproduce, append `probes/legacy.rs` to `src/plugin.rs` **in a disposable checkout
of ac2adc9** and run the command above with a checkout-local target directory.
Both tests should fail on that baseline. Do not overwrite current development files.
The source contains only authored scripts/events and no private library content.

These findings keep V2-01 open. The native kernel demonstrates retained terminal
ownership, but plugging a retry vector into the old callback would not solve
admission/overflow for every host path. Coherent persistence likewise needs a bounded
capture contract; restarting forever under continuous edits is not a complete fix.
V2-13/15 must solve these before production cutover, and confirmed fixes must precede
using affected legacy behavior as an oracle.

## Next work and release gates

Complete the baseline fixes and resource/queue inventory, then extend the kernel with
explicit families, expression ownership, raw/projected controller state and unified
event/continuation scheduling. Add automatic KSP ownership bridging and fault cleanup;
retain immediate command/query semantics. Gate/pedal policies, scoped DSP, immutable
plans, production streaming, coherent state and host integration remain open.

No UI replacement, new external dependency, proprietary compatibility claim or
production default change is included. No macOS/Windows or real DAW validation was
performed in this Linux slice. Full release/cross-platform gates remain necessary
before production integration.
