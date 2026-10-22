# Corpus gaps 6 and 9: "Filter: filter law" and unsupported "effect" (Kontakt 8)

Status: analysis only. No DSP change in this pass. Neither gap closes here; the open items and
the reason each one stays open are listed below. Evidence addresses refer to the Kontakt 8
decompile export (`source/kontakt8/`, READY) and the existing `docs/re/engine` notes.

## What the survey counts

Source: `scratch/survey/full-20261022.jsonl` (full tier, 1395 instruments). Counts below are
instruments, grouped by library folder.

| Gap | Instruments | Where |
| --- | --- | --- |
| 6 "Filter: filter law" | 56 | 51 Conflux 1.1.0 (50 multis, 1 instrument), 3 Una Corda, 1 Morphology Evolved, 1 ANALOG STRINGS |
| 9 "effect" | 39 | 36 Conflux 1.1.0 (multis), 3 Areia 1.2.0 |

Both gaps are concentrated in Conflux.

## Gap 6: where the message comes from

- `crates/sampler-kontakt/src/effects/filter.rs`, `note_fidelity`: any filter whose table row is
  not `Exact` reports "filter law". Partial rows are Ladder LP1..Notch (saved 30..41), Daft LP/HP
  (70/71), Formant II (91), Simple LP/HP (120) and 3x2 Versatile (19).
- Per-entry evidence: the one per-entry dump in scratch (`ray-unsupported.tsv`, Conflux Ray Line,
  program 1) has 78 "filter law" entries: 72 Ladder LP4, 5 Daft LP, 1 Ladder BP4.
- So the bulk of gap 6 is Ladder. Update 2026-10-22: the flag 0x49 byte is decoded
  (`ladder-cutoff-byte.md`); Ladder LP4 with HQ off is Exact, so its refusal goes away. The
  other Ladder rows stay Partial. The Daft rows are a separate, smaller open item (see Daft).

### Ladder: what the decompile confirms

- Cutoff setter (0x140b00b80): the byte at core+0x49 picks the cutoff lane. Zero reads the
  direct state at core+0x4fc4; non-zero reads the ramped lane block at core+0x5010 instead.
- Ramp state machine for that lane (0x140b036a0, on the 32-frame clock): the target minus
  current is multiplied by a coefficient into a per-tick step (core+0x5014). The lane itself
  advances on a 4-frame cadence in 0x140b00b80. The target setter (0x140b04d60) takes the
  coefficient `1 / (core+0x28 * 1500 + 150)` (constants as recorded in `filt-ladder-monark.md`).
  The pole and table constants are already in `ladder_kernel.rs`.
- The writer of core+0x49 is a single store (0x140b04a20). The apply function (0x1409619b0,
  file `0140960000.c`) reaches it for the Ladder selectors (internal ids 22..33) through the
  wrapper at 0x140aefe50, which is passed the record object's byte at +0x29. A second caller
  (inner case 0x58, not a Ladder id) writes the same byte into another object (base +0x84).
- Resolved: the wrapper at 0x140aefe50 is a 12-byte tail jump; the argument passes through
  unchanged. The byte is the record's +0x29, which the reader sets to 1 for v0x90..0x94 and reads
  from the stream for v0x95 (`ladder-cutoff-byte.md`). The v0x92 value is 1.
- Rust keys this on the explicit `cutoff_path` flag, not on the record version.

### Daft (5 entries, Ray Line)

- Partial. Open items as already recorded in `PARITY_MATRIX.md`: ramp tick rate (caller of vtable
  slot 8 not found), countdown arming, and the Daft ENGINE_PAR ids. The saturation and
  interpolation stand-ins listed in the 2026-10-19 diagnostics are no longer the open part: the
  knees and the 2x Kaiser path are in `daft.rs`.

## Gap 9: unsupported "effect"

- Emitted by `effects/mod.rs` when a saved effect module is not modelled (NotModeled). The reason
  text comes from the `unproved()` tables in `color.rs`, `timemod.rs`, `dynamics.rs` and
  `mangling.rs`. A record with no table entry reports only its name, version and length.
- Identified with `fx_survey` (`crates/sampler-kontakt/examples/fx_survey.rs`, now reads every
  program of a `.nkm`; parse only, no sample loads), `FX_VALUES=1`, 2026-10-22:

| Effect (module) | Instruments | Library | Doc / status |
| --- | --- | --- | --- |
| Replika (0x5a) | 18 programs | Conflux multis | `k-replika.md`: reader only, core loop not ported |
| Flair (0x5c) | 9 | Conflux multis | `modfx-flair-phasis-vibrato.md`: not ported |
| Supercharger (0x60) | 8 | Conflux multis | `dyn-compressors.md`: CompXT core unrecovered |
| Vibrato Chorus (0x69) | 7 | Conflux multis | `modfx-flair-phasis-vibrato.md`: not ported |
| Phasis (0x5b) | 5 | Conflux multis | `modfx-flair-phasis-vibrato.md`: not ported |
| Wow / Flutter | 1 | Conflux multis | matrix row partial: per-sample kernel not located |
| Rotator (0x22) | 1 | Areia Sound Design | `color.rs` unproved; the matrix row is partial (core ported) |
| Convolution (0x16), version 0x70, length 121 | 2 | Areia Legacy Patches | no unproved reason; the 0x16 reader has no version gate and rejects this layout |

- fx_survey: 36 of the 100 program items in the 50 Conflux multis carry an unmodelled effect; 3 Areia instruments do.
- Each row needs its own core recovered from the decompile before anything can be ported with
  native vectors. Nothing in this table can be closed by one small change. The smallest entry is
  Convolution v0x70 (2 instruments): the 0x16 reader has no version gate, and the v0x70 layout is
  not recovered, so the record is dropped.
- Native-data tests cannot be written for any row yet: `kemu` drivers exist only for Choral and
  Legacy Chorus/Flanger (`tools/re/kemu_timemod.py`); the rest need the core first.

## Open items (in order)

1. Done: the byte that drives core+0x49 per record version (`ladder-cutoff-byte.md`).
2. Native capture with the flag set (kemu or Wine oracle) to turn the flag-1 snap law from
   decompile-confirmed into vector-confirmed. Current kemu vectors all have flag 0.
3. Identify the 39 effect instruments' modules with the census test, then pick the module with the
   most instruments and move it out of the `unproved()` tables with its own evidence.
4. Daft: find the vtable slot 8 caller and the countdown arming (matrix open items).

## Evidence confidence

- High: gap 6 is Ladder LP4 plus a few Daft entries; gap 9 is Conflux-dominated (counts above).
- High: the mechanism of the Ladder flag path and the lost argument at 0x140aefe50.
- Not established: the v0x92 flag value, which effect modules the 39 instruments use.
