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
  decay/low/damp setter runs (`plate.rs`); parameter ramps are not
  reproduced. Decompile pass 2026-10-22 corrected the ramp shape: it is a
  per-frame linear ramp (see "Plate decompile pass" below), not a block count.
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
| Plate core | Full core (20 AP, shelves, ring, 16 lines, Hadamard, panning) | Ramp length and increment writer (per-frame ramp of 8 lanes is confirmed); Init LP frequency source (+0x68); display decay/predelay laws | Find the increment writer in the apply path; locate +0x68 writer; find display vtable |
| Galois core | Full core, vector-verified | Dry/wet lane (dispatcher lane 8) has no binding; set fully wet, Return/Dry carries mix | Find lane 8 binding in `0x141aea370` setter |
| Raum Grounded/Airy | Full core | Tempo-synced predelay (`240·n/(f·m·bpm)`) not bound, read as ms; feedback limiter reduced to ±2.5 clamp (gain table missing) | Extract limiter `0x141ae3ba0` gain table; sync table |
| Raum Cosmic | Chain bank (8 stages, 2 lines each, LFO, sign flips, Catmull-Rom, matrices, rings) from decompile; no native vector | Predelay feedback, b80 feedback, post-filter laws, wet law (inside Raum outer `1419878b0`); FDN bank sticky flag | Dedicated pass on `1419878b0`; Kontakt capture for a native vector |
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

## Decompile recovery (Kontakt 8 source READY)

Sources: `source/kontakt8/` buckets and PE constants. Labels: CONFIRMED (read
from code and constants), INFERRED, UNKNOWN.

### Legacy Reverb input low-pass: implemented
- Two-pole Butterworth-like biquad, embedded per reverb object (`0x14095b2d0`,
  `0x14082a0b0`), not a one-pole. Cutoff `x = 0.75(1-damping)+0.25` CONFIRMED.
- Frequency law: truncated exp(6.15x) to order 8, normalized, sine-warped
  biquad. -3 dB at 44.1 kHz: 248.0, 1194.2, 5369.3 Hz at x = .25, .5, .75
  (INFERRED: re-implementation of the closed form, not native-captured).
- Repo stand-in (one-pole, 20 Hz·1000^x) replaced. Possible stereo/colour field
  swap in the Legacy loader remains UNKNOWN (not changed).

### Raum Cosmic: chain bank implemented (decompile structure, no native vector)
- Chain bank (flag 0, eight serial two-line stages) structure, oscillators, sign
  flips, Catmull-Rom reads, matrices and ring lengths: CONFIRMED from decompile and
  setter emulation. Predelay feedback, post filters and wet/dry law: UNKNOWN.
- Sticky bank flag: a Cosmic instance after Grounded/Airy may use the FDN bank
  (flag 1). Host sequence behaviour UNKNOWN.

### Raum tempo-synced predelay: partial
- Predelay ms when synced = clamp(60000·x·v/bpm, 0, 2000), v floored at 1 by a
  byte flag, bpm default 120 when < 0.1 (`0x140a74bf0`). CONFIRMED.
- Core lane 1 gets the converted time (the repo's "core 2..5" note is wrong).
- Enum-to-division table NOT FOUND. The repo's 240·n/(f·m·bpm) is equivalent
  only if x·v = 4n/(fm) (INFERRED). Engine has no host bpm path here yet.

### Raum limiter: partly recovered, placement corrected
- NI::DSP::Limiter (`0x141ae3ba0`) runs once per block on the outputs, not
  inside the feedback loop. CONFIRMED. Its output clamp is ±2.5 (CONFIRMED).
- The repo's ±2.5 clamp sits inside the feedback path (`raum.rs` Outer), which
  the decompile does not show. Left unchanged to keep Grounded/Airy vectors
  bit-identical; deviation recorded here.
- Gain table: G = 2^(u/60 − 20), u = index, 0 dB at 1200, built at runtime
  (`0x1404d14d0`). CONFIRMED.
- Detector and smoothers: arithmetic CONFIRMED. Time constants 1/15 and 1/3 at
  44.1 kHz are implausibly long for a limiter; meaning UNKNOWN.

### Galois lane 8: premise corrected
- Dispatcher `0x14198aa20` belongs to the Raum core (GaloisReverbExtended), not
  the Galois reverb. CONFIRMED.
- No saved field maps to Galois lane 8; the saved layout has 10 values. CONFIRMED.
  The repo's 1.0 has no binary source; its runtime source is UNKNOWN. The
  Galois saved-to-lane mapping is INFERRED (enum type, then time, size, damping,
  modulation, diffusion, predelay, high cut, low shelf, stereo).

## Binding-table cross-check (origin/engine/k-ksp-engine-params, fx_legacy_delay_reverb_mod.tsv)

- **0x140aa3d10 is the Legacy Reverb process** (module 0x15, `Effects/Reverb`,
  `BParFXReverb`), not a new module. Its structure (8 comb lines with one-pole
  damping, four allpasses, gain/ramp, shared prepare/finalize) is the Freeverb
  network already in `design.rs freeverb()`. Not verified this pass: the lead's
  "equal-length rings" claim (the repo encodes the classic Freeverb lengths, set up
  by the sample-rate-dependent setter `0x14094c760`), and the allpass offsets
  `+0xb88..+0xc60` against `ALLPASS_MS`. Treat both as open.
- Legacy saved predelay: `ENGINE_PAR_RV_PREDELAY` table law (101 points) equals the
  repo's `tables::RV_PREDELAY` (e.g. 0.1 → 1.140032 ms, 0.5 → 17.0788 ms,
  1.0 → 180 ms). CONFIRMED by binding table.
- Legacy RV_SIZE, RV_STEREO, RV_COLOUR, RV_DAMPING: identity laws, fields 1, 2, 3,
  4 at slots 12, 20, 16, 24. Matches the repo lanes.
- **Galois (`Effects/GaloisReverb`, `BParFXGaloisReverb`)**: RV2_TIME field 1,
  SIZE 2, DAMPING 3, MOD 4, DIFF 5, PREDELAY 6, HIGHCUT 7, LOWSHELF 8, STEREO 9,
  all identity. The saved layout matches the decompile's offsets (+0x08..+0x28).
  This confirms the field mapping that was INFERRED above. Lane 8 is still not in
  the table.
- **Raum unit tables** (`Effects/Raum`): the unit parameter writes field 1 as a
  sync flag (stored 1 for keys ≥ 5), field 15 as the beat count x, and field 16 as
  a floor flag (0.125 for keys 0–4, 0 otherwise). Known x values at keys 5, 6, 7,
  8, 10, 16, 20: 4, 2.666667, 2, 1.333333, 0.6666667, 0.1666667, 0.04166667.
  Keys 9, 11–15, 17–19 are not sampled. Check: x = 4 at 120 BPM gives 2000 ms,
  the clamp maximum, consistent with `predelay_ms = 60000·x·v/bpm`.
  Partial: the full key set is still needed, and the engine has no host-tempo path
  into the space build yet.

## Freeverb lengths and Raum tempo sync

- **Freeverb (`0x14094c760`)**: the comb and allpass tuning constants (25.31 to
  36.67 ms combs, 12.61/10.0/7.73/5.10 ms allpasses, spread 0.52154, scale 0.001)
  match the repo's `ALLPASS_MS`/`COMB_MS` and the spread. No code change.
- **Raum sync unit (`0x140a74f30`)**: keys 5 to 21 store a beat count (4, 8/3, 2,
  4/3, 1, 2/3, 1.5, 0.5, 1/3, 0.75, 0.25, 1/6, 0.125, 1/12, 1/16, 1/24, 1/64). Keys
  0 to 4 are free milliseconds (sampled). The synced law `0x140a74bf0` is
  `beats · 60 / bpm · v · 1000` ms, with bpm 120 below 0.1 (constants 60, 1000,
  0.1, 120 read from the binary).
- Implemented: `Network::raum_beats`; `Outer` ring holds a 2 s ceiling and reads
  `D = round(secs·rate)` behind the write; host tempo flows `dsp.rs` →
  `Space::process` → `Outer::process`.
- **Open**: `v` is taken as the normalised predelay knob (`core[1]`); the caller's
  clamp bounds (2000 ms observed at 120 BPM and x = 4) are assumed to be 0 to 2 s.
  Key 21's flag (+0x21) is not read by the law (+0x20); the floor is not applied.
  No live Kontakt render has checked either point.

## Plate decompile pass (2026-10-22, Kontakt 8 export READY)

Read from the existing export only (no new decompile). Labels as above.

- **Constructor** `141a81d80` (0141a80000.c): sets defaults decay 0.5 (+0x48),
  low 0.5 (+0x54), high damp 0.5 (+0x50), core 8 = 1 (+0xf0), and core 7 at
  +0x4c = 0.1 (CONFIRMED value; the repo's "core 7 fixed 1" is the apply path,
  which is not located). Calls `141ac46d0(obj, 2)` to set two channels and
  96100-frame channel buffers. The constructor does not build any section.
- **Prepare** `141aa2300` (0141aa0000.c): stores sample rate at +0x64, block
  size, tap offset `int(sr·0.005)` term, then `141a92180`, then the three
  setter calls (0, 1, 9, 10) and `141abd580` and `141aba8b0`. CONFIRMED.
- **Init low-pass** is `141a92180`: `tanf(+0x68 · c)` bilinear first-order
  section; coefficient law CONFIRMED. Its frequency at +0x68 is written by no
  Plate function in the export (UNKNOWN source). The repo's 7500 Hz value
  matches the native impulse vector but its origin is not located here.
- **Setter** `141ac7280` (0141ac0000.c): each control writes a *target* lane
  (+0x163470.. block) and a dirty flag (+0x163490). When the flag at +0x8 is
  set the targets are copied to the live lanes at once; otherwise the live
  values glide. Core 1 (decay) calls `141abcff0` (line biquad rebuild), core
  9 (low) `141ac3430`, core 10 (high damp) `141abec10`. CONFIRMED.
- **Ramp**: eight live lanes (+0x163430..+0x16344c) advance by one increment
  per frame inside the process loop (`141a7a1a0` and `141a7b670`, 0141a70000.c
  near lines 7078-7093). Lane 0 is predelay (read index
  `int(p·k·sr)` clamped to ≥ 1, wrap 0x17763; k is the constant at
  `DAT_1447148a4`, 0.25 per repo, not re-read), lane 1 decay (line biquads
  rebuilt while its increment is nonzero), lane 3 low (shelf gain as a power
  of ten of the lane, `141a92270`), lane 4 high damp (shelf corner from a
  constant, gain via `141a92080`, then `141a972c0`), lane 5 stereo (pan).
  CONFIRMED structure; the constants were not re-read. The increment writer and the ramp length
  are NOT located in the export: the repo's `int(sr·0.02/bs+0.5)` remains
  UNVERIFIED.
- **Display laws** (decay 2.993x + 1.007 s, predelay 0.25x s): no address
  found in the setter or constructor. Still UNVERIFIED.

Plate status after this pass: core, shelves, lines and panning are
decompile-backed and vector-tested (`impulse_matches_native`). Open: ramp
length and increment writer, the source of the init LP frequency, display laws.
Parity needs a native capture of a ramped parameter change (not done here).

## Raum Cosmic decompile check (2026-10-22)

- The Cosmic findings note (`~/.cache/k-reverbs-oracle/findings/cosmic.md`)
  was re-read against the export. Dispatch (`14198db10` flag, algorithm from
  Raum+0x9f4), decay law `141a..` `14198d2a0` (c = cos(1.17v), s = sin(1.17v)),
  size `14198ec30`, modulation `14198e820`, rate `14198e830`, and the Raum
  outer process `1419878b0` (6436 bytes, SIMD) are present in 0141980000.c.
- Not closed here: predelay feedback gains, b80 feedback identity, post-filter
  laws and the Cosmic wet law. These sit inside `1419878b0`, which is a large
  SIMD function; they need a dedicated pass. No native vector exists for
  Cosmic, so no Cosmic row can reach parity without a Kontakt capture.
- The repo's Cosmic chain bank (`design/cosmic.rs`) stays as implemented.
  Its status is partial, not "none" (the gap table below was out of date).

## Plate constants pass (rdata cache, 2026-10-22)

Values from `docs/re/kontakt-rdata/constants.tsv` (f32 column). Each law below
is the one the Rust core already uses; the constants now confirm them.

| Constant | Value | Used for | Status |
|---|---|---|---|
| `DAT_1447148a4` | 0.001 | predelay ms to frames: `p · 250 · 0.001 · sr` | CONFIRMED (matches 0.25·p·sr) |
| `DAT_144fc4a18` | 250 | predelay target scale (ms) | CONFIRMED |
| `DAT_1447148b8` | 0.02 | not the Plate ramp (used by other modules) | Plate uses `DAT_147697658` = 0.0200000014; see docs/re/engine/k-plate-ramp.md |
| `DAT_1479ab65c` | 23561.9453 = 7500·π | init low-pass prewarp `tanf(x · 7500π)` with `x = +0x68` | CONFIRMED constant; `+0x68` = 1/sr is INFERRED (writer not found by pattern) |
| `DAT_1479ab620` | 5600 | damp shelf corner (Hz) | CONFIRMED |
| `DAT_144714a00` | 10 | base of the gain powers | CONFIRMED |
| `DAT_1447148e0` | 0.2 | damp gain `10^(0.2·c − 0.2)`, c = 1 − hd | CONFIRMED |
| `DAT_1479ab340`, `DAT_1479ab2e0` | 1.71875, 1.4375 | low gain `10^(c·1.71875 − 1.4375)`, c = 1 − ls | CONFIRMED |
| `DAT_1479ab61c`, `DAT_147698a7c` | 4851, 6000 | line damping frequency `c·4851 + 6000` (Hz), c = 1 − hd | CONFIRMED (matches `line_biquad` f2) |
| `DAT_144714914` | 0.9 | allpass coefficient | CONFIRMED |
| `DAT_14470bad4` | 1.0 | clamps | CONFIRMED |

Init low-pass: the design routine `141a92180` runs inside prepare (`141aa2300`),
before the four setters. The setters for decay, low and damp write shared
coefficient slots (`+0x360..+0x37c`) in the damp path, so whether the 7500 Hz
section survives into production is **open**. The constructed-core native
impulse vector matches with it on. Production keeps it off (`init_lp = false`)
until that is settled.

Still open after this pass: the parameter-ramp increment writer (the lanes and
the 0.02 s length are known, the code that writes `+0x163450..+0x163468` is not
found), and the display laws.

## Raum Cosmic: where the open laws sit (2026-10-22, not finished)

- The Raum outer process `1419878b0` (0141980000.c) does not reference the
  Cosmic-specific fields by the offsets in the findings note; those offsets are
  used by a different SIMD-heavy routine.
- The double-state loop in `141966ad0` was once a b80 candidate. It is REJECTED: it
  belongs to the Freak class vtable. b80 is the NI::DSP::Limiter (see
  docs/re/engine/k-raum-cosmic-laws.md, section 0).
- Still open after the laws pass: the parameter writing +0x8c (feedback law), the
  SVF preset writer, and the limiter parameter 1. No native Cosmic vector exists.
