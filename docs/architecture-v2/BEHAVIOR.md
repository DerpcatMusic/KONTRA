# Native behavior execution

This is a clean-sheet, bounded musical instruction path. It does not use the old
parser, VM or compatibility bridge. A [new KSP source subset](KSP_FRONTEND.md) now
compiles into it. This remains partial V2-05 evidence, not full KSP/Lua support or
completion of the behavior milestone.

## Executed contract

Control-side `Program::new` validates immutable instructions. A prepared plan owns
its program table and optional note binding, validated together by `with_programs`.
A bound `Runtime::trigger` admits an input owner and runs the program instead of
automatic region playback. An empty program therefore suppresses playback while
preserving physical note pairing and terminal delivery. Generated children select
native regions directly; they do not recursively invoke the bound program.

The current instructions are deliberately concrete:

- `Play`: transpose the logical key, scale velocity, select the complete native
  layer set and create a generated child with a sample-duration release.
- `Wait`: suspend until an absolute engine sample boundary; zero executes inline.
- `End`: finish the callback. Reaching the instruction array's end also finishes.
- `SetLocal` / `AddLocal`: callback-local signed 64-bit integers with checked addition.
- `ReadKey`: read the originating note's logical key into a local.
- `Jump` / `JumpIfZero`: validated instruction targets; all branches consume fuel.

Transposition currently changes region selection; the resident source renderer
still runs at unity rate. This is not a pitch-resampling claim.

Generated children select linked, snapshot or independent expression inheritance,
and fixed or scaled velocity, separately from their explicit duration:
`Gate` follows the originating effective gate, `Frames` is independent, and
`FramesOrGate` ends on whichever condition happens first. The default wait lifetime
is gate-bound; `WaitLifetime::Callback` retains a callback across input release.
Sustain can keep an effective gate open after physical key-up. Faults and explicit
abort force root release and cancel pending gate-bound work. Already-admitted
independent notes keep their scheduled release and retain the input ancestor until
finished. Existing envelope tails finish normally; panic hard-stops all voices and
cancels every pending callback regardless of its wait lifetime. These are explicit
native policies, not claims about vendor cancellation behavior.

## Ownership and bounds

Each callback occupies one generational continuation slot and privately pins its
originating note. `Limits.behaviors` bounds slots and `behavior_fuel` bounds
instructions per synchronous run/resume. Programs, PCM and all arenas are prepared
off audio. No callback allocates, frees, locks, sleeps or performs file IO.

A callback record remains allocated after `Finished`, `Cancelled`, `FuelExhausted`
or `Fault(Error)`. `flush_behaviors` releases its private pin only when the sink
accepts that outcome. Rejection stops retry for that call. This prevents both lost
faults and premature host terminal notifications. Public `unpin` cannot consume
these private pins. A full continuation pool rejects a new bound input before
publishing any note. Every resource failure is observable.

The existing stable command queue now also holds resume actions. Waits enqueue
strictly future boundaries; zero waits consume fuel inline. Native service calls
inside a queued resume do not recursively drain later equal-time work. This keeps
callback execution ordered and its synchronous changes immediately visible.
Previously submitted same-frame events still execute in submission order, and
exclusive block-end work waits for the next render call, including an empty call.

Native helper costs remain bounded by prepared region candidates and arena/queue
capacities. Fuel is an instruction bound, not a time guarantee. A frame can execute
up to the admitted callbacks' fuel plus previously queued musical work; future
production budgets need workload measurements on target hardware.

## Invariant review

Remaining checked-index/unwrap diagnostics are retained, not suppressed. Internal
resume IDs originate only in the continuation arena; records cannot retire before
acceptance, and cancellation removes their queued resumes. Records privately pin
notes, so their owner lookups remain valid. Program indices are validated before
admission and the program table cannot mutate while the runtime owns it. Instruction
fetch checks its bound. Generated layer admission shares the existing prepared
selector and whole-family preflight. No untrusted bytecode pointer enters rendering.

## Evidence and runnable example

Behavior checks cover blocks 1–32, generated-note audio, suppression, duration,
exclusive-end and empty rendering, stable equal-time order, invalid programs/keys,
clock overflow, queue/voice/continuation saturation, fuel exhaustion, deferred
faults, panic, explicit abort, rejected completion and cross-runtime stale handles.
Allocation/free counters wrap actual program execution, waits and retirement.

The independent executable now offers:

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-native -- echo /path/to/new-echo.wav
```

The echo starts through native MIDI 2.0 ingress and the shared block processor.
It generates two 125 ms notes at frames 0 and 12,000, each with the region's 50 ms
release. The rendered 48 kHz file contains identical 8,400-frame voice waveforms,
an exact silent gap and an exact silent suffix after frame 20,400. It accepts one
original-input terminal. Existing output files are refused.

Rust 1.99 release/all-target Clippy, MSRV 1.92 and the historical v2 boundary checks
pass. The full-workspace comparison against `2643eb9` has no new errors.
The current authoritative, complete new-core Doctor scan
meets **90**, zero errors, 115 warnings, with all rules retained. Local test, scan
and audio evidence is under ignored `artifacts/architecture-v2/behavior-*`.

## Still open

Broader language parsing, shared/persistent state, real/array/string values, note-handle
locals, release/controller callbacks, consumed controller projections, rich engine queries,
beat/transport waits, async services and compatibility profiles are not implemented.
Their implementations must use these ownership/time services and extend the
executable contracts. No old-VM fallback is present.

## Callback-local state and bounded branches

Preparation derives each program's local count from validated 16-bit operands.
Runtime construction reserves a fixed stride from `Limits.behavior_cells` divided
by continuation capacity and checks each program width and allocation layout before
allocating. [Plan replacement](PLAN_ADOPTION.md) validates new widths against that
same reserved stride; old callbacks keep their original program and local bounds. A callback clears its own local range when
admitted. Values survive waits and completion backpressure; reused slots start at
zero. `behavior_local` validates both the generational handle and program-local
bound, so a caller cannot inspect another callback's cells.

Branch targets are validated before activation. A yielding loop can continue while
its originating note is held, with at most one queued resume per callback. A loop
without a positive wait exhausts instruction fuel and follows the native abort
policy. Integer overflow reports `Fault(ArithmeticOverflow)` instead of wrapping.
These are native arithmetic semantics, not a vendor-language emulation rule.

Tests run overlapping callbacks with different key-derived counters across blocks
1–16, and check exact independently expected audio. Additional checks cover reused
slots, reads at the exclusive wait boundary, stale handles, oversized local tables,
invalid branches, positive/negative integer overflow, zero-time loops, and release
of indefinitely yielding loops. Actual operations remain allocation/free checked.
The native echo now uses a local counter and loop instead of two written-out plays;
its rendered audio is byte-identical to the earlier straight-line program.

The first expanded scan scored 89. Shared checked local access and fault propagation
removed unchecked accesses without disabling rules; the authoritative scan is back
at 90. Local evidence uses the `artifacts/architecture-v2/locals-*` prefix.

## Independent callback and generated-note lifetimes

Continuation retention, generated duration and expression inheritance are separate
ownership decisions. `with_wait_lifetime(Callback)` permits waiting work on a still-
owned closed note, allowing later release-side processing. It cannot resurrect an
already retired handle. Completion still requires acceptance before the private
input pin is released. An input-linked generated note on an already-closed gate is
rejected; fixed-duration generation can proceed and retain the closed ancestor.

New checks compare independent and gate-bound waits across blocks 1–16, explicit
duration modes, closed-note callback admission, panic and explicit abort. A separate
fault case verifies that an independent child's scheduled release remains owned
and executable after its callback faults; the original input cannot retire early.
All paths are allocation/free checked. The core score remains 90. Evidence is under
`artifacts/architecture-v2/lifetime-*`.

Frontend research is pinned to the KSP manual showing **Kontakt 8.12**, consulted
2026-10-05: [manual version](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
[generated-note duration](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands),
[wait timing](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands),
and [release callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks).
These references motivate distinct ownership policies. No Kontakt binary comparison
or complete language implementation is implied; source-duration zero and vendor
microsecond rounding still require separate implementation/evidence.

## Generated-note reclamation under pressure

Before a behavior admits another generated note, exhausted note or expression
capacity triggers a bounded internal-owner sweep. Closed notes retire only after
all families, children, public pins and private work pins are gone. External input
owners remain allocated until `flush_ended` accepts their terminal; a waiting or
rejected external terminal does not block reclamation of unrelated internal notes.
Code retaining an internal note handle across later generation must pin it until
finished with it. Reclamation allocates/frees no heap memory and shares the same
iterative retirement path as explicit terminal draining.

The regression first failed with `Fault(Capacity)`. It now renders 100 short notes
using three note slots (two retained inputs and one reusable child), and separately
with three expression slots. Blocks 1, 7, 64 and 256 produce identical expected
pulses while the terminal sink rejects delivery. Completion and both original
terminals are accepted exactly once afterward. This removes dependence on host
block size or terminal-drain frequency for completed generated-note reuse; genuinely
simultaneous live owners still require sufficient declared capacity.

Evidence: `artifacts/architecture-v2/reclaim-*`. The authoritative new-core scan is
90, complete, zero errors and 118 warnings, with no rule suppression.
