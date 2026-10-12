# Vista Harp legacy filter — W15

Base: frozen 0.3.381 source `54d9a5c5`. Pending 382 source inspected at
`63c9ef8a`; its translator also omits legacy type 2.

## Ranked census

W12's resource census reports four `Filter: filter type` diagnostics and four
`effect` diagnostics on Vista Harp. These are duplicate reports for the same
four enabled physical slots, not eight distinct DSP blocks:

| Rank | Physical class | Scope/count | Audible role |
| --- | --- | --- | --- |
| 1 | Legacy filter type 2 | Group inserts 8/9/18/19, slot 0; four slots | Shapes damping-release samples; cutoff 0.5996094, resonance 0 |
| 2 | AHDSR → filterCutoff | Four routes on the same slots, intensity 1 | Opens the saved cutoff over the release envelope |

The 20 source-mode diagnostics are separate. RandomBipolar, LFO6 and module
frequency/intensity work belong to W6; this change does not claim them.

## Port

Pinned v1 `0cb7a8a0:src/engine/filter.rs` maps type 2 to one lowpass TPT
section (two poles) and uses `43.6 * 2^(8.96*x)` Hz and
`FRAC_1_SQRT_2 * 28^r` Q. The translator ports that ID mapping and those
f32 parameter laws onto the existing shared SVF kernel. It retains the physical
slot and carries its 8.96-octave cutoff span into addressed modulation; other
filter families keep their existing laws. All new metadata is preparation-only.

The native Legacy LP1 topology is unverified: this is the pinned v1 proxy,
not a native-fidelity claim. Generic cutoff modulation still uses the shared
64-frame clock and generic 20 Hz–20 kHz bounds, rather than v1's normalized-knob
clamp and 32-frame clock. No new kernel or dependency is introduced.

## Validation

The occupied-slot unit fixture failed on the base and passes after the port.
Three targeted legacy tests pass, including the neighboring SV slot's unchanged
law with signed modulation and a post-amplitude legacy slot. The installed
Vista Harp test passes and verifies all four slots/routes, with zero remaining
filter-type, effect or module-parameter omission diagnostics. It isolates the
middle-C damping-release sample, retaining the saved filter knob and the
authored cutoff envelope for separate static/envelope renders.

| Saved sample metric | Result |
| --- | ---: |
| Static filter residual relative to dry | −13.0615 dB |
| Envelope residual relative to static filter | −13.3504 dB |
| Level-normalized derivative energy change | −0.0227 dB |
| Dry left/right RMS, 0–500 ms | −51.3376 / −53.9161 dB |
| Static-filter left/right RMS, 0–500 ms | −51.3361 / −53.9178 dB |

The saved damping sample is already dark. The first A/B assertion incorrectly
required a spectral change below −0.05 dB; the final guard checks the lowpass
direction and substantial filter/envelope waveform residuals. Rendering is
finite and passes the audio-thread no-heap guard. Core/KSP/Kontakt area
`cargo test --profile ci --no-run` also passes. This is execution evidence;
native fidelity and quiet CPU acceptance remain open. Sample/output PCM
stays in RAM; no WAV is written. Numeric receipts go to
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-vista-harp/`.

Receipts: `ranked.json`, `legacy-lowpass-red.log`,
`legacy-contracts-green.log`, `release-sample-first-threshold.log`,
`release-sample-ab.log`, `area-no-run.log`, and `validation.json`.
