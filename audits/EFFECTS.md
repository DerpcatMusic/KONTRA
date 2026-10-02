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
| Filter / EQ | 0x92 | `type(i), type(i)` repeated, then EQ (types 22..24): `(type-21) x (freq, bandwidth, gain)`; any other type: `cutoff, resonance` (normalized) | high for layout (all local lengths match), see Group filters and EQs for laws |
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
an owned `FxProcessor` holding all DSP state, leaving out unimplemented
slots and send slots nothing taps; bypassed slots are built so scripts can enable them. All allocation happens there; `FxProcessor::process`
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
- Exact Kontakt reverb/convolution gain staging; validate against Kontakt renders when available.

## Group-to-bus routing (2026-09-29)

Signal flow, per render block: voices of groups routed to bus b (by
`$ENGINE_PAR_OUTPUT_CHANNEL` = 1000+b; stored routing is always the instrument)
render into bus b's input; every fed or still-ringing bus runs its chain, then its
fader and pan (ramped), and adds into the instrument sum with the unrouted groups;
then instrument volume/pan/Tone, the insert rack (Send Levels taps into the send
rack), and the main rack. Bus output other than the instrument is not modelled.
Input buffers exist for every stored bus (16 x 2 x max_block floats, 128 KiB at
1024 frames) so routing can change at any frame without allocating. Bypassed
effects (and their IRs) are built too, so scripts can switch them on.


No stored group output selector was found. Group children are only 0x38, 0x3b, 0x3c and 0x4a; `GroupParams` reads a fixed public-data prefix and `fx_idx_amp_split_point` is undocumented (likely the amp position in the old group insert chain, unverified). All 12,592 local buses output to the instrument (`output = -1`). Only Areia uses buses actively (convolution on Bus 1-5, distinct pans), and its scripts reference `$ENGINE_PAR_OUTPUT_CHANNEL` and `$NI_BUS_OFFSET`: routing is most likely script-set via `set_engine_par`. Unverified next step: dump group public/private bytes past `interp_quality` for Areia groups differing only by mic tag.

## Group filters and EQs (2026-09-30)

Group insert rack: group private data holds 136 x 12-byte records plus a 24-byte trailer,
then `BParamArray<BParFX,8>` (read by `Group::insert_fx`). Local `BParFXFilter` instances
by type: 22 (1-band EQ) 12,340; 23 (2-band) 5,720; 24 (3-band) 1,401; 3: 556; 2: 268;
54: 64; 52: 55; 6: 24; 55/57: 20 each; 51: 12; 19: 2. Almost all are Afflatus; Dolce,
Una Corda, Solo, Vista, Pacific and Areia have tens to hundreds.

DSP (`src/engine/filter.rs`): per-voice stereo TPT SVF sections (Simper), coefficients at
control rate (32 frames, or once per block without modulation), recomputed only when a
section's modulated knobs change, denormal flush per block. Bypassed units and EQ bands
within 0.01 dB of flat are skipped (state cleared). Groups without filters, EQs, Stereo
Modellers or Inverters keep the old render path (no per-voice cost beyond a `None` check).

| Law | Value | Confidence |
|---|---|---|
| type -> response | 2 LP, 3 HP, 4 BP (1 SVF); 5 LP, 6 HP, 7 BP, 8 notch (2 SVFs); 9 LP (3); 52 LP, 53 BP, 54 HP (1); 55 LP, 56 BP, 57 HP (2) | medium for 2..9 (KSP `$FILTER_TYPE_*` order), low for 5x |
| cutoff | `43.6 Hz * 2^(8.96 x)` | medium (matches 2827 Hz at the typical stored 0.68) |
| resonance | `Q = 0.707 * 28^x` | low |
| EQ freq | `20 Hz * 10^(3 x)` | medium |
| EQ bandwidth | `0.3 + 2.7 x` octaves, `Q = 1/(2 sinh(ln2/2 * bw))` | low |
| EQ gain | `18 (2x - 1)` dB, bell `A = 10^(dB/40)` | medium |
| modulation | internal AHDSRs, velocity/key/CC sources add to the normalized knob, negated when the invert button is on; `$ENGINE_PAR_CUTOFF`/`RESONANCE`/`FREQ1..3`/`BW1..3`/`GAIN1..3` = x/1e6 | medium |
| output gain | per slot, linear; `$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN` (and the racks' output gain and dry level) = `16 x^3` (+24 dB): Afflatus 2 Horns sets 396851 on a group slot storing 1.0000056, and 125919 on a send storing 0.0319443 | high |
| Stereo Modeller | spread `2x - 1` (Afflatus sets 434210, stores -0.13158; Solo 500000 stores 0), side scaled by `1 + spread`, then the rack's pan law (far side `1 - |pan|`); `$ENGINE_PAR_STEREO_PAN` assumed `2x - 1` | medium |

Channel mixing: group Stereo Modellers (on every Vista group, e.g. Cellos pan -0.47, gain
1.26; also Solo, Afflatus, Pacific, Una Corda, Areia), Inverters and every active slot's
output gain are linear and act on both channels alike, as the filters do, so they compose
into one 2x2 matrix applied after the filters (ramped over a block when a script changes
it). Inverters are gain trims across the corpus (Vista 1.005, Dolce 0.5..1.59, Una Corda
2.0 on 281 groups): their two flags (layout `flag_0, flag_1`, meaning unverified) are false
on every active instance but one in Una Corda, which is warned about. Group slots store
`dry_level` 1.0 everywhere; it is ignored (adding it would double the signal). Bypassed
filters, EQs and mixers are kept so `$ENGINE_PAR_EFFECT_BYPASS` (group >= 0) switches them.

Filter types used in the corpus (all 44,497 group slots): 2, 3, 6, 52, 54, 55, 57 and EQs
22..24. Directions check out: 3 is HP (cutoff 0 with a sweep envelope), 2 LP (Harp
envelope), 52 LP (Solo opens it with cutoff 1e6), 54/57 HP and 55 LP (Areia's stored open
values). No slot uses a ladder or 1-pole type beyond these, so none is modelled. Type 19
(Una Corda "Room Noise Hiss", bypassed, 13 floats: cutoff, resonance, three types, three
bypasses, two shifts, two resonances, gain) is the Versatile filter the script names;
type 51 (Una Corda RESONANCE groups, bypassed, 2 floats) is unknown. Both pass through
and are warned about if enabled.

EQ gain envelope invert (Vista legato groups: AHDSR attack 0, decay ~139 ms, sustain 0,
intensity 1 on eqGain2/3 of a 0 dB EQ): the flag is read correctly, not backwards. Vista
stores invert on 50 of 64 such routes; mic siblings with identical shapers differ only in
the flag (Cellos `dc legatodyn1` inverted, `cl legatodyn1` not), and the flag byte
(`0x12`) is the same on all. Group 16 (`cl legatodyn1`), the +7 dB treble case, is one of
the 14 outliers; flipping the reading would make the other 50 boost instead. Kontakt's
default is non-inverted, so the designers pressed invert on most routes, which reads as a
cut at onset. This is in tension with the volume invert flag, which stays ignored because
sibling mic copies disagree on it (MODULATION.md); the two readings are unreconciled.

Cost (`cargo test --release --no-default-features --lib engine::filter::tests::bench --
--ignored --nocapture`, per voice, 128-frame stereo blocks of noise at 48 kHz, load ~12,
three alternating runs each):

| Group | Before ns/block | After ns/block | 100 voices after |
|---|---|---|---|
| Vista Cellos legato (EQ3, HP, 2 envelopes) | 2,796..2,922 | 806..916 | 3.1% of the block |
| Vista Cellos sustain (Stereo Modeller only) | not applied | 41..59 | 0.2% |
| Vista Harp (LP, envelope) | 1,027..1,060 | 1,013..1,031 | 3.8% |
| Solo Pads (2 filters, Stereo Modeller) | 1,212..1,314 | 1,322..1,387 | 5.0% (the Stereo Modeller is new work) |

Not implemented: Versatile (19) and type 51 (warned on import), pseudo stereo and
Inverter flags (warned), the amp split point (filters run after the amp envelope),
`get_engine_par_disp` strings (still the raw value; Kontakt's formats are not recorded
locally). Ladders, phaser, formant, Solid G-EQ and the drive effects are covered below.

## Effect blocks and model filters (2026-10-01)

Inventory: `kontakto audit-dsp <root>` lists every filter type, effect (group, insert,
send, bus, main), modulator, modulation target and script-set `$ENGINE_PAR_*` /
`$FILTER_TYPE_*` / `$EFFECT_TYPE_*` in the corpus, static and from each instrument's init
callback, ranked by instruments using it, and whether it plays. 788 instruments: 214 of
300 items did not play before this work, 72 of 304 after (left: modulation the importer
skips, `MOD_TARGET_INTENSITY`, `GN_GAIN`, convolution IR shaping, Rotator, Jump, legacy
Reverb, Versatile, type 51, group Compressor, Inverter flags, ANALOG STRINGS' LFOs).

Filter type ids (`engine::filter::filter_type`, `KSP_FILTER_TYPES`):

| Id | Type | Evidence | Confidence |
|---|---|---|---|
| 100..105 | AR LP2, LP4, HP2, HP4, BP2, BP4 | ANALOG STRINGS' script sets `$FILTER_TYPE_AR_LP2` on exactly the groups storing 100 | high for 100, medium for the run |
| 106, 107 | Daft LP, HP | next in reference order; stored 106 on 283 ANALOG STRINGS groups | low |
| 13 | Phaser | stored by ANALOG STRINGS, the remaining unexplained id | low |
| 70, 71 | Formant 1, 2; 90 formant too | as above | low |
| 52..57 | SVF 1 and 2 section LP/BP/HP (not AR) | Solo, Areia, Una Corda store them with SVF knobs | medium |
| 19 | Versatile | Una Corda's script names it on the groups storing 19 | medium |
| SV_NOTCH4 | 1000 (no preset stores it) | | none |

`$ENGINE_PAR_EFFECT_SUBTYPE` (group slots) switches the filter type: the script's value is
the type id itself (ANALOG STRINGS sets 106 on slots storing 106). All are the linear
response of the analog topology factored into the same TPT sections
(`src/engine/filter/models.rs`): ladders are 2 or 4 one-pole stages in a feedback loop,
pass band compensated (AR); formant: three vowel formants
as peaking sections, Talk morphing A-E-I-O-U, Size shifting, Sharp narrowing them
(`FORMANT_TALK/SHARP/SIZE`); phaser: four first-order all-passes summed with the input
(its feedback is not modelled).

### Daft response correction (2026-10-02)

The [NI filter reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference)
describes Daft LP and HP as two-pole filters with a 12 dB/octave slope.
Subtype 106/107 now selects the existing two-pole SVF with unity pass-band
gain, correcting the previous four-pole ladder mapping. ANALOG STRINGS has 283 stored type-106 group slots, including
261 active slots with the same cutoff/resonance settings.

**Approximation remains:** the existing SVF provides a stable linear response,
not the nonlinear Massive algorithm. It uses the existing cutoff and Q laws;
resonance peaks at the cutoff and does not add a feedback-related pass-band
loss. This avoids the large attenuation of the uncompensated two-pole ladder
proxy. No new gain/resonance law was invented; exact Kontakt gain, resonance,
saturation, and waveform parity still require reference renders.

Rack effects (`src/fx/blocks.rs`; stored values are what presets hold, scripts set
normalized x = value/1e6 through a law):

| Effect | DSP | Laws (stored) | Confidence |
|---|---|---|---|
| Compressor | dB peak detector, linked, attack/release one-poles | threshold -60..0 dB linear, ratio `50^x`, attack `1000 x^3` ms, release `5000 x^3` ms | low |
| Limiter | input gain into a 0 dBFS ceiling | in-gain -24..+24 dB: ANALOG STRINGS' script sets 500011 on a slot storing 0.00053 dB, `(0.500011 - 0.5) * 48`; release `10 * 100^x` ms (starts at the stored 10 for 0) | high for in-gain, low for release |
| Solid Bus Comp, Feedback Compressor | as the compressor, feedback detector for the latter | stepped ratio/attack/release, makeup | low |
| Delay | stereo ring, ping-pong pan, damping low pass, feedback | time `2000 x^3` ms; `*_UNIT` raw (`-1` stored, read as free ms; a sync unit reads time x 125 ms, 120 BPM) | low |
| Chorus, Flanger | modulated delay lines, feedback (flanger) | normalized | low |
| Phaser | 6 allpasses | normalized | low |
| Transient Master, Lo-Fi, Distortion, Skreamer, Tape Saturator, Saturation | per-sample shapers (Pade tanh), Lo-Fi bit/rate reduction | normalized; Saturation shape -1..1 | low |
| Filter, EQ, Solid G-EQ | the group filter sections (`RackFilter`) | as group filters; G-EQ +-15 dB, bands 30-450, 200-2500, 600-7000, 1500-16k Hz, shelves unless the bell switch is on | low |

Effect `0x1d` (class name Surround Panner) is Saturation: `$ENGINE_PAR_SHAPE` reaches it
and its first float is -1..1. Group slots of the drive kinds, Compressor and Solid G-EQ run per voice
(with eight fixed drive/dynamics states); the group modulation
targets `shaper`, `distortionIntensity`, `bitdepth` and `downsample` move them
(normalized). A rack Filter/EQ storing output gain 0 and dry level 0 is read as unset
(unity): ANALOG STRINGS' active insert EQ is stored so and no script sets it. Bypassed
send slots return nothing.

Level checks (`kontakto render`, before and after): Dolce, Pacific, Vista, Solo, Una
Corda, CHORUS and Afflatus 2 Horns unchanged. Mega Brass +1 dB (Skreamer, Saturation;
one Skreamer stores +10.5 dB output, so audit-libraries' chord is 11 dB louder). ANALOG
STRINGS +8 dB (one note) / +11.6 dB (audit-libraries chord): the compressor's stored
+9 dB output gain now applies. None of these levels is checked against Kontakt.


### Native group insert order and Amplifier split

The [NI signal-flow manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view)
defines eight group insert slots and an Amplifier split: modules execute in slot order,
with the chosen rightmost modules after the Amplifier. The native parser's
`fx_idx_amp_split_point` is preserved as `Group::amp_split_slot` (0 all after,
8 all before). The compiled per-voice chain applies each module's output gain and
Stereo Modeller in its own slot, and applies the existing voice envelope/gain/pan ramp
at that split. Previously inserts all ran after the Amplifier and all output gains
collapsed to one final matrix, which changes nonlinear detector/shaper inputs.

Group Compressor now reuses the existing bounded rack compressor and its existing
parameter laws, with independent detector state per voice. This is the generic model
above, with the same stated confidence; it does not establish Kontakt algorithm or
sonic equivalence. Unknown Amplifier metadata retains existing post-Amplifier routing
with a diagnostic and does not enable a new compressor. Active pre-Amplifier inserts
use their own voice processing, as do inline gains/mixers before a later active filter
and pending inline matrix ramps: their section states are not the canonical states
used by the shared lane. The shared post-Amplifier linear optimization remains for
chains where it is applicable. The unfiltered SIMD path is unchanged.

Feedback Compressor, Limiter and Solid Bus Compressor use that same fixed Comp
state and their existing rack parameter laws when native Amplifier placement is
known. Opaque parameter records and unknown placement stay unsupported. The added
family routing has one authored group/rack PCM-reference regression covering both
Amplifier sides, changed native control readback and allocation guards; validation
passed on the release build. No locally installed authentic group preset of these
three families was available for a control/audio check.

Group [Transient Master](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference)
similarly reuses the existing fixed rack envelope-shaping
model and its decoded input/attack/sustain/smooth fields with known Amplifier
placement. Its constructor and reset are shared with the rack. One authored
regression compares burst attack/body/silence PCM and native attack/sustain edits
on both Amplifier sides, guards allocations and reports the actual state sizes.
This separate follow-up passed on the release build; no authentic local group
preset or Kontakt audio reference was available.

The fixed filter/EQ capacity now covers all eight native insert slots and up to
32 two-pole sections (eight four-band EQs), instead of four units/eight sections.
The render loop visits compiled units and their actual sections; empty storage is
not processed. The shared lane still accepts at most four active sections and
larger chains use the per-voice path. One authored eight-EQ rack-reference check
covers Amplifier splits 0/4/8, slot 7 native gain readback, changed finite PCM and
allocation guards, and passed on the release build. This addresses a capacity
warning observed in external library reports; no installed authentic over-capacity
group was available. Section state plus tuning keys add 1248 fixed bytes per voice
(1.22 MiB for 1024 voices); the temporary control rows add 192 stack bytes. The
compiled test measured Section at 40 bytes and VoiceFilter at 3052 bytes. The
Transient Master state is 36 bytes and fits the existing 72-byte VoiceEffect
enum, adding no fixed voice storage.

On the previously checked Rust 1.98.1 release build, both the old Drive and the new VoiceEffect
enum are 72 bytes. The fixed voice state adds 128 bytes for per-slot gain/mixer
smoothing and a four-byte type revision counter, and removes the old 16-byte
aggregate smoothing matrix: 116 bytes of added inline fields per voice (116 KiB
for 1024 voices); VoiceFilter is 1804 bytes. Live filter subtype
changes invalidate shape-sensitive coefficient caches even when cutoff/resonance are
unchanged. Chains beyond the native eight-slot/32-section bound, opaque filter subtypes,
unimplemented group send/dynamics families and reverb/IR shaping gaps remain explicit.

Validation: the split 0/6/8 rack-reference PCM, changed native compressor threshold,
live same-knob subtype retuning and interleaved-state eligibility regressions passed,
as did the 83 playback tests. Three local factory states preserved split 8 for all
six selected groups. Matched scripted parent gates and both layers' native Saturation
readback produced changed PCM in each state. Their native routing and an explicit
split-0 reference completed 4500 resident Engine::render calls without allocations;
all PCM was finite and peaks stayed below 1. The reference changes only authored
routing metadata in the same engine and is not Kontakt audio. These are functional
checks; they do not establish throughput or Kontakt algorithm equivalence.
