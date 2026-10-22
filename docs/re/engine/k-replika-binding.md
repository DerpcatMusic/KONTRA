# Kontakt 8 Replika: setter index to control, under kemu emulation

Scope: the core `NI::DSP::FXFDelayLight` (object 0x14980 B) run unchanged under `tools/re/kemu.py`
(no host, no Wine, no Kontakt). Harness: `tools/re/replika_kemu_vectors.py`. Follows
`k-replika-core.md` and `k-replika-gaps.md`. Vectors: `vectors/replika-native-*.json`.

Method. Ctor `141a7f4d0`, slot 1 init `141a9e300`, slot 3 prepare `141aa1580` (48000 Hz, 32-frame
quantum), then per quantum: setter `141ac5980` for each routed index (clamped to 0..1 as the wrapper does),
update `141ab4a10`, process `141aa9480` (stereo). The slot addresses are asserted against the object vtable.
Defaults: the reset constants of `1408ddbc0` (checked in `constants.tsv`: `144f38814` = 0.762405,
`144f38764` = 0.3, `14470bad0` = 0.5, `14470bad4` = 1.0), written before prepare. The reset function
itself is not emulated (it goes through the wrapper vtable `+0x100`).

Harness choices, not Kontakt defaults: TIME (index 8) = 0.02 (so the echo lands inside the 4096-frame
window), HIGHCUT (index 3) = 1.0 (neutral; raw 0 would be the 200 Hz cut). Index 52 has no reset entry;
its implicit value is 1.0 (observed: writing 1.0 leaves the output unchanged, 0.0 silences it).

Binding procedure. For each index: setter(i, 0) and setter(i, 1), everything else at default, 4096
frames. Three stimuli: (a) impulse in L, default bases; (b) two-channel noise (seeded, +/-0.25),
default bases; (c) two-channel noise with feedback (index 5 = 0.8) as base. Plus the core-object state
diff after one setter call (no processing): bytes changed, labelled C+offset or P0..P5+offset, where
P0..P5 are the 112-byte control blocks at the triple pointers (`C+0x80` .. `C+0xf8`).
Effects are in the delta column; "state" lists what the setter writes.

Labels: CONFIRMED = emulated delta and state agree with the decompile text. INFERRED = name from the
shape of the emulated response. UNKNOWN = no name.

## Index table

| idx | control (label) | emulated delta (v=0 vs v=1, others default) | state written |
|---|---|---|---|
| 0 | input level A (INFERRED) | v=0: L silent; v=1: x3.98 (energy 15.8); default 0.762 = unity | P0, P2, P3, P4 +0x0 |
| 1 | input level B (INFERRED) | identical to 0 with L-only input; not separated (R-only test open) | P0, P2, P3, P4 +0x4 |
| 2 | LOWCUT (INFERRED name) | v=1: highpass, impulse peak 1.00 -> 0.88, energy 1.00 -> 0.91; v=0 = default | F1, F2 sections |
| 3 | HIGHCUT (INFERRED name) | v=0: lowpass at 200 Hz, peak 0.32, energy 0.106; v=1 neutral | F1, F2 sections |
| 4 | dry/wet balance (INFERRED) | v=0: dry impulse only at frame 0, echo gone; v=1 (default): wet only | P0, P2, P3, P4 +0x8, +0xc |
| 5 | FEEDBACK (INFERRED from recirculation) | v=0.8: second echo at frame 4024 (0.96 of the first; energy 1.92); v=1: sustained tail (energy 2.44, late 1.44); default 0: none | P +0x18; head 2 and F1 state |
| 6 | sub-effect bypass A (+0x914) | v=0 and v=1 both silence the default output | C+0x914; bypass fade (Ctl 0x60) |
| 7 | sub-effect bypass B (+0x915) | v=0 and v=1 both silence the default output | C+0x915; bypass fade |
| 8 | TIME (CONFIRMED anchor) | echo frame = 96000 v + 2 - 600 v15: v=0.01 -> 782, 0.02 -> 1742, 0.03 -> 2702 (2000 ms x v) | head 1/2/3 +0x58 (speed ratio), head time fields |
| 9 | TYPE (INFERRED) | v=0: mode 0, echo 1622 (vs 1742); v=1: mode 3 = default, no change | C+0x93c (mode); head fields |
| 10 | extra delay / bypass fade (UNKNOWN role) | v=1: echo 1742 -> 2382 (+640 frames); v=0 = default | P +0x30; P +0x60 via fade |
| 11 | delay scale (UNKNOWN role) | v=0: impulse at frames 0-63, no echo after 64; v=1: nothing in the window; default 0.5 -> 1742 | P +0x34 (smoother 1) |
| 12 | head gain, echo level (INFERRED) | v=0: echo removed (peak 0.11, energy 0.013 vs 1.0); v>=0.5: identical to default | head 1, 2, 3 gain targets (+0x584c) |
| 13 | dry pan pair (INFERRED) | silent at default (dry is off, index 4 = 1.0); with index 4 = 0: v=0 vs v=1 differ (max 0.5) | P +0x1c, +0x20 |
| 14 | wet L gain 2(1-v) (INFERRED pan) | v=0: x2 (peak 2.0, energy 4); v=1: silent; default 0.5 = unity | P +0x24, +0x28 |
| 15 | delay offset (INFERRED) | echo -600 frames per unit: v=0 -> 1922, default 0.3 -> 1742, v=1 -> 1322 | P +0x38 (smoother 3) |
| 16 | WIDTH (INFERRED; law CONFIRMED by energy) | L = (0.5 + v) tap, R = (0.5 - v) tap: v=0 energy 0.5, v=1 energy 2.5; default 0.5 gives R = 0 | P +0x64 (side scale) |
| 17 | DUCKING SENSE (name INFERRED; case 0x11 CONFIRMED): gate threshold on \|mid\| = max(1-v, 0.001) | burst (sense 0.8, amount 0.5): echo peak 0.50 -> 0.081; sense 0 with amount 0.5: identical to amount 0 | C+0x13e18 |
| 18 | DUCKING AMOUNT (name INFERRED; case 0x12 CONFIRMED): gate target -60 v dB | burst (sense 0.8): amount 0 identical to default; amount 0.5 echo energy 504 -> 3.86 | C+0x13e10 |
| 19 | DUCKING RELEASE (name INFERRED; case 0x13 CONFIRMED): closed-branch factor 0.05^(1/((200 + 7300 v) x 0.001 x rate)) | burst (sense 0.8, amount 0.5): release 0 energy 3.86, release 1 (7500 ms) energy 0.53 | C+0x13e28 |
| 20 | raw store only | none (raw store C+0x840+4i) | none |
| 21 | raw store only | none | none |
| 22 | raw store only | none | none |
| 23 | raw store only | none | none |
| 24 | raw store only | none | none |
| 25 | double slope 280 v + 20 (UNKNOWN role) | none (stimuli a, b, c) | C+0x928, C+0x930 |
| 26 | candidate select flag (+0x916) | none | C+0x916 |
| 27 | raw store only | none | none |
| 28 | PATH SELECT = head mode (CONFIRMED by emulation, name INFERRED): k = min(4, trunc(5 v)) | k = 1 (v 0.2-0.4): mode 1, max diff 0.33 vs default (noise); k = 2 (v 0.4-0.6): mode 2; k = 3 (0.6-0.8): mode 3 (not emulated); k = 0 or 4 (v < 0.2, v >= 0.8): mode 0 | raw C+0x8b0; C+0x604, C+0x830 = k; heads H+0xc0 = k; kernel select H+0x270/H+0x340 (kernel 1 at k = 1, kernel 2 at k = 2, observed); F1, F2 +0x4 |
| 29 | SATURATION (CONFIRMED anchor) | v=0.5: peak 0.78 (vector); v=1: peak 0.32, energy 0.10 (default 1.0) | C+0x142b0 target (live: C+0x142ac); drive/gain copies |
| 30 | rotator coefficients, LFO rate (UNKNOWN consumer) | none | C+0x980..0x997; P +0x3c |
| 31 | head sub-state; echo moves (UNKNOWN role) | v=1: echo 1742 -> 2116, energy 0.14 | heads 1, 2, 3 sub-state |
| 32 | head sub-state (UNKNOWN) | none | heads 1, 2, 3; F1 |
| 33 | raw store only | none | none |
| 34 | sub-object rebuild (321 B; calls 141aca670, 141abbf60) | none | head and gate state |
| 35 | pow law P(v) (UNKNOWN role) | none | P +0x40 |
| 36 | raw (UNKNOWN role) | none | P +0x44 |
| 37 | head sub-state (UNKNOWN) | none | heads |
| 38 | raw store only | none | none |
| 39 | sub-object rebuild (148 B) | none | lane/sub-object state |
| 40 | raw (UNKNOWN role) | none | P +0x4c |
| 41 | raw (UNKNOWN role) | none | P +0x50 |
| 42 | sub-object (35 B) | none | heads, lane state |
| 43 | sub-object rebuild (69 B; calls 141acb2b0) | none | head and gate state |
| 44 | pow law P(v) (UNKNOWN role) | none | P +0x54 |
| 45 | raw (UNKNOWN role) | none | P +0x58 |
| 46 | head sub-state (UNKNOWN) | none | heads |
| 47 | raw store only | none | none |
| 48 | sub-object rebuild (644 B) | none | F and lane state (C+0x12240..) |
| 49 | heap sub-object (26 B) | none | heap state |
| 50 | sub-object (100 B) | none | F and lane state |
| 51 | sub-effect gate parameter (CONFIRMED by state only) | none | gate objects C+0x11600, C+0x12a80 (1100 B) |
| 52 | tap gain, Ctl 0x68 (INFERRED) | v=0: silent; v=1 = default (implicit 1.0) | P +0x68 |

Anchors checked: TIME 8 (confirmed: 2000 ms per unit at mode 3, `k = 0.5`), LOWCUT 2 and HIGHCUT 3
(confirmed by response shape; both written into F1 and F2), SATURATION 29 (confirmed), TYPE 9 (confirmed
as mode switch; the name follows from the TSV). FEEDBACK is index 5, not 12: index 12 is head-1 gain.

## Stimulus-specific results

- Under noise with feedback at 0.8 (stimulus c), no index in the 33 dormant rows changed the output
  (rows 13, 17-27, 30, 32-51). Gate, LFO and ducking terms need their partners (index 17 for the gate,
  index 4 for dry, index 5 for the feedback path) to be active; single-index tests from default cannot see them.
- Targeted pairs: 13 with index 4 = 0; 17, 18 and 19 with index 5 = 0.8 and index 17 or 18 set.
  These are the only evidence for rows 13, 17, 18 and 19 above.
- Ducking is indices 17, 18, 19 (the gate of `141aa9480`, steps 10 and 14). At the reset sense
  (threshold 1.0) the gate never opens on the impulse (|mid| <= 0.5), so the impulse binding could not show it. Under a 1 kHz burst (see
  Ducking below) each index moves the echo. The earlier note that `141abd290` is the ducker is wrong: it is
  the head position update (GAP-M). Candidates 25 and 26 change flags only, not verified.
- No index moves the output through an LFO. The rotator coefficients are written by index 30
  (C+0x980..0x997), but its consumer (GAP-I) is not in the loop as read.

## Default impulse (vector `replika-native-default.json`)

Input L impulse at frame 0, TIME 0.02, default bases. L: one sample of 1.000024 at frame 1742
(36.3 ms). R: zero. Energy 1.00005 (single sample). Plausible: the nominal 40 ms TIME minus 3.7 ms
from index 15 (default 0.3, -180 frames). No dry (index 4 = 1.0), no recirculation (index 5 = 0), width
0.5 gives R = 0 for an L-only input, gain 1.0 at the default bases.

## Blockers hit under emulation

| Where | Blocker | Fix |
|---|---|---|
| ctor `141a7f4d0` reaches `141a7d920` (decompile line 9295) | import `?_Random_device@std@@YAIXZ` (noise seed) not stubbed | kemu.py stub with a fixed seed 0x2545F491 (deterministic noise state; the noise output is not a Kontakt-exact vector) |
| prepare `141aa1580` (ring and buffer zeroing up to 4000 ms) | exceeds the 4M-instruction call budget | harness budget 400M for prepare only; prepare then runs in seconds |
| kemu heap (512 MB, bump allocator, no free) | exhausted after ~23 renders in one process; the next `malloc` import stub (0x7f0000006250) faults | one emulator per render (`fresh()`); the process state then matches a single vector run |

No other import stops the run: setter `141ac5980`, update `141ab4a10`, process `141aa9480`, and the
table builder `1404d14d0` complete for all 53 indices and the seven cases. Global state is not reused
across renders (fresh image per render).

## Still unknown

- Names for 0/1 (which tap; R-only test), 6/7 (bypass targets), 10, 11, 25, 26, 30-32, 34-37, 39-50.
- Feedback law: the second echo is 2282 frames after the first at TIME 0.02, gain 0.96 at index 5 = 0.8.
  The decay per cycle and the index 5 law need a longer run (more than 4096 frames).
- Gate release (19) only shows during the gate transition; the tail shape needs a windowed test.
- LFO (depth, rate, modulation) has no audible index here. Ducking names (sense, amount, release) are INFERRED
  from the case laws; the saved-field tie is not confirmed (see Ducking below).
- Index 12 as head gains at v>=0.5 is a no-op in our bases; heads 2 and 3 are not separated.
- The wrapper's real default values for TIME, HIGHCUT and index 52 are not in `1408ddbc0`.

## Vectors

`docs/re/engine/vectors/replika-native-<case>.json`, keys: name, rate (48000), block (32), defaults
([index, value] before prepare), writes (per quantum [[index, value]]), stim ([L, R] float32), outL, outR.
Cases: default; time-v0p01 (TIME 0.01: echo 782); time-v0p03 (TIME 0.03: echo 2702); feedback-v0p8
(index 5 = 0.8: echo 1742 and 4024); saturation-v0p5 (29 = 0.5: peak 0.78); lowcut-v0p5 (2 = 0.5:
peak 0.99); highcut-v0p5 (3 = 0.5: peak 0.52).

## Piece 1 status (`engine/replika-port`)

Ported in `sampler-core` `dsp/replika` and checked against the native vectors (max abs error 1.19e-7 to 2.38e-7).
"Wired" means `replika_config` sets the control from the saved Kontakt value; "core" means the core runs it with the
reset value otherwise.

| idx | control | piece 1 |
| --- | --- | --- |
| 0, 1 | input level A, B | core (law in `k-replika-gaps.md`, piece 1); not wired (reset 0.762405) |
| 2 | LOWCUT | core, wired |
| 3 | HIGHCUT | core, wired |
| 4 | DRY/WET | core; not wired (reset 1.0, wet only) |
| 5 | FEEDBACK | core, wired (1.2 x value) |
| 8 | TIME | core, wired (mode 3, 2000 ms x value) |
| 12 | HEAD GAIN | not ported (floor unbacked) |
| 13, 14 | PAN DRY, PAN WET | core matrix; not wired (reset 0.5) |
| 15 | DELAY OFFSET | core; not wired (reset 0.3) |
| 16 | WIDTH | core side scale; not wired (reset 0.5) |
| 29 | SATURATION | core, wired |
| 52 | TAP GAIN | core; not wired (implicit 1.0) |

Piece 2 (ducking): indices 17, 18, 19 ported as the gate and wired from the saved DUCKING_* fields (see below).
Piece 3 (mode 1): the selector is closed (index 28, below); the mode-1 kernels are not ported.

## Ducking (piece 2)

Decompile (CONFIRMED arithmetic, `0141aa0000.c`, `0141ac0000.c`):
- Setter cases `141ac5980`: 0x11 (17) `+0x13e18 = max(1 - v, 0.001)` (3614-3618); 0x12 (18) `+0x13e10 = -60 v`
  (3621); 0x13 (19) `+0x13e28 = 0.05^(1 / ((200 + 7300 v) x 0.001 x rate))` (3624-3631). Setup `141aa1580`
  (1100-1105): rate `+0x13e00`, base `+0x13e30` = 0.05, retention `+0x13e20` = 0.05^(333.3333175 / rate),
  `+0x13e04` = 200, `+0x13e08` = 7500.
- Per frame (`141aa9480`, 5551-5558): mid = 0.5 (tapA + tapB), the taps after saturation (step 7). If |mid| < threshold:
  env = env x release; else env = (env - target) x retention + target. env in dB, starts at 0.
- Wet gain (5598-5625): T4 index = env x 9.96578407 + 1200 (both channels), so the gain is 10^(env/20);
  W = OUT x Ctl(0xc) x Ctl(0x24 or 0x28) x gain. This is the echo level, before the matrix.

Emulation (`tools/re/replika_ducking_kemu.py --trigger`, one render each; 2048 frames of 1 kHz sine at 0.5,
then silence; echo near frame 1754-4082):

| case (routes every quantum) | peak | energy | max abs diff vs amount 0 |
| --- | --- | --- | --- |
| default (no routes) | 0.500012 | 504.523 | 0 |
| amount 0 (17 = 0.8, 18 = 0) | 0.500012 | 504.523 | 0 |
| duck (17 = 0.8, 18 = 0.5) | 0.0811783 | 3.86113 | 0.483814 |
| sense 0 (17 = 0, 18 = 0.5) | 0.500012 | 504.523 | 0 |
| release 1 (17 = 0.8, 18 = 0.5, 19 = 1) | 0.0167662 | 0.531928 | 0.48419 |

Native vectors (`vectors/replika-native-ducking-*.json`, 4096 frames): `ducking-amount0` (17 0.8, 18 0),
`ducking-amount0p5` (17 0.8, 18 0.5, 19 0), `ducking-amount0p5-release1` (19 1). Port max abs error
1.19e-7, 7.45e-8 and 1.12e-8 (tolerance 1e-5).

Labels: the gate law and its three constants are CONFIRMED arithmetic and emulated. The names are INFERRED
(sense = trigger, amount = depth, release = decay). The saved tie (DUCKING_SENSE field 29 -> index 17,
DUCKING_RELEASE field 30 -> 19, DUCKING_AMOUNT field 31 -> 18) is INFERRED by name and by the saved order; no
decompile text binds the wrapper slots to the saved fields.

Not confirmed: the release factor is written by the setter from the block rate. Written before prepare
(rate 0, base 0) it is 0, which is instant release. The harness routes 19 after prepare every quantum, as
the wrapper does for routed indices; the reset default (19 = 0, no route) is only correct after prepare.
The port uses the prepared rate.

## Mode-1 selector (piece 3)

Selector: index 28, CONFIRMED by decompile and emulation. Setter case `0x1c` (`0141ac0000.c` 3648) sets flags and
calls `141aca670`; the value is the raw store `C+0x8b0`. `141aca670` (`0141ac0000.c`, 1200 B) computes
`k = min(4, trunc(5 v))` (`1447149f0` = 5.0, `constants.tsv`), writes `C+0x604` and `C+0x830`, then calls
`141aca4a0(H, uVar5, 0)` for the three inline heads (`C+0xac0`, `C+0x6380`, `C+0xbc40`), with uVar5 = k for k = 1 to 3 and 0 otherwise.
`141aca4a0` writes `H+0xc0 = mode` (`0141ac0000.c`, first line of the body). The head read `141ab7a90`
(`0141ab0000.c` 4456) takes `141ab82e0` (mode 0) when `H+0xc0 == 0`, else `141ab8c60`.

Other writers of `H+0xc0 = 1` (`141ab4e30`, `141ab5890`, `141ab69f0`; `0141ab0000.c` 2611, 3030, 3756) are not reached from the
core setter: their callers are the wrapper's lane objects (`141ac5340` case 6, `141abab20`, `141abdaf0`).
Index 9 (TYPE, `C+0x93c` = int(4 v)) does not write `H+0xc0`.

Emulation (`tools/re/replika_mode1_kemu.py`, one emulator per run; `H+0xc0` read after the setter and one
quantum):

| index 28 | 0.19 | 0.21 | 0.39 | 0.41 | 0.59 | 0.61 | 0.79 | 0.81 | 1.0 |
|---|---|---|---|---|---|---|---|---|---|
| H+0xc0 | 0 | 1 | 1 | 2 | 2 | 3 | 3 | 0 | 0 |

At 0.3 (mode 1), each 4096-frame noise run calls `141ab8c60` 12288 times (3 heads x 4096 frames), kernel
`141ab87d0` 24576 times (L and R) and the post filter `141aa3e00` 24576 times. The output moves by 0.33 against
the default. Index 9 at 0.3 moves the output by 0.49 with no mode-1 call (the TYPE path, not the head kernels).
Index 28 at 0.3 also sets the bypass and gain state of `141aca670`, so the difference is the head reader
path as a whole, not the kernels alone.

Reference vectors (emulated, not Kontakt renders, no port yet): `vectors/replika-native-mode1-index28-v0p3.json`
(impulse, peak 2.06e-4) and `vectors/replika-native-mode1-index28-noise-v0p3.json` (noise, peak 0.107, energy
2.63); 4096 frames, index 28 = 0.3 on every quantum. The mode-1 chain they need is listed in `k-replika-gaps.md`
(GAP-C).
