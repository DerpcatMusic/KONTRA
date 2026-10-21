# Kontakt 8 modulation envelopes: AHDSR, DBD, Flex

Evidence used: Kontakt 8 decompile (`source/kontakt8/`, READY), disassembly of the same
binary for argument order, constants read from the PE `.rdata` section, public NI manual
terms, and the recorded AHDSR control table in `docs/architecture-v2/KONTAKT_DSP_LAWS.md`.
Decompiled text was used only to confirm laws; the statements below are our own spec.

## Public documentation

NI modulation page returned HTTP 403. Terms from the search snippet only: AHDSR is an ADSR
with Hold and an "AHD Only" switch; DBD is a one-shot 0 → breakpoint → 0 envelope; Flex has
no shape limits. Curve, retrigger and release semantics are not confirmed by a public page.

## Verified laws

**Constants** (read from `Kontakt8.exe` `.rdata`, not from decompiled text):
`0.001f`, `0.5f`, `1/3f` (`0x3eaaaaab`), sign-bit mask (`0x80000000`), `1.0f`, `2.0f`,
`ln 2`, `8.922792434692383f`, `9.433565139770508f`.

**Time converters.** Two functions take a stored knob `x`, clamp it to `[0, 1]` and return
milliseconds: `exp(x·C + ln 2) − 2`, with `C = 8.9228` for attack/hold (`0x1407b85a0`) and
`C = 9.4336` for decay/release (`0x1407b85e0`). The engine's `u2n_env_a` / `u2n_env_d`
(`crates/sampler-core/src/voice_mod/laws.rs`) implement the same law. Check: at R = 1,500
these give `trunc(ms·0.001·R)` = 24/28, 256/332, 2415/3543, 22500/37500 at x = .25/.5/.75/1,
exactly the recorded control table (`envelope_time_matches_recorded_control_ticks`).

**AHDSR stored fields are unit knobs.** The v0x11 reader (`0x140cffb40`) reads, in order:
curve, attack, decay, hold, release, sustain, flag byte, then four 13-byte timing records.
That order matches the `ni-file` field order. Attack and hold go through the A law, decay and
release through the D law. Sustain is cubed: level `x³` (the stored state is `x³ + 0.075` with
a 0.075 floor, output `(state − 0.075)·scale + bias`). The engine's own stage scaling
(`EnvelopeState::scale_stage`) already treats the stage knob this way.

**DBD preparation** (`0x140ae5d70`, called from resolver `0x1409651b0` mode 2, disasm
around `0x1409652c4`): arguments are `(this, N0_ms, level, N1_ms)` with
`N_i = trunc(ms_i·0.001·rate + 0.5)`. The first segment slope is `(level − 1/3)/N0`, the
second is `(−level)/N1`. A stored zero flag (byte at +0xec) forces level and both durations to
zero. Stored record: float f0 = level (raw, used directly); f1 and f2 = durations through the
time converter of their sub-object (vtable slot `+0x28`); flag byte. The engine's
`Breakpoints::dbd` implements this law (counts, slopes, zero flag, refuses the unproved
`(N0 > 0, N1 = 0)` shape).

## Gap table

| Item | Shared engine | Kontakt 8 | Status |
| --- | --- | --- | --- |
| AHDSR time converter (attack/hold A, decay/release D) | `voice_mod::laws::u2n_env_{a,d}` with tick test | verified (decompile + recorded table) | **covered** |
| AHDSR stored knob → time and sustain (lowering) | `sampler-kontakt` `ahdsr_*` helpers (this PR; was raw ms / linear sustain) | verified | **fixed in this PR** |
| AHDSR stage laws (curve, floor, release restart) | `v1_voice_controls::Ahdsr` (native f32 path) | verified (lifecycle vectors) | covered, not rendered against Kontakt |
| AHDSR control rate R (cadence in ticks) | sample-rate frames | not proved | **open** |
| AHDSR note-off scheduling, voice reset, one-shot flag | partial (`one_shot`, `ahd_only`) | not recovered in this pass | open |
| DBD preparation law | `Breakpoints::dbd` | verified (disasm + constants) | **covered** |
| DBD time converter identity (which of A/D per field) | none | vtable slots not identified | **open** |
| DBD IR mapping (`InternalSource::Other 0x41`) | none, diagnosed | record layout known | **open** (needs converter identity) |
| DBD flag byte beyond zero | none | not recovered | open |
| Flex point/curve/release | `Breakpoints` + `flex_release_frames` | partly verified earlier | partial |
| Flex loop jump at sustain row | `loop_range` | `docs/re/kontakt-flex-loop-jump.md` | partial |
| Retrigger and legato | `Segment::retrigger` | not verified | open |
| Output smoothing | none separate | not verified | open |

## Evidence confidence

- AHDSR time converter and tick table: high (decompile, PE constants, recorded native table).
- AHDSR sustain cube and stored-knob interpretation: high for the decompiled reader and engine
  convention; end-to-end render against Kontakt not run.
- DBD preparation: high for the law; medium for the argument mapping (from disassembly).
- Open rows: nothing established beyond the table.
