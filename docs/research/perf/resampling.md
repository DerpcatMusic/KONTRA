# Resampling / interpolation: plain sample voice cost

Metric: CPU cost of one plain resident sample voice (no filter, amp envelope, stereo out), per 64-frame block at 48 kHz, single thread. Measured by the ignored test `source::voice_receipt::voice_receipt` (`crates/sampler-core/src/source/voice_receipt.rs`), which calls `Cursor::render` with `perf stat -e instructions,cycles` on the host (busy, so timings are noisy; instruction counts are the stable number).

## Before / after (ns per voice-block, 1 voice; 16/64/256 voices within a few percent)

| scenario | before | after | note |
|---|---|---|---|
| unity copy (step 1.0, frac 0) | 30 | 33 | direct tap path, unchanged |
| unity cubic (step 1.0, frac 0.5) | 356 | 360 | Catmull-Rom run, unchanged |
| down cubic (step 0.5) | 362 | 358 | unchanged |
| up polyphase (step 1.37) | 687 | 677 | bank, unchanged |
| up short sinc (step 3.0) | 7654 | 1051 | **-86%**: step 2..4 now use the polyphase bank |
| High long sinc (step 1.37) | 13906 | 14173 | unchanged (noise) |

Target from the header: plain voice ≤ 5 µs per block. Every Realtime scenario is now under 1.1 µs per voice-block at 64 frames; the only one above 5 µs is High quality (14 µs), which is the explicit long-sinc mode.

## Change

- `resample.rs`: the polyphase bank now covers steps up to 4 (16 stretches, still 8 per octave, so steps 1..2 are bit-identical). `MAX_TAPS` grows to 97.
- `resample/coefficient_reuse.rs` (test-only): tile rows sized to the step-2 bank, so its 26 KB budget holds.
- New test `wide_bank_tracks_short_sinc_between_steps_two_and_four`: the bank differs from the old per-frame sinc for steps 2..4; the test bounds the difference on a passband tone at 2e-3.

Not bit-identical for steps 2..4 (the audio changes by at most the bounded error above). Full `sampler-core` lib: 823 passed, 0 failed.

## Sources

- Niemitalo, "Polynomial interpolators for high-quality resampling": https://www.yehar.com/blog/?p=368 (via forum pointer: https://hydrogenaud.io/index.php/topic,126585.0.html). Not used: the 4-point 3rd-order optimal polynomial is not implemented; the oddio report says polynomial interpolators roll off before 20 kHz, so the polyphase bank stays the high-ratio choice.
- oddio issue on interpolator quality: https://github.com/Ralith/oddio/issues/12. Adopt the observation only: polynomial beats linear, still loses to polyphase.
- sfizz `sample_quality` opcode (1 linear, 2 polynomial, 3..10 windowed sinc): https://sfzformat.com/opcodes/sample_quality. Adopt the same tiered idea (we already have cubic / bank / sinc tiers). Reject nothing.
- sfizz 0.4.0 release (polynomial resampling added, default 2): https://newreleases.io/project/github/sfztools/sfizz/release/0.4.0.
- sfizz forum thread (Hermite default, Sinc 8..72 cost): https://www.kvraudio.com/forum/viewtopic.php?p=8090844.
- Cycling'74 sinc-interpolating playback object: https://cycling74.com/forums/writing-a-sinc-interpolating-playback-object. Confirms precomputed sinc tables with phase interpolation, as in `Polyphase`.

Not done: the Kontakt8.exe REA trace of its read/interp kernel, the sfizz source read, the per-note fixed-ratio fast path, the unrolled loop-crossfade split, and the f32/i16 convert-on-load study. Those remain open.

## Licences

No third-party code copied. Ideas only (cubic, polyphase bank and phase tables are our existing code).
