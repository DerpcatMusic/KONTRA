# Horns256 top20 self-frame diff

Attribution only: cycles:u, period100000, user frame-pointer call chains, main audio TID. Both captures begin after load/idle and cover the unchanged original4s12-note schedule at48kHz/256frames. No PCM or user-stack memory dumps. No timed acceptance claim from these profiles.

Exact v2 d8f1c6237eb774934b3a1914ee0bf0a698571c4c. Frozen v1 0cb7a8a0, approved CPU adapter41a4a5d8 (existing read-only binary at ~/.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1-profile). Root ~/.cache/kontra-v1 manifest and both binary hashes passed.

Physical functions (`perf report --no-children --no-inline`): v1 3230samples, v2 32753samples; zero lost samples. Inline iterator and intrinsic names are kept in the raw receipt but not used to rank physical frames. Percentages are self cycles across the captured main-thread run, not inclusive call-chain percentages.

| Rank | v1 frame | Self % | v2 frame | Self % |
|---:|---|---:|---|---:|
| 1 | `<engine::filter::VoiceFilter>::process_amplified` | 18.05 | `Cursor polyphase fused loop` | 14.22 |
| 2 | `<engine::filter::Section>::process_avx` | 15.05 | `<dsp::stereo::Stereo>::process` | 8.04 |
| 3 | `engine::voice::mix_avx512` | 8.30 | `<voice_mod::VoiceModState>::evaluate` | 6.40 |
| 4 | `<engine::voice::Voice>::render` | 6.78 | `<dsp::PreparedVoiceChain>::render::<false, source::PagedFrames>` | 5.87 |
| 5 | `<engine::Player>::render_block` | 6.22 | `held rack mixing kernel` | 5.03 |
| 6 | `fx::convolution::mac_avx2` | 4.27 | `<envelope::EnvelopeState>::level` | 4.38 |
| 7 | `<engine::voice::Envelope>::run` | 2.45 | `Cursor cubic block loop` | 4.06 |
| 8 | `<fx::convolution::Partitioned>::process` | 1.83 | `<envelope::EnvelopeState>::next` | 3.77 |
| 9 | `engine::voice::taps_avx2` | 1.83 | `dsp::process::<false>` | 3.51 |
| 10 | `unresolved frame 0x00000000001b7c8e` | 1.42 | `held EQ paired kernel` | 3.40 |
| 11 | `<engine::filter::Unit>::key` | 1.33 | `<Runtime>::render_voice::<false>` | 2.84 |
| 12 | `<fx::convolution::Convolver>::process` | 1.33 | `<dsp::PreparedVoiceChain>::finish` | 2.53 |
| 13 | `audio::integrate_avx512` | 1.24 | `cubic source gather` | 2.21 |
| 14 | `unresolved frame 0000000000000000` | 1.24 | `<Runtime>::prepare_voice` | 1.88 |
| 15 | `<realfft::ComplexToRealEven<f32> as realfft::ComplexToReal<f32>>::process_with_scratch` | 1.18 | `<dsp::ProcessorState>::finite` | 1.80 |
| 16 | `unresolved frame 0x00000000001b83b5` | 1.15 | `<stream::PageReader>::span` | 1.67 |
| 17 | `<audio::Pcm>::decode_avx2` | 1.08 | `<voice_mod::VoiceModState>::mix` | 1.43 |
| 18 | `unresolved frame 0x00000000001b7d2a` | 1.05 | `<Runtime>::service_streaming` | 1.39 |
| 19 | `<realfft::RealToComplexEven<f32> as realfft::RealToComplex<f32>>::process_with_scratch` | 1.02 | `unresolved frame 0x00000000001b7ad3` | 1.28 |
| 20 | `unresolved frame 0x00000000001b7ad3` | 0.90 | `<dsp::PreparedVoiceChain>::begin::<source::PagedFrames>` | 1.23 |

The three leading extra v2 frames are the `sampler_simd::dispatch::v3` fused polyphase source loop14.22%, `sampler_core::dsp::stereo::Stereo::process`8.04%, and `sampler_core::voice_mod::VoiceModState::evaluate`6.40%. V1 has no polyphase sinc bank; it runs its fused Hermite mixer and prepared filter/control chain instead. Its equivalent capabilities are inside native prepared kernels, so these are additional v2 implementation frames rather than absent v1 features.

Largest-frame change: keep the existing sinc/anti-aliasing contract and replace the accumulator memory boundary with v1 Section::process_avx paired-channel SSE lane reduction and low64 store (`0cb7a8a0:src/engine/filter.rs:1040–1048`). Exact add grouping stays `(sum0[k]+sum1[k])`, then stereo pairs. Coefficients, fused/ordinary dot products, cursor, envelopes, dispatch selection and resampling quality are unchanged. The old accumulator spill is visible in largest.annotate.txt; one reload instruction accounts for11.3% of local samples in that frame (sampling/skid is not an exact instruction timing).

Validation pending: 65540 reduction-bit cases, sinc/cubic/resampling tests, ignored paired fold-cost witness with legacy memory fold as RED, core area no-run, frozen original-driver candidate and quiet cold/warm Horns256 A/B. No READY or full CPU parity claim yet.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-horns-frame-diff-20261010/`. W9 profiling unit drained exit0; DIRECT W10 sent before kernel verification.

NEXT: kernel RED/GREEN → freeze → quiet Horns256 A/B.
