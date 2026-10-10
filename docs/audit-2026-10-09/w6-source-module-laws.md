# Original source-module control contracts

Base: `c1d8f2ca`. W6 supplies checked DSP laws; W9 owns whole-voice consumption and W5 owns addressed engine-parameter binding/lowering. This work does not broaden imported-instrument or whole-voice playback admission.

## Normalized LFO frequency

`v1_voice_controls::LfoFrequency::new(rate)` prepares the reciprocal of the **control rate**, audio rate divided by 32. `increment(normalized)` reproduces the original Digital Multi kernel's f32 clamp, exponential approximation, bit construction and reciprocal multiplication, then expands that f32 increment to f64. NaN and negative input become zero; input above one becomes one. The result is an increment per native control tick, not Hertz or an audio-sample increment. Invalid rates are rejected during preparation. Evaluation allocates nothing.

The original hash-pinned engine's `0x1408f1260` binds frequency destination ID26 by physical internal-source slot, independently of dense runtime source order. A matching event stamp supplies the lane at object+c8; a stale stamp clears it. `0x140b078d0` consumes that lane. Preserve physical group/source identities and native tick ordering when W5/W9 connect this law. Saved settings remain immutable; this helper neither reads initialized/live overlays nor introduces a second control service.

Pinned v1 `0cb7a8a0:src/modulation.rs` retains Module destinations and explicitly excludes externally driven LFOs from its saved-only clock. Its Hertz clock cannot implement this normalized lane. There is no working v1 source-frequency lowerer to copy; the new conversion is supported by bounded execution of original instructions. Existing copied v1 control machinery remains unchanged.

## Module intensity is per destination

Native destination ID25 expands over the destination source's ordered outgoing targets. The native output lane is `(0xe3 + destination_target_ordinal) * 16 + physical_internal_slot`; its initial value comes from that outgoing target's saved intensity. Thus a module intensity assignment is not a multiplication of the source waveform shared by all targets. The existing generic `ModScale` does not prove this native contract.

Original target-expression registration also distinguishes target flags, inversion, shaping and lag. Those combination laws still need numeric verification before admission. W9's rejection of ModScale/source intensity/frequency remains necessary; this slice supplies only the checked frequency helper and preserves the intensity witness.

The bounded original getter probe now checks 448 saved outgoing-target bases (all 16 target ordinals on sparse slots 0/1/7/15, including signed zero and exceptional float bit patterns), 56 saved frequency getters, and 36 missing/inactive source fallbacks. The original 16-target virtual getter is called; no helper is substituted. Frequency flag bit6 selects the saved normalized value; an active source without that flag returns the original default. This establishes saved bases, not admissible input validation or setter/live behavior.

The original expression emitter `0x140873040` is also executed for all 256 target flag bytes and both source-domain modes, with no shaping/lag/custom-expression override. All 512 emitted expressions select:

- Flag `0x04` clear: `base * (1 - (1 - processed_source) * intensity)`.
- Flag `0x04` set: `1 - (1 - base) * (1 - processed_source * intensity)`.

For the synthetic native source kind9, unipolar emission includes `fcinv_flip($signal)` and bipolar emission includes `frange_uni(fcinv_neg($signal))`. These are emitted intrinsic names, not a verified implementation of inversion or normalization. They must not be replaced by guessed arithmetic. The emitter's fifth argument is the source-domain mode selected by production preparation; it is not the serialized target invert flag. Native production preparation also sets default target data when flag `0x10` is absent; this emitter-only probe does not execute that defaulting stage.

The emitter probe substitutes only CRT memory operations (`malloc`, `free`, `memchr`, `memcpy`, `memmove`, `memset`). No arithmetic, formatting, parser or expression helper is substituted. A bounded callback-compilation attempt reaches a missing native parser reporting context at `0x140732b27`; it supplies no numeric callback verdict. Full callback arithmetic, inversion, shaping, lag, preparation defaulting and live writes remain unverified. This is a checked descriptor/combination contract for W5/W9, not module-intensity playback support.

The installed Conflux witness contains 198 nonzero Constant-to-intensity assignments: 66 each to physical internal slots 1, 2 and 3, whose outgoing target counts are respectively 7, 5 and 7. The test verifies those destination source rows and ordered targets exist. It does not claim those assignments execute.

## ID25 in the shared modulation evaluator

`ModScaleLaw::KontaktIntensity { depth, flags, unit }` represents one intensity
assignment to one outgoing target. The shared voice evaluator reads that target's
saved `ModRoute::depth`, divides it by the explicit signed `unit` (physical depth
units per normalized intensity), applies ID25, and converts back. This keeps the
additive branch correct for pitch/decibel units and for a zero saved target base.
The existing generic scale law remains `Multiply`; the shared IR lowerer selects
that default and does not create native assignments or new parameter bindings.

`apply(normalized_base, processed_source, source_bipolar)` implements both emitted
expressions. Its source-domain argument selects `(source + 1) / 2` for bipolar
input, independently of target flags. Its input has already passed conditional
inversion; it does not implement or claim the native `fcinv_*` intrinsics. Only
flag0x04 selects the combination expression. The helper preserves the stated
operator order without clamping or algebraic simplification.

Preparation accepts only saved flags0x10/0x14, unit-range assignment depth and
normalized saved base, and finite nonzero outgoing units. Lag, shaping,
sample-start initialization, native defaulting (missing flag0x10), other flags
including unverified inversion, and controller/pressure/timbre/bend/script live
sources are rejected. No initialized/live parameter overlay or native importer
admission is added. W9 still owns whole-voice consumption; W5 owns binding and
lawful unit conversion.

The compact fixture retains all512 captured emitter cases and their four unique
expressions. A test interprets the captured operators at92160 numeric points
across both source domains; opaque inversion calls receive already processed
signals. Separate audio tests exercise both branches in the actual voice render,
independent outgoing bases, signed physical units and heap guards. These verify
the derived expression contract in product code, not numeric native callback
parity. Pinned v1 `src/modulation.rs` rejects source-driven LFOs and supplies no
ID25 evaluator to copy; its saved-only rejection remains applicable.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-id25-intensity/SUMMARY.json`.
The mathematical RED exits101; all four new rendered checks also fail before
the shared evaluator fix. GREEN passes the512-case/92160-point matrix, all17
voice-modulation tests (one timing test remains ignored), and core area no-run.
Mathematical and rendered checks pass heap guards. Native compiled callback
parity, timed metrics and corpus-wide acceptance are not claimed.

## Verification

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-source-module-laws/frequency-SUMMARY.json`. The corrected wrapper uses this owned worktree's target. RED fails against a linear-frequency stub; GREEN passes all 11 copied-control tests, including 45 original f64 increment checkpoints and heap guards. `sampler-core` area no-run and the installed Conflux intensity witness pass.

`tools/dsp/verify_lfo_frequency.py` executes the original pinned bytes with no helper substitutions. It checks 16 physical-slot/freshness bindings and 15,600 frequency/phase/PCM values across three audio rates and five fragment sizes, including signed zero, NaN, infinities and the normalized grid. Original increment and phase state agree bitwise. This does not establish serialized setter conversion, host scheduling, full LFO admission, live writes or timed CPU/RSS/corpus parity.

`tools/dsp/verify_source_module_base.py` and `tools/dsp/verify_target_expression.py` retain the reproducible bounded getter/emitter checks. Their receipts are `native-source-module-base.json` and `native-target-expression.json` beside `intensity-SUMMARY.json`. These tools do not build or launch a Kontakt host. No cargo gate is repeated for this documentation/probe-only follow-up, and no CPU/RSS/corpus acceptance is claimed.

NEXT: ID25 intensity READY; saved Digital Multi remains a separate deliverable.
