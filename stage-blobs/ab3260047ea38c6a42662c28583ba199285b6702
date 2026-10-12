# Following-batch production modulation: bounded source-ready slice

## Status and exact source

**SOURCE_READY; Rust compile/runtime NOT_RUN; native parity UNKNOWN.** This is an independently complete source slice for a bounded set of outgoing internal-modulator routes, not completion of the full modulation request or product acceptance.

- Immutable base: `366515c96b583747d1bd77c881b1f6909e86cf0d`, tree `a3a49bd4427b9b6941ebdaf14948bfc844197365`, version 0.3.403.
- Implementation commit: `d49ab53e59ab240278bab051e49c61bf604063b5`.
- Additional production-pitch regression commit / final source pin: `c4fd26fd0a2f1dab3780d3c8b711c302f2713f53`, tree `f3963b8897d2a244a9da7630dc228094cbe9bda1`.
- Exclusive checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/production-modulation-next`, branch `pi/production-modulation-next`.
- `source-manifest.json` pins all 16 changed Rust files by Git blob and SHA256, implementing symbols/spans, and all ten authored regression names. All source locations below refer to the final source pin, not a moving branch.

Current frozen round5 candidate `bac3c616824cf26e2a888c09cb21433b727d9079` was not modified. No Cargo/rustc/clippy, host, native/runtime measurements, dependency installation, dev server, nested agent, timer, global build-environment override or generic gate probe was run. Source commits disabled Git hooks for those commits to avoid implicit compilation. The validated checkout has no `graft/` or AGENTS.md; the initial graft query resolved to an unrelated home graph, so bounded `rg` caller searches and exact-span reads were used. The root repository's navigation/safety instructions and full Ponytail skill were read.

## Authoritative requirements and native limitation

Cached public archives are under `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/`. The no-build check rehashed all three. Retrieval identities, URLs and times remain in that directory's `manifest.json`; exact headings/excerpts remain in `sections.json` and the coordinator's `round7-production-modulation-requirements.md`.

| Precise feature / authoritative section | Archive SHA256 | Requirement used here |
|---|---|---|
| [Kontakt Modulation → AHDSR](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation#ahdsr) | `ca330e3207a333061ebca4ee28b0545b5e93c223624ff69ca608c7a85d897c9d` | Retrigger on restarts on a note. Off keeps the position until the last note is released and restarts on the next note. AHD Only is separate. Off ownership is **not implemented by this slice**. |
| [KSP Engine Parameters → Modulation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters) | `4a15e3b10a9da8993c90ddc7681fee9fd62a7eafc5f3685f47b0c5ab37d1d16d` | MOD_TARGET_INTENSITY is positive assignment intensity; negative uses Invert. MP_INTENSITY has a different midpoint-centered bipolar range. INTMOD_BYPASS controls the internal source's bypass button. |
| [KSP Engine Commands → get_mod_idx(), get_target_idx(), set_engine_par(), get_engine_par()](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-commands) | `df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d` | Physical zero-based group/modulator slots; internal target ordinal in generic selector, unused selectors -1. Continuous amounts generally 0..1000000; switches 0/1. Internal target examples differ from external VEL_VOLUME generic=-1. |

REA `current_document({})` returned `target_unavailable` (no app open), matching the immutable `handoff/roadmap-round7-rea-current-document.json`. **No qualified immutable native receipt exists for these overlapping AHDSR or live intensity/bypass behaviors.** Public documentation is equivalent evidence for requirements only. Isolated v1 recurrence tests, AR coefficients, wrapper vectors, DFD serializer evidence and older generic core tests do not establish this lane's native behavior. No Kontakt waveform, clock, pause/resume, coefficient, release or whole-product parity is claimed.

## Admitted scope and deliberate policy

Only existing admitted LFO shapes/rates, or **retrigger-on non-primary AHDSR**, with all outgoing destinations satisfying the following predicate gain live lanes:

- simple source-level `volume`, `pan`, or `pitch`, no addressed module slot;
- saved magnitude finite in 0..1, independent saved sign flag clear;
- no enabled shaper and no lag;
- actual connected outgoing route, not the optimized primary unity-volume amplitude owner.

Every source keeps its original physical modulator slot. Every amount binding uses its original target ordinal. One bypass control is shared by all admitted outgoing routes of that source; another source has a different bypass. An alias whose route has no remaining zone consumer after key filtering is not published as a live engine binding.

This slice does **not** bind primary-amplitude bypass/amount, retrigger-off AHDSR, Flex, filter/module controls, sample-start, saved signed-depth assignments, MP_INTENSITY, Invert writes, retrigger writes, waveform/timing writes, external assignments, shaped or lagged sources. Existing saved behavior for those paths is unchanged. A source with an unsupported combination does not gain apparently successful live setters from this patch.

Supported saved bypass is preserved as a real neutral route mask at voice onset, not discarded admission data. **KONTRA implementation policy:** the source clock and existing state continue while bypass masks every outgoing depth to zero; resume does not reset source phase/envelope/lag. Existing 64-frame production cells and linear gain/pan interpolation remain; writes become observable at the existing next control evaluation. The docs do not establish Kontakt's exact pause/resume or sample-boundary law. That native question remains UNKNOWN, and the clock-policy regression is explicitly a KONTRA policy test.

Positive amount is represented in existing destination units: 0..1 attenuation/pan and 0..12 semitones for pitch. The positive setter does not replace saved Invert, and MP has no fallback to that setter. Switch conversion maps zero to off and nonzero to on with 0/1 getter output. No new native conversion coefficient or lane clock is introduced.

## Actual production consumer trace

All locations are pinned to `c4fd26fd0a2f1dab3780d3c8b711c302f2713f53`.

1. `sampler-kontakt::load`, `crates/sampler-kontakt/src/load.rs:142–191`, uses public `read_with_controls` and sample decode. The new fixture is an independently serialized NKS preset containing sparse internal slots 7/12 and an owned WAV. It does not inject a generic core-only modulation program.
2. `Translation::group`, `crates/sampler-kontakt/src/library.rs:1348–1490`, applies the bounded source/target predicate and retains eligible saved bypass. `internal_route_controls`, `1762–1831`, allocates source-local controls and physical `SourceControlAlias` records, preserving target ordinals and authored/script-init defaults. Primary amplitude promotion stays on its old path.
3. `SourceIndices::route_controls`, `crates/sampler-ir/src/lib.rs:206–221`, records only admitted route/depth/bypass references. `validate.rs:500–521,545–562` checks references, duplicate route binding and actual route-or-processor consumers for aliases. No generic setter mirror is added.
4. `lower_with`, `crates/sampler-core/src/lower.rs:653–695`, reuses existing control definitions and engine aliases. It omits filtered/unconnected route aliases and selects the switch law for bypass. `Lowering::program`, `1190–1470`, attaches sparse control IDs to real outgoing routes.
5. `Prepared::with_voice_modulation`, `crates/sampler-core/src/prepare.rs:774–902`, validates target/index/domain, preserves sparse indices when earlier generic DSP routes expand, and resolves IDs to prepared real-control indices. Invalid, duplicate, non-real and unsupported sample-start/direct-DSP live lanes cannot pass this boundary.
6. Existing `Runtime::set_engine_parameter_in`, `crates/sampler-core/src/engine_parameters.rs:603–677`, writes those real control cells via `edit_controls_now`; unbound addresses still return InvalidInput. There is no authored-getter-only success path. `EngineParameterLaw::Switch` is the only new service law.
7. Production onset callers `crates/sampler-core/src/prepare/selection.rs:930–938,1020–1048` and `Runtime::prepare_voice`, `render.rs:529–563`, pass the current plan's control base into the real modulation evaluator. `VoiceModState::evaluate`, `voice_mod.rs:1231–1364`, reads resolved sparse depth/bypass cells during the existing route reduction. `mix`, `1369` onward, remains the existing PCM consumer. `CELL` at `1436` still uses DSP BLOCK=64 (`dsp.rs:870`). No production call to isolated `v1_voice_controls` was added.

## Authored regression requirements — all NOT_RUN

Package `sampler-kontakt`, test target `production_modulation`, filter `serialized_`:

- `serialized_lfo_live_intensity_bypass_reaches_pcm_without_compacting_slots`
- `serialized_saved_bypass_stays_neutral_until_live_enable`
- `serialized_retriggered_ahdsr_routes_have_live_amplitude_and_pan_consumers`
- `serialized_retrigger_off_ahdsr_live_controls_remain_unbound`
- `serialized_invert_is_not_replaced_by_positive_or_mp_intensity`
- `serialized_overlap_release_keeps_other_voice_and_live_control_ownership`
- `serialized_filtered_routes_cannot_publish_getter_only_live_bindings`
- `serialized_live_writes_are_independent_of_host_render_partition`
- `serialized_bypass_masks_routes_without_restarting_the_source_clock`
- `serialized_pitch_intensity_changes_pcm_cursor_step_not_pan_or_voice_identity`

They author the full preset and sample, call public load→prepare→runtime, write documented engine addresses after preparation, and assert PCM levels or source-cursor step rather than only getter changes. They cover saved/default/sibling behavior, zero-size and irregular partitions, release and reused voice identity, filtered routes, invalid physical/target/MP addresses and render/write heap guards. Their assertions are requirements, **not observed execution**. The retrigger-off test checks explicit unbound live controls only; it does not establish overlapping retrigger-off behavior. Primary amplitude and filter-envelope live ownership are not covered or solved.

Six existing core/KSP test files receive only the new sparse ModProgram field's empty default; their source changes are schema maintenance, not executed regression receipts.

## Source-only checks and memory limits

- Direct rustfmt parser runs succeeded on changed Rust files, with only changed spans formatted in existing files. This is syntax parsing, not Rust type checking.
- `git diff --check` passed.
- `python3 docs/audit-2026-10-10/production-modulation-next/model_check.py` passed. `source-model-receipt.json` records 16 pinned source file digests, three official archive digests, five independently computed scalar PCM requirements and ten authored tests. It executes no Rust/native/host code.
- `ModShape`, `VoiceModState` and `bytes_per_voice` source bodies are byte-identical to base3665; their spans/digests are in the manifest. Sparse control metadata belongs to ModProgram/compiled Program, not an additional per-voice field or array. **This is not a blanket no-per-voice-memory-growth claim:** admitting previously bypassed sources can increase a program's existing source/route maxima and thus allocation sizes. No compiler ABI/layout, allocation totals, RSS or timing measurement was taken.
- Preparation adds source controls/aliases, sparse records and validation. Preparation's matching scans are bounded by authored program size and occur off audio. Route evaluation remains linear in routes plus live-control records per voice/cell, with no new callback allocation, locks, I/O or source coefficient calculation in this code. Actual RT safety, overhead and total RAM still need the integration test/heap gate and comparative measurements.

## Composition reservation and next real gates

**Same-file composition risk:** UVIround6 separately owns `EngineParameterLaw::decode` Exponential/ShiftedExponential normalized endpoint branches and `law_tests`. This lane changes Switch enum/valid/decode/encode arms only. `normalized_value` remains strict/unchanged, and the old endpoint branches are unchanged at this source pin. During cherry-pick conflict review preserve **both** UVI's exact endpoint branches and the Switch arms. Do not overwrite the whole match/file with either lane's version. Tests added by UVI must be retained.

NEXT, coordinator/integration owner only: include the ordered source commits in a **future combined freeze**, preserve UVI's endpoint slice, and use the sole authorized build/test cycle. Concrete validation requests are all ten `sampler-kontakt` `production_modulation::serialized_` tests, existing core `voice_mod`, `lower`, envelope/ownership/multicore/paged/selection/bus tests, existing KSP `mod_values`, and sampler-ir source/validation coverage. No immediate separate build is requested.

Separate unresolved seam: implement imported retrigger-off AHDSR ownership keyed by plan/group/physical source, with correct effective-key/voice-family counting and separate release tails. A copy of one voice's age or a globally free-running envelope is not the documented last-note contract. Required follow-up evidence/regressions: overlapping A/B under off/on, release A while B stays held, final release, C after final release; amplitude and supported non-amplitude owners, multi-zone and pedal/retirement behavior, and a qualified native receipt for corresponding behavior. The docs do not settle exact native lane clock or bypass recurrence. This larger ownership slice is not safely represented by this outgoing-route mask fix.

The original goal — polished product with significantly lower **CPU and RAM than both frozen v1 and Kontakt** — remains **UNACHIEVED**. No new comparative acceptance claim follows from this source-ready slice.
