# Engine unit: dyn-limiters-transient (Kontakt 8)

Scope: Kontakt Limiter (SerType 0x1c, `BParFXLimiter`), Kontakt Transparent
Limiter (SerType 0x5f, `BParFXHilbertLimiter`), Kontakt Transient Master
(SerType 0x43, `BParFXTrans`, v0x10) and Gainer (0x13, `BEffectGainer`).

Evidence: the Kontakt 8 decompile export (`source/kontakt8/`, READY) for the
functions named below, and float constants read from the Kontakt 8.13.1 image at
their data addresses. Laws are written here as our own spec; no decompiled text
is copied. Repo evidence (`docs/re/kontakt-bindings/limiter-dynamics.tsv`,
`docs/re/DSP_SYSTEM_INVENTORY.md`) is cited where it still holds.

Engine locations:
- Dynamics: `crates/sampler-ir/src/fx/dynamics.rs` (config),
  `crates/sampler-core/src/dsp/dynamics/` (`laws.rs` cores, `computer.rs`,
  `detector.rs`, `ballistics.rs`, `pipeline.rs`).
- Color (Gainer): `crates/sampler-ir/src/fx/color.rs`,
  `crates/sampler-core/src/dsp/color/mod.rs`, `crates/sampler-core/src/dsp/kernels.rs`.
- Kontakt mapping: `crates/sampler-kontakt/src/effects/dynamics.rs`,
  `crates/sampler-kontakt/src/effects/mod.rs` (dispatch ~390-470, Gainer ~1318).

## Kontakt Limiter (0x1c)

Process `0x140904890`, setter `0x140960140`, prepare `0x1408dd0a0`
(shared with the same object layout).

| Item | Law (verified) | Engine state |
|---|---|---|
| Input gain | setter stores the dB value x1 and applies `2^(x1/6)` (constant 1/6 at `0x144737728`, base 2 at `0x144714994`) | covered: `Limiter::new`, lane `ENGINE_PAR_LIM_IN_GAIN` (`-24 + 48u` dB is the script-side mapping) |
| Release | setter takes the release in ms, `N = max(release·1e-3·rate, 1)` frames; the falling level moves by `ln2/N` per frame (`0x144f38800` = ln2) toward the new peak; rises are instantaneous | exact: `PeakRelease`, `frames = max(s·rate, 1)`, coefficient `ln2/frames` |
| Release table (script) | 10…1000 ms table at u = 0..1 step 0.1 (repo evidence) | covered: `tables::LIM_RELEASE` |
| Detector | channel max of |x|, delayed 32 frames (`KONTRA_LATENCY`), then a biquad low-pass: RBJ, cutoff `rate/32`, Q = 1/(2·0.91) (`0x144f38830` = 0.91, `0x144f38688` = 1/32). Trig is a polynomial approximation of sin/cos; the same design as `rbj_low_pass` | covered: `LowPass2` with `rbj_low_pass(TAU/32)` |
| Gain computer | with y the detector output and c = min(y, 2): gain = 1 / (y − (1 − c/4)·c + 1), i.e. `1/(1+y²/4)` for y ≤ 2 and `1/y` beyond (constant 0.25 at `0x1447148e4`) | covered: `SoftClip` in `computer.rs` |
| Output | gain·input clamped to [−1, 1]: lower −1 (`0x144714a54`), upper 1.0 (the gain formula K, `0x14470bad4`) | covered: `LimiterOut`; traced 2026-10-22 |
| Low-level offset | the earlier note of −0.12 dB at low levels is not in the law (DC gain of the detector is 1). The Wine sweep (`lim-ramp3` run, not clean) matched the curve within ~0.1 dB | closed as measurement artefact |

## Kontakt Transparent Limiter (0x5f)

Core `NI::DSP::HilbertLimiter`. Process `0x141ae3400`, control setter
`0x141aea680`, effect-level control clamp `0x1409aa9e0` (clamps to 0..1, writes
core control via vtable slot 0x60, reads back slot 0x68 for the flag).

| Control | Law (verified) | Saved field (inferred order) | Engine |
|---|---|---|---|
| 0 | `gain = 10^(((1 − x)·40) · 0.05)`: drive 0…40 dB, applied to the input | drive (x0) | covered: `Hilbert::new` drive |
| 1 | release time `max(x, 0.001)` s, one-pole coefficient `1 − 1/(τ·rate)` | release (x1, s) | covered: `Hilbert::release` |
| 2 | ceiling `−0.35 − (1 − x)·39.7` dB | ceiling (x2) | covered: `Hilbert::ceiling` |
| 3 | boolean `x > 0.5`, stored at core +0x164. Selects the side-chain path: detector reads the second input pair (`param_2 + 0x10`) and the gain-mod branch uses 1.0 | side-chain byte | **not modelled**. The class `BEffectHilbertLimiterSC` exists, so side-chain is its own effect with an external key input. Diagnosed (`NotModeled`) |

Core: log-domain detector (dB = 20·log10), Hilbert allpass pair
(`0x141aa4400`, prototype coefficients and warp as in our `PROTOTYPE`/`WARP`),
dB ballistics with rise/fall/smooth coefficients from prepare, look-ahead
`128·max(1, rate/44100)`.

Traced (2026-10-22):
- Reader: vtable slot 8 of `BParFXHilbertLimiter` (vtable `0x144e751e8`, found from the constructor's store at `+0x2e0`). Version must be 0x51. Reads floats at +8, +0xc, +0x10 then one byte at +0x14. Saved order: drive, release, ceiling, side-chain. Matches `effects/dynamics.rs::hilbert`.
- Side-chain: the reader's byte selects `BEffectHilbertLimiterSC` (separate class). Core now supports the key input: the Hilbert detector reads a key buffer through the same drive (`Key::External`, `DynamicsSettings::process_keyed`). Validation rejects `Key::External` as unrouted (`ValidationError::Unrouted`) until a slot can supply a key bus. The Kontakt mapping still diagnoses the flag.
- Side-chain apply (vtable slot 7, `0x140d19bb0`): when the flag is set it clears a byte at `lookup(...) + 0x12064`. Meaning not traced.
- Not modelled: the side-chain branch in the process, which uses 1.0 for a gain term when the flag is set (`0x141ae3400`, `local_fc` branch on core +0x164).
- Dynamics ballistics constants (rise/fall/smooth) are taken from the existing core and not re-derived sample by sample.

## Kontakt Transient Master (0x43, v0x10)

Core `NI::DSP::Transient` (setter `0x141ac87b0`, process variants `0x141a4a9d0`
and output-clip `0x141a59d40`, prepare `0x141aa27a0`).

| Control | Law (verified) | Engine |
|---|---|---|
| 0 attack | `2 · (x − 0.5) · 120` (constant `0x144f38cd0` = 120) | covered: `Transient::new` `a + a` |
| 1 sustain | `2 · (x − 0.5) · 115` (`0x144fcfe3c` = 115) | covered |
| 2 gain | `(x − 0.5) · 12` dB for x ≥ 0.5, `· 48` dB for x < 0.5 (`0x14470baf8` = 12, `0x144f38c9c` = 48) | covered |
| 3…7 | five booleans `x > 0.5`, stored at +0x68…+0x6c | flag 3 carries the saved clip switch (inferred); 4…7 have no saved field and stay off |

Output clip is the separate process variant `0x141a59d40`: a second-order
peak tracker on the output, selected by the clip flag. The flag-to-variant
selector was not located; the existing `clip` bool drives it.

Traced (2026-10-22):
- Saved order (reader, vtable slot 8 of `BParFXTrans`, version 0x10 only): attack, sustain, gain (three floats), then the clip switch as an int that must be 0 or 1. Matches `effects/dynamics.rs::transient`.
- Controls are live buffers (`0x450` … `0x480`, one per control, clamped 0..=1 per block). Controls 3..7 are booleans (`x > 0.5`) at core +0x68..+0x6c.
- Process variant: `0x141aaac90` selects one of 64 variants from a 6-bit mask: bit5 = ctrl 3 (+0x68), bit2 = ctrl 4 (+0x69), bit3 = ctrl 5 (+0x6a), bit1 = ctrl 7 (+0x6b), bit4 = ctrl 6 (+0x6c), bit0 = +0x261. Variant 0 is `0x141a4a9d0`.
- Clip = ctrl 3: variant 32 (only ctrl 3 set) is `0x141a59d40`, the output-clip process (second-order peak tracker plus a log-table curve). So the saved clip switch maps to control 3 as implemented.
- Ctrl 4..7 and bit0 are live-only switches with no saved field. A loaded instrument has them off, so variant 0 runs. Their variant semantics are not implemented; automating them is unsupported.
- Constants traced again: attack ×120, sustain ×115, gain ×12/×48 (table `0x144f38cd0`, `0x144fcfe3c`, `0x14470baf8`, `0x144f38c9c`).

## Gainer (0x13)

Process `0x140aa2980` (per channel, over the block), mix helper `0x140ac21f0`.

| Item | Law (verified) | Engine |
|---|---|---|
| Smoother | `g ← g + (target − g) · k`, `k = 1/1800` (`0x144f385f8`, bits `0x3a11a2b4`); output uses g before the step | covered: `kernels::one_pole32`, `GAIN_K` |
| Unity window | if both the state and the target are in `[0.999, 1.001]` (`0x144756360`/`0x144756368`, double precision), the process skips the slew and the mix helper reads the input directly, so the output is the input and the state is not advanced | implemented: `kernels::gainer` returns `(1, current)` inside the window; test `gainer_inside_the_unity_window_passes_the_input_and_holds_the_slew` |
| Mix | helper adds `fVar(0x1a4) · source` into the output; source is the input in the window and the gained signal otherwise | covered for dry 0 (corpus) and the stored-dry path (`0.5 + 0.5g`, test in `effects/mod.rs`). The dry ≠ 0 case inside the window is unverified |
| Gain-copy (`0x1408374b0`) | copies the target (0x1c0) from the source; when the source byte 0x1a2 is set and the destination byte 0x1b7 is clear, the destination's current gain (0x1c4) snaps to the target. Otherwise the smoother keeps its state | not modelled: the engine does not copy stage objects. The snap is a no-op for our state (`gain_ready` covers the first block). Documented, not implemented |
| Dry path | the output goes through the common effect mix (`0x140ac28f0`: target 0x1a8, current 0x1a4, bypass 0x1b5) before the helper adds the source | covered by the common `Mix` path; the window shortcut (above) applies to the gain only |

## Oracle notes (2026-10-21)

- `tools/kontakt-oracle/eqrender.py` sent every set value as an integer (KSP `%cval`), truncating values in (0, 1) to 0. Fixed to a real array (`~cval`). Specs with fractional values from earlier runs are suspect.
- Shared Kontakt session: concurrent runs corrupt each other's renders. The one clean sweep was used for the limiter check above.

## Gap summary (after 2026-10-22 traces)

1. Transparent Limiter side-chain: core key input done (`Key::External`); the slot routing that supplies a key bus is not. The Kontakt flag stays diagnosed. The side-chain gain branch (core +0x164) is not modelled.
2. Transient controls 4..7 and bit0 (+0x261): live-only variant selectors, 64 variants. Only variant 0 (and the clip variant 32) are implemented. Automating the others is unsupported.
3. Gainer gain-copy: the snap-on-copy has no effect on our engine because stage objects are not copied. Documented.
4. Gainer dry ≠ 0 inside the unity window: unverified.

Closed: Limiter output clamp (lower −1, upper 1.0: the gain formula uses K = 1.0 at process line 623, and the output clamps to ±1); Transparent saved order and version gate 0x51; Transient saved order and clip = control 3.

## Evidence confidence

- High: Limiter (all rows), Transparent controls 0–2, Transient constants, Gainer smoother and unity hold (process and constants read from the image).
- Medium: saved-field orders and the Transient clip-flag mapping (inferred from the reader layout).
- Not verified: side-chain behaviour (not implemented), flags 4–7, gain-copy flags.

## Open blocker, Transient variant dispatch (2026-10-22)

The 6-bit mask is rebuilt in `FUN_141aaac90` (`0141aa0000.c:6084`, export) as
bit0 = `+0x261`, bit1 = ctrl 7 (`+0x6b`), bit2 = ctrl 4 (`+0x69`), bit3 = ctrl 5
(`+0x6a`), bit4 = ctrl 6 (`+0x6c`), bit5 = ctrl 3 (`+0x68`). The mask then
indexes an indirect jump table at `LAB_141aaaffc`. Ghidra did not recover that
table in text (`Could not recover jumptable`), so the mask-to-function map for
the other 62 variants is unknown from the export. Reading the table bytes needs
a binary read, which was not done in this pass. Controls 4–7 and bit0 therefore
stay unmapped. Variant 0 (`0x141a4a9d0`) and the clip variant (`0x141a59d40`)
are the only entries tied to a known function.
