# Architecture and open-source reference review

The user requires ongoing comparison with the supplied architecture and other
open-source samplers (2026-10-06). For each subsystem, read its governing contract,
trace a relevant pinned implementation and its tests, and convert applicable edge
cases into native regression tests or explicit remaining acceptance criteria.
Record intentional semantic differences. Reference code is evidence, not the
authority for KONTRA ownership or proof of Kontakt compatibility. Inspect licenses
before any reuse; the review below copies no implementation into the native core.

## Pinned inspections

| Reference | Revision | Inspected scope |
| --- | --- | --- |
| [LinuxSampler historical mirror](https://github.com/linuxsampler/linuxsampler/tree/104e4535d9546deb78bbe0d981aac3b4a5766dcc) | `104e4535d9546deb78bbe0d981aac3b4a5766dcc` (2020-05-19; not current upstream) | `src/scriptvm/tree.h:705–748`, `tree.cpp:366–393`, `src/engines/EngineBase.h:978–1012,1070–1090,1128–1148`, `common/InstrumentScriptVM.cpp:113–145`; pooled contexts and note/release state retention |
| [sfizz](https://github.com/sfztools/sfizz/tree/f5c6e29f23b8057867c08e88f5f6ac6738baa30b) | `f5c6e29f23b8057867c08e88f5f6ac6738baa30b` | `src/sfizz/Layer.cpp`, `MidiState.cpp`, `RegionStateful.cpp`, `tests/MidiStateT.cpp:22–108`; controller, release, reset and block-state code/tests |
| [Shortcircuit XT](https://github.com/surge-synthesizer/shortcircuit-xt/tree/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a) | `8785f09acd9f93682ce4f754fac1d3c62e5b1a9a` | `src/scxt-core/dsp/generator.cpp`; sample windows and fractional crossfade reads |

Retrieved source and repository tree metadata are cached under ignored
`artifacts/sampler-references/`; immutable source links are the reproducible record.
This is a focused source review, not execution or certification of either engine.

## Findings and native obligations

| Source observation | Governing native contract / decision | Evidence or remaining check |
| --- | --- | --- |
| sfizz [`Layer.cpp:111–128`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/Layer.cpp#L111) distinguishes key release from pedal-delayed release. | Architecture §7.2: physical key, effective gate, source EOF and retained selection context remain independent. | `tests/release_selection.rs`, `tests/articulation.rs` exercise both phases and attack EOF. Full SFZ release equivalence is unverified. |
| sfizz [`Layer.cpp:133–179`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/Layer.cpp#L133) updates CC conditions separately from evaluating a controller-triggered region. | Architecture §6.1 and `ARTICULATION_POLICIES_05`: a CC predicate gates note selection; it does not implicitly generate a note. | `tests/controllers.rs` now checks audible predicate selection and no advancement for ineligible gestures. Controller-triggered ownership remains open. |
| sfizz [`MidiState.cpp:88–127`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/MidiState.cpp#L88) retains the final event value across block boundaries and reserves event storage when block size changes. | Architecture §5.2–5.3: stable sample-time order and separate raw/downstream state. Native storage is construction-bounded; no resize on audio. | `tests/controllers.rs` checks exclusive-end/empty-block updates, one-bit 32-bit distinctions, rejection atomicity and heap-free retained snapshots. Script interception remains open; the effective bank is not presented as raw input history. |
| Shortcircuit [`generator.cpp:318–338`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/dsp/generator.cpp#L318) documents a former one-sample mismatch between main and crossfade interpolation; mirrored reads can need a different fractional phase. | Architecture §8.3 source boundaries; phase-aligned interpolation on both crossfade legs. | **Open:** crossfade implementation must test fractional forward/reverse endpoints, mirrored partner phase, nonunity rates, one-frame loops, stereo coherence and block partitioning. Native ping-pong traversal now has separate unrolled/analytic boundary tests; crossfade partner tests remain open. |
| Shortcircuit [`generator.cpp:46–92`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/dsp/generator.cpp#L46) separates advancing the cursor from constructing an interpolation window; loop padding differs from EOF padding. | Source views and interpolation must agree on first-pass, looped and released topology. | Existing source/resampling tests are partial. Extend explicit boundary checks for each new playback mode, including arbitrary seek/offset and high-ratio multi-wrap reads. |

## Review coverage still required

The complete core needs more than the inspections above. V2 task acceptance must
retain explicit tests and measurements for:

- DSP fundamentals: finite/denormal behavior, silence and DC, alias rejection,
  source-rate conversion, channel layout, extreme valid rates, envelope endpoints,
  parameter discontinuities and smoothing, feedback stability, bus tails and latency.
- Musical edges: repeated key identity, controller consumption, sustain/sostenuto
  overlap, half pedal and repedal, release bursts, legato priority/transition state,
  release velocity, tuning, MPE member reuse and MIDI 2 per-note identity/precision.
- Runtime stress: bounded work/storage, streaming underrun and recovery, reverse
  streaming, source replacement with held notes, voice stealing without stranded
  callbacks, terminal backpressure, transport changes and UI-closed recall.
- Advanced processing: loop crossfades/ping-pong, stretch/granular phase and latency,
  sample-accurate modulation, filter/effect automation, multichannel/multimic coherence.

Pin the relevant implementation and tests when each area is developed. Use official
protocol and Kontakt/KSP documentation plus reference observations for vendor
semantics; open-source similarity cannot establish full KSP parity. Keep original
reference documents and all 128 scenario records unchanged, and record actual
execution evidence separately in the subsystem documents and conformance map.

## Script-state reference review

The historical LinuxSampler mirror allocates execution contexts when loading a
script, separates resetting execution from resetting polyphonic data, and retains
completed note-handler contexts when their data is needed by release handlers.
Its [release dispatch](https://github.com/linuxsampler/linuxsampler/blob/104e4535d9546deb78bbe0d981aac3b4a5766dcc/src/engines/EngineBase.h#L978)
walks retained per-key script events; this is an inspected historical policy, not
a statement about current upstream or Kontakt. No external engine test was run.
Source and its COPYING file were inspected; no implementation was copied.

Architecture §12.1–12.2 and the NI manual govern the new implementation. Native
polyphonic storage belongs to a generational logical note, independently of callback
slot completion. A physical input pairs with that note, including repeated keys,
ports and performance domains. Release admission uses its reserved continuation;
it does not recycle a still-running note callback or infer identity from key alone.
Tests in `sampler-core/tests/behavior.rs` and `sampler-ksp/tests/compile.rs` cover
concurrent note/release waits, same-key distinct owners, source EOF, completion
backpressure, slot reuse, plan replacement and bounded failure. NKSP's broader
local/real variable extensions are not treated as KSP features.

## Existing KONTRA implementations as migration references

User direction on 2026-10-06 explicitly includes the v1 Kontakt and Falcon paths.
Inspect their real inputs, tests and retained evidence before implementing each
corresponding v2 service. Preserve useful behavior and regressions; do not treat
legacy output as the vendor oracle, import its ownership model wholesale, or add
a runtime bridge. Re-author fixtures against native services and the new frontend.

Reviewed source checkpoints:

- Kontakt: this worktree at `5e5340d`; `src/import.rs`, `src/ksp/runtime.rs`,
  selected `src/ksp/tests.rs` and the existing streaming test. Legacy source is
  unchanged at that checkpoint; native additions coexist only for development.
- Falcon/UVI: clean separate worktree
  `/home/derpcat/.codex/worktrees/kontakto-uvi-latest`, branch
  `codex/uvi-latest-integration`, commit
  `4bffbb18b867b0a8e84b435d11684784b237693c`. This tree has the implementation
  absent from the v2 branch. It is read-only reference material here, not merged.
  Review `src/uvi/program.rs:21–99,165–210`, `script.rs:1534–1574,2668–2699,2937–2972`,
  `player.rs:438–510` and hosted ownership regressions at `script.rs:6286–6335,6474–6532`.

| Salvage target | Concrete v1 evidence | Native obligation / status |
| --- | --- | --- |
| Container/resource semantics | Kontakt `src/import.rs:632–656,724–780,874–966`: linked/saved script preference, encodings, file tables, IR/resource dependencies and load warnings | Preserve source identity and diagnostics in source→semantic→prepared compilation. The current Latin-1 fallback is only an approximation of Windows-1252; reimplement correct decoding, including non-ASCII punctuation. Import migration remains open. |
| Ordered script stages | Kontakt `events_pass_through_slots_in_order` and `runtime.rs:2918–2999`: changes/suppression affect later slots; controller and note forwarding differ | Raw input, stage-visible state and committed engine state need separate contracts. New frontend still has one script table; slot forwarding and controller interception remain open. |
| Retained note state | Kontakt `a_released_parent_survives_its_waiting_note_callback`, `midi_channels_survive_waits_and_release_independently`; UVI exact-root/FIFO overlap, consumed-root and delayed-descendant tests | Native note/callback/cell retention is exercised, including terminal pressure and slot reuse. Add source-level channel and generated-handle fixtures as those APIs land; do not derive ownership from key alone. |
| Release selection and cleanup | Kontakt `release_callbacks_select_groups_for_their_generated_notes`, `a_waiting_ignored_release_finishes_state_cleanup_after_sound_off` | Group eligibility and script-state cleanup across ignored releases remain KSP obligations. Current native hard cleanup suppresses/cancels handlers; that policy is not proof of Kontakt cleanup parity. Preserve this difference explicitly until reference semantics are implemented. |
| UVI graph identity | `program.rs` preserves node parents, local oscillator values, raw units, connections and initially bypassed unsupported nodes | Preserve semantic hierarchy and required dormant capabilities; compile it to bounded native scopes/indices. Do not flatten Layer/Program gains into a voice or silently omit nodes. |
| UVI callback priority | Both `script.rs` dispatch paths choose `onEvent` before specialized handlers, with explicit `postEvent` forwarding | Recreate the dispatch/forwarding tests in the new frontend. Lua syntax support is not the UVI runtime contract. Native UVI execution remains open. |
| Source topology/streaming | `tests/playback.rs:842–956` compares RAM/streamed float and 24-bit PCM, offsets, pitched crossfades, reverse and alternating loops | Retain the authored input matrix; compare independently expected boundaries and PCM too. Both v1 paths can share a defect. New-core streaming and advanced loop modes remain open. |
| DSP defaults and clocks | UVI `UVI_PANLAW_DEFAULT_EVIDENCE.md`, `UVI_LFO_SMOOTH_EVIDENCE.md`, leaf comparison reports | Loaded defaults can differ from descriptor defaults; control lookahead must not commit future state. Carry exact reference version, operation order, parameters and successful-return scope into new fixtures. Reports are historical evidence, not new v2 passes. |
| Worker failure and timing | UVI `UVI_RUNTIME_QUEUE_INVESTIGATION.md`, `UVI_IMPLEMENTATION_STATUS.md` | Preserve first-cause reporting, owner-stamped completion and terminal cleanup. Bounded queues do not prove sustainable throughput; do not transplant the allocating worker/Lua/DSP bridge as the new realtime core. |

The legacy KSP test `computed_ui_and_execution_limits` explicitly accepts unknown
functions as diagnostic no-ops returning zero. That tolerance must not silently
turn missing v2 services into apparent support. Likewise, the UVI status reports
620 decoded Augmented Orchestra programs but zero admitted complete graphs at its
recorded census. Decoding, execution and fidelity remain separate gates.

Immediate fixture migration order: controller suppression/remapping and captured
callback channels; staged note/release forwarding; generated-event handles and
group eligibility; cleanup/state continuity; persistence/UI-independent state;
then vendor source graphs and DSP/streaming matrices. This supplements the full
KSP inventory and v2 task dependencies rather than substituting feature counts.

Reference execution in this worktree (2026-10-06): the four focused v1 tests
`controllers_can_be_filtered_and_remapped`, `events_pass_through_slots_in_order`,
`a_waiting_ignored_release_finishes_state_cleanup_after_sound_off` and
`a_released_parent_survives_its_waiting_note_callback` each passed under the locked
`ci` profile. Logs are retained in ignored `artifacts/v1-reference-*.log`. These
establish reproducible legacy regression inputs, not Kontakt equivalence or v2
coverage. The separate UVI checkout was inspected only; no tests were run there.


### Reflected-loop follow-up

The pinned Shortcircuit generator at `8785f09...` was additionally inspected at
[`generator.cpp:626–669`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/dsp/generator.cpp#L626)
(multi-turn reflection and separate initial/travel direction) and
[`generator.cpp:849–868`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/dsp/generator.cpp#L849)
(gated-loop exit and no continuing loop crossfade after exit). Its MIT license
was reviewed; no implementation was copied. The v1 authored unrolled fixture at
`tests/playback.rs:432–460` was also inspected. New native tests recreate independent
forward/reverse integer traversals, fractional reflection and high-rate multi-turn
reads in `tests/source.rs` and `tests/resample.rs`. These test native declared
semantics, not Shortcircuit/Kontakt/Falcon parity. Crossfade partner phase remains open.

## Envelope follow-up

Reviewed pinned sfizz `ADSREnvelope.cpp:122–180` and `tests/ADSREnvelopeT.cpp:32–112`
([implementation](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/ADSREnvelope.cpp#L122),
[tests](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/tests/ADSREnvelopeT.cpp#L32)).
Its BSD-2-Clause license was inspected; no implementation was copied or external
suite executed. Its attack publishes an advanced value; the native KONTRA contract
publishes the initial zero and reaches endpoints at the exclusive duration. Its
release uses an exponential threshold: native completion instead uses an exact
frame duration. Preserve these differences rather than treating all ADSRs alike.
Reference delay and release-in-attack cases inform the new native analytic tests.
This DSP inspection does not resume SFZ frontend work, which remains deferred.

Also inspected v1 `src/engine/ahdsr.rs:27–158` and UVI reference checkout
`src/uvi/modulation.rs:339–455`. Kontakt's older implementation uses a 32-frame
control clock, rounded f32 coefficients and a nonlinear source-parameter mapping;
UVI's DAHDSR uses a different curve map and carries fractional stage-clock overflow.
These observations are migration requirements, not new vendor measurements. Native
stage curves/delay/AHD are now executable, but vendor clock/parameter interpretation
and exact fixture migration remain open. Do not call the native curve unit either
vendor's curve unit or silently substitute audio-rate timing for an authored profile.

## Integer evaluator follow-up

Inspected v1 `src/ksp/parser.rs:144–155`, `src/ksp/vm.rs:681–714` and the authored
`integer_division_and_modulo_normalize_booleans_without_faulting_or_losing_waited_events`
fixture at `src/ksp/tests.rs:1246–1361`. The old evaluator explicitly wraps i32
operations and returns zero on zero division/remainder. The new implementation uses
Rust's defined wrapping primitives and independent i128 expected results in tests;
no old VM types or execution path are referenced. The v1 test was inspected, not
rerun for this change, and its vendor-behavior comments are not new reference evidence.
Numeric edge fidelity and full KSP expression semantics remain separate parity gates.


## Voice processor ownership and routing

For native voice-chain work, inspected pinned Shortcircuit
[`voice.cpp:69–105`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/voice/voice.cpp#L69)
for processor cleanup ownership and
[`voice.cpp:634–648`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/voice/voice.cpp#L634)
for explicit mono/stereo chain layout. No implementation was copied and these external
paths were not executed. Native state instead lives in control-prepared per-generation
banks with per-voice/channel history, retains DSP tails and leaves via the existing
retirement queue. Channel-layout expansion remains open; current processing is stereo.
[VOICE_DSP.md](VOICE_DSP.md) records implemented behavior, numerical sources and tests.


## Original note forwarding

Inspected existing `src/ksp/runtime.rs:2893–2953` (`finish`, `yielded`, note arm of
`forward`) for first-yield/completion dispatch and suppression. This is a regression
reference, not a vendor oracle or code port. The new core shares attack selection
preflight/commit and records mapping phase on the original logical note; KSP lowers
explicit forward/suppress instructions. Read the NI event-command restrictions on
pre-wait note mutation and its post-wait zone-ID example. New native/source tests
execute independently with high-resolution velocity, live expression, capacity,
release reserve and old-generation ownership checks. Multi-slot/release/controller
forwarding remains required, and no legacy runtime path is called.


## Note edits across the first wait

Re-read the pinned Kontakt 8.12 `change_note`/`change_velo` documentation: pre-wait
changes affect the attack; later changes still update the event variables. Inspected
legacy `src/ksp/calls.rs:577–592` and `src/ksp/vm.rs:1250–1268` at the previously
recorded source pin. Legacy calls clamp values and skip edits after `at_engine`,
while system-variable reads use the stored event fields. The new core deliberately
separates admission, script-visible and committed values instead of preserving that
legacy discrepancy. Native range validation is explicit and needs vendor probes.
`forwarding.rs` and KSP `arguments.rs` check this distinction with independent PCM
expectations and heap guards. No fresh Kontakt/Falcon execution comparison was run.


## Integer arrays and constants

Read the [NI variables reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables)
for signed-32 elements, the million-element maximum, constant dimensions and
last-initializer repetition. Inspected legacy `src/ksp/compile.rs:1129–1213`: it
folds literal initializers into prepared data and repeats the final computed
initializer through the tail. This comparison is a regression reference, not a
vendor oracle. The new compiler instead prepares constant values off audio and
uses bounded views into the existing native instance bank. No legacy VM is called
and no external implementation was copied. Zero-length rejection and invalid-index
fault behavior require vendor probes; native memory safety and ownership are tested.
