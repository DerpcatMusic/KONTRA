# Kontakt 8 Replika Delay (module 0x5A, saved v0x12): decompile status

Status: partial. Reader, wrapper and setter structure are confirmed from decompiled C
(`source/kontakt8/*.c`, READY). Only time is mapped; every other control is reported with
a reason. No DSP was ported: the per-sample core relies on constants that are not in the
decompiled text, and the wrapper-index to control-name binding is not tied. Nothing here
is measured against Kontakt.

Address convention: `0x1xxxxxxxx` names are Kontakt 8 functions. File:line refs point at
`source/kontakt8/<bucket>.c`.

## Confirmed structure

| Part | Address | What is confirmed | Confidence |
|---|---|---|---|
| Saved reader v0x12 | `0x140d02250` (`0140d00000.c:1463`) | Layout below; matches `read_replika` and the TSV slot column | high |
| Enum reader | `0x140a847f0` | Type stored as 0..4; any other value goes to an error routine (`0x140a874c0`) | high (mapping), medium (error path) |
| Time-unit sub-object | `0x140ab5da0` (`0140ab0000.c:3884`) | 3 floats (slots 0x88, 0x94, 0x98) and a flag; sync flag = value > 1e-9 | high |
| Outer wrapper | `0x1408f98d0` (`01408f0000.c:6067`) | 53 controls, 32-frame update quantum, enable byte + stream pointer per index, clamp [0,1], then core update and process | high |
| Core setter | `0x141ac5980` (`0141ac0000.c:3348`) | Switch over index 0..0x34 (53 indices); every index first stores the raw value at core+0x840+4i | high (switch), medium (per-index meaning) |
| TYPE engine parameter | `set_engine_par` id `0x3d5` (`0140780000.c:5921`, inside `0x140789c30`) | Integer type, range check 0..4, message "Invalid Replika delay type!" | high |
| Core per-sample loop | `0x141aa9480` (`0141aa0000.c:5277`, 3.4 KB) | Partly read: see "Core loop" | low |
| Saturator helper | `0x141aa44a0` (`0141aa0000.c:2492`) | tanh drive plus a polynomial-shaped state term; constants in struct fields | low |

### Saved stream layout (v0x12), confirmed against `0x140d02250`

Order read by the reader, with the struct offset each value is stored at:

1. enum type (saved field 0, struct +0xa8)
2. 10 floats: time, feedback, lowcut, highcut, saturation, tape age, flutter, quality, depth, rate (fields 1..10, offsets +0x08..+0x2c)
3. 3 flags: stereo (+0x64), noise (+0x65), pingpong (+0x66)
4. time-unit sub-object `0x140ab5da0` at +0x70: floats at +0x88 (field 14), +0x94 (field 15), +0x98 (field 16), flag +0x79
5. 4 floats: bbd type, amount, size, modulation (fields 18..21, +0x30..+0x3c)
6. flag: dense (+0x68)
7. 9 floats: shuffle, feel, accent, pan, width, lr offset, ducking sense, release, amount (fields 23..31, +0x40..+0x60)
8. flag: pingpong flip (+0x67)

The Rust reader in `effects/timemod.rs` (`read_replika`) follows the same order. The
TSV "slot" column equals these offsets (checked for TIME 8, FEEDBACK 12, DENSE 104,
PINGPONG_FLIP 103, TIME_UNIT 136/148).

## Index map (wrapper index to core setter)

The wrapper has 53 controls (index 0..52). The TSV has 30 distinct RDL names (32 distinct
name/field/slot rows, since TIME_UNIT has three). So the 53 controls are not the 30 saved
names; about 23 indices are derived or internal, or have no TSV row. Index meaning below is from the
setter switch only. `v` is the normalised input (0..1).

| idx | Setter behaviour (`0x141ac5980`) | Kind |
|---|---|---|
| 0, 1 | `0x141a90910` conversion into control block +0x0 / +0x4 | gain A / B |
| 2, 3 | linear into +0x10 / +0x14 (live path `0x141acc980` / `0x141acc930`) | linear |
| 4 | crossfade pair into +0x8 / +0xc: v > 0.5 gives (2(1-v), 1), else (1, 2v) | mix law |
| 5 | linear into +0x18 (`0x141ac9420`) | linear |
| 6, 7 | flag (v >= 0.5) at core +0x914 / +0x915, rebuilds `0x141abd210`, `0x141acc600` | bool |
| 8 | sub-mode 3: time from v x K1 clamped to K2, into +0x2c; else `0x141a9d4a0` / `9d7c0` / `9d660` | time (K1, K2 not in text) |
| 9 | integer mode, rounded, clamped to 0..3, into +0x93c; drives `0x141acbfb0` and bypasses sub-objects +0xac0, +0x6380, +0xbc40 | mode |
| 10, 11 | raw into +0x30 / +0x34; enable flags +0x14381 / +0x14382 | section enable |
| 12 | three section gains from `0x141abba60` (filter-like law); enable +0x14383 | section split |
| 13, 14 | crossfade pairs into +0x1c/+0x20 and +0x24/+0x28 | mix law |
| 15 | raw into +0x38; enable +0x14384 | section enable |
| 16 | raw into +0x64 | raw |
| 17 | max(1-v, K) into double +0x13e18 | law (K not in text) |
| 18 | v x K into double +0x13e10 | scale (K not in text) |
| 19 | pow law into double +0x13e28 from +0x13e00..+0x13e08 | law (constants not in text) |
| 20..24 | no case, raw store only | raw |
| 25 | double +0x928 = v x K + K' ; `0x141a9dbc0` if +0x916 clear | law (constants) |
| 26 | flag +0x916 (v >= 0.5) selects +0x920 or +0x928 | bool select |
| 27 | no case, raw | raw |
| 28 | flag +0x829, then `0x141aca670` | section update |
| 29 | raw into +0x142b0; live path sets +0x142ac and runs `0x141accd10`. +0x142ac gates the saturator in the loop | saturation amount (candidate) |
| 30 | pow law into +0x3c | law (constants) |
| 31 | raw into the +0x248 chain, enable +0x294 | raw/section |
| 32 | `0x141ac44a0` on the three sections | section |
| 33 | no case, raw | raw |
| 34 | flag +0x82b, `0x141abbf60` | section update |
| 35 | pow law into +0x40 | law (constants) |
| 36 | raw into +0x44, `0x141acc9d0` | raw |
| 37 | `0x141abc0d0` on the sections | section |
| 38 | no case, raw | raw |
| 39 | sqrt(v) into +0x48, `0x141accd80` | law |
| 40, 41 | raw into +0x4c / +0x50 | raw |
| 42 | `0x141acac40` on the sections | section |
| 43 | flag +0x82a, `0x141acb2b0` | section update |
| 44 | pow law into +0x54 | law (constants) |
| 45 | raw into +0x58 | raw |
| 46 | `0x141acb240` on the sections | section |
| 47 | no case, raw | raw |
| 48 | raw through pointer +0x188 | raw |
| 49 | raw into the +0x218..+0x260 chain, `0x141accb60` | raw/section |
| 50 | raw into +0x5c, `0x141accb10` | raw |
| 51 | sub-objects +0x11600 and +0x12a80, index 3, value 0 or 1 (v >= 0.5) | bool |
| 52 | raw into +0x68 | raw |

Legend: "raw" means only the core +0x840 store happens here (plus any listed flag). "Section"
means a call that acts on the three sub-objects +0xac0, +0x6380, +0xbc40.

## Parameter table (saved names)

Field and slot come from the reader and the TSV. "Core idx" is the only place the binding
could be tied, and it is not (no name-to-index evidence in the decompile text).

| Name | Field | Slot | Kind | TSV law | Core idx | Status and reason |
|---|---|---|---|---|---|---|
| TYPE | 0 | 0 | I | table 0..4 | not tied | ENGINE_PAR 0x3d5 range-checked 0..4. NotModeled (no IR type). |
| TIME | 1 | 8 | F | linear, 2000 ms at 1.0 | 8 (candidate) | Mapped: one tap at 2000·v ms, documented TSV law. Oracle did not confirm (k-delays.md). |
| FEEDBACK | 2 | 12 | F | identity | not tied | UnknownLawOmitted (IR feedback exists, law unknown) |
| LOWCUT | 3 | 16 | F | identity | not tied | NotModeled (no IR filter in Line) |
| HIGHCUT | 4 | 20 | F | identity | not tied | NotModeled |
| SATURATION | 5 | 24 | F | identity | 29 (candidate) | NotModeled (saturator `0x141aa44a0` constants absent) |
| TAPEAGE | 6 | 28 | F | identity | not tied | NotModeled |
| FLUTTER | 7 | 32 | F | identity | not tied | NotModeled |
| QUALITY | 8 | 36 | F | identity | not tied | NotModeled |
| DEPTH | 9 | 40 | F | identity | not tied | UnknownLawOmitted (IR tap depth_ms exists) |
| RATE | 10 | 44 | F | identity | not tied | UnknownLawOmitted (IR LFO rate exists) |
| STEREO | 11 | 100 | B | bool | not tied | UnknownLawOmitted (IR input matrix exists) |
| NOISE | 12 | 101 | B | bool | not tied | NotModeled |
| PINGPONG | 13 | 102 | B | bool | not tied | UnknownLawOmitted |
| TIME_UNIT | 14 | 136 | table | sync table | not tied | NotModeled; sync path is not in `replika_config` (unit > 1e-9 returns no config) |
| (field 15) | 15 | 148 | table | TSV row | not tied | Read, not applied |
| (field 16) | 16 | 152 | F | not in TSV | not tied | Read, not applied |
| BBDTYPE | 18 | 48 | F | identity | not tied | NotModeled (no BBD tables extracted) |
| AMOUNT | 19 | 52 | F | identity | not tied | NotModeled |
| SIZE | 20 | 56 | F | identity | not tied | NotModeled |
| MODULATION | 21 | 60 | F | identity | not tied | UnknownLawOmitted |
| DENSE | 22 | 104 | B | bool | not tied | NotModeled |
| SHUFFLE | 23 | 64 | F | identity | not tied | NotModeled |
| FEEL | 24 | 68 | F | identity | not tied | NotModeled |
| ACCENT | 25 | 72 | F | identity | not tied | NotModeled |
| PAN | 26 | 76 | F | identity | not tied | UnknownLawOmitted (IR tap pan exists) |
| WIDTH | 27 | 80 | F | identity | not tied | UnknownLawOmitted |
| LR_OFFSET | 28 | 84 | F | identity | not tied | UnknownLawOmitted (IR tap offset exists) |
| DUCKING_SENSE | 29 | 88 | F | identity | not tied | NotModeled (envelope follower at +0x142c8 seen; law not read) |
| DUCKING_RELEASE | 30 | 92 | F | identity | not tied | NotModeled |
| DUCKING_AMOUNT | 31 | 96 | F | identity | not tied | NotModeled |
| PINGPONG_FLIP | 32 | 103 | B | bool | not tied | UnknownLawOmitted |

Reason classes follow `sampler_ir::Reason`. UnknownLawOmitted means the IR could represent
the control but the source law is unverified, so it is omitted rather than approximated.
NotModeled means there is no IR equivalent yet.

## Core loop (`0x141aa9480`), partly read

Per frame, as far as the text shows:

- Two double-precision oscillators with a rotation recurrence (state +0x970/+0x978, coefficients +0x980/+0x988) and two ramp generators with bounded steps (+0x940..+0x95c). They are LFO-like. Their use is not traced.
- Two one-pole envelope followers on |x| (state +0x142c8 left, +0x142f8 right). Attack and release coefficients come from +0x142e0/+0x142e8 and +0x14310/+0x14318, chosen by comparing to the state. These are the ducking candidate. Their coefficients are computed with `powf` from +0x142ac and +0x142b0 (constant `DAT_144714a00`, value not in text).
- Saturation: when the stored amount +0x142ac is non-zero, the sample is replaced by `0x141aa44a0(x)`, then blended: out = g·x·(1-a) + a·sat(x), with g from the section gain at +0x80+0x68 and a = +0x142ac.
- Writes into a stereo ring at `local_res8` (param+0x50), with read offsets from `local_120`/`local_128`. The read/write recurrence and the ring length are not extracted.
- Section gains multiply the result (four product terms from +0x3c0, +0x300, +0x480, +0x540).

Not established: the ring length, the read offset law (time to frames), the line filter,
the feedback path, the BBD tables, and how the envelope coefficients are set (the
update path at `0x141abd290` is not read).

## Constants missing from the decompile text

The decompile prints `DAT_<addr>` for many literals. Their values are in the binary's data
section, which this unit did not read (rule: no Ghidra, REA or binary open). The laws
below cannot be ported without them:

- Time: `DAT_144f39e1c` (scale), `DAT_14756bdf8` (clamp), `DAT_14470bad0` (0.5 threshold, inferred), `DAT_14470bad4` (1.0, inferred).
- Saturator and envelopes: `DAT_144714a00`, `DAT_144f38dbc`, `DAT_144f38910`, `DAT_1447617a0`.
- Filters and pow laws: `DAT_144714998`, `DAT_144f38cbc`, `DAT_1479ab4d8`, `DAT_1479ab4d0`.
- Index 17, 18, 19: `DAT_1447148a4`, `DAT_144fc4d50`, `DAT_144714988`, `DAT_14769abe8`.

Inline constants (1, 2, 0.5, sqrt) are the only ones read directly.

## Gap table

| Module | Engine covers | Missing | Action |
|---|---|---|---|
| Replika Delay (0x5A) | Time (2000·v ms, one tap, no feedback) through `replika_config`. Reasons for all 30 saved names. | Feedback, filters, saturation, ping-pong, stereo, BBD, ducking, tape, flutter, modulation, noise, shuffle and the sync path. Index binding. Ring recurrence. | Needs: a read-only constant dump of the listed DAT addresses (coordinator decision), the ENGINE_PAR name-to-id table, and the update path `0x141abd290`. |
| Replika core (`0x141aa9480`) | Nothing ported | Whole loop | See "Core loop". |

## Tests

- `replika_time_law_is_documented_and_the_rest_is_reported` (`effects/timemod.rs`): reads a v0x12 stream, checks time maps to 500 ms at 0.25 (2000 x 0.25), and checks that all 28 other controls are reported with a reason and none is `Reason::Unknown`. No native vector: this is a law check, not a native comparison.
- `replika_and_friends_stay_unsupported_with_a_reason`: unchanged, covers other versions.

No native-data vector exists for any Replika behaviour. The Wine oracle capture
(k-delays.md) could not move feedback, time or tone, so nothing was fixed against it.

## Open items

1. Constants: a read-only dump of the `DAT_` values above from the Kontakt 8 image. Needs coordinator approval, because it reads the binary.
2. Binding: the ENGINE_PAR name-to-id table, so each saved name ties to a wrapper index. Only TYPE (0x3d5) is tied, and only by the error string.
3. Update path: `0x141abd290` and `0x141acc600`, which set the envelope and section coefficients.
4. Core loop: finish `0x141aa9480` (ring length, read offsets, feedback) and `0x141aa44a0`.
5. The oracle path for Replika: `set_engine_par` with a known control, per k-delays.md.
