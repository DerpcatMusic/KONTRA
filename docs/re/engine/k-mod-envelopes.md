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

**AHDSR stored times are milliseconds (correction 2026-10-23).** The v0x11 reader
(`0x140cffb40`) stores the raw f32 fields with no law: curve, attack, decay, hold, release,
sustain, flag byte, then four 13-byte timing records (same order as `ni-file`). The
time converters above clamp to `[0, 1]`, so applying them to a stored ms value (80 ms → 15 s)
was the Dolce staccato silence bug. Corpus evidence across Kontakt 168/171/174/177 files
(Dolce, Chorus, Pacific, Vista, Una Corda, ANALOG STRINGS, Afflatus): stored values are clean
musical milliseconds (125.01, 80.01, 1050.0, 25000.04 = law max) and sustain is the only knob.
The converters apply to script and automation values, not to these fields. Sustain is cubed:
level `x³` (stored state `x³ + 0.075`, output `(state − 0.075)·scale + bias`).

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
| AHDSR stored time → ms and sustain (lowering) | `sampler-kontakt` `ahdsr_*` helpers: times pass through as ms (PR #476), sustain cubed | verified (corpus survey, 7 libraries) | covered |
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
- AHDSR sustain cube: high for the decompiled reader and engine convention; end-to-end render
  against Kontakt not run. Stored times are ms: high (raw reader + corpus values).
- DBD preparation: high for the law; medium for the argument mapping (from disassembly).
- Open rows: nothing established beyond the table.

## Final pass (2026-10-22): decompile results per row

**DBD (done for v0x12).** The live preparation `0x140ae4b50` converts both durations with the
decay law (`0x144e79a98`, the D converter), so the DBD converter is D (identity open in the
earlier pass is closed). The static preparation `0x140ae5d70` receives the converted
durations (disasm `0x1409652c4`). Lowering: `dbd_breakpoints` in `sampler-kontakt/src/library.rs`
builds the three-point ramp (1/3 at once, to `level` over `u2n_env_d(values[0])`, to 0 over
`u2n_env_d(values[2])`); the flag zeroes it; refused: v0x10/0x11 (legacy tails unproven) and
`first > 0, second = 0`. Frame rounding is the lowering's f64 `round(ms·rate/1000)`, which
matches `trunc(x + 0.5)` except at exact f32 boundaries. Parity needs a native vector; none
exists (`NATIVE_ORACLE.md` lists DBD as never decoded).

**AHDSR (laws proved, rate owner-side).**
- Retrigger: `0x140ae5820` restarts at the first nonzero stage's start (attack output starts at
  0, not the current level). Release captures the current affine output (`0x140ae5e40`). A new
  note builds a fresh instance (`0x140604cd0`, `0x140ae45f0`). The engine's native path models a
  fresh voice per note, which matches.
- Output smoothing: none. The kernel `0x140ae40f0` computes `(state − 0.075)·scale + bias` per
  sample and then `state *= multiplier[stage]`.
- Note-off: the kernel releases from sustain (stage 3) when `+0xb3` is set and the envelope is
  not AHD-only (`+0x118`). `+0xb3` is set by the owner's event scan (`0x14097be30`, release event
  inside the block). Event timing belongs to the owner, not the envelope.
- Control rate R: kernel counts are `trunc(ms·0.001·R)` with R stored in the object (`+0x14`).
  The writers of `+0x14` (`0x140ac3720`, `0x140ac39f0`) are owner objects, so the envelope's R is
  not identified. The engine's `Fs/32` remains an assumption to verify when the owner is traced.

**Flexible envelope (retrigger proved, no vector).** `0x14094ce70` restarts the state at the
**last** point's level, resets the carry and sets the segment index to −1 before the first
advance (`0x140ae5450`). `flex_breakpoints` already encodes this with a leading point at the
last level, so the engine matches. Smoothing: none; the segment is per sample (`0x140ae44e0`).
The segment count law `0x140ae5450` (`trunc(time·R + 0.5)` with carry) depends on the same
unidentified R.

Blocking for parity on all three rows: native-data vectors do not exist for these modules
(the Wine oracle has no envelope probe), and R is owned by the caller.
