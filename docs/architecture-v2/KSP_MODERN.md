# KSP modern design

KONTRA plays KSP scripts as a modernized player, not a 1:1 clone. Rule: where a
behaviour is observable from a script (results, timing a script can see, error
semantics), match Kontakt 8. Underneath, the runtime may be faster and smarter.

Every symbol row in [docs/re/ksp/SYMBOLS.tsv](../re/ksp/SYMBOLS.tsv) is marked
`class = compat` (a Kontakt 8 symbol, legacy scripts rely on its meaning) or
`class = modern-extension` (a KONTRA-only addition that legacy scripts never use).
Coverage and the doctor are described in [docs/re/ksp/COVERAGE.md](../re/ksp/COVERAGE.md).

## 1. Compile once, typed fast path

Current state, from the code:

- `compile_with` lowers source to a `Script` whose callbacks run as
  `sampler_core::ops::Op` sequences. Scripts are not re-parsed per callback.
- `init_cache.rs` caches initialized state, keyed by source, layout and context
  identities (`Script::setup()`).
- Values are `Value::Int(i32)`, `Value::Real(f64)`, `Value::Text(String)` (`model.rs`).
- `on init` is evaluated by the control-thread evaluator (`eval.rs`) under a fuel budget.

Target:

- Every note, release, controller, poly-aftertouch and RPN callback lowers to typed
  integer/real ops with no text and no heap allocation on the audio thread. Text and
  UI callbacks stay on the control thread.
- No per-callback re-interpretation of HIR: a callback is an op slice plus pre-bound
  event and variable slots.
- Status: "no heap on the audio thread" is not yet proven. It needs an allocation
  audit of the `Op` executor in `sampler-core` (allocation guard tests exist for
  event-ID fixtures; they do not cover every callback class). Until then it is a
  design target, not a claim.

## 2. Compatibility limits: kept or lifted

Kept limits are observable by scripts and must match Kontakt 8. Lifted limits are
KONTRA-internal and do not change script results.

| Limit | Kontakt 8 evidence | KONTRA | Class |
| --- | --- | --- | --- |
| Integer width | 32-bit signed (manual, Variables) | `i32` | kept |
| Array size per declaration | max 1,000,000 indices (manual) | 1..1000000 (`sema.rs`) | kept |
| `ui_table` columns | max 128 (manual; binary message) | 128 | kept |
| `ui_xy` elements | max 32 (manual; binary message) | 32 | kept |
| Total array cells | not documented | unlimited in `Limits::LIBRARY` | lifted |
| Declared variables | not documented | 65,536 (`Limits::LIBRARY`) | internal, to verify |
| Source size | not documented | 80 MiB (`Limits::LIBRARY`) | internal |
| Lowered instructions | not documented | 40,000,000 (`Limits::LIBRARY`) | internal |
| `on init` evaluation | not documented | fuel 200,000,000 (`INIT_FUEL`) | internal safety bound |

Nothing in the table is established as a Kontakt cap beyond the rows marked
manual or binary. Real-number precision and callback time budgets are not measured
against Kontakt and are not stated here.

## 3. MPE and per-note expression (modern extension)

Kontakt 8 gives scripts no inbound per-note data. The manual's `on note_controller`
section states that Kontakt "does not yet receive MIDI 2.0 messages from the outside",
and the event parameters (`$EVENT_PAR_*`) carry no per-note pitch, pressure or slide.
Per-note controllers exist only as values a script sends to another slot
(`set_note_controller()`). The engine already tracks MPE per-note defaults
(`sampler-core` `MpeDefaults`; `mpe_children` tests), so exposing that state is wiring.

Proposed additive symbols (`class = modern-extension`, status `unsupported` until
implemented):

- `$EVENT_PAR_MPE_PITCH`: per-note pitch bend, semitones.
- `$EVENT_PAR_MPE_PRESSURE`: per-note channel pressure, 0..1.
- `$EVENT_PAR_MPE_SLIDE`: per-note slide / timbre (CC74), 0..1.

Rules: they read through the existing `get_event_par()` path; a script that does not
mention them behaves exactly as under Kontakt 8; reading them outside a note context
is an error, as with other `$EVENT_PAR_*` symbols.

## 4. Diagnostics and profiling

- `ksp_symbols.py doctor FILE` prints every KSP symbol a script uses, with its
  status (`KSP-FULL`, `KSP-SIMPLIFIED`, `KSP-STUB`, `KSP-UNSUPPORTED`), its corpus
  rank, and `KSP-UNKNOWN` for `$`-symbols outside the surface map. `--fail-on` makes
  it a gate.
- Compile diagnostics already use `Kind::{Error, Warning, Approximate, Unsupported}`
  (`diag.rs`). Each first use of an approximated builtin reports once.
- Runtime observation: `RuntimeCoverage` records evaluator hits per span (opt-in,
  control thread). Proposed next: per-callback counters (fuel or ops used, time
  bucket) exposed the same way, so a slow script is attributable to a line.

## 5. Conformance

Every catalogue row names the sampler-ksp or sampler-kontakt tests that mention it,
or `untested`. `ksp_symbols.py check` fails if a row has no status, no test link,
or if the committed catalogue drifts from a fresh sync. It runs in the CI lint job.
