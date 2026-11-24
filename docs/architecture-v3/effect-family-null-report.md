# EffectFamily null report

Phase 3 of [13-IR-VNEXT.md](13-IR-VNEXT.md) defers folding the 31
`EffectKind` variants into `EffectFamily` until measurement shows which
kinds run the same recurrence and which are audibly distinct. This is that
measurement, covering every rack effect kind plus the voice-level `Comb`.

**Harness:** `crates/engine/effects/tests/effect_family_null.rs`. Each rack
pair renders the same input through the processor the rack builds
(`processors::processor`, span by span, the wet path only) and reports the
residual `b − a` relative to `a`. It is unaligned, because latency is part of
what a kind does. Inputs are 0.5 s at 48 kHz: a unit impulse, a 1 kHz sine at
−6 dBFS, and fixed-seed LCG noise (±0.5, decorrelated L/R). A `b` written as
`X → Y` is a chain rendered in series. `renders_are_deterministic` checks that
every kind renders bit-identically twice, and `voice_comb_null_table` exercises
`ir::EffectKind::Comb` directly via `kontra_dsp::delay::Comb`. To reproduce the
table:

```sh
build-slot cargo nextest run -p kontra-effects --test effect_family_null --no-capture
```

−inf is a bit-exact null. A residual below −60 dB does not establish
inaudibility; detection depends on playback level, frequency, duration, and
masking. A residual near 0 dB or above means different processing.

## Findings

| Pair | Worst rms null | Verdict |
|---|---:|---|
| Gain / Matrix / Stereo(0.5, 0) / Invert(swap) | −inf | **Folded internally.** All four are one 2×2 matrix unified in `matrix::utility_matrix` (used by both `matrix::matrix` and `voice_rack::is_matrix_effect`). |
| Eq(flat) vs Gain(1) | −inf | **Folded internally.** A flat EQ passes its input bit-exactly; `VoiceEq::process` now skips flat bands with settled state and `VoiceEq::retune` skips unchanged bands. |
| Strip(C, Tanh 2) vs Compressor → Shaper Tanh 2 | −inf | **Conditional fold.** The null uses `dry = 0.0` and `output = 1.0`. Use two fully wet slots for a fully wet Strip; otherwise preserve the single mix of the original dry input after the full chain. |
| Compressor(1000:1, 0.01 ms) vs Limiter(PEAK, same ceiling) | −51.4 (peak −38.1 on noise) | **Near-fold.** Within ~50 dB rms but not bit-exact: the limiter catches transients the compressor's attack lets through. Fold only with the limiter's peak path kept as a detector mode. |
| Shaper Tanh(2) vs Atan(2) | −26.3 | Distinct curves. They are already one kind (`Shaper`) with a `Curve` parameter, so nothing to fold. |
| Shaper Tanh(2) vs Saturate(2) | −5.5 | Distinct curves, same `Shaper` kind. |
| Shaper Soft(0) vs Gain(1) | +3.0 (sine −9.1) | **Not transparent.** The shaper oversamples through a halfband allpass pair (`dsp/drive/src/shaper.rs`), which shifts phase and adds latency even at zero drive, so a "bypassed" shaper is not a gain. |
| Overdrive vs Shaper Tanh(4) | +97.6 (noise +13.2) | **Distinct.** Multiband split + feedback + 4× oversampled curve table is a separate recurrence from broadband `Shaper`. |
| Clipper(diode) vs Shaper Tanh(4) | +10.0 (noise +6.4) | **Distinct.** Per-frame nonlinear diode circuit (coupling HP + tone LP + antiparallel diodes + DC block). |
| Crush(hold+6bit) vs Gain(1) | +6156.1 (noise +2.2) | **Distinct.** Sample-and-hold + quantizer cannot fold into linear utility or waveshaper. (Impulse residual is huge because sample-and-hold zeros/shifts the single-frame impulse relative to `a`.) |
| Crush(raw) vs Crush(Converter) | +6147.3 (sine +3.1) | **Distinct modes.** Raw hold/bits vs modelled oversampled companding converter pair; keep `Converter` as a typed mode inside `Crush`. |
| Compressor vs Strip(C, Soft 0) | +3.0 | Same cause as Shaper Soft(0): the strip's oversampled shaper stage. |
| Compressor feed-forward vs feedback detector | −6.8 | **Distinct.** Keep `Detector` as a parameter. |
| Transient(+6/−6 dB) vs Compressor | −4.4 | **Distinct.** Differential fast/slow attack-sustain envelope shaper vs threshold gain computer. |
| RingMod(Oscillator) vs Sidechain / Radio | +1.1 / +12.6 | **Distinct modes.** Keep `RingSource` (`Oscillator`, `Sidechain`, `Radio`) as a typed parameter. |
| SVF LP2 vs ZeroDelay ladder LP2 vs OnePole | +3.2 to +7.1 | **Distinct kernels.** Keep `Ladder` as a parameter. |
| Cascade(3-stage) vs SVF LP2 | +7.8 | **Distinct.** Three-stage morphing SVF cascade with peak normalization. |
| Clocked(Allpass) vs VowelLow / Phaser | +2.2 / +3.3 | **Distinct.** Control-clocked stepped cutoff/resonance allpass/vowel filters. |
| Formant(First) vs Formant(Second) | +10.2 | **Distinct.** Parallel resonant vowel bands; keep `VowelSet` as a parameter. |
| Delay chorus vs flanger (fb 0.7) | +0.4 | Distinct settings of one kind (`Delay`). |
| Phaser vs Notches | +3.7 | **Distinct recurrences.** First-order versus second-order allpass chains. Do not fold. |
| Delay(chorus) vs Phaser | +4.6 | Distinct. |
| Ensemble(7-voice) vs Delay(chorus) | +1.2 | **Distinct.** 7-voice circulant scatter matrix choir. |
| Scanner(LC vibrato) vs Delay(chorus) | +7.0 | **Distinct.** Lumped LC delay line with sweeping stereo pickup. |
| Shifter(1.5×) vs Delay(chorus) | +5.9 | **Distinct.** Dual-head crossfaded pitch-shifting grain delay. |
| CombBank(4-voice) vs Delay(flanger, fb 0.7) | +8.5 | **Distinct.** 4-voice scanned stereo comb bank with soft-clipped feedback. |
| Comb(feedback) vs Comb(feed-forward) (voice-level) | −6.2 | **Distinct modes.** Keep `forward: bool` mode; `Comb(fb=0)` nulls bit-exactly (`−inf`) against `Gain(1)`. |
| Reverb(FDN) vs Hadamard / Combs / Lines | +0.8 / +1.4 / +0.1 | **Distinct networks.** Keep `Network` (`Lines`, `Combs`, `Hadamard`, default `Fdn`) as a typed parameter. |
| Convolution(delta) vs Gain(1) | −inf | **Bit-exact identity at unit delta.** |
| Convolution(hall IR) vs Reverb(FDN) | +3.6 | **Distinct.** Partitioned FFT convolution vs algorithmic feedback networks. |

**For Phase 3:** the measured redundancies are the matrix kinds (Utility, now
unified in `matrix::utility_matrix`), flat EQ (now fast-pathed in `VoiceEq`),
`Comb(fb=0)`, unit-delta `Convolution`, and the wet path of `Strip` =
`Compressor` → `Shaper`. If splitting `Strip`, retarget `ShaperAmount` routes
from the `Strip` slot to the new `Shaper` slot, preserving the route source and
depth. Every other effect kind is a distinct DSP recurrence and must remain a
typed mode/variant inside its family rather than being merged away. `Limiter`
is the one borderline case, at −51.4 dB.

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
| Drive | Overdrive vs Shaper Tanh(4) | impulse | 97.6 | 99.0 | -134.7 | -37.2 |
| Drive | Overdrive vs Shaper Tanh(4) | sine | 25.9 | 24.5 | -27.5 | -1.9 |
| Drive | Overdrive vs Shaper Tanh(4) | noise | 13.2 | 13.8 | -17.2 | -3.5 |
| Drive | Clipper(diode) vs Shaper Tanh(4) | impulse | 10.0 | 4.7 | -47.1 | -37.2 |
| Drive | Clipper(diode) vs Shaper Tanh(4) | sine | 0.2 | 0.4 | -7.5 | -1.9 |
| Drive | Clipper(diode) vs Shaper Tanh(4) | noise | 6.4 | 12.8 | -8.8 | -3.5 |
| Drive | Crush(hold+6bit) vs Gain(1) | impulse | 6156.1 | 6153.1 | -inf | -43.8 |
| Drive | Crush(hold+6bit) vs Gain(1) | sine | -9.9 | -5.9 | -9.2 | -9.0 |
| Drive | Crush(hold+6bit) vs Gain(1) | noise | 2.2 | 6.2 | -11.0 | -10.8 |
| Drive | Crush(raw) vs Crush(Converter) | impulse | 6147.3 | 6137.3 | -inf | -52.6 |
| Drive | Crush(raw) vs Crush(Converter) | sine | 3.1 | 4.1 | -9.1 | -9.5 |
| Drive | Crush(raw) vs Crush(Converter) | noise | 0.6 | 4.6 | -10.8 | -19.0 |
| Dynamics | Transient(+6/-6 dB) vs Compressor | impulse | -19.0 | -19.2 | -42.8 | -43.9 |
| Dynamics | Transient(+6/-6 dB) vs Compressor | sine | -4.4 | -5.1 | -9.0 | -16.9 |
| Dynamics | Transient(+6/-6 dB) vs Compressor | noise | -4.5 | -4.8 | -10.8 | -18.5 |
| Modulation | RingMod(Oscillator) vs RingMod(Sidechain) | impulse | 0.1 | 0.5 | -48.5 | -74.5 |
| Modulation | RingMod(Oscillator) vs RingMod(Sidechain) | sine | 1.1 | 3.7 | -12.9 | -18.3 |
| Modulation | RingMod(Oscillator) vs RingMod(Sidechain) | noise | 0.2 | 2.0 | -17.6 | -32.4 |
| Modulation | RingMod(Oscillator) vs RingMod(Radio) | impulse | 12.6 | 0.2 | -48.5 | -36.1 |
| Modulation | RingMod(Oscillator) vs RingMod(Radio) | sine | 0.6 | 3.2 | -12.9 | -21.1 |
| Modulation | RingMod(Oscillator) vs RingMod(Radio) | noise | 0.5 | 0.8 | -17.6 | -26.9 |
| DelayMod | Ensemble(7-voice) vs Delay(chorus) | impulse | 1.2 | 0.0 | -39.5 | -44.5 |
| DelayMod | Ensemble(7-voice) vs Delay(chorus) | sine | 0.6 | 2.2 | -4.4 | -9.2 |
| DelayMod | Ensemble(7-voice) vs Delay(chorus) | noise | 1.0 | 2.7 | -6.5 | -12.3 |
| DelayMod | Scanner(LC vibrato) vs Delay(chorus) | impulse | 5.6 | 6.6 | -48.8 | -44.5 |
| DelayMod | Scanner(LC vibrato) vs Delay(chorus) | sine | 4.3 | 1.3 | -10.7 | -9.2 |
| DelayMod | Scanner(LC vibrato) vs Delay(chorus) | noise | 7.0 | 4.7 | -18.4 | -12.3 |
| DelayMod | Shifter(1.5x) vs Delay(chorus) | impulse | 5.9 | 4.9 | -49.1 | -44.5 |
| DelayMod | Shifter(1.5x) vs Delay(chorus) | sine | 3.0 | 4.8 | -9.1 | -9.2 |
| DelayMod | Shifter(1.5x) vs Delay(chorus) | noise | 4.2 | 4.7 | -14.5 | -12.3 |
| DelayMod | CombBank(4-voice) vs Delay(flanger, fb 0.7) | impulse | 6.1 | 4.0 | -47.2 | -42.3 |
| DelayMod | CombBank(4-voice) vs Delay(flanger, fb 0.7) | sine | 8.5 | 9.1 | -13.4 | -5.5 |
| DelayMod | CombBank(4-voice) vs Delay(flanger, fb 0.7) | noise | 5.3 | 6.4 | -14.0 | -10.2 |
| FilterEq | Cascade(3-stage) vs SVF LP2 | impulse | 7.8 | 6.3 | -65.8 | -58.4 |
| FilterEq | Cascade(3-stage) vs SVF LP2 | sine | 6.9 | 7.1 | -18.6 | -15.1 |
| FilterEq | Cascade(3-stage) vs SVF LP2 | noise | 7.8 | 6.6 | -32.7 | -25.3 |
| FilterEq | Clocked(Allpass) vs Clocked(VowelLow) | impulse | -1.9 | -8.5 | -44.7 | -46.3 |
| FilterEq | Clocked(Allpass) vs Clocked(VowelLow) | sine | 2.2 | 3.0 | -9.7 | -17.6 |
| FilterEq | Clocked(Allpass) vs Clocked(VowelLow) | noise | -1.8 | 0.6 | -11.7 | -13.3 |
| FilterEq | Clocked(Allpass) vs Phaser | impulse | -0.6 | -4.0 | -44.7 | -42.5 |
| FilterEq | Clocked(Allpass) vs Phaser | sine | 3.3 | 6.7 | -9.7 | -11.3 |
| FilterEq | Clocked(Allpass) vs Phaser | noise | 0.9 | 2.2 | -11.7 | -9.5 |
| FilterEq | Formant(First) vs Formant(Second) | impulse | -1.1 | -7.1 | -47.0 | -43.7 |
| FilterEq | Formant(First) vs Formant(Second) | sine | 10.2 | 4.7 | -11.1 | 1.2 |
| FilterEq | Formant(First) vs Formant(Second) | noise | -1.2 | 0.0 | -14.0 | -10.8 |
| Space | Reverb(FDN) vs Reverb(Hadamard) | impulse | 0.5 | 0.0 | -42.7 | -51.7 |
| Space | Reverb(FDN) vs Reverb(Hadamard) | sine | 0.3 | -0.1 | -3.2 | -13.1 |
| Space | Reverb(FDN) vs Reverb(Hadamard) | noise | 0.8 | 1.0 | -13.7 | -20.3 |
| Space | Reverb(FDN) vs Reverb(Combs) | impulse | 0.7 | 0.0 | -42.7 | -49.9 |
| Space | Reverb(FDN) vs Reverb(Combs) | sine | 0.8 | 0.1 | -3.2 | -13.1 |
| Space | Reverb(FDN) vs Reverb(Combs) | noise | 1.4 | 2.0 | -13.7 | -18.0 |
| Space | Reverb(FDN) vs Reverb(Lines) | impulse | 0.0 | -0.1 | -42.7 | -61.9 |
| Space | Reverb(FDN) vs Reverb(Lines) | sine | -0.3 | -0.4 | -3.2 | -23.7 |
| Space | Reverb(FDN) vs Reverb(Lines) | noise | 0.1 | 0.1 | -13.7 | -30.3 |
| Space | Convolution(delta) vs Gain(1) | impulse | -inf | -inf | -43.8 | -43.8 |
| Space | Convolution(delta) vs Gain(1) | sine | -inf | -inf | -9.0 | -9.0 |
| Space | Convolution(delta) vs Gain(1) | noise | -inf | -inf | -10.8 | -10.8 |
| Space | Convolution(hall IR) vs Reverb(FDN) | impulse | 1.1 | 0.0 | -37.4 | -42.7 |
| Space | Convolution(hall IR) vs Reverb(FDN) | sine | 3.6 | 7.8 | -5.0 | -3.2 |
| Space | Convolution(hall IR) vs Reverb(FDN) | noise | 0.5 | 0.5 | -4.4 | -13.7 |

### Voice-level `Comb` table

| family | pair | signal | rms null dB | peak null dB | a dBFS | b dBFS |
|---|---|---|---:|---:|---:|---:|
| DelayMod | Comb(feedback) vs Comb(feed-forward) | impulse | -7.6 | -7.9 | -41.5 | -42.3 |
| DelayMod | Comb(feedback) vs Comb(feed-forward) | sine | -6.2 | -6.2 | -11.3 | -7.9 |
| DelayMod | Comb(feedback) vs Comb(feed-forward) | noise | -7.6 | -6.8 | -8.5 | -9.3 |
| DelayMod | Comb(fb=0) vs Gain(1) | impulse | -inf | -inf | -43.8 | -43.8 |
| DelayMod | Comb(fb=0) vs Gain(1) | sine | -inf | -inf | -9.0 | -9.0 |
| DelayMod | Comb(fb=0) vs Gain(1) | noise | -inf | -inf | -10.8 | -10.8 |
