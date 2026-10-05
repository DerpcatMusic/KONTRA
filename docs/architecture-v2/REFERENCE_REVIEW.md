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
| [sfizz](https://github.com/sfztools/sfizz/tree/f5c6e29f23b8057867c08e88f5f6ac6738baa30b) | `f5c6e29f23b8057867c08e88f5f6ac6738baa30b` | `src/sfizz/Layer.cpp`, `MidiState.cpp`, `RegionStateful.cpp`; controller, release and event-state code |
| [Shortcircuit XT](https://github.com/surge-synthesizer/shortcircuit-xt/tree/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a) | `8785f09acd9f93682ce4f754fac1d3c62e5b1a9a` | `src/scxt-core/dsp/generator.cpp`; sample windows and fractional crossfade reads |

Retrieved source and repository tree metadata are cached under ignored
`artifacts/sampler-references/`; immutable source links are the reproducible record.
This is a focused source review, not execution or certification of either engine.

## Findings and native obligations

| Source observation | Governing native contract / decision | Evidence or remaining check |
| --- | --- | --- |
| sfizz [`Layer.cpp:111–128`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/Layer.cpp#L111) distinguishes key release from pedal-delayed release. | Architecture §7.2: physical key, effective gate, source EOF and retained selection context remain independent. | `tests/release_selection.rs`, `tests/articulation.rs` exercise both phases and attack EOF. Full SFZ release equivalence is unverified. |
| sfizz [`Layer.cpp:133–179`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/Layer.cpp#L133) updates CC conditions separately from evaluating a controller-triggered region. | Architecture §6.1 and `ARTICULATION_POLICIES_05`: a CC predicate gates note selection; it does not implicitly generate a note. | Controller predicates and controller-triggered ownership require separate positive tests. Merely storing CC values does not complete either. |
| sfizz [`MidiState.cpp:88–127`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/MidiState.cpp#L88) retains the final event value across block boundaries and reserves event storage when block size changes. | Architecture §5.2–5.3: stable sample-time order and separate raw/downstream state. Native storage is construction-bounded; no resize on audio. | `tests/controllers.rs` checks exclusive-end/empty-block updates, one-bit 32-bit distinctions, rejection atomicity and heap-free retained snapshots. Script interception remains open; the effective bank is not presented as raw input history. |
| Shortcircuit [`generator.cpp:318–338`](https://github.com/surge-synthesizer/shortcircuit-xt/blob/8785f09acd9f93682ce4f754fac1d3c62e5b1a9a/src/scxt-core/dsp/generator.cpp#L318) documents a former one-sample mismatch between main and crossfade interpolation; mirrored reads can need a different fractional phase. | Architecture §9.1 source boundaries; phase-aligned interpolation on both crossfade legs. | **Open:** crossfade/ping-pong implementation must test fractional forward/reverse endpoints, mirrored partner phase, nonunity rates, one-frame loops, stereo coherence and block partitioning. Current exact-wrap tests do not satisfy this. |
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
