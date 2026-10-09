# Whole voice profile before the v1 port

CONTENDED-ATTRIBUTION, diagnostic only. No acceptance timing or native parity claim.

Exact v2 base: `54d9a5c5da45ada122694bde78e3b4aaf36dc731`; frozen v1 source `0cb7a8a0`. Original Horns256 audit schedule (48 kHz, 12 notes, 4 seconds). `perf_event_open` is denied in this runtime and passwordless sudo is unavailable; settings were not changed. The fallback arms a 500 µs **audio-thread CPU clock** timer after loading and lets GDB collect only PC/TID/time. Raw logs remain in tmpfs. Inline frames are resolved from executable LOAD segments, including the separate code segment's file/virtual offset. No memory, PCM, or user-stack dumps are persisted.

This attributes all sampled main-thread work, without first filtering for DSP. Percentages refer to the audit steady wall window (.25–1 s); debugger stops perturb pacing. V1 has only 46 samples and coarse (~2.17 point) proportions, so do not compare their implied absolute CPU times. V2 has 705 samples. Both activity observers reported QUIET, but the coordinator requested the conservative label above because unrelated KURV builds could add noise.

| Rank | v1 self frame | % | v2 self frame | % |
|---|---|---:|---|---:|
| 1 | `unresolved shared library` | 21.74 | `<sampler_core::dsp::eq::Eq>::process` | 12.34 |
| 2 | `<kontakto::engine::filter::Section>::process_avx` | 17.39 | `<sampler_core::dsp::control::ControlRamp>::value` | 9.36 |
| 3 | `kontakto::engine::voice::mix_avx512` | 15.22 | `mix_gains` | 8.94 |
| 4 | `<kontakto::engine::voice::Voice>::render` | 13.04 | `sampler_core::dsp::process::<false>` | 8.37 |
| 5 | `apply` | 10.87 | `unresolved shared library` | 4.82 |
| 6 | `<kontakto::engine::filter::VoiceFilter>::process_amplified` | 6.52 | `mix` | 3.83 |
| 7 | `<kontakto::engine::voice::Voice>::advance` | 4.35 | `<sampler_core::voice_mod::VoiceModState>::evaluate` | 3.55 |
| 8 | `<kontakto::engine::filter::Unit>::key` | 2.17 | `sampler_simd::dispatch::v3::<(), sampler_simd::dispatch::run_fused<(&mut sampler_core::source::Cursor, &mut [[f32; 2]], &mut sampler_core::envelope::EnvelopeState), (), <sampler_core::source::Cursor>::render_run<sampler_core::source::PagedFrames>::{closure#4}, <sampler_core::source::Cursor>::render_run<sampler_core::source::PagedFrames>::{closure#5}>::{closure#0}>` | 2.98 |
| 9 | `spec_fill<f32>` | 2.17 | `<sampler_core::dsp::stereo::Stereo>::process` | 2.70 |
| 10 | `<kontakto::engine::filter::Section>::process` | 2.17 | `run<sampler_core::source::{impl#3}::render_run::{closure#5}::{closure_env#0}<sampler_core::source::PagedFrames>>` | 2.55 |

The attribution supports carrying v1's prepared insert/control layout and fused mixing with its voice loop. EQ target reads, control interpolation and dry/wet gain calculation together occupy a large share of the v2 sample. Its EQ already caches coefficients, but checks all targets for every frame; the port must keep prepared coefficients and test controls outside that inner loop. Settled mix controls should feed one gain/mix loop. Moving only Hermite interpolation leaves these costs in place.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-v1-whole-voice-20261009/gdb-profiles/{v1,v2}/profile.json`, numeric metrics and hashed diagnostics beside them. The request and grant were removed after the captures; `DIRECT-W8-HANDOFF.json` records the direct drain.

W6 owns the copied raw envelope/LFO/mod state API. Raw Kontakt f32 setters and source admission must be carried forward during preparation, with arbitrary/unadmitted v2 sources kept intact. Filter/module destinations need a shared single-advance projection before a whole Horns voice can be admitted.

NEXT: failing-first held EQ/mix checks → prepared control/filter bridge → whole voice render port.
