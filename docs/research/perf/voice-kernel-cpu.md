# Voice kernel CPU: time-stretch, resampling and effects per voice

Status: measurement and negative-result record. No engine code changed.

## Metric

CPU cost of ONE sample voice per 64-frame block at 48 kHz, single thread, niced.
Marginal cost = slope of audio-thread p50 (µs per block) between 16 and 256 voices,
because a fixed per-block runtime cost (~0.85 µs) sits under every voice count.
Instruction counts are the per-voice-block figure: `perf stat -e instructions:u`
on runs of 4000 and 12000 blocks, differenced (removes setup), divided by voices.

Harness: `crates/sampler-core/examples/voice_cpu.rs` (resident synthetic PCM, one
note per voice, 3.17-semitone transpose for every workload except `plain`).
Built with `--profile corpus` (release codegen, thin LTO, symbols kept).
Host load averaged 35–40 on 16 cores during runs, so timings are noisy; instruction
counts are the stable number. Base: `origin/main` cec3f9f34.

## Measurements (per voice, per 64-frame block)

| workload | what it exercises | marginal µs/voice/block | instr/voice/block | share of 5 µs target |
|---|---|---|---|---|
| plain | no transpose, cubic/unity path, amp env | 0.16 | ~1.8 k (27 instr/frame) | 3% |
| fractional | 3.17 st, polyphase short sinc (radius 12) | 1.12 | ~10.9 k | 22% |
| filtered | sinc + SVF low-pass on the voice chain | 0.80 | ~9.5 k | 16% |
| modulated | filtered + envelope and LFO routed to pitch, pan, cutoff, gain | 2.71 | ~24.6 k | 54% |

Fixed per-block cost (1 voice, p50 1.03 µs) is ~0.85 µs of that; the 1-voice
instruction figure is dominated by it and is not a per-voice number.

Anomaly to chase, not explained here: `fractional` (no chain) costs 40% more per
voice than `filtered` (sinc plus an SVF). Both run `render_run` → polyphase
`run_polyphase_four`; the no-chain path also shows `Cursor::render_filtered`
at 10–17%. Needs a look at why the chained path is cheaper before any kernel
work on it.

## Profile (gprofng sampling, `--profile corpus`, 64 voices)

`perf record` is not permitted here; `gprofng collect app -p on` works.
Samples are coarse (about 1000 per run), so read the shares as rough.

- fractional: polyphase `dot_four` inside `run_polyphase_four` 72%, `render_filtered` 10%, `clock_gettime` 6% (the harness's own per-block clocks).
- modulated: polyphase 35%, `PreparedChainRef::render` self 10%, `VoiceModState::evaluate` 7%, `memcpy` 7%, SVF `FilterContext::process` 6%, `finish` 4%, `prepare_voice` 3%.

The sinc kernel is already the four-output SIMD form (`Polyphase::dot_four`,
`sampler_simd::dispatch_fused`, AVX2/FMA wide path, bit-checked against four
`dot` calls in `four_output_polyphase_preserves_each_fused_and_unfused_dot_bit`).
Its receipt test (`four_output_polyphase_cpu_receipt`) measures four-output at
0.66–0.77 of four scalar-dispatch `dot` calls. That is already landed.

## Ceiling estimate for the sinc kernel

Per output frame: 8 chunks of 8 f32 (taps × 2 channels, stride 64), each chunk
= 3 loads + sub + 2 FMA. Ideal on AVX2 is about 48–60 instructions/frame; measured
is roughly 120 per frame in the kernel (170 for the whole voice, ~10.9 k / 64). Remaining headroom in the kernel
is roughly 1.3–2×, from:

1. `black_box` spill of the 8-lane accumulators per output (forces store/reload each frame).
2. Pre-differenced row table (`b - a` stored), removing one sub per lane. Bit-exact; expected ~10–15%, below the 20% bar.
3. Reassociating to `A + t·D` (two accumulators, one lerp per frame). Changes rounding, so it needs a max-abs error budget. Not attempted.

None of these is a 20% win on the whole voice by itself, so none is landed.

## Not done in this lane

- Streaming and page-fed voices (Areia). The fixture is resident, so the real-library gap is not measured here. That belongs to the streaming lane.
- Convolution and group/insert effects, measured separately under `eq-stereo` and the effects benches, not in this run.
- Block-rate vs sample-rate control audit for the modulated path. `VoiceModState::evaluate` is already block-rate; its 7% is per-block route work, which is the next candidate after the sinc kernel.

## Sources

Not fetched in this session. Listed as leads to verify before adoption (no code copied):

- https://ccrma.stanford.edu/~jos/resample/ — Julius O. Smith, resampling and windowed-sinc theory (ideas).
- https://github.com/sfztools/sfizz — BSD-2 sample engine; block-rate modulation (license check before any reading of code).
- https://github.com/linuxsampler/linuxsampler — GPL-2; ideas only.
- https://github.com/surge-synthesizer/surge — GPL-3; SIMD voice ideas only.
- https://docs.rs/wide — permissive SIMD crate (candidate dependency, not adopted).
- https://www.intel.com/content/www/us/en/docs/intrinsics-guide/ — AVX2/FMA instruction costs for the ceiling estimate.

## Reproduce

```
nice -n 10 build-slot cargo build --profile corpus -p sampler-core --example voice_cpu
B=<target>/corpus/examples/voice_cpu
$B --workload fractional --voices 64 --block 64 --blocks 4000
nice -n 10 gprofng collect app -p on -o f.er $B --workload modulated --voices 64 --blocks 60000
gprofng display text -functions f.er
```
