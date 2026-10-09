# W15: physical EQ frequency and bandwidth owners

Continuation of gain step `fdf6d4e0`. Port from v1
`0cb7a8a0:src/engine/filter.rs`: `Knob::parse`, `normalized`,
`band_settings`, `Proto::bell` and settled coefficient reuse.
The dedicated TPT owner still explicitly describes a v1 fallback.

`eqFreq1..3` and `eqBandwidth1..3` now use the same physical slot/band
resolver as gain. Any of these routes retains the complete physical band
list. Each live band owns gain in dB and frequency/bandwidth in normalized
0..1 knob units. Frequency is `20 * 10^(3*x)` Hz; bandwidth is
`0.3 + 2.7*x` octaves. Native FREQ/BW aliases, saved getter values,
script initialization, descriptors and generic per-voice modulation all
reach those actual lanes. Signed depth adds before knob saturation.
Static EQ without a control binding retains the existing RBJ path.

The TPT owner caches coefficients per voice using all three float32 knob
values. Flat intervals suspend histories as in v1. The trace publishes
`frequency_knob`, `bandwidth_knob`, and `gain_db`, matching the lane domains.
No new dependency, shared cache or audio-thread storage is added.

Control-thread preview API, used by playback coefficients too:
`PeakingEq::frequency_hz(normalized: f32, rate: u32) -> f32` includes the
0.49 * sample-rate guard; `PeakingEq::bandwidth_octaves(normalized: f32) -> f32`
returns width; `PeakingEq::bandwidth_q(normalized: f32) -> f32` returns the
bell Q. These are EQ-specific laws. LP4 has `LadderSettings::cutoff_hz`,
but LP4/Daft native resonance must not use the EQ bandwidth-to-Q converter.
The registry's native law converts engine integers into lane units; it does
not supply a generic physical filter conversion or couple distinct node IDs.

Failing-first execution showed unsupported core ownership and missing translator
routes. The original cache log was absent at final copying; recovered transcript
excerpts are preserved in `red-contract-evidence.txt` beside the final receipts. Runtime coverage
checks +12 dB at the modulated center, native edits to held notes, bandwidth
response off center, two independent voice projections, and zero heap calls.
Translator checks preserve band indices, native aliases, and script defaults.

Targeted checks pass: both real runtime Peak tests, both EQ lane/eligibility
checks and five Kontakt EQ translator tests. Invalid-domain and preview-adapter checks also pass; area compile-only
checks cover sampler-core, sampler-ir and sampler-kontakt.

Isolated gate-item A/B: Afflatus Chapter II Brass / 2 Horns KS, zone 8426,
key 60, velocity 64/127, 0.5 s at 48 kHz stereo. Both sides activate the saved
bypassed EQ slot and remove authored gain routes; native band 2 gain stays
+9 dB. Frequency 0.7 -> 0.3 at bandwidth 0.4 changes output energy
+1.700845780 dB (difference energy -3.145015403 dB relative to baseline).
Bandwidth 0 -> 1 at frequency 0.5 changes output energy +6.362226545 dB
(difference energy +1.264788366 dB relative to baseline).
Trace input is identical for each pair; only the selected knob changes.
All four renders are audible, finite, heap-free and stay in RAM. No WAV or
decrypted payload is persisted. Exact receipts, binary identity and logs:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-eq-knobs/`.
Quiet CPU/load/underruns and native Kontakt EQ fidelity remain UNKNOWN.
The native coefficient/wrapper gain scaling and modulation cadence require
separate host validation; neither coverage nor this fallback clears release.

NEXT: W12 target recount and coordinator-owned quiet performance acceptance.
