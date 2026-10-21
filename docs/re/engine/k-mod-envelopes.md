# Kontakt 8 modulation envelopes: AHDSR, DBD, Flex

Status: **draft, waiting on the Kontakt 8 decompile.** `source/kontakt8/` was empty and had no `READY` marker when this unit started. Everything below comes from repo evidence, public docs and the current engine. Nothing here is taken from decompiled text. The DBD wiring and the AHDSR control rate are the open items that need `READY`.

Scope: envelope curves, retrigger, release, time laws, smoothing. Not in scope here: LFO, follower, glide, external performance modulators, and the envelope *routing* into targets.

## Public documentation (NI Kontakt manual, KSP reference)

The NI modulation page returned HTTP 403 to WebFetch, so this comes from the search snippet and the cached manual index only:

- **AHDSR**: ADSR plus a Hold stage. "AHD Only" limits it to attack, hold, decay. Per-stage time controls, attack curve.
- **DBD**: one-shot, zero → adjustable breakpoint (above or below zero) → zero. Two decay counts, not a sustain.
- **Flex**: no shape limits, points in time, levels and curves, times from milliseconds to minutes.
- Curve, retrigger and release behaviour is **not** confirmed by a public Kontakt page reached here. Do not write those laws from the HISE docs; HISE is a different product.

## Repo evidence used

- `docs/architecture-v2/KONTAKT_DSP_LAWS.md` (AHDSR section, lines ~61–136): verified control conversion, control kernel, attack curve `b`/ratio law, stage lifecycle, release countdown. Control rate `R` is explicitly **not** proved to be sample rate / 32.
- `docs/re/kontakt-flex-loop-jump.md`: Flex release/loop jump at the sustain row.
- `docs/re/kontakt-catalog/parser_coverage.md`, `ser_classes.tsv`: class ids and versions. AHDSR `0x3F` v `0x10–0x11`; FM7/Flex `0x40` v `0x10–0x12`; DBD `0x41` v `0x10–0x12`.
- `docs/re/NATIVE_ORACLE.md` line 87: DBD `0x41` listed as "never decoded".
- `vendor/ni-file/src/kontakt/objects/native_modulators.rs` `EnvelopeDbd`: versions 0x10 (`FFFFF`), 0x11 (`FFFFFB`), 0x12 (`FFF` + two packed records + `B`). Field names unknown; values stored raw.
- `crates/sampler-kontakt/src/library.rs` ~1880: DBD, glide and follower are **diagnosed as unsupported** (`Reason::UnknownLaw`), with the open items written inline.

## Modules

### AHDSR (`BParEnv_AHDSR`, 0x3F)

- Params: attack_ms, hold_ms, decay_ms, sustain (0..1), release_ms, attack_curve (float), `ahd_only` (flag), unknown tail (52 bytes in the common case).
- Engine: `Ahdsr` in `crates/sampler-core/src/v1_voice_controls.rs` (native f32 geometric path, `native_amplitude`). `ahdsr_coefficients` / `ahdsr_curves` in `library.rs:1088–1093`.
- Verified laws (KONTAKT_DSP_LAWS.md): time `x` → ms via `exp(x·8.9228 + ln2) − 2`; decay/release multiplier base 9.4336; sustain storage `x³ + 0.075`; attack curve `b`, ratio, floor `0.075`; release restarts from `1.075` geometric state; zero release skips to inactive; release countdown is N ticks exactly.
- Edge cases checked: zero-length stages skipped; whole-block vs split-block identical.

### DBD (`BParEnv_DBD`, 0x41)

- Params: three floats in all versions, plus v0x10/0x11 two more floats, or v0x12 two packed records; optional flag byte.
- Engine block exists: `Breakpoints::dbd` (`crates/sampler-core/src/blocks/envelope.rs:748`): linear ramp 1/3 → level over N0 frames, then to 0 over N1, counts `trunc(ms·0.001·rate + 0.5)`. Tests at `envelope.rs:1315–1333`.
- **Not wired.** `modulation_objects.rs:64` maps DBD to `InternalSource::Other { chunk_id: 0x41 }`, and `library.rs:~1880` diagnoses it.

### Flex (`BEnvelopeFM7`, 0x40)

- Engine: `Breakpoints` + `flex_ratio` / `flex_curve` (closed form of the setter recurrence), `flex_release_frames`, loop range (sustain-row jump). `library.rs:6326–6345` `flex_breakpoints`.
- Native curve constants at `flex_ratio` are proved only for the stated branches; `c` clamping is **not proved**.

## Gap table

| Item | Shared engine has | Kontakt 8 needs | Status |
|---|---|---|---|
| AHDSR stage laws, lifecycle, release countdown | Yes (`v1_voice_controls::Ahdsr`, verified against v1 oracle) | — | Covered for tested lengths |
| AHDSR time base (control rate R) | Uses sample-rate frames | Prove R from `0x140ae46d0`/`0x140ae40f0` | **Open** (decompile) |
| AHDSR note-off scheduling / voice reset / one-shot flags | Partial (`one_shot`, `ahd_only`) | Scheduling and reset in the voice path | **Open** |
| AHDSR curve-only update invalidation | Not modelled | Setter path | **Open** |
| DBD breakpoint law (1/3 start, linear, counts) | `Breakpoints::dbd` with tests | Verify start 1/3 and count rounding | Engine exists; law from docs/RE pending |
| DBD IR mapping + saved-duration resolver (helper +0x28, `0x1409651b0`) | None | Unit law of saved durations; v0x10/0x11 legacy tail | **Blocked on decompile** |
| DBD flag byte semantics | None | Meaning of trailing flag | **Open** |
| Flex point/curve/release | Yes, with the release law | Loop jump flag `+0x194` only partly modelled | Mostly covered |
| Flex clamp of curve `c` | Native branch order | Confirm `c` range | Open |
| Retrigger (AHDSR and Flex) | `Segment::retrigger` restart from current level | Kontakt retrigger/legato rule | **Open** (not in verified docs) |
| Smoothing of envelope output | Not modelled separately | Any output smoothing in the stage kernel | **Open** |

## Plan once READY exists

1. `rg -n "0x140ae46d0|0x140ae40f0|0x140ae5820|0x140ae5e40|0x1409651b0|0x140ae43d0|0x140ae5990|0x140ae5d70" /data/dev/workspace/Research/KONTRA/source/kontakt8` and read only those spans.
2. Recover R, DBD resolver unit law, DBD flag, retrigger/legato rule and output smoothing from the decompile; record them here as our own spec.
3. Wire DBD: map `Modulator::Dbd` into a `ir::InternalSource` variant, lower to `Breakpoints::dbd`, remove the `UnknownLaw` diagnostic only for proven fields.
4. Add one golden test for the DBD breakpoint sequence and one for AHDSR R-dependent timing.

## Evidence confidence

- AHDSR stage laws: high (verified in repo, not re-run here).
- DBD engine block: medium (tests exist, not connected to any real file).
- Everything marked Open: none established here.

## Status update: measurement and decompile access (2026-10-21, later)

- **Wine oracle:** `tools/kontakt-oracle/` has no envelope or modulator probe. Its Lua usage covers instrument, group, zone, sample-loop and effect parameters (`set_engine_par` for effect ENGINE_PARs). Nothing confirms that envelope/modulator parameters (AHDSR times, DBD counts, Flex points) are reachable through `set_engine_par` or the Kontakt 8 Lua API. Building the DBD/AHDSR time-law measurement therefore needs a new probe whose access path is verified first, then a realtime render through `render.py`.
- **Decompile:** `source/kontakt8/` still holds no files; `source/ghidra/analyze-Kontakt8.log` is being written, with no export yet. The DBD saved-duration resolver (`0x1409651b0`) and the AHDSR control rate remain unrecovered.
- **Result:** no envelope law was added in code. Anything implemented now would be a guessed constant, which the brief forbids.
