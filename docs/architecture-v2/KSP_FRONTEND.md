# Clean-sheet KSP source subset

`sampler-ksp` is a new control-thread compiler depending only on `sampler-core`.
It has no dependency on the existing parser/VM, plugin or import model. Its explicit
profile identifier is **`ksp-8.12-note-release-subset-v1`**. The source reference is the
[KSP manual showing Kontakt 8.12](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
consulted 2026-10-06. This is partial V2-05/V2-14 evidence.

The [full KSP completion map](KSP_PARITY.md) now inventories the entire functional
manual surface. This subset is not the product's completion target.

## Accepted shape

An optional first `on init` declares polyphonic integer variables. One `on note`
and/or one `on release` follows; duplicate or misplaced callbacks fail compilation.
A note callback must begin with `ignore_event($EVENT_ID)`. Release-only scripts keep
ordinary native attack selection. Init currently accepts declarations only.

Note/release bodies accept literal `wait(...)` and bare `play_note(...)` calls,
and assignments from signed 32-bit integer literals, `$EVENT_NOTE`, or another
declared polyphonic integer. Note-owned values start at zero and remain shared
between the originating note and its release callback, including overlapping waits.
Generated notes use `$EVENT_NOTE` with optional constant transposition, constant
velocity 1–127, zero offset and positive constant duration. Brace comments and
whitespace are accepted. Literal assignment covers -2147483648 through 2147483647;
wait/play operands retain their explicit nonnegative/positive command ranges.

This subset follows documented command units and distinguishes fixed duration from
input-linked playback. [Generated notes](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands)
and [waits](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands)
use microseconds. Lowering rounds upward to engine frames; this is an explicit
native policy, not verified Kontakt rounding. Compilation targets a supplied sample
rate and must be repeated when that rate changes.

The leading suppression is required because implicit forwarding and its interaction
with waits are not implemented. Generated voices use fixed normalized velocity and
independent expression rather than inheriting the suppressed input's expression.
The [event-command documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
notes that ignoring an event loses its volume/tune/pan information. The core now
exposes fixed/scaled velocity and expression inheritance as separate native choices.

## Explicit rejection and limits

The compiler rejects arithmetic expressions, dynamic command arguments (including
`$EVENT_VELOCITY`), globals, arrays, real/string values, conditionals/loops, other
callbacks, implicit note forwarding, nested comments,
nonzero sample offsets, input-linked negative duration and whole-source duration zero.
Some primitives already exist natively; that alone does not establish their KSP
semantics. Unknown syntax is never ignored and never sent to the old VM.

`Limits.source_bytes`, `Limits.instructions` and `Limits.variables` bound source,
total emitted code across callbacks and declarations. Symbol resolution happens on
control using a bounded standard-library map; names do not enter audio execution.
The parser walks input directly without recursion or a token-array allocation.
Malformed input returns a byte offset and a specific diagnostic; it never publishes
an executable partial program. Compilation/allocation happen before runtime creation.
Rendering uses the existing prepared programs, bounded continuations and fuel.

Capability status is deliberately separated: the declared syntax parses and lowers;
independent native scheduling/audio/ownership tests pass; Kontakt behavioral and audio
fidelity remain **unverified**. No Kontakt binary comparison has been performed.
Polyphonic declarations and scalar assignments now lower into
[note-owned integer cells](BEHAVIOR.md#note-owned-integer-state). Full typed arithmetic,
global/script-instance state and the other language/value services remain open.
Native storage is signed 64-bit; this compiler admits only signed 32-bit values,
key reads and copies. It does not lower KSP arithmetic into native checked-64-bit
addition or claim vendor overflow behavior.

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
