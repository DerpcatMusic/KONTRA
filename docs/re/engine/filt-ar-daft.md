# Engine unit: AR LP/HP/BP (2, 4, 2/4) and Daft / Daft HP (Kontakt 8)

Status: decompiled Kontakt 8.13.1 (`source/kontakt8/`, READY) and the image constants read from
`re-binaries/Kontakt8.exe` confirm the AR cutoff law and the Daft loop, gain, cutoff, feedback,
resonance, output, resampler and knees. Kontakt-recorded audio is still not compared (Wine oracle
not run for these units). Confidence: high on constants and recurrence order, medium on start-up and
ramp-clock timing (see open items).

Scope: Kontakt 8 only. Code: `crates/sampler-core/src/dsp/filter/` (`ar.rs`, `ar_kernel.rs`,
`daft.rs`, `table.rs`, shared `linear.rs` `Zdf`, `shaper.rs` `Knee`, `dsp/kaiser.rs` x2 resampler);
mapping: `crates/sampler-kontakt/src/effects/`; neutral config: `crates/sampler-ir/src/{fx/filter.rs,lib.rs}`.

## Shared pieces

- **Table** (`filter/table.rs`): one exponential table for AR and Daft. `FUN_1404d14d0` fills
  `T[i] = f32(pow(2, i*(1/60) - 20))`, i in 0..2400 (constants `2.0`, `1/60`, `20.0` read as f64 from
  `0x144714998`, `0x144714938`, `0x144761788`). Lookup (`FUN_140b04a30`, `FUN_140b04c80`):
  signed 16-bit index; negative reads 0; interior `frac * (T[i+1]-T[i]) + T[i]`.
  Past index 2400: Daft reads the native tail `0x14a6b7100` (`1.7526e-37`, then clamps to 1 Hz);
  AR reads the last entry (its native vectors require it; the decompile's AR path is not
  separately confirmed here). `dynamics/computer.rs` keeps the last-entry behaviour.
- **Knees** (`filter/shaper.rs`): `Knee { limit, slope }`. AR's state knee is `Slope::Direct(0.0005)`
  at limit 1000 (`c - (|c| c) w`). Daft's two knees are `Fused` (below).
- **Zdf** (`filter/linear.rs`, filt-sv): AR sections run on it bit-identically.
- **x2 resampler** (`dsp/kaiser.rs`): `FUN_140ae22e0` configures two objects per Daft channel:
  `(rate, 2, 1, 1)` selects mode 4 (`FUN_140ae2c40`, x2 up) and `(rate, 1, 2, 1)` mode 5
  (`FUN_140ae3300`, x2 down). Both are the Kaiser 16-tap x2 pair already in `kaiser.rs`; the
  wrapper `FUN_140ae2190` processes whole blocks, which is equivalent to per-sample up then down.

## AR family (saved 100..=108)

### Identity and modes
Saved 100..108 → internal 76..84 → `BFilterDJ` (wrapper selector `FUN_140af0080`, core `FilterDJ`).
Mode table as before: 100→0 2-pole LP, 101→1 BP, 102→2 HP, 103→3 4-pole LP, 104→4 4-pole BP,
105→5 4-pole HP, 106→6 combined LP, 107→7 combined BP, 108→8 combined HP.

### Laws (confirmed against `FUN_140b04a30`, `Kontakt8.exe`)
- Cutoff position: `min(x*145, 140) * 5 + 1381.881591796875`; constants 145.0 (`0x144fcfe48`),
  140.0 (`0x144fcfe44`), 5.0 (`0x1447149f0`), 1381.88 (`0x144f38d20`).
- Hz via the shared table. Pole polynomial: `((f^4 w^3 c1 + f^2 w c2) w + pi) f w` with `c1 = 40.80263`
  (`0x144fcfe28`), `c2 = 10.335426` (`0x144fcfe04`), `w = 1/rate`, `pi = 0x1447149d4`. Matches
  `ar_kernel::targets` term for term, including the left-to-right order.
- Section, detector, floor and gain constants: covered by the 243 static/ramp vectors
  (`ar_native_vectors.json`, binary identity recorded in `w15-ar-native.md`) at the 2e-6 tolerance;
  the Rust checkpoints pass (6 focused tests) and the golden hash `ar_kernel::tests::output_is_bit_stable`
  holds across the refactors below.

### Refactor done
- `ar_kernel` section runs `Zdf::tick` with `STATE_KNEE` (`Slope::Direct(0.0005)`, limit 1000). The old
  `saturate` is deleted; old and new knee and section arithmetic agree bit for bit (2e6 knee,
  2e5 section probes). End-to-end hash unchanged.
- `ar_kernel`'s own table and lookup moved to `table.rs` (`Past::LastEntry`).

### Gap table (AR)
| Behaviour | Status |
|---|---|
| Cutoff position, Hz table, pole polynomial | confirmed (decompile + image constants) |
| Table recipe | confirmed (`FUN_1404d14d0`), now shared |
| Section / detector / state knee | covered by vectors (2e-6); `Zdf` + `Knee` bit-identical to the old arithmetic |
| Past-table lookup | AR vectors say last entry; the decompile's cutoff function writes the tail. Unreachable (max index 2081), recorded, not changed |
| Start-up of lanes and ramp clock | covered by vectors for the tested ramps; native ctor path not modelled |
| Host recording (Kontakt-rendered audio) | **open**: Wine oracle not run |
| CPU at 300 voices | **open**: quiet run not done |

## Daft (saved 70 = LP, 71 = HP)

### Identity and parameters (confirmed, `FUN_140b05900` dispatcher)
Setter index: 0 gain (`FUN_140b05290`), 1 cutoff (`FUN_140b04c80`), 2 resonance (`FUN_140b05900`
case 2), 3 response (`FUN_140b05900` case 3). Object `NI::KONTRAFX::FilterDaft` (`FUN_140af4550`
ctor), prepare `FUN_140afd210(rate, block)`, process `FUN_140affa30`, ramp `FUN_140acb6c0`.

| Lane (ramped) | Native target | Law (confirmed) |
|---|---|---|
| 0 gain amplitude (`0x48e50`) | `lookup(x*119.589409 + 1200)` | no clamp on the signed leading value (`0x144fc5450`, `0x144fc4b50`); unity at 0; +12 dB at 1 |
| 1 cutoff Hz (`0x48e54`) | `clamp(lookup(x*625 + 1481.881592), 1, 30000)` | `0x144fcfe6c`, `0x144fcfe80`, `0x144fcfea4` |
| 2 feedback (`0x48e58`) | `1.6 - min(hz*0.5, 14000) * 5.714285725844093e-5` | `0x144714964`, `0x144fcfea0`, `0x144fcfc8c`; derived from the target cutoff |
| 3 resonance (`0x48e5c`) | `1 - (1-x)^2` (f32 order as native) | `FUN_140b05900` case 2 |
| response (immediate, not ramped) | LP weight 1 and HP weight 0 below 0.5; the reverse from 0.5 | `trunc(2x)` clamped to 0..1, set by `FUN_140b05900` case 3 |

Ramp (`FUN_140acb6c0`): per control quantum, countdown on the object's clock; delta
`(target - current) * (1 / (quanta * 32 * 2))` in f32, snapped below 1e-15; `quanta = trunc(2 rate
* 0.002 / 64 + 0.5)`, at least one (`0x144f3860c` = 0.002). Lanes advance once per 2x sample.

### Per 2x-sample loop (`FUN_140affa30`, transcribed in f32 with the native order)
Constants read from the image: stage A series `pi^4/120`, `pi^2/12`, `pi/2`; stage B series
`2 pi^4/15`, `pi^2/3`, `pi` (all at `0x144fcfd40..`, `0x144fcfe08..`, `0x144714960`, `0x1447149d4`);
drive `0.6` (`0x144725080`); damping `0.8` (`0x144f38820`); output trim `1.2` (`0x144fcfd68`); output
gain `1.8` (`0x144714980`); knees: `(1 - 0.5|c|) c` at clamp 1 (`0x14470bad4`, `0x144714a54`) and
`(1 - 0.125|c|) c` at clamp 4 (`0x144f39df4`, `0x144f38e04`, `0x1447149e4`).

Stage A runs at the quarter angle `tan(pi f / (4 rate))`, stage B at the full angle
`tan(pi f / (2 rate))`, both as the native truncated series. Each sample: input `gain * x * 0.6`;
a two-stage zero-delay update with states `[band-high, band, low]`; the first knee on the high
state, fed back with the feedback lane; the second knee on the stage output; output
`((sat - low) * HP + low * LP) * (1.8 - 1.2 r^2)`.

Resampling: Kaiser x2 up before the loop, x2 down after (`dsp/kaiser.rs`). The previous stand-in
(linear up, two-tap down, soft rational saturator, single tan) is removed.

### Changes made in this pass (native vs previous kernel)
- Loop replaced: previous single-stage TPT with the rational `soft` is gone. The native two-stage loop
  and both knees are in `daft.rs`; the rational `soft` and the local 2x helpers are deleted
  (`filter/oversample.rs` removed).
- Feedback is a ramped lane derived from the target cutoff (was recomputed from the ramped cutoff).
- Cutoff is clamped to 1..30000 Hz only (was also clamped to 0.45 rate).
- Ramp quanta uses the native rounding (was `ceil`): 22.05 kHz gives 1 quantum, not 2.
- Gain is unclamped in the kernel (signed input), and the Kontakt mapping no longer clamps
  `extra[0]` to 0..1 (native setter takes the signed value).
- Table and tail: shared `table.rs`, Daft reads the native tail past index 2400.
- Golden hashes recorded from this kernel (`daft::tests::output_is_bit_stable`, responses LP and HP).
  The old hashes are invalid by design.

### Gap table (Daft)
| Behaviour | Status |
|---|---|
| Gain, cutoff, feedback, resonance, response laws | confirmed (image constants, setter dispatch) |
| Table recipe and tail | confirmed (`FUN_1404d14d0`, `FUN_140b04c80`) |
| Two-stage loop, constants, order | transcribed from `FUN_140affa30`; golden hash guards it; **not yet confirmed against Kontakt audio** |
| Knees (clamp 1 / 0.5 and clamp 4 / 0.125) | confirmed as constants; placement as in the loop |
| 2x resampler | confirmed as Kaiser x2 (mode 4 up, mode 5 down); reuses `kaiser.rs` (its taps are documented there) |
| Ramp clock | approximated: 32-frame control quantum, countdown and snap as `FUN_140acb6c0`; the caller clock (`FUN_140b03620`, `FUN_140b00b80`) not traced in full |
| Start-up | **open**: native lanes start at zero in the ctor and ramp in; this kernel snaps to the first target |
| Response mode ramp | native sets the weights immediately; matched |
| Kontakt audio | **open**: Wine oracle (level sweep at several gain and resonance settings) not run |

## Open items (in order)
1. Trace the native ramp caller (`FUN_140b03620`, `FUN_140b00b80`) for the countdown and start-up.
2. Wine-oracle level sweep of Daft LP and HP (gain 0 and +1, resonance 0 and 1, cutoff 0.2 / 0.6 / 0.95)
   against this kernel's probe response (recorded: passband gain 1.08 at zero resonance; LP cutoff
   sweeps monotone; HP mirrors LP).
3. AR past-table tail: resolve the decompile (tail) vs vector (last entry) conflict if an AR input
   ever reaches index 2400.
