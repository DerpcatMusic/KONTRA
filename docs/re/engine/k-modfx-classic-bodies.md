# Kontakt 8 classic modulation bodies: Flair, Phasis, Vibrato/Chorus

Status: **mapped, not ported.** This unit locates the audio process bodies of
Flair (0x5c), Phasis (0x5b) and Vibrato/Chorus (0x69) in the Kontakt 8.13.1
image and records the shared frame they run in. No DSP law for these three is
recovered yet, so nothing is implemented in `sampler-core` and no parity row
changes. Companion to [k-modfx-classic.md](k-modfx-classic.md), whose
"audio dispatch records" section is superseded for these three modules.

Evidence base: decompile bucket files `source/kontakt8/01419*.c`,
`0140990000.c`, `0140aa0000.c` (Ghidra export, READY), pefile vtable dumps
(`scratch/k-modfx-classic/vt.py`) and RIP-relative `lea` targets read with
capstone (`scratch/k-modfx-classic/bodies/dis.py`). Addresses are image VAs.

## Where the audio lives

Each module's DSP object is a C++ class in `ni::effektrig::dsp`. Its ctor
writes the derived vtable at offset 0 and a second base vtable at offset 8.
The audio body is a plain virtual, not a record-table entry.

| Module | DSP class | Ctor | Primary vtable | Audio body (slot 7, +0x38) | Core (slot 19, +0x98) |
| --- | --- | --- | --- | --- | --- |
| Flair | `Flair` | `FUN_14195cd70` | `0x1476999d0` (2nd base `0x147699a88`) | `FUN_14195f600`, 3417 B | `_guard_check_icall` (no-op) |
| Phasis | `Phasis` | `FUN_14197aff0` | `0x14769bf80` | `FUN_14197cfe0`, 1857 B | `_guard_check_icall` (no-op) |
| Vibrato/Chorus | `hammondvibratochorus::HammondVibratoChorus` | `FUN_14196a550` | `0x14769a6e0` | `FUN_14099eea0`, 340 B (wrapper) | `FUN_14196d230`, 1133 B |

- Flair and Phasis: the body sits directly in slot 7. Slot 19 is a no-op in
  both, so the earlier note that Flair's `+0x98` resolves to a getter or no-op
  is confirmed, and the body is reached through slot 7 instead.
- Vibrato/Chorus: slot 7 is a generic wrapper. It resamples to the internal
  rate when the flag at `+0x57c` is set (`FUN_141ee2160`), calls the core
  through `*(this)+0x98` (slot 19 = `FUN_14196d230`), and resamples back.
  The internal frame count is a rate-ratio conversion of `n` using the fields
  at byte `+0x360`, `+0x35c`, `+0x3f4` and the float at `+0x3f8` (passed to the
  resampler as its rate).
- The four-argument signature in all three is `(this, in_ptr_array, out_ptr_array, frames)`.
  Flair and Phasis write their outputs through `FUN_141ee2160` at the end.

## Shared frame (Flair and Phasis)

Both bodies run the same outer steps. Read in this order:

1. Pre-stage. Flair calls `FUN_14195db30(this+0x1050, n)`, and when byte
   `+0x1280` is set also `FUN_141ee2160` on that sub-object. Phasis calls
   `FUN_14195db30(this+0x3c8, n)` and `FUN_141ee2160(this+0x3c8, in, n)`
   unconditionally. The returned count is the loop bound.
2. Per-frame loop over `frames` (Flair: `local_288 < local_284`; Phasis:
   `uVar33 < uVar32`). Inside it:
   - Two-channel pre-stage at `+0xe30..0xec8` (Flair): one-pole-like
     recursions with state pairs at `+0xeb0/+0xeb8` and `+0xec0/+0xec8`.
   - Four-lane ramp bank at `+0x478` (phase) with `+0x460` (base),
     `+0x418` (weight) and gains at `+0x4a0..+0x4ac`. Lane increments are the
     constant `1.0` (float) from `0x144714b70`, so in this loop the bank is a
     per-sample counter ramp. Any rate scaling happens outside it (not traced).
     The ramp feeds `+0x400`.
   - Voice loop (Flair): a pointer walk calling `FUN_14195d9b0` per voice. The
     voice table is not traced yet. Phasis: `FUN_14197e6e0` and
     `FUN_14197dad0` per frame (allpass per the earlier note, not verified),
     with flag masks `+0x750 & 0x666` and `0x333`
     selecting `FUN_14197ef70` and `FUN_14197eaf0`.
   - Output mix into the `longlong*` out array.
3. Post: `FUN_141ee2160(sub, &buf, frames, out)` when oversampling is on.

Vibrato/Chorus core (`FUN_14196d230`):

- Per frame: a four-lane ramp bank at `+0x2f8` (same constant increments), then
  seven weighted voice objects at `this+0x98 + i*0x40` (`i = 0..6`, slot 2 `+0x10`
  called when weight `+0x298[i] != 0`), outputs at `+0x280`, scaled by `+0x320`.
- Input gain is ramped toward `+0x5d4` with coefficient `+0x5cc`, threshold `+0x5d8`
  and a latch at `+0x5dc`. The absolute-value mask is `0x144714b90`.
- Mid = `0.5 * (L + R)` (constant `0x14470bad0`). Two one-pole stages with state
  pairs `+0x5f8..+0x610` and `+0x618..+0x624`.
- Ladder: `FUN_14196b9e0(this+0x790)` when `+0x5c0 < 3`, else
  `FUN_14196c600(this+0x17b0)`. Both are about 3 KB. The earlier note calls
  them the wave-digital LC ladder; that is not verified here. Combine with
  `FUN_141969370`.
- Output: dry/wet at `+0x58` (`dry = 1 - mix`), written to both channels.

## Constants read from the image

| VA | Meaning | Value |
| --- | --- | --- |
| `0x144714b70` | per-lane ramp increment (f32 x4) | `1.0` |
| `0x14470bad0` | mid factor | `0.5` (lane 0) |
| `0x14470bad4` | unity | `1.0` |
| `0x144714988` | f64 unity used in comparisons | `1.0` |
| `0x144714998` | f64 | `2.0` |
| `0x1447149a8` | f64 | `4.0` |
| `0x144714b90` | absolute-value mask | NaN as f32 (bit mask) |

Random start phase (Flair ctor): a process-global MINSTD (`x * 48271 mod 2^31-1`,
seed from `std::random_device`, 0 mapped to 1) draws the start value per
instance. Start phases are therefore not reproducible across runs, and a golden
test must inject the phase.

## Gap table

| Module | Engine status | Found | Missing | Blocker |
| --- | --- | --- | --- | --- |
| Flair (0x5c) | unsupported (reported) | body, frame, ramp bank, pre-filter, RNG | voice table and `FUN_14195d9b0` internals, pitch note writer `+0x40`, `FUN_14195db30` and `FUN_141ee2160` bodies, output weights | ~6 KB of helpers to read; no audio vectors |
| Phasis (0x5b) | unsupported (reported) | body, frame, allpass stage `FUN_14197dad0`, per-frame `FUN_14197e6e0`, masks `+0x750` | `FUN_14197cd00`, `FUN_14197d910` (notch table at `+0x9b4`), `FUN_14197ef70/eaf0`, oversampling | no Phasis law yet; TSV notch table only |
| Vibrato/Chorus (0x69) | unsupported (reported) | wrapper, core, two trapezoidal stages, dry/wet, LC ladder entry points | LC ladder bodies `FUN_14196b9e0` and `FUN_14196c600` (about 6 KB), `FUN_141969370`, `FUN_1419614e0` (1282 B), `FUN_141961030` | ladder equations not extracted |

Shared helpers still unread (sizes from the export): `FUN_14195db30` 137 B,
`FUN_141ee2160` 372 B (resampler), `FUN_14195d9b0` 347 B, `FUN_141961db0` 423 B,
`FUN_141961f60` 562 B, `FUN_141960af0` 622 B, `FUN_1419622b0` 185 B,
`FUN_14197cd00` 721 B, `FUN_14197d910` 436 B.

## Next step

Read the ladder bodies and the resampler first: they gate both Vibrato/Chorus
and the Flair/Phasis oversampling path. Then port the shared frame as one
config-driven core in `sampler-core` with a recorded vector per module, once an
oracle capture exists (`tools/kontakt-oracle`, one session). Until then the
three modules stay unsupported and diagnosed.
