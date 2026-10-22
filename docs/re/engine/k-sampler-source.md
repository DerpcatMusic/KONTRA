# Kontakt Sampler and DFD source modules

Status: **partial; implementing without waiting for the decompilation (user decision, 2026-10-21).** Earlier notes: the export at
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
| G1 | Group interpolation quality | `interpolation` / `interp_quality` i32 on the group record (`vendor/ni-file/src/kontakt/objects/group.rs:131`). Enum values and resulting kernel: **unrecovered** | Parsed into `mapping.rs:24`; no IR field; `library.rs` never reads it. The resampler is chosen once per engine instance (`sampler-core/src/lib.rs:1210, 1387`), not per group or voice | **Missing.** Now diagnosed: non-zero values are reported as `interpolation quality` (`library.rs`, test `unmodeled_group_interpolation_is_reported_not_dropped`). Corpus census: 0 non-default files in 226,278 (0x95) and 5,675 (0x96) group records, so the default is 0 and no corpus instrument hits this path | Recover the enum and the per-value kernel from Kontakt 8 (group sampler routine). Then add a per-group `interpolation` selector in `sampler-ir` that picks a kernel per voice. Kernel tables are already cached per quality (`OnceLock`), so selection is cheap. Until then, report non-default values as not modeled |
| G2 | DFD preload size (confirmed unit, default, global override, 6 KiB window granularity; window ported as `dfd_window.rs`, consumer not wired) | `dfd_channel_preload_size` i32 on the instrument (`kontakt.rs:29`). Unit (frames, bytes, per channel) and effect on prefetch: **unrecovered** | Parsed and copied to IR; nothing reads it. `stream.rs` uses its own headroom policy (`policy.headroom`) | **Missing.** Now diagnosed as `DFD preload override` (`library.rs`, program translation). Public docs (NI DFD tab, third-party guides) describe the preload as kilobytes per sample, default 60 KB; the census values (3, 8, 21, 31, 42, 729, nonzero in 1,000+ programs) are consistent with KB but **unverified**. Applying it would change head sizing (`stream.rs` `head_frames`) for real libraries, so it is not applied yet | Recover the consumer in Kontakt's streaming code. Map it to the existing stream policy's per-channel head. Do not change prefetch numbers without a measured underrun/latency comparison |
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

## Kontakt 8 evidence (decompile, `source/kontakt8/`, confirmed 2026-10-21)

**DFD preload (G2), confirmed unit and default**
- Stored in bytes in the instrument object at `+0x21b70` (i32). The text-preset key `dfdChannelPreloadSize` is parsed with `atoi` into that offset (`0140920000.c`, `FUN_14091ff80` region, around line 8258).
- Instrument default is `0xf000` = 61440 bytes = 60 KiB, set at init (`0140940000.c`, around line 10579).
- Global override: flag `DAT_14a5b96cc` switches the head size to a global value `DAT_149e55070`. Its UI/setting names are `overrideInstPreloadSize` and `overrideInstDFD` (`0140730000.c`).
- Window law (`FUN_14082a530`, `0140820000.c:7321`): applies only when the runtime source mode at `+0xba8` is 4, otherwise the window is 0. For mode 4 the preload is scaled by a channel-count argument and rounded up to multiples of `0x1800` = 6144 bytes. `FUN_14057fe40` (`0140570000.c:10923`) turns the same preload into a UI value `(preload/0x1800 - 1) * k`.
- Window arithmetic, ported in `crates/sampler-kontakt/src/dfd_window.rs` (body of `FUN_14082a530`, re-read 2026-10-22): `P` = preload bytes per channel, `C` = channels, `B` = bits, `F = ceil(B/8)*C` (bytes per frame), `pos` = `+0x9d0` (reverse: `-(+0x9d4)`), `off` = `+0x9d8`, `d = pos - off`. Streamed mode only: `base = P*C`; if `d < 0`, `base += F*d`; `total = base + off*F*2`; window `W = ceil(total/0x1800)*0x1800` (C division truncates); start `= max(d, 0)`; reverse negates `W`. A negative `off` resolves through the linked object (`+0xa8`): null, blocked (`+0x177c`) or `+0xa09` set gives `off = 0`; a negative linked offset skips rounding and keeps `W = base`. Non-streamed: `W = 0`.
- The Time Machine path quantizes the same preload to a `dfd_buffersize` of `floor(preload/0x1800)*6` (`01410a0000.c`, `FUN_1410a6450`). Its validation message is `invalid dfd_buffersize, expected [6..240] and multiple of 6` (`01410a0000.c`, string). The 6..240 KiB range is therefore a Kontakt constraint for that buffer, not verified for plain DFD.
- DFD eligibility: a sample whose source record has flag `+0x177c` set is rejected with `%s cannot be loaded in DFD mode!` (`01408d0000.c`, `FUN_1408d5c80`). Adding samples to a group in mode 4 is rejected with `These samples cannot be added to a group which is in DFD mode!` (`0140890000.c`, `FUN_140890100`) when the group has children at `+0x118` and the sample is not already streamed.

**Corpus values (corrects an earlier misreading)**
- The census `fields.tsv` columns are *file counts*, not values. The earlier "729" was a count.
- Scratch survey over 2,113 NKI paths, first program chunk only, read via `read_chunks` and `Program::params`: `dfd_channel_preload_size` = 61440 in 723 programs and 0 in 11. Unreadable or bank-only files: 1,379 (not surveyed further).
- So the corpus uses the default almost everywhere and the override is rare. The old "diagnose every nonzero value" rule would have fired on every default program. The diagnostic now fires only for values that are neither 0 nor 0xf000.

**Group interpolation (G1)**
- Binary group `interp_quality` (i32): 0 in every census record (226,278 with 0x95; 5,675 with 0x96; 0 nonbaseline files).
- The text-preset key `interpQuality` is parsed in `0140910000.c` (around line 11901). Its only decoded token is `normal`, which stores 0 at `+0xd8` (program/global scope, not the group). `interpQualityOffline` is a separate key (line 11374). The remaining enum tokens are not recovered in this function, so the binary group enum is still unrecovered.
- Runtime resampler selection is not recovered from the decompile yet.

**Zero rule (corrected 2026-10-22):** the four decompiled read sites of `+0x21b70` (`0140820000.c`, `0140580000.c`, `0140570000.c`, `01410a0000.c`) use the value directly, with only the global-override flag as a branch. None tests for zero. The "zero uses global policy" note in `KONTRA_OBJECTS.md` is therefore unverified.

## Evidence confidence

- Format fields, loop and reverse behaviour, offsets, voice generations: **high** (repo code and tests; not compared with Kontakt output).
- DFD preload unit (bytes), default (61440), global override and 6 KiB window granularity: **high** for the decompiled constants; **medium** for the window formula until the channel argument of `FUN_14082a530` is traced.
- Runtime mode 4 mapping (stored DFD mode 3 to runtime 4): **unrecovered**; `FUN_14074688d` only shows mode 0 becoming 4 when a flag at `+0x2a96a` is clear.
- Binary group interpolation enum and kernels: **unrecovered**. Corpus value is 0 everywhere, so no measured consumer.
- Interpolation and DFD streaming parity with Kontakt output: **none** (no native render compared).

## Round 2026-10-22: mode-4 path, HQI, Exact.nki, window wiring status

**Mode-4 path (runtime source mode at `+0xba8`).** Stored group mode 3 is DFD; the window routine and the DFD menu id (4, `0140610000.c` source menu) both use runtime 4. Two set points found:
- `0140760000.c:19175`: a mode-0 (Sampler) source becomes 4 when its resident flag `+0x2a96a` is clear and `+0x120 < 0`. So a non-resident Sampler group streams like DFD.
- `0140760000.c:17182`: mode 4 is set unconditionally on the branch guarded by `FUN_14051b400(7)`, which returns the constant 1 (`0140510000.c:8772`). The branch runs when `+0xfc >= 0`.
- The store that maps stored mode 3 to runtime 4 at group load was not located in the decompile. The 3 → 4 mapping is therefore inferred from the window gate and the menu id, not seen directly.

**HQI (group `interp_quality`).** Enum recovered from the group editor (`0140610000.c:3785`, shown for file version 0xd): 0 Standard, 1 High, 2 Perfect. Native dumps show `interp_quality = 0` in every surveyed group.
- Kernel per value: **not recovered.** The decompile has no named resampler kernel; constant searches (Blackman, Kaiser, sinc) found nothing usable.
- Render vector: **blocked.** Kontakt's Lua API has no getter or setter for HQI (only `get_group_name/pan/playback_mode/start_options/tune/volume` are used in `dump.lua`). Setting HQI needs the GUI group editor, which the harness does not script. No Kontakt Lua API reference is on disk.

**`Exact.nki` (correction).** Earlier notes said native Kontakt loads this file. It does not: the oracle reports `load_instrument failed` for the same input (`oracle/set2.jsonl`). KONTRA's rejection (`ambiguous monolith sample name`, `samples.rs:185`) is consistent with native. The `diff.py` mismatch was a difference in error objects, not in loaded state. Not a loader bug.

**DFD window wiring (status: not wired).** Blockers, in order:
1. The window needs disk bytes per frame (bits × channels). The IR asset has no bit depth; `Samples::frames` (`samples.rs`) reads headers but has no production caller, so a header path would have to be added.
2. A group-level DFD preload field does not exist in the IR; `ir::kontakt::Group` has `source.mode` only.
3. `start_ranges_geometry` (`stream.rs`) has no channel count (`Pcm` does not expose it), so the floor cannot be computed there yet.
When all three are in place the floor is `frames = ceil(window_bytes / bytes_per_frame)`, added to `frames` in `start_ranges_geometry`, default-off behind a `StreamPolicy` flag until a render vector decides it.

**Vectors.** `docs/re/engine/vectors/kontakt-native-playback-modes.tsv` (23 groups); `tests/native_oracle.rs::stored_source_modes_match_kontakt_playback_modes`.
