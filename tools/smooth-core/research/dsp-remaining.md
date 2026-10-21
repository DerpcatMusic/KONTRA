# DSP parity: the remaining items

Static evidence only (radare2 on `/tmp/opencode/rea/soothe3-restored.bin`, base
`0x180000000`; dumps under `RunScratch/smooth/feat/dsp/`). Addresses in the
dumps are the low 24 bits of the VA (`0x8900bb` is `0x1808900bb`). Each item
records what was verified, what was inferred, and what stays open.

Status legend: **verified** (read from the image), **inferred** (the form is
read, one constant is an inference), **open** (not settled).

## 4. Output trim, wet trim, mix and bypass: smoothing (verified form, inferred X)

**Ramp records.** The process function keeps four gain ramps in the engine
object, each `{target, gain, rate}` at stride `0xc`:

| record | target | gain | rate | meaning |
|---|---|---|---|---|
| `+0xbf28` | `+0xbf28` | `+0xbf2c` | `+0xbf30` | dry |
| `+0xbf34` | `+0xbf34` | `+0xbf38` | `+0xbf3c` | wet |
| `+0xbf40` | `+0xbf40` | `+0xbf44` | `+0xbf48` | non-solo (see item 1) |
| `+0xbf4c` | `+0xbf4c` | `+0xbf50` | `+0xbf54` | solo (see item 1) |
| `+0xbf58` | `+0xbf58` | `+0xbf5c` | `+0xbf60` | out |

**Step, verified** (`0x899ab0`, `callees/pdg-899ab0.c`): per sample,

```c
g = (t - g) * a + g;          // a = rate
for each channel: x[i] *= g;  // applied across the block, sample by sample
```

The helper also flushes denormal-range targets and gains to zero (the masked
compares against `0x18209ee00`). The port flushes gains below `1e-20`.

**Constructor, verified** (`A-88fa90.txt` 8900bb..890225):

```c
xmm6 = -33.3333;                         // f32 0xc2055556, read from 0x18209da5c
rate = 1.0f - EXP(xmm6 / X);            // X = float at [frame+0x218], see below
// dry/wet/nonsolo/solo/out each take rate; gains start at their initial targets
```

`EXP` is `0x181cbdf57`, a thunk through an import slot (`jmp [0x181db0820]`).
Its target did not resolve statically. The form `1 - exp(...)` is read; the
exact function is inferred from it being a one-pole coefficient.

**`X` is inferred to be the sample rate.** `-33.3333 = -1/0.03` and
`exp(-1/(0.03 * fs))` is a 30 ms one-pole at sample rate `fs`. The port's
`mix_smoothing_ms` default of 30 ms already agrees. `X` is the fourth float
argument of the state fill at `0x88fa90` (stored `xmm3 -> [rsp+0x20]` at entry,
so `[rbp+0x218]`). Its caller `0x88f430` passes it as `cvtsd2ss xmm3, xmm6`,
with `xmm6` a double from `xmm2`. The caller at `0x88f3f1` is control-flow
flattened (`jmp r9` dispatch), so the double's source was not traced. Open:
the exact `X` expression. Inferred: `X = fs`.

**Port change.** `dsp.rs` now runs three `SmoothedGain` ramps (dry, wet, out)
with coefficient `ramp_coefficient(fs, mix_smoothing_ms) = 1 - exp(-1/(tau fs))`.
The old single mix ramp used `1/N` (linear), which is the same to within
`1e-7` at 30 ms.

**Bypass, verified** (`891dd4..891ed8`): the dry target is 1, the wet target is
0, and the out target is 1 when bypassed. Delta targets are `dry = wet*mix`
and `wet = -wet*mix`, so the sum is `wet*mix*(dry - wet)`. Normal targets are
`dry = 1 - mix`, `wet = wet*mix`. The port now uses these as crossfades instead
of a hard switch.

Tests: `dsp::tests::the_output_trim_ramps_on_the_thirty_millisecond_constant`,
`bypass_crossfades_without_a_level_step`, `a_decayed_gain_is_flushed_to_zero`,
and the ported `dsp::ramp::the_mix_ramp_matches_the_reference`.

## 1. Band solo (91): output path (mix verified, filter inferred)

Evidence: `RunScratch/smooth/feat/dsp-remaining/S-solo.md`.

- Param 91 "Band solo" is discrete 0..8, stored at `+0x100` as `plain - 1`
  (`-1` = none). Port: `SmoothParams::band_solo`, `Settings::solo_band`.
- Gate (`0x8923a7..0x8923cc`): skipped when bypassed (`+0xa9`); runs when the
  solo-active byte (`+0xaa`, the soloed band's `used`) is set, or when the solo
  gain is above `0.001`.
- Solo stream: the main pointers after the dry ramp go through the band's filter
  (`0x89a1a0`, per-sample TPT state-variable filter; the mode comes from the
  band shape via `0x89bf10`, verified grouping, names inferred). The result goes
  to a per-process arena (`[rbp+0x470+8k]`).
- Mix, verified op order per sample: `slot *= g_solo` (record `0xbf4c`),
  `main *= g_nonsolo` (record `0xbf40`), `main += slot` (`0x892723`). Non-solo
  target is 1 when nothing is soloed; solo target is 1 when soloed.

Port (`dsp.rs`): `soloing_a_band_auditions_it_alone` pins it (a 1 kHz tone passes,
10 kHz is cut below 0.05). Filter tuning: frequency from `center_from_normalized`,
Q from the stored width, `k = 1/Q`.

Open:
- The shape-5 copy in `0x89a1a0` looks inverted (memcpy with the arena slot as
  the source). Not reproduced; the port's solo audio is always the filter.
- Whether the solo input is post-reduction (the reduction's buffer origin
  `[rbp+0xf20]` is not traced). The port uses the pre-out main signal.
- The 0x1370/0x10a0 kernels were verified for dispatch variant 0 only.

## Commands

```
build-slot cargo test -p smooth-core --lib
```

## 3. Latency and plan switching: partly verified, not matched

Evidence: `RunScratch/smooth/feat/dsp-remaining/L-latency.md`.

- Reported latency (verified): `L = [obj+0x2c] * H + [obj+0x278]`, with
  `H = 0` when `[obj+0x237]` or `[obj+0xab]` (low latency) is set. Otherwise
  `H = (S2c8 + S2f10)/2 + S30 + S2c78` (or `S30 + S2c78` when `P+0x58 == 0`),
  where `S = obj+0x5f8`, `P = [S]`. The earlier note's `- S2c0` term is wrong.
- `[obj+0x2c] = max(1, trunc((SR + 1.0) / 44100))` (verified, `0x18088fb42`).
- Low-latency value `[obj+0x278]`: no writer found. The plan sizes come from
  `0x18089c480` (called with 2048 and 256; inferred).
- Plan selection: setParam cases 2 and 4 set dirty bytes at `obj+0x3240/0x3241`;
  the rebuild that consumes them was not located.

Port: `EngineBank::latency()` is `n - 1 (+ m/2)` for the active plan and does
not match the reference's numbers. This is not changed: the reference values
are not recovered.

Verified in the port: moose's VST3 and CLAP glue watch `latency()` on each
block and request a host restart on a change, so a plan switch updates the
host's delay compensation. The new test
`dsp::tests::the_impulse_delay_is_the_reported_latency_in_every_plan` checks
that an impulse arrives exactly `latency()` samples late in all seven plans at
44.1 and 96 kHz.

## 2. Mid/side (16, `+0xa6`): not settled

Evidence: `RunScratch/smooth/feat/dsp-remaining/MS-sc-meter.md`.

- The only process read of `+0xa6` is at `0x892781`, used as a boolean. There
  is no channel-count gate in the mode branch.
- The helper at `0x8a3b20` computes `X0+X1` and `X0-X1`, then overwrites both
  with `g * A`, where `g` is 0.5 (site A) or 1.0 (site B). The sum and difference
  are dead stores as compiled, so no decode was found.
- No `1/sqrt(2)` constant appears in the chain. The port keeps `1/sqrt(2)` for
  encode and decode, which is not supported by this evidence. Not changed.
- Wet, dry and delta in M/S are not established.

## 5. Sidechain mux (`+0xa5`) and meter (visitor slot 13): partly verified

- No process-path reads of `+0xa5`. The detector source is chosen by whether a
  4th argument is null (`0x890bee`). The caller is vtable-dispatched and not
  resolved. The earlier note claiming a silent sidechain mutes the output is
  unsupported; no mute branch was found. Not implemented.
- Meter (verified formula): published at `0x89392a` (edx=13), gated by a visitor
  query. Value is the max over channels of the max over the block of the signed
  L2 buffers, initialised to `-FLT_MAX`. No abs, square, dB or smoothing. The
  buffers are probably the detector's, after M/S (inferred). Not implemented in
  the plugin: the telemetry channel is UI-owned and the buffer origin is not
  confirmed.

## 6. FFT forward scale: not re-attempted this round

The earlier note in `magnitude-scale.md` stands. The constructor is still not
located. Unchanged.

## Commands

```
build-slot cargo test -p smooth-core -p smooth-plugin
```
