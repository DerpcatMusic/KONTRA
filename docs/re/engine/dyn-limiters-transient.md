# Engine unit: dyn-limiters-transient (Kontakt 8)

Scope: Kontakt Limiter (SerType 0x1c, `BParFXLimiter`), Kontakt Transparent
Limiter (SerType 0x5f, `BParFXHilbertLimiter`), Kontakt Transient Master
(SerType 0x43, `BParFXTrans`, v0x10 only) and Gainer (0x13, `BEffectGainer`).

Status: **Kontakt 8 decompile not yet exported** (`source/kontakt8/READY`
absent; the directory is empty). Everything below is taken from evidence already
in the repo (`docs/re/kontakt-bindings/limiter-dynamics.tsv`,
`docs/re/DSP_SYSTEM_INVENTORY.md`, the recovered-core comments in
`crates/sampler-core/src/dsp/dynamics/` and `crates/sampler-core/src/dsp/color/`).
Rows marked **pending READY** must be re-checked against the Kontakt 8 export
before they are called parity.

Shared engine locations:
- Dynamics family: `crates/sampler-ir/src/fx/dynamics.rs` (config),
  `crates/sampler-core/src/dsp/dynamics/` (`laws.rs` cores, `computer.rs`,
  `detector.rs`, `ballistics.rs`, `pipeline.rs`).
- Color family (Gainer): `crates/sampler-ir/src/fx/color.rs`,
  `crates/sampler-core/src/dsp/color/mod.rs`.
- Kontakt mapping: `crates/sampler-kontakt/src/effects/dynamics.rs`,
  `crates/sampler-kontakt/src/effects/mod.rs` (dispatch at ~390-470, Gainer ~1318).

## Kontakt Limiter (0x1c)

| Item | Law / value | Engine state |
|---|---|---|
| `LIM_IN_GAIN` (stored field 0, slot 8) | `-24 + 48u` dB; core gain `2^(x/6)` | covered: `Limiter::new`, lane `ENGINE_PAR_LIM_IN_GAIN` |
| `LIM_RELEASE` (stored field 1, slot 12) | ms table at u = 0..1 step 0.1: 10, 16.68, 27.07, 43.26, 68.45, 107.65, 168.68, 263.68, 411.55, 641.72, 1000 | covered: `tables::LIM_RELEASE`, lane `ENGINE_PAR_LIM_RELEASE`; stored value taken as ms (`effects/dynamics.rs::limiter`) |
| `LIM_THRESHOLD`, `LIM_CEILING` | no stored field of this class | nothing to map |
| Static curve | `y / (1 + y^2/4)` for y <= 2, held at 1.0 above | covered: `computer.rs` (Kontakt Limiter), `SoftClip` stage |
| Look-ahead | 32 frames at 48 kHz, output onset 32 samples after dry | covered: `KONTAKT_LATENCY` = 32 |
| Detector | channel max, peak-hold follower with release, RBJ low-pass fixed at rate/32 | covered: `ChannelMax`, `PeakRelease`, `LowPass2` |
| Release shape | recovery follows exponential decay, half-life ~ release time | approximate: fits within +-30 % (repo evidence). **Gap** |
| Low-level offset | earlier note: -0.11..-0.13 dB at y < 0.12. Re-check, Wine sweep `lim-ramp` (200 Hz ramp, u=0.5, R-channel reference): median gain -0.04 dB in [-20,-12) dBFS, -0.26 dB in [-12,-6), -1.15 dB in [-6,-1); the modelled curve gives -0.03, -0.26, -0.98 at the same medians. Offset not reproduced beyond about 0.1 dB; still open pending a clean repeat |

## Kontakt Transparent Limiter (0x5f)

Core `NI::DSP::HilbertLimiter`: Hilbert allpass pair (7-stage chains, prototype
coefficients from `0x1479ab7f0`), gain smoother, ceiling in dB.

| Item | Engine state |
|---|---|
| Core DSP (Hilbert pair, release, ceiling, gain smoother) | covered: `laws.rs` `Hilbert`, `detector.rs` (Hilbert), tests in `dynamics/tests.rs` (`Native NI::DSP::HilbertLimiter` section) |
| Saved drive / release / ceiling -> controls 0..2 | **inferred, not traced**; mapping in `effects/dynamics.rs::hilbert` (drive -> threshold field, ceiling -> ratio field, release in seconds) |
| Release unit | the saved release is documented "(s)" but the mapping treats the normalized value as seconds with no measurement. **Gap** |
| Side-chain byte | **Gap**: `Reason::NotModeled`, rejected at load (flag meaning unassigned) |
| Save-order to control-id | **Gap**: inferred from the four-control layout; pending READY (`BEffectHilbertLimiter`, setter `0x141aea680`) |

## Kontakt Transient Master (0x43, v0x10)

Core `Transient`: high-pass 10 Hz detector, six envelope followers (`k[0..9]`),
transient mix, DC block.

| Item | Engine state |
|---|---|
| Attack / sustain mix | covered: `computer.rs` `attack * A + sustain * (S - 0.33 A) + gain`, `laws.rs` `Transient::new` |
| Gain: `(g - 0.5) * 12` dB above 0.5, `* 48` dB below | covered from the recovered core comments; **pending READY** for measurement |
| Output clip switch | covered: `clip` flag in `Law::KontaktTransient` (process `0x141a59d40`) |
| Controls 0..3 order | **inferred, not traced**; pending READY (setter `0x141ac87b0`) |
| Controls 4..7 | no saved field; kept off. Confirm no other live binding (pending READY) |
| Other saved versions (not 0x10) | **Gap**: only v0x10 is accepted (`effects/mod.rs` dispatch); other versions are unsupported, not guessed |

## Gainer (0x13)

Shared color family; `output = input * (dry + g)`, `g` slews toward target.

| Item | Engine state |
|---|---|
| Stored gain (linear float) | covered: `Params::Gainer`, `color::gainer`, lane `ENGINE_PAR_GN_GAIN` |
| Per-frame smoothing `g += (target - g) * F(1/1800)` (bits `0x3a11a2b4`), state read before advance | covered: `kernels::gainer`, `color/mod.rs` (`GAIN_K`), tests `color/tests.rs` (`gainer_matches_the_frozen_slew_equation`, `gainer_slews_toward_a_moving_target_with_the_native_coefficient`) |
| Dry mix: fresh module reads `0.5 + 0.5 g` | covered for the stored-dry-level path; test `gainer_mixes_its_stored_dry_level_with_the_gained_signal` (`effects/mod.rs`). Corpus only has dry 0 |
| Unity shortcut when current and target both in [0.999, 1.001] | **Gap**: the output-side helper was stubbed in the earlier recovery; behaviour not modelled |
| Gain-copy flags (source byte 0x1a2, destination byte 0x1b7) | **Gap**: lifecycle meaning unassigned; not used by the engine |

## Oracle notes (2026-10-21)

- `tools/kontakt-oracle/eqrender.py` sent every set value as an integer (KSP `%cval`), so any value in (0, 1) became 0. Fixed to a real array (`~cval`). Every earlier spec with fractional values should be treated as suspect until re-run.
- Wine sweeps share one Kontakt session and one PipeWire sink. Under contention (other units running) the R reference and some steps came back inconsistent, so only the single clean run above is used. Repeat when the oracle is free.
- Measured: Gainer unity shortcut is implemented from the inventory description only; not yet checked with the oracle.

## Gap summary

1. Limiter: release fit tolerance (+-30 %); low-level offset to re-check on a clean oracle run. Process `0x140904890` for the exact release law.
2. Transparent Limiter: side-chain byte, control-id order, release unit. Needs `BEffectHilbertLimiter` and setter `0x141aea680`.
3. Transient Master: control-id order, controls 4..7, non-v0x10 versions. Needs setter `0x141ac87b0`.
4. Gainer: unity shortcut is now implemented (pass-through when both gain ends are in [0.999, 1.001]); the dry != 0 case inside the window is unverified. Gain-copy flags still need `0x1408374b0`.

Code changed: Gainer unity-window pass-through (`kernels::gainer`, test in `color/tests.rs`).
The remaining items are unmeasured or need the Kontakt 8 export; none is guessed.

## Evidence confidence

- High: stored-field layouts and the Limiter/Gainer ballistics already measured in
  `limiter-dynamics.tsv` and the inventory (PROVED rows).
- Medium: Transparent Limiter and Transient Master mapping (inferred layout).
- Pending READY: all decompiled-function claims that are not in the repo already.
