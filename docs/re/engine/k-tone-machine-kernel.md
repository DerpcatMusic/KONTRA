# Kontakt 8 Tone Machine render kernel (vtable slot 23)

Status: **kernel and scheduler ported, not wired, no native parity.** The render
kernel, its grain scheduler and the mark search are ported in
`crates/sampler-core/src/source/tone_machine.rs` with decompile-derived unit tests.
The source pass, the mark table builder, the parameter setters and the caller that
drives the render are not recovered, so the default render cannot be produced and
`StretchMode::ToneMachine` still runs the shared stand-in engine. Matrix row:
`docs/re/engine/PARITY_MATRIX.md` (Tone Machine, stays `missing`).

Scope: Kontakt 8 only. Addresses are Kontakt 8.exe VAs, read from the decompile
export (`source/kontakt8/0140ce0000.c`, `0140cd0000.c`, `0140860000.c`, `0140880000.c`,
`0140870000.c`, `0140530000.c`). Constants are from `docs/re/kontakt-rdata/constants.tsv`.
No C is pasted; the laws below are restated.

## Functions

| Native | Size | Role | Port |
| --- | --- | --- | --- |
| `BToneMachine` slot 23, `0x140ce3ff0` | 2197 B | render kernel, branch `DAT_14a2a5918 < 6` | `Tone::render` |
| `0x140cda5d0` (`0140cd0000.c:5474`) | 1073 B | grain scheduler, called once per control block | `Tone::schedule` |
| `0x140ce5120` (`0140ce0000.c:2966`) | 483 B | mark search (index at or after a position) | `Tone::search` |
| `0x14053cb70` (`0140530000.c:8503`) | 109 B | mark value at index, clamped | `Tone::value` |
| `0x140883ce0` / `0x140883f10` | 7 / 68 B | source channel count (`+0x1790`) / block frames (`+0x1794`) | `Source::channels` |
| `0x14086e030` (`0140860000.c:8162`) | 2845 B | source pass: reads frames at a position into buffers | `Source::read` (trait, not ported) |
| `0x140cd94c0` (`0140cd0000.c:4551`) | 244 B | constructor | `Tone::new` |
| `0x140cdaa10` (slot 22) / `0x140cdaea0` (slot 18) | 89 / 173 B | parameter setters (not read yet) | not ported |

Object: `0x420` bytes (`0x140d20640`), vtable `0x144fd0bb0`. Constructor state: `+0x4c = 1.0`,
`+0xbc = -1`, `+0xd0 = 1.0`, the rest zero. The kernel resets `+0x3c = 0` and `+0xd0 = 1.0` at
every call, and `+0xe4 = +0x74 - +0xbc` when `+0xe4 > 0x7ffffffe`.

## Object fields (native offsets)

| Offset | Type | Meaning (as used) |
| --- | --- | --- |
| `+0x30` | f64 | rate ratio (set elsewhere) |
| `+0x38` | f32 | speed (set elsewhere) |
| `+0x40`, `+0xc0`, `+0xc8` | pointer | per-32-frame control curves: level, divisor, ratio (null = not set) |
| `+0x3c` / `+0x48` / `+0x4c` | f32 | level (clamped), last level, cached `4^level` |
| `+0x50` | f32 | ratio; scheduler clamps it to `[0, 1]` |
| `+0x58` | f64 | time accumulator in frames |
| `+0x60` | i32 | current mark index |
| `+0x64` | i32 | end time |
| `+0x68` / `+0x6c` | i32 | loop start / loop end (negative = no loop) |
| `+0x70` / `+0x71` / `+0xb4` | u8 | loop hit / head-0 fade suppressed / done |
| `+0xbc` | i32 | period in frames (`-1` before analysis) |
| `+0xd0` / `+0xd4` | f32 | divisor (curve `+0xc0`, floor 0.001) / `period / divisor` |
| `+0xd8` / `+0xdc` | f32 | ramp (0 to 1) / ramp scale |
| `+0xe0` / `+0xe4` | i32 | last mark / reset base |
| `+0x74`, `+0x78` | i32[2] | direct read position per head |
| `+0x84`, `+0x88` | i32[2] | crossfade read position per head |
| `+0x94`, `+0x98` | f32[2] | direct length per head (frames) |
| `+0x9c`, `+0xa0` | f32[2] | crossfade length per head (frames) |
| `+0xa4`, `+0xa8` | f32[2] | window phase per head |
| `+0xac`, `+0xb0` | f32[2] | phase increment per head (`1 / crossfade length`) |
| `+0x7c`, `+0x8c` | i32[2] | saved and restored unchanged by the kernel (not modelled) |

The offsets `+0x74..+0xb0` are two-element arrays: index 0 at the first offset, index 1 at
+4. The scheduler writes both heads.

## Kernel (`0x140ce3ff0`)

Per call, the kernel loops over control blocks until the frame budget `*param_6` is spent.
Each block:

1. Samples the three curves at `pos >> 5` (positions advance by the frames produced). Level
   is clamped to `[-1.5, 1.5]` (constants `DAT_144fd0dd8`, `DAT_14470bad8`). Divisor is
   floored at `0.001` (`DAT_1447148a4`). Ratio is copied.
2. For each head `h`:
   - direct read: `min(budget, len[h])` frames from `pos[h]` into the output (`h = 0`) or the
     lanes (`h = 1`); `pos[h]` moves and `len[h]` drops by the frames produced;
   - if `xlen[h] < 1`, the head is done (direct only);
   - else crossfade read: `min(budget, xlen[h])` frames from `xpos[h]` into the taps; then for
     each frame `j` and channel `k`: `dst[k][j] = w * dst[k][j] + (1 - w) * tap[k][j] * fade`,
     where `w = window(2 * phase[h])` and `fade = 0` only for head 0 when `+0x71` is set. The
     phase then advances by `inc[h]`.
3. Mix stage: the ramp step is `speed * rate * gain * ramp_scale` when it is at least `1e-6`
   in magnitude (`DAT_14471489c`), else 0. For each frame, output = `(1 - ramp) * output +
   ramp * lane`, and the ramp advances by the step, capped at 1.
4. Time: `+0x58 += speed * rate * gain * frames` (f32 product, f64 sum).
5. Budget: `*frames -= frames`; if it is below 1 the call returns (see deviations).
6. Calls the scheduler. A `false` return sets `+0xb4` (done).

Window (`fVar18` in the native): for `x = 2 * phase`, `w = x^3 / 2` for `x <= 1`, and
`w = 1 - (2 - x)^3 / 2` for `x > 1`. Constants `0.5` (`DAT_14470bad0`) and `2.0`
(`DAT_144714994`). This is the cubic in-out window; it is the same window the Time Machine 1
scheduler uses (`time_machine.rs`).

Gain: `gain = 4^level` (`DAT_1447149a8 = 4.0`), recomputed only when the level changes.

## Scheduler (`0x140cda5d0`)

Called once per control block. With `base = pos[0] - reset`:

- If the loop is set (`+0x68`, `+0x6c` both `>= 0`) and `loop_start <= time`, the loop-hit flag
  is set and `time = loop_end`.
- It returns `false` (finished) when `end <= time`, when `mark` is the last mark, or when the
  mark value at `mark` is above `end`.
- Otherwise: `mark = search(time, mark, nearest)`. Head 0 gets `pos[0] = min(value(mark), end)`,
  `reset = pos[0]`, `xpos[0] = pos[0] + base`. `unit = period / divisor`. The walk forward
  in steps of `speed * rate * gain * unit` (only when at least `1e-5`, `DAT_144f38790`)
  finds the time `t_end` at which the mark changes. Head 1 gets `pos[1]` and `xpos[1]` from
  that mark. When the mark changed, `ramp_scale = 1 / (t_end - time)` and `ramp = 0`.
- Lengths per head: `len[h] = unit`, clamped so the direct read does not pass the table head;
  `xlen[h] = unit`, clamped the same way, scaled by `clamp(ratio, 0, 1)`, floored at 1;
  `phase[h] = 0`; `inc[h] = 1 / xlen[h]`.

Mark table (`+0x20`): `head` is the first dword (compared against positions); the mark
values are the dwords at `+8` of 12-byte entries, with the count at `+4`. A missing table
behaves as an empty table (value 0, last index -1).

Mark search (`0x140ce5120`): returns the first index whose value is at or after `pos`,
searching backwards from `start` and then forwards. With `nearest` false it may choose the
following mark when that one is closer. At the end it returns the last index when the loop
is not set, else 0.

## Deviations from the native, and why

- The native loop is `do ... while (true)` with an empty `if (*param_6 < 1) { }`. The only
  exit it can have is that check, so the port returns there. This is an inference (the
  export dropped the statement), not a read.
- If a call produces zero frames and the scheduler reports done, the port returns. The native
  would spin. The port also `assert`s the block is at most `CAP` (4096) frames.
- The source pass takes the frame budget as an in/out count (`remaining`), inferred from
  `iVar13 - local_2c4` in the kernel. The cursor `local_2a0` (-1 sentinel) is not modelled.
- The taps offset is reset per control block. The native advances it, but the taps are only
  read at the offset where this block wrote them, so the effect is not observable.

## Gap table

| Piece | Status |
| --- | --- |
| Render kernel, two heads, window, ramp, time (slot 23) | ported, decompile-derived unit tests |
| Scheduler and mark search (`0x140cda5d0`, `0x140ce5120`, `0x14053cb70`) | ported, not tested against a native mark table |
| Source pass `0x14086e030` (interpolation, page refill `0x140828500`, `0x140870e30`, `0x1408f5110`) | **missing**; `Source` trait only |
| Mark table builder (what fills `+0x20`) | **missing** |
| Parameter setters (slots 18, 22) and the render caller (which sets `+0x30`, `+0x38`, `+0xbc`, curves) | **missing** |
| Record mapping: `formantShift`, `grainLength`, `dcFilter`, `hiQuality` to the object | **missing**; the loader still maps Tone onto the TM1 record fields, which is unverified |
| DC filter, formant, `hiQuality` laws | **missing** |
| Wiring into `StretchMode::ToneMachine` | **not done**: the default render cannot be produced yet |

## Oracle (native Tone, defaults)

`/data/dev/workspace/PluginArtifacts/kontra-oracle/tone-machine/tone_default.wav`, probe
`tone_default_probe.lua` (both next to it). Float32, 2 channels, 48 kHz, 6.112 s.
Stimulus: a 220 Hz sine, amplitude 0.5, 2.0 s, root key 60. The trigger is MIDI note 36 at
about 1.0 s (ignored), then note 60 about 100 ms later, with no note-off.

Measured (plain-Python pass over the WAV, `nice -n 19`): silence until about 1.00 s. Peak 0.037 in the first
50 ms, then 0.19 at 1.05 to 1.10 s, decaying to 0.005 at 1.40 s and silent from 1.50 s
onward. So the native Tone output is a single burst of about 0.45 s, not a sustained
220 Hz tone, even though the source sine lasts 2 s. The alignment between the probe timeline
and the recording is not established, so no sample-level comparison is claimed.

## Open items (in order)

1. Read the source pass `0x14086e030` for the resident case: the four taps, the interpolation
   weights at integer and fractional positions, and what `0x140870e30` returns.
2. Find what fills the mark table at `+0x20` (analysis at load or in the source), and what
   `0x140cdaea0` and `0x140cdaa10` write to `+0x30`, `+0x38`, `+0xbc`, `+0x64`, `+0x68`, `+0x6c`.
3. Find the render caller and the curve sources for `+0x40`, `+0xc0`, `+0xc8`.
4. Map the saved-record names to the object, then wire the kernel into `StretchMode::ToneMachine`
   and compare against the oracle with a stated tolerance.
