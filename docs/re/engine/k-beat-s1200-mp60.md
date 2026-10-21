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
