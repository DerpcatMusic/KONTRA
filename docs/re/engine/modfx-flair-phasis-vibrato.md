# Kontakt 8 modulation FX: Flair, Phasis, Vibrato/Chorus (unit doc)

Status: **not ported.** No Rust DSP law for these three modules is recovered
yet, so nothing is added to `sampler-core` and no parity row changes status.
They stay unsupported and report `NotModeled` (`unproved` in
`crates/sampler-kontakt/src/effects/timemod.rs`). Native vectors cannot be
written until the laws are recovered and an oracle capture exists.

Companion docs: [k-modfx-classic.md](k-modfx-classic.md) (module list, saved
fields, measurements) and [k-modfx-classic-bodies.md](k-modfx-classic-bodies.md)
(audio body locations). This doc adds what the decompile text shows for the
kernels and the parameter writer. Addresses are image VAs.

Decompile source: `/data/dev/workspace/Research/KONTRA/source/kontakt8/`
(`READY` present). Files are 64 KiB buckets; headers `// ==== <addr> <name> size=N`.
Functions marked **read** were read in full in this unit; the rest are only
located.

## Flair (0x5c)

- Audio body `FUN_14195f600` (3417 B, `0141950000.c`): **located**, only its
  opening read. The body runs the optional pre-stage, then a per-sample loop.
- Pre-stage: if byte `+0x1280` is set, `FUN_14195db30(this+0x1050, n)` returns
  the loop count and `FUN_141ee2160` runs on the same sub-object. Otherwise the
  count is `n`. (`FUN_14195db30` 137 B and `FUN_141ee2160` 372 B are not read.)
- Per-sample two-channel recursion (**read**): input from the two channel
  pointers, state pairs at `+0xeb0/+0xeb8` and `+0xec0/+0xec8`, coefficients at
  `+0xe90/+0xe98` (feedback-like) and `+0xea0/+0xea8` (gains), mix taps at
  `+0xe30/+0xe38` and `+0xe40/+0xe48`. The two channels are processed as a pair,
  with the same trapezoidal-style doubling as Phasis.
- Four-lane ramp bank (**read**): gate byte `+0x495`. Phase `+0x478` advances
  by lane increments from the constant `1.0` (4 x f32, `0x144714b70`), base
  `+0x460`, weight `+0x418`, scalar gain `+0x4a0`, lane gains `+0x4a4/+0x4a8/+0x4ac`.
  Output goes to `+0x400`. This is a per-sample counter ramp, as the earlier note said.
- Still missing: the voice loop (`FUN_14195d9b0`, 347 B, located only), the
  pitch note writer (`+0x40`), the start-phase writer, and the resampler body
  (`FUN_141ee2160`, shared with Phasis and Vibrato).
- Random start phase (from the ctor, per earlier note, not re-read here): a
  process-global MINSTD (`x * 48271 mod 2^31-1`, seeded from `random_device`,
  0 mapped to 1). A golden test must inject the phase.

## Phasis (0x5b)

- Audio body `FUN_14197cfe0` (1857 B, `0141970000.c`): **located**, not read.
- Allpass stage kernel `FUN_14197dad0` (788 B): **read**. Structure:
  - Calls the coefficient helper `FUN_14197bc10` first, which computes a 2x2
    state-space block per channel (`param_2`, `param_3` are two-element state
    vectors). Helper outputs are divided by a 2-lane `divpd` denominator built
    from `+0x930/+0x938/+0x940/+0x948/+0x950/+0x958/+0x960/+0x968/+0x970/+0x978`
    and a shared scalar `+0x920/+0x928`. Constants come from the image at
    `_DAT_144f3a0b0` / `_UNK_144f3a0b8` (identity-like start values, read in
    `FUN_14197bc10` as the state init) and `_DAT_144f39280` / `_UNK_144f39288`.
  - Stage count is the int at `+0x754`. Per-stage state block is 0x20 bytes,
    starting at `+0x7a0`, stride 0x20: `+0` ch1 s0, `+8` ch2 s0, `+0x10` ch1 s1,
    `+0x18` ch2 s1. Shared scalars: `+0x770` ch1 k, `+0x778` ch2 k, `+0x780` ch1 h,
    `+0x788` ch2 h, `+0x790` ch1 cross, `+0x798` ch2 cross.
  - Per stage, per channel (ch1 shown, ch2 uses `+0x778/+0x788/+0x798`):
    `v = ((x*cross - s1)*k + s0) * h`, then `s0 = 2*v - s0` and `s1 += 2*k*v`.
    This is a trapezoidal integrator pair. The stage output feeding the next
    stage is `x - v` in the read code, but the decompiler lane merge
    (`CONCAT44` of the low dword of the `divpd` result) makes the exact
    subtraction ambiguous. Treat that step as unconfirmed.
  - After the stages, a final block at `+0x3d*0x20` (derived from the stage
    count) is updated the same way, scaled by the float at `+0x758` (input
    gain) and mixed with floats `+0x75c` and `+0x760`.
- Coefficient helper `FUN_14197bc10` (444 B): **read** (first 95 lines). It
  seeds the 2x2 state vectors, then loops over stages computing the per-stage
  block. Remaining lines (the tail with the final-stage block and the write back
  to `param_2/param_3`) are not read yet.
- Parameter writer `FUN_14197d910` (436 B): **read**. Table-driven.
  - Normal entries: `value = param * slope + offset`, where slope and offset are
    floats in a 12-byte record table at `DAT_14a64efc8` / `DAT_14a64efc4`
    (stride 0xc, indexed by the parameter id). The value is written to a
    per-slot float array, a dirty flag is set, and a second write goes to a
    secondary array when the mode byte at `+0xc00` is non-zero.
  - Notch-table entries (record index `-1` in the first table): a 0x14-byte
    record table `DAT_14a64ee80` (stride 0x14: slot index, flag, base index,
    count), with interpolation `iVar4 = trunc((count+1) * param)` clamped to
    `[0, count]`. The record at `+0x15d8 + slot*0x14` gets the index and a float
    fraction, and a dirty bit is set in `+0xc10`. This is the binding for the
    Phasis notch table (`ENGINE_PAR_PHASIS_NOTCHES` in the TSV).
  - The table values (`DAT_14a64efc0`, `DAT_14a64ee80`, `DAT_14a64f088`) are image
    data, not in the decompile text. They are **not read** in this unit (CPU rule:
    no re-opening the binary). The saved-field to parameter-id binding therefore
    stays open.
- Vibrato/Chorus is not related to Phasis; do not reuse the notch-table writer
  for it.

## Vibrato/Chorus (0x69, `hammondvibratochorus::HammondVibratoChorus`)

- Wrapper `FUN_14099eea0` (340 B) and core `FUN_14196d230` (1133 B): located,
  not read in this unit. Entry points and field offsets are in
  [k-modfx-classic-bodies.md](k-modfx-classic-bodies.md) and are unchanged.
- LC ladder bodies `FUN_14196b9e0` (3055 B) and `FUN_14196c600` (3074 B),
  `FUN_141969370` (613 B): located, not read. The "wave-digital LC ladder"
  label is still unverified.
- Nothing from this module was re-read or confirmed in this unit.

## Gap table

| Module | Status | Recovered in this unit | Missing | Blocker |
| --- | --- | --- | --- | --- |
| Flair (0x5c) | unsupported | per-sample two-channel recursion, four-lane ramp bank and gate (read) | voice loop `FUN_14195d9b0`, pre-stage `FUN_14195db30`, resampler `FUN_141ee2160`, pitch note `+0x40`, start-phase writer | ~5 KB of helpers unread; no audio vectors; random start phase must be injected |
| Phasis (0x5b) | unsupported | allpass stage kernel and state layout (read); coefficient helper head (read); parameter writer with notch-table branch (read) | coefficient helper tail, `FUN_14197cfe0` body, `FUN_14197d740` (461 B), `FUN_14197d910` table values, oversampling, `FUN_14197ef70/eaf0` | table values need the image; no Phasis law yet |
| Vibrato/Chorus (0x69) | unsupported | none new | ladder equations, core body, wrapper | all bodies unread here |

## Engine status (unchanged)

- No change to `crates/sampler-core/src/dsp/**` or `crates/sampler-ir/src/fx/**`.
- Parity rows for Flair, Phasis and Vibrato/Chorus stay `missing`.
- The existing Phasis allpass cascade in `SweepLaw::Sweep` must not be reused as
  Phasis: the recovered kernel is a 2x2 state-space stage with a per-stage
  coefficient helper, which is a different law.

## Next steps (in order)

1. Finish `FUN_14197bc10` tail and `FUN_14197cfe0` to pin the Phasis per-stage
   coefficient law and the per-frame order.
2. Read `FUN_14197d910`'s table values from the image once a single oracle
   capture is allowed, then map the saved notch indices.
3. Flair: voice loop `FUN_14195d9b0`, pre-stage `FUN_14195db30`, resampler
   `FUN_141ee2160`, then the pitch note writer.
4. Vibrato: the LC ladder entry points and the core `FUN_14196d230`.
5. Only then add a shared config-driven core in `sampler-core` with a recorded
   vector per module. Do not add per-product copies.

## Confidence

- Structure of the Phasis allpass stage and state layout: medium-high (read,
  addresses cited, but the coefficient helper tail is not read).
- Parameter-writer shape: high. Table values: unknown.
- Flair ramp bank and gate: high for the operations, medium for the increment
  semantics (taken from the constant `1.0` as the earlier note states).
- All laws: none yet. No parity claim.
