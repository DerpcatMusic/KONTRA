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
| Tempo-synced speed | `timing_unit` = 1, `timing_value` (beats) | sync speed law | - | **partly**: `SPEED_UNIT` writes decode to `timing_value` / `timing_unit` (measured table, `SPEED_UNIT_BEATS`, all kinds). The tempo-to-speed law is not measured yet (UnknownLaw, reported) |

Law/edge notes recorded in code: TM1 does not floor the base speed (TM2 and Pro floor at 0.01).

### Time Machine 2 (stored mode 5)

| Control | Stored field | Units | Status |
| --- | --- | --- | --- |
| Speed | `common_float_7` | ratio, floored at 0.01 | carried |
| Transient Copy | `dfd_flag` | bool | carried, unused |
| Transient Size | `dfd_integer_1` | source positions | carried, unused |
| Grain profile | `dfd_integer_2` | index 0..=4 | carried, unused |

Engine: **own pitch-synchronous core** (`Scheduler::Pitch`): autocorrelation period, forward pitch
mark, Hann grain over two periods on the synthesis grid. Not a port of Kontakt's estimator, and not
measured against Kontakt 8 yet (native vectors pending). Previously a WSOLA stand-in.

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
| TM2 PSOLA mark analysis | partial: own `Scheduler::Pitch` (autocorrelation period, forward mark) | yes | Kontakt's estimator is FFT-based (`FUN_1419d2d80`, below); transient scheduling unrecovered; no native vectors |
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
changes for either kind, so those writes are reported as unapplied. `TRANSIENT_SIZE` and
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

## Class vtables from RTTI (`tools/re/tm/`)

The export has no names for the stretch methods, so the MSVC x64 RTTI in `Kontakt8.exe` is the
source of class identity. `rtti_vt.py NAME` finds the `.?AV/.?AU NAME@@` type descriptor, the
complete object locator and the vtable (`vtable[-1]` holds the COL pointer). `slots.py NAME` maps each
vtable slot to its decompile header and size. Results:

| Class | Vtable | Notes |
| --- | --- | --- |
| `BTimeMachine2` | `0x144fd07d0` | slot 3 is the parameter transfer (`FUN_140cdb250`, 66 bytes); slot 5 `FUN_140ce6c00` |
| `BTimeMachine3` (Pro) | `0x144fd0530` | slot 3 transfer `FUN_140cdb2a0` (90 bytes); slot 10 `FUN_140ce6200` |
| `BTimeMachine` (TM1) | `0x144fd06f0` | slot 3 transfer `FUN_140cdb300` (108 bytes) |
| `CPSOLAAnalysisRT` | `0x1476a6598` | code slots 0-5 and 14 in `01419d0000.c`; `FUN_1419d0ed0` (230 B) and `FUN_1419d1600` (400 B) are the largest |
| `CPSOLAPreAnalysis` | `0x1476a6528` | code slots 0-2 (`FUN_1419d1140`, `FUN_1419d12a0`, `FUN_1419d11f0`) |
| `CElastiqueProCore` | `0x1476a6bb8` | slot 2 `FUN_1419d64d0` (931 B), slot 6 `FUN_1419d6890` (1076 B), slot 11 `FUN_1419d7230` (894 B) |
| `CElastiqueProV3Direct` | `0x1476a69b8` | 16 slots of 11 bytes: a thin accessor set, not the render |

The TM2 and Pro transfers name the state fields: the speed setter is reached through vtable slot 5
(`*(param+0x28)`) with the speed in the source record's float at `+0x38`; the int at `+0xbc` (TM2) and
`+0x1ac`/`+0x1cc` (Pro) are the stored integer controls, and the flags at `+0xb8` and `+0x1c4`
are the transient/formant flags.

The per-block processing methods (the CPSOLA and CElastique render loops) are the large slots
listed above. Their algorithm is not yet read, so no core is implemented from them.

## Measured in this pass (native, Kontakt 8)

| KSP write | Stored field | Kinds | Law | Evidence |
| --- | --- | --- | --- | --- |
| `TRANSIENT_SIZE` (id 54) | `dfd_integer_1` | TM2 only | table, 11 points at 0.1 (100 -> 5000) | `tm_source_sweep.tsv` |
| `TM_LEGATO` (id 53) | `common_flag_11` | TM1, TM2, Pro | switch (any non-zero is on) | `tm_source_sweep.tsv` |

`TM_LEGATO` is reported, not applied, because the IR has no legato field and no engine consumer.
`TRANSIENT_SIZE` is applied to the TM2 record, but the engine does not consume it yet, so it changes
the carried state only.

## Render loops read so far (own spec; decompile is data, no pasted code)

Vtable slots come from `tools/re/tm/slots.py`. Addresses are VAs in `Kontakt8.exe`.

### CPSOLAAnalysisRT (`0x1476a6598`)

- **Slot 1** (`FUN_1419d0ed0`, 230 B) is a reset, not a render: it copies the window/hop
  parameters (`+0x68`, `+0x6c`, `+0x70`, `+0x74`) into the working fields, clears the accumulators,
  sets the hop to `buffer_length / (channels * 2)` (`+0xa4`), starts the synthesis object at zero, and
  clears the pitch tracker state when present.
- **Slot 2** (`FUN_1419d1600`, 400 B) is the per-hop step. It appends the `n` new input frames to the
  sample counter (`+0x80`), runs the pitch tracker over the newest frames (`FUN_1419d1a80`, which feeds a
  windowed frame to the estimator and writes period/confidence into `+0x3c..+0x4c`), then places pitch
  marks. The mark advance is `ratio = clamp(+0xf8 / period, <= +0x50)` when a period is found, otherwise
  the last period in `+0xf4` (initialised from `+0x9c`). While the next mark (double `+0x140`) is inside
  the counter, the loop advances it by the ratio and calls the synthesis object's slot 7 with that mark.
  The call's arguments in the export do not match the slot 7 signature, so the emit is not yet confirmed.
  Mark time, `+0x7c`, is written back, and the buffer shift `FUN_1419cfd70` runs last.
- **Pitch tracker** (`CPitchTrack`, vtable `0x1476a6608`): the object is allocated with analysis size
  `0x1000` / hop `0x100` / window `0x400` for the quality mode 0 (`FUN_1419d1840`). Its ring buffer is
  masked (`& mask`). Period estimator `FUN_1419d2d80` (1441 B, `01419d0000.c:2094`) reads as: RMS of
  the rectified frame sets the level (`+0x28`, `+0x2c`) and a relative change (`+0x30`); the frame is
  windowed into `+0x58`, transformed through the vtable at `+0xb0`, and a peak-search over spectral bins
  between two constant-derived bounds picks the period candidates. It is spectral, not autocorrelation.

**Attribution correction:** the PSOLA analysis object (`CPSOLAAnalysisRT`) is constructed only inside
Elastique core setup (`FUN_1419d64d0` at `01419d0000.c:4801`, `FUN_1419c8b60` at `01419c0000.c:6128`,
both calling `FUN_1419ced60`). The TM2 mode-5 engine kind (`+0x138`) is not resolved, so the TM2 core
above is our design, not a recovered Kontakt TM2 algorithm.

### CPSOLASynthesis, TM2 ratio setter (vtable `0x1476a6040`, slot 7 = `FUN_1419ce540`, 131 B)

Constants read from `Kontakt8.exe` with `tools/re/tm/pe_const.py`:

| VA | Value | Use in the setter |
| --- | --- | --- |
| `0x14470bad4` | 1.0 | `K` in `K / speed` |
| `0x144fc4a1c` | 256.0 | clamp on the ratio `K / speed` |
| `0x144714a34` | 128.0 | scale: window/overlap length = `int(min(K/speed, 256) * 128)` |
| `0x14470bae8` | 0.0 | added to the scaled length |
| `0x144737748` | 0.99 | step factor for `speed >= 1` (`K/speed <= K`) |

For `speed < 1` the setter stores the rate `K/speed` and the reciprocal, and sets a flag at `+0x44`
to `0x3f7d70a4` (0.99). The Pro variant (`0x1476a6f08`, slot 7 `FUN_1419db490`) uses the same constants at
offsets `+0x3c..+0x54`.

### CElastiqueProCore (`0x1476a6bb8`), slots 2, 6, 11

- **Slot 2** (`FUN_1419d64d0`, 931 B) builds the core: it allocates a 0x670-byte block through
  `FUN_1419dcbe0` and a 0x148-byte block through `FUN_1419dd830`, then creates the sub-objects that
  carry the stages. It is setup, not a block render.
- **Slot 6** (`FUN_1419d6890`, 1076 B) is a phase dispatcher: the argument `param_4` selects the stage
  (`(param_4 + 1) % n`), each stage requires a minimum fill (257 frames for stage 1, 129 for stage 2) before
  it runs, and it calls sub-object virtuals (`+0x10`, `+0x28`, `+0xa8`). This is the block pipeline.
- **Slot 11** (`FUN_1419d7230`, 894 B) sets the time/pitch ratio: it rejects a request where
  `ratio * input < min` (returns 2), stores `1/ratio` and `ratio * input`, switches the resampler
  (`FUN_1419dd1d0`), and calls the stretch virtual `+0x60`.

Elastique is a third-party core. Only the dispatch and the ratio law are documented here. The algorithm
inside the stage virtuals is not read, so no Elastique stage is implemented.

## Status of the cores

- TM2 core: implemented as `Scheduler::Pitch` in `crates/sampler-core/src/source/time_machine.rs`
  (own design; tests on synthetic sines: unity passthrough (zero latency), marks on period boundaries,
  half-speed duration, octave-up pitch). It does not reproduce Kontakt's estimator or emit law. The
  emit call (7-argument call in `FUN_1419d1600`) is still unconfirmed.
- Pro stage: the Pro synthesis variant (`0x1476a6f08`) has the same ratio setter at other offsets. The
  formant and envelope order laws are still unread, so no Pro stage is implemented.

## Record-to-object transfers (decompile, `0140cd0000.c:6014-6040`; MP60/TM2 pass)

The per-class parameter transfers copy stored record fields into the engine object. Record offsets are the
source record's; object offsets are the `BTimeMachine2`/`BTimeMachine3` members. Field names come from the
measured KSP sweep above, not from the export.

| Transfer | Speed setter | Copies (record -> object) |
| --- | --- | --- |
| TM2 `FUN_140cdb250` (66 B) | record float `+0x38` via vtable `+0x28` | int `+0xbc` -> `+0xbc` (Transient Size, `dfd_integer_1`); byte `+0xb8` -> `+0x17` (Transient Copy, `dfd_flag`) |
| Pro `FUN_140cdb2a0` (90 B) | record float `+0x38` via vtable `+0x28` | ints `+0x1ac` and `+0x1cc` -> same offsets; int `+0x1c8` -> `+0x1c8`; byte `+0x1c4` -> `+0x1c4` (formant/envelope controls and flag, names not confirmed) |

What this does not give: the consumers of these members. TM2's transient scheduling and Pro's Elastique formant
and envelope-order laws are still unread, so nothing above changes the rendered audio.

## Consumer search on the cached rdata (docs/re/kontakt-rdata, no binary opened)

Vtable slots come from `vtables.tsv.gz`; bodies from the decompile export. What was narrowed, and what was not:

- **TM2 constructor** `FUN_140cd9320` (93 B, vtable `BTimeMachine2`): transient size `int +0xbc = 1000`,
  transient copy `byte +0x17 = 1`, `float +0xc0 = 0.7`, speed `+0x4c = 1.0`. The measured KSP range for
  `TRANSIENT_SIZE` is 100..5000, so 1000 sits inside it; this supports `+0xbc` = Transient Size. The constructor
  does not build an analysis object.
- **Second write path** for `+0xbc` and `+0x17`: `FUN_140cdac40` (135 B, TM2 vtable slot 18) takes the speed from a
  sub-object (`param+0x20`, vtable slot 5, via `FUN_1408a8390`), then writes `+0xbc` from block `+0x9c` and `+0x17`
  from block `+0x9a`. The record transfer `FUN_140cdb250` (slot 3) reads record `+0xbc` and `+0xb8`. The two source
  blocks are different, so the stored-field mapping still needs the KSP sweep to pick the right one.
- **TM2 render slots** `FUN_140ce2d20` (23, 2208 B), `FUN_140ce0dd0` (24, 2198 B) and `FUN_140cdc4d0` (25, 2056 B) do
  not read `+0xbc`, `+0x17` or `+0xb8` in their bodies. The transient-size consumer is not in these methods, so
  it is reached through another object. Not located.
- **Pro** `FUN_140ce6080` (slot 21, 14 B) copies block `+0x2d9d8` into Pro `+0x1a4` (constructor value 1024). The
  consumer of `+0x1a4` is not located in the Pro render slots checked so far.

Still open: the TM2 transient consumer, the Elastique Pro formant and envelope-order laws, and the grain-profile
field (`+0xc0`, constructor 0.7, unread).
