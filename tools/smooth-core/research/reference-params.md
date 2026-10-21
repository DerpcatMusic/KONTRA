# Reference parameter setter: index map

The setter at `0x894370` (entered through the setParam thunk `0x88f4e0`, wrapper slot
+0x08) takes an index and a value. It has 143 cases (0..=142). Each case stores the value
into the DSP object, sets dirty bytes, or both. This file maps every index to its object
offset, its stored form, the SMOOTH parameter it most likely feeds, and how sure we are.
Translation: `tools/smooth-core/src/reference/params.rs`. Differential fixture:
`oracle/params_setter_fixture.tsv`, written by `re-tools/emulate/setter_oracle.py`.

## What is verified and what is not

- **Verified (differential, 5,352 calls in 1,200 sequences).** Every byte the setter
  writes matches the Rust translation: the stored value, every dirty byte, and every
  early return. Float results from `powf`, `logf`, `expf` and `asinh` agree within 1e-6
  relative. The fixture uses the identity host map and a zeroed object before each
  sequence. The test is `reference::params::tests::setter_matches_emulated_reference`.
- **Not verified.** The SMOOTH column is a candidate reading. Only the rows marked
  `high` rest on a fact in the binary or in the SMOOTH code: the transform and range
  match exactly. For `medium` and `low`, the arm writes the right kind of value, but the
  correspondence to a SMOOTH parameter is a guess.
- **Host-owned, not in the image.** Each call passes a descriptor (`rdx`) carrying the
  host's range (`min`, `max`, f64 at +0x28 and +0x30) and a mapping function pointer
  (+0x50). The setter clamps the normalized value to [0, 1], calls the map, then clamps
  to [min, max]. The map and the ranges are host data, so the image does not contain
  them. The table's "range" column is SMOOTH's range, which is the assumption to check.

## Calling convention (from the disassembly, not the decompile)

| Item | Disassembly | Decompile said |
|---|---|---|
| thunk | `mov rcx,[rcx]; cvtsd2ss xmm2,xmm2; jmp 0x894370` | (not in corpus) |
| `min`, `max` | `movsd` then `cvtpd2ps`: **f64** at +0x28, +0x30 | f32 |
| map | `mov rax,[rdx+0x50]; call rax`: function pointer, `f64 f(desc, f64 x)` | `(**(arg2+0x50))(...)`, a vtable |
| index | `mov eax, dword [rsi]`: u32 at +0 | u64 |
| x (clamp) | `x >= 0 ? x : 0`, then `x <= 1 || NaN ? that : 1`. **NaN gives 0** | NaN gives 1 |
| y (clamp) | `y >= lo ? y : lo`, then `hi >= y ? that : hi`. NaN gives `hi` | (same shape) |
| u | `roundf(y)`, then `cvttss2si r9d`: int32, out of range gives `0x80000000` | uint |
| x >= 0.5 | `comiss x, [0.5]; setae` | `bVar4` |
| solo | `lea eax,[r9-1]; cmp eax,ecx; cmovg` with **signed** compare | unsigned |
| return | `rax` is leftover dispatch state, not a value the setter defines | `uVar7` |

## Dirty groups

Each block writes one slot across four byte arrays: `0x3240`, `0x5e88`, `0x8b18`, `0xb760`.
The slot is A=0 (index 2), B=1 (4), C=2 (5, 8, 9), D=3 (6, 7, 10..=13), F=4 (band fields),
E=5 (1, 14, 15). Index 4 also sets `0x4a`. An index that compares its stored value and
finds no change returns early (1, 14, 15) or skips its dirty byte (2 and 4).

## Map

Columns: Index; Object offset; Stored (type); Setter transform (0x894370); Dirty block;
Rust field; SMOOTH parameter (candidate); SMOOTH unit, range, default and law; Confidence;
Note. The "SMOOTH" column is the candidate. `none` means no SMOOTH parameter maps to it.

| Index | Object offset | Stored (type) | Setter transform (0x894370) | Dirty | Rust field | SMOOTH parameter | SMOOTH unit; range; default; law | Confidence | Note |
|---|---|---|---|---|---|---|---|---|---|
| 0 | 0x54 | f32 | x (clamped norm) | none | `r000_norm_54` | `depth` | 0..10; 0..10; default 5; lin | low | normalized depth stored; 0..10 SMOOTH maps to x |
| 1 | 0xab | bool | x >= 0.5 | E | `r001_flag_ab` | `low_latency` | flag; 0..1; default 0; disc | high | zero-latency byte (`reference_plan.rs`); no change = early return |
| 2 | 0x104 | u32 | roundf(y), as i32 then u32 | A | `r002_int_104` | `quality` | enum; 0..2; default 0; disc (normal, high, ultra) | medium | enum 0..2; A block on change |
| 3 | 0x50 | u32 | roundf(y) | none | `r003_int_50` | `mode` | flag; 0..1; default 0; disc (0 soft, 1 hard) | low | int 0/1; no dirty |
| 4 | 0xac | bool | x >= 0.5 | B (+0x4a) | `r004_flag_ac` | `linear_phase` | flag; 0..1; default 0; disc | medium | plan-level toggle; B block and 0x4a; no SMOOTH Settings binding |
| 5 | 0x58 | f32 | x (clamped norm) | C | `r005_norm_58` | `focus` | norm; 0..1; default 0.5; lin | medium | norm, 0..1 |
| 6 | 0x5c | f32 | x (clamped norm) | D | `r006_norm_5c` | none (reference-only) | host-supplied range | none | no SMOOTH counterpart |
| 7 | 0x60 | f32 | x (clamped norm) | D | `r007_norm_60` | none (reference-only) | host-supplied range | none | no SMOOTH counterpart |
| 8 | 0x64 | f32 | y (plain, clamped) | C | `r008_plain_64` | `link` | %; 0..100; default 100; lin | low | 0..100 %, stored plain |
| 9 | 0x68 | f32 | y (plain, clamped) | C | `r009_plain_68` | none (reference-only) | host-supplied range | none | no SMOOTH counterpart |
| 10 | 0x6c | f32 | y (plain, clamped) | D | `r010_plain_6c` | `detail` | 0..10; 0..10; default 5; lin | low | stored plain; SMOOTH detail is v/10 in Settings |
| 11 | 0x70 | f32 | y (plain, clamped) | D | `r011_plain_70` | `attack` | 0..10; 0..10; default 5; lin | low | stored plain; SMOOTH attack is v/10 in Settings |
| 12 | 0x74 | f32 | y (plain, clamped) | D | `r012_plain_74` | `release` | 0..10; 0..10; default 3; lin | low | stored plain; SMOOTH release is v/10 in Settings |
| 13 | 0x78 | f32 | y (plain, clamped) | D | `r013_plain_78` | none (reference-only) | host-supplied range | none | no SMOOTH counterpart |
| 14 | 0xa5 | bool | x >= 0.5 | E | `r014_flag_a5` | `sidechain` | flag; 0..1; default 0; disc | high | sidechain (per the request); no change = early return |
| 15 | 0xa7 | bool | x >= 0.5 | E | `r015_flag_a7` | `sidechain_listen` | flag; 0..1; default 0; disc | medium | no change = early return |
| 16 | 0xa6 | bool | x >= 0.5 | none | `r016_flag_a6` | `delta` | flag; 0..1; default 0; disc | low | no dirty |
| 17 | 0x7c | f32 | x (clamped norm) | none | `r017_norm_7c` | `detail_tilt_low` | x; 0.1..10; default 1; sym_skewed | medium | SMOOTH has no Settings field for this; no SMOOTH engine binding |
| 18 | 0x90 | f32 | x (clamped norm) | none | `r018_norm_90` | `detail_tilt_high` | x; 0.1..10; default 1; sym_skewed | medium | as index 17 |
| 19 | 0x10c | bool | x >= 0.5 | F | `bands[0].used` | `band1_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 20 | 0x10d | bool | x >= 0.5 | F | `bands[0].enabled` | `band1_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 21 | 0x110 | f32 | y | F | `bands[0].frequency` | `band1_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 22 | 0x114 | f32 | 24 x - 12 | F | `bands[0].depth` | `band1_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 23 | 0x118 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[0].half_bandwidth_oct` | `band1_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 24 | 0x11c | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[0].slope` | `band1_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 25 | 0x120 | f32 | y | F | `bands[0].focus` | `band1_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 26 | 0x124 | u32 | roundf(y) | F | `bands[0].shape` | `band1_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 27 | 0x128 | u32 | roundf(y) | F | `bands[0].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 28 | 0x12c | bool | x >= 0.5 | F | `bands[1].used` | `band2_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 29 | 0x12d | bool | x >= 0.5 | F | `bands[1].enabled` | `band2_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 30 | 0x130 | f32 | y | F | `bands[1].frequency` | `band2_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 31 | 0x134 | f32 | 24 x - 12 | F | `bands[1].depth` | `band2_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 32 | 0x138 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[1].half_bandwidth_oct` | `band2_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 33 | 0x13c | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[1].slope` | `band2_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 34 | 0x140 | f32 | y | F | `bands[1].focus` | `band2_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 35 | 0x144 | u32 | roundf(y) | F | `bands[1].shape` | `band2_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 36 | 0x148 | u32 | roundf(y) | F | `bands[1].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 37 | 0x14c | bool | x >= 0.5 | F | `bands[2].used` | `band3_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 38 | 0x14d | bool | x >= 0.5 | F | `bands[2].enabled` | `band3_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 39 | 0x150 | f32 | y | F | `bands[2].frequency` | `band3_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 40 | 0x154 | f32 | 24 x - 12 | F | `bands[2].depth` | `band3_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 41 | 0x158 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[2].half_bandwidth_oct` | `band3_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 42 | 0x15c | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[2].slope` | `band3_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 43 | 0x160 | f32 | y | F | `bands[2].focus` | `band3_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 44 | 0x164 | u32 | roundf(y) | F | `bands[2].shape` | `band3_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 45 | 0x168 | u32 | roundf(y) | F | `bands[2].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 46 | 0x16c | bool | x >= 0.5 | F | `bands[3].used` | `band4_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 47 | 0x16d | bool | x >= 0.5 | F | `bands[3].enabled` | `band4_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 48 | 0x170 | f32 | y | F | `bands[3].frequency` | `band4_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 49 | 0x174 | f32 | 24 x - 12 | F | `bands[3].depth` | `band4_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 50 | 0x178 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[3].half_bandwidth_oct` | `band4_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 51 | 0x17c | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[3].slope` | `band4_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 52 | 0x180 | f32 | y | F | `bands[3].focus` | `band4_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 53 | 0x184 | u32 | roundf(y) | F | `bands[3].shape` | `band4_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 54 | 0x188 | u32 | roundf(y) | F | `bands[3].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 55 | 0x18c | bool | x >= 0.5 | F | `bands[4].used` | `band5_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 56 | 0x18d | bool | x >= 0.5 | F | `bands[4].enabled` | `band5_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 57 | 0x190 | f32 | y | F | `bands[4].frequency` | `band5_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 58 | 0x194 | f32 | 24 x - 12 | F | `bands[4].depth` | `band5_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 59 | 0x198 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[4].half_bandwidth_oct` | `band5_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 60 | 0x19c | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[4].slope` | `band5_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 61 | 0x1a0 | f32 | y | F | `bands[4].focus` | `band5_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 62 | 0x1a4 | u32 | roundf(y) | F | `bands[4].shape` | `band5_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 63 | 0x1a8 | u32 | roundf(y) | F | `bands[4].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 64 | 0x1ac | bool | x >= 0.5 | F | `bands[5].used` | `band6_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 65 | 0x1ad | bool | x >= 0.5 | F | `bands[5].enabled` | `band6_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 66 | 0x1b0 | f32 | y | F | `bands[5].frequency` | `band6_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 67 | 0x1b4 | f32 | 24 x - 12 | F | `bands[5].depth` | `band6_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 68 | 0x1b8 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[5].half_bandwidth_oct` | `band6_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 69 | 0x1bc | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[5].slope` | `band6_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 70 | 0x1c0 | f32 | y | F | `bands[5].focus` | `band6_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 71 | 0x1c4 | u32 | roundf(y) | F | `bands[5].shape` | `band6_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 72 | 0x1c8 | u32 | roundf(y) | F | `bands[5].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 73 | 0x1cc | bool | x >= 0.5 | F | `bands[6].used` | `band7_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 74 | 0x1cd | bool | x >= 0.5 | F | `bands[6].enabled` | `band7_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 75 | 0x1d0 | f32 | y | F | `bands[6].frequency` | `band7_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 76 | 0x1d4 | f32 | 24 x - 12 | F | `bands[6].depth` | `band7_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 77 | 0x1d8 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[6].half_bandwidth_oct` | `band7_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 78 | 0x1dc | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[6].slope` | `band7_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 79 | 0x1e0 | f32 | y | F | `bands[6].focus` | `band7_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 80 | 0x1e4 | u32 | roundf(y) | F | `bands[6].shape` | `band7_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 81 | 0x1e8 | u32 | roundf(y) | F | `bands[6].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 82 | 0x1ec | bool | x >= 0.5 | F | `bands[7].used` | `band8_active` | flag; 0..1; default 1 (bands 1-3), 0 (4-8); disc | medium | stored as x >= 0.5 |
| 83 | 0x1ed | bool | x >= 0.5 | F | `bands[7].enabled` | `band8_bypass` | flag; 0..1; default 0; disc | low | stored enabled; SMOOTH has bypass, so the inverse sense is a guess |
| 84 | 0x1f0 | f32 | y | F | `bands[7].frequency` | `band8_frequency` | Hz; 20..20000; default 100/4000/16000/1000/1000/1000/1000/1000; offset_log | medium | plain value y, clamped |
| 85 | 0x1f4 | f32 | 24 x - 12 | F | `bands[7].depth` | `band8_depth` | dB; -10..10; default 0 (2.5 band 2); lin | high | stored 24x-12 = 1.2 * dB (SMOOTH engine scale) |
| 86 | 0x1f8 | f32 bits | log2(e) asinh(1/(2y)) | F | `bands[7].half_bandwidth_oct` | `band8_q` | lin; 0.1..10; default 0.707; pivoted_log | high | stores half bandwidth in octaves, asinh(1/(2Q)) * log2(e): a Q-to-bandwidth law |
| 87 | 0x1fc | f32 bits | expf(ln6 + x (ln128 - ln6)) | F | `bands[7].slope` | `band8_slope` | dB/oct; 6..128; default 27.71; log | high | stores expf(ln 6 + x (ln 128 - ln 6)): the log(6,128) law |
| 88 | 0x200 | f32 | y | F | `bands[7].focus` | `band8_balance` | lin; 0..1; default 0.5; lin | medium | plain y; SMOOTH focus field |
| 89 | 0x204 | u32 | roundf(y) | F | `bands[7].shape` | `band8_shape` | enum; 0..7; default 0 / 2 / 7 / 2 ...; disc | high | roundf(y) as the shape index |
| 90 | 0x208 | u32 | roundf(y) | F | `bands[7].selector` | none (reference-only; SMOOTH selector always 0) | host-supplied range | none | `BandRecord::selector` |
| 91 | 0x100 | i32 | min(roundf(y) - 1, 7), signed; host 0 gives -1 | none | `r091_solo_100` | `band_solo` | disc; 0..8; default 0; 0 off, 1-8 band N | high | stores N-1; host 0 stores -1 (0xffffffff) = no solo |
| 92 | 0xa0 | f32 | y, or 60 when 0.999 <= x | none | `r092_plain_a0` | `max_cut` | dB; 0..24; lin; default 24 | high | the 60 at top is SMOOTH's `60 if v >= 23.97` |
| 93 | 0x94 | f32 | powf(10, y * 0.05) | none | `r093_gain_94` | `wet_trim` | dB; -18..18; lin; default 0 | high | stores the linear gain 10^(dB/20) |
| 94 | 0x98 | f32 | y * 0.01 | none | `r094_scaled_98` | `mix` | %; 0..100; lin; default 100 | high | stores the fraction v/100 |
| 95 | 0x9c | f32 | powf(10, y * 0.05) | none | `r095_gain_9c` | `output` | dB; -18..18; lin; default 0 | high | stores the linear gain 10^(dB/20) |
| 96 | 0xa8 | bool | x >= 0.5 | none | `r096_flag_a8` | `bypass` | flag; 0..1; default 0; disc | medium | no dirty |
| 97 | 0xa9 | bool | x >= 0.5 | none | `r097_flag_a9` | `stereo_mode` | flag; 0..1; default 0; disc (0 l/r, 1 m/s) | low | no dirty |
| 98 | 0x20c | f32 | x (clamped norm) | none | `ext[0].norm` | none (reference-only) | host-supplied range | none | multichannel/weight group 0 (candidate; SMOOTH `weight_groups` unfilled) |
| 99 | 0x210 | bool | x >= 0.5 | none | `ext[0].flags[0]` | none (reference-only) | host-supplied range | none | group 0 flag 0 |
| 100 | 0x211 | bool | x >= 0.5 | none | `ext[0].flags[1]` | none (reference-only) | host-supplied range | none | group 0 flag 1 |
| 101 | 0x212 | bool | x >= 0.5 | none | `ext[0].flags[2]` | none (reference-only) | host-supplied range | none | group 0 flag 2 |
| 102 | 0x213 | bool | x >= 0.5 | none | `ext[0].flags[3]` | none (reference-only) | host-supplied range | none | group 0 flag 3 |
| 103 | 0x214 | f32 | x (clamped norm) | none | `ext[1].norm` | none (reference-only) | host-supplied range | none | multichannel/weight group 1 (candidate; SMOOTH `weight_groups` unfilled) |
| 104 | 0x218 | bool | x >= 0.5 | none | `ext[1].flags[0]` | none (reference-only) | host-supplied range | none | group 1 flag 0 |
| 105 | 0x219 | bool | x >= 0.5 | none | `ext[1].flags[1]` | none (reference-only) | host-supplied range | none | group 1 flag 1 |
| 106 | 0x21a | bool | x >= 0.5 | none | `ext[1].flags[2]` | none (reference-only) | host-supplied range | none | group 1 flag 2 |
| 107 | 0x21b | bool | x >= 0.5 | none | `ext[1].flags[3]` | none (reference-only) | host-supplied range | none | group 1 flag 3 |
| 108 | 0x21c | f32 | x (clamped norm) | none | `ext[2].norm` | none (reference-only) | host-supplied range | none | multichannel/weight group 2 (candidate; SMOOTH `weight_groups` unfilled) |
| 109 | 0x220 | bool | x >= 0.5 | none | `ext[2].flags[0]` | none (reference-only) | host-supplied range | none | group 2 flag 0 |
| 110 | 0x221 | bool | x >= 0.5 | none | `ext[2].flags[1]` | none (reference-only) | host-supplied range | none | group 2 flag 1 |
| 111 | 0x222 | bool | x >= 0.5 | none | `ext[2].flags[2]` | none (reference-only) | host-supplied range | none | group 2 flag 2 |
| 112 | 0x223 | bool | x >= 0.5 | none | `ext[2].flags[3]` | none (reference-only) | host-supplied range | none | group 2 flag 3 |
| 113 | 0x224 | f32 | x (clamped norm) | none | `ext[3].norm` | none (reference-only) | host-supplied range | none | multichannel/weight group 3 (candidate; SMOOTH `weight_groups` unfilled) |
| 114 | 0x228 | bool | x >= 0.5 | none | `ext[3].flags[0]` | none (reference-only) | host-supplied range | none | group 3 flag 0 |
| 115 | 0x229 | bool | x >= 0.5 | none | `ext[3].flags[1]` | none (reference-only) | host-supplied range | none | group 3 flag 1 |
| 116 | 0x22a | bool | x >= 0.5 | none | `ext[3].flags[2]` | none (reference-only) | host-supplied range | none | group 3 flag 2 |
| 117 | 0x22b | bool | x >= 0.5 | none | `ext[3].flags[3]` | none (reference-only) | host-supplied range | none | group 3 flag 3 |
| 118 | 0x22c | f32 | x (clamped norm) | none | `ext[4].norm` | none (reference-only) | host-supplied range | none | multichannel/weight group 4 (candidate; SMOOTH `weight_groups` unfilled) |
| 119 | 0x230 | bool | x >= 0.5 | none | `ext[4].flags[0]` | none (reference-only) | host-supplied range | none | group 4 flag 0 |
| 120 | 0x231 | bool | x >= 0.5 | none | `ext[4].flags[1]` | none (reference-only) | host-supplied range | none | group 4 flag 1 |
| 121 | 0x232 | bool | x >= 0.5 | none | `ext[4].flags[2]` | none (reference-only) | host-supplied range | none | group 4 flag 2 |
| 122 | 0x233 | bool | x >= 0.5 | none | `ext[4].flags[3]` | none (reference-only) | host-supplied range | none | group 4 flag 3 |
| 123 | 0x80 | f32 | x (clamped norm) | none | `r123_norm_80` | `attack_tilt_low` | x; 0.1..10; default 1; sym_skewed | medium | norm; SMOOTH Settings holds the plain value (attack_low, etc.) |
| 124 | 0x84 | f32 | x (clamped norm) | none | `r124_norm_84` | `attack_tilt_high` | x; 0.1..10; default 1; sym_skewed | medium | norm; SMOOTH Settings holds the plain value (attack_low, etc.) |
| 125 | 0x88 | f32 | x (clamped norm) | none | `r125_norm_88` | `release_tilt_low` | x; 0.1..10; default 1; sym_skewed | medium | norm; SMOOTH Settings holds the plain value (attack_low, etc.) |
| 126 | 0x8c | f32 | x (clamped norm) | none | `r126_norm_8c` | `release_tilt_high` | x; 0.1..10; default 1; sym_skewed | medium | norm; SMOOTH Settings holds the plain value (attack_low, etc.) |
| 127 | 0xf0 | bool | x >= 0.5 | none | `flags_f0[0]` | none (reference-only) | host-supplied range | none | byte 0xf0 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 128 | 0xf1 | bool | x >= 0.5 | none | `flags_f0[1]` | none (reference-only) | host-supplied range | none | byte 0xf1 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 129 | 0xf2 | bool | x >= 0.5 | none | `flags_f0[2]` | none (reference-only) | host-supplied range | none | byte 0xf2 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 130 | 0xf3 | bool | x >= 0.5 | none | `flags_f0[3]` | none (reference-only) | host-supplied range | none | byte 0xf3 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 131 | 0xf4 | bool | x >= 0.5 | none | `flags_f0[4]` | none (reference-only) | host-supplied range | none | byte 0xf4 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 132 | 0xf5 | bool | x >= 0.5 | none | `flags_f0[5]` | none (reference-only) | host-supplied range | none | byte 0xf5 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 133 | 0xf6 | bool | x >= 0.5 | none | `flags_f0[6]` | none (reference-only) | host-supplied range | none | byte 0xf6 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 134 | 0xf7 | bool | x >= 0.5 | none | `flags_f0[7]` | none (reference-only) | host-supplied range | none | byte 0xf7 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 135 | 0xf8 | bool | x >= 0.5 | none | `flags_f0[8]` | none (reference-only) | host-supplied range | none | byte 0xf8 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 136 | 0xf9 | bool | x >= 0.5 | none | `flags_f0[9]` | none (reference-only) | host-supplied range | none | byte 0xf9 (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 137 | 0xfa | bool | x >= 0.5 | none | `flags_f0[10]` | none (reference-only) | host-supplied range | none | byte 0xfa (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 138 | 0xfb | bool | x >= 0.5 | none | `flags_f0[11]` | none (reference-only) | host-supplied range | none | byte 0xfb (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 139 | 0xfc | bool | x >= 0.5 | none | `flags_f0[12]` | none (reference-only) | host-supplied range | none | byte 0xfc (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 140 | 0xfd | bool | x >= 0.5 | none | `flags_f0[13]` | none (reference-only) | host-supplied range | none | byte 0xfd (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 141 | 0xfe | bool | x >= 0.5 | none | `flags_f0[14]` | none (reference-only) | host-supplied range | none | byte 0xfe (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |
| 142 | 0xff | bool | x >= 0.5 | none | `flags_f0[15]` | none (reference-only) | host-supplied range | none | byte 0xff (`0x71 + index`), 16 slots; candidate per-channel or per-band flags |

## Reference parameters with no SMOOTH counterpart

These indices write state the reference uses. SMOOTH has no parameter for them, so
the host would have to send them from some other plugin or preset.

- 6, 7, 9, 13: normalized or plain values in the 0x58..0x78 block (dirty C/D). No SMOOTH
  counterpart. Candidates such as "stereo" or "focus pair" are not supported by evidence.
- 98..122 (25 indices): five groups of one normalized value and four flags, at `0x20c`..
  `0x233`. The group count (5) matches neither SMOOTH's 8 bands nor its 2 weight groups.
  Candidate: the multichannel or weight-group descriptors (`ChannelGroup` in `Settings`,
  which SMOOTH never fills), but that is not evidenced.
- 127..142 (16 indices): flag bytes at `0xf0..=0xff`. Candidate: per-channel or per-band
  flags for a multichannel layout.
- The band `selector` field (indices 27, 36, 45, 54, 63, 72, 81, 90). The reference
  stores `roundf(y)`. SMOOTH's `BandRecord::selector` exists but SMOOTH always writes 0
  (`lib.rs:841`), so there is no parameter to drive it.

## SMOOTH parameters the reference has no setter for

None, once the candidates above are accepted. All 90 SMOOTH plugin parameters have a
candidate index. Low-confidence candidates: `depth`, `mode`, `link`, `detail`, `attack`,
`release`, `delta`, `stereo_mode`, and the eight band `bypass` fields (the inverse sense
of `enabled` is a guess). Medium-confidence ones need the host's range to confirm.
The SMOOTH `Settings` fields that no setter index feeds are `active_channels`,
`mix_smoothing_ms`, `adaptive_baseline`, `weight_groups`, `weight_group_count`,
`weight_stereo_mode`, `spatial_curve_scalar` and `zero_m_after_history`.

## Gaps and open items

- **Host map and ranges.** The reference's per-index mapping (linear, log, OL, PL, sym_skewed)
  and its [min, max] are host data. The table gives SMOOTH's ranges as the candidate.
  `from_smooth` uses the stored formulas, not the host map (see below).
- **Setter return value.** Not reproduced. The fixture records `rax`, but it is a leftover
  dispatch byte in most cases. Nothing in this translation reads it.
- **Detail tilts** (17, 18) and **attack/release tilts** (123..=126) take normalized values.
  SMOOTH stores plain values after its sym_skewed map, and the inverse map is not in the
  image. `from_smooth` leaves these at zero.
- **Quality, low latency, linear phase** (2, 1, 4) are plan selectors. SMOOTH keeps them in
  `EngineConfig`, not `Settings`. `from_smooth` leaves them at zero.
- **Solo off.** Host 0 stores -1, which the translation keeps as `0xffff_ffff`. The
  `from_smooth` path uses the same value for `solo_band: None`.
- **Dirty consumers.** The dirty bytes are written and tested here. Nothing in this
  translation reads them, and the reader (presumably the process path) is not identified yet.
- **`from_smooth`.** It writes the stored form directly from SMOOTH's plain values, using
  the setter's formulas: `10^(dB/20)` for the gains, `asinh` for the band term, `expf` for
  slope, and `24x-12 = 1.2*dB` for band depth. It sets every dirty byte. It does not go
  through the setter, because SMOOTH's log, OL, PL and sym_skewed inverses are not in
  the image.
