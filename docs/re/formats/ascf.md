# Ample `.ascf` (`aswv`) sample container

Black-box analysis of the 482 `.ascf` files under `/data/main/Libraries/Ample`
(AGM 162, AGVC 170, AMH 150; 20.4 GB). Every claim below was checked against
those files by `crates/sampler-ample` (`installed_libraries_all_load`).

## Verified: container

A file is a run of blocks, back to back, with no index, footer or padding.
Block `k+1` starts exactly where block `k`'s PCM ends. Scanning all 482 files
this way ends exactly at end-of-file in every case: 17,101 blocks total, 4 to
174 per file.

| Offset | Size | Field | Notes |
| --- | --- | --- | --- |
| 0 | 4 | magic `aswv` | ASCII |
| 4 | 14 | `PleaseDontHack` | constant marker, not a cipher |
| 18 | 1 | channels | `1` (11,826 blocks) or `2` (5,275) |
| 19 | 1 | bytes per sample | always `3` (24-bit) |
| 20 | 4 | frames, u32 LE | sample frames per channel |
| 24 | 4 | zero | always 0 in observed headers |
| 28 | 4 | tag | ASCII `boom` in every block |
| 32 | `frames * channels * 3` | PCM | see below |

Next block offset = `offset + 32 + frames * channels * 3`.

The header is 32 bytes. An earlier reading put the PCM at byte 38; that was
wrong. Byte 32 is the first frame, and its zero value is real silence.

## Verified: codec

PCM is uncompressed, signed, little-endian 24-bit, interleaved by channel
(`L0 R0 L1 R1 ...` for stereo). No compression, no block-level keystream and
no per-block checksum were found. The samples are smooth, use the full 24-bit
range (peak about 1.9e5 in the sampled blocks, well within range), and the
low byte is not systematically zero. Decoding to `f32` is `value / 2^23`.

## Observed, not decoded

- **Sample rate.** Not in the container. Spectral checks gave no clear answer:
  a few files have a little energy above 11 kHz, most have almost none. The
  rate has to come from the library's configuration. `sampler-ample` takes the
  rate from the caller and does not guess one.
- **Block length.** Frame counts vary (for example 22050, 57330, 705600,
  815850). They are not multiples of one fixed unit.
- **Tag `boom`.** Constant in all 17,101 blocks, so it carries no information.

## Not in the container: key, velocity, round-robin and loops

The container does not say which key, velocity layer, round robin or loop
point a block belongs to. That mapping lives in two places:

1. **`*_DBConfig.cf`** (one per library). Its content is a custom
   base64-like encoding of the config, not plain text. The `aswv` marker
   suggests it is meant to resist inspection. Per the license rule this is an
   encoded/protected container, so I **stopped** and did not decode it. Key,
   velocity, RR and loop mapping for every instrument is therefore blocked
   until the user decides how this file should be handled.
2. **`attack-*.dat`** (plain text, one per library, `AGVC`, `AMH`). Each line
   is `<patch>-<note>-<layer>-<rr>|<value>`. Observed structure:
   - AGVCBg: 1385 lines, 18 patches, notes 40 to 86, layers 1 to 4, RR 1 to 4.
   - AGVCNk: 1373 lines, same key scheme.
   - AMHBg: 1917 lines, notes 36 to 84, layers 1 to 4, RR 1 to 8.
   - `<value>` ranges from 277 to 3026 frames (AGVC) and 815 to 2867 (AMH).
   The key scheme is clear. The meaning of `<value>` is an assumption (most
   likely an attack or loop-start frame) and has **not** been verified. The
   line count does not match the block count (6,526 blocks in AGVC), so these
   lines do not map one-to-one onto blocks.

What is still needed to play one Ample instrument: the decoded `.cf` tables
(key range, root key, velocity range, RR, loop start/end per block), plus the
sample rate. Both come from the config, so they are the next RE step.

## Reader

`crates/sampler-ample`: `scan` lists blocks by reading only the 32-byte
headers, `decode` returns stereo `f32` frames in the sampler's
`Decoded`-compatible shape (mono duplicated). Errors cover bad magic, truncated
data, and unknown layout (anything other than 1 or 2 channels at 3 bytes).
Tests: synthetic tiling and decode, error cases, all 482 installed files scan
and tile exactly, and a known-value decode of `AGMCm-1.ascf` block 0.

Zone mapping into `sampler_ir` is deliberately not written. It needs the
decoded `.cf` tables above, and a mapping without them would be a guess.
