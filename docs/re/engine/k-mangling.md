# Kontakt 8 Mangling effects: Beat Masher, Beat Slicer, Gater, Reverse Grain, Transpose Stretch

Status: **no DSP law verified; call structure mapped.** The Kontakt 8 decompile export is now READY (`/data/dev/workspace/Research/KONTRA/source/kontakt8/`), and the mapping in the next section comes from it. The public Kontakt effect reference returns HTTP 403 to WebFetch. Nothing below is a verified law. The entries are repo evidence (cited) or open questions. This PR diagnoses the five modules by name and reason instead of dropping them silently.

## Evidence used

- Serialization IDs and reader VAs: `docs/re/kontakt-catalog/ser_classes.tsv` (`BParFXBeatMasher` 0x71 @ 0x140ac4ee0; `BParFXGater` 0x72 @ 0x140ac5980; `BParFXBeatSlicer` 0x73 @ 0x140ac5070; `BParFXReverseGrain` 0x74 and `BParFXTransposeStretch` 0x75 both @ 0x140ac5fa0, different vtables 0x4e745d0 / 0x4e74730).
- Field layouts: `docs/re/kontakt-catalog/reader_schemas.tsv` (Beat Masher `FFFBBBI`, Gater `Fx4Bx4`, Beat Slicer `FFBBBII`, Reverse Grain and Transpose Stretch `Fx4BBB`). These are reader schemas, not field meanings.
- Decode status: `docs/architecture-v2/KONTAKT_FX_MOD_FORMAT.md` (~line 300): Gater and Reverse Grain have fixed layouts that decode, with `param_N` fields whose physical and KSP laws are unassigned. Native SerType returns 0x72 and 0x74 for these two. Rows at lines 257-258 mark 0x72/0x74 as retained opaque; Beat Masher, Beat Slicer and Transpose Stretch have no structural row.
- Coverage: `docs/architecture-v2/DSP_COVERAGE.md:175-183`: all five `missing`, `unmeasured`, 0 corpus instruments.
- Audit ledger: `tools/coverage/data/audit/kontakt.tsv` KE064 (0x72 Gater, PROVED serialization ID); KE065 (0x74 Reverse Grain / Transpose Stretch collision, UNPROVEN).
- Repo mapping before this PR: `crates/sampler-kontakt/src/effects/mod.rs` named 0x72 and 0x74 only; 0x71, 0x73 and 0x75 showed as `unknown effect 0x7x`.
- KSP parameter names: `crates/sampler-ksp/src/vendor_constants.txt` (`ENGINE_PAR_BEATMASHER_*`, `BEATSLICER_*`, `GATER_*`, `RG_*`, `TS_*`, `NI_BEATMASHER_LENGTH_*`) and ledger rows in `crates/sampler-ksp/src/parity_ledger.rs:264-277, 497-508, 670-676, 869-875, 1168-1179`, all `Approximate`.
- Shared engine: `crates/sampler-core/src/source` and `ownership.rs:759` already have a time-stretch engine (`StretchSpec`, `StretchMode::TimeMachine2`, grain size). `crates/sampler-core/src/dsp/timemod` has delay/modulation cores only; no gate, step-sequencer, granular or slicer core exists.
- Public docs: `docs.native-instruments.com/.../effect-reference` returns 403. A web search found no Kontakt 8 parameter list for these modules.

## Module notes

| Module | Serialization | Reader fields | Parameter names available (KSP) | Known law | Open |
|---|---|---|---|---|---|
| Beat Masher | 0x71 | `FFFBBBI` | `BEATMASHER_{GATE,LENGTH,MASH,MIX,REVERSE,ROTATE,WRAP}`, `NI_BEATMASHER_LENGTH_*` (note-value enum) | none | field-to-parameter binding, repeat/slice engine, length enum to beats |
| Beat Slicer | 0x73 | `FFBBBII` | `BEATSLICER_{BUZZ,GATE,MIX,PATTERN,SLICE,STYLE,TWOBARS}`, `SLICE_ATTACK/RELEASE` | none | pattern generator; whether slice events reuse the sampler's `START_CRITERIA_SLICE_IDX` path |
| Gater | 0x72 | `Fx4Bx4` | `GATER_{GATE,MIX,MUTE_INPUT,NOISE,RATE,RATE_SYNC,SHAPE,STUTTER}` | none | gate envelope shape, step rate law, sync table, noise source |
| Reverse Grain | 0x74 | `Fx4BBB` | `RG_{FORWARD,GRAIN,INVERT_GRAINS,MIX,PITCH,REVERSE,SPEED}` | none | grain size/overlap law, reverse-grain scheduling, pitch law |
| Transpose Stretch | 0x75 | `Fx4BBB` (shared with 0x74) | `TS_{AMOUNT,GRAIN,KEY,MIX,SIZE,STRETCH,TWOBARS}` | none | whether 0x74 and 0x75 are really different objects (KE065); stretch engine binding |

Units, ranges, defaults and display conversions are unknown for all five. The KSP names are evidence the parameters exist, not their laws.

## Decompile findings (Kontakt 8 export, READY)

Confirmed from `source/kontakt8` and a read-only copy of the Ghidra project. No law below is final.

- Module classes exist as `BeatMasherModule`, `BeatSlicerModule`, `GaterModule`, `ReverseGrainModule`, `TransposeStretchModule`. Their factories register under those names (`FUN_141ef6a70`, bucket `0140650000.c` lines ~1335-2195). Constructors: Beat Masher `FUN_14064d520`, Beat Slicer `FUN_14064d670`, Gater `FUN_14064ed20`, Reverse Grain `FUN_14064fe60`, Transpose Stretch `FUN_140650a90`.
- The five vtables share one skeleton. Against Hilbert Limiter's vtable they differ in four slots only: 0, 3, 16 and 66. Slot 0 is the same in the four stretch/slice/grain modules, and Gater differs there from the others.
- Slot 16 (initialiser) builds child objects by id. It uses a lookup `FUN_141ef9200(module, id)` that walks a child list and matches each object's id at +0x58. Gater's init fetches ids 0x14, 0x15, 0x19, 0x1a and creates parameter objects with ids 0x17, 0x18 and 0x5, 0xa, 0xd, 0x7, 0xc (`FUN_1405079e0` / `FUN_140507a40`). The ids are not yet mapped to KSP names.
- Slot 66 (parameter binding) runs only when the module reports type 0x31 in its descriptor (`FUN_140655b30`). It binds five parameter groups and one id 0x1f2.
- The Gater's pre-processing, gate envelope, step rate and noise are not located yet. The per-sample audio routine is not identified: it is not among the vtable slots decompiled so far.

What this changes in the gap table: rows still `Missing`, but the next step is now concrete: identify the audio routine (base-class process, probably a slot above 66 or a call from slot 16's children) and the id-to-name map for each module.

## Gap table

| Module | Shared engine already covers | Missing or unverified |
|---|---|---|
| Beat Masher | Sampler voice playback; stretch engine for time-changed grains (`sampler-core` source stretch) | Repeat/slice re-trigger engine, wrap/rotate/reverse semantics, length enum; no dedicated core. |
| Beat Slicer | Slice start criteria and slice attack/release (`START_CRITERIA_SLICE_IDX`, `SLICE_ATTACK/RELEASE`) | Pattern/style/gate generator and two-bar timing; no gate or pattern core. |
| Gater | Voice gain and LFO-style modulation exist in the timemod/voice-mod family | Step/gate envelope core (shape, rate, sync, stutter, noise). No such core exists. |
| Reverse Grain | Stretch engine granular path (`grain_ms`); sampler reverse playback | Grain scheduler with reverse/invert/forward selection and its pitch law. |
| Transpose Stretch | Stretch engine (`StretchMode::TimeMachine2`, `speed`, `grain_ms`) | Mapping of `TS_*` to the stretch spec, two-bar mode, key/transpose law; 0x74/0x75 identity unresolved. |

Shared-core question: Beat Masher, Reverse Grain and Transpose Stretch all reduce to grain/repeat playback on a source, which the existing stretch engine may cover after mapping. Gater and the Beat Slicer pattern generator need a new general gate/step core. Neither is built here.

## What this PR changes

- `crates/sampler-kontakt/src/effects/mangling.rs`: reason table for 0x71-0x75 (`unproved`). Each reason names the reader VA and field layout and says what is not extracted.
- `crates/sampler-kontakt/src/effects/mod.rs`: names 0x71, 0x73, 0x75 in the module table; chains `mangling::unproved` into the NotModeled diagnostic; two tests (reason table covers 0x71-0x75 only; a slot for each module reports `NotModeled` with its name and reason).
- No DSP, no coefficients, no format-crate mapping: none could be verified.

## Next steps

1. When `source/kontakt8/READY` appears, read the reader functions at the VAs above (`rg -n "0x140ac4ee0|0x140ac5980|0x140ac5070|0x140ac5fa0" source/kontakt8`) and the three setter/playback paths. Record fields, enums, ranges and defaults here.
2. Black-box measurement with `tools/kontakt-oracle/probe_fx.py` (`scan`, then `sweep` with `--pars`) on Mangling presets. This binds stored fields to ENGINE_PAR names and gives the law shapes. Not run yet: needs factory preset paths and a Wine session.
3. Map to the shared engine: stretch-spec for Reverse Grain / Transpose Stretch / Beat Masher if the decoded laws match; one new general gate/step core for Gater and the Beat Slicer pattern. Each law gets a golden or null test.
4. Resolve KE065: confirm whether SerType 0x75 is ever written in the corpus.

## Confidence

- High: the five modules exist with the serialization IDs and reader VAs above, and the repo has no DSP for them.
- High: no current code applies the KSP `ENGINE_PAR_BEATMASHER/BEATSLICER/GATER/RG/TS_*` writes. `rg` finds them only in the KSP crate, the parameter-name table and the ledger. Not verified: whether the KSP lowering emits a diagnostic for these writes.
- Low: every parameter meaning, unit and law.

## Extraction map (decompile, 2026-10-22)

Source: `source/kontakt8/*.c` (`// ==== <addr> FUN_<addr> size=N` headers), vtables from `docs/re/kontakt-rdata/vtables.tsv.gz`. Sizes are decompiled byte sizes, not disassembly.

The five modules are the Traktor-family FX wrapper with a different core. Each has the same three-level shape:

| Level | Beat Masher | Beat Slicer | Gater | Reverse Grain | Transpose Stretch |
| --- | --- | --- | --- | --- | --- |
| Class (NI::DSP) | `BeatMasher2` | `BeatSlicerB` | `Gater` | `ReverseGrains` | `Transpose` |
| Wrapper vtable (slot 23 = process) | `ModulatableDSPCoreWrapper<BEffectBeatMasherTraits>` 0x144e92750 | `...BeatSlicerTraits` 0x144e92a08 | `...GaterTraits` 0x144f16360 | `...ReverseGrainTraits` 0x144e93a78 | `...TransposeStretchTraits` 0x144e93f50 |
| Driver (slot 23 target) | `FUN_1408fd2e0` (1221 B) | `FUN_1408fd7b0` (1221 B) | `FUN_1409003c0` (1237 B) | `FUN_140901fb0` (1221 B) | `FUN_140903c90` (1221 B) |
| Block core | `FUN_141aa4540` (207 B) | `FUN_141aa5c10` (207 B) | `FUN_141ae2d20` (563 B) | `FUN_141ae3ef0` (151 B) | `FUN_141ae5180` (151 B) |
| Per-sample kernel | `FUN_141aa4610` (5630 B) | `FUN_141aa5ce0` (not sized) | `FUN_141ae2f60` (1171 B) + `FUN_141a9d870` (738 B) + `FUN_141acb6f0` (398 B) | `FUN_141ae3f90` (4592 B) | not located yet |

Shared by all five drivers: `FUN_140ac28f0` (1710 B, pre-block) and `FUN_140ac21f0` (1777 B, post-block), both calling `FUN_14097cc50` (58 B). The drivers loop in blocks of at most 0x20 frames (`param_1+0x1bc`) and do per-block peak tracking on the input (the `DAT_144714b90` mask) before calling the block core.

Gater specifics read so far (not verified):
- The driver flips a one-shot state (`+0x3dc` 1 -> 2) once the phase `+0x3c8` passes a threshold.
- The block core advances a double-precision phase (`+0xf8`, step `+0x100`, wrap `DAT_144714998`) and a float phase (`+0x2c`, step `+0x34`), and calls `FUN_141acb6f0` (398 B) with the phase-derived value. This is the rate/sync path.
- The per-sample kernel `FUN_141ae2f60` keeps a 32-bit LCG (`x = x*0x41c64e6d + 0x3039`, the noise source) and a gate with a three-stage step/trigger latch (`+0x74` flag bits 1, 2, 4, each with its own ramp at `+0xb0/+0xb4`, `+0xc0/+0xc4`, `+0xd8`). The gain shaping is a cubic polynomial with constants `_DAT_1479ab1bc`, `_DAT_1479ab1d4` and `_DAT_147698870`. Shape, rate table and stutter mode are not yet tied to those fields, and the constants are not extracted.

Beat Masher kernel `FUN_141aa4610` (5630 B) is not read beyond its header (it reads counters at +0x15c and +0xc4).

What this does not give: no coefficient, no field-to-KSP-name map (the `ENGINE_PAR_*` names come from `a80000.c` `FUN_140981540` registrations, e.g. `beatmasher_rotate` 0x1e9, `gater_rate` 0x1f2), no unit or range, no vector. Each module still needs the per-sample kernel read end to end, the constants pulled from `docs/re/kontakt-rdata/constants.tsv`, and a kemu vector run (`tools/re/*_kemu_vectors.py`) before any of it is a law.

Next steps, in order: (1) Gater `FUN_141ae2f60` + `FUN_141a9d870` + `FUN_141acb6f0` into a kemu vector template; (2) Reverse Grain `FUN_141ae3f90` (4592 B) with its scheduler; (3) Beat Masher `FUN_141aa4610`; (4) Beat Slicer `FUN_141aa5ce0` and the Transpose Stretch kernel (not located); (5) KE065 (0x74 vs 0x75 identity), still open.

## Gater pass, 2026-10-23 (read, no port)

Listings are read from the decompile cache (`source/kontakt8/*.c`, not copied here, per ENGINE.md evidence rule). Scratch helper `fn.py` lives outside the repo. Status: understanding only; no kemu vector, no Rust code, parity rows stay `missing`.

Constants resolved from `docs/re/kontakt-rdata/constants.tsv` (u32 column is authoritative for f32 literals):

| Symbol | Address | Value | Used for |
| --- | --- | --- | --- |
| `DAT_144714988` | 0x144714988 | 1.0 (double) | clock period, wrap test |
| `DAT_144714998` | 0x144714998 | 2.0 (double) | phase wrap step |
| `DAT_14470bb18` | 0x14470bb18 | -1.0 | phase wrap correction |
| `DAT_144f38dd0` | 0x144f38dd0 | -2.0 | phase wrap correction |
| `DAT_14470bae8` | 0x14470bae8 | 0.5 (double) | round-to-nearest in clock split |
| `DAT_14470bad0` | 0x14470bad0 | 0.5f | half-period scale (segment length) |
| `DAT_14470bad4` | 0x14470bad4 | 1.0f | unity level, ramp ceiling |
| `DAT_144714930` | 0x144714930 | 0.98f | per-sample smoothing of the kernel output level |
| `DAT_144f3859c` | 0x144f3859c | 1e-30f | denormal guard added to the smoothed level |
| `DAT_144714994` | 0x144714994 | 2.0f | peak-tracker decay step |
| `DAT_1447148a0` | 0x1447148a0 | 1e-5f | one-shot gate: peak below threshold x 1e-5 |
| `DAT_144725080` | 0x144725080 | 0.6f | peak-tracker factor (multiplied per sample) |
| `DAT_144f385a4` | 0x144f385a4 | 1e-18f | denormal guard added to kernel recursion |
| `DAT_144714b90` | 0x144714b90 | 0x7fffffff | abs mask on the peak tracker |
| `DAT_144714ba0` | 0x144714ba0 | sign mask (assumed 0x80000000, not read) | negation by XOR in the segment setup |
| `DAT_144f38920` | 0x144f38920 | (not read) | segment-length test in FUN_141abb0e0 |
| `DAT_144714a54` | 0x144714a54 | (not read) | attack slope numerator in FUN_141a883f0 |
| `_DAT_1479ab0e8` | 0x1479ab0e8 | f32 -1.5669e38 (u32 feebc2a0) | compare with abs(x): never true as read, so the branch that zeroes the step is dead. Double-check before relying on it |
| `_DAT_1479ab1bc` | 0x1479ab1bc | 0.13333f | cubic shaping coefficient in the kernel curve (x^3 term) |
| `_DAT_1479ab1d4` | 0x1479ab1d4 | 0.33333f | cubic shaping term |
| `_DAT_147698870` | 0x147698870 | 0.36f | added to the shaping sum before the division |
| `_DAT_1479af27c` | 0x1479af27c | 2^-31 (4.6567e-10 f32) | LCG int to float in [-1, 1) for the noise source |

Object roles from the driver and block core (offsets relative to the wrapper object, unless noted):
- Driver `FUN_1409003c0`: frame count at +0x120, channel count at +0x124, block cap at +0x1bc (set to 32 when zero), the block core object at +0x1c0, and a sync flag at core+0x1e (chooses `+0x28` tempo period over `+0x24` free rate). Before the loop it prepares per-channel pointers (+0x408, +0x498). It tracks the input peak as `|x| & 0x7fffffff`, stored at +0x3c8 (double). One-shot state +0x3dc goes 1 to 2 when the peak falls below `+0x3d8 x 1e-5`.
- Block core `FUN_141ae2d20` (core = wrapper+0x1c0): picks rate `+0x24` or `+0x28` into the clock (`FUN_141acae30`, which stores twice the value at clock+0x24). Clock object is at core+0x128. Per block it ticks the clock (`FUN_141acb6f0`, segment schedule), then runs the kernel once per frame through a per-voice struct at core+0x200 (input samples at +0xc and +0x58, outputs at +0x54 and +0x5c). Noise and rate contributions come through `FUN_141a98d20` (reads clock+0x70, the step level).
- Clock `FUN_141a9d870` (per sample): advances the phase double at clock+0x8 by the per-sample increment clock+0x18, wraps at 1.0, and updates two step segments (48-byte records at clock+0x78 and clock+0xa8, selected by the toggle at clock+0x38). Segment states: 0 attack (level is linear in time, goes to unity), 1 hold until the end time at +0x8c, 2 release (linear decay to zero), 3 done. Output level clock+0x70 is the running max over the two segments.
- Segment start `FUN_141acbc20`, `FUN_141abb0e0` (reads clock+0x24 and clock+0x2c, schedules the next segment with the half-period check), `FUN_141a883f0` (re-seeds the clock state): all three flip the toggle at clock+0x38, set the attack/release slopes and write the segment record. Their exact field-to-parameter meaning is not yet tied to KSP names.
- Kernel `FUN_141ae2f60` (per-frame): 4000-entry float ring at core+0x138 indexed by write position +0x130 and read position +0x134 (delayed read, wrap at 4000). LCG seed at +0x108 (`x = x*0x41c64e6d + 0x3039`). Flags at +0x74 bits 1, 2, 4, 8, 16, each a ramp with a level at +0x78 and a per-bit slope/offset pair (+0xb0/+0xb4, +0xc0/+0xc4, +0xd0/+0xd4, +0xe0/+0xe4, +0xf8/+0xfc). Output level path: smoothing `0.98 x + 1e-30`, then a cubic shaping of the ramp value, then the L/R outputs at +0x54 and +0x5c. The 1e-18 guard sits in the same recursion.

Still open for this module, in order: (1) the setter that writes ENGINE_PAR_GATER values into these fields (the module class vtable slots 1 and 2 at 0x1408dc160 / 0x140971020 are the candidates; `FUN_1408dc160` only clears state and calls `FUN_141ae1390`); (2) the stored-field to parameter binding for the `Fx4Bx4` record (reader VA 0x140ac5980); (3) the rate table behind `FUN_141acb6f0` for synced rates; (4) a kemu driver for the kernel and clock with a constructed object (ctor `FUN_14064ed20`); (5) the Rust port in `sampler-core/src/dsp` and a vector test.
