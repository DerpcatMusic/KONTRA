# V2 handoff — 2026-10-06

Work paused at the user's request after completing the expression-to-filter change.
Latest implementation commit: `62d94904` (`feat(core): bind expressive filters to retained note owners`).
The subsequent documentation commit contains this handoff. No implementation remains
deliberately half-edited at this checkpoint.

## Start here

- Branch: `docs/plan-v2-architecture`.
- Worktree: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-deca12d9`.
- **The independent native core is real and executable, but the complete core/product is not finished.**
- **The production plugin and UI still use v1. No new-core Linux CLAP build has been installed for Bitwig.**
- **Neither full Kontakt nor Falcon support, nor performance superiority, has been established.**
- Resume only when asked. Preserve the clean-sheet direction: one native runtime,
  shared prepared IR/services, no v1 runtime dependencies, compatibility shims or dual engines.
- SFZ and all other format frontends remain deferred until Kontakt/Falcon parity
  and matched-quality performance requirements are met. OSS DSP references are allowed.
- Use `graft/` before source searches/reads, following the user's AGENTS instructions.
  Do not start agents unless delegation is explicitly authorized.

## Status inventory

“Implemented” below describes a tested native slice, not completion of its whole
subsystem or vendor-equivalent behavior. No broad completion gate is closed.

| Area | Implemented and executable | Partial / not implemented | Detail |
| --- | --- | --- | --- |
| Ownership and lifecycle | Generational notes, families, voices, expressions and plans; raw/effective gates; child links; release context; tails; bounded retirement and off-audio destruction | Complete stealing/overload and production resource policies | [Ownership](OWNERSHIP_SLICE.md), [scheduling](SCHEDULING_SLICE.md), [tasks](TASKS.md) |
| Prepared runtime and modularity | Independent native crates; immutable prepared plans; bounded behavior IR; ordered note/controller/release modules; source frontends use shared services | Complete universal semantic instrument schema and all vendor modules | [Shared IR](SHARED_IR.md), [behavior](BEHAVIOR.md) |
| Selection | Indexed regions, coordinated multimic takes, sequential/random/no-repeat/shuffle decisions, scoped articulation/controller snapshots, independent key/gate release layers | Full vendor phrase/legato/switch/selection semantics and persisted recall | [Variation](VARIATION.md), [articulation](ARTICULATION.md), [release selection](RELEASE_SELECTION.md) |
| Source rendering | Resident/paged stereo PCM; bandlimited fractional rate conversion; native tuning and live pitch; source ranges; reverse/wrap/ping-pong/finite loops; linear wrap crossfades | Other source families such as stretching, granular and synthesis; remaining loop/crossfade profiles; measured vendor equivalence | [Resampling](RESAMPLING.md), [source views](SOURCE_VIEWS.md) |
| Envelopes and DSP | Curved DAHDSR/AHD; pre/post-envelope voice chains; gain/matrix/biquad EQ; stereo bus DAG; integer stereo feedback delays; automated state-variable filters; bounded tails/fault containment | Full effect catalogue, nonlinear/oversampled processing, fractional/modulated delays, feedback filtering/diffusion, broader channel layouts and source-specific DSP profiles | [Voice DSP](VOICE_DSP.md), [bus DSP](BUS_DSP.md) |
| Modulation | Event-rate pressure/timbre to gain/pan/pitch; shared-control sample-clock gain and cutoff/Q ramps; pressure/timbre filter destinations with retained expression ownership | Full modulation graph, LFO/envelope source routing, additive/multiplicative destination composition, per-expression smoothing and complete vendor laws | [Modulation](MODULATION.md), [filter destinations](VOICE_DSP.md#note-expression-filter-destinations) |
| Streaming | Revision-owned assets; exact traversal/guard demand; bounded page cache and worker transfer; live demand service; seekable WAV range decoding; paged rendering; 1 ms starvation fade followed by DSP tails | Production worker/resource coordinator, cold-onset preload policy, deadlines/admission under real storage load, offline policy and broader codecs | [Streaming](STREAMING.md) |
| MIDI | UMP framing/channel-voice decoding; MIDI 1/2 notes, pedals and channel modes; MIDI 2 Pitch 7.9 attributes; fixed-zone MPE, captured released-member state, RPN sensitivity and zone pedals | Complete ordinary channel/per-note MIDI 2 expression and management; MIDI-CI, SysEx/JR/device transport, broader MPE configuration and production host adapters | [MIDI ingress](MIDI_INGRESS.md) |
| KSP | Independent integer source subset; globals/polyphonic cells/arrays; control flow and user functions; note/release/controller/scalar UI handlers; waits/generated notes/event aliases/groups; multi-module routing | Full language/types/callback/builtin/API parity; virtual controllers; complete persistence/async/engine services; stage-scoped stored-event note-off; all source UI services | [KSP frontend](KSP_FRONTEND.md), [parity map](KSP_PARITY.md), [machine inventory](KSP_SURFACE.json) |
| Kontakt import | New bounded clear NKS/NIS/FastLZ source decoding, expanded metadata/group/zone/script records; authored saved-script native execution | Complete instrument semantic admission, resources/sample formats, all container profiles, full library playback and vendor differential validation | [Kontakt source](KONTAKT_SOURCE.md) |
| Falcon/UVI and Lua | v1/other-worktree reference investigation; shared-core contracts; pinned Luau embedding probe | New complete Falcon frontend, UVI API/object/runtime semantics, full graphs and library playback. Luau is not a production runtime dependency | [Shared IR](SHARED_IR.md), [reference review](REFERENCE_REVIEW.md) |
| Headless controls/state | Stable scalar controls, coherent bounded capture, recall/edit ownership, queued interactions, callbacks with retained module/performance context and DSP bindings | Complete instrument serialization, typed/non-scalar UI state, host automation/state and async resources | [Control state](CONTROL_STATE.md) |
| Product UI and hosts | Existing v1 UI/plugin remains available; independent native offline WAV executable exists | New-core production CLAP/UI cutover, sample-section upgrade, complete generic/stock/bitmap KSP UI, `.nckp`/`.nckc`, Komplete UI, Falcon Lua UI and related assets/gestures | [UI frontends](UI_FRONTENDS.md), [native executable](NATIVE_ENTRY.md), V2-16 in [tasks](TASKS.md) |
| Parity/performance | Native correctness fixtures, heap guards, pinned OSS reviews and local render/gesture workloads | Licensed/versioned Kontakt/Falcon differential evidence and matched-quality comparative benchmarks; full supplied-scenario execution | [Conformance allocation](CONFORMANCE_MAP.md), [reference review](REFERENCE_REVIEW.md), [workloads](RENDER_WORKLOADS.md) |

The KSP inventory currently has 1,605 extracted identifiers and 42 named partial
overrides. Its default is missing implementation/unverified Kontakt fidelity. These
are inventory counts, **not a compatibility percentage**. All 128 supplied scenarios
are allocated to tasks; allocation does not mean that all scenarios run or pass.

## Recent implementation progress

The resumed work completed these commits:

- `f8fbe8ae`: variable `play_note` durations in UI/controller callbacks and shared
  functions; parent-only durations are validated against real callback ownership.
- `e1f90218`: causal stereo feedback delays in voice/bus chains, preallocated rings,
  bounded history invalidation, independent feedback/tail/retirement evidence.
- `72a4487a`: five native state-variable filter responses with sample-clock cutoff/Q
  automation; shared coefficient calculations, independent histories, generation
  retention and a moving-filter render workload. `GainControl` was directly renamed
  to `ControlRange`; no compatibility alias exists.
- `62d94904`: pressure/timbre filter destinations keyed by retained expression
  owners; link/snapshot/detach/reuse checks and real MPE channel-reuse audio tests.
  Note-dependent filter parameters are rejected on summed buses.

Immediately preceding work already on this branch includes live page demand/service,
seekable WAV decoding, paged rendering/starvation fades, and routed UI callbacks.
See `CHANGELOG.md` and the linked subsystem documents for that accumulated work.

## Known engineering limits and next priorities

1. Complete scoped native modulation, remaining DSP families/effects, real resource
   admission/streaming policies and missing MIDI behavior. Preserve source units and
   explicit owner/rate boundaries; do not introduce a parallel vendor engine.
2. Continue Kontakt semantic lowering and KSP parity, and implement Falcon/UVI modules
   against the same services. The v1 references are evidence, not runtime dependencies
   or trustworthy vendor oracles. Luau adoption still needs allocator/GC, API and
   callback-lifetime evidence; do not move per-sample DSP into a VM.
3. Resolve stored-event `note_off` across script stages. A naive change to a global
   release-start index can suppress upstream callbacks when the physical key later
   releases. Keep physical input lifetime separate from each module's released view.
4. Wire the production host/state/UI only with concrete ownership and resource
   contracts. All requested Kontakt UI generations and the sample-section upgrade
   remain product requirements; headless scalar callbacks do not complete them.
5. Establish real vendor fixtures and matched-quality performance comparisons before
   claiming parity/superiority or beginning other format frontends.

Per-voice throughput remains a material limitation: the recorded four-automated-filter
1,024-voice workload misses the 48 kHz/64-frame deadline. Shared bus processing is
cheaper but is not semantically interchangeable with per-voice effects. Timings are
local/unpinned, not guarantees or competitor comparisons. Expression coefficient
windows currently cost roughly 2 KiB per expression-dependent filter per configured
expression slot; long per-voice delay rings also need explicit admission budgeting.

## Validation at handoff

- All five native packages: **374 tests passed on Rust 1.92**.
- Strict all-target Clippy passed for all five native packages.
- Root `v2_ksp` boundary tests: **2 passed**; existing old-core warnings remain.
- Core/MIDI full release suites passed (286 tests); after the final borrow-only
  refinement, the touched SVF/MPE release suites passed again (17 tests).
- Supplied files' SHA-256 checks all pass; the originals remain unchanged under
  `references/`.
- Rust Doctor was not rerun for this checkpoint, per the user's priority change.
  Historical scores do not establish the current score.
- No DAW or new-core production UI validation was performed.

Logs are ignored local artifacts: `artifacts/expression-filter-{final-msrv,final-clippy,
boundary,release,final-release}.log`. Earlier work uses `routed-duration-*`, `delay-*`
and `svf-*`. These files are local evidence, not checked-in deliverables.

Reproduce from this worktree (the normal `/tmp` had unrelated space pressure):

```bash
RUSTC_WRAPPER= TMPDIR=/home/derpcat/.cache/kontra-rust-doctor-t3code-deca12d9 CARGO_TARGET_DIR="$PWD/target-core" cargo +1.92.0 test --locked --offline -p sampler-core -p sampler-ksp -p sampler-midi -p sampler-native -p sampler-kontakt
RUSTC_WRAPPER= TMPDIR=/home/derpcat/.cache/kontra-rust-doctor-t3code-deca12d9 CARGO_TARGET_DIR="$PWD/target-core" cargo clippy --locked --offline -p sampler-core -p sampler-ksp -p sampler-midi -p sampler-native -p sampler-kontakt --all-targets -- -D warnings
RUSTC_WRAPPER= TMPDIR=/home/derpcat/.cache/kontra-rust-doctor-t3code-deca12d9 CARGO_TARGET_DIR="$PWD/target" cargo test --locked --offline --profile ci --test v2_ksp
```

Use [TASKS.md](TASKS.md) for stable task IDs, dependencies and completion gates.
This handoff is a checkpoint, not a replacement for the architecture or detailed
source/capability inventories.
