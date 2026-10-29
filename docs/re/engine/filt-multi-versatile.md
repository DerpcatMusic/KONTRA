# Multi-filters: 3x2 Versatile and Dual SKF (Kontakt 8)

Unit `filt-multi-versatile`. Saved types: **19** (3x2 Versatile) and **130** (Dual SKF).
Companion families that share the same cores: Monark 140..=143 and Simple LP/HP 120 (both in
`dsp/filter/kontaktfx.rs`), and the legacy/SV classes in `dsp/filter/cascade.rs`.

## Status of the evidence

- **Kontakt 8 decompile is ready** (`source/kontakt8/READY`). The checks in the section
  "Decompile confirmation" below read the export directly. Constants that are global data
  (`DAT_*`) were not resolved to values in this pass; the vector tests pin them numerically.
- Earlier text below: **Kontakt 8 decompile was not ready when this was first written.** `/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`
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
| Dual SKF core | `crates/sampler-core/src/dsp/filter/kontaktfx.rs` (`Skf`, ~L526) | One core for both channels: per-channel filter and oversampler state, shared controls (native layout, Module 2). |
| Oversampling | `crates/sampler-core/src/dsp/kaiser.rs` (shared `Kaiser` stage) | Dual SKF and Monark use it below 66 kHz. |

## Decompile confirmation (Kontakt 8 export)

Functions read in `source/kontakt8/` (headers `// ==== <addr> <name>`):

| Native function | Checked against | Result |
| --- | --- | --- |
| `FUN_14095db50` Versatile cutoff setter | `Versatile::note` (clamp, x111, +20), shifts (`+ SHIFTB x80`, `+ SHIFTC x80`), stage g = `2^((note-69)/12) x 440 x 2pi/rate` clamped by limit | matches; stage 1 and 2 limits read from ramp fields (see row 2) |
| `FUN_140966230` resonance setter | `q` two-segment law around 0.5 (slopes 0.66 / 1.3), limit `(q+1.3075) q 0.293 + 0.7352`, `d = 2 - 2q`, comp `wet/((1-r)1.4+0.05)+(1-wet)` | matches (stage 0) |
| `FUN_14096f840` normaliser | `norm = 3 / (t1+t2+t3+comp1+comp2+comp0)` with the three `(g_i / g_j) 6 - 5` denominators | matches |
| `FUN_140aef6e0` process | per stage: `hp = shaper(b) + (y - (b d + l))` (k = 1), `b += hp g`, `l += b g`, mode = `wl l + wb b + wh hp`, `y += (mode - y) wet`; shaper constants shared by the three stages; output `gain x norm` smoothed | matches Chamberlin with feedback 0 and eps 0 |
| `FUN_140b059c0` Dual SKF setParam | knob index 0..3 clamped to [0,1]; index 4 stored as `2v - 1` | matches `kontaktfx.rs` `set` |
| `FUN_140aea690` control clock | cutoff moves by `(goal - cur) x c` per control tick | constant `c` is a global; Rust uses 0.015 (not resolved here) |
| `FUN_140ab0xxx` generic ramp setter | writes (target, current) pairs at `+0x430..+0x46c` | stage 1 and 2 limits (`+0x44c`, `+0x450`) are ramp currents, so their targets are not Versatile-specific code yet |

Not resolved in this pass:
- The record-to-slot mapping for Dual SKF (row 5). The reader `FUN_140d00db0` is a tag-based chunk
  reader for all BFilter records and reads no 130 knobs (Module 2).
- The Versatile stage 1/2 limit targets (row 2).
- The SSE path for 32+ frame calls (row 3): not located as a separate function in this pass.

## Module 1: 3x2 Versatile (type 19)

**Parameters (stored, 0..=1 unless noted).** Cutoff and Resonance (the record's main pair), then
11 object-order knobs: `SHIFTB`, `SHIFTC`, `RESB`, `RESC`, `TYPEA`, `TYPEB`, `TYPEC`, `BYPA`,
`BYPB`, `BYPC`, `FILTER_GAIN`. Reader offsets 0x38..0x60 (`map` in `effects/filter.rs`).

**Laws the core implements** (`cascade.rs` `versatile`):

- Sample-rate constant `k = 6.283 / rate` (`0x401921cac083126f` is 6.283, 0.003% under 2π; the kemu vectors need it).
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

**Mapping from the saved record.** The extension reads five knobs after three kind words (type 130
layout in `effects/mod.rs`). It maps the first record value to the LP cutoff slot and the second to
the HP cutoff slot (the `cutoff`/`resonance` names), and the drive is `(v + 1) / 2`. The field-to-slot
assignment is **unconfirmed**:
- The generic BFilter reader `FUN_140d00db0` reads no Dual SKF knobs for type 130. Its gates for
  cutoff, resonance and the 0x3f/0x48 fields exclude 0x82, and it returns at `2 < type - 0x5f`.
- The BFilterDualSKF vtable slots 7, 8, 12 to 14, 23 and others are forwarders (`rcx += 0x210`, then a
  jump through the wrapper's vtable slot of the same number). Slots 0, 3 and 4 are the destructor, a
  copy and a type compare. The typed knob reader for 130 was not located in the export.
- Native knob order is fixed by the core: index 0 is the stage 0 cutoff (the HP output), 1 its
  resonance, 2 the stage 1 cutoff (LP), 3 its resonance, 4 the drive (`FUN_140b000c0`, `FUN_140b03630`).
  The Rust mapping and the native order agree on what each index does; what is open is which record
  value feeds which index.
- The Rust `KontaktFilter::new` modulation pair `[2, 0]` moves knob 2 (LP cutoff) with cutoff
  modulation and knob 0 (HP cutoff) with resonance modulation. This follows the same unconfirmed mapping.

**Core.** One `Skf` recurrence holds both sections (HP and LP) and their resonance feedback.
Native setter 0x140b059c0, prepare 0x140afd350, process 0x140b000c0, cutoff law 0x140b06580 (per
the file header). Below 66 kHz the core runs ×2 through the shared Kaiser stage, then decimates.

**Native layout (this unit, Kontakt 8 decompile and PE).** The object is a BFilter of 0x51A0 bytes,
built by factory `FUN_1407c95c0`: BFilter base `FUN_140a856c0`, then wrapper ctor `FUN_140ae5f40` at
+0x210, so the core is at +0x420. Prepare `FUN_140afd350(core, rate, quantum)` sets the oversampling
factor (2 below 66 kHz, else 1), the Kaiser stages, the ramp count (2 ms) and the initial controls.
Process `FUN_140b000c0(core, in[], out[], n)` takes planar channel pointers and the channel count from
core+0xc. Per channel it runs upsample, the two sections, downsample. Each channel has its own
history (0x18 bytes at core+0x20+0x18ch, and one Kaiser state per channel). The control ramp is copied
per channel and only the last channel commits it, so both channels see the same control trajectory.

**Evidence.** kemu vectors (`tools/re/kontaktfx_kemu_vectors.py`, Kontakt 8 PE, core-level calls:
setter, prepare, tick, process):
- `kontaktfx_vectors.json`, DualSKF cases at 48 kHz mono (impulse and square, 3 knob sets). The
  regenerated values match the previous file to 5e-9 of peak.
- `kontaktfx_stereo_vectors.json`: independent L/R noise, 256 frames, at 44.1, 48 and 96 kHz, steady
  and moving knobs (one knob set per 32-frame block). The native channels 0 and 1 agree when the input
  is identical in both channels. Writing a changed knob only (as the Rust test does) gives the same
  output as writing all knobs at every block (checked: max difference 0).
- The test `dual_skf_stereo_matches_the_native_vectors` checks the stereo vectors at 1e-6 of peak, and
  `kontaktfx::tests::matches_the_native_classes` checks the mono cases. Both pass.

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
| 1 | Versatile law at other cutoffs | Cutoff setter `FUN_14095db50`, stage 1/2 resonance setters and the scalar process run natively under kemu | Native sweep cutoff 0.1 to 0.9 at resonance 0.5, LP type, wet 1, shift 0, gain 0.5 (`versatile_kemu_vectors.json`); VERS-lp keys reproduce exactly | Closed for these settings | None for these settings; other types and resonances stay covered by the code |
| 2 | Versatile shift, morph, wet and gain sweeps | Full law in code | Native setters for shift, morph, wet and gain not located; the harness writes LP weights, wet 1, gain 0.5 and shift 0 itself | Open | Locate the setters that write the stage weights (+0x3e0..+0x3f4), wet (+0x404, +0x408, +0x40c), gain (+0x420) and shift (+0x428, +0x42c), then drive them natively |
| 3 | Versatile SSE rounding path (≥32-frame calls) | Dispatcher `FUN_140aec840` uses SSE branch `FUN_140aee4b0` when `DAT_14a5ed92c` is set and the call has 32+ frames (flag is 0 in the image, set at run time) | Under kemu, one channel at 64 frames equals the scalar path; stereo and hardware rounding unchecked | Partial | Stereo native vector; a hardware capture if one exists |
| 4 | Versatile clock constant (0.015) | Constant read from the image: `DAT_144f38664` = 0.015; clock is virtual slot 0xb8 (`FUN_140aea690`) | Moving-control vectors for the clock not run; the Rust clock order (process then clock) is unverified for Versatile | Constant closed; moving open | Run `FUN_140aea690` natively with moving goals and compare with `ClockedClass::clock` for Versatile |
| 5 | Dual SKF field-to-slot mapping | Mapping in `effects/filter.rs`; native knob order fixed | Which record value feeds which knob: the generic reader gives no 130 knobs and the typed reader was not located | Unconfirmed | Decompile of the typed 130 knob reader (stream order and store offsets into setParam), or a Kontakt saved-state round trip. The `[2, 0]` modulation pair depends on it |
| 6 | Dual SKF sample rates other than 48 kHz | Core handles any rate; ×2 Kaiser below 66 kHz (prepare `FUN_140afd350`) | 44.1 and 96 kHz kemu vectors, mono and stereo, pass; 192 kHz not vectored (same path as 96 kHz) | Closed at 44.1, 48, 96 kHz | 192 kHz kemu vector if a 192 kHz path is ever used |
| 7 | Dual SKF stereo interleave | One core: per-channel filter and Kaiser state, shared controls | Native layout confirmed in `FUN_140b000c0` (planar, per channel); kemu stereo vectors bit-match with moving knobs | Closed (kemu oracle) | Stereo capture from the real host is not needed for the core |
| 8 | Dual SKF in the KSP parity ledger | Engine is implemented | `sampler-ksp` ledger still lists `$FILTER_TYPE_DUAL_SKF` as Approximate | Doc drift | Separate KSP-side change, out of this unit |
| 10 | Explicit-stage and Soft move (this PR) | Bit-identical (stage vectors 2e-5 tolerance and the filter suite, 63/63) | Done |
| 9 | Stale status docs | `docs/architecture-v2/DSP_COVERAGE.md` carries a stale-status banner | `docs/FEATURES.md` rows 113–115 still say ✗ for these types; the file is pinned to `d36380b` (2026-10-03) | Doc drift | Leave the pinned audit as-is; the next audit refresh should update it |

## Validation of the generator (this pass)

`tools/re/kontaktfx_kemu_vectors.py` reproduces the stored 48 kHz mono cases:
- DualSKF: all three knob sets, impulse and square, within 5e-9 of peak.
- BlueMonark: mode 0 variant 0 (both knob sets) within 1e-9 of peak. Modes 1, 2 and 3 (variant 0) differ
  by up to 0.05 of peak, and variants 1 and 2 (modes 0 and 2) differ by 1.4 to 1.6 of peak. The mode and variant
  setup entry points were checked (setType before or after setters and prepare, and the mode table
  `DAT_144fcf368` and the Monark tables `monark.f32le` were compared with the PE image and match). These
  cases are not reproduced under kemu and are carried over unchanged. Monark is the next unit to
  investigate (`filt-ladder-monark.md`).

## Changes in this unit

- `effects/filter.rs`: 3x2 Versatile fidelity is now `Partial` with a caveat that names the
  vector coverage (row 1–4). It was `Exact`, which overclaimed the vectors. The misplaced doc
  comment above `VERSATILE_UNSET` moved to `map`.
- No new DSP. New: the kemu generator, the stereo vectors and their test. Rows 5 to 7 updated.
- Earlier pass: no new tests: the existing `stage_classes_match_the_native_models`,
  `matches_the_native_classes` and `effects::filter` tests cover the behaviour, and they pass
  (`build-slot cargo test -p sampler-core --lib filter -j 4`: 62 passed; `-p sampler-kontakt --lib
  effects::filter`: 4 passed).

## Confidence

- Versatile: **medium**. Laws and topology match the recorded vectors and the native cutoff and
  resonance sweeps (LP type, wet 1, shift 0, gain 0.5). Morph, wet, gain and shift settings are the
  code's own statement until their native setters are driven.
- Dual SKF core: **high** at 44.1, 48 and 96 kHz, mono and stereo, kemu oracle. The field-to-slot
  mapping is **low** until row 5 is confirmed.

## Waiting on

- Row 2: native setters for the morph, wet, gain and shift knobs. Row 3: stereo native vector and hardware check of the SSE branch. Row 4: moving-control vectors for `FUN_140aea690`. Row 5 (Dual SKF reader for type 130) is outside this unit.

## Constant resolution (static PE read, this pass)

Global constants were read from the Kontakt 8 image (`re-binaries/Kontakt8.exe`, RVA = VA - 0x140000000,
`.rdata`), not from the decompile text. Confirmed: clock move factor 0.015 (`DAT_144f38664`), shaper
0.06 / 0.0075, sample constants 440, 1/12, 69, 20, 80, 2pi, 2.0, 1.3075, 0.293, 0.7352, 1.4, 0.05,
3.0, 6, 5, 0.66, 0.65, 1.3. Each matches `chamberlin.rs`, `shaper.rs` and `cascade.rs`.

Stage 1/2 limit targets are not from the parameter store on this path: the native resonance setters
(`FUN_1409663f0`, `FUN_1409665c0`) write them with the same law as stage 0 (checked at resonance
0.2, 0.6 and 0.9). Still open: the Dual SKF record-to-slot map (Wine probe blocked, see
PARITY_MATRIX), and the Versatile morph, wet, gain and shift setters.

## Native sweeps (filt-pro53-versatile)

- `tools/re/versatile_kemu_vectors.py` drives `FUN_14095db50` (which calls `FUN_140966230` and the
  normaliser), the stage 1 and 2 resonance setters and the scalar process `FUN_140aef6e0` under kemu.
  It reproduces the existing VERS-lp keys (`cascade_stage_vectors.json`) exactly and writes
  `versatile_kemu_vectors.json` (24 cases).
- Test `versatile_native_sweeps_match` (`cascade.rs`) checks the sweeps at 2e-5 of each vector's peak.
- Stage fields read from the process code: stage 0 g +0x3c8, damping +0x3d4, weights +0x3e0/+0x3e4/+0x3e8,
  wet +0x404; stage 1 g +0x3cc, damping +0x3d8, weights +0x3ec/+0x3f0/+0x3f4, wet +0x408; stage 2 g +0x3d0,
  damping +0x3dc, weights +0x3f8/+0x3fc/+0x400, wet +0x40c. Output gain target +0x420 times normaliser
  +0x41c, smoothed with the rate constant +0x470. Notes +0x424; shift raw +0x428 and +0x42c.
