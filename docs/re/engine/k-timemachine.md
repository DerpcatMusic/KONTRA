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

## Wiring finding (checked in code)

- The engine is live: `dsp.rs` allocates `StretchEngine`s and `source.rs` renders them through
  `StretchParams` (around `source.rs:2804-2850`).
- `lower.rs::stretch_spec` (`crates/sampler-core/src/lower.rs:244`) copies only kind, HQ, speed,
  smooth and grain. The TM2 controls (Transient Copy/Size, Grain profile) and the Pro controls
  (Keep Formants, Formant Shift, Env Order) are dropped at that boundary. The Kontakt reader does
  report them as NotModeled, so nothing is silently ignored at load, but the core cannot see them.
- Changing the core `Spec` to carry them is a prerequisite for any TM2/Pro law. It is not done in
  this PR because the fields would be unread until the laws exist.

## Plan once READY exists

1. Recover TM2 mark detector and transient scheduling from `source/kontakt8/` (`rg` on
   `Time Machine 2` / `PSOLA` / `transient`), record constants here.
2. Recover the Pro backend's formant and envelope-order laws; decide the shared formant-
   preserving core (extend `time_machine.rs` `Scheduler`, not a Pro-only copy).
3. Recover the tempo-sync law from the timing field path and replace the UnknownLaw note.
4. Add one golden / recorded-vector test per recovered behaviour.

Until then the stand-ins stay labelled and reported; nothing here is claimed as parity.

## Decompile evidence (Kontakt 8, `source/kontakt8/`, READY)

Verified in the export (function names are `FUN_<addr>`; identity comes from class names in bodies):

- **Engine classes.** Time Machine 2 is `BTimeMachine2` (ctor `FUN_140cd9320`, `0140cd0000.c`).
  Time Machine 1 is `BTimeMachine` (ctor `FUN_140cd9470`). Pro is `BTimeMachine3` (ctor
  `FUN_140cd9380`). The stretch backends are `BTM3ElastiqueWrapper<CElastiqueDirectIf,0|1>`,
  `BTM3ElastiqueWrapper<CElastiqueProDirectIf,1>`, `BTM3ElastiqueWrapper<CElastiqueProV3DirectIf,0|1>`,
  plus `CPSOLAAnalysisRT` / `CPSOLAPreAnalysis` (PSOLA) and `PSOLASynthForElastiqueProV3` (`01419d0000.c`).
- **Backend selection.** `FUN_140ce7230` picks the wrapper from a kind (2 = Elastique direct,
  3 = Elastique Pro V3) and a second flag (0/1, which looks like HQ). It is called from `FUN_140ce7b70`
  (voice reset), which is driven by `FUN_140ce8d20` and `0140980000.c:1139`.
- **Constructor defaults** (field identity not yet confirmed, values as stored):
  - TM2: 1000 at +0xbc, 0.7 (`0x3f333333`) at +0x18, speed 1.0 at +0x4c, a flag at +0x17 set to 1.
  - Pro: 10000 at +0x19c, 1024 (`0x400`) at +0x1a4, 2 at +0xec and +0x35, 128 (`0x80`) at +0x39
    (matches the public Formant order default of 128), a flag at +0x1c4 set to 1.
  - TM1: speed 1.0 at +0x4c.
- **UI labels:** "Time Machine 1", "Time Machine 2", "Time Machine Pro" (`0140610000.c:3769-3773`), and
  Time Machine 2 toggles "Transient Copy" (param id `0x317`, `01405f0000.c:7584`), "High Quality" (`0x311`)
  and "Internal Trigger" (`0x315`) in the same switch. Pro's record name is "TimeMachinePro".

## Still unrecovered (not implemented; no law fabricated)

- Which stored mode (2, 5 or 8) selects which backend: the caller chain runs through a mode switch I
  did not finish tracing.
- The mapping from stored fields (`dfd_flag`, `dfd_integer_1/2`, `pro_flag_*`, `pro_float_*`) to
  BTimeMachine2/3 members, and the units of Transient Size and Formant Shift.
- PSOLA mark analysis and transient-copy scheduling (`CPSOLAAnalysisRT`), and the Elastique Pro V3
  formant and envelope-order laws. Elastique is a third-party library; reproducing it needs an own
  implementation written from measured behaviour, not from its code.

## Gap-table status

| Row | Status after decompile |
| --- | --- |
| TM2 PSOLA mark analysis | evidence located (`CPSOLAAnalysisRT`), law not extracted |
| TM2 Transient Copy / Size / Grain profile | UI label and param id found; field mapping open |
| Pro Elastique backend | backend classes located; algorithm not reproducible from the code |
| Pro Keep Formants / Formant Shift / Env Order | constructor defaults found (order 128, env 2); setters open |
| Pro v0x100 legacy record | open |
| TM1 HQ path, tempo-sync law | open (not in this pass) |

## KSP parameter writes: measured laws (native vectors)

Measured with `tools/kontakt-oracle/tm_sweep.py` on Kontakt 8 under Wine. Each sweep step is a
fresh instrument whose init script runs `set_engine_par(PAR, raw, 0, -1, 0)`; the saved
`.nki` is read back with `tm_source_dump` (ni_file's group source reader). Only fields that differ
from the fresh-instrument baseline are reported.

| KSP write | Stored field | Kinds | Law (raw 0..1e6) | Evidence |
| --- | --- | --- | --- | --- |
| `SPEED` | `common_float_7` | TM1, TM2, Pro | table, 21 points at 0.05 (0 -> 0, 0.5 -> 1, 1 -> 8) | `docs/re/kontakt-bindings/tm_source_sweep.tsv` |
| `SMOOTH` | `machine_float_1` | TM1 | identity 0..1 | `docs/re/kontakt-bindings/tm_source_sweep.tsv` |
| `GRAIN_LENGTH` | `machine_float_2` (ms) | TM1 | table, 0.3 -> 100 ms (21 points) | `docs/re/kontakt-bindings/tm_source_sweep.tsv` |
| `FORMANT_SHIFT` | `pro_float_1` | Pro | linear -1..1 | `docs/re/kontakt-bindings/tm_source_sweep.tsv` (0, 0.25, 0.75, 1) |
| `TMPRO_KEEP_FORMANTS` | `pro_flag_2` | Pro | switch, 0 = off | `docs/re/kontakt-bindings/tm_source_sweep.tsv` |

Generic index: the corpus writes group-level parameters with generic -1 (`set_engine_par(p, v, group, -1, -1)`),
and generic 0 also reaches the record. Both are read (`group_engine_write`), and the sweep shows the same result for each.

`SMOOTH` and `GRAIN_LENGTH` on TM2 and Pro: measured with generic -1 at raw 0 and 1e6; no stored field
changes for either kind, so those writes are reported as unapplied. `SPEED_UNIT`, `TRANSIENT_SIZE` and
`TM_LEGATO` have no measured law.

Applied in `crates/sampler-kontakt/src/time_stretch_writes.rs` and tested against the measured points.
The engine's speed, smooth and grain change with these writes, while `FORMANT_SHIFT` and
`KEEP_FORMANTS` only update the carried Pro record, because the Pro formant stage does not exist yet.

## Oracle status

The Wine oracle (`tools/kontakt-oracle/render.py` with a Lua probe; PipeWire `kontra_ref` sink) can
render Kontakt audio. No native Time Machine audio render has been captured, so no row is at
`parity` in the audio sense: the laws above are parameter-level measurements.

## Per-instance state blocks (decompile, `0140d20000.c`)

Each Time Machine voice owns one state block, allocated and constructed by:

| Allocator | Block (bytes) | Constructor | Class |
| --- | --- | --- | --- |
| `FUN_140d203e0` | 0xd8 (216) | `FUN_140cd9470` | `BTimeMachine` (TM1) |
| `FUN_140d204c0` | 200 (0xc8) | `FUN_140cd9320` | `BTimeMachine2` (TM2) |
| `FUN_140d205e0` | 0x1d0 (464) | `FUN_140cd9380` | `BTimeMachine3` (Pro) |

Correction: `FUN_140d202f0` (0x1800 block, constructor `FUN_140cd8ea0`, vtable `BMPC60`) is the
MPC60 Machine state, and `FUN_140d20350` (0x1fc0 block, constructor `FUN_140cd9220`, vtable
`BSP1200`) is the S1200 Machine state. Neither is a Time Machine.

The process loops of the TM2 and Pro states are not identified yet. They are reached through
vtables, and the `CPSOLA*` and `CElastique*` methods carry no names in the export, so the cores
below remain unimplemented.
