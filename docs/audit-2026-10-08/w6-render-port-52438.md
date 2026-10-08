# W6 render port at 52438eb4

Base: `52438eb49b3d9daa45e88eeead48f610c9dfaa95`. Branch: `v2/w6-cpu-52438`.
Receipt directory: `~/.cache/kontakto-w6/cpu-52438/`.

## Stage localization

Six GDB runs use the existing cpu_audit schedule at 48 kHz / 64 frames on the
same gate cells: Una Corda Cotton, ANALOG STRINGS and Areia Full Ensemble.
V1 is the frozen `cpu-audit-v1`, SHA256 `b9998ca2ce2f2ed4f9f88bbfb11c5e884fa162a87cdf89f26ece6f1248fdc6ab`.
V2 is exact base source, symbolized corpus/ThinLTO (no default features;
clap,library-access), SHA256 `d90996bf914827be006cb42d65d98d028566e467ad5a1d88c7947accf4aa14da`.
Gate timing instead uses ci/no-LTO/default features; these are diagnostic
profiles, not acceptance timings. V1 integrity checks passed. No perf or system
settings changed. Sampling records named frames and source locations, no values,
registers, sample payloads or memory dumps.

Each sample stops the process during the four-second sequence, reads the main
audio thread's stack, then continues. Sleeping samples are excluded; attribution uses the
first project source frame, including inline frames. Samples cover the whole
sequence, not only steady notes. Counts are approximate stage localization,
not a decomposition of p50/p99 or cycle-weighted CPU. Low counts on Cotton and
Analog preclude fine percentage conclusions. Debugger timings are unscored.

| Exclusive bucket | Cotton v1/v2 | Analog v1/v2 | Areia v1/v2 |
| --- | ---: | ---: | ---: |
| Voice loop/transport | 1 / 6 | 4 / 13 | 6 / 37 |
| Resampler | 0 / 2 | 0 / 2 | 0 / 61 |
| Envelopes/modulation | 1 / 1 | 1 / 4 | 45 / 34 |
| Filters | 0 / 0 | 6 / 0 | 0 / 0 |
| FX | 0 / 0 | 4 / 6 | 2 / 1 |
| Mixing | 0 / 1 | 0 / 2 | 0 / 51 |
| Streaming | 0 / 5 | 0 / 5 | 2 / 81 |
| KSP | 0 / 0 | 10 / 0 | 1 / 0 |
| Event dispatch | 0 / 0 | 0 / 0 | 0 / 0 |
| Active samples | 2 / 15 | 25 / 32 | 56 / 265 |

Zeros mean no sampled IP in that bucket, not zero cost. The exact raw stacks,
source locations, logs and classification are in the receipt directory.

Areia localizes v2's bulk cost to streaming (31%), resampling (23%) and mixing
(19%). W9 retains streaming ownership. The first DSP target is the block cubic
path from `0cb7a8a0:src/engine/voice.rs::mix_avx2`.

## Port and preservation boundary

Copy v1's tap-column staging and Catmull-Rom polynomial into a separate four-frame
kernel, adapting its fixed-point/f32 phases to v2's full f64 phase and coefficient
law. Wider CPU dispatch uses the existing sampler-simd mechanism. V2's sequential
fractional cursor recurrence, f32 sample conversion, envelope advancement,
nonfinite guards and multiplication/addition order are preserved. Short tails,
seams, reverse traversal, high quality and downsampling keep their scalar paths.
No cache, demand, streaming, filter/FX trait or mod-evaluation API changes.

Targeted checks compare the new columns with the scalar kernel and compare
complete output/cursor/envelope state through all tail sizes, fractional steps
and curved envelope transitions. The offline witness renders the cpu_audit note
schedule to float WAVs for an A/B comparison; those WAVs must be deleted after
comparison, with only hashes and numeric errors retained.

Validation: both optimized bit-exact tests pass, along with 44 source,
resampling and paged-render regression tests. Root `cargo test --no-run` passes.
Matching scalar/cubic cpu_audit and WAV witnesses use ci/default features, the
same configuration as the gate. The scalar witness compiles the new branch out;
all other source and build settings match the candidate. Exact binary hashes
are in `cubic-BUILD.json` in the receipt directory.

Status: HOLD for WAV comparison and quiet CPU acceptance. No READY or CPU
acceptance claim. CPU acceptance must use unprofiled ci/default-feature binaries
against the frozen v1 in a quiet A/B. No release or install.

NEXT: targeted checks, full-signal preservation and unprofiled same-cell CPU.
