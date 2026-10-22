# Kontakt 8 legacy filters: spec and gap table (`filt-legacy`)

Scope: Kontakt 8 filter types LP1/2/4/6, HP1/2/4, BP2/4, BR4, Legacy Ladder (Pro-53 and Vowel
share its stage core), and Simple LP/HP. Engine core: `crates/sampler-core/src/dsp/filter/`
(`cascade.rs`, `ladder_kernel.rs`, `kontaktfx.rs`). Mapping: `crates/sampler-kontakt/src/effects/filter.rs`.

Status: laws come from repo evidence (`docs/re/kontakt-bindings/filter-laws.tsv`, recorded native
vectors, kemu runs) and from **Kontakt 8 measured through Wine** (`tools/kontakt-oracle/eqrender.py`,
`eqspecs.py filtlegacy`; raw output in `Research/KONTRA/oracle-filt-legacy/`, not in the repo). The
decompiled export was not used (user decision: do not wait for READY); refine with it if it lands.

## Kontakt measurements (Wine oracle, 2026-10-21)

Left-channel impulse responses, 48 kHz, reference = bypass, probes 30 Hz to 15 kHz. Test
`legacy_responses_match_the_kontakt_measurements` (`cascade.rs`, data `cascade_kontakt_oracle.json`)
compares the compiled cascade unit's 16384-frame impulse response at the same probes:

- Types: LP2, HP2, BP2, HP4, BP4, BR4 (new measurements) and Legacy Ladder (type 17); LP1/LP4/LP6 were
  measured earlier (`filter-laws.tsv`).
- 48 cases x 10 probes, every probe above -60 dB, match within **0.1 dB** (largest error ~0.06 dB,
  at the 30 Hz end of HP2/HP4/BP4/BR4; Kontakt values are rounded to 0.01 dB).
- Legacy Ladder: knob 0 with resonance 0 does **not** match (Kontakt ~6 dB/oct shallow tail,
  -12.3 dB at 30 Hz, -46.1 dB at 1 kHz; ours -14.7 dB at 30 Hz, -126.9 dB at 1 kHz). Excluded from
  the golden. Knobs .25 to 1 and resonance .5/.9 match. Cause not found; the knob-0 clamp of the stage
  cutoff law is the suspect.
- Simple LP/HP (120): inconclusive. CUTOFF and RESONANCE do not change the measured response (flat
  -0.25 dB, identical at every setting), so the mapping `lp = CUTOFF, hp = RESONANCE` is not confirmed.
  Its knobs need another engine-parameter name, which is not in the repo.
- Stage 16 (Pro-53), 14/15 (Vowel) and 19 (Versatile) were not measured in Kontakt yet.

## Knob 0 across the order-4/6 family (2026-10-21, parity matrix)

Kontakt's knob-0 response differs from the engine for LP4 and LP6 (and the Legacy Ladder, LP4-like),
at resonance 0. Kontakt shows about 8 dB/oct above 100 Hz (LP4: -9.5 dB at 60 Hz, -20 at 100, -36 at
500); the engine's 4-pole curve falls 24 dB/oct. The knob-0 cases are excluded from the golden.
Ruled out: a prior-knob state carry-over (warming to knob .5 first gives the same shape), the
knob-to-warp mapping (`FUN_14095c9c0`: `(e-1) 0.00257`, `+0.002`, `* 44011.976/rate`, `pi/2 * 0.99`,
`t = 0.99878 w`, the two `sin(0.5 -+ t)` poles: all equal the engine's `knob_position`, `angle` and
`lp_pole`). LP2, HP2, HP4, BP2, BP4 and BR4 match at knob 0. Open: the order-4 second section, whose
HP pole sits at -1 when `t -> 0` (`FUN_14095c9c0` second `FUN_1408e4970` call), and the native
normalisation `dVar4` at that point.

## Saved type ids (measured)

| Kontakt name | Saved id | Core | Config fidelity |
| --- | --- | --- | --- |
| Legacy LP1 / HP1 | 2 / 3 | cascade, 1 pole | Exact |
| Legacy BP2 | 4 | cascade, 2 pole | Exact |
| Legacy LP2 / HP2 | 6 / 7 | cascade, 2 pole | Exact |
| Legacy LP4 / HP4 / BP4 / BR4 | 8 / 9 / 10 / 11 | cascade, 4 pole | Exact |
| Legacy LP6 | 12 | cascade, 6 pole | Exact |
| Pro-53, Vowel A/B, Legacy Ladder, 3x2 Versatile | 16, 14, 15, 17, 19 | stage core | Exact |
| Simple LP/HP | 120 | KONTAKTFX (`kontaktfx.rs`) | **Partial** (changed, see below) |

Source: `docs/re/kontakt-bindings/filter-type-ids.tsv`, `crates/sampler-kontakt/src/effects/filter.rs`.

## Per module

Parameters are the saved knobs (`CUTOFF`, `RESONANCE`), both 0..1 on the engine's native clock
(32-frame control block, 2 ms ramps).

**Legacy 1-pole LP1 / HP1 (2, 3)**
- Cutoff law (measured, `filter-laws.tsv` rows "legacy LP1POLE / HP1POLE"): LP1 -3 dB at 249 / 1189 /
  5355 Hz for knob .25 / .5 / .75 (x4.6 per quarter); HP1 208 / 990 / 4389 Hz. Knob 0 bottoms at
  38-59 Hz.
- Resonance has no effect. Order 1, 5.3-5.6 dB/oct.
- Native prepare: one-pole prototype `b0 = b1 = 0.26276`, pole `0.29341`, store `44011.976 / rate`.

**Legacy 2-pole LP2 / HP2 / BP2 (6, 7, 4)** and **4-pole LP4 / HP4 / BP4 / BR4 (8, 9, 10, 11)**, **6-pole LP6 (12)**
- One design (`cascade.rs` header): Butterworth prototype per section, `Q_k = 1 / (2 cos((2k+1) pi / 2N))`,
  bilinear at `tan(w/2) = 0.5463`, warp by the stored knob through an 8-term exp series, feedback taps
  scaled by `1 + f r'` with `r' = 1 - (1 - res)^2`, first-order allpass substitution for the move.
- LP4 / BP4 knee clamp on section 1 output (`0.6`), HP4 on its input; LP6 on section 2 (`0.7`); BR4
  sums two sections in parallel, double coefficients.
- Measured (`filter-laws.tsv`, LP4/LP6): at resonance 0 the knee is sharp and flat, -3 dB at the same
  corners as LP1 (246 / 1192 / 5364 Hz), 24-25 dB/oct (LP4) and 36-37 dB/oct (LP6): maximally flat
  pole distribution. Resonance .5 / .9 on LP4: +20.9 / +37.3 dB peak at ~1.7 kHz (knob .5), passband
  -5.2 / -9.9 dB. LP6: +30.5 / +52.3 dB, passband -11.2 / -22.9 dB.
- Stereo: no L-to-R coupling (right output exactly 0 for left-only input).

**Legacy Ladder (17), Pro-53 (16), Vowel A/B (14, 15), Versatile (19)**
- Shared stage core (`cascade.rs` `Legacy::Ladder|Pro52|VowelA|VowelB|Versatile`). Setter and process
  (Pro-53 FUN_14095d940 / FUN_1409099a0; Legacy Ladder FUN_14095d630 / FUN_140aef200; Vowel
  FUN_14095dd00 / FUN_14095e250 / FUN_140914a20) match the native code under kemu to 2e-5.
- Scalar summation order only; the SSE path for >= 32-frame calls differs in rounding.
- Kontakt measurement of these four types is **not** in `filter-laws.tsv`; the Exact claim rests on
  kemu against the native code.

**Simple LP/HP (120)**
- KONTAKTFX core: FilterDJExtRange mode 2 high pass into FilterLadderExtRange mode 0, 2 knobs (LP, HP).
- Core is bit-exact in mono against the native class under kemu at 48 kHz (`kontaktfx.rs`, `kontaktfx_vectors.json`). The DJ core alone is replayed slot by slot from the wrapper's own element (`simple_lphp_kemu_vectors.json`, test `simple_lphp_dj_replays_native_slot_writes`).
- Stereo: runs mono cores per channel with shared controls. The native channel interleave is unprobed.
- Alternate smoother (the DJ block's rate clamp, taken only when a slot rate is > 0): **unreachable**, see the proof
  below (2026-10-22). The old `u8`, `1/(p2 1500 + 150)` note has no source in the export.

## Control clock (shared by the legacy cascade and stage cores)

- Native control clock `0x140ae9f90` (vtable slot 23 of each class; kernel slot 33, setter slot 36).
  A host call of N frames is processed in 32-frame segments. The countdown `obj+0x1bc` is 32 after a
  reload and carries across host calls. A moving call (goal differs from the current knob by more than 1e-4)
  opens with an empty segment when the countdown is 0 (a previous call ended on a boundary), then
  one clock step per segment, and one more step after the last segment. Verified by kemu for LP1 and HP1
  (`tools/re/legacy_lp1_process_vectors.py`, `lp1_process_vectors.json`, host calls of 17 to 64 frames).
  Step counts per 32 frames therefore depend on the host call size: 1.25 for 128-frame calls, 1.5 for
  64, 2.0 for 32 (kemu). Other clocked classes use the same native function; not probed per class.
- The alternative branch (global `0x144fd4c54`) is dead: the image holds 0 there and the export
  never writes it, so only the `else` path runs. The offset slew `+0x1f8` is not referenced in
  `FUN_140ae9f90`. Both are closed (`cascade.rs` process comment).
- Coefficient update cadence of LP1/HP1: measured with kemu process vectors (see above).

## Gap table (closed against the decompile, 2026-10-21)

Evidence: the Kontakt 8 export at `source/kontakt8/` (`rg -n "^// ==== <addr> "` for the function),
and the image's read-only data read with `pefile` (the static value of a global, not a decompile).

| Item | Engine covers | Evidence | Status |
| --- | --- | --- | --- |
| Clock alternative branch (`DAT_144fd4c54`) | ignored (else path) | the global is 0 in the image's `.rdata`; no writer in the export; `FUN_140ae9f90` runs only its `else` path | **closed**: dead branch |
| Clock offset slew `+0x1f8` | not modelled | not referenced in `FUN_140ae9f90` | **closed** for the clock; the cascade's own slew stays unmodelled and unused |
| LP1 / HP1 update cadence | countdown model (above) | `FUN_140ae9f90` process-level kemu vectors, real vtables | **closed** for LP1 and HP1 (2e-6 per sample); other classes unprobed |
| LP2 .. LP6, BP2, BP4, BR4 response | Kontakt-measured to 0.1 dB above -60 dB (golden) | measurement | **closed** |
| Stereo (legacy family) | per-channel state, no L to R coupling | measured, and the process loop of `FUN_140afef00` keeps per-channel state with shared controls | **closed** |
| Legacy Ladder setter (17) | note `100 p + 20`, index `5 * note + 1381.88`, `g = table * pi / rate`, clamp `1.57`, `0.5`, `res * 4`, `+1e-12` | `FUN_14095d630` and `FUN_140aef200` constants read from the image | **closed**: every constant equals the engine's `note_hertz`, `legacy_ladder` and `Ladder2x` |
| Legacy Ladder stage nonlinearity | `clip`: `v (1 - 0.01 |v|)` for `|v| <= 50`, else `+-25` | `FUN_140aef200` uses `0.01`, `50`, `25`, `0.769230x` (`big_g`), `k = g - 1` | **closed**: identical |
| Legacy Ladder knob 0 (res 0) | engine matches the Kontakt shape at knobs .25 to 1 | the setter and process agree with the engine, so the linear structure is the same; the knob-0 tail still differs (Kontakt about 6 dB/oct, -12.3 dB at 30 Hz) | **open**. Reproduced alone (no carry-over from BR4). Next: a knob sweep 0 to 0.25 to find where it leaves the engine's curve, and the runtime-generated exponential table entries near index 1481 |
| Simple LP/HP (120) knobs | `lp = CUTOFF, hp = RESONANCE` (unconfirmed) | the wrapper setParam (vtable slot 12, `0x140961190`) forwards an int to both children's slot 12, which store ints in the cores (+0xc/+0x10); no float reaches the DJ setter on that path (kemu: no DJ slot byte changes for indices 0..3). The DJ setter `FUN_140b058a0` is reached from the child drivers' route flags (`FUN_1408fa000`, `FUN_1408fac90`) | **open**: the knob source is the route path or the record reader; neither is traced to the saved record |
| Simple LP/HP alternate smoother (DJ block rate clamp, rate > 0) | not implemented; unreachable | every writer of a DJ slot rate writes -1 (ctor, `FUN_140b04b60`, `FUN_140b058a0` index 1, copy `FUN_140af6300`); the process `FUN_140afef00` never reads the rate; the wrapper setParam never reaches the slots; the kemu capture has rate -1 at every block | **closed** for the wrappers' own writers. Residual risk: a record reader writing a rate directly (not found in the export) |
| Pro-53 (16), Vowel A/B (14/15), Versatile (19) | stage core, kemu match 2e-5 | not measured | **open**: add to `LEGACY_UNMEASURED` |

Notes on the decompile: 428 functions in the export are marked `decompile failed`. Those are not
usable. None of the closures above depends on them.

## Change in this unit

- `crates/sampler-kontakt/src/effects/filter.rs`: Simple LP/HP (120) fidelity `Exact` -> `Partial`, with
  a dedicated caveat, so the import reports it instead of silently claiming full parity. No DSP change.

## Shared core (filt-sv API)

Built on `engine/filt-sv` (`dsp/filter/linear.rs` `Zdf`, `dsp/filter/shaper.rs` `Knee`, `pade`).

- **Knee.** The legacy BFilter clip (LP4/BP4 on section 1, HP4 on its input, LP6 on section 2) was an
  inline closure `(v - c) s + c` with `c = clamp(v, -1, 1)`. It is now `Knee::excess(s)`, a third
  `Slope` shape in `shaper.rs`, and the cascade `Design` stores `clip: Option<(usize, Knee)>`. The
  operation order is the same, so the native vectors (`legacy_classes_match_the_native_vectors`, 2e-5)
  and the Kontakt golden are unchanged. The packed cell layout is unchanged.
- **Not moved onto `Zdf`.** The legacy 2/4/6-pole sections are direct-form-I biquads with
  coefficients from the Butterworth warp, not zero-delay SVF stages. A `Zdf` realisation has the same
  transfer function but different rounding, and no vector proves it bit-identical, so the DF1 kernel
  stays. The one-pole LP1/HP1 and BR4 use the same DF1 tick.
- **Duplicates still open** (other lanes): the DF1 section tick also appears in `eq_family/legacy.rs`,
  `space/shape.rs`, `space/network.rs`, `timemod/dual_x.rs`, `color/tonestack.rs`. Sharing one runner
  there needs a bit-identity vector per caller and is not done here.

## Knob 0 resolved: analysis window, not a law (2026-10-22)

The Wine oracle's `eqrender.py` analyses a 16384-frame window that starts 256 frames before the response's
loudest sample (`spectrum`: `part = x[start - 16 ...]`, `ev = peak - 240`). For LP4 and LP6 at knob 0 the
window drops the early part of the impulse response, so the measured shape is a window artefact. Taking the same window in the engine reproduces the
Kontakt curve to 0.02 dB (LP4: -0.43, -9.51, -20.12, -28.06, -36.33, -42.38, -48.40 dB at 30..2000 Hz).
The golden test now uses that window for every case, and includes the knob-0 rows for LP4, LP6 and Ladder.
The earlier "knob-0 mechanism" notes above are superseded by this.

## Knob-0 status (superseded, kept for the record)

- The LP4 setter (`FUN_14095c9c0`) and the allpass substitution (`FUN_1408e4970`) were compared term by
  term with `cascade.rs` `knob_position`, `angle`, `lp_pole`, `substitute`, `warp` and the LP4 design.
  Every constant and operation matches, including the normalisation `(2 - sum c) / sum b`, `n = q 0.35 + 0.65`,
  and both sections using the same low-pass pole `a`. The formulas predict the Kontakt-measured shape
  at knobs .25 and above (golden), and cannot produce the knob-0 tail at any small knob (engine sweep 0 to 0.05).
- So the knob-0 difference is not in the section law. It must come from code the setter does not
  reach at knob 0 or from the process path. Not yet identified.
- A Kontakt fine sweep (0 to 0.25) to find the transition is the next measurement. It could not be
  run on 2026-10-22: the `kontra_ref` PipeWire sink is SUSPENDED and the known-good Ladder spec now
  measures silence on the monitor (see the oracle note in the parity matrix PR). Routing is the
  user's audio setup; it was not changed.
- Simple LP/HP: the element constructor `FUN_1407ccbb0` (`01407c0000.c:8077`) builds the HP child and the LP child (layout in the proof below); the record-to-parameter mapping is still not traced.

## Simple LP/HP DJ block: proof (2026-10-22)

Evidence: kemu on the Kontakt 8 image, real constructors and vtables. Capture `tools/re/simple_lphp_kemu_vectors.py`
(output `crates/sampler-core/src/dsp/filter/simple_lphp_kemu_vectors.json`), replayed by the Rust test
`simple_lphp_dj_replays_native_slot_writes`.

**Layout (measured; corrects earlier notes).** Element (`FUN_1407ccbb0`, count 1), vtable `0x144f124a8`.
Child `+0x210` is BFilterSimpleLP and holds the Ladder core at `+0x420` (vtable `0x144fcf7d8`). Child `+0x52e0` is
BFilterSimpleHP and holds the DJ core at `+0x54f0` (vtable `0x144fcf580`, mode 2 from `FUN_140b04660(core, 2)` in
the constructor). The earlier notes put the cores at `+0x210` and `+0x52e0`; those are the children.

**Per 32-frame block.** Wrapper slot 23 (`0x140908ae0`) calls the HP child's slot 23 (`0x1408fa000`), then the LP
child's (`0x1408fac90`). Each driver applies its route setters (flags at child `+0x540..` and `+0x4f90..`, values
clamped to [0, 1]), then the core's tick (slot 8) and process (slot 7). The wrapper's own slot 7 (`0x140959ce0`) is
not a process: it stores a channel pointer in the child (`0x140ab9a30`). The earlier note "process slot 7" was
wrong. The core slot 23 (`0x14050a460`) is a no-op, but the children's slot 23 are not.

**DJ slot fields** (core-relative; slots at `+0x2b8`, `+0x2e0`, `+0x308`): cur +0, step +4, rate +8, target +0xc,
dirty +0x10, count +0x14, n +0x18, scale +0x20 (`1/(n 32)`, 0.015625 at 48 kHz), state +0x24. The process reads cur
and step, not rate. The block `0x140b03490` reads rate and runs the clamp only when rate > 0; it writes cur, step,
dirty, count and state.

**Every writer of a rate (+8) on the DJ core:**
- constructor: -1 for all three (dynamic);
- cutoff setter `FUN_140b04b60` (index 0, also called by prepare `FUN_140afd0f0`): -1 for slots 0 and 1;
- setter index 1 `FUN_140b058a0` (resonance raw): -1 for slot 2, and sets its target;
- copy `FUN_140af6300` (vtable slot 19): -1 for all three (dynamic: a source rate of +0.5 comes out -1);
- setter index 2 (`FUN_140b04660`, mode) and index 3 (route scratch at `+0x2c`) write no slot field;
- vtable slot 1 `FUN_140afb790` zeroes targets and slot 4 `FUN_140afc990` snaps cur to target: no rate.

So the earlier gate "DJ setter index 1" is wrong: index 1 writes -1, which disables the clamp. The "lines 851..855"
that read `+0x2c0` belong to `FUN_140af13d0` (another class), not to the DJ process.

**Wrapper setParam (vtable slot 12, `0x140961190`).** The int in edx goes to child slot 12 (`0x140960dc0`), which
stores it in the child (`+0x124`, `+0x128`) and calls core slots 21 and 22 (int stores at core `+0xc` and `+0x10`).
xmm2 is not read on this path. Dynamic check: indices 0..3 with value 0.5 change no byte in the DJ slot region.

**Conclusion.** The alternate smoother (rate > 0) cannot be entered from the wrapper's setParam path, nor from any
writer found in the export. The DJ block's clamp is dead code for this module, so no DSP change is made. Open, not
proven: a record reader that writes a rate directly through a base register other than `param_1` (none is named in
the export; the reader for saved type 120 was not located). Also open: the knob source (route flags or the reader),
the Ladder setter index that carries CUTOFF, the stereo interleave, and whether the Rust modulation pairs
(`modulated: [0, 1]`) match the native route indices (route 1 on the DJ child is the resonance raw, not the cutoff).

## Pending

1. Legacy Ladder knob 0: knob sweep and table check (above).
2. Simple LP/HP knob path: find the record reader for saved type 120 and check which setter index and value it writes. The wrapper setParam path is closed (it never reaches the DJ slots). Re-measure after that.
3. (done) LP1/HP1 update cadence: kemu process vectors, see above.
4. Stage types 14/15/16/19: add to the oracle spec and extend the golden test.
