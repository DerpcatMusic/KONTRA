# W9 next CPU port — source-only, acceptance pending

Quiet run receipt: `v2/w9-v1-stream-model-52438@e30383ad`, `cpu-quiet-2026-10-09.md`. W9 finished all nine QUIET cells at08:49:16Z, removed its request, and directly handed the slot to W8. Streaming remains HOLD: Areia32 had1 underrun, v1zero. No stream code or budget is included here.

## Existing top-three attribution

Use the post-neutral-filter three-repeat audio-TID attribution in `~/.cache/kontakto-fix-cpu/neutral-stage-attribution/aggregate.json` and its independently recorded profile metrics. This is historical diagnostic evidence, not a new profile of608a/494b or the Areia128 candidate. Exclusive sampled user-CPU costs are estimates per64-frame steady block, not stage latency percentiles. Source review confirms that the selected slot's repeated coefficient evaluation remains in608a and W6's494bb088 extraction.

The old probe retained total block/per-voice timing medians, but not the steady integral of active voices. The approximate voice normalization below divides exclusive stage CPU by the ratio of median block CPU to median amortized-per-voice CPU (v1≈10.436, v2≈8.000 for piano). Ratios of medians are a proxy, not an exact voice-block denominator. No equal-PCM or equal-polyphony claim follows. The new shared probe records exact `steady_voice_blocks` for the next comparison.

| Piano64 cold cost centre | v1 / v2 sampled µs per block | v1 / v2 approximate µs per voice-block | Better v1 path and port boundary |
|---|---:|---:|---|
| Slot mixing |0.475 /8.186|0.045 /1.023|`port from v1 0cb7a8a0:src/fx/processor.rs`: Slot::process settles wet/dry outside its sample loop. Copy block-constant coefficients, adapting v2's ramped bypass; preserve shared kernels and IEEE operation order.|
| Envelope/modulation and filter-factor work |0.059 /7.830|0.006 /0.979|`port from v1 0cb7a8a0:src/engine/filter.rs` and `src/engine/voice.rs`: sparse addressed modulation and settled control reuse are better mechanisms. Nonzero-delta conversion is already ported; remaining full factor-array resets and route scans need an independent sparse-lifetime test, not another copy of the accepted fix.|
| Resampling/source traversal |0.237 /6.584|0.023 /0.823|`port from v1 0cb7a8a0:src/engine/voice.rs`: bounded source windows, whole-step and fused SIMD mix are faster. V2's eight native loops/crossfades and core unsafe boundary require adaptation; changing interpolation quality is outside scope.|

For FX64 cold, the three largest identified costs are envelope/modulation, stream service, and mixing: v1/v2 block costs1.008/14.058,0.415/12.397,0.178/9.372µs. Using the same approximate normalization (22.003/24.009), these are0.046/0.586,0.019/0.516,0.008/0.390µs per voice-block. The stream mechanism is already under the separate HOLD trial; this branch does not duplicate it.

## One selected fix

Pick slot mixing, the largest residual actionable piano bucket after the accepted neutral-filter change. Branch `v2/w9-v1-slot-mix-608`, worktree `/home/derpcat/.t3/worktrees/KONTAKTO/w9-slot-mix-608`. Product prerequisite is W6's shared-kernel source494bb088 (608a lineage); W6 confirmed no coefficient-hoisting overlap. Reuse the existing unmerged64919587 port, adapting its scalar/lane sites to `kernels::mix_gains` and `kernels::mix`. The earlier trial never established a uniform end-to-end win, so reuse is not performance acceptance.

Scope: ControlRamp::settled, scalar voice/bus Mix, lane Mix, their existing scalar/batched callers in render.rs/bus.rs, the unit regressions, and the shared audit metadata. No settings, assets, sample/DSP precision, exports, dependencies, or persisted formats change. Moving ramps still use the previous frame equation; bypass still rests processor state; trace/fault and shared gain arithmetic stay at their existing owners.

Red-fixture commit2e8c193b contains a thread-local test-only ramp-evaluation counter and a block-work bound for both layouts at1/7/64 frames. Its parent repeats evaluations per sample (at64 frames, source-derived scalar386 /lane193 calls for an active settled slot). Fix commit ebf437a4 hoists the settled gains (source-derived scalar2 /lane1 calls). These counts describe the inspected code, not executed test output. Existing independent frozen-equation tests are extended across settled and active ramp boundaries, bypass, signed-zero input, subnormals, short/masked blocks and eight-voice lanes.

## Validation status and next gate

**Prepared only.** Rust parser checks and `git diff --check` pass. The new Rust red/green, root no-run, heap/trace regressions and library timings are unrun because the coordinator requested source-only work and the quiet handoff chain is active. No CPU improvement or READY claim. The source branch is committed locally; push follows required compile validation. The already-pushed streaming receipt is documentation-only on previously compiled source.

After the quiet chain:

1. Run the exact unit `dsp::kernel_tests::settled_slot_evaluations_do_not_scale_with_block_length` on red2e8c193b in an isolated own checkout, through `kontakto-heavy`; expect assertion failure rather than a compiler error.
2. On the fix, run the settled/frozen-bit kernel fixtures, buses/control DSP/trace/parallel/heap regressions, then default root `cargo test --no-run`, serially through the wrapper.
3. Freeze matched494bb088 before/fix probes and an equivalence-checked v1 probe with the identical denominator metadata. Next authorized quiet window measures original piano/FX/Vista and Areia32, cold/warm p50/p99/deadlines/underruns/RSS. Keep all rows, calibrate p99 against A/A noise, and retain zero tolerance for new underruns.

NEXT: execute the staged red/green and compile checks outside the quiet chain; next quiet slot measures this one port.
