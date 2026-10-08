# KSP fields for the shared scanner

Handoff for `tools/kontra-scan@54f7ea57` extension. Source references below use v2 `7e82b152` or pinned v1 `0cb7a8a0`; measurement evidence is on `audit/ksp-20261008`. This specifies scanner instrumentation, not product fixes. All counters belong to the shared collector.

Historical 7e82 implementation: `tools/kontra-scan@abf248cd0b99d884b9f2456914362a7bd8e81869`, resolving source-slot counting independently of saved-table integrity; incomplete raw histograms export `unknown`. Current installed product checkpoint is 9993 with scanner-only Native observer `ef88dcb6`; see [checkpoint evidence](ksp.md#integration-checkpoint-9993-preliminary-current-witness). The sigil admission map/source spans below describe 7e82: current 9993 admits `!` as `ir::Saved::Texts` and sets the persistence callback type separately. Keep this version distinction when using the accessor handoff. Await completed current-digest shared coverage for whole-corpus production rates.

## Slot admission and ownership

Count the raw Script chunks **before** importer filtering. V2 production access is `BParScript::try_from(chunk)?.params()?`, with `BParScriptParams::{bypass,text,textfile_name,persistent}`; see `crates/sampler-kontakt/src/library.rs:201` and `vendor/ni-file/src/kontakt/objects/bpar_script.rs:24`.

Keep `owner` (bank-global / bank-container / embedded program / standalone program), `program_index`, `wire_slot` (zero-based among that owner's Script chunks), and `runtime_slot` separately. Increment `wire_slot` even for bypass/empty slots. V2 stores `min(wire_slot, 255)` in `ir::Behavior.slot` (`library.rs:231`); keep the full raw index in scanner metadata. V1 `src/import.rs:825` pushes only admitted sources into `scripts`, so its runtime slot can be compacted. `Runtime::faults()` reports **one-based** slots (`v1 runtime.rs:1630`); map back through the runtime-to-wire mapping.

Suggested mutually exclusive raw categories:

| Field | Exact definition |
|---|---|
| `slots_seen` | Every Script chunk encountered, including a chunk whose parameter decoding fails. |
| `slots_decode_failed` | BParScript/source parameter decoding failed; do not classify it as empty. Independent saved-table framing failure/unsupported framing does not change this category. |
| `slots_bypassed` | Successfully decoded `params.bypass == true`; bypass takes precedence over text/link state. |
| `slots_inline_nonempty` | Not bypassed, `params.text.is_some()` and `!text.trim().is_empty()`. |
| `slots_linked_only` | Not bypassed, no nonempty inline text, and a nonempty trimmed `textfile_name`. |
| `slots_empty` | Not bypassed, no nonempty inline text, no nonempty link. |

The five categories after `slots_seen` partition it. Retain independent `inline_text_absent`, `inline_text_blank`, and `linked_name_present` flags if finer counts are wanted, without logging the link/name/text. These are trimmed-content categories; separately preserve the production admission result, since v2 tests a linked name with `!name.is_empty()` rather than trimming it.

**Effective admission differs:** v2 `library.rs:213` admits non-bypassed/nonempty inline text and reports linked-only source as unsupported (`:240`). V1 `src/import.rs:632` resolves `Resources/scripts` first, falls back to saved inline text, and can admit linked-only slots. Record `effective_source_kind = linked | inline | none` from each production path; do not resolve extra links in v2 and thereby change the measured behavior. V1 `script_text` (`import.rs:648`) handles UTF-8/BOM, UTF-16LE and legacy encoding. A failed raw UTF-8 check must not be mislabeled empty.

The existing specialized KSP probe skipped bypass/empty and preserved wire slots, but **did not retain bypass/empty totals**. Its measured active population was 788 NKI slots / 1,141 NKI+NKM slots. Do not invent skipped-slot counts from those denominators. The ownership-labelled NKM follow-up identifies all 353 measured NKM slots as embedded-program scripts; the 45 v2 failures are not bank-global slots.

## Clean compilation versus admission

V1 `src/ksp/compile.rs:459`: `Program.errors: Vec<String>` means **non-init callback blocks that failed compilation and were disabled**. `compile.rs:646` rejects a failed init block, but pushes an error and calls `disable(block)` for other failed callback blocks. It is not the count of all source/function errors. `Program.diagnostics: BTreeSet<String>` is a separate diagnostic set.

Use `compile_admitted = compiler returned Ok`, `disabled_block_errors = program.errors.len()`, `compile_clean = compile_admitted && disabled_block_errors == 0`, and a separate `compile_diagnostic_count`. Capture these at the **existing production compile** in `v1 runtime.rs:1252`, preserving real `Setup`, inherited conditions and resource path. `Runtime::diagnostics()` (`:1642`) combines slot errors, compiler diagnostics, UI diagnostics, disabled callbacks, runtime faults and service notes; its total is not a disabled-block count.

Probe accessor: `tools/audit-ksp/prepare-v1.sh` exposes unchanged `performance_view::prepare(...).and_then(compile::compile_prepared(...))`, returning `Program.errors.len()` and diagnostic count. That isolated accessor uses eight outputs/zero zones/default inherited conditions; it is evidence, **not** a production adapter to transplant. Resource-aware evidence: 30 NKM slots admit a v1 Program with **19 disabled-block errors each**, while v2 rejects `subscribe_async`. Another 15 `mf_get_first` slots have v1 zero disabled-block errors but nine diagnostics; admission still does not prove implemented MIDI-object behavior.

V2 `sampler_ksp::compile_with` (`src/lib.rs:556`) returns `Result<Script, Error>`. `Script::warnings()` (`:165`) yields typed `Error::{kind,builtin,offset,line,column,message}`; kinds are Error / Warning / Approximate / Unsupported (`diag.rs:25`). Record safe enum kind and static `builtin`, plus numeric position. Do not persist `message`, `Error::to_string()`, or `ir::Unsupported.value`; they can contain source/vendor/user data.

V2 initializes several times: import engine harvest (`sampler-kontakt/library.rs:267`), dynamic-rack scan (`:283`, short-circuiting `.any`), and final runtime preparation (`load.rs:611`). Tag these attempts separately. Per-slot compile/init success counts should use final runtime preparation; do not count three frontend calls as three distinct scripts.

## Saved-entry sigils

Record **raw** saved-table counts before `library::saved` drops entries. V2 bounded accessors: `sampler_kontakt::Script::parse`, `.persistent: Option<Strings>`, `Strings::{len,iter}` (`src/script.rs:7`, `:23`, `:73`). `None` and `Some(empty)` differ. For each entry count only its first-byte sigil into a fixed histogram; retain neither name nor payload. Distinguish absent table, present-empty table, table framing error, and unknown/empty entry sigils. `BParScriptParams.persistent` alone cannot establish raw framing validity: the vendor reader drops a malformed/old table through `entries().unwrap_or_default()` (`bpar_script.rs:59`).

| Sigil | Meaning | v2 `library::saved` at `:1460` | v1 `ksp::saved_persistence` at `src/ksp/mod.rs:164` |
|---|---|---|---|
| `$` | Integer; menu declaration means native item **position**, not semantic item value | `ir::Saved::Int` | `Value::NativeInt` |
| `~` | Real scalar | `Saved::Real` | `Value::Real` |
| `@` | Scalar text | `Saved::Text` | `Value::Text` |
| `%` | Integer array | `Saved::Ints` | `Value::IntArray` |
| `?` | Real array, including XY coordinates | `Saved::Reals` | `Value::RealArray` |
| `!` | LF-separated text array | **Dropped** | `Value::Array` of text cells |

Count both `raw_saved_entries_by_sigil` and `admitted_saved_entries_by_sigil`; a raw `!` entry does not mean it was restored. Counting only `ir::Behavior.state` hides the dropped arrays. Keep histogram denominators explicit: all wire slots versus effective active slots, and instrument versus selected snapshot state.

Declaration-aware reader already exists on `v2/gpt-decipher-persist@6ae15820`, implementation `847d4670`: `src/persistence.rs:76` `SavedEntry`, `:102` `SavedEntry::from_bytes(raw, Option<WidgetKind>, Limits)`, `:58` `SavedValue`, and `:10` `ArrayTail`. Menu/ordinary integer share `$`; tables/ordinary arrays share `%`; use declarations for subtype. Ordinary numeric tails RepeatLast; tables/XY are Exact. `make_instr_persistent` has no special sigil. Do not conflate v1 text-array `split('\n')` (retains terminal empty cell) with the declaration-aware reader's LF terminator policy; a histogram needs no payload decoding.

Evidence: reused typed-reader census has 2,707 `!` records in 422/834 master NKI/NKM paths; fresh active-slot census finds 369/781 NKI paths with raw `!`. V1 `src/ksp/tests.rs:903` `saved_persistence_decodes_every_kind` is an authored typed-decoding test. The v2 source comment above `library::saved` says arrays are left out, but its actual match accepts `%`/`?`; follow code, not that stale comment.

## Init and persistence_changed completion

Use phase statuses such as `absent`, `compile_disabled`, `not_started`, `started`, `completed`, `faulted`, `budget_stopped`, `waiting`, `deferred`, `dropped`, with phase-specific safe diagnostic counters. Do not collapse absence, compiler-disabled entry, and completed callback. Count completion at a callback return/VM yield, not from an importer return or a UI artifact.

**V2:** `eval.rs:147` creates one evaluator with INIT_FUEL=200,000,000 (`:28`). `:174` calls `e.block(init.body)?`; an init evaluation fault propagates. Restore follows at `:180`. `:187` calls the persistence body, but **catches its error**, warns with an internal `on persistence_changed at load:` prefix, and still returns `Ok(initial)` at `:195`. `Script::warnings()` includes these init warnings (`lib.rs:782`). Therefore `compile_with().is_ok()` or `init_engine_pars().is_ok()` is **not** persistence completion. `init_engine_pars` discards the warning collection entirely. Warning recording caps at 1,000 (`eval.rs:211`), so even absence of the prefix cannot prove completion. Add scanner phase counters at the existing `e.block` boundaries before warning suppression; internally classify a fault, then discard free-form text. Both phases consume the same evaluator fuel; its budget message says "on init" even when the persistence phase exhausted it. Preserve the existing incorrect `$NI_CALLBACK_TYPE` behavior while instrumenting; changing it would be a product fix.

**V1:** `runtime.rs:1049` `Runtime::load` invokes `load_slot`. At `:1265` it executes `run_init`, restores persistent state, then `:1275` spawns `Callback::PersistenceChanged` and calls `settle`. `run_init` (`:1288`) treats Yield::Done as completion; Yield::Wait errors; Yield::OutOfFuel/OutOfTime records a stopped-init fault **and returns Ok** (`:1322`). Thus `Runtime::load().is_ok()` is not proof of full init completion. Instrument the actual yield there. Persistence runs through `spawn_cb` (`:2756`) and `resume` (`:2789`): Yield::Done finishes; Wait suspends; callback-cap exhaustion faults/finishes; block/time exhaustion queues continuation (`:2832–2876`). A missing entry or callback-pool drop is not completion, and `settle` return need not mean a suspended persistence callback finished. Track its phase through these existing paths.

`Runtime::faults()` exposes static-message `LiveFault {slot,line,message,context,last_action,count}` (`v1 runtime.rs:191`, `:1630`) and `fault_occurrences_omitted()`. `faults().count()` counts distinct retained fault records; summing `.count` is a different occurrence metric. Static messages are safe; do not dump arbitrary action operands, variable names, or formatted diagnostic strings. Generic array-bounds/PGS records are not necessarily fatal callback termination. Our 140 NKI slots with retained load faults cannot be renamed 140 failed init callbacks; sampled fault kinds were bounds and PGS, not budget stops.

Executed authored evidence: `audit_followup.rs:10` verifies native-menu restore and numeric repeated-tail restoration before a one-statement persistence callback sets a label to `-3` (passes); `strings.rs:135`, `:157` verify persisted arrays/tails (pass). `audit.rs:264` `persistence_callback_has_its_own_type` executes persistence code but observes INIT type 0 instead of 11 (fails). Three follow-up persistence edge probes fail, but they test restoration policy, not callback termination. No per-phase whole-corpus completion rate was measured by the old isolated probe; that is precisely the shared extension requested here.
