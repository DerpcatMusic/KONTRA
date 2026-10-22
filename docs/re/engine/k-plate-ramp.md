# Kontakt 8 PlateReverb: parameter-ramp law

Status: 2026-10-22, reading only. Sources: Kontakt 8 decompile export
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/*.c`) and the rdata cache
(`docs/re/kontakt-rdata/constants.tsv`, `vtables.tsv.gz`). No build, run, Ghidra, radare2 or REA.

Labels: CONFIRMED = read in the decompile, constants row or vtable row. INFERRED = from
structure or naming, not read directly. UNKNOWN = not located.

## Answers

1. **Writer (CONFIRMED).** The increments at +0x163450..+0x16346c are written by
   `FUN_140acb7d0` (0140ac0000.c:9295, 320 B), a generic 8-lane smoother state machine.
   Plate reaches it via vtable slot 8 = `FUN_141ab4dc0` (0141ab0000.c:2502, 12 B),
   which calls `FUN_140acb7d0(core + 0x163430)`.
   The literal search missed it because the helper indexes the lane block as
   `undefined8 *`: the dirty byte, active byte, countdown, N and rate sit at byte
   offsets 0x60, 0x61, 0x64, 0x68, 0x6c, 0x70 from +0x163430, never spelled as
   +0x163490 in that function.
   **Caller (CONFIRMED).** The Plate wrapper process `FUN_1408fbf60`
   (01408f0000.c:7883-7976, 509 B; vtable of `IDSPCoreWrapper<PlateReverb>` 144f14f58
   slot 23) calls the core tick (slot 8, line 7934) once per 32-frame control block.
   The tick runs after the setter loop (7918-7927) and before the core process
   (slot 7, line 7962).
2. **Ramp length (CONFIRMED formula, block argument CONFIRMED = 32).**
   `N = max(1, (int)(sr * 0.0200000014f / 32.0f + 0.5f))` control blocks, so the ramp
   is `32 * N` frames. Computed in `prepare` `FUN_141aa2300` (0141aa0000.c:1303-1319),
   which stores N at +0x163498. The block argument 32 comes from the wrapper's
   sample-rate slot `FUN_1409705d0` (0140970000.c:260-276): `(*core+0x18)(core, sr, 0x20)`.
   Not a per-frame or per-host-block count.
3. **Increment (CONFIRMED).** `inc_i = (T_i - L_i) * rate`, with
   `rate = 1 / (32 * N)` stored at +0x16349c (multiplied reciprocal, not a divide).
   Computed in `FUN_140acb7d0` on the tick that sees the dirty flag (9311-9359).
   Per frame, `L_i += inc_i` unconditionally in `FUN_141a7a1a0` (0141a70000.c:7080-7095)
   and its twin `FUN_141a7b670` (7977-7992), inside the per-frame loop (7072 to 7793).
4. **Resets (CONFIRMED).** `prepare` does NOT reset the ramp state. It writes N, rate,
   sr and 1/sr, then calls setters 0, 1, 9, 10. Those dirty lanes 0, 1, 3, 4, so they
   glide from their current live values to the targets over N blocks. Reset
   `FUN_141a9b630` (0141a90000.c:6731-6850) does not write any ramp word.
   It only recomputes the predelay read index from live lane 0 (line 6834).
   The full clear is `FUN_141a9eb50` (vtable slot 1, 0141a90000.c:9921), which the factory
   `FUN_1407cc440` calls right after constructing the core (01407c0000.c:7825).

## Call chain (CONFIRMED unless marked)

```
host -> wrapper slot 2  1409705d0   sets sr, (*core+0x18)(core, sr, 32) = prepare
host -> wrapper slot 23 1408fbf60   process; loops over control blocks of 32 frames
   per control block:
     for k in 0..13: if armed[k] (wrapper +0x163680+k, writer UNKNOWN)
                       core slot 12 (0x60) = setter 141ac7280 (k, clamp(value, 0, 1))
     core slot 8 (0x40) = FUN_141ab4dc0 -> FUN_140acb7d0(core+0x163430)   [ramp writer]
   core slot 7 (0x38) = 141aaa9b0 -> 141a7a1a0 (ch > 2) or 141a7b670 (else)
                       per-frame: L += I, reactions gated by active byte
wrapper slot 3  140835db0 (copy state) -> core slot 19 (0x98) 141a917b0
```

The wrapper process at 1408fbf60 passes `iVar6 = min(countdown, remaining)` frames
to slot 7. The countdown is `wrapper+0x1bc`, reset to 0x20 (32) at each control block.
Automation values are read at the start of each control block (`local_res8` advances 4 B
per block).

Vtable rows used (`1479aa858` = PlateReverb@DSP@NI, `144f14f58` = IDSPCoreWrapper<PlateReverb>):
core slot 1 = 141a9eb50, 3 = 141aa2300, 7 = 141aaa9b0, 8 = 141ab4dc0, 12 = 141ac7280,
18 = 141a99840, 19 = 141a917b0; wrapper slot 2 = 1409705d0, 3 = 140835db0, 23 = 1408fbf60.
Core object is at wrapper +0x1c0 (factory `FUN_1407cc440`: core ctor 01407c0000.c:7821,
defaults init 7825). CONFIRMED.

## Ramp state (core byte offsets)

| offset | size | name | written by | read by |
|---|---|---|---|---|
| +0x163430..+0x16344c | 8 f32 | live L0..L7 | tick (snap), process (+= inc), clear 141a9eb50 | process, predelay, reactions |
| +0x163450..+0x16346c | 8 f32 | inc I0..I7 | tick 140acb7d0 only | process (+=) |
| +0x163470..+0x16348c | 8 f32 | target T0..T7 | setter 141ac7280, copy 141a917b0, default 141a9eb50 | tick |
| +0x163490 | u8 | dirty | setter, copy 141a917b0 (set); tick (clear) | tick |
| +0x163491 | u8 | active (ramp running) | tick (set 1 on start, 0 at end) | process (gates reactions) |
| +0x163494 | i32 | countdown (blocks) | tick (= N on start); decremented each non-dirty tick | tick |
| +0x163498 | i32 | N (blocks) | prepare 141aa2300 (line 1315) | tick |
| +0x16349c | f32 | rate = 1/(32 N) | prepare (line 1317) | tick |
| +0x1634a0 | i32 | state | tick (2 on trigger, 3, 4 idle, 0) | tick |
| core +0x8 | u8 | instant flag | UNKNOWN (see U2) | setter (copies T to L at once if set) |
| core +0x64 | f32 | sample rate | prepare | predelay |
| core +0x68 | f32 | 1/sr | prepare (computed line 1311, stored 1316) | - |
| core +0x1633f0 | i32 | predelay write index | prepare (0x17763) | process |
| core +0x1633f4 | i32 | predelay read index | setter 0, reset | process |

Lane-to-parameter map (from the setter switch 141ac7280):

| lane | target | setter case (wrapper id) | field | use (process) |
|---|---|---|---|---|
| L0 | T0 +0x163470 | 0 (predelay, ms * 250) | +0x44 | read index = w - int(L0 * 0.001 * sr), clamp >= 1, wrap +0x17763 (7118-7129) |
| L1 | T1 +0x163474 | 1 (decay) | +0x48 | `FUN_141abcff0` line biquad rebuild while I1 != 0 (7097) |
| L2 | T2 +0x163478 | 7 | +0x4c (default 0.1) | role UNKNOWN (read at 7649) |
| L3 | T3 +0x16347c | 9 (low) | +0x54 | shelf gain `powf(10, L3 * 1.71875 - 1.4375)` -> 141a92270 (7101-7106) |
| L4 | T4 +0x163480 | 10 (high damp) | +0x50 | line damping `L4 * 4851 + 6000` Hz; gain `powf(10, L4*0.2 - 0.2)` -> 141a92080, 141a972c0 (7108-7116) |
| L5 | T5 +0x163484 | 13 (0xd) | +0x58 (default 1.0) | pan / stereo (7131-7178) |
| L6, L7 | T6, T7 +0x163488, +0x16348c | none | - | no setter case writes them (inert, INFERRED) |

Wrapper ids 2..6, 11, 12 have no setter case (no ramp). Id 8 stores +0xf0 only, no ramp.

## Smoother state machine (`FUN_140acb7d0`, CONFIRMED)

Dirty branch (flag at +0x163490 set):
1. clear the dirty byte; state := 2.
2. For lanes 0..7 (two loops of four): `inc = (T - L) * rate`. If `inc*inc < 1e-15` (double
   compare, `DAT_144fc3270`), then `inc = 0` and `L = T` (snap). Accumulate `sum += inc*inc`.
3. If `sum > 0`: countdown := N (+0x163498), active := 1 (+0x163491). Else state := 4 (no ramp).
Idle branch (dirty clear):
1. countdown -= 1 (int, no clamp).
2. If countdown == 0: `L = T` for all 8 lanes, incs := 0, active := 0, state := 4, return.
3. Else: state 4 -> 0; state 2 -> 3.

Timing: trigger at tick k sets countdown = N. The increments act on the frames of blocks
k .. k+N-1 (32 N frames). The snap happens at tick k+N. Dirty set again before the
countdown ends restarts the ramp from the current live values, with countdown reset to N
(see U1 for the consequence when the flag stays armed).

Process loop reactions (per frame, gated by active byte at +0x163491 and the lane's own
increment): lane 1 (line biquads), lane 3 (low shelf), lane 4 (damp), lane 0 (predelay
index), lane 5 (pan). The reactions are recomputed every frame while the ramp is active.

## Constants (rdata constants.tsv)

| address | f32 | decoded | used for |
|---|---|---|---|
| DAT_147697658 | 0x3ca3d70b | 0.0200000014 | ramp length (seconds) in prepare, line 1303 (CONFIRMED) |
| DAT_1447148b8 | 0x3ca3d70a | 0.0199999996 | 141a842e0 cases 0,1 (display/unit map), NOT the ramp (see correction C2) |
| DAT_14470bad0 | 0x3f000000 | 0.5 | rounding in N (prepare line 1310) |
| DAT_14470bad4 | 0x3f800000 | 1.0 | clamps, 1/sr, rate numerator |
| DAT_1447148a4 | 0x3a83126f | 0.001 | predelay ms -> s |
| DAT_144fc4a18 | 0x437a0000 | 250 | predelay ms target scale (setter case 0) |
| DAT_144fc3270 | f64 | 1e-15 | snap threshold on inc^2 (double) |
| literal 0x17763 | - | 96099 | predelay wrap and initial write index |
| literal 0x20 | - | 32 | control block size (wrapper), prepare block argument |

## Corrections to docs/re/engine/k-reverbs.md (not edited here)

- C1. "tap offset `int(sr·0.005)` term" in the prepare section is not what prepare does.
  The int at +0x163498 is the ramp block count `int(sr·0.02/32 + 0.5)` (CONFIRMED,
  0141aa0000.c:1310-1315).
- C2. "Ramp duration constant DAT_1447148b8 = 0.02 s" is the wrong constant for the ramp.
  Prepare uses DAT_147697658 (0.0200000014). DAT_1447148b8 is used by `141a842e0`
  (switch cases 0 and 1: `(x - c) * 0.02`), whose callers are in 01407b0000.c (5338-5523,
  wrappers 1407b9740 etc.). Not Plate.
- C3. "+0x68 = 1/sr (INFERRED)" is CONFIRMED: prepare computes `1.0 / sr` at
  0141aa0000.c:1311 and stores it to `param_1 + 0xd` (+0x68) at line 1316.
- C4. "The increment writer and the ramp length are NOT located" is now resolved:
  writer 140acb7d0 (via slot 8), length in prepare, caller in wrapper 1408fbf60.
- C5. Related sibling: `141aa1280` (0141aa0000.c:731, 753 B, another class with sr at
  +0x8) uses the same `int(sr * 0.02 / bs + 0.5)` form and `fVar3 = sr*DAT_147697658`.
  INFERRED that it is the same generic smoother law. Not studied.

## Open items

- **U1 (UNKNOWN, top priority).** Writer of the wrapper armed array +0x163680 (14 bytes).
  Only the reader (1408fbf60, 7918) exists. No writes found under any spelling
  (`0x163680`, `-0x1636xx`, `0x1636xx`). The process never clears these bytes.
  Consequence: if they stay armed, the setter runs and dirties the core every control
  block, so the tick retriggers every 32 frames. Then each block moves L by
  (T - L)/N, which is a one-pole approach with tau = N blocks (about 20 ms), not the
  linear 20 ms ramp. If they are one-shot change flags cleared elsewhere, the ramp is
  linear. Needs a native step test (Kontakt capture) or the writer.
- **U2 (UNKNOWN).** Writer of the core instant flag at +0x8. The setter reads it; no
  Plate writer found (0141aa0000.c:472 is FXFDelayLight-class). Consequence: if it is 0
  at first prepare, the defaults cleared by 141a9eb50 (live = 0) glide up to the defaults
  (decay 0.5, low 0.5, damp 0.5) over N blocks after the first prepare.
- **U3 (UNKNOWN).** Role of lane 2 (+0x163438, param id 7, +0x4c). Not traced past its
  read at 7649.
- **U4 (UNKNOWN).** Host caller of wrapper slot 2 (sample-rate change).
- **U5 (INFERRED).** Slot 7 `141aaa9b0` is a tail-call dispatcher; the decompile drops its
  arguments. Frames passed = iVar6 (<= 32).

## Port consequence

A per-frame ramp in our Plate needs, per core:
1. State: 8 live, 8 inc, 8 target, countdown, N, rate, state, dirty, active. Live and
   target are per lane; param ids map as in the lane table above.
2. Control rate of 32 samples. Sample each parameter at the start of each 32-sample block.
   A change sets dirty. In the block tick, if dirty: `inc = (T-L) * rate`,
   `rate = 1/(32*N)`, `N = max(1, round(sr*0.02/32))` (trunc of +0.5). Snap if inc^2 < 1e-15.
   Set countdown = N and active = 1 if any inc survived; otherwise settle now.
3. Per sample: `L += inc` for all 8 lanes. While active, recompute the reactions
   (predelay index, line biquads, low shelf, damp, pan) at least per control block.
   Per-sample recompute is what Kontakt does; a per-block recompute is a documented
   deviation, not a bug.
4. Countdown end: `L = T`, incs = 0, active = 0.
5. Prepare: recompute N and rate for the new sr. Do not clear live lanes, countdown or
   state. Re-arm predelay, decay, low and damp so they glide to the new targets. A
   reset (the `141a9b630` analogue) must not clear ramp state.
6. The repo's current `int(sr*0.02/bs+0.5)` uses the host block size. Kontakt uses 32
   (the control block). At 48 kHz with a 512-sample host block that gives N = 2 (1024
   frames) instead of N = 30 control blocks (960 frames), and Kontakt re-arms only at
   32-sample granularity. Use 32 for N and count ramp frames, not host blocks.
7. Do not ship the retrigger-every-block behaviour (U1) until the writer or a native
   step test settles it. Until then a linear ramp is the documented choice (INFERRED).

Parity needs a native step capture (decay or low, 48 kHz): 960 linear frames if U1 is
one-shot, or an exponential tail if the flags stay armed.

## Implementation

In the repo (branch `engine/plate-ramp`):

- `crates/sampler-core/src/dsp/space/design/plate.rs`: `LaneRamp`, a private 8-lane
  smoother with `set_target`, `tick_block` and `frame`. Lane 0 predelay, 1 decay,
  2 core 7, 3 low, 4 high damp, 5 stereo. Lanes 6 and 7 have no setter case and stay
  unused at 0. `N = max(1, int(sr·0.02/32 + 0.5))` (the f32 constant `0x3ca3d70b`),
  `rate = 1/(32 N)`, `inc = (T - L)·rate`, snap when `inc² < 1e-15`, countdown N
  blocks. The ramp is linear.
- Tests (decompile-derived, one per behaviour): N for 48 kHz = 30 and 44.1 kHz = 28;
  the lane reaches the target after N blocks and not before; the per-frame increment
  is `(T - start)/(32 N)`; the snap rule. `impulse_matches_native` is unchanged.
- Coefficients are not rebuilt by the smoother. The ponytail note on `LaneRamp` names
  the per-block rebuild (native: per frame) and the upgrade path.

Open: U1 (writer of the armed array `+0x163680`) is still not found. The ramp is linear
until it proves otherwise. A native step capture settles it.

Live path (checked 2026-10-22): none. The Plate lowers to a constant
`Processor::Space` (`sampler-kontakt` `effects/space.rs` `plate()`, `slot_plate`).
`Config::network` has `address: None`, and `Config::lanes()` returns nothing for
`KontaktPlate` (only SparkVerb and Freeverb have lanes), so `live_knobs` is `None`.
The only `Space::retune` caller (`dsp.rs`, the `ProcessorRef::Space` arm) needs live
knobs. So `LaneRamp` is not reachable from a running instrument.

No native ramp vector exists. Parity still needs a Kontakt step capture.
