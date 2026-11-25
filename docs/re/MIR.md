# Kontakt 8.13.1 music information retrieval (`detect_*`)

Source: `Kontakt 8.exe` (8.13.1, x64), decompiled with r2ghidra. The KSP
commands call `ni::kreator::mir::detect*` (`kreator-common/src/MIR`, names
at 0x1479c4860..). Each wrapper loads the zone's sample at its own rate
(0x141c2bcb0), averages the channels (0x141c2bb50) and resamples to
22050 Hz (0x141c2c250, a polyphase resampler at 0x141ee5da0).

## detectKey (0x141c24e10)

Analyser 0x141d5d850, fed by 0x141de5150, finished by 0x141d646d0.

- Frames: 5513 samples (0.25 s) every 2756 (0x141de5150), a symmetric Hann
  window `0.5 (cos(πx) + 1)`, x from -1 to 1 (0x141d61860), zero-padded into
  an 8192-point FFT. Magnitudes of bins 0..4095 are scaled by 1/4096.
- Bands (0x141df54d0 with the sparse branch 0x141df6280): 96 Gaussians, band
  n at MIDI 27 + n (440 Hz tuning). σ = 0.25 (fc − fc·2^(−1/12)), weights
  where |f − fc| < 3σ, scaled to unit sum.
- Level: `20 log10(max(x, 1e-4)) + 80`, less the curve
  `63.70603 − 30.53453 log10(27 + n)`, floored at 0.
- Masking (0x141df5bd0(96, 5), 0x141df6d30): each band spreads
  `10^(w·x/20) − 1` to its neighbours with w = 2/3 and 1/3 (0 on itself). The
  band then loses `20 log10(1 + spread)`, floored at 0.
- Chroma: bands 6..89 (MIDI 33..116) add to class (n + 3) mod 12. Frames are
  averaged.
- Gate (0x141d646d0): no key (−1) when the mean square of the 22050 Hz signal
  is under −60 dB. A signal shorter than one frame is one zero-padded frame.
- Match (0x141d64370): the chroma and five profiles (data at
  0x147c105b0..0x147c106c8, built by 0x141d63ad0; two major, three minor)
  are standardised (mean 0, sd with n − 1, floor 1.19e-7, 0x141de44d0).
  - The score of root r is `Σ chroma[k] · profile[(k − r) mod 12] / 12`,
    times 1.05 for a minor profile.
  - The highest score wins, and on a tie the first one.
  - The result is r + 12 × minor (0x141d66ad0).
- 0x141d63360 and 0x141d630f0 only feed a confidence that KSP does not read.

## detectTempo (0x141c25550)

A beat tracker runs (0x141d5dce0, 0x141de5270, 0x141d64a10), but the wrapper
calls the BPM step 0x141d641f0 with flag 1. With that flag the step always
takes 0x141d61cf0, so the tempo comes from the length alone:

- Range 60..300 BPM (0x141d5d0c0), target 120.
- seconds = resampled length / 22050. 5 ms or less reads 0.
- f = 60 / seconds. Halve while f ≥ 60, then double, collecting each value,
  until f > 300.
- The candidate nearest 120 wins (stable sort 0x141d5b200, so on a tie the
  slower one).

## detectSampleType (0x141c27c30) and the taggers

0x141d5d300 parses a model that 0x141d6cf10 builds from base64 chunks
(0x147a44080.., 76 characters each; 969 bytes decoded, NI's own binary
format). 0x141de4c80 scores the features of 0x141e00b70 with 0x141d5f8a0:

- The features come from a 1024-point STFT (0x141df51f0), 64 mel bands
  (0x141df4ac0), then 0x141df8950(64, 2, 1), 0x141e08600 and 0x141e01420.
- They are scaled (0x141d5f9c0) and scored by an RBF SVM:
  `Σ αᵢ exp(−γ‖x − svᵢ‖²)`.
- Platt scaling turns the score into a probability:
  `1 / (1 + exp((f − ρ) A + B))`.
- The sample is a drum (1) when p > 0.5, otherwise an instrument (0).

detectDrumType (DrumTagging.cpp) and detectInstrumentType
(InstrumentTagging.cpp) are other taggers of this kind, not yet decoded. KONTRA
does not have these models, so its classifiers fail
(`$NI_DETECT_*_TYPE_INVALID`).
