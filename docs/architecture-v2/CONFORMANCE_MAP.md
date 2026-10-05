# Conformance scenario allocation

This is a planning index of the supplied [catalogue](references/CONFORMANCE_SCENARIOS.json),
not a test result. All **128 original records retain `specified_not_executed`**; this
preserves the supplied source rather than serving as a live result database.
[Implementation evidence](IMPLEMENTATION.md) records the exercised subset separately. The catalogue
contains 100 proposed native contracts, 15 documented profile rules and 13 reference
probes. Its original bytes and statuses are preserved.

Each ID below has one primary implementation task. Tasks may share fixtures and
exercise related cases earlier; the milestone is the planned home for full work,
not a claim that every listed feature ships in its first slice. A declared unsupported
advanced capability cannot be counted as passing its positive behavior scenario.

- **native:** ratify the proposed KONTRA policy, then execute it.
- **documented:** pin the protocol/product/profile and primary documentation before testing.
- **probe:** capture reference observations; there is no supplied golden result to assume.

The scenario JSON contains stimulus, objectives, observations and source IDs. It is
not a machine-executable event script; initially implement small Rust fixtures for
selected IDs rather than a generic interpreter for natural-language specifications.
Results should be stored separately with case ID, outcome (pass/fail/blocked/unverified),
engine/build/profile, fixture hash, seed, rate, partition, command and artifact path.
No fabricated vendor observations or compatibility percentages.

V2-01 characterizes existing tests and reproduces risks before these implementations.
M1 exercises terminal admission/retry through a headless sink; V2-15 proves the actual
host adapter. M1's minimum KSP slice maps to KSP_RUNTIME_01–03 and relevant identity/
scheduling cases; the broader KSP probes belong to V2-14.

| Scenario ID | Title | Kind | Primary task | Gate |
| --- | --- | --- | --- | --- |
| `PROTOCOL_ADAPTERS_01` | MIDI 1 zero-velocity convention | documented | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_02` | CLAP zero-velocity note-on | documented | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_03` | Explicit-ID same-key overlap | documented | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_04` | CLAP wildcard match | documented | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_05` | Missing-ID repeated-key pairing | probe | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_06` | VST3 note tuning units | documented | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_07` | No double dispatch across protocols | native | V2-15 | M4 |
| `PROTOCOL_ADAPTERS_08` | High-resolution preservation | native | V2-15 | M4 |
| `NOTE_IDENTITY_01` | Transpose and release identity | native | V2-03 | M1 |
| `NOTE_IDENTITY_02` | Child-note ownership | native | V2-03 | M1 |
| `NOTE_IDENTITY_03` | Detached generated note | native | V2-03 | M1 |
| `NOTE_IDENTITY_04` | Stale handle after slot reuse | native | V2-03 | M1 |
| `NOTE_IDENTITY_05` | One-shot ends while key held | native | V2-03 | M1 |
| `NOTE_IDENTITY_06` | Family versus source targeting | native | V2-03 | M1 |
| `NOTE_IDENTITY_07` | Ownership admission refusal | native | V2-03 | M1 |
| `NOTE_IDENTITY_08` | Script error cleanup | native | V2-05 | M1 |
| `EVENT_SCHEDULING_01` | Within-block onset | native | V2-04 | M1 |
| `EVENT_SCHEDULING_02` | Block partition invariance | native | V2-04 | M1 |
| `EVENT_SCHEDULING_03` | Equal-time stable order | native | V2-04 | M1 |
| `EVENT_SCHEDULING_04` | Cancel before delayed onset | native | V2-04 | M1 |
| `EVENT_SCHEDULING_05` | Zero-time generator loop | native | V2-04 | M1 |
| `EVENT_SCHEDULING_06` | Tempo change during beat wait | probe | V2-04 | M1 |
| `EVENT_SCHEDULING_07` | Transport seek epoch | native | V2-04 | M1 |
| `EVENT_SCHEDULING_08` | Swallowed controller visibility | native | V2-04 | M1 |
| `ARTICULATION_POLICIES_01` | Velocity boundary and transform | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_02` | Latched keyswitch | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_03` | Momentary keyswitch | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_04` | Next-note switch consumption | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_05` | CC gate versus CC trigger | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_06` | Channel articulation with MPE | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_07` | Recorded interval legato | native | V2-08 | M2 |
| `ARTICULATION_POLICIES_08` | First and legato state definition | probe | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_01` | Coherent multimic round robin | native | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_02` | Independent release sequence | native | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_03` | Sequence scope isolation | native | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_04` | Seeded reproducibility | native | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_05` | Shuffle bag versus random | native | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_06` | Decent random distinction | documented | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_07` | Advancement on rejected regions | probe | V2-08 | M2 |
| `VARIATION_AND_RELEASE_SELECTION_08` | Release multiplicity with microphones | probe | V2-08 | M2 |
| `PEDALS_AND_GATES_01` | Sustain delayed gate release | native | V2-06 | M1 |
| `PEDALS_AND_GATES_02` | Sostenuto captured set | native | V2-06 | M1 |
| `PEDALS_AND_GATES_03` | Repeated key under sustain | native | V2-06 | M1 |
| `PEDALS_AND_GATES_04` | Half-pedal damping | native | V2-06 | M1 |
| `PEDALS_AND_GATES_05` | Repedal capture window | native | V2-06 | M1 |
| `PEDALS_AND_GATES_06` | Pedal release burst budget | native | V2-06 | M1 |
| `PEDALS_AND_GATES_07` | All-notes versus all-sound off | native | V2-06 | M1 |
| `PEDALS_AND_GATES_08` | Release without live attack source | probe | V2-06 | M1 |
| `VOICE_ALLOCATION_01` | Logical versus source polyphony | native | V2-03 | M1 |
| `VOICE_ALLOCATION_02` | Expensive engine budget | native | V2-09 | M2 |
| `VOICE_ALLOCATION_03` | Family stealing | native | V2-09 | M2 |
| `VOICE_ALLOCATION_04` | Bounded stealing fade | native | V2-09 | M2 |
| `VOICE_ALLOCATION_05` | Steal then release callback | native | V2-09 | M2 |
| `VOICE_ALLOCATION_06` | Natural source completion | native | V2-09 | M2 |
| `VOICE_ALLOCATION_07` | Shared bus tail ownership | native | V2-09 | M2 |
| `VOICE_ALLOCATION_08` | Release-priority reserve | native | V2-06 | M1 |
| `KSP_RUNTIME_01` | Polyphonic note state | documented | V2-05 | M1 |
| `KSP_RUNTIME_02` | Wait continuation | documented | V2-05 | M1 |
| `KSP_RUNTIME_03` | Generated note duration classes | documented | V2-05 | M1 |
| `KSP_RUNTIME_04` | Read after engine parameter write | probe | V2-14 | M4 |
| `KSP_RUNTIME_05` | Script-slot event propagation | probe | V2-14 | M4 |
| `KSP_RUNTIME_06` | Persistence and initialization order | probe | V2-14 | M4 |
| `KSP_RUNTIME_07` | Integer edge semantics | probe | V2-14 | M4 |
| `KSP_RUNTIME_08` | Unsupported API diagnostic | native | V2-07 | M2 |
| `UVI_RUNTIME_01` | Default and explicit event forwarding | documented | V2-18 | M5 |
| `UVI_RUNTIME_02` | onEvent callback precedence | documented | V2-18 | M5 |
| `UVI_RUNTIME_03` | Coroutine-local state | documented | V2-18 | M5 |
| `UVI_RUNTIME_04` | Widget versus coroutine context | probe | V2-18 | M5 |
| `UVI_RUNTIME_05` | Object hierarchy and indexing | documented | V2-17 | M5 |
| `UVI_RUNTIME_06` | Mapping dimensions are explicit | documented | V2-17 | M5 |
| `UVI_RUNTIME_07` | Async request versus completion | probe | V2-18 | M5 |
| `UVI_RUNTIME_08` | Lua pool and execution exhaustion | native | V2-18 | M5 |
| `SAMPLE_SOURCES_01` | Unity-ratio direct path | native | V2-09 | M2 |
| `SAMPLE_SOURCES_02` | Pitch and sample-rate ratio | native | V2-09 | M2 |
| `SAMPLE_SOURCES_03` | High-transposition alias rejection | native | V2-09 | M2 |
| `SAMPLE_SOURCES_04` | Loop boundary guards | native | V2-09 | M2 |
| `SAMPLE_SOURCES_05` | Reverse and ping-pong endpoints | native | V2-09 | M2 |
| `SAMPLE_SOURCES_06` | Loop crossfade dual demand | native | V2-09 | M2 |
| `SAMPLE_SOURCES_07` | Sustain loop exit to recorded tail | native | V2-09 | M2 |
| `SAMPLE_SOURCES_08` | Source start versus live scrub | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_01` | Identical correlated crossfade | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_02` | Uncorrelated crossfade power | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_03` | Parameter morph units | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_04` | Muted layer reactivation | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_05` | Nonlinear placement preservation | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_06` | Per-voice state sharing guard | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_07` | Parallel path latency | native | V2-09 | M2 |
| `MORPHING_AND_AUDIO_GRAPH_08` | Explicit feedback only | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_01` | Free-running versus note-reset LFO | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_02` | Constant and event-rate folding | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_03` | Audio-rate modulation retention | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_04` | CLAP scoped parameter modulation | documented | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_05` | MPE channel independence | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_06` | Fourteen-bit and NRPN state | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_07` | Filter modulation stability | native | V2-09 | M2 |
| `MODULATION_AND_EXPRESSION_08` | Retune existing versus future notes | native | V2-09 | M2 |
| `STREAMING_AND_ASSETS_01` | Cold attack readiness | native | V2-11 | M3 |
| `STREAMING_AND_ASSETS_02` | Injected storage stall | native | V2-11 | M3 |
| `STREAMING_AND_ASSETS_03` | Shared decoded pages | native | V2-11 | M3 |
| `STREAMING_AND_ASSETS_04` | Start-offset preload budget | native | V2-11 | M3 |
| `STREAMING_AND_ASSETS_05` | Reverse/high-rate demand | native | V2-11 | M3 |
| `STREAMING_AND_ASSETS_06` | Prepared-plan held-note swap | native | V2-10 | M3 |
| `STREAMING_AND_ASSETS_07` | Bound retained generations | native | V2-10 | M3 |
| `STREAMING_AND_ASSETS_08` | Failed asynchronous load rollback | native | V2-10 | M3 |
| `CONTROLS_AND_PERSISTENCE_01` | UI-closed musical equivalence | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_02` | Stable automation identity | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_03` | Widget restore callback order | probe | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_04` | Coherent snapshot under playback | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_05` | Profile version pinning | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_06` | Relink and missing assets | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_07` | Live-set prepared transition | native | V2-13 | M4 |
| `CONTROLS_AND_PERSISTENCE_08` | Obsolete async completion | native | V2-10 | M3 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_01` | Parser structural limits | native | V2-07 | M2 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_02` | Archive and path constraints | native | V2-07 | M2 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_03` | Queue saturation classes | native | V2-12 | M3 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_04` | Native builtin execution budget | native | V2-12 | M3 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_05` | Nonfinite DSP input | native | V2-12 | M3 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_06` | Long-duration clock arithmetic | native | V2-03 | M1 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_07` | Unsupported feature honesty | native | V2-07 | M2 |
| `ROBUSTNESS_AND_RESOURCE_BOUNDS_08` | Callback allocation audit | native | V2-12 | M3 |
| `HOST_LIFECYCLE_AND_OFFLINE_01` | Terminal output backpressure | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_02` | Terminal retry and same-key retrigger | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_03` | Variable and zero block sizes | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_04` | Suspend and reactivate | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_05` | Sample-rate reconfiguration | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_06` | Real-time versus offline contract | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_07` | Host pool refusal | native | V2-15 | M4 |
| `HOST_LIFECYCLE_AND_OFFLINE_08` | Multi-output layouts | native | V2-15 | M4 |
