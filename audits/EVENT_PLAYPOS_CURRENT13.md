# EVENT_PAR_PLAY_POS: CURRENT13 plan and synthetic fixtures only

Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5`, tree
`4af7c9babd93c49cdb0a026a63d25e6c433498b0`. No product edits.
Sibling event-zone lane owns `script_params.rs`; the spans below are proposals,
not reservations or edits. No automatic merge. Root checkout was not edited.
No cargo/rustc/clippy/build/test executables, native apps, proprietary inputs,
perf/host jobs, installations, publication, wrappers, delegates or schedules.
Rust tests: **NOT_RUN / NOT_COMPILED**. Native: **UNKNOWN**.
CPU AND RAM lower than BOTH references: **UNACHIEVED**.

## Primary authority, not project claims

Archived NI [Events and MIDI](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants#events-and-midi)
says this is a **get_event_par-only** parameter:
“Returns the absolute position of the play cursor within a zone in microseconds”.

Archive root:
`/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/`.
Verified exact cached primary hashes:

- `official-docs/ksp-variables.html`: SHA256
  `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e`.
- `round13-selected-sections/ksp-variables-events-and-midi.txt`: SHA256
  `ac32835f066694494c6481952980781dbe7c0346f1e40072e89934b78387ee51`.

`round13-event-consumer-requirements.md/json` describes historical source3665.
It supplies the question and cached authority, not CURRENT13 proof or test credit.
ZONE_ID's highest-zone rule is separately documented; it does **not** establish
which voice PLAY_POS selects. No new fetch or native execution was done.

## Exact CURRENT13 retrace / callers

All paths and lines refer to the base above. The fixture checker pins complete
file hashes so these conclusions cannot silently apply to a changed snapshot.

| Current source | Concrete finding |
| --- | --- |
| `crates/sampler-ksp/src/builtins.rs:480,616` | Numeric selector 12 and symbolic `$EVENT_PAR_PLAY_POS` registered. Numeric ABI is this implementation's mapping, not proven by the prose. |
| `crates/sampler-core/src/lib.rs:198-215` | `EventInfo` has no play-position variant. |
| `crates/sampler-ksp/src/lower.rs:1930-1979,2244-2305,2881-2888` | `Lower::builtin` routes metadata/custom/gain getters but no constant PLAY_POS; `event_param` covers only volume/tune/pan. Computed selector emits `ReadEventParameter`. |
| `crates/sampler-ksp/src/lower.rs:2788-2812` | Unmatched constant getter reaches warning/result 0, not a generic Host getter. |
| `crates/sampler-core/src/behavior.rs:2380-2442` | `ReadEventInfo` calls `read_event_info`; dynamic `ReadEventParameter` handles 7/8/10/11/13/15, omits 12, then returns 0. |
| `crates/sampler-core/src/script_params.rs:801-845` | `Runtime::read_event_info` resolves plan-qualified source alias, then metadata. ZONE walks note families/voices and returns first nonstolen source_zone; it neither reads a cursor nor supplies PLAY_POS policy. |
| `crates/sampler-core/src/note_event.rs:123-136` | `resolve_source_event` checks plan handle, original generational note and plan; unknown/retired/other-plan note is None, stale/foreign plan is an error. Do not bypass it using host IDs, indices or MIDI-object markers. |
| `crates/sampler-core/src/lib.rs:505-534` | Each `Voice` owns sample index, cursor, family, source_zone, started/stolen flags. No event-wide source cursor/rate. |
| `crates/sampler-core/src/prepare.rs:192-194,411-415` | `Pcm::sample_rate()` is asset rate; prepared PCM is indexed by voice.sample. |
| `crates/sampler-core/src/prepare/selection.rs:939-996` | Actual attack admission applies source_offset_micros with prepared PCM rate, then source-frame start modulation, then pitch step; release cursor follows a different admission path. |
| `crates/sampler-core/src/source.rs:113-127,311-360` | Ordinary step = source_rate/output_rate * pitch ratio. Cursor position is unsigned traversal distance plus fractional phase. `with_offset` converts micros using source rate, not output or pitch time. |
| `crates/sampler-core/src/source.rs:450-456,478-504,685-711,797-810` | trace_position maps integer traversal through serial/count/loop/reverse geometry and discards fraction. Invalid index falls back to start for tracing; that is not a valid play-position result. Fraction advances independently to avoid loss above 2^53. |
| `crates/sampler-core/src/source.rs:712-764,814-852,1007-1013` | Crossfade has two addresses; ping-pong reverses local direction. Adding fraction using initial Direction alone is wrong on reflected spans. Boundaries need explicit mapping, not interpolation across a wrap jump. |
| `crates/sampler-core/src/source.rs:605-673` | Cold onset holds offset without advancement; later starvation can advance silently. Host elapsed time does not describe both. |
| `crates/sampler-core/src/render.rs:700-722,916-938` | Trace caller records source_frame from integer trace_position; rendering calls per-voice cursor.render (direct or chain). Neither trace nor output clock is a microsecond query seam. |

Only production callers of `read_event_info` are the two instruction branches
in `behavior.rs` above. `ReadEventInfo` producers are lower.rs:1946,1973,2141;
`ReadEventParameter` producer is lower.rs:2299. No caller edits in this lane.

## Minimum implementation plan (conditional, not implemented)

1. **Resolve the policy gate first.** The primary specifies units/read-only status,
   not event-to-voice reduction, fractional integer rounding, trim origin, reverse
   endpoint conventions, crossfade leg, wavetable position, or no-voice return.
   A safe full native-compatible scalar cannot be completed from this evidence.
   Restrict an initial own implementation to one eligible ordinary forward PCM
   voice away from boundaries, or explicitly report unsupported ambiguity. Such
   a restriction/error/result is an own policy, not an asserted NI law.
2. **Source seam: `source.rs`.** Add a non-mutating internal accessor returning
   optional mapped integer source frame plus fractional phase/local direction.
   Keep traversal integer precision and reuse index/segment geometry. Do not change
   rendering or trace_position. Start with the admitted subset; no `index=None`
   fallback to start. Preserve fraction before conversion. Do not convert the whole
   traversal to f64 or interpolate index(n) to index(n+1) across a discontinuity.
   Expand reverse/wrap/reflection/count/serial/crossfade/wavetable only after each
   mapping and boundary convention has an explicit policy and fixture.
3. **Owner seam: `script_params.rs` (sibling integration only).** Resolve alias
   through `resolve_source_event`, inspect that note's bounded family/voice lists,
   and pair selected cursor with `plans[plan].prepared.pcm[voice.sample].sample_rate()`.
   Decide started/stolen/tailing/cold-held eligibility explicitly. Do not select
   highest source_zone, first linked voice, fastest voice, minimum or average by
   analogy. No new voice cache or event-shared cursor needed. Per-query scan
   O(owned families + voices), O(1) scratch, no allocations, locks, IO or RT logs.
4. **Units seam.** For an admitted mapped source coordinate x and source rate r,
   unrounded units are x * 1_000_000 / r. Pitch affects cursor advance, not divisor.
   Account for view origin only after explicit coordinate policy. Use checked wide
   integer quotient/remainder for frame time plus a bounded fractional contribution;
   never round frames first or cast a large complete frame index to f64. Validate
   r > 0 and range before narrowing to the runtime i64 result. Native rounding and
   result width are not established; choose/document an own rule if required.
5. **Dispatch seams: `lib.rs`, `lower.rs`, `behavior.rs`.** Add `EventInfo::PlayPosition`,
   route constant symbolic/literal 12 to `ReadEventInfo`, and dynamic 12 to the same
   read service. Preserve plan/stage/local validation and other selectors. No setter,
   generic Host effect, trace dependency, async service, manifests or wrapper edits.
   Coordinate the enum and script_params match exhaustiveness with the sibling.
6. **Integration fixtures, later authorized execution only.** Static symbolic,
   literal and variable-held 12 must agree after voice admission; exercise generated
   IDs after wait as well as host-created events. Cover two asset rates, pitch change,
   nonzero start/offset, loop/release boundaries, stale generation/foreign plan,
   cold hold versus silent advancement, and multi-voice ambiguity. Keep existing
   pending query fixture; its expected 12=0 is own pending behavior, not native law.
   No runtime tests have been executed here.

## Independently authored synthetic fixtures and limits

`tools/check_event_playpos_current13.py` is stdlib Python only. It verifies the
pinned source snapshot and evaluates independent exact rational arithmetic,
not Rust and not the native engine. Candidate geometry labels deliberately
separate mathematical inputs from unsettled PLAY_POS return behavior.

- Zero-origin forward PCM: 125 us at 44.1 kHz gives 5 + 41/80 source frames.
  Trace integer converted to time is 50000/441 us, not 125 us.
- 240 output frames at 48 kHz: source coordinates 220.5 frames at 44.1 kHz
  versus 480 frames at 96 kHz both convert to 5000 us. Octave-up first voice
  reaches 441 source frames = 10000 us. Output elapsed time remains 5000 us.
- Nonzero trim, reverse, wrap, reflected ping-pong and dual-address crossfade
  fixtures retain explicit candidate coordinates; they do not assert native values.
- Two voices with different source rates/offsets produce 125 versus 1000 us:
  highest-zone ID, min, max or average cannot be inferred from unit conversion.
- A half source frame at 44.1 kHz is 5000/441 us: floor 11 versus nearest 11;
  three-quarter frame is 2500/147 us: floor 17 versus nearest 17. One source
  frame is 10000/441 us: floor 22 versus nearest 23, exposing rounding choice.
- Large exact frame arithmetic retains a quarter-frame at 2^54. End/no-zone,
  deferred, stolen/tail, wavetable and crossfade/loop-boundary native answers remain
  UNKNOWN, without invented zero, -1, saturated result or winning voice.

Existing Rust fixture names inspected, all **NOT_RUN / NOT_COMPILED** here:
`dynamic_event_parameter_reads_match_static_fields_and_pending_projection`
(selection_contract.rs:717-742, own pending selector12=0),
`offsets_preserve_source_time_and_bounds_without_advancing_loop_passes`
(source.rs:1512-1561), `fractional_phase_survives_large_positions_and_guard_mapping`
(source.rs:1565), `an_unpreloaded_start_offset_waits_without_skipping_or_changing_direction`
(cold_offset.rs:133), and `staccato_released_before_late_onset_plays_its_correct_offset_then_releases`
(cold_offset.rs:138). Historical execution receipts were not promoted to CURRENT13.

No official ledger, sections.json or manifest was edited. The only implementation
in this deliverable is the standalone synthetic/static Python checker. This plan
is blocked on policy/evidence for a complete scalar native PLAY_POS contract,
not on missing access to the per-voice cursor or source rate.
