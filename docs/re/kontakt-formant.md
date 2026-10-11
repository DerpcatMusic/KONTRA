# Formant I: native table leads

Status: native saved-state Formant I implemented (round 3).

Docs first: Kontakt 8 [Filter Reference, Formant I](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference),
Talk, Sharp and Size; KSP [Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), filter controls.

Historical binary leads: wrapper `0x1408fa7b0`, core setter
`0x140b05ac0`, sample-rate setup `0x140afd4f0`, clock `0x140b03690`,
audio `0x140b00750`. The wrapper clamps control inputs to [0,1] and
updates them every 32 samples. The audio core has three parallel
second-order bandpasses, not three serial peaking EQ sections.

Formant I rows read from frequency table `0x144fcea10`, radius table
`0x144fcea90` and amplitude table `0x144fceb10`:

| Row | Frequencies, Hz | Radii (rounded) | Amplitudes (rounded) |
| --- | --- | --- | --- |
| 0 | 233, 326, 1914 | .9039, .9233, .9785 | .44669, .044976, .029107 |
| 1 | 317, 451, 2270 | .9039, .9233, .9339 | .21727, .27102, .10339 |
| 2 | 726, 1190, 2383 | .9436, .9613, .9132 | .5176, .4384, .3303 |
| 3 | 429, 1920, 2987 | .9573, .9573, .9556 | .56235, .71781, .37498 |
| 4 | 370, 2578, 4731 | .9436, .9706, .9666 | .45, .6675, .5295 |

Talk selects 4·Talk with linear interpolation between adjacent rows.
Core index 1 gives frequency scale u + max(u,.5); core index 2 gives
t = (2 − v)v, radius scale .8999999762 + .1000000238t and amplitude
scale 5(1 + t²). Saved assignment is now proved: reader `FUN_140d00db0` stores Talk/Sharp/Size
at +2c/+30/+34. Assignment `FUN_1409619b0`, at 0x14096468f–0x1409646af,
calls `FUN_1409a80a0` with Talk/Sharp, assigning core indices 0/2.
At 0x140964787–0x14096478f it calls `FUN_140aeffe0` with Size, assigning
index 1. radare2 disassembly confirmed these loads/calls.

For each band, z is scaled angular frequency, g = S(z)(1 − radius),
b₀ = g/(1 + g), b₂ = −b₀, a₁ = −2C(z)/(1 + g),
a₂ = (1 − g)/(1 + g). S and C are the native sine/cosine polynomials.
With histories w₁,w₂, w = x − a₁w₁ − a₂w₂; output is the sum of
amplitude·(b₀w + b₂w₂) over the three bands. Coefficients update every
four samples; control interpolation also needs to be preserved.

Confirmation: read-only PE extraction reproduced all five rows. Using
Talk=0, Size=Sharp=.5 and 48 kHz, a double-precision calculation
with extracted f32 constants gives the first impulse components
.01258623117, .001489377111, .002562107300, sum .01663771558.
The focused test `effects::tests::formant_native_parallel_impulse_and_saved_sharp_size_mapping`
checks this first sample and Sharp=0/Size=1 (.0361916923), both channels.
It failed first on the old proxy diagnostic; the native processor now passes.
The serial EQ proxy and its fixed .25 trim are removed.

Evidence: same-SHA complete-export Ghidra cache, functions above, and radare2
on `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe` (SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`).
radare2 hexdumps confirmed the three tables (0x144fcea10–0x144fcebc0),
sine/cosine polynomial coefficients (0x149e745e0/0x149e74600), and
.9 radius base at 0x144714914. Setter disassembly 0x140b05ac0 confirms
Sharp/Size branches. These are computed vectors, not host recordings.

Limit: static saved controls are implemented. Native 15 ms control ramps,
32-sample wrapper clock and four-sample coefficient clock remain to be wired
when live Formant controls are supported; existing diagnostics for unrelated
filter laws remain.
