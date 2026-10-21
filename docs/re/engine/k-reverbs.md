# Kontakt 8 reverbs: Plate (0x57), Reverb / Galois (0x59), Raum (0x65), Legacy Reverb (0x15)

Status: **spec draft, waiting on decompiled source.** `source/kontakt8/READY`
does not exist yet and `source/kontakt8/` is empty; the Ghidra job
(`source/ghidra/Kontakt8.lock`) is still in RTTI analysis. Everything below
comes from the public manual, the existing repo evidence and the current
engine code. Items marked **unverified** need the decompiled text (or a native
recording) before they can be called parity.

Docs: Kontakt 8 Effect Reference, Reverbs (Plate, Reverb, Raum, Reverb Delight,
Reverb and Convolution); KSP engine parameters. Repo evidence:
`docs/re/kontakt-reverb.md`, `docs/re/kontakt-reverb-convolution.md`,
`docs/architecture-v2/KONTAKT_DSP_LAWS.md`, `docs/re/DSP_SYSTEM_INVENTORY.md`.

## Shared engine surface

- Config and topology enum: `crates/sampler-ir/src/fx/space.rs` (`Topology`
  variants `KontaktPlate`, `Galois`, `Raum`, `Freeverb`).
- Core: `crates/sampler-core/src/dsp/space/` (`mod.rs` parameter gates at
  L61-63; `design.rs` dispatch L27-31; `design/plate.rs`, `design/raum.rs`,
  `design/galois_vectors.rs`; `network.rs` shared line/allpass/matrix kit).
- Kontakt format mapping: `crates/sampler-kontakt/src/effects/space.rs`
  (`legacy` L123, `plate` L155, `galois` L177, `read_raum` L196, `raum` L224,
  `why_unbound` L243).

## Module specs

### Plate Reverb (0x57)

- Parameters (saved, 0..1): decay (core 1), low shelf (core 9), high damp
  (core 10), predelay (core 0), stereo (core 13). Core 7 fixed 1, core 8 = 1
  (`plate.rs` header).
- Decay display: `2.993·x + 1.007` s; predelay display `0.25·x` s; width `x`
  (`space.rs:155-165`). Display-only fields; the core uses the controls.
- Law: input `0.5·(L+R)`; 20 first-order allpasses `a = 0.9`; shelves at
  1000 Hz (decay), 160 Hz (low), 5600 Hz (damp); early-reflection ring (32
  taps, sign table) into 4 groups; 16 lines through two-shelf biquads and a
  16-point Walsh-Hadamard; four panned groups; wet predelay
  `max(int(0.25·p·sr),1)`; level `0.04216965`.
- Edge cases: Init's 7500 Hz bilinear low-pass is lost on `clear()` until a
  decay/low/damp setter runs (`plate.rs`); parameter ramps
  `int(sr·0.02/bs+0.5)` blocks are not reproduced.
- Evidence: core setter `0x141ac7280`, apply `0x140963eb3`, shelf
  `0x141a92480`, Hadamard/taps tables `0x1479aa750`/`0x1479aa7d0`.
- **Unverified:** the 2.993/1.007 decay and 0.25 predelay display laws in
  `space.rs` have no cited address.

### Reverb / Galois (0x59)

- Parameters: type (Room 0 / Hall 1) plus nine normalized controls: time,
  size, damping, modulation, diffusion, predelay, high cut, low shelf, stereo
  (`space.rs:177-190`).
- Laws in the display fields: decay `0.5·e^(3.7·time)` s (setter
  `0x141aea370` case 0); predelay `11025·x` samples at 44.1 kHz.
- Core: 31 modulated delay lines, orthogonal m-sequence matrix, Room or Hall
  tables, allpass diffusion, 200 Hz low shelf, high cut. Verified against the
  emulated kernel (`galois_vectors.rs`).
- Dispatch: `0x141ae2c60` selector bits; the 8-entry RVA table at
  `0x141ae2cf4` resolves to `141ad7b70, 141acda50, 141acf210, 141ad0590,
  141ad18e0, 141ad3230, 141ad4f20, 141ad6370` (`kontakt-reverb.md`).

### Raum (0x65)

- Parameters (v0x10 reader `0x140ac5cc0`): predelay, feedback, low shelf, high
  cut, reverb, diffusion, decay, modulation, size, damping, rate, mix; bytes
  freeze and sparse; a type enum (Grounded, Airy, Cosmic).
- Grounded and Airy: `GaloisReverbExtended` (`design/raum.rs` header): decay
  `T = 199.75x/(75(1-x)+1)+0.25` s; prime line lengths; gain
  `√(1-10^(-4/min(T,20)))·(a√size+b)`; high-band damping per line; two f64
  phasor modulators at `1.5x+1` Hz; 31 lines; coupled units with angle
  `±0.35x` (Grounded) or `±0.2625x` (Airy); output `√(1-mix)·in + √mix·G·wet`.
  Setter `0x14198aa20`, process `0x1419878b0`.
- Predelay field ms→core: raw ≤1 passes; else `√(0.0125t)` below 80 ms or
  `√((t-80)·0.00052083)+1`, halved, capped at 1 (`space.rs:196-222`).
  Display predelay: `0.08·(2x)²` for x≤0.5, else `1.92·(2x-1)²+0.08` s.
- Cosmic: eight-stage allpass chain, **not built**.

### Legacy Reverb (0x15)

- Parameters: predelay (ms, 0..180), size, stereo, colour, damping (0..1).
- Setter (case 0x3e of `FUN_1409619b0`): comb feedback `size·0.3+0.65`; comb
  damping `1-(1-colour)·0.35`; channel-mix `(stereo+1)/2`; input lowpass
  normalized cutoff `(1-damping)·0.75+0.25`; allpass gain 0.5; input gain
  0.015. Freeverb delay lengths (`FUN_140aa3d10`, `FUN_14094c760`).
- Core: `Topology::Freeverb` (`space.rs:123-148`).
- Note: `FUN_140aa3d10` calls a virtual processor at wrapper `+0x1100` before
  its own chain; not the Galois path (`kontakt-reverb.md`).

## Gap table (shared engine vs Kontakt 8)

| Module / item | Engine covers | Missing or approximate | Next step |
|---|---|---|---|
| Plate core | Full core (20 AP, shelves, ring, 16 lines, Hadamard, panning) | Init low-pass after `clear()`; ramps; display decay/predelay laws lack addresses | Confirm display laws from decompile; add ramp law |
| Galois core | Full core, vector-verified | Dry/wet lane (dispatcher lane 8) has no binding; set fully wet, Return/Dry carries mix | Find lane 8 binding in `0x141aea370` setter |
| Raum Grounded/Airy | Full core | Tempo-synced predelay (`240·n/(f·m·bpm)`) not bound, read as ms; feedback limiter reduced to ±2.5 clamp (gain table missing) | Extract limiter `0x141ae3ba0` gain table; sync table |
| Raum Cosmic | none | Eight-stage allpass core unbuilt; reported unmodeled | Extract Cosmic setter/process once source READY |
| Legacy Reverb | Freeverb network, setter laws, predelay ms | Input lowpass uses a one-pole stand-in at 20 Hz·1000^cutoff; the native biquad prototype coefficients are not in the image | Recover prototype coefficients from decompile |
| Reverb Delight (0x7a) | field binding measured | Amp, tremolo, spring-reverb cores; MONO unproved | Out of this unit's scope unless assigned |

## Test plan (after READY)

- One golden/recorded-vector test per recovered module where the decompile
  gives constants: Plate shelf coefficients (`0x141a92480`), Raum gain and
  damping (`0x141985e30`, `0x141985cf0`), Legacy input lowpass once the
  prototype is known.
- Do not add per-parameter snapshot tests; reuse existing `space/tests.rs`
  vectors.

## Evidence confidence

- Plate and Galois cores: high (already verified in repo tests against
  emulation or native vectors, per their headers).
- Raum Grounded/Airy: medium-high (law text is specific; native comparison
  depends on the limiter table and sync predelay).
- Legacy: medium (setter laws cited; input filter approximated).
- Cosmic, Delight, sync and limiter: no evidence in the engine yet.

## Implementation status (2026-10-21)

- Decompile export still incomplete (`source/kontakt8/` empty, no READY).
  Kontakt 8 is installed under Wine (`tools/kontakt-oracle/`), so black-box
  measurement is available but not yet run for any gap below.
- Engine kit is already general: `network.rs` (lines, allpass, shelves,
  matrices, modulation) and `design.rs` build every reverb as a `Spec`.
- Duplicate first-order shelf designs, to merge behind one warp selector once
  the laws are confirmed: `design.rs` `shelf` (tan warp, `0x141649120`),
  `design/plate.rs` `shelf` (sin warp `k`, `0x141a92480`), `spark_law::shelf`,
  and Raum's `Svf::low_shelf`. Guard: `design/tests.rs` shelf vectors.
- No code changed in this step; the gaps above remain open.
