# Compressor: alternate mode is a different detector

Status: native Enhanced (mode 1) implemented; Pro extraction remains separate.

Docs first: Kontakt 8 [Effect Reference, Compressor](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
Classic/Enhanced/Pro modes and Stereo Link; KSP
[Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).

Historical binary leads: selector `0x1409047a0` uses wrapper+0x1cc:
mode 0 calls `0x140ab2250`, mode 1 `0x140ab2c80`, mode 2
`0x140ab35d0`, then shared mixing.

Mode 1 uses the absolute linked signal, subtracts the threshold and
clamps negative excess to zero. Positive excess is scaled, transformed
through a piecewise linear table, passed through a two-state attack
recurrence and a decaying peak hold; the output gain is 1/(1+held).
With Stereo Link enabled the input to detection is the signed channel
mean, not the maximum of channel magnitudes. This cached interpretation
does not yet include all branch constants, parameter mappings or mode 2.

Reader `FUN_140d00850` (version 0x70) reads an integer mode, followed by
threshold/ratio/attack/release and link byte. The importer had read this as
f32, misidentifying integer 1 as a subnormal float. Assignment
`FUN_1409619b0`, 0x140961a47–0x140961a73, proves the setter arguments.
radare2 confirmed selector/assignment and setter `FUN_140ab9220`,
0x140ab938d–0x140ab9470. Threshold is exp2(saved_dB*f32(1/6)), rather
than a generic base-10 dB conversion. The .0004 floor applies only to
the denominator of the excess scale, not to the detector threshold.

Attack k=2513.2/(rate*max(attack_ms,1)); multiplier is
f32(1+.2895294507215381*ln(attack_ms)). Saved ratio below .04 uses 5,
otherwise 1/(ratio+.13)-.884; divide by max(threshold,.0004), then
multiply by the logarithmic attack multiplier. Release multiplier is
1-35.566/rate for release<50 ms; otherwise 1-1778.3/(rate*release_ms).
All arithmetic except ln and its multiplier is f32.

Positive scaled excess uses the following native linear segments:

| Upper endpoint | Slope | Intercept |
|---|---|---|
| .5 | .46 | 0 |
| 1.207 | .381895 | .0390524 |
| 2.207 | .34 | .08962 |
| 3.621 | .29703 | .184456 |
| 5.621 | .26 | .31854 |
| 8.449 | .23338 | .468168 |
| 12.449 | .215 | .623465 |
| infinity | .198195 | .832671 |

With attack velocity v and filtered state y, v+=(curve-(1.8*v+y))*k;
y+=(v*k+1e-12); hold=max(hold*release,y); gain=1/(1+hold).
`FUN_14094b710` resets these states to zero. radare2 confirms recurrence
0x140ab31f1–0x140ab3276 and all 32 constants individually, including
piecewise values 0x144fc3330–0x144fc4600, setter constants
0x144fc3088/0x144fc4bd4/0x144fc3f58/0x144fc47f4/0x144fc4bb4,
ratio constants 0x1447148bc/0x144fc3608/0x144fc3ce8, attack damping
0x144714980, and anti-denormal 0x144f385a8. Evidence is the same-SHA
complete-export cache and readonly Kontakt8.exe copy (SHA-256
0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8).

Focused failing-first test
`effects::tests::enhanced_compressor_uses_native_detector_and_integer_selector`
uses native integer mode 1, threshold=-6, ratio=.5, attack=1 ms,
release=100 ms and 48 kHz. Four consecutive inputs [1,-.5] give left
.999157310, .997557640, .995283782, .992414534; right stays -.5.
With link enabled, [1,-1] cancels in the signed detector and passes unchanged.
Computed f32 vectors, not native recordings. Classic's existing proxy is
unchanged. Pro `FUN_140ab35d0` has a sliding peak window, log/exp approximation,
knee and detector lowpass; its coefficients and window scheduling must be
extracted before replacing that mode's still-diagnosed fallback.
