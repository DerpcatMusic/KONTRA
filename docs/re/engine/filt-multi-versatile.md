# Multi-filters: 3x2 Versatile and Dual SKF (Kontakt 8)

Unit `filt-multi-versatile`. Saved types: **19** (3x2 Versatile) and **130** (Dual SKF).
Companion families that share the same cores: Monark 140..=143 and Simple LP/HP 120 (both in
`dsp/filter/kontaktfx.rs`), and the legacy/SV classes in `dsp/filter/cascade.rs`.

## Status of the evidence

- **Kontakt 8 decompile is not ready.** `/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`
  did not exist when this was written. Everything below comes from the existing repo evidence
  (the code comments that cite native addresses and the recorded vectors), not from a fresh read of
  the exported C. Native addresses are quoted as the repo already records them.
- The spec is our own statement of laws and topology. No pseudocode is copied into the repo.

## Shared engine map

| Concern | Where | Notes |
| --- | --- | --- |
| Saved type table, knob mapping | `crates/sampler-kontakt/src/effects/filter.rs` (`build`, `map`) | Fidelity and caveat per type. |
| Filter reader call | `crates/sampler-kontakt/src/effects/mod.rs` (~L1217) | Passes the record's extra knobs only for 19, 120, 130, 140..=143 and 63..=65. |
| IR configuration | `crates/sampler-ir/src/fx/filter.rs` (`Variant::Versatile`, `KontaktFx::DualSkf`) | Config types only; no DSP. |
| Range validation | `crates/sampler-ir/src/validate.rs` (~L373) | Versatile knobs must lie in 0..=1. |
| Versatile core | `crates/sampler-core/src/dsp/filter/cascade.rs` (`versatile`, ~L554; `ClockedClass::clock`, ~L1967) | Chamberlin stages, shared with Pro-53. |
| Dual SKF core | `crates/sampler-core/src/dsp/filter/kontaktfx.rs` (`Skf`, ~L526) | Mono core; stereo runs one core per channel. |
| Oversampling | `crates/sampler-core/src/dsp/kaiser.rs` (shared `Kaiser` stage) | Dual SKF and Monark use it below 66 kHz. |

## Module 1: 3x2 Versatile (type 19)

**Parameters (stored, 0..=1 unless noted).** Cutoff and Resonance (the record's main pair), then
11 object-order knobs: `SHIFTB`, `SHIFTC`, `RESB`, `RESC`, `TYPEA`, `TYPEB`, `TYPEC`, `BYPA`,
`BYPB`, `BYPC`, `FILTER_GAIN`. Reader offsets 0x38..0x60 (`map` in `effects/filter.rs`).

**Laws the core implements** (`cascade.rs` `versatile`):

- Sample-rate constant `k = 2π / rate` (`0x401921cac083126f` is 2π).
- Stage 0 note = `clamp01(cutoff) * 111 + 20`. Stage 1 = note + `SHIFTB * 80`, stage 2 = stage 1 +
  `SHIFTC * 80`.
- Each stage's resonance `r` maps to `q` by a two-segment law (`r * 1.3` below 0.5, a steeper
  segment above). A limit caps the damping gain `g` per stage.
- Morph `TYPE` in 0..1 crossfades low → band → high: below 0.5 weights low and band; above it,
  band and high.
- `BYP` is each stage's wet amount. The wet law and the normaliser `norm` are computed from the
  three stages' `comp` values (`wet / ((1 - r) * 1.4 + 0.05) + (1 - wet)`).
- `FILTER_GAIN` is `10^((g - 0.5) * 3)` times the normaliser.
- Control clock: `ClockedClass::clock` moves both cutoff and resonance by `0.015` of the distance
  to the goal on each control tick (cascade.rs ~L1967).

**Topology as implemented.** `Chamberlin::tick` runs the three stages **in series in one loop**:
each stage's output is `y += (mode - y) * wet`, so stage 2 receives stage 1's blended output. There
is no parallel bank. The "x2" in the name is the two extra stages (B and C), not stereo. Stereo is
one core per channel.

**Defaults.** None are stated in the repo. The mapping takes all 11 knobs from the saved record;
the zero placeholders in `VERSATILE_UNSET` are replaced in `map`.

**Evidence.**
- Recorded vectors: `cascade_stage_vectors.json`, keys `VERS-lp` and `VERS-mixed` (cutoff 0.5,
  resonance 0 / 0.3 / 0.5, impulse and ±4 square). The test
  `cascade::tests::stage_classes_match_the_native_models` checks them at error < 2e-5. It passes.
- The repo comments say the setter and process were matched under the `kemu` native-code emulator,
  so the match is against native instructions, not a rewrite.

## Module 2: Dual SKF (type 130)

**Parameters (native order).** HP cutoff, HP resonance, LP cutoff, LP resonance, drive
(`KontaktFx::DualSkf`, `ir/fx/filter.rs`).

**Mapping from the saved record.** The top-level `cutoff` feeds the LP cutoff slot, and the
top-level `resonance` feeds the HP cutoff slot. The reader stores drive as `2v - 1` in the
record, so the mapping uses `(v + 1) / 2`. Those top-level fields are read from the BFilter reader,
not from a recorded vector, so the field-to-slot assignment is **unconfirmed**. See the gap table.

**Core.** One `Skf` recurrence holds both sections (HP and LP) and their resonance feedback.
Native setter 0x140b059c0, prepare 0x140afd350, process 0x140b000c0, cutoff law 0x140b06580 (per
the file header). Below 66 kHz the core runs ×2 through the shared Kaiser stage, then decimates.

**Evidence.** `kontaktfx_vectors.json` has three `DualSKF` cases at 48 kHz. The test
`kontaktfx::tests::matches_the_native_classes` checks them. It passes. The header records
bit-exact agreement with the native class at 48 kHz.

## How multi-filters compose from the shared cores

The shared cores come from the filt-sv lane (`dsp/filter/linear.rs`, `dsp/filter/shaper.rs`) and
this unit (`dsp/filter/chamberlin.rs`):

- `linear::Explicit`: the explicit (Chamberlin) stage. `hp = shaper(b) + (y k - (b d + l))`,
  `b += hp g`, `l += b g + eps`, and the output blends toward a weighted low/band/high mix by `wet`.
  Its band state takes a `Soft` transfer in the loop.
- `shaper::Soft`: `(a - c |s|) s`, the Pro-53 feedback and the Versatile band shaper.
- `shaper::unit_sqrt`: `v / sqrt(v^2 + 1)`, the Dual SKF resonance soft clip.
- `linear::Zdf` and `shaper::Knee` (filt-sv): the trapezoidal SVF used by SV family and Multi.

Compositions:

- **3x2 Versatile = three `Explicit` stages in series** (`chamberlin.rs::Chamberlin::tick`), with a
  `Soft` feedback of the last low state subtracted from the input. Each stage's morph weights and
  wet are its own. This is serial, not parallel.
- **Dual SKF = two sections in series in one recurrence** (`kontaktfx.rs`). Section 0 is the HP
  path, section 1 the LP path, each with its own `unit_sqrt` clip. Its recurrence is ZDF-shaped but
  its resonance term couples to the output (`k (q, r)` applied after the `y` solve), so it keeps its
  own loop rather than `Zdf`. Moving it onto `Zdf` was not attempted: it would change the recurrence, so it needs its own vector check.
- **Parallel** multi-filters (SV Par. LP/HP, Par. BP/BP) are a filt-sv composition, not this unit.
- Monark and Simple LP/HP stay in `kontaktfx.rs`: their feedback nonlinearities (Padé, noise,
  per-channel history) do not fit the shared stages.

## Gap table

| # | Item | Engine covers it now | Missing or unproved | Status | Needed to close |
| --- | --- | --- | --- | --- | --- |
| 1 | Versatile law at other cutoffs | Stage note law, morph and wet law, norm, gain law implemented | Vectors use cutoff 0.5 only | Partial | Kontakt 8 decompile (READY) of `FUN_14095db50`, the resonance functions and `FUN_14096f840`; more vectors at other cutoffs |
| 2 | Versatile shift, morph, wet and gain sweeps | Full law in code | No sweep vectors; the mixed case uses one shift and one morph set | Partial | Recorded or native vectors across the knob ranges |
| 3 | Versatile SSE rounding path (≥32-frame calls) | Scalar order matches the scalar native path | The SSE path differs in rounding, as for Pro-53 | Missing | A native vector captured with 32+ frame calls |
| 4 | Versatile clock constant (0.015) | Implemented from `0x140aea690` | Not checked beyond the vectors | Partial | Same decompile as row 1 |
| 5 | Dual SKF field-to-slot mapping | Mapping in `effects/filter.rs` | Top-level `cutoff`/`resonance` → LP/HP slots is from the reader only | Unconfirmed | Decompile of the BFilter reader for type 130, or a Kontakt saved-state round trip |
| 6 | Dual SKF sample rates other than 48 kHz | Core handles any rate; ×2 Kaiser below 66 kHz | Vectors only at 48 kHz | Partial | Native captures at 44.1, 96 and 192 kHz |
| 7 | Dual SKF stereo interleave | One core per channel with shared controls | Native channel interleave unprobed; vectors are mono | Missing | Stereo native capture |
| 8 | Dual SKF in the KSP parity ledger | Engine is implemented | `sampler-ksp` ledger still lists `$FILTER_TYPE_DUAL_SKF` as Approximate | Doc drift | Separate KSP-side change, out of this unit |
| 10 | Explicit-stage and Soft move (this PR) | Bit-identical (stage vectors 2e-5 tolerance and the filter suite, 63/63) | Done |
| 9 | Stale status docs | `docs/architecture-v2/DSP_COVERAGE.md` carries a stale-status banner | `docs/FEATURES.md` rows 113–115 still say ✗ for these types; the file is pinned to `d36380b` (2026-10-03) | Doc drift | Leave the pinned audit as-is; the next audit refresh should update it |

## Changes in this unit

- `effects/filter.rs`: 3x2 Versatile fidelity is now `Partial` with a caveat that names the
  vector coverage (row 1–4). It was `Exact`, which overclaimed the vectors. The misplaced doc
  comment above `VERSATILE_UNSET` moved to `map`.
- No new DSP. No new tests: the existing `stage_classes_match_the_native_models`,
  `matches_the_native_classes` and `effects::filter` tests cover the behaviour, and they pass
  (`build-slot cargo test -p sampler-core --lib filter -j 4`: 62 passed; `-p sampler-kontakt --lib
  effects::filter`: 4 passed).

## Confidence

- Versatile: **medium**. Laws and topology match the recorded vectors at one cutoff. Other settings
  are the code's own statement.
- Dual SKF: **medium-high** at 48 kHz, mono. The field-to-slot mapping is **low** until row 5 is
  confirmed.

## Waiting on

- READY on the Kontakt 8 decompile (rows 1, 2, 4, 5). Until it exists, the PR stays a draft.
