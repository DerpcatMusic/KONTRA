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

Corrected per-worktree validation GREEN: 65540 reduction-bit cases, four resampler unit tests, 18 resampling integration tests, SIMD dispatch test, core area no-run. Ignored cost witness RED101 (legacy ratio0.995919) → GREEN0 (legacy1118671ns/candidate650912ns per1048576 folds, ratio0.581862). Isolated witness is diagnostic; workload gate decides acceptance. Candidate source5340a425, binary SHA256700d7ffcf5bbdfe396116324b9ddb3385fded4b6eed01caf2ca4467c5c14b946.

| Cache | Version | Median µs | p99 µs | Underruns | Event/render heap calls |
|---|---|---:|---:|---:|---|
| cold | v1 | 167.863 | 295.746 | 0 | 0/0 |
| cold | before | 2140.921 | 3306.043 | 0 | 0/0 |
| cold | after | 2146.290 | 2534.998 | 0 | 0/0 |
| warm | after | 2194.291 | 2540.469 | 0 | 0/0 |
| warm | before | 2072.749 | 2459.517 | 0 | 0/0 |
| warm | v1 | 175.333 | 326.816 | 0 | 0/0 |

All six admitted rows QUIET; 141 steady blocks each, peak192 voices. Each cold eviction verified pages_after0. First v1 attempt contended by KURV and a later before-v2 attempt contended by pitch-tracker are preserved and excluded. The admitted quiet v1 cold row was retained across the foreign-drain interruption; other rows completed after both owners ACKed source-only.

**REJECTED / DO NOT APPLY**: SSE fold cold median+0.25%, p99−23.32%; warm median+5.86%, p99+3.29%. No whole CPU speedup or v1 parity claim. Disassembly confirms the fold narrowed the bank dot loop from AVX256 (six YMM arithmetic instructions) to SSE128 (zero YMM arithmetic instructions). The isolated fold witness missed this surrounding-loop regression. Next correction must preserve the eight-lane wide accumulator through the fold.

AVX correction frozen and untimed GREEN: source8c206006a751353382683f4492185bddd96a21c9, binary SHA256e4e148fb792891ae3209cbe7d5342d0b00cfb691264074a3cf396e13796a5853. A private checked CPU capability is prepared with the bank; the fold keeps all eight AVX lanes without an atomic feature check per sample. Baseline SSE/scalar paths retain the same add grouping. Generated largest-frame dot has six YMM FMA instructions and zero XMM FMA instructions. The first correction witness called the AVX helper from baseline code (ratio2.2613) and failed; rerunning both sides in production's existing wide dispatch gives RED101 ratio0.995327 → GREEN0 ratio0.467830 (legacy1404937ns/candidate657272ns per1048576 folds). All bit/resampling/SIMD/core no-run checks pass again. No corrected-candidate CPU claim or READY until its own quiet A/B, ordered after W10.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-horns-frame-diff-20261010/`. W9 profiling unit drained exit0; DIRECT W10 sent before kernel verification.

NEXT: ordered frozen AVX256 correction A/B after W10 DIRECT.
