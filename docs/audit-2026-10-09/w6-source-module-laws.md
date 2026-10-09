# Original source-module control contracts

Base: `c1d8f2ca`. W6 supplies checked DSP laws; W9 owns whole-voice consumption and W5 owns addressed engine-parameter binding/lowering. This slice does not broaden playback admission.

## Normalized LFO frequency

`v1_voice_controls::LfoFrequency::new(rate)` prepares the reciprocal of the **control rate**, audio rate divided by 32. `increment(normalized)` reproduces the original Digital Multi kernel's f32 clamp, exponential approximation, bit construction and reciprocal multiplication, then expands that f32 increment to f64. NaN and negative input become zero; input above one becomes one. The result is an increment per native control tick, not Hertz or an audio-sample increment. Invalid rates are rejected during preparation. Evaluation allocates nothing.

The original hash-pinned engine's `0x1408f1260` binds frequency destination ID26 by physical internal-source slot, independently of dense runtime source order. A matching event stamp supplies the lane at object+c8; a stale stamp clears it. `0x140b078d0` consumes that lane. Preserve physical group/source identities and native tick ordering when W5/W9 connect this law. Saved settings remain immutable; this helper neither reads initialized/live overlays nor introduces a second control service.

Pinned v1 `0cb7a8a0:src/modulation.rs` retains Module destinations and explicitly excludes externally driven LFOs from its saved-only clock. Its Hertz clock cannot implement this normalized lane. There is no working v1 source-frequency lowerer to copy; the new conversion is supported by bounded execution of original instructions. Existing copied v1 control machinery remains unchanged.

## Module intensity is per destination

Native destination ID25 expands over the destination source's ordered outgoing targets. The native output lane is `(0xe3 + destination_target_ordinal) * 16 + physical_internal_slot`; its initial value comes from that outgoing target's saved intensity. Thus a module intensity assignment is not a multiplication of the source waveform shared by all targets. The existing generic `ModScale` does not prove this native contract.

Original target-expression registration also distinguishes target flags, inversion, shaping and lag. Those combination laws still need numeric verification before admission. W9's rejection of ModScale/source intensity/frequency remains necessary; this slice supplies only the checked frequency helper and preserves the intensity witness.

The installed Conflux witness contains 198 nonzero Constant-to-intensity assignments: 66 each to physical internal slots 1, 2 and 3, whose outgoing target counts are respectively 7, 5 and 7. The test verifies those destination source rows and ordered targets exist. It does not claim those assignments execute.

## Verification

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w6-source-module-laws/frequency-SUMMARY.json`. The corrected wrapper uses this owned worktree's target. RED fails against a linear-frequency stub; GREEN passes all 11 copied-control tests, including 45 original f64 increment checkpoints and heap guards. `sampler-core` area no-run and the installed Conflux intensity witness pass.

`tools/dsp/verify_lfo_frequency.py` executes the original pinned bytes with no helper substitutions. It checks 16 physical-slot/freshness bindings and 15,600 frequency/phase/PCM values across three audio rates and five fragment sizes, including signed zero, NaN, infinities and the normalized grid. Original increment and phase state agree bitwise. This does not establish serialized setter conversion, host scheduling, full LFO admission, live writes or timed CPU/RSS/corpus parity.

NEXT: numerically verify per-target intensity combination, then saved Digital Multi fade geometry and W5/W9 admission.
