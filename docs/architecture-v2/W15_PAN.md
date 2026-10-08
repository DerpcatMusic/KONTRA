# W15: authored pan routing

Kontakt's shared target translator now retains `pan`, including signed depth,
invert, shaper and lag. It reaches the existing allocation-free voice pan
consumer; no DSP kernel or renderer API changed. Pinned v1's parameter adapter
has no equivalent general pan route to port.

Failing-first: `authored_pan_target_reaches_the_shared_voice_pan_route` failed
before the translator change and passed after. An isolated actual Morphology
AHDSR-to-pan route changed stereo balance by 5.0509941535 dB; dry RMS was
[-43.3536064, -45.0707910] dB, wet [-48.4046006, -45.0707910] dB.
Gate PCM/output stayed in memory; no decoded samples or WAVs were written.
Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-offline-ab.log`.

CPU acceptance versus frozen v1 and W12 full-slot recount remain pending.
NEXT: Constant loop routing and the v1 LoFi/Formant ports.
