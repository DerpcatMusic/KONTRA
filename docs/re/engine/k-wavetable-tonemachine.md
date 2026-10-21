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
- Table must be complete 2048-frame cycles; otherwise `Table::new` returns `None` and playback is refused.
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

### Current engine behaviour

- Kontakt mode 1 is **not** mapped. `library.rs` falls through to the generic
  arm and reports `"Tone Machine source (played as a sampler)"` with mode 1.
  Audio is therefore wrong (plain sampler playback, no pitch/formant engine).
- `SOURCE_MODE_TM1` is 2 (Time Machine 1). Mode 1 does not share Time Machine's
  stretch path and must not be routed through `StretchKind::TimeMachine1`.
- `docs/architecture-v2/DSP_COVERAGE.md` records Tone Machine as `missing`,
  `unmeasured`, zero corpus instances.
- No `Tone`-named engine core exists for this mode. The existing `Tone`
  matches are the post-chain tone filter and colour stages, which are unrelated.

### Parameters (public documentation, unverified)

Secondary source (Elephorm course, French; not NI): Smooth, Speed, Formant,
DC Filter, Legato. Described as granular pitch/formant change independent of
playback speed. NI's manual (Kontakt 5 §20.3 excerpt) says it adjusts speed and
pitch of samples independently. **Names, ranges, defaults and the saved field
layout for mode 1 are not yet recorded in the repo** and must come from the
Kontakt 8 decompile and a saved Tone Machine instrument. Do not guess.

| Field | Saved location | Law / units | Status |
| --- | --- | --- | --- |
| Speed, Pitch | — | — | unknown |
| Formant, DC filter, Legato, Smooth | — | — | unknown |

### Gap table

| Requirement | Engine status | Gap | Owner step |
| --- | --- | --- | --- |
| Tone Machine readout (granular or otherwise) | Missing | Whole algorithm | Decompile + measure |
| Tone Machine saved-field decode | Missing | Mode 1 record layout | Decompile |
| Tone Machine diagnostic | Present (played-as-sampler report) | Keep until DSP exists | Done |
| Wavetable quality parameter | Silent ignore | Read and diagnose or implement | Decompile |
| Wavetable amplitude forms | Missing | Laws | Decompile |
| Wavetable blinds/fold/exp/log/sync forms | Missing | Laws | Decompile |
| Wavetable phase randomness | Rejected + diagnosed | Generator law | Decompile |
| Wavetable inharmonic / modulation oscillator | Rejected + diagnosed | FM/PM/RM and clocks | Decompile |
| Live `wavetableModFrequency/Amount/Inharmonic` routes | Missing | No target | Decompile |
| Anti-aliasing / readout kernel | Unverified | Kernel and sample-rate law | Decompile + measure |

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
