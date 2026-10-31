# Kontakt 8 Hot Solo (k-amps-a): measured model

Scope: `BParFXHotSolo` (module id 0x54, stored version 0x10), effect path `Effects/HotSolo`.
Bass Invader shares module id 0x54 at version 0x52 and is a separate record and a separate DSP
class; the reader below throws on any version other than 0x10, so the two never mix.

Status: **modelled at one measured operating point.** The module's linear tone stage is
`ir::EffectKind::Amp` (`crates/format/kontakt/src/hot_solo.rs`), built from a measured transfer
function, not from a reconstruction of Kontakt's coefficients. Electric Mint is the operating
point: its saved record is what the parity render compares, and it is the record the previous
"located, not modelled" note in this file was written about.

## What the code contributed

Read from `/home/dracula/Armory/VSTs/kontakt-8.13.1-re/Kontakt 8.exe` (FileVersion 8.13.1,
SizeOfImage 0xb254000) with Kuna; addresses are VAs.

| Address | Size | What it does |
| --- | --- | --- |
| `0x140ad5440` | 14548 B | The coefficient recalculation. Two dirty flags gate it: `+0x715c` (cleared, then called) once the ramp at `+0x7200` reaches zero. It reads the knob floats, and writes ~600 products of them with the constant tables at `0x144fc3…` (e.g. `dat_144fc4708`, `dat_144fc3630`) into the coefficient block at `+0x7650` and onward. **Not recovered**: the products are Ghidra-ordered SIMD arithmetic over 300+ constants and it was not turned into a law; nothing in this model comes from it. |
| `0x140ad44f0` | 3167 B | The second recalculation. Its body shifts whole 8×u64 blocks (four 16-byte lanes) from one state region to another and zeroes the old one: this is the recursive network's state shuffle and reset, **not** a coefficient law. Called from the process when `+0x715d` is set and the ramp at `+0x71c8` is zero. |
| `0x140aa2ae0` | 272 B | Slot 23, the block process: chunks of `+0x1bc` = 32 frames; before each chunk it runs the pending recalculations (`+0x715c` → `0x140ad5440`, `+0x715d` → `0x140ad44f0`), steps the per-chunk state (`0x140acb910`, `this+0x7160`), then calls the kernel and the output tail. |
| `0x140aab1e0` | 5362 B | The kernel. Float work on four lanes with double-precision recursion at `+0x7650…`. The piecewise shaper's thresholds (`+0x73e0…`, `+0x73f0…`), slopes (`+0x7400…`) and gain terms (`+0x73a0…`) are **not** resolved. |
| `0x140ac21f0` | 639 B | The output tail. One gain per bus, ramped **per sample** toward its target by `(target − now)·dat_144f38638` with `dat_144f38638` = 1/180 (0x3bb60b61), so a moved gain settles in ~180 samples (3.75 ms at 48 kHz); then `out = gain · in`. This is the only gain staging whose coefficient is read. |
| `0x140d01b50` | — | The record reader (v0x10 only): eight floats at +0x08…+0x24, then the two bytes at +0x28 (overdrive channel) and +0x29 (mono). |

The chain of evidence that this module is the whole of the Electric Mint parity gap is the
orchestrator's stage-by-stage null test: the band difference survives every instrument and bus
insert being removed, so no other slot explains it.

## The measurement

Same note (MIDI 60, velocity 100, 1.5 s) through real Kontakt 8.14.1 and through KONTRA, 1/3-octave
band levels from `tools/oracle/compare.py`. Two independent Kontakt/KONTRA pairs
(`/tmp/opencode/oracle-mcp/parity/`, `parity-hf-before/`, both 2026-10-30) agree to **0.32 dB RMS**
band by band, so the curve is a property of the two engines and not of one render's round robin.

The difference below is `Hot Solo`'s own response: KONTRA carries it as a pass-through, so
`Kontakt − KONTRA` is the missing stage.

| Hz | 40 | 50 | 63 | 80 | 101 | 127 | 160 | 202 | 254 | 320 | 403 | 508 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| dB | 0.0 | 3.2 | 4.9 | 5.5 | 5.6 | 3.2 | 5.8 | 3.9 | 4.4 | 5.5 | 5.5 | 7.7 |

| Hz | 640 | 806 | 1016 | 1280 | 1613 | 2032 | 2560 | 3225 | 4064 | 5120 | 6451 | 8127 | 10240 | 12902 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| dB | 8.6 | 9.1 | 11.9 | 13.8 | 16.5 | 17.4 | 18.4 | 19.4 | 20.2 | 20.7 | 20.9 | 19.5 | 17.9 | 15.6 |

Shape: a level step of +4.5 dB above ~50 Hz, a broad rise from 500 Hz, a peak near 5–6.5 kHz at
+21 dB, and a shelf back down by 13 kHz.

## The model

`ir::Amp` — a pre-gain, four bands in series, an output gain — built from a least-squares fit of
that curve as RBJ biquads (`tools/dsp-research/hot_solo_fit.py`, scipy, 80 restarts, bounded; the
script prints these four bands):

| Band | Type | Hz | Q / slope | dB |
| --- | --- | --- | --- | --- |
| 0 | low shelf | 40 | 1.0 | −6.67 |
| 1 | peak | 2903 | 0.374 | +13.69 |
| 2 | high shelf | 4539 | 1.0 | +8.77 |
| 3 | — | — | — | flat |

Stage gain: ×1.672 (+4.46 dB) at the measured record. Fit residual against the measured curve:
**0.81 dB RMS, 1.44 dB worst band**.

Gains: the record's prenormal and master are applied as their ratio to the measured point
(`MEASURED_GAIN · prenormal/0.333 · master/0.420`, and 0 when either is 0). Native support is
thin and is stated where it is used: both knobs mute at 0, and master 0.5 → 1.0 measured +5 dB
where a proportional law predicts +6 (that render clipped at full scale).

## Before and after

`tools/oracle/compare.py`, same Kontakt capture
(`/tmp/opencode/oracle-mcp/parity-hf-before/kontakt-as-capture.wav`), KONTRA rendered by
`kontra-host --example render` from this branch:

| | level (Kontakt − KONTRA) | peak | timbre (band spread) | per-band error, RMS / median |
| --- | --- | --- | --- | --- |
| before (Hot Solo as `Other`) | +3.05 dB | +4.03 dB | 7.04 dB | 11.50 / +7.60 dB |
| after | **+0.02 dB** | **−0.00 dB** | **0.78 dB** | **0.94 / +0.55 dB** |

Per band after, 40 Hz → 12.9 kHz: 0.0, +0.7, +1.2, +0.5, −0.1, −0.4, −1.1, 0.0, −0.1, +0.5,
0.0, +0.6, +1.0, +0.2, +1.4, +1.5, +2.2, +1.5, +0.9, +0.6, +0.9, +1.0, +1.3, +0.5, −0.1, −1.0.
The worst band is 1613 Hz at +2.2 dB; the 13 dB tilt of the gap is gone.

One correction pass was needed: the biquad fit gave ×1.672 (+4.46 dB), and the engine's own
render then read 2.30 dB loud against the same capture, so the stage gain is ×1.283 (+2.17 dB).
That number is calibrated on this comparison, which is why it is stated rather than derived.

## What is still not modelled

1. **The coefficient law (`0x140ad5440`).** The bands above are a transfer function, not the
   module's own filter topology. If the law were read, the bands would become functions of the
   five tone knobs instead of constants.
2. **The five tone knobs' laws (presence, depth, treble, mid, bass).** Only the measured
   combination is modelled; a record that differs by more than 0.001 on any of them is
   reported (`Hot Solo tone knobs not modelled (…)`) with impact Audible, so no instrument
   silently gets Electric Mint's tone. Reading them needs a `.nki` knob sweep: patch the record
   (the reader's offsets are above), render each, band-analyse. The patching and rendering
   method is in this unit's work, but the Kontakt GUI could not be driven reliably enough on this
   machine to complete it (`oracle_load`'s Ctrl+O is intermittent, and a second session held the
   shared Xvfb and null sink for most of the window).
3. **The shaper and the overdrive channel.** `0x140aab1e0`'s piecewise thresholds are unread, and
   the overdrive channel (byte +0x28) is a different voicing with inverted polarity in the native
   render. Electric Mint stores overdrive 0, so its pre-overdrive knob is inactive (measured
   2026-10-21: pre-overdrive 0, 0.5 and 1 give identical output), and the modelled path is the
   non-overdrive one only. A record with overdrive set is reported.
4. **Mono (byte +0x29)** changes which output carries the processed signal in the native render;
   not modelled, and reported.
5. **The kernel's recursion.** Whether the tone path is exactly linear is assumed, not proved:
   the fit is level-independent evidence only in that a 1/3-octave difference taken at one
   velocity is reproduced to 0.8 dB. A two-velocity render through the same stage would settle it.

## Evidence

- Binary: `Armory/VSTs/kontakt-8.13.1-re/Kontakt 8.exe`. Decompile: Kuna
  `decompile-all --addr …` of the eight addresses above (kept at `/var/tmp/hotsolo-re/funcs.c`);
  constant `dat_144f38638` read from `.rdata` at file offset 0x4f36838.
- Measured pairs: `/tmp/opencode/oracle-mcp/parity/kontakt-as-capture.wav` + `kontra.wav`,
  `.../parity-hf-before/…`, band levels by `tools/oracle/compare.py`.
- Module record, DSP class, process entry and output tail: [k-amps-a-hotsolo.md](k-amps-a-hotsolo.md)
  (unchanged; this file adds the measurement and the model).
- Record values in Electric Mint: prenormal 0.333, pre-overdrive 0.470, master 0.420,
  presence 0.962, depth 0.603, treble 0.603, mid 0.752, bass 0.643, overdrive 0, mono 0
  (insert slot 3 of the program's 0x3a rack, read by `kontra-format-kontakt`'s own parser).
