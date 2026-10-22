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
- So the bulk of gap 6 is Ladder, and the Ladder rows stay Partial until the flag 0x49 path below
  is proved. The Daft rows are a separate, smaller open item (see Daft).

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
- Gap in the decompile: the wrapper at 0x140aefe50 is decompiled with one parameter, so the byte
  is dropped and the store reads an unrecovered register. The byte source is therefore NOT
  established. The v0x95 record's second flag is the documented candidate
  (`filt-ladder-monark.md`); the v0x92 value (Conflux) is not traced.
- Rust today keys this path on record version (`ladder_kernel.rs::instant_cutoff`, versions
  0x90..=0x92 on). That is a proxy, unverified for v0x92. Because 72 of the 78 entries are Ladder
  LP4 and Conflux stores v0x92, the proxy decides the path for most of gap 6. It is left as is:
  changing it without the byte source would be a guess.

### Daft (5 entries, Ray Line)

- Partial. Open items as already recorded in `PARITY_MATRIX.md`: ramp tick rate (caller of vtable
  slot 8 not found), countdown arming, and the Daft ENGINE_PAR ids. The saturation and
  interpolation stand-ins listed in the 2026-10-19 diagnostics are no longer the open part: the
  knees and the 2x Kaiser path are in `daft.rs`.

## Gap 9: unsupported "effect"

- Emitted by `effects/mod.rs` when a saved effect module is not modelled (NotModeled). The reason
  text comes from the `unproved()` tables in `color.rs`, `timemod.rs`, `dynamics.rs` and
  `mangling.rs`.
- The survey jsonl keeps only the count, not the effect name. Identifying the 39 instruments'
  effects needs a per-instrument read. This pass did not run one (CPU rule: no library scans).
- Candidate modules already marked unproved (none confirmed as present in Conflux or Areia):
  Surround Panner, Stereo Tune, Rotator, Skreamer Deluxe, Chainsaw, amp/cabinet/pedal models,
  Replika, Phasis, Flair, Vibrato Chorus, Supercharger GT, Beat Masher, Gater, Beat Slicer, Reverse
  Grain, Transpose Stretch.
- Identification command for a later window: the ignored census test
  `crates/sampler-kontakt/tests/w6_conflux_intent.rs` (installed Conflux, via `kontakto-heavy`),
  which records the `effect` detail for each unsupported entry. Or `fx_survey` with `FX_VALUES=1`
  on the Conflux and Areia folders.

## Open items (in order)

1. Recover the byte that drives core+0x49 for each Ladder record version. Needs the call-site
   argument at the 0x140960000 apply (decompile re-export, not a new Ghidra session in this pass).
2. Native capture with the flag set and clear (Wine oracle), then decide Exact versus Partial for
   Ladder LP4 with moving cutoff. Current kemu vectors all have flag 0 (`ladder_kemu_vectors.json`).
3. Identify the 39 effect instruments' modules with the census test, then pick the module with the
   most instruments and move it out of the `unproved()` tables with its own evidence.
4. Daft: find the vtable slot 8 caller and the countdown arming (matrix open items).

## Evidence confidence

- High: gap 6 is Ladder LP4 plus a few Daft entries; gap 9 is Conflux-dominated (counts above).
- High: the mechanism of the Ladder flag path and the lost argument at 0x140aefe50.
- Not established: the v0x92 flag value, which effect modules the 39 instruments use.
