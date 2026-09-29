# Kontakt effects: identification, layouts, support

Source: every `.nki`/`.nkm` under `/mnt/MAIN_STORAGE/Libraries/Kontakt` (781 presets,
0 read failures). Regenerate the counts with `kontakto audit-fx [folder]`; inspect one
preset with `kontakto inspect-fx <preset>`. Only parameter values and file names were
examined, never sample, script or IR audio.

Confidence: **high** = class name, value count and value ranges all agree;
**medium** = two of those; **low** = position only, or a guess recorded so the parser
has names. `param_n` / `flag_n` mean "position known, meaning unknown".

## Where effects live

| Place | Chunk path | Meaning | Confidence |
|---|---|---|---|
| Program child `0x3a` #0 | `Program/0x3a` (8 slots) | Instrument **insert** FX | high (Send Levels always in slot 8, inline FX) |
| Program child `0x3a` #1 | `Program/0x3a` | Instrument **send** FX (parallel) | high (only reverbs/delays, dry level 0) |
| Program child `0x3a` #2 | `Program/0x3a` | Instrument **main** FX (after sends) | medium (only Gainer +6 dB in 10 Solo presets; KSP `$NI_MAIN_BUS`) |
| Program child `0x45` x16 | `BInsertBus/0x3a` | Instrument buses "Bus 1".."Bus 16" | high |
| Group children | none | No group has a `0x3a` rack in any local preset: group insert FX are unused locally | high |

`BParamArray<BParFX,8>` (`0x3a`): `u8 structured(0)`, `u16 version (0x10|0x12)`, then 8x
`u8 present` + `Chunk`. ni-file now errors (instead of panicking) on other versions.

`BParFX` (`0x25`, v0x50) slot wrapper: no public data; the effect object is child 0; the
22-byte **private** data holds the state shared by all effects:

| Bytes | Field | Evidence | Confidence |
|---|---|---|---|
| u32 | internal effect type (2 comp, 6 surround, 7 filter, 8 lofi, 9 stereo, 10 distortion, 11 send levels, 14 phaser, 15 flanger, 16 chorus, 18 delay, 19 convolution, 20 gainer, 21 skreamer, 22 rotator, 26 tape, 27 transient, 28 G-EQ, 29 bus comp, 30 FB comp, 39 reverb) | 1:1 with serialization ID in all 1,700 slots | high |
| u32, u8 | always 0 | | - |
| u8 | **bypass** | 1 on every effect of script-toggled racks (Solo Pads), 0 on the always-on convolution/reverb/send levels | high |
| f32 | **output gain** (linear, processed signal) | insert conv 0.032 (-30 dB wet), send reverb 1.0, bus conv 0 | medium |
| f32 | **dry level** (linear, unprocessed signal added) | 0 on send FX and inline FX, ~1.0 on insert conv/chorus/flanger/phaser | medium |
| i32 | always -1 | | - |

DSP rule used: `out = output_gain * effect(x) + dry_level * x`. KSP exposes exactly
these as `$ENGINE_PAR_*_EFFECT_OUTPUT_GAIN` / `$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL`.

`BInsertBus` (`0x45`, v0x11) public data: `utf16 name, f32 volume, f32 pan, i32 output
(-1 = instrument out)`, sometimes 2 trailing zero bytes. ni-file had volume and pan
swapped; values (1.0 / 0.0056) settle it: **high**.

## Local instance counts

| Kind (ser ID) | Class | Rack | Active | Bypassed | Status |
|---|---|---|---|---|---|
| Send Levels (0x17) | BParFXSendLevels | insert | 784 | 3 | **routing implemented** |
| Convolution (0x16) | BParFXIRC | insert | 348 | 0 | **implemented** |
| | | send | 3 | 0 | **implemented** |
| | | bus | 75 | 6 | parsed; bus routing not applied |
| Reverb (0x59) | BParFXGaloisReverb | send | 376 | 1 | **implemented** (own algorithm) |
| Gainer (0x13) | BParFXGainer | insert | 133 | 0 | **implemented** |
| | | main | 10 | 0 | **implemented** |
| Stereo Modeller (0x1f) | BParFXStereoSpread | insert | 3 | 0 | **implemented** |
| | | bus | 0 | 6 | - |
| Solid G-EQ (0x44) | BParFXSSLGEQ | bus | 2 | 12 | passthrough + warning (the 2 active ones are flat: all gains 0.5) |
| Compressor (0x19) | BParFXCompressor | insert | 0 | 6 | bypass only |
| Tape Saturator (0x42) | BParFXTape | insert / bus | 0 | 3 / 6 | bypass only |
| Filter (0x18) | BParFXFilter | bus | 0 | 6 | bypass only |
| Skreamer (0x21) | BParFXSkreamer | bus | 0 | 6 | bypass only |
| Surround Panner (0x1d) | BParFXSurroundPanner | bus | 0 | 6 | bypass only |
| Chorus / Flanger / Phaser (0x11/0x12/0x14) | legacy | insert | 0 | 3 each | bypass only |
| Delay (0x10) | BParFXDelay (legacy) | send | 0 | 3 | bypass only |
| Distortion (0x1e), Lo-Fi (0x20), Rotator (0x22) | | insert | 0 | 3 each | bypass only |
| Transient Master (0x43), Solid Bus Comp (0x46), Feedback Comp (0x4c) | | insert | 0 | 3 each | bypass only |

Every local non-bypassed instance except the two flat bus G-EQs has DSP. The 3-instance
kinds all come from the Solo/Areia "Pads" sound-design racks, where the script enables them.

## Parameter layouts (effect object public data)

All layouts match the byte length of every local instance (no instance falls back to
`opaque`). Values are floats unless marked (b = u8 bool, i = i32).

| Kind | Version | Layout | Confidence |
|---|---|---|---|
| Gainer | 0x50 | `gain` linear (1.0011, 2.0) | high |
| Send Levels | 0x51 | `u32 n=8, n x send level (linear, per send slot)`, `u32 m=17, m x f32` (all 1.0, unknown) | high for sends (0.25/0.128/0.0625 = -12/-18/-24 dB) |
| Stereo Modeller | 0x70 | `spread, pan, pseudo(b)` (`$ENGINE_PAR_STEREO*` order) | medium; spread stored 0 on active ones, read as offset from 100% width (-1 mono .. +1 200%) - **low** |
| Reverb (Galois) | 0x10 | 10 normalized values in `$ENGINE_PAR_RV2_*` order: `type, time, size, damping, mod, diffusion, predelay, high_cut, low_shelf, stereo` | medium: 10 values = Reverb2's 10 params, send-only, typical 0/0.37/0.5x4/0/0/0/1. Raum has 15 params, so 0x59 is *not* Raum |
| Convolution | 0x70 | `f32 -1, f32 0, predelay_ms (0/40), ER: length_ratio 1, low_cut 20 Hz, high_cut 20 kHz; LR: same three; f32 -1; 5 x b (0,1,1,1,0); u32 8 + 8 x (0..1 curve x); u32 8 + 8 x (0..-79 dB curve y); i32 ir_index` | high for the band values and `ir_index`; medium for predelay; unknowns named `unknown`/`flags` |
| Solid G-EQ | 0x10 | `lf_gain, lf_freq, lf_bell(b), lmf_gain, lmf_freq, lmf_q, hmf_gain, hmf_freq, hmf_q, hf_gain, hf_freq, hf_bell(b)` (normalized, 0.5 = 0 dB) | medium (10 f + 2 b = KSP list minus the HP/LP filters added later) |
| Compressor | 0x70 | `param_0 (0), threshold_db (-24, -18.6), ratio (0.25, 0.205), attack_ms (50, 8.8), release_ms (300, 86), link(b)` | medium for dB/ms fields, low for ratio encoding |
| Delay (legacy) | 0x51 | `time_ms 500, damping, pan, feedback, time_unit (-1), time_free_ms 500, param_6, flag_7(b)` | low beyond time |
| Chorus | 0x51 | `depth, speed, phase, speed_unit(-1), speed_free, param_5, flag_6(b)` | low |
| Flanger | 0x51 | `depth, speed, phase, feedback, color, speed_unit(-1), speed_free, param_7, flag_8(b)` | low |
| Phaser | 0x51 | `depth, param_1, speed, param_3, speed_unit(-1), speed_free, param_6, flag_7(b)` | low (speed identified by its free copy) |
| Filter | 0x92 | `filter_type(i)=50, param_1(i)=50, cutoff, resonance` | low |
| Distortion | 0x60 | `param_0, drive, damping` | low |
| Lo-Fi | 0x70 | `bits, frequency, noise_level, flag_3(b), noise_color` | medium |
| Skreamer | 0x50 | `tone, drive, bass, bright, mix` (KSP order) | medium |
| Rotator | 0x50 | `speed, balance, accel_hi, accel_lo, distance, mix` (KSP order, count matches) | medium |
| Tape Saturator | 0x10 | `gain, warmth, hf_rolloff, quality(b)` | medium |
| Transient Master | 0x10 | `input, attack, sustain, smooth` | medium |
| Solid Bus Comp | 0x12 | `threshold, ratio, attack, release, makeup, mix, link(b), flag_7(b), param_8` | low |
| Feedback Comp | 0x10 | `input, ratio, attack, release, makeup, mix, param_6, hq_mode(b), link(b), flag_9(b)` | low |
| Surround Panner | 0x80 | `param_0, param_1` | low |

## Convolution impulse responses

The IR is **not** stored in the effect: `ir_index` indexes the preset's
`FNTableImpl.other_filetable` (entry 0 is the preset itself). Paths are relative to the
preset folder and usually point into the resource container, e.g.
`../../../Samples/Afflatus_Brass.nkr/Resources/ir_samples/<name>.WAV`. Una Corda uses
`../Samples/Resources/ir_samples/...` with the container named in
`special_filetable`; the importer maps any `.../Resources/...` path into that `.nkr`.
Una Corda's `EMT140` IR member has a damaged archive header (see `archive-health.json`),
so it is reported as unavailable and passes through.

IRs are used raw (no normalization). Evidence: the insert convolutions set wet to -30 dB
with dry 0 dB; with raw hall IRs that yields a -20 dB reverb-to-direct ratio (measured on
Afflatus "4 Horns Performance"), while unit-energy normalization would make them inaudible.
Which of the five flags is Kontakt's auto-gain is unknown (they are identical in every preset).

## DSP

Ownership: `ProgramFx` (on `Instrument.fx`) is an immutable description, `Clone` and
shared with the UI; IRs sit behind `Arc`. `ProgramFx::processor(rate, max_block)` builds
an owned `FxProcessor` holding all DSP state, leaving out bypassed and unimplemented
slots and send slots nothing taps. All allocation happens there; `FxProcessor::process`
never allocates, locks or panics and splits blocks longer than `max_block`.

Signal path: `Engine` owns the processor next to its bank (`Engine::set_fx`) and runs it
on the whole part output inside `Engine::render`, after voice rendering and the output
stage (instrument volume/pan, Tone), so racks, `kontakto render` and tests all hear it.
It runs with no voices active so tails ring out; once the input has been below -120 dBFS
for longer than the summed tails of all slots (reverb: 2 x RT60 + predelay; convolution:
IR length) it stops processing until input returns. `Engine::reset` clears all effect
state without allocating.

Plugin: the loader builds the processor at the host rate with the bank and hands both
over in one `ready` message; replaced banks and processors go back through `discard` and
are dropped on the loader thread. `reset(rate)` records the rate; the next loader run
(forced by the reset) rebuilds processors whose rate differs from the cached instrument
description and hands them over alone, keeping the bank. Until that arrives (one loader
run) the old processor keeps running at its original rate.

- **Convolution**: two-stage zero-latency partitioned FFT (`realfft`). Head: partitions of
  `next_pow2(max_block)` (32..512) covering the first 16 blocks, recomputing the partial
  block each call (no added latency for any host block size). Tail: 16x larger partitions;
  its one-block latency equals its IR offset, and its history multiply-accumulate is spread
  evenly over the head blocks, so the only per-tail-block spike is one FFT pair. Verified
  against direct convolution to 1e-4 for block sizes 1, 37, 64, 128, 300. IR resampled
  linearly to the host rate; predelay applied; ER/LR band filters and length ratios other
  than the local defaults (20 Hz / 20 kHz / 1.0) are warned about, not applied.
- **Reverb**: 8-line FDN, Hadamard feedback, per-line damping, quadrature-LFO delay
  modulation, 2-stage input diffusion, predelay, input high cut, output low shelf, stereo
  width. Kontakt's algorithm is unknown; normalized mappings are guesses (low confidence):
  RT60 = 0.2 s x 100^time (0.37 -> 1.1 s), size scales delays 0.5..1.5 (Room x0.55),
  damping cutoff 18 kHz x 0.05^damping, predelay 0..250 ms, high cut 20 kHz x 0.025^x,
  low shelf 0..-18 dB, modulation depth 0..1.5 ms.
- **Gainer**, **Stereo Modeller**: vectorizable per-sample loops.
- **Send Levels** (in `ProgramFx`): taps the insert signal at its slot into each send slot
  (`level[send.slot] * output_gain`); send returns are summed back, then the main rack runs.
  Bypassed or unimplemented send slots return nothing.

Benchmark (`cargo test --release --no-default-features --lib fx::tests::bench -- --ignored
--nocapture`, Ryzen 7 7800X3D, quiet machine, stereo, 128-frame blocks at 48 kHz; budget
per block 2.67 ms):

| Effect | Mean ns/block | Worst ns/block | Share of block |
|---|---|---|---|
| Gainer | 62 | - | <0.01% |
| Stereo Modeller | 161 | 1,530 | 0.01% |
| Reverb | 5,524 | 52,801 | 0.21% |
| Convolution 0.5 s IR | 4,774 | 91,792 | 0.18% |
| Convolution 2 s IR | 8,256 | 94,812 | 0.31% |
| Convolution 5 s IR | 14,323 | 97,932 | 0.54% |

Worst cases include the first (cold) block and scheduler noise; under a load average of
20 the means roughly doubled. No Kontakt measurement exists for comparison on Linux.

## Open questions

- Semantics of Send Levels' second table (17 values), the convolution curve/flags, and
  the `-1` in legacy FX `*_unit` fields.
- Group-to-bus routing (engine) - bus chains are parsed and preparable but not mixed.
- Exact Kontakt reverb/convolution gain staging; validate against Kontakt renders when available.

## Group-to-bus routing (2026-09-29)

No stored group output selector was found. Group children are only 0x38, 0x3b, 0x3c and 0x4a; `GroupParams` reads a fixed public-data prefix and `fx_idx_amp_split_point` is undocumented (likely the amp position in the old group insert chain, unverified). All 12,592 local buses output to the instrument (`output = -1`). Only Areia uses buses actively (convolution on Bus 1-5, distinct pans), and its scripts reference `$ENGINE_PAR_OUTPUT_CHANNEL` and `$NI_BUS_OFFSET`: routing is most likely script-set via `set_engine_par`. Unverified next step: dump group public/private bytes past `interp_quality` for Areia groups differing only by mic tag.
