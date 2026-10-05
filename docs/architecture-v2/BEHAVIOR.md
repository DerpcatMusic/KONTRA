# Native behavior execution

This is a clean-sheet, bounded musical instruction path. It does not use the old
parser, VM or compatibility bridge. It is partial V2-05 evidence, not KSP or Lua
support and not completion of the behavior milestone.

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

Generated children link their gate and expression to the original logical note.
Their own duration can end them earlier. A closed effective root gate cancels waits
and releases linked children; sustain can keep that root gate open after physical
key-up. Faults and explicit abort force root release and cancel pending linked work.
Existing envelope tails finish normally; panic hard-stops them. These are native
policies, not claims about vendor wait/cancellation semantics.

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
meets **90**, zero errors, 113 warnings, with all rules retained. Local test, scan
and audio evidence is under ignored `artifacts/architecture-v2/behavior-*`.

## Still open

Language parsing, shared/persistent state, real/array/string values, note-handle
locals, release/controller callbacks, consumed controller projections, rich engine queries,
beat/transport waits, async services and compatibility profiles are not implemented.
Their implementations must use these ownership/time services and extend the
executable contracts. No old-VM fallback is present.

## Callback-local state and bounded branches

Preparation derives each program's local count from validated 16-bit operands.
Runtime construction checks the maximum program width times continuation capacity
against `Limits.behavior_cells`, including multiplication and allocation-layout
limits, before allocating storage. A callback clears its own local range when
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
