# Clean-sheet KSP source subset

`sampler-ksp` is a new control-thread compiler depending only on `sampler-core`.
It has no dependency on the existing parser/VM, plugin or import model. Its explicit
profile identifier is **`ksp-8.12-note-subset-v0`**. The source reference is the
[KSP manual showing Kontakt 8.12](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/welcome-to-ksp),
consulted 2026-10-05. This is partial V2-05/V2-14 evidence.

The [full KSP completion map](KSP_PARITY.md) now inventories the entire functional
manual surface. This subset is not the product's completion target.

## Accepted shape

One `on note` callback must begin with `ignore_event($EVENT_ID)`. Following statements
may be literal `wait(...)` calls and bare `play_note(...)` calls. A generated note
uses `$EVENT_NOTE`, optionally transposed by a constant, constant velocity 1–127,
zero sample offset and positive constant duration. Ordinary brace comments and
whitespace are accepted. Integer literals are restricted to the positive signed
32-bit range documented by [NI](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables).

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

The compiler rejects dynamic expressions (including `$EVENT_VELOCITY`), assignments,
variables, conditionals/loops, other callbacks, implicit forwarding, nested comments,
nonzero sample offsets, input-linked negative duration and whole-source duration zero.
Some primitives already exist natively; that alone does not establish their KSP
semantics. Unknown syntax is never ignored and never sent to the old VM.

`Limits.source_bytes` and `Limits.instructions` bound source and emitted code.
The parser walks input directly without recursion or a token-array allocation.
Malformed input returns a byte offset and a specific diagnostic; it never publishes
an executable partial program. Compilation/allocation happen before runtime creation.
Rendering uses the existing prepared programs, bounded continuations and fuel.

Capability status is deliberately separated: the declared syntax parses and lowers;
independent native scheduling/audio/ownership tests pass; Kontakt behavioral and audio
fidelity remain **unverified**. No Kontakt binary comparison has been performed.
There is no new KSP global/polyphonic variable model yet; native callback locals and
[note-owned integer cells](BEHAVIOR.md#note-owned-integer-state) must not be presented
as complete KSP variable semantics.

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

Three frontend checks cover malformed/unsupported constructs, source/instruction/
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
