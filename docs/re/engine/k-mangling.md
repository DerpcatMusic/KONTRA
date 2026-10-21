# Kontakt 8 Mangling effects: Beat Masher, Beat Slicer, Gater, Reverse Grain, Transpose Stretch

Status: **no DSP law recovered.** The Kontakt 8 decompile is not complete (`/data/dev/workspace/Research/KONTRA/source/kontakt8/` has no `READY` marker and no files yet), and the public Kontakt effect reference returns HTTP 403 to WebFetch. Nothing below is a verified law. The entries are repo evidence (cited) or open questions. This PR diagnoses the five modules by name and reason instead of dropping them silently.

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
