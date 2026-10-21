# Kontakt 8 EP Preamps (k-amps-a, slug epp): recovered layout and topology

Scope: effect class `BParFXEPPreamps` (module 0x6C, stored versions 0x10 and 0x11), its DSP `BEffectEPPreamps`, and the core `ni::effektrig::dsp::epianopreamps::EPianoPreamps` that the DSP owns.

Status: **the stored layout, the class and vtable map, the enum-to-id scales, the mode quantizers and the core's per-frame structure are recovered from the decompile. The parameter names of the floats, the drive tables, the EQ coefficients and the modulation laws are not. No engine model is implemented.**

## Validation

- **No measured render exists.** The EP Preamps preset folder is not on this Kontakt install (see `k-amps-a.md`). Nothing below is checked against audio.
- Evidence is the decompile (`source/kontakt8/`, 1113 buckets) plus constants and vtables read from `Kontakt8.exe` with `pefile`. Addresses are function starts in the decompile.
- Offsets: "effect +X" means inside the 0x6680-byte `BEffectEPPreamps` object; "core +X" means inside the core object, which sits at effect +0x1c0. The process and mode functions use core offsets.
- Derived invariants are stated where they follow from the code (for example, the stored enums and the core quantizers invert each other).

## Classes and vtables

| Object | Evidence | Notes |
| --- | --- | --- |
| Stored effect `BParFXEPPreamps` | reader `FUN_140ac5570` (v10 and v11); `ser_classes.tsv` vtable RVA 0x4e74310 | Reads the stored fields below. |
| DSP effect `BEffectEPPreamps` | RTTI COL, vtable VA 0x144e92f58; ctor `FUN_1407c73e0` | Effect object is 0x6680 bytes; the core is at effect +0x1c0. |
| Core `EPianoPreamps` | RTTI COL, primary vtable VA 0x147698370 (second base 0x147698428); ctor `FUN_1419446d0` (6103 B) | Constructor copies coefficient tables from image data. |
| Core process | core vtable slot 19 (+0x98) = `FUN_14194e240` (4359 B) | Per-frame loop over the block; 4-lane modulation, drive, EQ, modulation switch, output. |
| Effect block loop | effect vtable slot 23 = `FUN_1408ff1a0` | Runs the core in sub-blocks (default 32 frames, effect +0x1bc). Tracks the peak. Slot 23 differs from the Fuzz vtable, so it is this class's own instance. |
| Parameter setter | effect vtable slot 32 = `FUN_1409aa760` | `setParam(id, value, force)`: stores at effect +0x64dc+4·id, then calls `FUN_141952ba0` on the core with the value and the value clamped to [0, 1]. |
| Stored-to-parameter sync | effect vtable slot 29 = `FUN_140aba5e0` (636 B) | Writes numeric ids from the stored enums. It also calls a virtual on the nested block with the stored float at +0x20, which is not interpreted. |
| Mode update | `FUN_141949310` (2357 B), core-relative | Derives the drive and EQ modes from core floats +0x6a8 and +0x6b0; clears state on change. |

## Stored layout (reader `FUN_140ac5570`)

Schema from `reader_schemas.tsv`: v10 `Fx11BBIx4`, v11 `Fx11BBIx4Bx4`. Reader order is the order below.

| Offset (effect) | Type | Reader | Use |
| --- | --- | --- | --- |
| +0x08, +0x0c, +0x10, +0x14 | float | `FUN_142a780f0` | Parameter names not recovered. |
| +0x1c, +0x20, +0x24, +0x28 | float | `FUN_142a780f0` | Parameter names not recovered. |
| +0x48 nested (`FUN_140ab5da0`) | float, float, float, byte | | Floats at +0x60, +0x6c, +0x70; byte at +0x51. |
| nested +0x50 | derived bool | | Set when the first nested float is above about 1e-9 (constant 0x144737708). Not stored. Meaning not proved. |
| +0x30 | byte | `FUN_142a780c0` | Parameter name not recovered. |
| +0x34 | enum 0..4 | `FUN_140a847f0` | Feeds numeric id 0 (scale 0.25). |
| +0x38 | enum 0..5 | `FUN_140a84870` | Feeds numeric id 2 (scale 0.2). |
| +0x3c | enum 0..6 | `FUN_140a84920` | Feeds numeric id 0xb (scale 1/6). |
| +0x40 | enum 0..4 | `FUN_140a847f0` | Feeds numeric id 0x12 (scale 0.25). |
| +0x18, +0x19, +0x1a, +0x1b (v11 only) | 4 bytes | `FUN_142a780c0` | Read only in v11. Meaning not proved. |

Counts: 11 floats (8 direct, 3 nested), 2 stored bytes (+0x30 and nested +0x51), 4 enums, and 4 more bytes in v11. An enum outside its range throws in the reader, so the ranges are hard limits.

## Parameter ids and names

The binary's name table for this module (`$ENGINE_PAR_EPP_*`, in declaration order, 18 names):

0 DRIVE_MODE, 1 DRIVE, 2 EQ_MODE, 3 EQ_BASS, 4 EQ_MID, 5 EQ_TREBLE, 6 ELECTRIC_C_SOFT, 7 ELECTRIC_C_MEDIUM, 8 ELECTRIC_C_TREBLE, 9 ELECTRIC_C_BRILLIANT, 10 PASSIVE_BASS, 11 TREMOLO_MODE, 12 TREMOLO_WAVE, 13 TREMOLO_RATE, 14 TREMOLO_RATE_UNIT, 15 TREMOLO_AMOUNT, 16 TREMOLO_WIDTH, 17 MONO.

The sync writes these numeric ids; the scales are `float` constants from the binary:

- id 0 ← enum(+0x34) × 0.25
- id 2 ← enum(+0x38) × 0.2
- id 0xb ← enum(+0x3c) × 1/6
- id 0x12 ← enum(+0x40) × 0.25

The sync writes other ids (0, 1, 3 to 10, 0xc, 0xd, 0x14 to 0x16) with values the decompile does not recover, so their defaults are unknown.

**Hypothesis, not proved.** The enum scales match the core quantizers exactly (below), so id 0 is probably DRIVE_MODE and id 2 is probably EQ_MODE, matching the first and third names. Id 0xb (7 values) is a candidate for TREMOLO_MODE, because the post-EQ switch has six non-zero cases (see Core block). Two things conflict with a plain index mapping: id 0x12 (18) is past the 18-name table, and the 5-valued enum at +0x40 has no obvious name. The name-to-id table is not recovered.

## Mode quantizers (core, `FUN_141949310`)

- Drive mode = truncate(5 × x), clamped to 0..4, where x is core float +0x6a8. A negative product gives 0. On change the function calls two helpers, `FUN_141953b50` and `FUN_14194aa60`, that were not read.
- EQ mode = truncate(6 × x), clamped to 0..5, where x is core float +0x6b0. On change the function zeroes the filter states, and EQ mode 3 also sets its coefficients in the same function.

With the sync scales, stored enum k gives drive mode k and EQ mode k for every legal k. This is the strongest evidence that +0x34 is the drive mode and +0x38 is the EQ mode.

## Core block (`FUN_14194e240`, per frame)

Shown in the order the code runs. Laws are not recovered.

1. **Modulation pass (gated by core byte +0x7d5).** Four-lane float blocks are advanced by per-lane constants from the table at 0x144714b70 (lanes 0 and 1 are 1.0) and scaled by core gains +0x7e0, +0x7e4, +0x7e8 and +0x7ec, against two tables at core +0x758 and +0x7a0. This looks like the LFO state for the modulation, but its mapping is not proved.
2. **Drive (only when drive mode ≠ 0).** Input gain is core float +0x62e8. Then a table shaper, selected by drive mode: modes 1 and 2 call `FUN_14194ce60` on two tables per channel (core +0xa90/+0xe20 and +0x11b0/+0x1540); modes 3 and 4 call `FUN_14194cfb0` on core +0x26b0/+0x2da0 and +0x18d0/+0x1fc0. Then a zero-delay (trapezoidal) two-integrator stage (state +0x351c and +0x3520, gain +0x3514, +0x3518, +0x3510), with a 0.5 averaging step (constant 0x14470bad0). Core float +0x62ec multiplies the stage output. Mode 0 skips the shaper and this stage.
3. **EQ (by EQ mode 0..5, double precision, per channel).** Mode 0 bypasses. Mode 1 is an inline two-section recursion; mode 2 calls `FUN_14194e030` on two states; mode 3 is an inline recursion plus `FUN_14194d930` on two states; mode 4 calls `FUN_14194d1e0`; mode 5 calls `FUN_14194d4d0`. Coefficients come from the mode-update path and from the constructor tables.
4. **Modulation switch (core int +0xa88, cases 1 to 6; 0 or other values bypass).** Each case multiplies the two channels by a gain built from modulation helpers. Cases 1 and 6 use `FUN_14194d120` twice plus `FUN_14194e100` (case 1, depth core +0x5b50) or `FUN_14194e180` (case 6, depth core +0x6038). Cases 2 to 5 use `FUN_14194d120` once per channel with their own fields. Cases 2 and 4 normalise with a square root and keep separate per-channel states, which may be the width behaviour. The switch variable is not yet tied to a stored field; see the enum hypothesis above.
5. **Mono and output.** Core byte +0x62f0 selects one summed channel scaled by 0.5 instead of two channels, before the drive. Final output per frame = sample × core float +0x62f4 × the smoothed level at core +0x62d4.
6. **Level smoothing.** Core floats +0x62d0 (coefficient), +0x62d4 (current), +0x62d8 (target) and +0x62dc (threshold). When the distance to the target is below the threshold, the code snaps current to target only if byte +0x62e0 is set, and then clears it and sets byte +0x62e1. Otherwise it moves one pole toward the target with coefficient +0x62d0. The source of the target is not traced.

Constants read from the binary: 5.0 (drive scale, 0x1447149f0), 6.0 (EQ scale, 0x144f38bd0), 0.25, 0.2 and 1/6 (id scales), 1.0 and 0.5 (clamp and averaging), 0x7fffffff (absolute-value mask), and about 1e-9 (nested bool).

## Gap table

| Piece | Recovered | Missing |
| --- | --- | --- |
| Stored layout | Offsets, types, enum ranges, counts, v10 and v11 difference | Names of the 8 direct floats, the 3 nested floats and the stored bytes; meaning of the v11 bytes |
| Parameter ids | Four enum-to-id scales; names in binary order | Id-to-name table; defaults of the other ids |
| Mode quantizers | Drive 0..4 from 5x; EQ 0..5 from 6x | Which parameter writes core +0x6a8 and +0x6b0 |
| Drive | Stage order; mode-to-shaper call map; gain placement | Table contents and shaper law; input and stage gain values; stage coefficients |
| EQ | Mode switch; state layout; helper per mode | Coefficient laws and helper bodies (`FUN_14194e030`, `FUN_14194d930`, `FUN_14194d1e0`, `FUN_14194d4d0`) |
| Modulation | Four-lane block shape; six-case switch; depth fields | Rate, wave and depth laws; the helpers `FUN_14194d120`, `FUN_14194e100`, `FUN_14194e180`; which field selects the case |
| Width and mono | Mono sum; two-channel path in cases 2 and 4 | Width law as a named control |
| Level smoothing | One-pole glide with snap flag | Source of the target; constants per parameter; `FUN_141952ba0` body |
| Sample-rate setup | Not located | Coefficient law for the drive, EQ and modulation filters |

## Why no model is implemented

The core's coefficients come from several helpers of 300 to 2800 bytes each, plus constructor tables, and the modulation switch has six laws whose field and tables are not yet tied together. Reproducing it in f32 and f64 with the same operation order is a multi-function port, and nothing can check it: there is no render. The brief forbids guessing constants, so no `AmpModel` variant or `amp()` mapping is added. `unproved()` in `crates/sampler-kontakt/src/effects/color.rs` continues to report 0x6c.

## Next

1. Name the 8 direct floats and the stored bytes by tracing the caller of sync `FUN_140aba5e0` and the apply path of the BParFX object. Apart from +0x20 (passed to the nested block's virtual), the sync does not read the direct floats.
2. Tie the modulation switch variable (core +0xa88) to its stored enum and check the enum-to-case mapping, including the float truncation of 6 × (k/6).
3. Port the mode-update path and the drive shaper first (smallest closed piece), then the EQ helpers one mode at a time, starting with mode 1.
4. Measure once a preset containing EP Preamps is available, then fit the modulation rate law.

## Confidence

- Stored layout and enum ranges: high (the reader code is direct).
- Class and vtable map: high (RTTI and vtable reads agree with decompile names).
- Enum-to-id scales and the drive and EQ quantizers: high for the arithmetic, medium for the id-to-parameter link.
- Name-to-id mapping and the enum-to-modulation-case link: low (hypotheses only).
- Core block structure: medium-high (whole process body read; helper bodies not read).
- Any DSP law: none recovered.
