# Kontakt 8 source modules: Beat Machine, S1200 Machine, MP60 Machine

Status: **waiting on READY.** `/data/dev/workspace/Research/KONTRA/source/kontakt8/` is empty and has no `READY` marker, so no decompiled function has been read. Nothing below is a recovered law. Every item is either repo evidence (cited) or an open question.

## Evidence used

- Stored source-mode IDs and the field layout per mode: `docs/architecture-v2/KONTAKT_OBJECTS.md` (source-mode records, 0x0e). Stored 4 = Beat Machine (internal 5), 6 = S1200 (internal 7), 7 = MP60 (internal 8).
- Loader: `crates/sampler-kontakt/src/library.rs:954-963` (`source_mode_name`), `:1617-1644` (non-TM sources are "played as a sampler" and reported `NotModeled`).
- Group start criteria for legacy beat-sequencer groups and slice events: `docs/re/KONTAKT_START_CONDITIONS.md:88-90`.
- Engine lo-fi core: `crates/sampler-core/src/dsp/color/lofi.rs` (sample-hold, bit quantizer, LCG noise), mapped from `ENGINE_PAR_BITS` / `ENGINE_PAR_FREQUENCY` at `crates/sampler-kontakt/src/library.rs:2764-2765`.
- S1200 filter parameter: `$ENGINE_PAR_S1200_FILTER_MODE` (`crates/sampler-core/src/engine_parameter_names.rs:484`); parity ledger rows `$NI_S1200_FILTER_{LOW,LOW_MID,HIGH_MID,HIGH,NONE}` and `$NI_SOURCE_MODE_*` marked `Approximate` / `OPAQUE` (`crates/sampler-ksp/src/parity_ledger.rs:1413-1436`).
- Coverage: `docs/architecture-v2/DSP_COVERAGE.md:214-216` lists all three as `missing`, `unmeasured`, corpus 0.
- Public docs: the Kontakt manual page (`docs.native-instruments.com/.../classic-view`) returned HTTP 403. Web search returned no parameter lists, only tutorial indexes and a community feature request asking for SP1200/MPC60 engines.

## Stored record fields

| Stored mode | Module | Extension fields (`KONTAKT_OBJECTS.md`) | Meaning |
|---:|---|---|---|
| 4 | Beat Machine | `slice_float_1`, `slice_float_2`, `slice_flag_1` (+ another bool at v0x106) | unassigned |
| 6 | S1200 Machine | none (common fields only) | control values must come from elsewhere (script / engine parameters) |
| 7 | MP60 Machine | none (common fields only) | same as S1200 |

Open question: S1200 and MP60 carry no per-group control fields, so their controls are either stored as engine parameters or implied by the mode. This needs the decompiled reader and a saved-instrument cross-check.

## Gap table

| Module | Shared engine already covers | Missing or unverified |
|---|---|---|
| Beat Machine | Sampler voice playback; slice start criteria (`START_CRITERIA_SLICE_IDX`, slice attack/release params) | Beat-pattern / slice sequencer generator (legacy sequencer identified in group `+0xba8 == 5`, timing law unrecovered); slice record fields have no assigned meaning; no `Beat Machine` source path (currently played as a sampler, reported). |
| S1200 Machine | Sampler playback; lo-fi core (hold period from `Frequency`, quantizer from `Bits`, noise) | Mode-to-LoFi mapping and its laws are not verified for this module; sample-rate reduction law is the Kontakt LoFi law, not confirmed for S1200; filter (`$ENGINE_PAR_S1200_FILTER_MODE`, low / low-mid / high-mid / high) has no engine core; ledger status `Approximate`. |
| MP60 Machine | Sampler playback; lo-fi core as above | Same as S1200 except no filter stage is listed in the ledger; its own bit/rate law unverified. |

Confidence: high that the three modes are currently played as a plain sampler and reported (the diagnosis is correct). Low on everything else.

## Planned work (after READY)

1. Read `source/kontakt8` for the three mode branches of the source-mode reader and the playback dispatch; record slice-field and S1200/MP60 control sources here.
2. Map S1200 / MP60 onto `sampler-ir` lo-fi and a general filter core only if the decompiled law matches an existing core. Do not add a per-product DSP copy.
3. Beat Machine: map slice events onto the existing slice start criteria; the pattern generator needs its own evidence.
4. Add a golden or null test for each verified law. Unsupported parts stay reported.

No engine code or tests changed in this PR yet, because no law has been verified.

## Decompile evidence (READY, 2026-10-21)

Confirmed in `source/kontakt8/` (addresses are Kontakt 8.exe VAs; the exports are data, not pasted here):

- **S1200 filter mode is engine parameter 0x318 (792).** The `set_engine_par` validator (`0140780000.c`, function `FUN_140789c30`, around line 6145) accepts `0 <= value < 6`, else reports "S1200 Machine filter mode is out of range!". Six values: the ledger's Low / Low-mid / High-mid / High / None plus one more. The filter's law and the stored field are still unlocated.
- **Source-mode names and stored order** match `KONTAKT_OBJECTS.md`: the enum name table is built in `FUN_141315310` (`0141310000.c`, names DirectFromDisk, TimeMachine2, TimeMachinePro, BeatMachine, S1200Machine, MP60Machine).
- **Beat Machine has its own internal-trigger group path.** `0140870000.c` disallows "internal-trigger beatmachine group" in a group-disallow routine; `FUN_141310df0` (`0140490000.c:767-774`) loads the beat-machine and S1200 name strings.
- **Engine names `bitdepth` = 0x65 (101) and `downsample` = 0x66 (102)** are registered in `0140a60000.c:118-119`. These are the Lo-Fi insert parameters (already mapped in `library.rs:2764-2765`), not S1200 source controls.
- **A `SrcModePreset` class exists** (constructed in `FUN_140d1d860`, `0140d10000.c:8497`, type 4 preset). Its member layout and what it computes are not yet decoded.

Not found in the export (still open):

- Parameter IDs and names for the S1200 bit-depth / rate controls, and for MP60's controls (no "S1200"/"MP60" control names in any string table I could find; the only S1200 engine name is the filter-mode error text).
- The per-voice DSP path for modes 4, 6 and 7. The sample-rate / bit-depth law, the filter coefficients, and the slice/pattern generator are unlocated.
- The stored control fields for S1200 / MP60 (common fields only, per `KONTAKT_OBJECTS.md`).

Gap table status: unchanged. No row is closed. The next searches are `SrcModePreset` method bodies and the group voice-render dispatch on source mode.
