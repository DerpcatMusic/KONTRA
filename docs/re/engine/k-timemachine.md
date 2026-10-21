# Kontakt Time Machine family (TM1, TM2, Time Machine Pro)

Status: **partial, waiting on Kontakt 8 decompilation (READY not present)**.
Scope: Kontakt 8 only. TM1 is native in the shared engine; TM2 and Pro are labelled
stand-ins until their laws are recovered from the binary.

## Evidence used (no decompilation done here)

- Shared engine: `crates/sampler-core/src/source/time_machine.rs` (modes, two schedulers, `Params`).
- IR config: `crates/sampler-ir/src/lib.rs` (`StretchKind`, `TimeStretch`).
- Kontakt record mapping: `crates/sampler-kontakt/src/library.rs` (`stretch_from_record`, stored modes 2 / 5 / 8).
- Engine parameter names: `$ENGINE_PAR_TM_LEGATO`, `$ENGINE_PAR_TMPRO_KEEP_FORMANTS`
  (`docs/architecture-v2/KSP_AUDIT_ENGINE_PAR.tsv`, both currently "no engine consumer").
- Public docs: the Kontakt 8 manual Source-module page returned HTTP 403 to the fetcher, so
  control ranges and defaults below are **not confirmed from the manual**. Public search only
  confirmed that Pro exposes Keep Formants, Formant Shift and Envelope Order.

## Per-module spec

### Time Machine 1 (stored mode 2)

| Control | Stored field | Units / range | Default used | Status |
| --- | --- | --- | --- | --- |
| Speed | `common_float_7` | ratio, clamped `0..=64` | 1.0 | native |
| Smooth | `machine_float_1` | 0..=1 | 0.5 | native (two-reader crossfade, `N * smooth` frames) |
| Grain | `machine_float_2` | ms, engine clamps 0.3..=100 | 20 ms | native |
| HQ analysis | `machine_flag` | bool | false | **not modelled**: non-HQ scheduler used |
| Tempo-synced speed | `timing_value` > 1e-9 | sync law | - | **not modelled** (UnknownLaw, reported) |

Law/edge notes recorded in code: TM1 does not floor the base speed (TM2 and Pro floor at 0.01).

### Time Machine 2 (stored mode 5)

| Control | Stored field | Units | Status |
| --- | --- | --- | --- |
| Speed | `common_float_7` | ratio, floored at 0.01 | carried |
| Transient Copy | `dfd_flag` | bool | carried, unused |
| Transient Size | `dfd_integer_1` | source positions | carried, unused |
| Grain profile | `dfd_integer_2` | index 0..=4 | carried, unused |

Engine: **WSOLA stand-in** (`Scheduler::Grains`, 46 ms grain, overlap 4, search 0.5).
The original is PSOLA with mark analysis; not native. Reported as NotModeled.

### Time Machine Pro (stored mode 8)

| Control | Stored field | Status |
| --- | --- | --- |
| Speed | `common_float_7` | carried, floored at 0.01 |
| HQ resource class | `pro_flag_1` | carried; only selects WSOLA grain 93/46 ms and search stride |
| Keep Formants | `pro_flag_2` | carried, **unused** (`$ENGINE_PAR_TMPRO_KEEP_FORMANTS` has no consumer) |
| Formant Shift | `pro_float_1` | carried, **unused** |
| Envelope Order | `pro_float_2` | carried, **unused** |

Record version `0x100` uses a legacy field order and is reported, not mapped.
Engine: **WSOLA stand-in** in place of the Elastique backend. Not native.

## Gap table

| Capability | Shared engine today | Kontakt 8 | Gap |
| --- | --- | --- | --- |
| TM1 two-reader scheduler, smooth, grain | yes (`TwoReader`) | yes | HQ analysis path missing |
| TM1 tempo-synced speed | no | yes | sync law unrecovered |
| TM2 PSOLA mark analysis | no (WSOLA stand-in) | yes | mark detector + transient scheduling unrecovered |
| TM2 Transient Copy / Size / Grain profile | carried in IR, unused | yes | wire into scheduler once TM2 law known |
| Pro Elastique backend | no (WSOLA stand-in) | yes | backend unrecovered; formant-preserving stretch absent |
| Pro Keep Formants / Formant Shift / Env Order | carried, unused | yes | needs formant-preserving path |
| Pro v0x100 legacy record | not mapped | yes | field order unrecovered |

## Plan once READY exists

1. Recover TM2 mark detector and transient scheduling from `source/kontakt8/` (`rg` on
   `Time Machine 2` / `PSOLA` / `transient`), record constants here.
2. Recover the Pro backend's formant and envelope-order laws; decide the shared formant-
   preserving core (extend `time_machine.rs` `Scheduler`, not a Pro-only copy).
3. Recover the tempo-sync law from the timing field path and replace the UnknownLaw note.
4. Add one golden / recorded-vector test per recovered behaviour.

Until then the stand-ins stay labelled and reported; nothing here is claimed as parity.
