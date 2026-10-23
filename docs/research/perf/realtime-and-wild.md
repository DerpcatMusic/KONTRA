# Real-time safety, jitter and "wild ideas": perf research lane

Branch: `perf-research-realtime-and-wild`. Scope: audio-thread jitter tooling,
voice-cost measurement, build flags, and a sweep of public sampler / game-audio
voice-management techniques. Each entry says what it teaches and what we do with it.

## Metric

**Cost of one plain sample voice per 64-frame block at 48 kHz, single thread, niced.**
Plain = resident PCM, no filter, default (linear) interpolation, amp envelope, stereo out.

Measured with `crates/sampler-core/examples/render_bench.rs` (release, `thin` LTO,
the repo default). Three runs per config, minimum p50 reported. Machine shared with other agents.

| active voices | capacity | p50 µs / block | µs / voice / block | ns / frame / voice |
|---:|---:|---:|---:|---:|
| 16 | 64 | 1.68 | 0.105 | 1.6 |
| 16 | 4096 | 3.91 | 0.244 | 3.8 |
| 256 | 256 | 24.65 | 0.096 | 1.5 |

Reading: the plain voice is roughly **0.1 µs per block, about 50x under the 5 µs target**
for this synthetic fixture (constant PCM, default envelope). Real instruments add
filters, cubic/sinc kernels, and stereo cross-fades, so they cost more; the real-instrument
bench (`sampler-perf`, which needs licensed Kontakt/UVI libraries) is the gate for that.

Open lead: with 16 active voices, capacity 4096 costs +2.2 µs per block over capacity 64
even though `render_prepared_voices` already walks only occupied bits of `voice_activity`
(`crates/sampler-core/src/render.rs:~412`). The extra cost is outside the voice loop
(candidates: onset/cohort prep, per-capacity buffers). Not yet attributed; `perf record`
is not permitted on this host, so attribution needs instrumented timers.

## Sources

Real-time tooling and voice management:

1. FMOD Core API, managing resources / virtual voices — https://fmod.com/docs/2.03/api/managing-resources-in-the-core-api.html . Virtual channels are chosen by audibility (volume, distance, priority, group gain); the lowest-priority, least-audible channel is virtualised first when the real-channel limit is hit. **Adopt the idea** (audibility-ranked eviction), not the API.
2. FMOD Studio virtual voice system — https://documentation.help/fmod-studio-api/virtualvoices.html . Audibility inputs and the hysteresis of "virtual" vs "real". Same conclusion as (1).
3. Audiokinetic Wwise Q&A, "virtual voices bad practice" — https://www.audiokinetic.com/qa/6164/virtual-voices-bad-practice . Staff advise the game, not the voice pool, should decide audibility for large ambience sets. **Reject** automatic virtualisation as a default for our engine: it changes which samples play, which breaks the "plays correctly" rule in `docs/` memory.
4. SoLoud voice management — https://solhsa.com/soloud/voicemanagement.html . Pauses inaudible voices and resumes them when audible. Confirms the pause/resume pattern is common; our voices carry filter and delay state, so resumption must be exact or we reject it.
5. Clang RealtimeSanitizer (RTSan) docs — https://rocm.docs.amd.com/projects/llvm-project/en/docs-10.0.0/LLVM/clang/html/RealtimeSanitizer.html . Marks real-time functions and reports `malloc` / lock calls at run time with negligible overhead. **Adopt as a CI idea** for C++ bindings; no Rust binding was found.
6. `assert_no_alloc` crate — https://docs.rs/assert_no_alloc/ (source: https://github.com/Windfisch/rust-assert-no-alloc). Global allocator wrapper that aborts on allocation inside a guarded closure; disabled in release by default. Caveat from the docs: it catches only heap allocation on the guarded thread, not locks. **Adopt** as a debug-build guard for the audio callback (dev-dependency, not shipped).

Sample playback and interpolation:

7. sfizz 0.4.0 release notes — https://newreleases.io/project/github/sfztools/sfizz/release/0.4.0 . Resampling quality is a per-region opcode (default 2, more costly than linear). Confirms that per-region quality tiers are the common industry shape. The search did not surface sfizz's voice/SIMD internals; source reading is the next step (BSD-2, allowed to read and re-implement).
8. SuperCollider `BufRd` help — https://help.sccode.org/UGens/Oscillators/BufRd.html . Interpolation 1 (none), 2 (linear), 4 (cubic) selected per UGen. No published per-option CPU numbers. **Adopt the per-voice quality knob** (we already have one).
9. SuperCollider `PlayBuf` (2017 wiki) — https://github.com/supercollider/supercollider/wiki/PlayBuf-2017 . PlayBuf keeps its phase as a double, avoiding the float phase precision limit of BufRd beyond 2^24 frames. **Relevant**: check our cursor precision for very long resident samples.

Measurement and benchmark caveats:

10. GVI performance whitepaper (Tascam) — https://tascam.com/downloads/tascam/511/gvi_performance_whitepaper.pdf . Compares voice counts on simple sine instruments and states explicitly that filters, interpolation and crossfades are excluded. **Reject as a metric**: the same caveat applies to our synthetic `render_bench`; the real-instrument bench stays the gate.
11. HISE forum, sampler voice count — https://forum.hise.audio/post/49260 . Users disagree whether unused voices cost CPU. Our measurement (the 4096-capacity lead above) suggests they can, at a small per-slot cost.

Build flags and PGO:

12. Rust internals thread, "Profile-guided optimization: how well does it work for you?" — https://internals.rust-lang.org/t/profile-guided-optimization-how-well-does-it-work-for-you/11108 . Early PGO results were weak because of a Cargo issue later fixed; PGO now works. **Adopt as a later step** (needs a training bench; see the flags section).
13. "PoC: using LLVM's profile guided optimization in Rust" — https://unhandledexpression.com/using-llvm-pgo-in-rust . Shows the raw-profile workflow for Rust. Used as the method reference, not measured here yet.
14. `multiversed` crate — https://rust-digger.code-maven.com/crates/multiversed . Runtime dispatch across `x86-64-v4 / v3` targets via `multiversion`. **Reject for now**: adds a runtime branch per call-site; `target-cpu` flags in our own binary are the cheaper first test (see measurements).

## Build-flag measurements

Method: `perf stat -e instructions,cycles` on the whole `render_bench` process (three
configs, ~2100 blocks each, setup included), host load ~11-34 on 16 cores, one process at
a time, `nice -n 19`. Instructions are the metric; cycles are reported for reference only.

| build | instructions | cycles | vs base (instructions) |
|---|---:|---:|---:|
| base: release, thin LTO (repo default) | 1.262 G | 403 M | — |
| `maxperf`: fat LTO, codegen-units=1 | 1.265 G | 324 M | +0.3% |
| `RUSTFLAGS=-C target-cpu=x86-64-v3` (release) | 1.098 G | 340 M | -13% |

Reading:
- `maxperf` (fat LTO + one CGU) does not reduce instruction count. Its cycle drop (~20%)
  is within host-load noise and is not claimed.
- `target-cpu=x86-64-v3` saves ~13% of instructions on this fixture. This is below the
  20% landing bar, and it is not shippable as-is: a plugin built with AVX2 won't load or
  will fault on older x86 hosts. Multiversioning (item 14) would be the way to keep it,
  at the cost of a runtime branch. Not landed.
- No PGO build was attempted (budget: two builds total). PGO is the next candidate if the
  `x86-64-v3` gain is worth pursuing with dispatch.

## Outcome

- No engine change landed. No technique met the 20% bar on the metric.
- The voice-LOD / virtual-voice idea was analysed, not prototyped: a plain voice costs
  ~0.1 µs per block, so culling audible-zero voices saves almost nothing on the metric,
  while provable "below -100 dB" culling of voices with filter/delay state is a
  correctness risk. Recommendation: do not implement until real-instrument numbers show
  voice cost dominates.
- Open lead for a follow-up: the +2.2 µs per block cost of capacity 4096 vs 64 at 16
  active voices (see Metric section).
