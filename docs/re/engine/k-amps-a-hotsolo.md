# Kontakt 8 HotSolo (k-amps-a): module record

Scope: `BParFXHotSolo` (module id 0x54, stored version 0x10), effect path `Effects/HotSolo`. Part of the amp unit in [k-amps-a.md](k-amps-a.md). Bass Invader shares module id 0x54 but is version 0x52 and is a separate record.

Status: **located, not modelled.** The stored fields, the DSP class and its process entry are identified. The transfer, tone and output laws are not recovered, so nothing here is a spec for DSP, and no engine code was added. HotSolo stays in `unproved()` in `crates/sampler-kontakt/src/effects/color.rs`.

## Stored record (reader 0x140d01b50, version 0x10)

The reader is 0x140d01b50 (`ser_classes.tsv`, `reader_schemas.tsv` row `Fx8BB`). It throws on any version other than 0x10.

| Index | Record offset | Kind | ENGINE_PAR (oracle name) | Binding in `fx_other.tsv` |
| --- | --- | --- | --- | --- |
| 0 | +0x08 | float | HS_PRENORMAL | yes (field 0, slot 8, identity) |
| 1 | +0x0c | float | HS_PREOVERDRIVE | yes (field 1, slot 12) |
| 2 | +0x10 | float | HS_MASTER | yes (field 2, slot 16) |
| 3 | +0x14 | float | HS_PRESENCE | yes (field 3, slot 20) |
| 4 | +0x18 | float | HS_DEPTH | yes (field 4, slot 24) |
| 5 | +0x1c | float | HS_TREBLE | yes (field 5, slot 28) |
| 6 | +0x20 | float | HS_MID | yes (field 6, slot 32) |
| 7 | +0x24 | float | HS_BASS | yes (field 7, slot 36) |
| 8 | +0x28 | byte bool | HS_OVERDRIVE (channel select) | **no row** |
| 9 | +0x29 | byte bool | HS_MONO | **no row** |

- The eight floats are read by one reader helper and the two bools by another, so each record slot is typed as its helper implies.
- The binding table has no rows for the two bools. Their names come from the oracle spec (`tools/kontakt-oracle/ampspecs_a.py`) and their offsets come from the reader. Add the rows before mapping.
- The existing `amp()` arm in `sampler-kontakt` cannot be extended yet, because the transfer and tone cores are missing.

## DSP class and entry points

- RTTI `.?AVBEffectHotSolo@@` (type descriptor RVA 0xa3bb018). Primary vtable RVA 0x4fc0900 (VA 0x144fc0900), 32 slots. The second COL candidate has no vtable reference and was ignored.
- Slot 0 is the deleting destructor (0x140a92e90). Slot 1 is the init (0x140a9b360, 2460 bytes). Slot 3 (0x140a95f40, 592 bytes) copies another object's state block, including the dirty flags. Slot 5 (0x140ab7a60, 639 bytes) is the post-init recalculation path. It calls slot 26 (0x140a93ae0) and then the recalculation functions below.
- **Slot 23 (0x140aa2ae0, 272 bytes) is the block process.** It takes `(this, block, flag)`. It works in chunks of 32 frames, counted at this+0x1bc. Before each chunk it checks the pending-recalc flags. The kernel for a chunk is 0x140aab1e0 and the output tail is 0x140ac21f0.
- Slot 29 is a bare return stub for this class. The `apply` at slot 29 seen in Fuzz is not HotSolo's entry point.
- Init writes 1.0 at +0x7274 and +0x71b4. It also constructs nine sub-objects with 0x140adda10 at offsets +0x27d0, +0x2ff0, +0x3810, +0x4030, +0x4850, +0x5070, +0x5890, +0x60b0 and +0x68d0. Eight of them are initialised with the second flag set and the ninth with it clear. Each sub-object's table comes from a virtual call (slot 1) and an iterative solve. This resembles the effektrig tables in `filter::ladder_kernel`, but that is not confirmed. Which knob each sub-object serves is not established.

### Dirty flags and state

- +0x715c: a coefficient recalculation is pending. The process clears it and calls 0x140ad5440 (14548 bytes) once the ramp value at +0x7200 reaches zero.
- +0x715d: a second recalculation is pending. The process calls 0x140ad44f0 (3167 bytes) once the ramp at +0x71c8 reaches zero.
- +0x7160..+0x7290: a per-block state area, stepped once per chunk by 0x140acb910.
- +0x1c0: a flag that appears to select how the input lanes are combined. When set, the kernel sums the lanes with a 1/N scale, where N is the lane count at +0x124. When clear, it runs each lane separately.
- +0x1a4 and +0x1ac: an output gain and its ramp state. The tail 0x140ac21f0 ramps the gain and adds the result into the output buses. The bus count is at +0x128.

### Kernel 0x140aab1e0 (5362 bytes, SSE and double precision)

- Float work runs on 4-lane vectors, and the recursive state is double precision at +0x7650..+0x76e8.
- The per-lane sums and products suggest a multi-stage recursive network. Its feedback path is not resolved.
- The shaper is piecewise. Two groups of threshold compares on four lanes select between three segments. The thresholds are at +0x73e0..+0x73ec and +0x73f0..+0x73fc. Slopes and offsets come from +0x7400..+0x745c, +0x7420..+0x7438 and +0x7440..+0x745c. Gain terms are at +0x73a0..+0x73ac and +0x73c0..+0x73cc. Which stage each group serves is not resolved.
- The kernel makes no direct calls to other decompiled functions, so its whole topology is inline.
- Not reconstructed. The decompile is Ghidra SIMD output, and matching its float operation order needs more work than this unit allowed.

### Coefficient recalculation

- 0x140ad5440 (14548 bytes) computes coefficients and laws from the cached knob values. It is the largest function in the chain and is not read yet.
- 0x140ad44f0 (3167 bytes) is the second recalculation. It is not read yet.
- Helpers 0x140ad0b40, 0x140ad0ee0, 0x140ad1f10, 0x140abf590 and 0x140acccb0 are called from init and recalc. They are not read yet.

### Knob cache and how the record reaches the DSP

- The path from the stored record into the DSP knob cache (+0x7108..+0x7118) is not located. Function 0x140abcb30 writes that cache and takes the same kind of arguments (two bools and eight floats, with flags stored at +0x1c0 and +0x7104). Its only call site found is factory case 0x15 (the reverb module), and that call passes two arguments. So it is **not** confirmed as HotSolo's apply. It is a lead only.
- The factory containing case 0x15 is 0x1409619b0 in `0140960000.c`. No case 0x54 for HotSolo was found in that bucket. The other buckets have not been searched.

## Measured behaviour (oracle, 2026-10-21, Kontakt 8 under Wine)

Render: `/home/derpcat/.cache/kontra-amps-a/hotsolo.npz`, 30 steps. Stimulus is a 200 Hz ramp from 0 to 1 over 0.6 s at 48 kHz. Each output is 0.9 s long. "Peak" below is the maximum absolute sample over the whole output, and "RMS" is over the 0.6 s stimulus window. Per-step values are not sample-exact (see harness caveats).

- **Bypass.** The bypass render equals the stimulus delayed by 51 samples and scaled by 0.76766 (-2.30 dB). The residual against that model is about 5e-8. The scale factor comes from the render path or an output gain whose source is not identified. Divide it out before any absolute level comparison, or compare against the bypass render.
- **Default** (oracle "pre" values: knobs at 0.5, overdrive 0, mono 0). Left peak 0.730, RMS 0.46. Right peak 0.766 with RMS 0.3134 and correlation 0.988 to the bypass at zero lag (the per-step lag can lower this).
- **Master.** Master 0 mutes left (peak 0.028, RMS 0.0027). Master 1 raises left RMS to 0.82 and peak reaches 1.00, which looks like full-scale clipping. Right is unchanged.
- **Prenormal.** Prenormal 0 mutes left (peak 0.023, RMS 0.0021). Right is unchanged.
- **Depth.** Depth 0: left peak 0.477, RMS 0.32. Depth 1: left peak 0.859, RMS 0.52.
- **Bass.** Bass 0: left peak 0.477, correlation 0.48 with default. Bass 1: left peak 0.820.
- **Treble.** Treble 0: left peak 0.858. Treble 1: left peak 0.608.
- **Presence.** Presence 0: left peak 0.720. Presence 1: left peak 0.694.
- **Mid.** Mid 0: left peak 0.744. Mid 1: left peak 0.737.
- **Overdrive (channel select).** Overdrive 1: left RMS 0.536 and correlation -0.91 with default, so a different voicing and possibly inverted polarity. Not characterised further.
- **Pre-overdrive.** With overdrive 0, pre-overdrive 0, 0.5 and 1 give identical left output. It is inactive while the overdrive channel is off. It has not been measured with overdrive 1.
- **Mono.** Mono 1: right RMS rises from 0.31 to 0.62 and peak reaches 1.00. Left changes to RMS 0.38. Mono changes which output carries the processed signal. This is the largest routing effect found.
- Right's RMS is 0.3131 to 0.3134 in every step except Mono 1. Right does not follow Master, Prenormal, Depth, Bass, Treble, Presence, Mid or Overdrive.

### Harness caveats (affect every model in this unit)

- **Step-to-step alignment is not sample-exact.** Default and overdrive 0 have the same settings, but their first 0.1 s differ (relative residual 0.43). After that they match to within 0.1% (residual 0.001 to 0.003) once a constant lag is removed. The lag differs per step: -2 samples for overdrive 0, -6 for prenormal 0.5. Compare steady-state windows from 0.1 s on, with a per-step lag search, and set any tolerance from that noise floor.
- Silent steps (prenormal 0, master 0) cannot be aligned by correlation. Use the bypass render for alignment in those cases.
- The bypass scale of 0.76766 must be accounted for before any absolute level comparison.

## Gap table

| Part | Located | Recovered | Gap |
| --- | --- | --- | --- |
| Stored fields and reader | yes (0x140d01b50) | yes (8 floats, 2 bools) | bool rows missing from `fx_other.tsv` |
| DSP class and vtable | yes (0x144fc0900) | yes | none |
| Block process and chunking | yes (slot 23, 0x140aa2ae0) | yes | none |
| Dirty flags and recalc triggers | yes | yes | none |
| Knob cache source (record to DSP) | no | no | locate HotSolo's apply in the other buckets |
| Coefficient laws (0x140ad5440, 0x140ad44f0) | yes | no | read and extract |
| Kernel topology and shaper | partly (structure visible) | no | reconstruct the SIMD ops and feedback network |
| Output routing (overdrive, mono) | no | no | identify how 0x1c0 and the overdrive flag select channels |
| Output scale (0.76766 bypass factor) | no | no | identify its source |

## Next steps

1. Locate HotSolo's apply, which writes the knob cache into the DSP object. Search the remaining `source/kontakt8` buckets for writes to +0x7108..+0x7118 and +0x715c.
2. Extract the coefficient laws from 0x140ad5440 and 0x140ad44f0. Record each knob's law from the decompile, and check it against the renders.
3. Reconstruct kernel 0x140aab1e0 as a topology (input gain, shaper knees, feedback, tone sections, output). Confirm against steady-state renders with per-step lag alignment.
4. Add the two bool rows to `fx_other.tsv`. Measure overdrive 1 with pre-overdrive sweeps, and mono 1 with the left input isolated.

## Evidence

- Decompile files: `source/kontakt8/0140d00000.c` (reader 0x140d01b50), `0140a90000.c` (0x140a95f40), `0140a80000.c`, `0140aa0000.c` (0x140aa2ae0), `0140ab0000.c` (0x140ab7a60, 0x140a9b360, 0x140abcb30, 0x140aab1e0), `0140ac0000.c` (0x140ac21f0), `0140960000.c` (factory 0x1409619b0).
- Vtable: RTTI search in `re-binaries/Kontakt8.exe` (pefile, read-only). Class COL 0x98c2a38.
- Measurements: the npz above, analysed with ad hoc `numpy` scripts (not kept). Method: bypass lag and scale by least squares, per-step lag search by cross-correlation, and steady-state windows from 0.1 s on.

## Confidence

- Stored layout and the eight float binding names: high. The two bool offsets: high for position, medium for the names.
- Vtable and block process: high.
- Coefficient, kernel and output laws: none. Nothing here should be implemented as a law.
