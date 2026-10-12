# NEXT: explicit-path NKA service is missing, not an implemented Host service

This is source inspection and an actionable narrow follow-up, not product code.
No file service was added to the fade/Lua batch. No library, NKA or host process
was opened. No Rust test executed. Native runtime parity remains UNKNOWN.

## Authoritative contract checked

NI KSP Manual:
<https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands#load_array_str-->
and `#save_array_str--`; also the same page's General Information and
`#save_array--` for format/async context. Retrieved2026-10-10, current unversioned
page with8.12 sidebar, not a measured installed-version contract.

Coordinator archive `handoff/official-docs/ksp-load.html` SHA256
`9bcd5f671eac2301211c5d7c542d8fc506657c6517535709674fe31d14abffbf`
was rehashed and matched `official-docs/manifest.json`. Text lines55/128 contain
precise load/save sections. The manual says:

- Explicit absolute path, forward slash separators.
- File first line names the array; followed by one value per line.
- Integer, real and string arrays may be saved.
- Synchronous use in `on init`; asynchronous use in `on persistence_changed`,
  `on ui_control` and `on pgs_changed`.
- Async ID and `on async_complete` status must reflect the action.
- `load_array_str` does NOT implicitly persist its target.
- Save creates a new file only when its parent directory exists.

Documentation is the REA-equivalent precise native specification evidence for
this inspection. It does not establish malformed-file coercion, header sigil
edge rules, atomic-write implementation, Windows replacement behavior, or
native callback timing. Those remain UNKNOWN. No vendor code was copied.

## Exact429e source proof

All source spans below pin
`429e7dff3ecb0f53aef32c43201ab796ccc8c28c` unless marked v1:

- `crates/sampler-ksp/src/builtins.rs:118-119`: signatures exist `[A S] -> Int`.
- `lower.rs:2684-2699`: generic Host effect and immediate result0, no job ID.
- `lower.rs::effect/emit_effect/effect_args:1405-1481`: effect forwards numeric
  args and first text argument. `lower.rs::arg:1350-1364` maps an `Arg::Var` to
  its UI ID, or ZERO for a non-UI array. A plugin consumer alone therefore
  cannot recover a general target's name/type/cell range from this effect.
- `eval.rs:857-877`: init requests retain the variable's authored name.
  `eval.rs:1650-1667`: `LoadArrayStr/SaveArrayStr` only queue requests and return0;
  synchronous read results do not reach later init statements through this path.
- `src/plugin.rs:1725-1751`: drains effects; only MIDI_SERVICE has a dedicated
  host branch. Other effects go to `ScriptUi.apply`.
- `crates/sampler-ksp/src/lib.rs:295-477`: UI apply service has no NKA handler;
  unmatched services returnfalse. Init engine-start replay near1335-1352 only
  handles engine writes and purge requests, not array-file actions.
- Bounded exact-name/NKA search of429e `src/`+`crates/` finds seven occurrences:
  two census strings, two builtin names and three UI file-extension/drop tests.
  `LoadArrayStr/SaveArrayStr` enum-name searches also checked evaluator/lowerer;
  no production NKA worker/reader/writer consumer found. This is stronger than
  checking literal builtin spellings alone, but not a dynamic runtime trace.

Verdict: signature/Host routing EXISTS; actual explicit-path NKA service is
MISSING in the examined production v2 paths. Code presence alone cannot support
an implemented load/save or native-parity roadmap label.

## Reusable OWN v1 implementation, read-only

`0cb7a8a0:src/ksp/calls.rs:400-490` has explicit-path async request admission,
prepared path/snapshot capacity, IDs and failure completion; synchronous init
load/save branches follow. `0cb7a8a0:src/ksp/arrays.rs:1-72` separates prepared
jobs from worker I/O/recycling; `:114-164` writes a sibling temporary, flushes,
preserves permissions and replaces the destination; `:166-186` parses typed
one-value-per-line files. Authored tests start at193/209.

These are our code, not decompiled native code. The frozen tree was not edited.
Do not blindly port v1's Value/VarId/runtime ownership wholesale. Its permissive
bare headers, lossy UTF8 and filesystem case fallback are historical own
policies, NOT verified Kontakt behavior. Its atomic-write path still needs
platform-specific failure/old-bytes-preserved checks when adapted.

## Smallest COMPLETE next implementation boundary

A codec alone or another UI effect handler would still fail the contract. Next
work must retain a typed array descriptor (name/type/bank/range), return an
allocated async ID rather than0, and complete the same generation/instance.
Separate init from callback execution:

1. Init: explicit-path I/O on the preparation owner must occur before the next
   script statement observes array contents, with no implicit persistence.
2. Live callback: use prepared bounded request/path/save-snapshot storage;
   off-audio read/parse/write; apply load results and deliver async completion
   through the audio-owned state. Reject stale generation/instance completions.
3. Preserve finite typed values, failure atomicity, existing destination bytes,
   missing-parent/read-only failures and bounded resource limits. No allocation,
   free, lock or filesystem I/O on audio. Reuse existing core async machinery
   only after checking its MIDI-specific identity/ownership constraints.

Coordinator must route this as a separate source slice; do not sneak it into
the current tested429e freeze or the prepared fade pair. No architectural seam
was changed in this inspection. General file-dialog modes, `fs_navigate`,
resource-container implicit paths and unrelated script services are out of scope.

Prepared next regression requirements (not authored/executed in this slice):
integer/real/string synchronous init roundtrip with authored temp-directory NKA;
header/name/type mismatch and malformed data leave target untouched; explicit
load does not persist; async ID/completion success/failure/wait ordering; save
snapshot reflects request time; missing/read-only parent never truncates prior
bytes; stale generation does not edit a new instance; allocator guard covers
admission/application/completion. Only authored temp files, not proprietary
library contents. Native malformed-header and OS clock behavior remain UNKNOWN.
