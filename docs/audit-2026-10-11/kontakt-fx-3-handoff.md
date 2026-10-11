# Branch + worktree path + commit SHAs (oldest first)

Round 3, package `kontakt-fx`; branch `gpt/kontakt-fx-2`, worktree
`/home/derpcat/.t3/worktrees/KONTAKTO/gpt-kontakt-fx-2`.

1. `39ce0658` — merge requested integration branch (through `a3be3c28`).
2. `9fcadcfd` — Morphology selectors and compensation.
3. `9dc8fd3c` — Inverter assignment proof / false diagnostic removal.
4. `c53a1305` — Formant I native parallel bandpasses.
5. `66dddb22` — saved filter type 13, four-stage Phaser.
6. `0ced2b56` — legacy Phaser topology, oscillator and control laws.
7. `34bb7a9e` — Enhanced compressor detector and integer mode decoding.
8. `64b7612a` — convolution Auto Gain precision.
9. `f81f640f` — saved instrument bus pan audio law.
10. `73a49604` — live Shape / Drive / Damping physical-slot bindings.
11. `11981562` — synced Phaser low-tempo fallback.

This receipt's commit follows those. No push or PR; integration worktree was
not modified. All implementation commits carry the requested co-author trailer.

# Per item: before → after, root cause, evidence (test names, measurements, file:line)

Vendor sections are NI Kontakt 8 [Filter Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference)
(Ladder HP/BP/Peak/Notch, Phaser, Formant I),
[Effect Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference)
(Inverter, Legacy Phaser, Compressor, Convolution/Reverb, Saturator, Distortion),
[KSP Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters),
and [Bus Insert Effects](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view).
Each linked module note below names the precise controls, cache functions,
radare2 confirmations, formulas and limits. Code was independently written.

Binary source: complete Ghidra cache in read-only
`t3code-80fe786b/artifacts/engine-analysis-2026-10-07/kontakt-engine/complete-export/pseudocode.c`.
Only exact function spans were read. Radare2 MCP read
`/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
The updated protocol accepts this cache as step 2; no live REA wait was needed.

Retained library intent: prior metadata-only ANALOG inspection recorded 483
groups and authored Shape/Drive/Damping, Formant Talk/Size and bypass controls;
algorithm selectors were not script-written. Prior saved-payload evidence
included Morphology 35/41 and filter 13/39. Dolce routes groups into a shared
summed instrument bus with saved pan. These are historical reach/intent leads,
not a new survey. No protected script or PCM was logged or written this round.

| Item | Before → after / root cause | Focused evidence and source |
| --- | --- | --- |
| 1. Morphology 35/39/41 | LP-only output → all 12 native selector rows, with selector-specific compensation. Saved 13 is handled separately below. Missing weighted output taps and wrong shared compensation were the root cause. | `dsp/ladder_kernel.rs:422`, `morphology_hp2_bp4_notch_use_native_weights_and_compensation`: first sample HP2 47/192, BP4 47/128, Notch 47/96, endpoint gains .4/.38/.38. Before: HP2 produced LP4 47/768. [Extraction](../re/kontakt-morphology.md), audio `140b00b80`, MCP table `144fcebc0..144fcef50`. |
| 2. Inverter | Diagnosed assumed order → proved first saved flag = polarity, second = L/R swap. Existing implementation was correct; only its false diagnostic was removed. | `effects.rs:3293`, `inverter_saved_flags_match_native_assignment_and_audio`: (.25,-.5) → (-.25,.5), (-.5,.25), (.5,-.25). Failed first on old diagnostic. [Proof](../re/kontakt-inverter.md): reader `140d01db0`, assignment `140961f91..fac`, audio `140908e60`. |
| 3. Formant I | Serial vowel peaks with fixed trim → three native parallel bandpasses. Proved saved Sharp = core index 2, Size = index 1. | `effects.rs:2719`, `formant_native_parallel_impulse_and_saved_sharp_size_mapping`: 48 kHz first impulse .01663771558 at Talk 0/Sharp .5/Size .5, .0361916923 at Sharp 0/Size 1; both channels. Failed first on proxy diagnostic. Core `dsp/formant.rs:22`. [Extraction](../re/kontakt-formant.md): `140b00750`, setter `140b05ac0`, saved assignment `14096468f..14096478f`, MCP tables `144fcea10..144fcebc0`. |
| 4a. Filter 13 | Unsupported saved type → native four allpasses, .85*resonance feedback and half dry/wet output. | `effects.rs:2811`, `phaser_filter_native_four_stage_impulse`: L=[.926062942,-.136394471,-.114671811,-.095316768], R=-.5*L. Failed first on old diagnostic. [Extraction](../re/kontakt-morphology.md): audio `140909810`, coefficient setter `14095d770`, constructor `1407c9a70`. |
| 4b. Legacy Phaser | Unimplemented → four allpasses with native f32 leapfrog sine/cosine oscillator, depth polynomial, quarter-turn stereo Phase, free/synced Speed. | `effects.rs:2792`, `legacy_phaser_free_speed_phase_and_four_stage_vector`: L=[.857850552,-.106721222,-.290372699,-.274628252]; R=[-.476720124,-.0511267483,.0421471819,.0595008172] at 48 kHz, Depth .5, Feedback .25, Speed 1, Phase 1. Failed first. Synced 2 beats/120 BPM matches exactly; .05 BPM fallback failed first and then matches. Core `dsp/phaser.rs:58`. [Extraction](../re/kontakt-phaser.md): `140aa3830`, `140a736a0`; MCP branch `140a736ac..b5` and constants .1/120. |
| 5a. Enhanced compressor | Generic alternate-mode fallback / integer selector misread as f32 → native mode-1 detector and integer decoding. | `effects.rs:2774`, `enhanced_compressor_uses_native_detector_and_integer_selector`: L=[.999157310,.997557640,.995283782,.992414534] for four [1,-.5] frames, mode 1, -6 threshold, .5 ratio, 1 ms attack, 100 ms release, 48 kHz. Linked [1,-1] cancels in signed-mean detector and passes. Failed first on selector. Core `dsp/enhanced_compressor.rs:17`. [Extraction](../re/kontakt-compressor.md): `140ab2c80`, `140ab9220`, reader `140d00850`; MCP confirmed piecewise constants and recurrence. |
| 5b. Convolution Auto Gain | f32 energy sum loses low tails → native f32 square / f64 sum / narrowed sqrt and cap. | `effects.rs:1872`, `convolution_auto_gain_keeps_native_double_energy_sum`: unit then 10,000*.0001, expected gain bits `3f3502a2` (.7070714235), old `3f3504f3` (.7071067691). Failed first. Implementation `effects.rs:1324`. [Extraction and remaining reverb work](../re/kontakt-reverb-convolution.md): `1419b2bc0`, MCP `1419b31f0..326e`. |
| 6. Dolce bus pan | Dropped pan-only bus / unsupported pan → retained bus with native diagonal stereo balance after inserts/volume. Located actual audio, not only serialization. | `effects.rs:1852`, `instrument_bus_pan_only_uses_native_stereo_balance`: [1,1] → [1,0],[1,.5],[1,1],[.5,1],[0,1] for pan -1,-.5,0,.5,1. Failed first on old diagnostic. Bus builder `effects.rs:1509`. [Extraction](../re/kontakt-bus-pan.md): `1408ef2d0` calls `140824910`, which calls stereo gains `140823d70`; MCP confirms clamp/subtract/add paths. |
| 7. Live Shape/Drive/Damping | Init-only static kernels / no runtime bindings → direct template-copy controls addressed by physical slot/rack, retaining filter histories. | `effects.rs:1825`, `live_shape_drive_and_damping_reach_only_the_physical_slot`: correct write failed with InvalidInput before wiring; wrong slot remains rejected; live outputs .75/.74903756/.031603266 for Shape/Drive/Damping after zero then x=.25 at 48 kHz (2e-6 tolerance). `lower.rs:34`, `dsp/distortion.rs:277`. [Extraction](../re/kontakt-drive-parameters.md): MCP confirms direct copies `1408378f0`, `1408373c0`, `140837e40`, distinguishing internal-modulation clocks. |

All numerical checks are computed extracted-law vectors, not new Kontakt
recordings or whole-library parity measurements. No performance, memory or
native recording null claim is made.

Validation, all via `/home/derpcat/.cache/kontakto-heavy`:

- Scoped `cargo check -p sampler-kontakt -p sampler-core` passed (5.45 s).
- Only owned focused tests listed above ran; all passed. Every fix's
  failing-first direction was observed, including the final Phaser fallback.
- One final workspace `cargo test --no-run` passed, exit 0 (34.04 s).
  Existing warnings remain. No full suites, release builds or corpus runs.
- `git diff --check` passed. Own target/scratch removed after completion;
  no owned dev server, host recording or build remains running.

# Files touched outside your scope (and why)

- `sampler-ir/src/lib.rs`, `validate.rs`: native/live variants and finite/range
  admission required to carry the laws through translation.
- `sampler-core/src/lower.rs`, `trace.rs`: shared processor/control lowering
  and trace metadata. Existing physical-slot engine bindings are reused.
- `sampler-core/src/bus.rs`, `render.rs`, `parallel.rs`: pass runtime tempo
  to native synced Phaser in all processing contexts.
- `sampler-core/src/dsp/kernel_tests.rs`, `dsp/svf.rs`: exhaustive test-context
  initializers gained tempo only. Core DSP modules/routing are FX scope.
- `src/sound/v2.rs`, `src/ui/chain.rs`: exhaustive processor names/details;
  duplicate arms introduced in earlier FX commits were removed.
- This receipt and requested `docs/re/` evidence notes.

No changes to `library.rs`, `engine_parameters.rs`, `voice_mod.rs` or generic
modulation registry internals. The requested integration merge contains
coordinator/modulation work in those files, separate from these commits.

# Still open: item + concrete reason

- Modern Reverb: MCP recovered all eight `141ae2c60` dispatch targets
  (`141ad7b70`, `141acda50`, `141acf210`, `141ad0590`, `141ad18e0`,
  `141ad3230`, `141ad4f20`, `141ad6370`). Functions are present; delay
  initialization, feedback/diffusion matrices and selector interpolation
  remain incompletely extracted. Existing approximation and warning remain.
- Convolution: Auto Gain is fixed, not the whole preparation pipeline.
  Native resizing `1419b3450`, filtering `1419b27a0`, early/late `1419b1e30`
  and envelope `1419b2320` remain unimplemented; size/filter/decimation
  diagnostics remain. No absent-function or MCP blocker is claimed.
- Compressor Pro (`140ab35d0`): sliding window scheduling and log/exp/knee
  coefficient extraction incomplete; its diagnosed fallback remains.
  Classic's existing proxy was not changed or re-certified.
- Dynamic Formant/filter-Phaser controls: 15 ms/32-frame/4-frame native
  scheduling remains unbound. Morphology HQ/0x95 secondary cutoff remains
  diagnosed. Legacy Phaser only supports the extracted saved control modes.
- Live bus pan slew (+/-20/sample-rate), bus phase invert and L/R swap are
  not represented by the static pan matrix.
- Shape/Drive/Damping: direct script writes work; internal-modulation clocks,
  Enhanced/Drums Saturator and dynamic algorithm switching remain open.
- Whole-host parity and current library reach are not verified; this round
  follows the explicit test-little rule and uses computed vectors.

# Anything the coordinator must act on (conflicts, follow-ups)

Merge this branch into integration, then run the one coordinator-owned suite
and survey. The incoming merge was clean. Shared `lower.rs`, IR, DSP context
and UI arms are the likely reconciliation points if integration moved again.
No manual conflict markers remain.

Prioritize the remaining native Reverb/convolution pipeline and Pro detector
extraction, then dynamic filter/bus clocks. Use the recovered dispatch targets
above rather than retrying live REA. Inspect actual library output after merge;
this receipt does not turn synthetic vectors into a production host null.

Skipped: full suites, corpus runs, native recordings and guessed DSP laws.
Risk: remaining approximation/control-clock paths above still limit parity.
🌱 graft reported ~659k tokens saved this turn, 6 calls; no dollar figure reported.
