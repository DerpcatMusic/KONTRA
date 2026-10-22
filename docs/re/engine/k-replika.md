# Kontakt 8 Replika Delay (module 0x5A, saved v0x12): port status

Status: partial. Only the free time law is ported (through `replika_config`, as before) and
its formula is now written in terms of the decompiled constants. The per-sample core
(`NI::DSP::FXFDelayLight`, object 0x14980 bytes) is not ported: the line ring, feedback,
filters, saturator coefficients and ducking all depend on update code that is not read yet.
Nothing here is measured against Kontakt. Method: decompile text (`source/kontakt8/*.c`) and
`docs/re/kontakt-rdata/constants.tsv` only. No binary, Ghidra, radare2 or oracle was opened.

Address convention: `0x1xxxxxxxx` names are Kontakt 8 functions; file refs are `source/kontakt8/<bucket>.c`.

## Object and entry points

| Part | Address | Note | Confidence |
|---|---|---|---|
| Saved reader v0x12 | `0x140d02250` | Vtable slot 8 of `BParFXReplikaDelay` (vtable `144e750e0`). Layout below. | high |
| Saved writer | `0x140d117f0` | Vtable slot 9, 560 bytes, not read | - |
| Param object ctor | `0x140701a80` (slot at `+0x3e0`, `param_1[0x7c]`) | Sets default object fields; not read for names | - |
| Effect wrapper ctor | `0x1407c7d40` | Allocates 0x14980 bytes; core is `NI::DSP::FXFDelayLight` (`IDSPCoreWrapper`) | high |
| Effect vtable | `144e963f0` | `BEffectReplikaDelay`, slots 0..2 are IDSPCoreWrapper methods | medium |
| Per-quantum wrapper | `0x1408f98d0` | 32-frame quantum; for index 0..52 with enable byte at `+0x14580+i`, clamp [0,1], core vtable `+0x60` setParam(i, v), then `+0x40` update, `+0x38` process. Core object at `+0x1c0` | high |
| Reset defaults | `0x1408ddbc0` | Calls wrapper vtable `+0x100`(index, value) for the defaults table below. Caller not traced | medium-high |
| Core setter | `0x141ac5980` | Stores raw at core `+0x840+4i`, then switch on index 0..52 | high (switch), medium (meaning) |
| Core loop | `0x141aa9480` | Per sample; partly read (see Core loop) | low |
| Saturator | `0x141aa44a0` | `tanh(x*k)` path plus a polynomial state term; state from `0x140d947a0` | low |
| TYPE engine parameter | `set_engine_par` id `0x3d5` (`0140780000.c:5921`) | Integer, range check 0..4, "Invalid Replika delay type!" | high |

## Saved stream (v0x12), reader `0x140d02250`

Order read, with the struct offset each value is stored at. Field numbers are the TSV
`field` column.

| Field | Kind | Struct | Name (TSV) |
|---|---|---|---|
| 0 | int | +0xa8 | TYPE (table 0..4) |
| 1..10 | float | +0x08..+0x2c | TIME, FEEDBACK, LOWCUT, HIGHCUT, SATURATION, TAPEAGE, FLUTTER, QUALITY, DEPTH, RATE |
| 11, 12, 13 | flag | +0x64, +0x65, +0x66 | STEREO, NOISE, PINGPONG |
| 14 | float | +0x88 | TIME_UNIT (table; sync) |
| 15 | float | +0x94 | TIME_UNIT (table, 0 or 500) |
| 16 | float | +0x98 | none in TSV (read, not applied) |
| 17 | flag | +0x79 | none in TSV (read, not applied) |
| 18..21 | float | +0x30..+0x3c | BBDTYPE, AMOUNT, SIZE, MODULATION |
| 22 | flag | +0x68 | DENSE |
| 23..31 | float | +0x40..+0x60 | SHUFFLE, FEEL, ACCENT, PAN, WIDTH, LR_OFFSET, DUCKING_SENSE, DUCKING_RELEASE, DUCKING_AMOUNT |
| 32 | flag | +0x67 | PINGPONG_FLIP |

Reconciliation: the TSV has 30 distinct RDL names on 32 rows (TIME_UNIT appears on fields
1, 14 and 15). The stream has 33 fields. Fields 16 (float) and 17 (flag) have no RDL row.
So 31 fields carry a name, and 2 are read but unnamed.

## Index map (wrapper index 0..52 to core setter)

Kinds come from the switch in `0x141ac5980` (0141ac0000.c:3348). Indices 8, 9 and 29 were
re-read this pass; the rest are carried from the previous pass and not re-verified.
"Default" is the value the reset function `0x1408ddbc0` passes to the same index. `-` means
the reset does not set it. "Tie" is the RDL name the index is bound to; no index has a
name-to-index table in the cache (ENGINE_PAR name ids are not in the decompile text).

| idx | Setter behaviour | Default | Tie |
|---|---|---|---|
| 0, 1 | gain A / B, conversion `0x141a90910` into control block +0x0 / +0x4 | 0.762405 (`144f38814`) | open |
| 2, 3 | linear into +0x10 / +0x14 (live `0x141acc980` / `0x141acc930`) | - | open |
| 4 | crossfade into +0x8 / +0xc: v > 0.5 gives (2(1-v), 1), else (1, 2v) | 1.0 | open |
| 5 | linear into +0x18 (`0x141ac9420`) | - | open |
| 6, 7 | bool at core +0x914 / +0x915 (v >= 0.5), rebuild `0x141abd210` / `0x141acc600` | 0 (idx 7) | open |
| 8 | sub-mode 3: ms = clamp(k·v·4000, 0, 8000) into +0x2c, k = 0.5 or 1.0 (flag +0x14385). Else `0x141a9d4a0` / `9d7c0` / `9d660` | - | **TIME** (mode-3 branch; medium) |
| 9 | integer mode trunc(4v) clamped 0..3 into +0x93c, unchanged while flag +0x14385 is set; drives `0x141acbfb0`; mode != 3 bypasses sub-objects +0xac0, +0x6380, +0xbc40 (`0x141abde30`) | - | open (mode; TYPE 4 would clamp to 3, unproved) |
| 10, 11 | raw into +0x30 / +0x34; enable flags +0x14381 / +0x14382 | 0 / 0.5 | open |
| 12 | three section gains from `0x141abba60`; enable +0x14383 | 0.5 | open |
| 13, 14 | crossfade pairs into +0x1c/+0x20 and +0x24/+0x28 | 0.5 / 0.5 | open |
| 15 | raw into +0x38; enable +0x14384 | 0.3 (`144f38764`) | open |
| 16 | raw into +0x64 | 0.5 | open |
| 17 | max(1-v, K) into double +0x13e18 (K = `1447148a4` 0.001, not checked in code) | 0 | open |
| 18 | v·K into double +0x13e10 | 0 | open |
| 19 | pow law into double +0x13e28 from +0x13e00..+0x13e08 | 0 | open |
| 20..24 | raw store only | - | open |
| 25 | double +0x928 = v·K + K'; `0x141a9dbc0` if +0x916 clear | 0 | open |
| 26 | flag +0x916 (v >= 0.5) selects +0x920 or +0x928 | 1.0 | open |
| 27 | raw | - | open |
| 28 | flag +0x829, then `0x141aca670` | - | open |
| 29 | raw into +0x142b0. If the live flag +0x8 is set: +0x142ac = v and `0x141accd10`; else +0x142b8 marks a pending change | - | **SATURATION candidate** (saturator amount; medium-low) |
| 30 | pow law into +0x3c: 2 ^ ((v · D(1479ab4d8) - D(1479ab4d0) - 69) · D(144f38910)) · D(1447617a0). Last two factors unread | 0 | open |
| 31 | raw into +0x248 chain, enable +0x294 | 0 | open |
| 32 | `0x141ac44a0` on the three sections | 1.0 | open |
| 33 | raw | - | open |
| 34 | flag +0x82b, `0x141abbf60` | 0 | open |
| 35 | pow law into +0x40 | 0 | open |
| 36 | raw into +0x44, `0x141acc9d0` | 0 | open |
| 37 | `0x141abc0d0` on the sections | 0 | open |
| 38 | raw | - | open |
| 39 | sqrt(v) into +0x48, `0x141accd80` | 0 | open |
| 40, 41 | raw into +0x4c / +0x50 | - | open |
| 42 | `0x141acac40` on the sections | 0 | open |
| 43 | flag +0x82a, `0x141acb2b0` | 0 | open |
| 44 | pow law into +0x54 | 0 | open |
| 45 | raw into +0x58 | 0 | open |
| 46 | `0x141acb240` on the sections | 0 | open |
| 47 | raw | - | open |
| 48 | raw through pointer +0x188 | - | open |
| 49 | raw into +0x218..+0x260 chain, `0x141accb60` | - | open |
| 50 | raw into +0x5c, `0x141accb10` | - | open |
| 51 | sub-objects +0x11600 and +0x12a80, index 3, value 0 or 1 (v >= 0.5) | 1.0 | open |
| 52 | raw into +0x68 | - | open |

Tied: 1 of 53 (idx 8 = TIME, mode-3 branch only). Candidate: idx 29 = SATURATION. Open: 51.
The 53 controls are more than the 33 saved fields, so at least 20 indices are derived or
internal. Which ones is not established.

## Parameter table (saved names)

| Name | Field | Slot | TSV law | Core idx | Status |
|---|---|---|---|---|---|
| TYPE | 0 | 0 | table 0..4 | open | ENGINE_PAR 0x3d5 range-checked. NotModeled (no IR type). |
| TIME | 1 | 8 | linear, 2000 ms at 1.0 | 8 | Ported: `replika_time_ms`, free time, mode-3 formula. Applies only in mode 3. |
| FEEDBACK | 2 | 12 | identity | open | UnknownLawOmitted |
| LOWCUT | 3 | 16 | identity | open | NotModeled |
| HIGHCUT | 4 | 20 | identity | open | NotModeled |
| SATURATION | 5 | 24 | identity | 29 (candidate) | NotModeled (saturator needs update path) |
| TAPEAGE | 6 | 28 | identity | open | NotModeled |
| FLUTTER | 7 | 32 | identity | open | NotModeled |
| QUALITY | 8 | 36 | identity | open | NotModeled |
| DEPTH | 9 | 40 | identity | open | UnknownLawOmitted |
| RATE | 10 | 44 | identity | open | UnknownLawOmitted |
| STEREO | 11 | 100 | bool | open | UnknownLawOmitted |
| NOISE | 12 | 101 | bool | open | NotModeled |
| PINGPONG | 13 | 102 | bool | open | UnknownLawOmitted |
| TIME_UNIT | 1 / 14 / 15 | 8 / 136 / 148 | sync table | open | NotModeled (sync path not ported; unit > 1e-9 returns no config) |
| (field 16) | 16 | 152 | none | open | Read, not applied |
| (field 17) | 17 | 121 | none | open | Read, not applied |
| BBDTYPE | 18 | 48 | identity | open | NotModeled |
| AMOUNT | 19 | 52 | identity | open | NotModeled |
| SIZE | 20 | 56 | identity | open | NotModeled |
| MODULATION | 21 | 60 | identity | open | UnknownLawOmitted |
| DENSE | 22 | 104 | bool | open | NotModeled |
| SHUFFLE | 23 | 64 | identity | open | NotModeled |
| FEEL | 24 | 68 | identity | open | NotModeled |
| ACCENT | 25 | 72 | identity | open | NotModeled |
| PAN | 26 | 76 | identity | open | UnknownLawOmitted |
| WIDTH | 27 | 80 | identity | open | UnknownLawOmitted |
| LR_OFFSET | 28 | 84 | identity | open | UnknownLawOmitted |
| DUCKING_SENSE | 29 | 88 | identity | open | NotModeled |
| DUCKING_RELEASE | 30 | 92 | identity | open | NotModeled |
| DUCKING_AMOUNT | 31 | 96 | identity | open | NotModeled |
| PINGPONG_FLIP | 32 | 103 | bool | open | UnknownLawOmitted |

Field 17 slot `121` is the flag at struct +0x79 (`0x79 = 121`). Field 16 slot is +0x98.

## Time law (ported)

Setter case 8 (`0x141ac5980`, mode-3 branch): `ms = clamp(k · v · 4000, 0, 8000)`, with
`k = 0.5` when flag +0x14385 is clear and `1.0` when set. Code: `replika_time_ms` in
`effects/timemod.rs`. `replika_config` maps stored v (0..1) to a single tap at that time, with a
2000 ms line. For v in 0..1 this equals 2000 · v, which matches the TSV law. The default
mode is not set by the reset table, and mode 3 needs index 9 = 3. So the 2000 · v law is
confirmed in the mode-3 branch only. The other branch (`0x141a9d4a0` and the calls after it) is
not read. Applicability to the 10 corpus instruments is therefore not established.

## Constants (checked this pass)

Each row: f32 hex in constants.tsv (u32 column), then the decimal, checked against the u32.

| Address | Value | Used by | Source column |
|---|---|---|---|
| `14470bad0` | 0.5 (0x3f000000) | time k, index 9 scale, section halves | f32 |
| `14470bad4` | 1.0 (0x3f800000) | reset default, flags | f32 |
| `144f39e1c` | 4000 (0x457a0000) | time scale | f32 |
| `14756bdf8` | 8000 (f64 0x40af400000000000) | time clamp | f64 |
| `1447149e4` | 4 (0x40800000) | index 9 mode scale | f32 |
| `144714a00` | 10 (0x41200000) | saturator drive base (powf) | f32 |
| `144f38dbc` | -0.5 (0xbf000000) | saturator output exponent | f32 |
| `144714994` | 2 (0x40000000) | index 4, crossfade | f32 |
| `144714998` | 2.0 (f64 0x4000000000000000) | index 30 pow base | f64 |
| `144714988` | 1.0 (f64 0x3ff0000000000000) | envelope step | f64 |
| `144714b90` | 0x7fffffff | abs mask (f32) | u32 |
| `144f38814` | 0.762405 (0x3f432cf9) | reset default, index 0/1 | f32 |
| `144f38764` | 0.3 (0x3e99999a) | reset default, index 15 | f32 |
| `144fc4b50` | -60 (0xc2700000) | table index offset | f32 |
| `144fc44fc` | 9.96578 (0x411f73da) | table index scale | f32 |
| `1447148a4` | 0.001 (0x3a83126f) | index 17 floor (lead) | f32 |
| `1447148b8` | 0.02 (0x3ca3d70a) | 20 ms ramp near `0x141ac5140` (lead, not read) | f32 |
| `1447148c0` | 0.05 (0x3d4ccccd) | lead | f32 |
| `1447148e0` | 0.2 (0x3e4ccccd) | lead | f32 |
| `144714914` | 0.9 (0x3f666666) | lead | f32 |
| `144fc4a18` | 250 (0x437a0000) | lead | f32 |
| `1479ab340` | 1.71875 (0x3fdc0000) | lead | f32 |
| `1479ab2e0` | 1.4375 (0x3fb80000) | lead | f32 |
| `1479ab61c` | 4851 (0x45979800) | lead | f32 |
| `147698a7c` | 6000 (0x45bb8000) | lead | f32 |
| `1479ab620` | 5600 (0x45af0000) | lead | f32 |

The "lead" rows came from another session and are verified only as data (value vs u32 hex).
Their uses are not read in this pass.

## Core loop (`0x141aa9480`), partly read

Per sample, as far as the text shows:

- Two double-precision rotators (state +0x970/+0x978, coefficients +0x980/+0x988) and two
  bounded ramp generators (+0x940..+0x95c). LFO-like; their use is not traced.
- Two envelope followers on |x| (state +0x142c8 left, +0x142f8 right), attack and release
  coefficients from +0x142e0/+0x142e8 and +0x14310/+0x14318. These are the ducking candidate.
  Update path `0x141abd290` not read.
- Saturation: if the amount +0x142ac is non-zero, `sat = 0x141aa44a0(x)` and the output is
  `g · x · (1 - a) + a · sat(x)`, with `a = +0x142ac`. In the saturator, drive = `powf(10, a)`
  (`+0x1409c`), and an output term = `powf(10, -0.5 · a)` (`+0x140a0`). These two follow from
  the code, the constants are checked. The polynomial coefficients (`+0x14..+0x20`, `+0x38`,
  `+0x3c`, `+0x44`, `+0x50`) are set elsewhere and not read.
- Stereo ring writes at `param+0x50`, read offsets from `local_120`/`local_128`. The ring
  length and the read-offset-to-frames law are in `0x141ee2160` and are not read.
- A T4 lookup table (`DAT_14a6b2600`, 2400 entries) built at startup by `FUN_1404d14d0`
  (pow loop) is indexed as `v · 9.96578 - 60`. Not read.

## Gap table

| Part | Ported | Missing | Confidence |
|---|---|---|---|
| Time (free, mode 3) | `replika_time_ms`, 2000 · v tap through `replika_config` | Mode 1/2/0 branch, sync (TIME_UNIT), feedback in the line | high (formula), low (default mode) |
| Feedback, low cut, high cut | none | Line filter and feedback path (`0x141aa9480` tail, `0x141ee2160`) | - |
| Saturation | none | Saturator update path, coefficients at `+0x14..+0x20` | low |
| BBD type tables | none | Tables for BBDTYPE | - |
| Ping-pong, stereo, width, pan, LR offset | none (reported) | Ring recurrence, pan law | - |
| Tape age, flutter, modulation, noise | none (reported) | LFO and noise (`0x141aa9480` rotators, `0x141abd290`) | - |
| Ducking | none (reported) | Envelope update path `0x141abd290` | - |
| Index binding | 1 of 53 | Name-to-index table | - |

## Tests

- `replika_time_law_is_documented_and_the_rest_is_reported` (`effects/timemod.rs`): time 0.25
  gives 500 ms through `replika_time_ms`, and the other 28 controls each carry a reason.
  No native vector exists for Replika; the test checks the decompile-derived formula only.

## Open items

1. Read the update path `0x141abd290`, `0x141acc600`, `0x141acc980` and `0x141a9d4a0` (mode
   selection, time branch, envelope and saturator coefficients).
2. Read `0x141ee2160` (ring length and read offsets) and `0x140d947a0` (state update).
3. Find the name-to-index binding: the ENGINE_PAR table, or the generic setter path
   `0x140754060` that writes the per-index stream pointers at param `+0x145f0`.
4. Confirm the index 9 mode values and whether TYPE 4 folds into mode 3.
5. The `0x141ac5140` (size 392) and `0x141ac54b0` (size 540) neighbourhood uses the 20 ms
   ramp constant (`1447148b8`). Not read.
