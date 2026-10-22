# Kontakt 8 Wavetable and Tone Machine source modules

Status: **partial, waiting on Kontakt 8 decompile.** The Kontakt 8 C export
(`Research/KONTRA/source/kontakt8/READY`) was absent when this was written
(directory empty), so no function-level Tone Machine or Wavetable law is
recorded here. Everything below is either repo evidence, public documentation,
or an explicit "unknown".

Scope: Kontakt 8 only. UVI/Falcon follows later.

## Evidence used

| Source | What it gives |
| --- | --- |
| `crates/sampler-core/src/wavetable.rs` | Cycle readout (2048 frames), cubic readout, linear morph between cycles, phase-form `warp` routine |
| `crates/sampler-core/src/wavetable_native_vectors.json` | Original-byte vectors for `0x140567aa0` (phase forms, 490 cases; test `phase_forms_match_original_kontakt_instructions`) |
| `crates/sampler-kontakt/src/wavetable.rs` | Saved-state admission (`admitted`) and its tests |
| `crates/sampler-kontakt/src/library.rs` ~1582–1640 | Source-mode dispatch: mode 9 → Wavetable, modes 1 / 2 / 5 / 8 / 4 / 6 / 7 → reported |
| `crates/sampler-ir/src/lib.rs` ~813–835 | `Wavetable` IR: `position`, `phase`, `form1`, `form2`, `form1_type`, `form2_type`; `supports_form` |
| `crates/sampler-core/src/lower.rs` ~270–310 | Lowering of the five live `ENGINE_PAR_WT_*` controls |
| `docs/re/conflux-phase-forms-20261013.md` | Phase-form identities 0/7–12/14–17/31–33 and constants |
| `docs/re/survey-conflux-gaps.md` | Six wavetable destinations and nine live engine parameters; no live route for them yet |
| `docs/architecture-v2/KONTAKT_OBJECTS.md` ~238–246 | Serialized source-mode table (Tone Machine = 1, Wavetable = 9) |
| `docs/architecture-v2/DSP_COVERAGE.md` ~210 | Tone Machine: `missing`, `unmeasured`, 0 corpus instances |
| `docs/architecture-v2/KSP_SYMBOLS.md` ~919–932 | Names of all `ENGINE_PAR_WT_*` parameters |
| NI KSP / Kontakt manual (online) | Parameter names and intent; the docs pages returned HTTP 403 to the fetcher, so ranges below are **not** confirmed from them |

## Wavetable (serialized mode 9)

### Parameters

| Parameter (KSP name) | Saved field | Units / law (repo) | Status in engine |
| --- | --- | --- | --- |
| `ENGINE_PAR_WT_POSITION` | `position` | 0..1 linear; selects a cycle, linear morph between adjacent cycles | Implemented (`Table::render_frames`) |
| `ENGINE_PAR_WT_PHASE` | `phase` | 0..1 onset phase within the cycle | Implemented; admitted range 0..1 |
| `ENGINE_PAR_WT_FORM` | `form1` | 0..1 amount for the first phase form | Implemented for admitted forms |
| `ENGINE_PAR_WT_FORM2` | `form2` | 0..1 amount for the second phase form | Implemented for admitted forms |
| `ENGINE_PAR_WT_FORM_MODE` | `form_type` (one-based) | Native phase-form ordinal 0..33 (see table) | Admitted ordinals only: 0, 7–12, 14–17, 31–33 |
| `ENGINE_PAR_WT_FORM2_MODE` | `form2_type` (one-based) | As above | As above |
| `ENGINE_PAR_WT_QUALITY` | `quality` | Unknown domain | **Not read.** Admission ignores it silently. Unverified |
| `ENGINE_PAR_WT_PHASE_RAND` | `phase_random` | Non-zero rejected | Rejected at admission; diagnosed by the source-mode report |
| `ENGINE_PAR_WT_INHARMONIC` / `_MODE` | `inharmonic`, `inharmonic_enabled` | Enabled rejected | Rejected at admission; diagnosed |
| `ENGINE_PAR_WT_MOD_WAVE` / `_TYPE` / `_AMOUNT` / `_TUNE` / `_TUNE_UNIT` | `mod_wave`, `mod_type`, `mod_amount`, `mod_tune` | `mod_type != 0` rejected | Rejected at admission; diagnosed |
| Live destinations `wavetablePosition`, `warpFactor`, `warpFactor2` | (routes) | Routed to `Target::Wavetable` | Implemented |
| Live destinations `wavetableModFrequency`, `wavetableModAmount`, `wavetableInharmonic` | (routes) | None | **Missing** (see survey-conflux-gaps) |

Amount forms and form values are 0..1 in the saved record; the native script
path (`lower.rs`) sets the form-mode range to 0..33 and other values to 0..1.

### Phase forms (native `0x140567aa0`)

| Ordinal | Form | Engine | Evidence |
| --- | --- | --- | --- |
| 0 | identity | Yes | vectors |
| 7 | PWM | Yes | `0x140567d2d` |
| 8, 9, 10 | asymmetric (2M/2P/…) | Yes | `0x140567d5b..` |
| 11 | Flip | Yes | `0x140567e64` |
| 12 | Mirror | Yes | `0x140567ed5`, seam constants |
| 14, 15, 16 | asymmetric | Yes | `0x140567d5b..0x140567e5f` |
| 17 | Seesaw | Yes | `0x140567f83` |
| 31, 32, 33 | second bend | Yes | `0x140567bf7..0x140567c84` |
| other IDs (e.g. Blinds, Fold, Exp, Log, sync forms) | — | **No** | Names exist (`KSP_SYMBOLS.md`), laws not measured |

### Behaviour and edge cases (repo-verified)

- Cycle length is always 2048 frames. The source sample rate and mapped root do not set the fundamental (`source.rs`: 440 Hz × 2048 / output rate step).
- Table starts on a 2048-frame boundary and holds at least one cycle; `Table::new` floors to whole cycles and ignores a trailing partial cycle (v1 behaviour, Kontakt's partial-cycle rule not recovered). The loader (`load.rs`, `wavetable_cycles_admitted`) admits the same geometry; a misaligned start or a sub-cycle zone is dropped.
- Phase stays inside one cycle across block seams.
- Readout is 4-tap Hermite (`hermite`, v1 arithmetic) per cycle, then linear blend between the two cycles selected by `position`.
- Key tracking must be on (`admitted` checks `key_tracking`); otherwise the group is reported, not played as a wavetable.

### Unknown or unmeasured (must come from the decompile)

1. Quality/anti-aliasing: whether `quality` changes the readout kernel or
   sample rate, its value domain, and the Kontakt default.
2. Whether Kontakt's readout is cubic/Hermite or another kernel; current
   readout is the v1 port, so exact parity is **unverified**.
3. Amplitude forms (not in the admitted set).
4. Phase randomness law (clock, distribution, per-voice or per-note).
5. Inharmonic / modulation-oscillator FM/PM/RM and their clocks.
6. Native control smoothing for position/forms.
7. Per-voice phase ownership across retrigger.

## Tone Machine (serialized mode 1)

### Engine behaviour (after this unit)

- Stored mode 1 maps to `StretchKind::ToneMachine` and to
  `time_machine::Mode::ToneMachine` in the shared time-stretch engine. It is no
  longer "played as a sampler".
- Saved fields: the Time Machine 1 machine record (Smooth F, Grain F, HQ B) as
  the repo's record comment says mode 1 shares field shapes with mode 2. The
  stored speed and timing flag follow Time Machine 1 (no 0.01 floor; speed 0
  freezes; tempo-sync reported).
- Backend: `Scheduler::Grains` with `grain_ms 30, overlap 4, search 0.5`.
  These constants are **UNMEASURED** and are a labelled stand-in. The importer
  reports the mode as `NotModeled` ("stand-in granular engine, unmeasured"), so
  the audio is not claimed as parity.
- `DSP_COVERAGE.md` still records Tone Machine as unmeasured.

### Parameters (public documentation, unverified)

Secondary source (Elephorm course, French; not NI): Smooth, Speed, Formant,
DC Filter, Legato. NI's manual (Kontakt 5 §20.3 excerpt) says it adjusts speed
and pitch of samples independently. Formant, DC Filter and Legato have no saved
field mapping yet.

### Unknowns to resolve by oracle measurement or decompile

1. Tone Machine grain size, overlap, alignment search, window (replace the
   stand-in constants).
2. Whether the speed floor and freeze semantics match Time Machine 1.
3. Formant, DC filter and Legato saved fields and their laws.
4. Whether Tone Machine's record really matches Time Machine 1 (repo comment
   only; no saved Tone Machine instrument checked).

### Gap table

| Requirement | Engine status | Gap |
| --- | --- | --- |
| Tone Machine mapping and engine slot | Done (stand-in, `NotModeled`) | Constants |
| Tone Machine readout parity | Missing | Measure against Kontakt |
| Formant / DC filter / Legato | Missing | Fields and laws |
| Tone Machine record layout | Repo evidence only | Saved-instrument check |

## Implementation plan (after READY)

1. Confirm READY; decode `ENGINE_PAR_WT_QUALITY`, the mode-1 record and the
   amplitude/other form laws from the Kontakt 8 C export by name lookup (no
   pasted pseudocode in the repo).
2. Add general cores in `sampler-core/src/source/`, selected by a config
   variant on the `Wavetable` IR (for example a quality/kernel selector).
3. Add the Tone Machine source as a source-mode config in `sampler-ir`, not a
   Kontakt-only path.
4. Add golden vectors for each newly measured law (one test each). Remove
   none of the existing tests.

No code was changed in this unit. The only blocker is the decompile.

## Decompile evidence (Kontakt 8 C export, READY)

Searched `Research/KONTRA/source/kontakt8/` by class and string names. Function
names are generic (`FUN_*`), so the render kernels still need tracing from the
factories. Confirmed so far:

- **Tone Machine is its own engine object, not the Time Machine 1 object.**
  `BToneMachine::vftable` is set in `FUN_140cd94c0` (`0140cd0000.c:4553`), which
  allocates a `0x420`-byte state (`FUN_140d20640`, `0140d20000.c:653`). Time
  Machine 1 uses `BTimeMachine` (`FUN_140cd9470`, `0x1d0` bytes). The
  "Tone shares TM1 scheduler" assumption in the stand-in is therefore unproven.
- The Tone state embeds two `BFilter2Pole` and one `BFilter2PoleHP` (ctor offsets
  `+0x1e`, `+0x2cc`, `+0x2e4`, 16 filter-state pairs). This is consistent with
  the "DC Filter" control being a 2-pole high-pass, but the coefficient law is
  not yet read.
- The shared machine parameter table (`FUN_140d1ba90`, `0140d10000.c:7343`)
  names the controls for the machine family: Speed, Smooth, Formant, Form 1,
  Phase, Ph Rand, Position, Inharm, Form 2, Mod Amt, Mod Tune, Grain, Slice
  Atk/Rel, Transient. "Ph Rand", "Inharm", "Mod Amt" and "Mod Tune" are the
  wavetable controls whose laws remain unrecovered.
- The DC Filter label and its switch (`01405f0000.c:7535`) are UI-side. The
  `Legato` label (`01405f0000.c:7625`) is also UI-side.

Not yet recovered: the Tone render kernel (grain or formant law), its grain
size and overlap, the DC filter coefficients, and the wavetable quality,
phase-randomness and inharmonic laws.


## Final-lane update (phase forms and remaining rows)

- Phase switch `0x140567aa0` (`0140560000.c:5389`) has these branches beyond
  the original-byte set: sync 1–3 and 24–26 (`fmod((15a^3+1)·p, 1)`), bend P/M/MP
  4–6 (cosine and arcsine laws with 1/π = `0x3ea2f983`), quantize 13
  (`trunc(2^((1-a)·15+1))` steps, round, minus 1e-5, clamp at 0), exp 28
  (`p^(9a+1)`), log 29 (`1-(1-p)^(9a+1)`), logexp 30 (exponent `18(0.5-a)+1` on
  the log side, `18(a-0.5)+1` on the exp side). Constants are read from the PE
  at their VAs (`0x14470bad0`=0.5, `0x14470bad4`=1.0, `0x144714a0c`=15,
  `0x1447149fc`=9, `0x144714a14`=18, `0x1447148a0`=1e-5, `0x144714998`=2.0 as
  double). Vectors: `tools/dsp/verify_wavetable_phase_libm.py` emulates the
  switch with libm helpers as Python stubs, so the numbers are helper-substituted.
- Amplitude forms 18 (Saturate), 19 (Fold), 20–23 (2/4/6/8 Blinds) and wrap 27
  return identity in this leaf; their consumer is not located. Not admitted.
- Modulation oscillator: callers of the phase switch in `0140cd0000.c` (e.g.
  `0140cd5af0`, 1497 bytes) run a phase accumulator with FM/PM terms and a
  2400-entry table read. This is the inharmonic/mod path. Not ported.
- Quality: `$NI_WT_QUALITY_*` values LOFI 0, MEDIUM 1, HIGH 2, BEST 3 (`crates/sampler-ksp/src/builtins.rs`, v1 port). The readout kernel for each level is not recovered.
- Phase randomness, live wavetable routes: no native law recovered yet.

## Corpus gap 10 (2026-10-22 survey)

- "Complete N-frame cycles, zone dropped" (32 instruments): `wavetable::Table::new`
  refused any span that was not a whole number of 2048-frame cycles, so the
  zone was dropped at prepare. It now floors to whole cycles and ignores the
  trailing partial cycle (v1 behaviour; `tests/wavetable.rs` oracle uses
  `len / CYCLE`). **Kontakt's partial-cycle rule is not recovered.** This is a
  labelled floor, not parity, and the matrix row stays partial.
- "Wavetable source" unsupported (20 instruments): the admission rejects phase
  random, inharmonic and the modulation oscillator. Their native laws are not
  recovered (see the decompile notes above). Not changed here.
