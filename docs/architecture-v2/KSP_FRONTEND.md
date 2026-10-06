# Clean-sheet KSP source subset

`sampler-ksp` is a new control-thread compiler depending only on `sampler-core`.
It has no dependency on the existing parser/VM, plugin or import model. Its explicit
profile identifier is **`ksp-8.12-note-release-subset-v1`**. The source reference is the
[KSP manual showing Kontakt 8.12](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
consulted 2026-10-06. This is partial V2-05/V2-14 evidence.

The [full KSP completion map](KSP_PARITY.md) now inventories the entire functional
manual surface. This subset is not the product's completion target.

## Accepted shape

An optional first `on init` declares ordinary/script-instance and polyphonic integer variables and supported scalar
UI controls. Optional `on note` and/or `on release` follows; duplicate or misplaced callbacks fail compilation.
A note callback may suppress its original attack with `ignore_event($EVENT_ID)`. Release-only scripts keep
ordinary native attack selection. Init accepts declarations, `make_perfview` and literal scalar initialization, including inline `declare $name := value`.
Init-only instruments keep native attack selection.

Note/release bodies accept `wait` and bare `play_note` calls, assignments, integer
expressions and the control flow described below. Expressions can read signed
32-bit literals, `$EVENT_NOTE`, `$EVENT_VELOCITY`, `$NOTE_HELD` and declared
global/polyphonic/control integers. Note-owned values remain shared between the
originating note and its release callback, including overlapping waits. Generated
note key, velocity and duration accept evaluated integer expressions; sample offset
currently requires literal zero. Supported key/velocity ranges are 0–127 and 1–127,
with positive microsecond duration. Waits accept nonnegative microseconds.

This subset follows documented command units and distinguishes fixed duration from
input-linked playback. [Generated notes](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
and [waits](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands)
use microseconds. Lowering rounds upward to engine frames; this is an explicit
native policy, not verified Kontakt rounding. Compilation targets a supplied sample
rate and must be repeated when that rate changes.

An unsuppressed note callback forwards its original event at the first `wait`, `exit`
or normal completion. Forwarding retains its note identity, high-resolution velocity
and live expression owner. Explicit generated voices instead use normalized velocity
and independent expression rather than inheriting the suppressed input's expression.
The [event-command documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
notes that ignoring an event loses its volume/tune/pan information. The core now
exposes fixed/scaled velocity and expression inheritance as separate native choices.

## Explicit rejection and limits

The compiler rejects real-valued expressions, arrays, real/string values, select/case,
compound Boolean expressions, other callbacks, multi-slot event forwarding, nested
comments and nonzero sample offsets. Evaluated out-of-range command arguments fault
at execution before publishing a child or timer. Input-linked negative duration and
whole-source duration zero are still unsupported and fault explicitly.
Some primitives already exist natively; that alone does not establish their KSP
semantics. Unknown syntax is never ignored and never sent to the old VM.

`Limits.source_bytes`, `Limits.instructions` and `Limits.variables` bound source,
total emitted code across callbacks and declarations. Symbol resolution happens on
control using a bounded standard-library map; names do not enter audio execution.
The parser walks input without a token-array allocation. Statements use iterative
block patching; integer expressions use precedence climbing with a fixed 64-level
nesting bound and checked temporary-register indices.
Malformed input returns a byte offset and a specific diagnostic; it never publishes
an executable partial program. Compilation/allocation happen before runtime creation.
Rendering uses the existing prepared programs, bounded continuations and fuel.

Capability status is deliberately separated: the declared syntax parses and lowers;
independent native scheduling/audio/ownership tests pass; Kontakt behavioral and audio
fidelity remain **unverified**. No Kontakt binary comparison has been performed.
Polyphonic declarations and scalar assignments now lower into
[note-owned integer cells](BEHAVIOR.md#note-owned-integer-state). Full typed arithmetic,
persistent/typed script state and the other language/value services remain open.
Native storage is signed 64-bit, while source integer expressions now use explicit
signed-32 operations. Native checked-64 addition remains separate for runtime counters.
The numeric edge policy and its unverified vendor fidelity are recorded below.

## Audition and evidence

The authored fixture can run without the legacy application:

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-native -- script crates/sampler-ksp/tests/fixtures/delayed-note.ksp /path/to/new-script.wav
```

This command auditions two seconds of a fixed-pitch sine sample mapped to all keys;
it is not a library renderer or pitch-tracking claim. The fixture waits 1.25 s,
after physical key-up at 0.5 s and effective gate release at 1 s, then starts a 125 ms note
with a 50 ms release tail. At 48 kHz the verified sound occupies frames
60,000–68,399, with exact silence outside that interval. One original-input terminal
is accepted. Invalid source is rejected before creating output; overwrites are refused.

Frontend checks cover malformed/unsupported constructs, source/instruction/
clock limits, deterministic arbitrary text, precise diagnostic offsets, and native
execution at 44.1/48/96 kHz over block sizes 1–16. The execution fixture transposes
selection, emits a fixed velocity from a silent original input, isolates expression,
waits past input release and verifies exact original identity retirement. Actual
execution/retirement is allocation/free checked.

Rust 1.99 release/all-target Clippy and MSRV 1.92 checks pass. The workspace
comparison against `fb22275` passes with no new errors.
Current new-core Doctor evidence is complete and authoritative: **90**, zero errors,
118 warnings, with all rules retained. Logs, source hashes and rendered audio use
ignored `artifacts/architecture-v2/ksp-*`. Full language/state/UI/host support remains
open in [TASKS.md](TASKS.md).

The same frontend can now audition a supported resident WAV instead of the demo
sine: `sampler-native script INPUT.ksp SAMPLE.wav OUTPUT.wav`. Compilation uses the
sample's rate and renders a fixed two-second window. See the
[native sample path and process-level checks](NATIVE_ENTRY.md#scripted-resident-wav-audition).

## Note/release state integration

`compile` returns an owned `Script`, containing its complete callback table and
declared note-cell count. `Script::bind` installs it on a same-rate prepared
instrument; a rate mismatch is rejected before activation. This replaces the
one-program return type without an old-API shim. The CLI sizes note storage from
the declaration count and has capacity for simultaneous note/release callbacks.

The core reserves a release continuation at triggered-input admission. Physical
key-up dispatches it once, independently of pedal-held gate closure, retaining
the original plan and state. Hard cleanup suppresses pending handlers. This
uses the documented [note-off callback](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-release)
and [polyphonic lifetime](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables#polyphonic----polyphonic-integer-)
requirements; detailed Kontakt cancellation, forwarding and rounding remain unverified.

`polyphonic-release.ksp` runs two overlapping physical same-key inputs with distinct
logical notes. Tests independently check retained values, both signed literal limits,
waits overlapping key-up and pedal-up, exact generated PCM, callback/terminal ownership
and zero heap activity at 44.1/48/96 kHz across blocks 1, 3, 7 and 16 with empty calls.
Negative checks cover scope/order, duplicate/unknown declarations and callbacks,
reserved prefixes, invalid assignment tokens, literal overflow, total code/variable
budgets and mismatched preparation rate. The real CLI also renders the release
fixture from WAV and checks its onset, stereo equality and source EOF. Evidence is
under `artifacts/ksp-state-*`; no Kontakt executable was used.

## Conditional callback execution

Note/release callbacks now accept nested `if (<scalar> <comparison> <scalar>)`,
optional `else`, matching `end if`, and `exit`. Scalars use the same checked i32
literal/built-in/polyphonic resolution as assignments. All six documented integer
comparisons (`=`, `#`, `<`, `<=`, `>`, `>=`) lower to native signed comparisons,
without subtraction overflow. Arithmetic/compound Boolean expressions remain open.
See [NI control statements](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements).

`$NOTE_HELD` reads the originating owner's current physical key state. It is zero
in the physical-release callback even when sustain still holds the effective gate;
a different same-key owner's key-up cannot change it. It is read again after a
wait rather than captured as a global key flag. The [NI built-in reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants)
defines the query in relation to the key causing the callback. Generated callback
contexts and hard-cleanup equivalence still require separate vendor evidence.

`exit` finishes this callback through the existing retained outcome path. It does
not release the original key or cancel the independently reserved release handler.
[NI function-local exit](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#exit)
will need a call-frame return when functions land; functions are currently rejected.

The parser uses an iterative control-side branch-patch stack bounded by already
emitted instructions and source bytes. It does not recurse with source nesting.
Both sides, including code after `exit`, are validated and charged against the
instruction budget. Two preallocated callback registers suffice for these scalar
comparisons. Waits retain only the existing program counter/owner/local state;
there is no runtime syntax tree, branch stack, name lookup or allocation.

`held-branches.ksp` checks overlapping same-key owners, nested alternatives, early
exit, simultaneous note/release waits and key-up under sustain at three sample
rates and four block sizes. Tests independently verify exact generated PCM,
retained cells and completion backpressure without heap activity. Native signed
comparisons cover i64 extrema, aliasing and both register bounds; source comparisons
cover i32 extrema and all six operators. Malformed/dead branches and 1,024 levels
of nesting exercise structural and total-budget validation. The actual WAV CLI
also executes delayed conditional playback between key-up and pedal-up. Validation
logs are under ignored `artifacts/ksp-branches-*`; Kontakt fidelity remains unverified.

This slice passes all four native crates in debug, release and Rust 1.92 tests,
strict all-target Clippy, the two root workspace boundary tests, and byte-identical
regeneration of the 25-chapter interface inventory.

## Repeating callbacks

Scalar conditions also drive `while (...) ... end while`; `continue` returns to
its innermost loop's condition, including from nested `if` blocks. The same bounded
control-side patch stack lowers both constructs into existing jumps. No new runtime
stack or scheduler was introduced. Every condition is evaluated again, so
`$NOTE_HELD` observes key-up after a resumed wait while other owners can continue.

All executed instructions consume the existing per-resume fuel. Positive waits
suspend onto the sample queue; zero waits do not reset fuel. Exhaustion retains
`Outcome::FuelExhausted`, releases the originating musical ownership according to
the native abort policy, and cannot strand an unreported callback. This is a native
realtime bound, **not** emulation of the NI manual's documented
[10-million-iteration loop guard](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements#while---).
Full vendor loop-limit/cancellation behavior remains an explicit fidelity gap.

`held-loop.ksp` independently repeats two same-key owners through alternating
continue/wait paths; exact PCM ends separately after their physical key-ups while
sustain remains down. Tests cover three rates/four block sizes, fixed child lifetime,
callback/terminal retirement, slot reuse and panic cancellation without heap activity.
Nested-loop continue, early exit, zero-time infinite loops, malformed block nesting
and code-budget boundaries have executable checks. The WAV CLI also renders two
expected repeat pulses and stops on key-up before pedal-up. Logs are retained under
ignored `artifacts/ksp-loops-*`.

The loop slice passes frontend/CLI debug, release and Rust 1.92 tests, strict
all-target Clippy for both affected crates, and byte-identical inventory
regeneration. Native core code and its public contract are unchanged by this slice.

## Shared scalar UI state

`compile(source, rate, limits, control_bindings)` requires an explicit persistent
`ControlId` for every declared UI variable. Missing, extra, duplicate-name and
aliased-ID bindings fail compilation. Bindings are bounded by `Limits.variables`.
No declaration-order hash or UI position becomes a persistent identity. The CLI
currently passes no bindings and therefore still rejects scripts with UI declarations;
the library integration tests exercise this path directly.

Supported declarations are `ui_knob(min,max,display_ratio)`, `ui_slider(min,max)`,
`ui_button` and `ui_switch`, with signed integer literal arguments. Knob display
ratios may be negative but not zero. Bounds must be ordered. Native default policy
is zero clamped to the declared range; literal init writes outside the range reject.
These corner policies are not yet verified against Kontakt. `make_perfview` records
presentation intent. `Script::controls()` exposes source-order metadata and stable
bindings; `Script::has_performance_view()` preserves authored view intent.

`Script::bind` replaces the complete script/control table off audio. It installs
native [headless control state](CONTROL_STATE.md), separate from note-owned cells.
Note/release assignments and conditions can read/write control integers through the
same native services as control clients. The source-name table and presentation
metadata do not enter realtime execution. Source reads use the original plan's
controls across waits and replacement.

The UI renderer, complete callback family, automation gestures, control properties
and resource skins remain open. Declarations and numeric values are **partial widget support**,
not rendered-widget parity. In particular, a KSP button's mouse-up callback and
host-automation restrictions must be preserved when that interaction layer lands;
see [NI widgets](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-widgets).
The complete frontend scope is tracked in [UI_FRONTENDS.md](UI_FRONTENDS.md).

`tests/controls.rs` drops all presentation metadata before rendering, changes a
control while a note callback waits, and independently checks exact generated PCM,
release access and separate polyphonic values over blocks 1/7/64 under the heap guard.
It also checks explicit identity mapping after declaration reordering, init-only
native attack selection, signed bounds, invalid bindings and malformed declarations.

## UI handler execution

`on ui_control($variable)` now compiles supported scalar control handlers into the
same native instruction IR. Each control accepts one declared handler. Unknown,
non-control and duplicate targets fail compilation, as do note-dependent operations
in the handler. Supported control reads/writes, scalar conditions/loops, `wait` and
`exit` execute with a plan-owned continuation. This is distinct from note/release
ownership and retains its own locals and original controls through replacement.

Interactions enter through `Runtime::invoke_control` or the bounded `Invoke` control
request. Handler capacity is reserved before the value changes. Source assignments
inside a handler do not recursively invoke it. The [NI UI callback](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-ui_control)
reference establishes the per-widget callback; global `on ui_controls`,
`on ui_update`, `$NI_UI_ID`, complete value types and widget-specific gesture order
remain open. Those missing constructs are rejected rather than silently omitted.

A source fixture changes a button, waits without any musical note, sets a switch
from another control and subsequently changes native note playback. It independently
checks exact PCM, retained outcomes and zero callback heap activity. This proves
native service integration, not Kontakt scheduling/gesture equivalence.

## Script-instance globals

Ordinary `declare $name` variables start at zero; inline signed literal initializers
and literal assignments in `on init` prepare their values off audio. Note, release
and UI callbacks share the same instance bank, including across waits. Polyphonic
variables retain their separate per-note cells. This follows the distinctions in
[NI's variable reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables).

The native service supports independently bound script instances in one generation.
The current `Script::bind` installs one KSP instance and its callback table; ordered
multi-slot event forwarding and native UVI callback registration remain open. Instance
state is not encoded as UI controls or borrowed from a note. Schema/operand validation
occurs before activation, and retired state travels back through the plan transfer
for off-audio destruction. Rebinding an instrument starts new initial state while
waiting old callbacks retain their originating bank.

The authored fixture combines two same-key note callbacks, a waiting UI callback,
global sharing, per-note memory, release reads and exact independently expected PCM
across blocks 1/7/64 under allocator instrumentation. Declarations also reject duplicate
names, malformed/range-invalid initialization and exhausted variable budgets. Full
init execution, constants, arrays, persistence and Kontakt differential
fidelity remain required work.

## Integer expressions

Assignments and comparison operands now accept parentheses, unary signs, `+ - * /`,
`mod`, `.and. .or. .xor. .not.`, and `abs`, `sgn`, `signbit`. `inc`/`dec` update a
declared global, polyphonic or scalar control lvalue. Multiplicative operators bind
above additive operators, then bitwise AND, then bitwise OR/XOR; equal-precedence
binary operators associate left. The [NI arithmetic reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/arithmetic-commands---operators)
defines the operations. Precedence and edge-policy review also inspected the v1
parser/evaluator as a regression source, not as vendor proof.

`Binary32`/`Unary32` instructions retain signed 32-bit results in the native registers.
Overflow wraps; integer division truncates toward zero; remainder retains the dividend
sign. Both division and remainder return zero for a zero divisor, and `MIN/-1`,
negation/absolute value of MIN retain the wrapping result. These reproduce the stated
legacy regression policy; direct Kontakt differential evidence remains open. An
out-of-i32 native register operand faults before changing its destination, rather
than silently truncating an unrelated 64-bit engine value.

Code generation reuses temporary registers by expression depth. The existing source,
emitted-code, runtime-local and instruction-fuel limits still apply. Parenthesis/unary
nesting is capped at 64 levels independently of source size. The native CLI reserves
the actual prepared maximum register width for each of its callback slots, so richer
expressions do not rely on the former fixed two-register allocation.

Tests cover independent wide-integer arithmetic references, precedence/associativity,
signed extrema, zero divisors, nested unary/functions, aliasing, invalid/wide operands,
malformed/deep expressions and variable updates. A heap-audited alternating-note
sequence computes its branch across waits after physical release and renders exact
PCM across blocks 1/7/64. The process-level WAV fixture now also computes a nested
global expression before waiting, then branches after key-up at 44.1/48/96 kHz.
Shifts, compound Boolean expressions, full init execution and other value types
remain required; this is not full-language parity.

Integer-expression validation: all 206 native tests pass in debug, release and Rust
1.92; strict all-target Clippy and both root boundary tests pass. The source process
fixture passes with its wider prepared register allocation. Logs use
`artifacts/integer-expressions-{debug,release,msrv,clippy,boundary,process}.log`.
The complete inventory still has 25 chapters, 288 sections and 1,605 identifiers;
24 named interfaces now have partial overrides, with vendor fidelity unverified.


## Evaluated musical arguments

`wait(expr)` and `play_note(key_expr, velocity_expr, 0, duration_expr)` now evaluate
through the same bounded integer IR. Arguments evaluate left to right into separate
registers; later nested expressions cannot overwrite earlier results. `$EVENT_VELOCITY`
reads the owner's onset velocity, rounded to the nearest seven-bit value. This read
never changes the core's high-resolution velocity. Generated KSP keys use plan tuning;
native `Play` still supports transposition of absolute-pitch parents and full-resolution
fixed/scaled velocities. The two forms share child admission and release reservation.

`MicrosToFrames` rounds upward using checked integer arithmetic, rejects negative
input and overflow before overwriting the register, and uses the runtime sample rate.
`WaitLocal` shares the immediate wait scheduler; zero advances inline and consumes
fuel. `PlayMidi` validates all evaluated operands before any selection or child
publication. Constants obey the same runtime checks. Native register widths derive
from every operand, including dead code; the CLI already allocates the prepared width.

Heap-audited source tests combine two same-key identities with distinct logical pitch,
velocity and computed delay, release both before wake-up, and independently check
exact PCM at 44.1/48/96 kHz across blocks 1/7/64. Additional tests cover zero waits,
invalid key/velocity/duration, conversion overflow, unchanged failed registers and
complete terminal cleanup. Native error policy and high-resolution quantization still
need Kontakt differential evidence; source support is partial, not vendor parity.

Evaluated-argument validation: all 209 native tests pass in debug, release and Rust
1.92; strict all-target Clippy and both root boundary tests pass. Logs are
`artifacts/dynamic-arguments-{debug,release,msrv,clippy,boundary}.log`. The manual
inventory remains 25 chapters / 288 sections / 1,605 identifiers, now with 25 named
partial overrides; vendor fidelity remains unverified.


## Original-event forwarding

The leading-ignore restriction has been removed. `ignore_event($EVENT_ID)` can be
conditional inside `on note`. The first `wait`, `exit` or normal callback completion
commits the unsuppressed original mapped attack exactly once. A zero wait is also
an explicit commit point but retains the native inline/fuel policy. Later waits,
completion or late suppression cannot duplicate or undo an already committed attack.
Release-event suppression remains unsupported; that needs its own forwarding phase.

The native `ForwardAttack`/`SuppressAttack` instructions use the existing note, its
original prepared generation, captured onset selection and live expression owner.
No replacement child, velocity quantization, expression reset or second host terminal
is introduced. Normal and deferred attacks share the same selection preflight,
release reservation and commit implementation. Native direct-call capacity failures
publish no partial sound/reservations and leave pending mapping retryable; a failure
inside a callback instead follows the existing retained-fault and gate-cleanup policy.

[NI event commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
restrict note changes to before the first wait and demonstrate waiting before querying
a voice's zone ID. The reviewed v1 `yielded`/`forward` path provides regression context
for first-yield/completion forwarding, not fresh vendor differential evidence. New
fixtures check ordinary empty callbacks, conditional suppression, first/repeated/zero
waits, exit, late suppression, original high-resolution/MPE-style expression ownership,
release reservations, failed admission, plan replacement and terminal backpressure.
All runtime paths are heap-audited. Ordered multi-slot/controller/release forwarding,
event mutation and full Kontakt/Falcon differential fidelity remain open.

Original-forwarding validation: all 218 native tests pass in debug, release and Rust
1.92; strict all-target Clippy and both root boundary tests pass. The targeted debug
check also verifies rejection after key-up while sustain still holds the gate, so
forwarding cannot reserve an already-passed release phase. Logs use
`artifacts/forward-attack-{debug,targeted,release,msrv,clippy,boundary}.log`.
