# EffectFamily null report

Phase 3 of [13-IR-VNEXT.md](13-IR-VNEXT.md) defers folding the 31
`EffectKind` variants into `EffectFamily` until measurement shows which
kinds run the same recurrence and which are audibly distinct. This is that
measurement.

**Harness:** `crates/engine/effects/tests/effect_family_null.rs`. Each pair
renders the same input through the processor the rack builds
(`processors::processor`, span by span, the wet path only) and reports the
residual `b − a` relative to `a`. It is unaligned, because latency is part of
what a kind does. Inputs are 0.5 s at 48 kHz: a unit impulse, a 1 kHz sine at
−6 dBFS, and fixed-seed LCG noise (±0.5, decorrelated L/R). A `b` written as
`X → Y` is a chain rendered in series. `renders_are_deterministic` checks that
every kind renders bit-identically twice. To reproduce the table:

```sh
build-slot cargo nextest run -p kontra-effects --test effect_family_null --no-capture
```

<!-- coderabbit-ai-generated:v1 -->

<!-- coderabbit-ai-generated-section:v1:a709d1909bab04dc3aabff7236a6d450c19e89104ce0cf99248bb75072b17438 -->

−inf is a bit-exact null. A residual below −60 dB does not establish
inaudibility; detection depends on playback level, frequency, duration, and
masking. A residual near 0 dB or above means different processing.

## Findings

| Pair | Worst rms null | Verdict |
|---|---:|---|
| Gain / Matrix / Stereo(0.5, 0) / Invert(swap) | −inf | **Fold.** All four are one 2×2 matrix (they already share `matrix::matrix`). |
| Eq(flat) vs Gain(1) | −inf | **Fold.** A flat EQ passes its input bit-exactly. |
| Strip(C, Tanh 2) vs Compressor → Shaper Tanh 2 | −inf | **Conditional fold.** The null uses `dry = 0.0` and `output = 1.0`. Use two fully wet slots for a fully wet Strip; otherwise preserve the single mix of the original dry input after the full chain. |
| Compressor(1000:1, 0.01 ms) vs Limiter(PEAK, same ceiling) | −51.4 (peak −38.1 on noise) | **Near-fold.** Within ~50 dB rms but not bit-exact: the limiter catches transients the compressor's attack lets through. Fold only with the limiter's peak path kept as a detector mode. |
| Shaper Tanh(2) vs Atan(2) | −26.3 | Distinct curves. They are already one kind (`Shaper`) with a `Curve` parameter, so nothing to fold. |
| Shaper Tanh(2) vs Saturate(2) | −5.5 | Distinct curves, same `Shaper` kind. |
| Shaper Soft(0) vs Gain(1) | +3.0 (sine −9.1) | **Not transparent.** The shaper oversamples through a halfband allpass pair (`dsp/drive/src/shaper.rs`), which shifts phase and adds latency even at zero drive, so a "bypassed" shaper is not a gain. |
| Compressor vs Strip(C, Soft 0) | +3.0 | Same cause as the row above: the strip's shaper stage. |
| Compressor feed-forward vs feedback detector | −6.8 | **Distinct.** Keep `Detector` as a parameter. |
| SVF LP2 vs ZeroDelay ladder LP2 vs OnePole | +3.2 to +7.1 | **Distinct kernels.** Keep `Ladder` as a parameter. |
| Delay chorus vs flanger (fb 0.7) | +0.4 | Distinct settings of one kind (`Delay`). |
| Phaser vs Notches | +3.7 | **Distinct recurrences.** First-order versus second-order allpass chains. Do not fold. |
| Delay(chorus) vs Phaser | +4.6 | Distinct. |

**For Phase 3:** the measured redundancies are the matrix kinds (Utility),
flat EQ, and the wet path of `Strip` = `Compressor` → `Shaper`. If
splitting Strip, retarget `ShaperAmount` routes from the Strip slot to the
new Shaper slot, preserving the route source and depth. Everything else that
shares a family is a distinct recurrence. It should become a family with a typed mode
(`Ladder`, `Detector`, `Curve`, allpass order), not be merged away.
`Limiter` is the one borderline case, at −51 dB.


<!-- /coderabbit-ai-generated:v1 -->
Not covered yet: `Overdrive`, `Clipper`, `Crush`, `RingMod`, `Transient`,
`Ensemble`, `Scanner`, `Shifter`, `CombBank`, `Cascade`, `Clocked`,
`Formant`, `Reverb` and `Convolution`. Their settings are curve tables or
measured laws, and a meaningful pair needs a translator-sourced preset. Add
a pair when a fold of one of them is proposed. `Comb` builds no rack
processor (it runs on each voice), so it needs a voice-level harness.

## Measured table

| family | pair | signal | rms null dB | peak null dB | a dBFS | b dBFS |
|---|---|---|---:|---:|---:|---:|
| Utility | Gain(0.5) vs Matrix(0.5·I) | impulse | -inf | -inf | -49.8 | -49.8 |
| Utility | Gain(0.5) vs Matrix(0.5·I) | sine | -inf | -inf | -15.1 | -15.1 |
| Utility | Gain(0.5) vs Matrix(0.5·I) | noise | -inf | -inf | -16.8 | -16.8 |
| Utility | Stereo{0.5, 0} vs Matrix(I) | impulse | -inf | -inf | -43.8 | -43.8 |
| Utility | Stereo{0.5, 0} vs Matrix(I) | sine | -inf | -inf | -9.0 | -9.0 |
| Utility | Stereo{0.5, 0} vs Matrix(I) | noise | -inf | -inf | -10.8 | -10.8 |
| Utility | Invert{swap} vs Matrix(swap) | impulse | -inf | -inf | -43.8 | -43.8 |
| Utility | Invert{swap} vs Matrix(swap) | sine | -inf | -inf | -9.0 | -9.0 |
| Utility | Invert{swap} vs Matrix(swap) | noise | -inf | -inf | -10.8 | -10.8 |
| FilterEq | Eq(flat) vs Gain(1) | impulse | -inf | -inf | -43.8 | -43.8 |
| FilterEq | Eq(flat) vs Gain(1) | sine | -inf | -inf | -9.0 | -9.0 |
| FilterEq | Eq(flat) vs Gain(1) | noise | -inf | -inf | -10.8 | -10.8 |
| FilterEq | SVF LP2 vs ZeroDelay ladder LP2 | impulse | 3.2 | 2.5 | -58.4 | -56.2 |
| FilterEq | SVF LP2 vs ZeroDelay ladder LP2 | sine | 6.9 | 6.5 | -15.1 | -9.1 |
| FilterEq | SVF LP2 vs ZeroDelay ladder LP2 | noise | 3.2 | 3.9 | -25.3 | -23.0 |
| FilterEq | SVF LP2 vs OnePole | impulse | 3.6 | 9.0 | -58.4 | -55.6 |
| FilterEq | SVF LP2 vs OnePole | sine | 7.1 | 6.0 | -15.1 | -12.0 |
| FilterEq | SVF LP2 vs OnePole | noise | 3.5 | 4.0 | -25.3 | -22.6 |
| Dynamics | Compressor vs Strip(Compressor, Soft(0)) | impulse | 3.0 | -0.0 | -43.9 | -43.9 |
| Dynamics | Compressor vs Strip(Compressor, Soft(0)) | sine | -9.1 | -8.8 | -16.9 | -16.9 |
| Dynamics | Compressor vs Strip(Compressor, Soft(0)) | noise | 3.0 | 6.2 | -18.5 | -18.5 |
| Dynamics | Strip(Compressor, Tanh(2)) vs Compressor → Shaper Tanh(2) | impulse | -inf | -inf | -40.4 | -40.4 |
| Dynamics | Strip(Compressor, Tanh(2)) vs Compressor → Shaper Tanh(2) | sine | -inf | -inf | -11.3 | -11.3 |
| Dynamics | Strip(Compressor, Tanh(2)) vs Compressor → Shaper Tanh(2) | noise | -inf | -inf | -12.9 | -12.9 |
| Dynamics | Compressor(feed-forward) vs Compressor(feedback) | impulse | -inf | -inf | -43.9 | -43.9 |
| Dynamics | Compressor(feed-forward) vs Compressor(feedback) | sine | -6.8 | -14.2 | -16.9 | -13.7 |
| Dynamics | Compressor(feed-forward) vs Compressor(feedback) | noise | -6.9 | -13.6 | -18.5 | -15.3 |
| Dynamics | Compressor(∞:1, −18 dB) vs Limiter(−18 dB) | impulse | -53.7 | -53.7 | -61.8 | -61.8 |
| Dynamics | Compressor(∞:1, −18 dB) vs Limiter(−18 dB) | sine | -61.5 | -57.2 | -21.0 | -21.0 |
| Dynamics | Compressor(∞:1, −18 dB) vs Limiter(−18 dB) | noise | -51.4 | -38.1 | -22.7 | -22.6 |
| Drive | Shaper Tanh(2) vs Atan(2) | impulse | -26.3 | -25.3 | -40.3 | -40.0 |
| Drive | Shaper Tanh(2) vs Atan(2) | sine | -33.0 | -30.1 | -4.8 | -4.6 |
| Drive | Shaper Tanh(2) vs Atan(2) | noise | -31.2 | -24.1 | -6.6 | -6.4 |
| Drive | Shaper Tanh(2) vs Saturate(2) | impulse | -5.6 | -5.4 | -40.3 | -46.8 |
| Drive | Shaper Tanh(2) vs Saturate(2) | sine | -5.5 | -5.4 | -4.8 | -11.4 |
| Drive | Shaper Tanh(2) vs Saturate(2) | noise | -5.6 | -5.5 | -6.6 | -13.1 |
| Drive | Shaper Soft(0) vs Gain(1) | impulse | 3.0 | 4.2 | -43.8 | -43.8 |
| Drive | Shaper Soft(0) vs Gain(1) | sine | -9.1 | -9.1 | -9.0 | -9.0 |
| Drive | Shaper Soft(0) vs Gain(1) | noise | 3.0 | 2.8 | -10.8 | -10.8 |
| DelayMod | Delay(chorus) vs Delay(flanger, fb 0.7) | impulse | -1.8 | -5.1 | -44.5 | -42.3 |
| DelayMod | Delay(chorus) vs Delay(flanger, fb 0.7) | sine | 0.4 | 5.4 | -9.2 | -5.5 |
| DelayMod | Delay(chorus) vs Delay(flanger, fb 0.7) | noise | -2.0 | 2.3 | -12.3 | -10.2 |
| DelayMod | Phaser vs Notches | impulse | 1.6 | -0.0 | -42.5 | -48.2 |
| DelayMod | Phaser vs Notches | sine | 3.7 | 5.3 | -11.3 | -12.9 |
| DelayMod | Phaser vs Notches | noise | 1.5 | 2.1 | -9.5 | -16.5 |
| DelayMod | Delay(chorus) vs Phaser | impulse | 4.1 | 0.0 | -44.5 | -42.5 |
| DelayMod | Delay(chorus) vs Phaser | sine | 2.0 | 5.0 | -9.2 | -11.3 |
| DelayMod | Delay(chorus) vs Phaser | noise | 4.6 | 8.7 | -12.3 | -9.5 |
