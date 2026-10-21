# Kontakt 8 Bass Pro (k-amps-a, slug basspro)

Scope: `BEffectBassPro` (DSP class), `BParFXBassPro` (stored record, module id 0x61, version 0x10,
reader 0x140d00450, `Fx17Bx5`). Status: **recovered structure and knob laws; no DSP implemented, no
`AmpModel` variant, no `amp()` mapping.** The native emulation harness does not yet give a validated
reference (see "Native emulation"), so no model was checked against the measured render.

## Evidence

- Decompile: `source/kontakt8/` (READY present). Functions are cited by address only.
- Binary: `re-binaries/Kontakt8.exe`, read with pefile for vtable slots and rip-relative targets.
- Bindings: `docs/re/kontakt-bindings/fx_other.tsv` (Bass Pro rows, field index = slot offset).
- Measurement: `/home/derpcat/.cache/kontra-amps-a/basspro.npz` (36 steps, ramp stimulus, 0.6 s
  input, 0.9 s renders). Compared under the coordinator's protocol: ignore the first 0.1 s, search
  the lag per step, compare steady-state windows only.

## Stored record and knob order

The reader writes 17 floats at record offsets 8 + 4k (k = 0..16) and 5 bytes at 0x4c..0x50. The
bindings table gives the same offsets (slot column).

| field | knob | record offset | apply argument |
| --- | --- | --- | --- |
| 0 | GAIN | 0x08 | 6th |
| 1 | BASS | 0x0c | 8th |
| 2 | MID | 0x10 | 9th |
| 3 | MIDFREQ | 0x14 | 11th |
| 4 | TREBLE | 0x18 | 10th |
| 5 | DRIVE | 0x1c | 7th |
| 6 | MASTER | 0x20 | 12th |
| 7..15 | GEQ 40, 90, 180, 300, 500, 1k, 2k, 4k, 10k Hz | 0x24..0x44 | 13th..21st |
| 16 | GEQ VOLUME | 0x48 | 22nd |
| bool | GEQ on | 0x4f | 2nd |
| bool | ULTRA LO | 0x4c | 3rd |
| bool | ULTRA HI | 0x4d | 4th |
| bool | BRIGHT | 0x4e | 5th |
| bool | MONO | 0x50 | 23rd |

The apply order is not the record order: GAIN, DRIVE, BASS, MID, TREBLE, MIDFREQ, MASTER, then the
GEQ bands. The caller (case 0x16 of the switch in 0x1409619b0, 0140960000.c) passes the fields in
that order.

## Object layout and vtable

- Vtable 0x144fc1000 has exactly 32 slots (0..0x1f); 0x144fc1100 is the next class's locator.
  Slots: 0 dtor 0x140a92b30, 1 init(rate) 0x140a9a310, 2 SetSampleRate 0x140ac3a00,
  3 copy-state 0x140a94ed0, 5 reset 0x140ab71e0, 7 SetOutputBuffer 0x140ab9a30,
  12 SetChannels 0x140abd610, 0x15 is-active 0x140a98850, 0x17 process 0x140a9ff80.
- Three sub-objects are wave-shaping tables (`NI::DSP::BASSPRO::WaveShapingTables`): BassProIntoV16
  at +0x14c0, BassProV16toV11 at +0x1cf0, BassProR35toV33 at +0x2510. Each is built by 0x140adda10
  with constants from `DAT_144fc3be8` / `DAT_144fc3a8c` / `DAT_144fc3d34` (not yet extracted).
- Channel inputs: pointers at +0x20 and +0x28, channel count +0x124, frame count +0x120. The output
  list at +0xa0 (set through slot 7) and its gate byte +0x1b4 are inferred from the tail's code, not
  from a run. Sample rate stored at +0x14.

## Host protocol (from the vtable functions)

1. ctor 0x140a859b0 (base 0x140a856c0), then init(rate) 0x140a9a310: builds the three tables, sets
   the nine GEQ band defaults (0x140acdd20 per band, k = 0..8), runs the band and master setups, and
   finally calls the reset through slot 5.
2. SetSampleRate (slot 2) stores the rate and runs the SR setup 0x140abe2b0.
3. SetChannels (slot 12) writes +0x124 and +0x128.
4. Apply 0x140ab8a90 (478 bytes) takes the 23 arguments above. It is called only from the effect
   update switch (0x1409619b0), not from the init.
5. Process 0x140a9ff80: the block is split into chunks of at most 32 frames (counter +0x1bc).
   Each chunk runs the ramp updates 0x140acbcb0 (state +0x2e10) and 0x140acbad0 (state +0x2ef0),
   then the kernel 0x140aa8160 (3666 bytes, four-lane SIMD, scratch in the context block at
   +0x9c010 and +0xac010), then the tail 0x140ac21f0 (1777 bytes), which writes the output list.

## Knob laws (apply callees)

Several callees read the knob as a float and map it through the exponential table
(`DAT_14a6b2600`, value/slope pairs). Only the callees named below were read.

| knob | callee | what was read |
| --- | --- | --- |
| GAIN | 0x140acf9f0 | writes +0x2d40 (table value, offset and scaled), +0x2e70 (times a constant when BRIGHT) |
| DRIVE | 0x140acf790 | not read |
| BASS | 0x140acf210 | not read |
| MID | 0x140ad0560 | not read |
| TREBLE | 0x140ad15b0 | not read |
| MIDFREQ | 0x140ad06d0 | not read |
| MASTER | 0x140ad18b0 | writes +0x2d50 (table value minus a constant, scaled) and +0x2e7c |
| GEQ 40..10k | 0x140acfdb0, _ff90, _fb30, _fd10, _fef0, _fbd0, _fc70, _fe50, _fa90 | not read; they feed the band state at +0x2d80 + 4k |
| GEQ VOLUME | 0x140ad0140 | writes +0x2d5c (table value), recomputes +0x2e7c |
| GEQ on | 0x140ad0030 | writes the flag +0x2d3e; recomputes all nine bands through 0x140acdd20 |
| ULTRA LO | 0x140ad1840 | not read |
| ULTRA HI | (inline in apply) | +0x2e88 = `DAT_144fc3f2c` when on, else 0 |
| BRIGHT | (inline in apply) | +0x2d3c flag, +0x2e94 = 1.0, +0x2e8c = 0x3f4b573f, +0x2e70 scaled |
| MONO | (inline in apply) | byte +0x1c0 |

The table form used by these callees is: index = a linear function of the knob, `(short)(int)index`;
0 below zero, linear interpolation inside 0..2399, and a constant (`DAT_14a6b7100`) at 2400 or above.

The per-band callee 0x140acdd20 reads the band gain at +0x2d80+4k through the table. When the GEQ flag
is off it uses a constant instead of the table (not checked, probably 1.0). A second value at
+0x2dc8+4k goes through the table and gives `(A - 1/A) / sqrt(G)`, which is stored per band. The
band frequencies are not yet recovered (see gaps).

## Gap table

| part | status | why |
| --- | --- | --- |
| knob to field binding | verified (apply caller, bindings, reader) | high confidence |
| state, reset, SR setup | partly read | 0x140abe2b0 (1342 bytes) not read |
| GEQ band law | partly read | band frequency mapping and Q not recovered |
| tone stack (bass, mid, treble) | not read | callees 0x140acf210, 0x140ad0560, 0x140ad15b0 |
| three wave-shaping tables | located, not extracted | built by 0x140adda10 |
| kernel 0x140aa8160 | not understood | 3666 bytes of SIMD code with 16-frame groups |
| tail 0x140ac21f0 | partly read | output mixing and level path |
| master, output level | partly read | 0x140ad18b0 law, output path unclear |
| exponential table | repo formula 2^(i/60 - 20) assumed | the binary image at `DAT_14a6b2600` is not that table; the native builder is not found |

No DSP law is claimed as recovered beyond the knob-to-callee map and the table-index form.

## Measurements (basspro.npz, steady window 0.1 .. 0.6 s)

Protocol: ignore the first 0.1 s, lag search per step over +-80 samples against the bypass step,
RMS gain in dB against the bypass in the same window. For distorted or clipped steps the bypass
correlation is weak, so the lag shown is informational only. The meaningful comparison is output
against output (ours against Kontakt), with the same protocol.

| step | lag | corr | RMS dB | peak |
| --- | --- | --- | --- | --- |
| bypass | 0 | 1.000 | 0.00 | 0.766 |
| default (all 0.5, flags 0) | -9 | 0.944 | +4.13 | 0.828 |
| GAIN = 0 | -9 | 0.947 | -98.3 | 0.0145 |
| GAIN = 1 | -2 | 0.828 | +6.22 | 1.000 |
| BASS = 0 | -20 | 0.940 | +3.36 | 1.000 |
| BASS = 1 | 5 | 0.947 | +4.92 | 0.996 |
| MID = 0 | -9 | 0.762 | -10.0 | 0.320 |
| MID = 1 | -7 | 0.888 | +8.60 | 1.000 |
| MIDFREQ = 0 | 43 | 0.723 | +3.88 | 0.813 |
| MIDFREQ = 1 | -9 | 0.944 | +4.13 | 0.828 |
| TREBLE = 0 | 4 | 0.940 | -5.21 | 0.327 |
| TREBLE = 1 | -28 | 0.871 | +8.43 | 1.000 |
| MASTER = 0 | -7 | 0.585 | -100.2 | 0.000 |
| MASTER = 1 | -8 | 0.856 | +9.04 | 1.000 |
| GEQ VOLUME 0 / 0.5 / 1 | -7 / -9 / -9 | 0.944 | +4.13 | 0.828 |
| BRIGHT = 1 | -19 | 0.925 | +4.66 | 0.886 |
| ULTRA LO = 1 | 9 | 0.968 | +3.19 | 0.759 |
| ULTRA HI = 1 | -12 | 0.943 | +4.16 | 0.826 |
| GEQ = 0 / 1 | -9 | 0.944 | +4.13 | 0.828 |
| MONO = 0 | 71 | 0.935 | +3.82 | 0.815 |
| MONO = 1 | -8 | 0.990 | +1.41 | 0.729 |
| DRIVE = 0 | 2 | 0.974 | +3.37 | 0.802 |
| DRIVE = 1 | -22 | 0.882 | +0.53 | 0.587 |

Checks on the protocol:

- Identical-setting steps match after the lag search. GAIN = 0.5, MIDFREQ = 0.5, MASTER = 0.5,
  GEQ = 0 against default give lag 0 and max difference 1.2e-5 to 5.4e-5 (under 0.01 % of peak).
- The MONO = 0 step has the same settings as default but does not match it. Its best correlation
  with default is 0.935 at lag +71, and its smallest max difference is 0.67 at lag +70 (search
  +-150). Its best match among the other steps (within +-30) is 0.70, with MIDFREQ = 0. The mismatch
  is not an onset or lag effect. Treat that step as suspect and re-render it.
- Steps with lag at the search edge (MIDFREQ = 0 at +43 with correlation 0.72) are dominated by
  waveform change, not by delay.

## Native emulation (not validated)

`re/bp_kemu.py` (in `/home/derpcat/.cache/kontra-amps-a/re/`, not in the repo) runs the native object
under kemu: ctor, init, SetSampleRate, SetChannels, output buffers, apply, then process on the
stimulus in 512-frame blocks. Findings so far:

- The NaN output went away once SetSampleRate and SetChannels were called through the vtable. The
  exponential table fill was added in the same change, so its own effect is not isolated.
- Output is not validated: in place (input = output buffer) the output equals the input exactly;
  with separate output buffers it is near zero. Setting the gate byte +0x1b4 makes the tail run,
  and then it faults at 0x140ac27e7 on a load from rdx = 0 (rdx is cleared at 0x140ac27a2 in the
  mix-add path). The host state that feeds that source pointer is not yet known.

## Open items

1. Find the host state that sets the source pointer for the tail mix (the load at 0x140ac27e7).
2. Read 0x140abe2b0 (SR setup) and 0x140ad1840 (ULTRA LO) to complete the state.
3. Extract the exponential table from the native builder, or confirm the repo formula at the bit
   level against a dump.
4. Re-render the MONO = 0 step.
5. Once the kernel is understood, port the chain as a configuration of the shared amp pieces and
   add the `AmpModel` variant and the `amp()` mapping, keeping `unproved()` until the render matches.

## Confidence

- Knob to field mapping and apply order: high (caller, bindings and reader agree).
- Callee list for each knob: high. The law inside each callee: low to medium.
- Kernel, tone stack, GEQ band frequencies, master law, output stage: not recovered.
