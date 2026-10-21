# Bass Invader (Kontakt 8, effect 0x54, record version 0x52): RE status (k-amps-a)

Status: **not modelled.** The core is partly recovered (smoothing, a 2x internal rate, three EQ
sections, output gains, the per-sample section form). Not recovered: the seven nonlinear network
subsystems, the internal resampler, the parameter-to-slot map for most fields, the stereo routing
(measured L != R, decompiled core writes one value to both channels), and the MONO control. Nothing
in `crates/` changes; `amp()` / `unproved()` in `crates/sampler-kontakt/src/effects/color.rs` still
report 0x54 as unproved. No constant here is fitted to the renders.

Sources: decompiled Kontakt 8 C in `source/kontakt8/` (buckets `0141920000.c`, `0141930000.c`,
`0140990000.c`, `0141ac0000.c`, `0141ee0000.c`), `re-binaries/Kontakt8.exe` (RTTI, vtables, data),
`docs/re/kontakt-bindings/fx_other.tsv`, and the oracle renders
`~/.cache/kontra-amps-a/bassinvader.npz` (29 steps; `tools/kontakt-oracle/ampspecs_a.py`).

## Identity and dispatch

- Class `ni::effektrig::dsp::bassinvader::BassInvader`. Primary vtable RVA `0x7696980`; a second COL
  gives a vtable at RVA `0x7696a38`.
- Wrapper: `BParFXBassInvader` (ser vtable RVA `0x4e75240`), `BEffectBassInvader`,
  `ModulatableDSPCoreWrapper<BEffectBassInvaderTraits,1>`.
- Constructor VA `0x14192e260`: copies constant tables from data at `0x14a648xxx`..`0x14a649xxx`.
- Vtable slot 3 (sample-rate setup, generic) VA `0x14099ae80`. Slot 7 (process wrapper) VA
  `0x14099f000`, which calls slot 19 once per block. Slot 19 (per-sample core) VA `0x141932430`,
  2461 bytes. Slot 1 VA `0x14099a270` calls slot 18 (VA `0x141933c20`) first.

## Stored fields (fx_other.tsv, version 0x52)

| index | parameter | stored byte | kind |
| --- | --- | --- | --- |
| 0 | VOLUME | 8 | float |
| 1 | TREBLE | 12 | float |
| 2 | HI_MID | 16 | float |
| 3 | LO_MID | 20 | float |
| 4 | BASS | 24 | float |
| 5 | BOOST | 28 | float |
| 6 | MASTER | 32 | float |
| 7 | LO_CUT | 36 | bool |
| 8 | MID_CONTOUR | 37 | bool |
| 9 | HI_BOOST | 38 | bool |

MONO (Kontakt 8.1) is in the manual only; it is not in the bindings or the reader schema.

## Harness facts (oracle, 200 Hz ramp 0 to 1 over 0.6 s)

- **Onset shift per step.** Steps with identical settings produce identical steady-state output after
  a per-step shift of the onset, not a change of the signal. Measured shifts run from -26 to +40
  samples, and some (BASS 0.5, the same as default) fall outside a +/-40 search. Aligning on the
  default step gives max |diff| 2e-7 for HI_BOOST=0 (lag -4), 7e-7 for LO_CUT=0 (lag 37), 4e-7 for
  MID_CONTOUR=0 (lag -16), 3e-7 for LO_MID=0.5 (lag 16), 5e-7 for VOLUME=0.5 (lag -26). Those are
  float-precision matches. Compare steady windows (0.1 s to 0.5 s), search the lag per step, and do
  not trust the first 0.1 s.
- **Bypass.** The bypass step is the stimulus times 0.76766, delayed 51 samples (fit residual 1e-8).
  This is a harness path constant.
- **Silence.** VOLUME 0 and MASTER 0 both silence the output (peak 0.00).
- **Peaks** (default about 1.00): BOOST 0 gives 0.31, LO_MID 0 gives 0.62, HI_MID 0 gives 0.92,
  LO_CUT on gives 0.69, MID_CONTOUR on gives 0.96. BASS, TREBLE and HI_BOOST sweeps stay near 1.00.
- **Left and right differ.** Steady window, default step: R is about 1.195 x L with relative residual
  0.20 (not a constant gain). Bypass has L == R. The decompiled core writes one value to both channels
  (see the stereo item below), so this is a conflict, not yet explained.

## Core structure (VA 0x141932430 and its setup), as far as read

1. **Internal rate.** The SR setup (`0x14099ae80`) computes R = 2^round(log2(2 * 44100 / sr)). At
   48 kHz, R = 2, so the core runs at 96 kHz internally and the wrapper resamples at the boundary.
   The resampler kernel is not identified (wrapper buffers at 0x600 and 0x618; copy helper
   `0x141ee2160`, mode 2 is a plain copy).
2. **Parameter smoothing.** Ten values at core offset `0x4c + 4*i` are advanced per sample when the gate
   at core offset `0x3d5` is set. The ramp is linear over about 75 ms (N = round(0.075 * R * sr / n)
   blocks, stored at `0x3dc`). The smoother objects at `0xb0 + 0x40*i` are not identified.
3. **Input.** L and R are summed and scaled by a constant (VA `0x147696ca4` = 3.6318988); exact zeros
   are flushed.
4. **EQ sections.** Trapezoidal state-variable sections (per sample: v3 = x - ic2; v1 = a1*ic1 + a2*v3;
   v2 = ic2 + a2*ic1 + a3*v3; ic1 = 2*v1 - ic1; ic2 = 2*v2 - ic2; y = m0*x + m1*v1 + m2*v2). Coefficients
   come from a designer (VA `0x141ac9260`) with A = 10^(G/40), x = pi * f / rate_internal, and Q = 0.7.
   Three sections are designed in the SR-independent chain: a 300 Hz low shelf, a 500 Hz peak, and a
   1 kHz high shelf (gains from converted values at core offsets 0x304, 0x308, 0x30c, written outside the
   Bass Invader files). A fourth section, a 220 Hz peak, sits in the tail and carries the BOOST control,
   as reported (0 dB at BOOST >= 0.21, down to -4 dB at 0). Field-to-gain mapping for the three EQ
   gains is not verified against the renders.
5. **Nonlinear networks (not decoded).** Seven subsystems at core offsets 0x680, 0xb50, 0x1180,
   0x14a0, 0x16b0, 0x1b90 and 0x17e8 are 4-lane networks with constant matrices (from the constructor
   data) and 1/sqrt(1 + z^2) soft clips with a Newton refinement. A double-precision 5-state section
   (VA `0x1419302c0`) sits in the tail. Its return value is typed void in the decompile, yet the core
   reads a result, so its output wiring is open. A tanh-style rational function (VA `0x14192d0e0`,
   clamp near +/-7.905) is also in the path; its branch and use are unverified.
6. **Output gain.** Two gains of the form min(10 x, 1) applied to two stored parameters, times the
   network output and 0.6557412 (VA `0x147696ca0`). One is the VOLUME factor (at core offset 0x4c). The
   other, at 0x70, is the factor that silences at 0, which matches MASTER, not HI_BOOST. A bool cannot
   produce the measured HI_BOOST = 0 and 1 behaviour through this factor, so the field index for this
   gain is inconsistent with the binding table and needs checking.
7. **Stereo.** The core sums L and R and writes the same value to both outputs. This conflicts with the
   measured L != R (above). Open: the resampler channel path and whether a second core object exists.

## Open items (blocking an implementation)

1. Stereo routing (measured L != R versus a mono-output core).
2. Internal resampler at R = 2: kernel, latency, and the 51-sample harness offset relation.
3. Seven network subsystems (items 5 above), including the undecoded callees (VA `0x1419311d0`,
   `0x1419305e0`, `0x1419313f0`, `0x141930870`, `0x141931640`, `0x141930b90`, `0x14192c7a0`,
   `0x14192c590`, `0x14192f170`, `0x14192ceb0`, `0x14192ee20`, `0x14192cf80`, `0x141932e70`).
   The input width of the 0x680 network and the 3-to-4 affine map (VA `0x14192ef70`) are ambiguous.
4. Parameter laws: the field-to-slot map for slots 1 to 4 and 6 to 9, the laws for BASS (k4), MASTER
   and LO_CUT (k6, k7, via VA `0x1419343e0`, 2529 bytes, not decoded), and MID_CONTOUR (k8).
5. Coefficient writers for the three converted EQ gains (core offsets 0x300..0x30c).
6. The constructor data tables (`0x14a648xxx`, `0x14a649xxx`) are not extracted.
7. MONO (8.1): not in the bindings, no render.

## Gap table

| Module | Engine coverage | Bindings | Laws | Status |
| --- | --- | --- | --- | --- |
| Bass Invader (0x54 v0x52) | none | yes (10) | partial: smoothing, 2x rate, EQ form, output gain | not modelled |

## Confidence

- Field order and byte offsets: high (bindings agree with the reader schema).
- Harness: onset shift per step and identical steady output: high (matches to 2e-7). Bypass gain and
  delay: high.
- Internal 2x rate (R = 2 at 48 kHz): medium-high (one function read, not executed).
- EQ section form and designer: medium (read, not verified).
- Networks, resampler, stereo, parameter laws: none yet.
