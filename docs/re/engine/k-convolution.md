# Kontakt Convolution (0x16) and Cabinet IR handling

Status: **partial.** The Kontakt 8 decompile is ready (`source/kontakt8/READY`); the rows below marked *decomp* were confirmed there (`source/kontakt8/01419b0000.c`, `0140ab0000.c`). The IR resize engine is not yet reconstructed, so the unit is not ready for integration. Public NI manual pages return HTTP 403, so parameter ranges come from repo evidence and the decompile only.

## Decompile confirmation (decomp)

- **Size setter `FUN_1419b6750`** (`01419b0000.c`): accepts early and late sizes only when both lie in `[DAT_14470bad0, DAT_14470bad8]` (lower bound 0.5); otherwise returns error `0x4c4b43` and nothing is stored. `param4 == 1` stores the late pair at `+0x74`/`+0x7c` and marks dirty bits `0x168`; otherwise the early pair goes to `+0x78`/`+0x80`, dirty bits `0x154`. The upper bound constant `DAT_14470bad8` must still be read from a dump; the repo's `1.5` is not yet confirmed by the decompile.
- **Preparation `FUN_1419b2bc0`**: a dirty-bit word at `+0x48` selects work. `0x4` runs the early resize `FUN_1419b3450(early_size / rate_ratio)`, `0x8` the late one, `0x10`/`0x20` the early/late filters `FUN_1419b27a0`, bit `0x1` rebuilds the head and crossover `FUN_1419b2870`. When the resize ratio equals `DAT_14470bad4` (1.0) the resize is an identity copy with zero padding to the block size, which matches the Rust path (`size == 1` skips the note).
- **Reverse**: swaps each channel's first `len/2` pairs over the IR length (`param_1[3]`), so it is a full frame reversal.
- **Early/late composition `FUN_1419b1e30` and envelope `FUN_1419b2320`** are the functions already reproduced in `shape.rs` (cos² crossfade, eight-knot envelope); their decompile matches the constants recorded there.
- **Resize `FUN_1419b3450`** (1535 bytes): a streaming loop that feeds 256-position steps into a stretch object at `+0x160` (virtual calls: process, query, flush). The object is built by `FUN_1419baf70`, whose vtable is **`CElastiqueMC`** (Zplane Élastique time-stretch). The Élastique algorithm is proprietary; it is not reproduced here and no bit-exact port is claimed.
- **Stretch engine constants (confirmed from the binary, `re-binaries/Kontakt8.exe`)**: size range bounds `0.5`, `1.0` (identity), `1.5` (so the Rust `0.5..=1.5` gate is correct); the `FUN_1419be690` setup sizes its transform as `2^floor(log2(N+1))` times a multiplier (`1/ln 2` = 1.4426950 is the stored constant at `0x144f38918`, `2.0` at `0x144714994`). Its frame, hop and window choices come from inside Élastique and are not implemented.
- **Parity options for IR size**: (a) license Élastique and port the engine behind a trait in `sampler-core`; (b) keep an independent stretch and gate it on a measured tolerance against Kontakt renders (same route as the Time Machine Pro WSOLA stand-in). Option (b) is the only one available without a licence; until it is measured, the `IR size` note stays `NotModeled`.
Remaining gaps after this confirmation: resize engine (Élastique, see above), S.Rate inner-rate preparation (`+0x354`), unset crossover default (wrapper `FUN_140ab5e00`), latency reporting, true-stereo routing. Parity is not claimed for any of them.

## Evidence used

- Saved record and wrapper preparation: `crates/sampler-kontakt/src/effects/mod.rs` (0x16 reader, ~l.427-449), `crates/sampler-kontakt/src/effects/space.rs` (`convolution`).
- Recovered IR preparation sub-laws (Auto Gain, filters, composition, envelope, resize entry points): `docs/re/kontakt-reverb-convolution.md`.
- Earlier blocked trace and the CConvolver lead addresses: `docs/re/kontakt-convolution.md`.
- Shared shaping (reverse, Butterworth early/late filters, cos² composition, 8-knot envelope, predelay truncation, Time window): `crates/sampler-core/src/dsp/space/shape.rs`.
- Partitioned zero-latency convolver: `crates/sampler-core/src/dsp/space/convolver.rs`.
- Resampler pair (Kaiser, 16 taps): `crates/sampler-core/src/dsp/space/resample.rs`.

## Module parameters (saved fields)

| Field | Stored as | Meaning in the engine | Status |
|---|---|---|---|
| `early[0]`, `late[0]` | f32 length ratio | IR size; only `0.5..=1.5` accepted, else 1 | Unmodeled (time-stretch engine, `0x1419b3450`) |
| `early[1..]`, `late[1..]` | f32 Hz | Low cut and high cut per part | Modeled (Butterworth, bypass rules as recovered) |
| `xpoint` | f32 fraction | Early/late crossover; negative = unset | Modeled when set; unset default not read |
| `predelay_ms` | f32 ms | Predelay after Auto Gain, truncated to frames | Modeled |
| `decimation` | f32 | S.Rate: wet path at `(int)(rate/S.Rate)` | Reported unmodeled (inner-rate IR preparation untraced) |
| `flags[0]` | bool | Reverse | Modeled |
| `flags[1]` | bool | Auto Gain | Modeled (`FUN_1419b2bc0`, see docs/re/kontakt-reverb-convolution.md) |
| `flags[2]` | bool | Preserve Length | **Now reported unmodeled** (was silently dropped) |
| `flags[3]` | bool | Bypass latency compensation | **Now reported unmodeled** (was silently dropped) |
| `flags[4]` | bool | Volume envelope on | Modeled (8 knots, amplitude-interpolated) |
| `curve_x`, `curve_db` | lists | Envelope knots | Modeled when exactly 8 |
| block size (4 bytes after `decimation`) | i32 | Read and discarded | Meaning not recovered; no field |
| `ir_index` | i32 | Other-files IR reference | Modeled through the impulse loader |

## Gap table

| Area | Shared engine already covers | Missing or unverified |
|---|---|---|
| Partitioning | Zero-latency non-uniform partitioned FFT convolver, head plus growing tail stages, silent-block skipping | Kontakt's own partition/block size is not mapped; the stored integer is discarded |
| IR trimming | `time` keep-fraction with half-cosine fade (UVI law); Kontakt has no trim control in this record | Kontakt's trimming, if any, beyond the envelope and filters, not extracted |
| IR stretch (size) | Nothing; size outside range falls back to 1 | STFT time-stretch engine `0x1419b3450` (setup `0x1419be690`, `0x1419baf70..`) not extracted; Preserve Length depends on it |
| EQ / filters | Butterworth early/late passes, composition | Unset-crossover default position not read |
| Latency | Zero-latency convolver | Latency report and the "bypass latency compensation" behaviour not modeled |
| Resampling | Kaiser integer-ratio pair; IR converted to host rate through its file rate | S.Rate inner-rate preparation (IR-rate rescale under flag `+0x354`) not traced |
| Cabinet IR handling | Stereo IR shaping only | No cabinet-specific path found in the 0x16 record; nothing added |
| True stereo / surround | Stereo kernel (left/right IR, `frames[..][0..2]`) | Multichannel IR channel routing is outside the stereo kernel; no evidence for the mapping |

## What changed in this unit

- `effects/space.rs` `convolution`: Preserve Length and latency-compensation bypass, when set, now produce `NotModeled` notes instead of being dropped. The test `convolution_reports_unmodeled_length_and_latency_flags` pins both notes and the silent off state.

## Not done, and why

- No convolution law was implemented: every missing item (IR stretch, S.Rate preparation, crossover default, latency, true-stereo routing) needs the decompiled source, which is not ready. Inventing these laws would be fabrication.
- The partition and block size is read and dropped by the reader; a field would change the serialized cache, so it stays as documented.
- Off-state of Preserve Length and latency bypass is silent because its native default is not recovered; a note on off would need that default first.

## Planned work (after READY)

1. Extract `0x1419b3450` (IR resize) and the STFT engine setup; record the size law and how Preserve Length gates it; implement it in `sampler-core` `dsp/space/shape.rs` as a config law, then remove the IR size note.
2. Trace the S.Rate inner-rate IR preparation (`+0x354`) and the unset crossover default; add golden tests.
3. Trace the latency report for convolution and bypass; decide whether the host latency field is affected.
4. Check the true-stereo / multichannel IR route in the CConvolver channel setup before claiming any surround behaviour.

Confidence: high that the flags were silently dropped before this change and are now diagnosed; high that every item in the gap table is unverified where marked; low on the exact native defaults.
