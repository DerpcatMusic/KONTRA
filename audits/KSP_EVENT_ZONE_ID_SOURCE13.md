# EVENT_PAR_ZONE_ID — source13 narrow consumer

Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5`. Source-only implementation.
**All authored Rust: NOT_RUN / NOT_COMPILED. Native UNKNOWN. CPU AND RAM versus BOTH references UNACHIEVED.**

## Primary authority, not project claims

NI [Events and MIDI](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants#events-and-midi), archived `ksp-variables.html` SHA256 `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e`; exact archived section SHA256 `ac32835f066694494c6481952980781dbe7c0346f1e40072e89934b78387ee51`. Both local bytes verified. Archive and excerpt are in the coordinator handoff, unchanged.

> Returns -1 if no zone is triggered. Returns the highest zone ID if more than one zone is triggered by the event.

The same section says active events only and to run the voice first, e.g. `wait(1)`. Unversioned cached manual; no native timing, invalid-ID, stolen/cold/tailing eligibility law inferred. Historical round13 requirements describe source3665; every path below was retraced on exact source13.

## Production change and callers

- `crates/sampler-core/src/script_params.rs::Runtime::read_event_info`: after existing signed32 conversion and original-plan alias resolution, scan all linked note families and voices, excluding existing `stolen` voices; reduce `state.source_zone` by maximum, initialized to -1. No eligible voice on a live resolved event returns -1. Unknown/retired/out-of-range aliases retain 0. Stale/foreign PlanId errors remain unchanged.
- Static producer: `crates/sampler-ksp/src/builtins.rs` registers `$EVENT_PAR_ZONE_ID` as selector10; `lower.rs:1953-1980` emits `ReadEventInfo { info: ZoneId }`.
- Computed-selector producer: `lower.rs:2294-2306` emits `ReadEventParameter`; `crates/sampler-core/src/behavior.rs:2380-2444` routes numeric10 to the same `read_event_info`. Selector12 PLAY_POS is untouched.
- Alias path: `note_event.rs:108-175` exports non-repeating positive aliases and resolves the originating plan plus generational NoteId. `sampler-ksp/src/lib.rs:828-830` retains the reserved 28-bit alias limit. Host external IDs, script slots, and marks are not source aliases. Allocation/exhaustion logic unchanged.
- Physical ID path: `groups.rs:27-33` retains zone IDs by region; `prepare/selection.rs:990-999` writes them into admitted voices, using the existing region+1 fallback only when no source map is supplied. `prepare.rs:982-990` preserves holes in source-zone lookup. No new dense ordinal lookup introduced.
- Lifecycle: `lib.rs:1732-1773` prepends admitted voices; `ownership.rs:320-340` prepends families. `ownership.rs:358-388` leaves ordinary release tails linked until done; `end_voice:528-554` and `retire_family:506-526` unlink retired nodes. `steal.rs:123,204,241` marks fade victims stolen. The query preserves that filter and all other eligibility, including cold/scheduled nodes. It does not accumulate zone history, descend child events, or mutate groups.
- Wait path: `lower.rs:2032-2044` forwards the stage before yielding; no voices are forced into existence by querying. A pending/unmapped/suppressed live event therefore reads -1 under this own policy.

The bounded scan is O(owned linked families + voices), no added storage or allocation. It can cost more than the former first-match return; this is not CPU/RAM acceptance evidence.

## Authored production-path witnesses

`sampler-ksp`, integration target `event_zone`, seven `#[test]` functions:

1. `highest_physical_zone_is_independent_of_voice_order_and_shared_sample`: three sparse-ID permutations, shared PCM, literal/computed selectors, pending-before-wait, source alias distinct from host ID.
2. `live_unmapped_or_suppressed_event_has_no_zone_after_wait`: live alias, zero voices, pending/after-wait -1.
3. `exhausted_voices_report_no_zone_until_alias_retirement_not_slot_reuse`: source exhaustion while held gives -1; accepted retirement and new admission leave cached old alias at 0, not the new voice.
4. `generated_alias_queries_its_own_zone_not_the_suppressed_parent`: child9001, parent-1, creating slot0; no cross-event voice scan.
5. `highest_zone_spans_attack_and_release_families_while_tails_remain`: low-ID release family at list head does not hide high-ID attack tail.
6. `stolen_fading_voices_do_not_supply_a_zone`: old live alias with stolen fade returns -1 despite linked physical voice.
7. `unknown_and_cached_group_selectors_keep_the_existing_zero_fallback`: unknown, signed32 overflow, cached ALL/marks retain0; no fanout expansion.

All seven wrap runtime actions in the existing `support::without_heap`. These are authored checks, not executed evidence.

## Affected historical witnesses and alias neighbors

- `sampler-ksp/strings::event_zone_id_is_nonzero_only_while_the_event_sounds` (274-288): executable assertions unchanged (nonzero live zone, unknown99999 ->0, channel0). Only misleading documentation corrected. Historical source3665 PASS is not transferred to this patch.
- `sampler-ksp/selection_contract::dynamic_event_parameter_reads_match_static_fields_and_pending_projection` (717-743): expected index10 changes0 -> -1 because no stage forwarding occurred; all other selectors unchanged, especially PLAY_POS index12=0. NOT_RUN here.
- All historical KSP test `get_event_par` occurrences were searched. No other literal ZoneId/selector10 assertion exists; other dynamic scalar fixtures read custom/source parameters. MIDI-object getters are a separate namespace.
- Alias guards in `sampler-core/event_ids` remain byte-identical: `source_ids_are_not_host_ids_and_never_alias_reused_note_slots_or_panic`, `source_id_lookup_requires_the_original_generation_and_runtime`, `source_id_exhaustion_is_explicit_and_does_not_exhaust_native_note_ownership`, `generated_result_aliases_are_preflighted_and_do_not_pin_completed_children`. Inspected source only, NOT_RUN here.

Related historical identities: `built-in-variables-and-constants:events-and-midi:symbol:$EVENT_PAR_ZONE_ID`, `...:symbol:$EVENT_ID`, `...:symbol:$EVENT_PAR_SOURCE`; `event-commands:get_event_par--:command:get_event_par`; `event-commands:get_event_ids--:command:get_event_ids`. This patch changes only the zone consumer, not their other contracts or published statuses.

## Validation and exclusions

`python3 tools/check_event_zone_source13.py`: exact narrow source diff and untouched producer/alias/lifecycle pins; 13 independent Python model cases; authored witness inventory. PASS means Python source/model checks only.

Installed `rustfmt`1.99.0 parses the edited Rust; new witness formatting checked. Syntax success is not typechecking. `git diff --check` clean. No cargo/rustc/clippy/build/tests/native applications/banks/perf/host/install/publish were run. No nested work. No target/wrapper or manifest/official-ledger edits. No automatic merge. Root frozen checkout untouched; private branch only.
