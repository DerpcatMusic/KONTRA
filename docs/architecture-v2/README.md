# KONTRA 2.0 architecture workbench

Status: independent native ownership, scheduling, resident DSP, behavior and MIDI services,
2026-10-05. Production playback has not been replaced or certified.

This work starts on `docs/plan-v2-architecture`, in the isolated T3 worktree
`t3code-deca12d9`, at source commit
`ac2adc981191347bbdacaee3a29c359464ec712e`. “2.0” names the architecture effort;
it does not change the current package version or release policy.

The proposed direction is one shared musical runtime with versioned format and
behavior adapters. The immediate job is to make ownership, scheduling, preparation,
and retirement independently testable in a clean-sheet product. Explicit user direction
on 2026-10-05 removes all 1.x compatibility requirements. Later direction permits
selective reuse and porting after native foundations are ready, without carrying
forward the old core or redundant ownership; see the current priority in PLAN.md.
MIDI 2.0 is required from the event-model design onward. Third-party instrument
compatibility remains a separate, evidence-backed capability.

## Working documents

The new [Kontakt source boundary](KONTAKT_SOURCE.md) preserves expanded records
without importing old engine types; container playback admission remains open.

1. [Current architecture and ownership map](CURRENT_STATE.md): inspected source,
   existing safeguards, coupling, and unresolved risks.
2. [Clean-sheet target architecture and delivery plan](PLAN.md): boundaries, lifecycle
   contracts, decisions, MIDI 2.0 scope and delivery gates.
3. [Task list](TASKS.md): ordered work with dependencies and acceptance criteria.
4. [Conformance allocation](CONFORMANCE_MAP.md): all 128 supplied scenario IDs
   assigned to implementation tasks, without claiming execution.
5. [First implementation evidence](IMPLEMENTATION.md): runnable checks, actual scope,
   reproduced legacy defects, and remaining gates.
6. [Family/expression implementation and measurements](OWNERSHIP_SLICE.md): current
   contracts, independent core checks and resident-render microbenchmark.
7. [Native gates and scheduling](SCHEDULING_SLICE.md): physical/effective keys,
   sustain/sostenuto, timestamped expression and retained work ownership.
8. [Independent native executable](NATIVE_ENTRY.md): owned prepared assets, indexed
   native layer selection and a runnable WAV render path.

9. [Rust Doctor and realtime DSP policy](RUST_DSP_POLICY.md): scan evidence, CI gate,
   Rust best practices and justified DSP decisions.

10. [Native envelopes and tail ownership](ENVELOPES.md): sample-time AHDSR, retained
    release tails and callback invariant review.

11. [Native source views and loops](SOURCE_VIEWS.md): independent PCM ranges,
    forward/reverse cursors, release exits and boundary evidence.

12. [Native MIDI/UMP ingress](MIDI_INGRESS.md): pinned wire protocol, precision,
    bounded decoding, sample-time note/pedal routing and fixed-zone MPE expression.

13. [Native bounded behavior execution](BEHAVIOR.md): generated notes, waits,
    suppression, instruction fuel and retained completion/fault ownership.

14. [Clean-sheet KSP source subset](KSP_FRONTEND.md): bounded source compilation,
    explicit unsupported diagnostics and execution through the new native runtime.

15. [Prepared-plan adoption](PLAN_ADOPTION.md): retained generations, bounded
    control/audio transfer and off-audio destruction.

16. [Resident render measurements](RENDER_WORKLOADS.md): reproducible polyphony and
    capacity workloads plus measured sustain optimization.

17. [Prepared note-expression modulation](MODULATION.md): typed destinations,
    event-rate projection, retained-plan ownership and actual MPE audio.

18. [Coordinated native variation](VARIATION.md): scoped sequential/random/no-repeat/shuffle takes, retained
    decisions, transactional advancement and per-generation mutable state.

19. [Retained release context](RELEASE_CONTEXT.md): key-up velocity, sample times,
    closure causes and note-owned lifetime across pedals and plan replacement.

20. [Native release selection](RELEASE_SELECTION.md): independent key/gate layers,
    reserved capacity, finite family lifetimes and cleanup suppression.

21. [Articulation routing and snapshots](ARTICULATION.md): explicit performance domains,
    silent latched switches, sparse filtering and onset/current release policies.

22. [Architecture and open-source reference review](REFERENCE_REVIEW.md): pinned
    sampler sources, relevant edge cases, native evidence and remaining checks.

23. [Full Kontakt 8.12 KSP completion map](KSP_PARITY.md): manual surface inventory,
    native service dependencies and explicit implementation/reference gaps.

24. [Native voice DSP chains](VOICE_DSP.md): gain/biquad processors, pre/post-envelope
    order, independent state, retained tails and bounded whole-chain choke.

25. [Native bus DSP](BUS_DSP.md): prepared summed-signal routing, shared kernels,
    independent bus histories, sample-clock controls and generation-owned tails.

26. [Sample streaming work](STREAMING.md): immutable asset IDs and traversal-derived
    demand; worker cache and paged playback are in progress.

## Implemented and still open

- Native core: generational note/family/voice/expression ownership, physical versus
  effective gates, retained key/gate release context, direct ownership release traversal,
  bounded scheduling/behaviors,
  resident selection, curved DAHDSR/AHD, voice-local gain/biquad chains, shared bus DAGs
  and scheduled family chokes,
  source views/loops, bandlimited rate conversion, root-key/native tuning and live pitch,
  initial expression, transactional multi-owner gestures and prepared event-rate
  pressure/timbre modulation to gain, balance and pitch.
- Selection: coordinated sequential/random/no-repeat/shuffle takes with explicit scope and
  storage budgets, retained note decisions and transactional multi-family admission.
  Key/gate release layers own reserved resources and independent phase decisions.
  Explicit performance domains, latched switches and articulation snapshots are native.
  Controller snapshots use bounded shared versions and compiled inclusive predicates.
  Controller-triggered notes, other switch/phrase policies and persisted recall remain open.
- Preparation: validated immutable PCM shared across plans, adoption between notes, original-generation tails
  and callbacks, and control-thread retirement. Native WAV rendering and the
  deliberately small KSP source subset run independently of the old engine.
- MIDI: UMP framing and MIDI 1/2 note/pedal/channel-mode ingress, MIDI 2 absolute pitch attributes; separate fixed-zone
  MPE pitch/pressure/CC74, whole-semitone RPN sensitivity and zone pedals. Full MIDI
  2 expression, MPE zone configuration/modes and raw scripting interception remain open.
- Product work remains substantial: broader modulation rates/scopes, automated/vendor filter profiles and effects, streaming,
  richer selection/behavior/imports, host integration, persistence and UI. The
  production plugin/UI still uses the old core. There is no new-core DAW build yet.
  Completion explicitly requires full Kontakt KSP parity, complete new-core UI wiring
  and an upgraded sample section; the current KSP subset is intermediate work.

Continue the native foundations and their ownership/performance checks before
selective porting and product cutover, following [TASKS.md](TASKS.md). High-ratio
filter cost and supported pitch range remain explicit DSP limits. No broad task
or format-conformance gate is closed by these partial implementations. The old-VM
KSP probe remains historical evidence, not the new scripting implementation.

## Preserved source material

The four attachments are retained byte-for-byte under `references/`. Their
SHA-256 hashes are recorded in [SHA256SUMS](references/SHA256SUMS).

| File | Role |
| --- | --- |
| [pasted-text.txt](references/pasted-text.txt) | Universal runtime, module boundaries, expression ownership, and MPE design brief |
| [deep-research-report.md](references/deep-research-report.md) | Broad research, semantic/DSP sketches, interchange and export ideas |
| [UNIVERSAL_SAMPLER_ARCHITECTURE.md](references/UNIVERSAL_SAMPLER_ARCHITECTURE.md) | Detailed proposed contracts, evidence limits, and staged delivery |
| [CONFORMANCE_SCENARIOS.json](references/CONFORMANCE_SCENARIOS.json) | 128 human-authored scenario specifications: 100 native contracts, 15 documented profile rules, 13 reference probes |

The references are research inputs, not descriptions of this checkout. External
vendor claims were not independently revalidated during this repository mapping.
The deep research report contains citation tokens from its original session;
those tokens are not recoverable source links. The architecture document includes
a named source registry. Resolve and pin the relevant primary source before
promoting a vendor rule to a verified compatibility claim.

Where the inputs differ, this plan proposes the following reconciliation:

- Preserve a source representation in addition to semantic IR and prepared render
  data; optimized region indices cannot replace script-visible source identities.
- Introduce KSP during the first semantic milestone. The broad report's initial
  scripting deferral would postpone the main stress test of KONTRA's actual needs.
- Share typed runtime services between languages, without assuming arbitrary
  UVI/Lua programs can be translated into the KSP VM.
- Treat crate lists and Rust snippets as sketches. In particular, preserve signed
  external host IDs; do not adopt the broad report's `Option<u32>` sketch blindly.
- Keep dynamic module ABI, exporters, new formats, and advanced DSP behind their
  own evidence gates. They are not prerequisites for a correct note lifecycle.

Existing [Falcon format](../FALCON_FORMAT_GROUNDWORK.md) and
[Falcon runtime](../FALCON_RUNTIME_UI_GROUNDWORK.md) audits remain relevant inputs.
They establish groundwork, not a working UVI engine. Existing compatibility notes
remain authoritative for current advertised behavior; this folder is a proposal.

## Verification and maintenance

Verify the preserved inputs from the repository root:

```sh
(cd docs/architecture-v2/references && sha256sum -c SHA256SUMS)
python3 -m json.tool docs/architecture-v2/references/CONFORMANCE_SCENARIOS.json > /dev/null
git diff --check -- . ':!docs/architecture-v2/references'
```

The whitespace check excludes the references because the supplied originals contain
trailing whitespace and are intentionally preserved unchanged. For staged changes,
add `--cached` to that check. Keep references unchanged; record amendments in the working documents. Source
line links in the map are anchored to the baseline commit above. Keep them as historical evidence when the new implementation replaces that subsystem. Task completion requires executable evidence;
an existing test name, a parsed JSON file, or a checked planning item is not a
passing conformance result. Keep measured results separate from the original catalogue.

[Resident resampling foundation](RESAMPLING.md) records fractional traversal,
static/root-key transposition, live pitch, initial expression, filter evidence and
remaining range, quality, ramp and performance work.
