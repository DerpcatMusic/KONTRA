# Clean-sheet KSP source subset

`sampler-ksp` is a new control-thread compiler depending only on `sampler-core`.
It has no dependency on the existing parser/VM, plugin or import model. Its explicit
profile identifier is **`ksp-8.12-note-release-subset-v1`**. The source reference is the
[KSP manual showing Kontakt 8.12](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
consulted 2026-10-06. This is partial V2-05/V2-14 evidence.

The [full KSP completion map](KSP_PARITY.md) now inventories the entire functional
manual surface. This subset is not the product's completion target.

## Accepted shape

An optional first `on init` declares ordinary/script-instance and polyphonic integer variables, integer constants/arrays and supported scalar
UI controls. Optional `on note`, `on release` and `on controller` follow; duplicate or misplaced callbacks fail compilation.
A note callback may suppress its original attack with `ignore_event($EVENT_ID)`. Release-only scripts keep
ordinary native attack selection. Init accepts declarations, `make_perfview` and constant-expression initialization, including inline `declare $name := value`.
Init-only instruments keep native attack selection.

Note/release bodies accept `wait`, bare or value-returning `play_note` calls, assignments, integer
expressions and the control flow described below. Expressions can read signed
32-bit literals, `$EVENT_ID`, `$EVENT_NOTE`, `$EVENT_VELOCITY`, `$NOTE_HELD` and declared
global/polyphonic/control integers, constants and indexed integer arrays. Note-owned values remain shared between the
originating note and its release callback, including overlapping waits. Generated
note key, velocity, source offset and duration accept evaluated integer expressions. Supported key/velocity ranges are 0–127 and 1–127,
with positive microsecond duration, `-1` for the originating effective gate, or `0`
for independent whole-source lifetime. Waits accept nonnegative microseconds.

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

The compiler rejects real-valued expressions, real/string arrays and values, other callbacks, multi-slot event forwarding, nested
comments. Evaluated out-of-range command arguments fault
at execution before publishing a child or timer. Durations below `-1` are invalid.
Input-linked generation requires the originating effective gate to remain open;
independent fixed/whole-source notes can be generated after its release.
Some primitives already exist natively; that alone does not establish their KSP
semantics. Unknown syntax is never ignored and never sent to the old VM.

`Limits.source_bytes`, `Limits.instructions` and `Limits.variables` bound source,
total emitted code across callbacks and declaration symbols. `Limits.array_cells` separately
bounds the aggregate declared array elements. Temporary constant-expression lowering
also obeys the instruction budget before its instructions are discarded. Symbol resolution happens on
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
without subtraction overflow. Compound conditions and arithmetic are now supported
through the expression lowering described below.
See [NI control statements](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements).

`$NOTE_HELD` reads the originating owner's downstream logical key state. It is zero
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

Ordinary `declare $name` variables start at zero; inline constant-expression initializers
and constant-expression assignments in `on init` prepare their values off audio. Note, release
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
init execution beyond constant preparation, persistence and Kontakt differential
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
Shifts, full init execution and other value types
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
restrict changes to the sounding note to before the first wait and demonstrate waiting before querying
a voice's zone ID. The reviewed v1 `yielded`/`forward` path provides regression context
for first-yield/completion forwarding, not fresh vendor differential evidence. New
fixtures check ordinary empty callbacks, conditional suppression, first/repeated/zero
waits, exit, late suppression, original high-resolution/MPE-style expression ownership,
release reservations, failed admission, plan replacement and terminal backpressure.
All runtime paths are heap-audited. Ordered multi-slot/controller/release forwarding,
broader event mutation and full Kontakt/Falcon differential fidelity remain open.

Original-forwarding validation: all 218 native tests pass in debug, release and Rust
1.92; strict all-target Clippy and both root boundary tests pass. The targeted debug
check also verifies rejection after key-up while sustain still holds the gate, so
forwarding cannot reserve an already-passed release phase. Logs use
`artifacts/forward-attack-{debug,targeted,release,msrv,clippy,boundary}.log`.


## Script-visible note edits

`change_note($EVENT_ID, expression)` and `change_velo($EVENT_ID, expression)` now
lower to shared native event-property writes inside `on note`. Expressions use the
existing signed-32 evaluator. Native validation accepts keys 0–127 and MIDI 1
velocities 1–127; out-of-range values fault before mutation. This is an explicit
native policy, not a measured claim about Kontakt's out-of-range behavior. Other
event targets/groups and use from other callbacks are rejected at compile time.

Each logical note has immutable admission properties, a current script-visible
view, and committed audio properties. Before forwarding, edits affect mapping,
velocity gain and release reservation. Forwarding commits them only after complete
preflight succeeds. After forwarding, edits update `$EVENT_NOTE`/`$EVENT_VELOCITY`
and subsequent generated-note expressions, while existing audio and release mapping
keep their committed properties. Suppressed notes can still edit/read their view.
Physical note-off pairing and the expression owner never change.

This follows the late-variable-update distinction in the pinned
[NI event-command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands).
The legacy `ChangeNote`/`ChangeVelo` implementation ignores edits after `at_engine`;
that behavior was not copied. Vendor differential evidence is still required.

`NoteProperties`, `initial_note_properties`, `note_event` and `edit_note_event` are
format-neutral native services. Initial values retain absolute MIDI 2 pitch and
full-resolution velocity. KSP explicitly projects these to key/7-bit semantics;
setting a KSP key replaces absolute pitch with the original plan's tuned key. Cold
per-note storage is allocated at runtime construction, generation-checked on every
public access, and reset at shared admission for both physical and generated notes.
No old-core dependency or format-specific ownership path is introduced.

Tests cover before/after-wait audio, read-after-write arguments, suppressed callbacks
continuing after key-up, release identity, failed preflight/retry, invalid inputs,
stale/reused handles, full-resolution admission, expression ownership and unreachable
instruction register/context validation. Runtime paths run under the heap guard.
The manual inventory now has 27 partial named overrides; none claims vendor parity.

Event-edit validation: all 221 native tests pass in debug, release and Rust 1.92;
strict all-target Clippy and both root boundary tests pass. Logs use
`artifacts/event-edits-{debug,release,msrv,clippy,boundary}.log`.


## Gate-linked and whole-source generated notes

Evaluated `play_note` duration `-1` now chooses native `Duration::Gate`; duration `0`
chooses `Duration::UntilSilent`. Positive values retain checked microsecond-to-frame
conversion. Source sentinel meanings follow the pinned
[NI general commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands).
The frontend emits ordinary conditional IR and explicit `DurationValue` policies;
negative source sentinels do not leak into native duration or scheduler APIs. Literal
durations use the direct branch when available; dynamic arguments preserve their
key/velocity registers and use a separate comparison register. All emitted paths
still count against preparation budgets and runtime instruction fuel.

Whole-source children have an independent gate and no release timer. Their logical
identity may retire once all families (including effects tails), scheduled work,
callback/manual pins and descendants finish. Completion does not manufacture a
musical note-off, release sample, release velocity or approximate event timestamp.
Unused release reservations are returned at retirement. Normal terminal flushing
and admission-pressure reclamation use the same owner walk; internal reclamation
never accepts a physical host terminal. Real physical keys retain their existing
release/terminal requirements. Unbounded loops deliberately remain live until an
explicit stop or panic, including when muted.

Checks include literal/dynamic zero and minus-one, blocks 1/7/64, layered source
ends, unmapped notes, effects-tail retention, muted/infinite loops, panic, exact PCM,
quota recovery before host-terminal acceptance and no runtime heap operations.
Current policy follows the parent's effective gate (including pedal holds); precise
Kontakt pedal/special-duration behavior remains a differential-test obligation.
Source note-end control is described below; broader event-targeted commands, sample
DFD offset limits and ordered script stages remain open. No vendor-fidelity claim is added.

Generated-lifetime validation: all 224 native tests pass in debug, release and
Rust 1.92; strict all-target Clippy and both root boundary tests pass. Logs use
`artifacts/note-lifetimes-{debug,release,msrv,clippy,boundary}.log`.


## Live DSP from headless UI callbacks

A prepared instrument can now bind its KSP scalar control identity to a native
`GainControl` processor. The end-to-end fixture invokes a plan-owned UI callback,
sets the gain control, waits 125 microseconds, then changes it again while a physical
note continues sounding. Both assignments update the existing shared control owner
and its native gain ramp. Independent expected PCM matches blocks 1/7/64 with no
heap work and no synthetic UI note. The production UI and vendor engine-parameter
commands are still separate open tasks; no widget renderer is claimed here.


## Bounded integer arrays and constants

`declare const $SIZE := 2 * 64` substitutes a read-only signed-32 value during
control-side compilation. `declare %notes[$SIZE]` prepares an array in the same
script-instance bank as globals. Arrays accept 1–1,000,000 elements, within the
caller-provided aggregate `Limits.array_cells` budget. The native CLI permits
1,000,000 total array elements. Dimensions and init writes require constant integer
expressions; ordinary globals/event reads are not constants. Omitted initialization
zeros the array; `(1, 2)` repeats its final value through the remaining elements.
The maximum and repeat behavior follow the [NI variables reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables);
rejecting zero-length arrays is an explicit native policy awaiting vendor probes.

Callbacks evaluate `%notes[index]` reads/writes, including nested indices, through
bounded native array instructions. `inc`/`dec` retain signed-32 wrapping;
`num_elements(%notes)` lowers to the declared count. Constants cannot be written,
and reserved host arrays are rejected until their own semantics exist. Continuation
markers (`...`) are accepted. Real/string arrays, bulk operations, persistence and
host-provided arrays remain open.

Arrays are bounded views, not another runtime owner. All views and register operands
validate before activation; each access validates its evaluated index before changing
a destination. A callback fault preserves earlier writes but cannot reach adjacent
state. Native instance IDs remain 16-bit, while cell offsets are 32-bit. Native
heap-guarded checks cover cross-instance isolation, waits, old-generation retention,
invalid indices, and dead-code bounds. Source checks cover overlapping note/release
callbacks driving PCM at blocks 1/7/64 and a million-element array followed by a scalar.
Kontakt fidelity remains unverified.

Validation: 239 native tests pass in debug/release and Rust 1.92; strict all-target Clippy and both
root boundary tests pass. Logs: `artifacts/script-arrays-*`.


## Per-note group selection

`allow_group(expression)` and `disallow_group(expression)` now lower onto native
per-note masks. `$ALL_GROUPS` can be used in integer expressions; `$NUM_GROUPS`
reads the originating prepared generation in executable callbacks. It is not yet
available during the frontend's constant-only init preparation. Group indices are
zero-based. UI/init group mutations are rejected. Invalid indices fault before a
mask write; invalid-index behavior still needs vendor comparison.

Note-callback edits stop taking effect after attack forwarding. Suppressed notes
can still configure the selection inherited by their generated children. Release
callbacks edit the same note's draft and commit its automatic release selection at
first wait, exit or completion. This commit occurs once; a later edit can configure
another generated child but cannot rewrite an already selected release or the
snapshot waiting for pedal-up. Suppression now defers that commit as described below;
ordered source slots and full Kontakt system-script behavior remain open.

The [NI group commands reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands)
documents note/release contexts, zero-based indices and restrictions on running
voices. First-yield release selection and the opaque numeric encoding of
`$ALL_GROUPS` (0x3fffffff) use the recorded legacy reference pending vendor probes;
the native IR uses an explicit all-groups target, not this sentinel.

Tests render independently expected overlapping attack/release layers at blocks
1/7/64, ignored late note edits, separately generated release children, malformed
indices and pedal-held release selection. Native tests cover multiword masks,
per-note isolation, slot reuse, failed-admission retry, ungrouped layers, take-history
eligibility and old-generation ownership. This is not full Kontakt group support:
names/lookup, purge, `%GROUPS_AFFECTED`, event-targeted group writes and source import
remain required work. Falcon hierarchy semantics are not inferred from Kontakt groups.

Group-selection validation: 246 native tests pass in debug, release and Rust 1.92;
strict all-target Clippy and both root boundary tests pass. Logs: `artifacts/groups-*`.


## Select dispatch, Boolean expressions and hexadecimal integers

`select (expression)` supports constant cases and inclusive constant ranges, nested
select/if/while, empty selects and `continue` targeting the enclosing loop. The selector
is evaluated once. Only the first matching case executes; waits resume inside that
body and jump past later cases. Existing native comparison/jump/local instructions
suffice, with no new dispatch table or runtime owner. Case constants use the same
bounded preparation evaluator as declarations. Descending bounds are normalized to
the range they name, following the legacy implementation pending vendor verification.
There is no `else`/`default` case. A signed full-range case can serve as a fallback.

Expressions now include integer comparisons, `and`, `or`, `xor`, `not`, and inclusive
`in_range(value, low, high)`. Nonzero integers are true and Boolean results are 0/1.
`and`/`or` short-circuit; `xor` evaluates both operands. `not` covers its comparison
but binds before `and`; `and` binds before equally ranked, left-associative `or`/`xor`.
Arithmetic and bitwise operations bind before comparisons. These precedence and
short-circuit policies follow the recorded v1 reference and still need vendor probes.
`in_range` evaluates all arguments and returns false for descending bounds.

KSP hexadecimal integers require a leading zero and `H`/`h` suffix. Up to 32 value
bits are accepted, interpreted as signed two's-complement; unary negation wraps just
like native signed-32 operations. Constant preparation evaluates pure comparisons,
ranges and forward short-circuit branches using native arithmetic/comparison helpers.
Runtime reads are rejected in constants even behind a branch that would skip them.

The [NI control statements reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-statements)
defines the operators, inclusive ranges, first-match selection and absence of a
default branch. Authored checks cover overlapping/descending/full-signed cases,
hex limits, nested waits and exact PCM at blocks 1/7/64, loop continue, guarded and
unguarded invalid array accesses, constant/runtime equivalence and numeric precedence.
A 1,024-level statement nesting fixture remains iterative; expressions keep their
64-level bound. A thousand-case unmatched dispatch exhausts bounded runtime fuel,
while an early match skips the remaining cases. Dead source branches still validate.
Full initialization, functions, typed values and vendor differential parity remain open.

Control-flow validation: 251 native tests pass in debug, release and Rust 1.92;
strict all-target Clippy and both root boundary tests pass. Logs: `artifacts/control-flow-*`.


## Source event identities and generated-note results

`$EVENT_ID` now evaluates to a stable source identity. `play_note(...)` can return
its generated event ID in an integer expression, including array writes and larger
arithmetic expressions. Arguments use checked temporary registers without clobbering
enclosing expressions or array indices. Nested calls share the 64-level expression
bound. Bare calls need no ID export; short-circuited calls generate no child.

The [NI general commands reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
documents retaining a generated ID for later event operations. New source IDs resolve
through the native generation-scoped alias service described in [BEHAVIOR.md](BEHAVIOR.md).
They are separate from host IDs and native slot indices. Aliases do not pin notes:
retirement invalidates them even if a variable/array still contains their integer.
Original and generated IDs remain distinct across overlapping callbacks and release.

The frontend bounds exports to 0x0fffffff, reserving upper bits for future source
selectors. This is an explicit implementation namespace, not a claim that Kontakt
assigns the same numbers. The counter never repeats within a runtime. Export exhaustion
fails before publishing a result-bearing child; failed admission can consume an opaque
ID without publishing it. Existing aliases and full-width native handles remain valid.
Native direct API calls can resolve an ID only against its originating plan generation.

Tests cover IDs in scalar/array expressions, native result/argument register aliasing,
all three generated lifetimes, independent PCM at blocks 1/7/64, original release IDs,
short-circuit suppression, bounded nesting, slot reuse, panic, rejected terminal
acceptance, plan replacement and exhaustion. Event enumeration/status, targeted source
mutations, marks/selectors, fades and ordered source stages remain open.

Event-ID validation: 257 native tests pass in debug, release and Rust 1.92; strict
all-target Clippy and both root boundary tests pass. Logs: `artifacts/event-ids-*`.

## Stored-event note ends

`note_off(id)` and `note_off(id, offset)` accept evaluated integer arguments from
note, release and UI-control callbacks. IDs resolve only within the callback's plan
generation; retired/unknown targets and already forwarded releases are no-ops. A plain
one-argument call preserves a generated fixed-duration policy. An explicit nonnegative
microsecond offset replaces pending note-end deadlines; zero releases immediately.
A queued host key-up is not a generated fixed duration. Arguments validate before
replacement and offsets use the same upward frame rounding as other commands.

This lowers to shared `KeyUpEvent` and `replace_key_up_at`, using the existing
key-release callback, pedal, group-selection, envelope and release-layer services.
Original release callbacks execute once; generated notes retain the existing
single-stage policy of skipping their creator's callback. Repeated self-release
cannot recurse. UI handlers target stored IDs without fabricating a note owner.

The native queue owns pending note ends through private work pins, including when
an independent source has finished. Replacing multiple deadlines returns their pins
and uses their available capacity; invalid inputs and failed capacity checks leave
them intact. Execution, early key-up, hard cleanup and panic release those pins.
Same-time ordering remains stable and an exclusive block-end event stays pending.

The NI general-command examples distinguish fixed durations and explicit overrides;
legacy timer/stage code was inspected as a reference, not executed or copied.
All-event/marked selectors and ordered script slots remain unimplemented. Pedal behavior, event lifetime details and exact timing
still need vendor differential evidence; this is not full `note_off` parity.

Validation: 261 native tests in debug, release and Rust 1.92, strict all-target Clippy,
and both root boundary tests. Logs: `artifacts/note-off-*`. New fixtures cover exact
PCM at blocks 1/7/64, callbacks, full-queue replacement, invalid offset/velocity,
exclusive-end ordering, silent-source ownership, panic and stale IDs after reuse.


## Suppressed release forwarding

`ignore_event($EVENT_ID)` now works in `on release`. The original physical key-up
is recorded once, with its original timestamp and release velocity. The callback
can suppress downstream release before its first forwarding boundary, wait, alter
group selection and call `note_off($EVENT_ID)` (or its timed form) to resume it.
A forwarded release cannot subsequently be ignored. First-yield/exit/completion
does not silently undo explicit suppression; an unresumed event remains live until
an explicit release or hard cleanup, including after a resident source finishes.

Native `SuppressRelease`, `suppress_release` and `resume_release` share one retained
note owner with the ordinary pedal/release path. Suppression postpones automatic
key-release layers and the effective gate; pedal-up cannot bypass it. Forwarding
commits the current group draft, starts the key-release layers once, then closes the
gate when pedals permit. Later group edits cannot rewrite the committed gate-release
selection. Resuming does not dispatch a second release callback or fabricate a new
physical key-up. `$NOTE_HELD` already reports false throughout the deferred period.

Timed forwarding uses `Event::ForwardRelease` and the same bounded, pinned deadline
replacement as key-up. Panic, all-sound-off and callback fault/cancellation discard
held release work. Explicit forced native closure also returns unused key-release
reserves before selecting its gate-release phase. No waiting callback or rejected
terminal can transfer ownership to a reused note slot.

The [NI event-command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
permits release suppression; native tests exercise its delayed-release shape through
our DSP at blocks 1/7/64, group edits, pedals before/after forwarding, reserved layers,
full command capacity and faults. Ordered source-slot propagation and Kontakt's
special hard-cleanup callback rules remain unimplemented/unverified. This still does
not establish vendor parity. Validation: 266 native tests in debug/release/Rust 1.92,
strict Clippy and both root boundary tests (`artifacts/release-forward-*`).


## Raw input versus script event ownership

Source `note_off` now uses a distinct native `ScriptKeyUp` operation. It may end a
logical script event and start its release callback while the host key remains down.
That external input remains paired with its original full note handle. The eventual
host note-off consumes the retained input, never a newer anonymous repeated key, and
does not replay the already-consumed script callback. Callback faults/cancellation
also retain input pairing after closing their owned sound. Panic/explicit native
owner abort may discard it deliberately.

`$NOTE_HELD` continues to describe the script event. MIDI/MPE use `input_held` for
physical tracking instead. Source deadline replacement cannot remove a queued host
key-up. If physical key-up suppresses its release before a queued script note-off
expires, that original script deadline remains able to forward the held release.
Its full generational identity, original plan and work pin remain unchanged.

Validation: 270 native tests in debug, release and Rust 1.92, strict all-target Clippy
and both root boundary tests (`artifacts/input-ownership-*`). Fixtures cover FIFO,
exact-ID retention, script/fault completion, source/host deadline separation, callback
counts and both MPE zones. Vendor-specific `$NOTE_HELD` projection and multi-stage
rules still need reference execution; no complete compatibility claim is made.


## Controller event execution

`on controller` now handles CCs through the shared native dispatcher. `$CC_NUM`
retains the callback's event number across waits; `%CC[index]` reads the latest
admitted input value in its performance domain, including consumed events. The
source view rounds full-resolution values to 0–127; ordinary forwarding retains
the original 32-bit value. `ignore_controller` consumes pending forwarding and
`set_controller(number,value)` emits a new CC downstream without reentering the
creating script. First wait, exit and callback completion forward an unsuppressed
input once; late suppression cannot undo an already-published update.

This follows the documented roles of [controller callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks)
and [ignore_controller/set_controller](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands).
The recorded v1 `SetController` command routes generated messages to the next slot;
the new implementation uses native services directly and has no v1 runtime dependency.
Ordered slots now have per-module incoming banks within each performance domain.
Matched vendor wake/forward ordering still requires separate comparison.

Controller callbacks share globals/control values with note/release/UI callbacks.
They own their original prepared generation, captured event value and channel scope,
with no fake note owner. Both ordinary MIDI ingress and MPE manager CCs use this
path; consumed pedals never reach the gate. Remapping any MPE manager CC to a pedal
retains the entire zone scope.

Current limits: only integer CC numbers 0–127 and values 0–127 in set_controller;
virtual pitch-bend/aftertouch IDs, parameter-controller dispatch, channel-mode
semantics and matched vendor behavior remain open. Generated notes from controller
and explicitly routed UI callbacks now use the shared native services.
Source CC reads/writes run in note, release, controller and routed UI callbacks; event-number
reads and consumption still require the controller context. Faults
are retained native outcomes; prior writes are not rolled back. No vendor parity
claim follows from these authored fixtures.

Note/release `%CC` reads and `set_controller` writes use the originating note's
retained performance domain and source channel. They remain valid across waits
and physical key-up; polyphonic captures stay separate on overlapping notes.
No controller event or fake note is created to supply a missing context. Plain
bare plan callbacks reject these operands. UI interactions now supply the explicit
performance/address binding described below.


## User-defined functions

Top-level `function name ... end function` definitions and `call name` now lower
into native instructions. Empty parentheses are optional; arguments/return values
are not part of this source form. Definitions must precede calls, following the
[NI function syntax](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-defined-functions).
Nested calls to earlier definitions expand without a runtime call stack. Branch and
loop targets relocate to each call site; normal return falls through without an
event-forwarding barrier. An explicit exit still ends its callback.

Each function prepares valid templates for the supported callback contexts using
the same statement parser and operand checks. Note/release suppression and waits
therefore retain their caller semantics; controller/UI calls do not acquire fake
notes. Shared globals, polyphonic cells, controls, fuel and wait lifetime remain
owned by the existing native plan/note/continuation. No second function scheduler
or variable store is added.

Unused bodies still validate, and invalid definitions/contexts, duplicate names,
forward calls, recursion and arguments fail compilation. The sum of maximal
per-function template widths is bounded by the instruction limit; at most four
context templates are retained. Final expanded callback instructions share the
existing total limit. Excessive expansion is reported before activation. If real
libraries exceed this storage policy, shared bytecode calls need explicit bounded
return storage; code size is not silently allowed to grow without limit.

Fixtures execute nested calls, loop/continue/select relocation, polyphonic state,
callback exit, and overlapping waits from all four supported callback contexts.
A note-edit fixture proves function return does not prematurely forward the event.
Malformed/dead functions and exponential call expansion reject during compilation.
Function calls in init and additional callback/language contexts remain open, along
with source/vendor fidelity outside this native subset.

Function validation: 286 native tests pass in debug/release/Rust 1.92, strict
all-target Clippy and both root boundary tests pass (`artifacts/ksp-functions-*`).


## Generated-note source offsets

The third `play_note` argument now accepts nonnegative evaluated microseconds and
lowers to the existing native child admission path. Every attack source converts
the captured value with its own asset rate, retaining fractional sample phase;
playback pitch and output sample rate do not change the selected source time.
Literal zero keeps the existing instruction/register footprint. Invalid evaluated
values fault before a child, source alias or duration command is published.

The native policy measures from the original view's leading edge in its playback
direction. Starting within a loop retains its boundaries and pass count. Starting
at/past its outward edge bypasses it; at/past the source end produces silence and
normal retirement. Offsets do not advance the event clock or envelope and do not
shift later automatic release-source starts. This is resident-source behavior.

The [NI command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
specifies microseconds and separately documents DFD's zone Sample Mod limit. That
streaming rule, source-engine rounding and loop/release details still require
profile implementation and vendor fixtures; this change does not claim them.

Authored fixtures compare offset playback with independently elapsed native PCM
for both directions, two asset/output rates, octave shifts, finite wrap/reflected/
crossfaded loops, bypassed loops and exhausted views at blocks 1/7/64. A fractional
1.5-frame onset checks retained interpolation history. Negative/overflowed source
expressions fail without partial child/timer publication, under heap guards.

Validation: 303 native release/Rust 1.92 tests and strict all-target Clippy pass
(`artifacts/source-offset-*`).


## Controller-generated notes

`on controller` can generate key/velocity/offset expressions with a literal positive
duration or whole-source duration zero, and store the resulting event ID for
`note_off`. These notes use the existing arenas, source IDs, selection, expression
owners and terminal cleanup. They have no physical input or fabricated parent.
Their plan, performance domain and channel address come from the retained callback,
including generation replacement during a wait. No automatic host terminal is sent.

A generated root captures current downstream selection state when admitted, with
independent expression and ordinary group defaults. It does not re-enter its
creating script. Fixed duration remains independent of callback completion/fault;
whole-source lifetime retires normally. Scoped hard silence also cancels waiting
controller callbacks from that exact input address, so delayed generation cannot
resurrect sound after the stop. Unrouted plan/UI callbacks still cannot invent an
audio destination. Gate-linked duration/inheritance require a real note owner.

The current KSP dynamic-duration lowering includes a gate branch and consequently
still requires note context; broader controller duration expressions remain open.
Script-stage routing, source-specific pedal projection and vendor ordering remain
unverified. The tests establish native lifecycle/routing, not complete KSP parity.

Fixtures cover two performance domains and distinct port/group/channel origins,
waits across plan replacement, source offsets, stored-ID stops, admission failure,
post-generation callback faults, scoped cancellation, generation retirement and
absence of host terminals. All rendering/cleanup paths are heap guarded.

Validation: 305 native release/Rust 1.92 tests, strict all-target Clippy and both
root boundary tests pass (`artifacts/controller-notes-*`).


## Stored event targets for pitch and velocity edits

`change_note` and `change_velo` now accept evaluated individual event IDs in the
note callback. Their native instructions select either the current callback note
or a source ID in the callback's retained generation. Current-event calls keep
their prior register/instruction footprint; stored IDs use the existing bounded
source-ID index. Values are checked before any mutation. Unknown/retired IDs are
no-ops and never resolve to a reused slot or another generation.

Edits still affect the script-visible event projection. Pending attack forwarding
commits it once; edits to an already running target leave its PCM mapping and
physical key identity unchanged. This follows the distinction in the
[NI event command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands);
Kontakt execution comparisons remain required. Source callback restrictions remain
explicit, and the commands do not accept `$ALL_EVENTS`/marked groups. Native
plan-owned services can edit a stored projection without fabricating a note owner.

Fixtures cover array/expression targets, two simultaneous events, retained initial
properties, unchanged running audio, physical key-up, retired-ID slot reuse,
original/new-plan isolation, invalid-value atomicity and unreachable register
budgets. Runtime paths remain heap guarded.

Validation: 307 native release/Rust 1.92 tests and strict all-target Clippy pass
(`artifacts/event-target-edits-*`).


## Controller-only module chains

`bind_controller_chain` binds separately compiled controller modules in source
order using native controller stages. Globals/polyphonic ranges retain separate
script-instance ownership; native program indices and UI callback bindings relocate
when tables combine. Control IDs remain caller-supplied, stable and unique across
the instrument. Duplicate controls or incompatible sample rates fail preparation.
Single-script `bind` uses the same module installation path.

Generated `set_controller` events enter the following module, while `%CC` reads
that module's incoming projection. A defined callback can consume/remap before
its first wait; the next stage sees only explicitly forwarded/generated events.
Source fixtures cover delayed remaps, independent same-named globals, native CC
precision/quantization boundaries and a UI callback relocated into the second
module. The shared runtime's slot reservations keep forwarding bounded.

The chain entry point explicitly rejects modules with note/release callbacks.
The general `bind_modules` entry point below now handles ordered note callbacks;
ordered release callbacks are described below. UI rendering, full stage/event semantics and
Kontakt/Falcon fidelity remain open. This is an executable controller slice, not
full multi-script support.

Validation: 312 native release/Rust 1.92 tests, strict all-target Clippy and both
root boundary tests pass (`artifacts/controller-stages-*`).


## Ordered note/controller modules

`bind_modules` combines compiled modules without flattening their event state.
Note forwarding, `play_note` and `set_controller` use the common native module
positions. Following scripts receive projected properties/group masks; a source
callback resuming after a wait continues to see its own values. A generated event
retains a creating-module view for stored-ID edits and enters subsequent note
callbacks without reentering its creator. `%CC` reads each callback's incoming
module view, including when a note was generated by a controller callback.

`tests/stages.rs` checks source programs across empty module positions, pitch/group
selection, waits, consumed parents, generated fixed-duration notes and cross-kind
controller generation. The full native validation is recorded in
[BEHAVIOR.md](BEHAVIOR.md#ordered-note-stages). Ordered `on release` callbacks now use the same module table; Stage-scoped stops and full language/vendor parity
remain open. The generated-note and group projection policies need matched
Kontakt runs in addition to these independently specified native fixtures.


## Ordered release callbacks

`bind_modules` now admits `on release` at every module position. Reached-stage
reservations, local `$NOTE_HELD`, release suppression/resumption and separate release
group drafts share the native services in
[BEHAVIOR.md](BEHAVIOR.md#ordered-release-stages). The KSP compiler's existing explicit
first-yield forwarding remains authoritative; the kernel does not insert a KSP
forward when another frontend's native program finishes.

The source fixtures exercise independent release holds in consecutive modules,
release-only modules, unreached original notes and generated timed-note releases
that bypass their creator. Parent-following `play_note(...,-1)` now follows the
creating module's release, with independent downstream child holds, immediate release
for children created after that stage's release, and channel pedal handling without
fake host inputs. The exact cross-slot meaning of source `note_off` and matched vendor
validation remain open; see [native parent links](BEHAVIOR.md#parent-release-links-across-modules). The shared native fixed-duration service now routes the generated note-off
through release callbacks, matching the Note On/Note Off distinction in the
[NI command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands).

## UI-originated musical events across modules

UI callbacks now receive an explicit native performance/address context through both
direct and queued control interaction. Their prepared binding records the exact module
position, including UI-only/empty event positions. `%CC`, `set_controller` and
independent `play_note` execute through the existing native event services and enter
following modules, without fabricating an incoming MIDI event or host note. This
extends the documented [UI callback](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-ui_control)
and [generated-note/controller operations](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
into our explicit ownership model; it does not establish matched Kontakt ordering.

A source fixture verifies remapped incoming CC reads from a UI-only third module,
a generated note transformed by the fourth module, CC generation before/after a wait,
retention of the original generation across replacement, independent performance
state, actual channel origin and exact native audio/release. See
[control ownership](CONTROL_STATE.md#routed-ui-performance-and-module-ownership).
Control IDs remain caller-bound persistent identities; module composition never
renumbers them or aliases unrelated controls. Parent-dependent note operations and
controller-event operands are rejected in UI handlers. Full source UI generations,
widget gestures, async resource work and stage-scoped stored-event stops remain open.
