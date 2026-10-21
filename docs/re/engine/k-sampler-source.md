# Kontakt Sampler and DFD source modules

Status: **partial, waiting on Kontakt 8 decompilation.** The export at
`/data/dev/workspace/Research/KONTRA/source/kontakt8/` had no `READY` marker and an
empty `kontakt8/` directory when this was written (2026-10-21). Laws below are limited
to what the repo already records (format fields, engine code, tests, docs). Items
marked *unrecovered* must not be implemented from guesses.

Scope: the Sampler source (stored mode 0) and DirectFromDisk (stored mode 3). Both play
the same sampled signal; DFD differs only in how frames reach memory. Wavetable, Time
Machine and the other source machines are other units.

## Modules and parameters

Format fields are read by `crates/sampler-kontakt/src/mapping.rs` (group and zone records)
and `crates/sampler-ir/src/kontakt.rs` (loop and instrument records). Engine-side
representation is `crates/sampler-ir/src/lib.rs` (`Playback`, `Looping`, `LoopRange`,
`LoopSlot`) and `crates/sampler-core/src/source.rs`.

| Parameter | Level | Format field | Unit / law | Default | Where it lands |
| --- | --- | --- | --- | --- | --- |
| Source mode | group | `source.mode` | 0 Sampler, 3 DFD | — | `library.rs:1623` (both accepted, no report) |
| Start | zone | `start` (i32) | source frames | 0 | `Playback::start` (`load.rs`) |
| End | zone | `end` (i32) | signed offset from source end; 0 = full length | 0 | `Playback::end` |
| Start modulation range | zone | `start_modulation` | source frames | 0 | `Playback::start_range` |
| Root key | zone | `root_key` (i16) | MIDI note | — | `KeyTracking::Tracked { root }` (`library.rs:3297`) |
| Zone tune | zone | `tune` (f32) | linear ratio, not semitones | 1 | `Pitch::Ratio` (`library.rs:1662`) |
| Group tune | group | `tune` (f32) | linear ratio, not semitones | 1 | step via `12·log2(zone·program)+transpose` (`library.rs:3276`) |
| Key tracking | group | `key_tracking` (bool) | on/off | — | `KeyTracking` (`library.rs:3297`) |
| Reverse | group/zone | `reverse` | flag | off | `Playback::reverse` (`library.rs:2194, 3326`) |
| Loop start/length | loop slot | `loop_start`, `loop_length` | source frames | — | `LoopRange` |
| Loop count | loop slot | `loop_count` | 0 = repeat indefinitely; positive = total passes | 0 | `LoopSlot::count` |
| Loop mode | loop slot | `mode` | until-release / continuous / slot | — | `Looping` |
| Ping-pong | loop slot | `alternating_loop` | flag | off | `LoopRange::alternating` (`docs/ALTERNATING_LOOPS.md`) |
| Loop tuning | loop slot | `loop_tuning` (f32, >0) | source-rate multiplier on repeats | 1 | `LoopSlot::tuning` |
| Crossfade | loop slot | `x_fade_length` | frames; clamped to file start | 0 | `LoopRange::crossfade` (`load.rs:655–669`) |
| Interpolation quality | group | `interpolation` (i32) | **unrecovered enum** | — | **parsed, never consumed** (see gap G1) |
| DFD preload size | instrument | `dfd_channel_preload_size` (i32) | **unrecovered unit and meaning** | — | **parsed, never consumed** (see gap G2) |
| Release trigger, release monophonic, release counter, voice group, amp split, muted, soloed | group | various | voice/group routing | — | outside this unit (voice-group/release units) |

## Engine laws already implemented (evidence in repo)

- **Position and step.** Each voice keeps an integer traversal distance and a separate
  fractional phase. The step is the asset/output rate ratio times pitch, limited to
  1/256 … 16 source frames per output frame (`resample.rs` `MIN_STEP`, `MAX_STEP`).
  Unsupported ratios fail before a voice slot is taken; nothing clamps silently
  (`docs/architecture-v2/RESAMPLING.md`).
- **Interpolation (engine, not Kontakt-verified).** `ResampleQuality::Realtime` (default)
  uses a four-point cubic for unity and upsampled voices and a radius-12 Blackman sinc
  for downsampled voices. `High` uses a radius-48 sinc for every non-unity step
  (`resample.rs`). The polyphase bank for `1 < step <= 2` uses 8 stretches/octave and
  65 rows (`docs/architecture-v2/RESAMPLING.md`). Kontakt's own interpolation is **unmeasured**
  (`docs/architecture-v2/DSP_COVERAGE.md`, Sampler row).
- **Loops.** Continuous, until-release and eight physical slots (`Looping::Slots`, with
  holes). Ping-pong reflects at the loop endpoints and resumes forward after release
  (`docs/ALTERNATING_LOOPS.md`). Until-release loops record the wrap boundary; the tail
  plays after release. Crossfade laws are amplitude and power complementary
  (`source.rs` test `crossfade_laws_keep_amplitude_or_power_complementary`).
- **Crossfade before start.** A loop before the sample start keeps its crossfade from the
  file; at file start the crossfade clamps to zero without a report
  (`load.rs:1973`). Test pins both cases.
- **Start inside or past a loop.** Start inside a loop plays to the loop end, then wraps
  to the loop start (which may precede the sample start). Start past the loop never
  enters it (`source.rs:2445` test).
- **Reverse.** Same increasing traversal coordinate; only the physical mapping changes
  (`stream.rs:821, 953`). Reverse and sample-start edges are pinned by
  `mapping_sample_markers_match_source_loop_and_reverse_boundaries` (`src/ui/mapping.rs`).
- **Offsets.** `with_offset` takes microseconds and converts to source frames at the
  source rate; offsets never advance loop passes (`source.rs:2486` test).
- **Voice generations.** Stream rings reject writes from a stale voice generation and
  never read a previous generation after a new offset (`crates/sampler-pool/src/ring.rs`
  test `a_new_offset_never_reads_a_previous_voice_generation`).
- **Streaming.** Head ranges, page residency and interpolation guards are retained per
  voice (`docs/architecture-v2/STREAMING.md`, `stream.rs`). Streamed playback reads the
  same PCM as resident playback. Not compared with Kontakt DFD buffering.
- **Tuning formula.** Group and zone tune multiply, then transpose adds semitones
  (`library.rs:3276`). Loop tuning is applied on repeats only.

## Gap table

| # | Item | Kontakt side | Engine side | Status | Next step |
| --- | --- | --- | --- | --- | --- |
| G1 | Group interpolation quality | `interpolation` / `interp_quality` i32 on the group record (`vendor/ni-file/src/kontakt/objects/group.rs:131`). Enum values and resulting kernel: **unrecovered** | Parsed into `mapping.rs:24`; no IR field; `library.rs` never reads it. The resampler is chosen once per engine instance (`sampler-core/src/lib.rs:1210, 1387`), not per group or voice | **Missing** and silently ignored (violates the "diagnose, don't ignore" rule) | Recover the enum and the per-value kernel from Kontakt 8 (group sampler routine). Then add a per-group `interpolation` selector in `sampler-ir` that picks a kernel per voice. Kernel tables are already cached per quality (`OnceLock`), so selection is cheap. Until then, report non-default values as not modeled |
| G2 | DFD preload size | `dfd_channel_preload_size` i32 on the instrument (`kontakt.rs:29`). Unit (frames, bytes, per channel) and effect on prefetch: **unrecovered** | Parsed and copied to IR; nothing reads it. `stream.rs` uses its own headroom policy (`policy.headroom`) | **Missing**, silently ignored | Recover the consumer in Kontakt's streaming code. Map it to the existing stream policy's per-channel head. Do not change prefetch numbers without a measured underrun/latency comparison |
| G3 | Sampler interpolation accuracy | Unmeasured against Kontakt | Cubic and sinc as above | Unverified | Compare against a Kontakt render of a pitch-shifted sine (needs native rig; not available here) |
| G4 | Loop count semantics | `loop_count` 0 = indefinite, positive = passes | Matches the IR comment | Consistent with format docs; not verified against a Kontakt render | Confirm with a counted-loop render |
| G5 | DFD vs Sampler signal | Both documented as the same signal | Single engine path; DFD accepted without report (`library.rs:1623`) | Matches the documented claim; DFD streaming cost is measured elsewhere (`DSP_COVERAGE.md`) | None for signal. Streaming buffer parity is G2 |
| G6 | Root key when absent | Format writes `root_key` on zones | Zone root maps to key tracking. Ranges struct carries root but only key-tracking reads it | Covered for tracked keys; no separate behaviour for untracked zones | Check against a fixed-pitch zone in the corpus |

## Design for the engine change (G1, not yet implemented)

1. Add `interpolation: Option<Interp>` to the group config in `sampler-ir` (`None` = the
   engine default). `Interp` is a closed enum of kernels the engine already has
   (`Realtime` cubic/short sinc, `High` sinc, plus any recovered Kontakt-only kernel).
2. Resolve the kernel per voice at admission from the group's selector, so one engine
   can host groups with different quality. Keep the no-allocation rule: store the
   resolved `&'static` kernel in the voice template.
3. Kontakt mapping: translate the stored i32 with the recovered table in
   `sampler-kontakt`. Unknown values produce a diagnostic and use the default, never a
   silent fallback.
4. Test: a golden short-kernel vector per quality (coefficient sums and a sine render
   with a fixed step), one test that a two-group instrument with different qualities
   produces different PCM.

No code change is made in this unit yet, because G1 and G2 need values only the
decompilation can provide.

## Evidence confidence

- Format fields, loop and reverse behaviour, offsets, voice generations: **high**
  (repo code and tests; not compared with Kontakt output).
- Interpolation and DFD streaming parity with Kontakt: **none** (no recovered law, no
  native render).
- Gap G1 and G2 existence: **high** (grep of the crates shows no consumer).
