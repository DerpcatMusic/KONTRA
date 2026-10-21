# Kontakt modulation sources: LFO, Multi Digital, Step modulator

Status: **partial**. Scope: Kontakt 8 only.
Evidence: decompile (`source/kontakt8/0140b00000.c`, `0140a70000.c`) for kernel structure and
the shared post stage; repo recovered vectors (`crates/sampler-core/src/blocks/lfo.rs`,
`blocks/step_table.rs`, `digital_fade.rs`); translator (`crates/sampler-kontakt/src/library.rs`,
`fn lfo`, ~L3050). The decompile has no symbols (`FUN_*` only); functions are located by the
addresses recorded in the repo's earlier notes and cross-checked by structure.

## Shared shape

Every Kontakt LFO is one `ir::Lfo` (`crates/sampler-ir/src/lib.rs`, `LfoShape`) evaluated by
`sampler_core::blocks::lfo`:

- rate: Hz when the sync note value is `-1`, otherwise `note * count` beats (`library.rs`).
- phase: `initial_values[3]`, cycle position in 0..1.
- waveform ids: sine 0, rectangle 1, triangle 2, saw 3, random 4, Multi 5, Digital Multi 6.
- `initial_values[0]` (fade ms): on the Multi and Digital Multi entries the translator treats it
  as a saved fade-in (`NativeFaded` / `DigitalSine`), not as a delay (`library.rs`, `fn lfo`).

## Post stage: the saved fade (0x140b06c90)

Shared by all five Multi and sine kernels (called at the tail of each kernel).

- State `f0` (`+0xf0`) is multiplied by ratio `f4` (`+0xf4`) per sample, then clamped:
  - legacy (`+0xe5` clear): to `[0, 1]`;
  - v73 mode (`+0xe5` set): to `[0.3, 1.3]`.
- Output is scaled by `(f0 - 0.3)`. The legacy ceiling therefore reaches 0.7, not 1. This is
  what `KontaktLfoFade` in `digital_fade.rs` reproduces, and what its v73 mode reflects.
- The sample count is clamped to the remaining count at `+0xfc`.
- Confirmed: the stage is a multiplicative ramp, not a hold. This supports the fade reading of
  `initial_values[0]`.
- **Not confirmed:** whether the ramp advances once per sample or once per 32 samples. The loop
  updates `f0` per sample, but the callers are virtual and do not show the block length, so the
  "audio / 32" control rate in `digital_fade.rs` remains unproven.
- **Not confirmed:** how `+0xf0`, `+0xf4` and `+0xfc` are initialised from the stored field, and
  the fade-off path (`f4 = 1`, no clamp). That is in the BLfo setup, not yet traced.

## Multi and Digital Multi mixers (0x140b07290 analog, 0x140b078d0 digital)

Weights are stored in order sine (`+0x128`), then saw, triangle, pulse, noise
(`+0x12c..+0x138`). The Rust order is `[sine, saw, triangle, pulse, noise]`.

| Term | Analog (0x140b07290) | Digital (0x140b078d0) | Rust (`multi_native`) |
|---|---|---|---|
| sine | `-w0 * sine` | `-w0 * sine` | matches |
| saw | `-w1 * saw2` (doubled saw, table) | `+w1 * (2d-1)`, `d` wrapped at p=0.5 | matches both |
| triangle | `+w2 * triangle table` | `+w2 * 4(p-0.25)` / `4(0.75-p)` | matches |
| pulse | `-w3 * rectangle(width)` (table) | `-w3 * (1 inside width, -1 outside)` | matches |
| noise | `+w4 * held` | `+w4 * held` | matches |
| normalise | `/ max(1, sum abs w)` when the flag is set | same | matches |

- Confirmed structure: the digital pulse outside the width is the bipolar clamp floor (`-1`),
  the same constant the width stream clamps to. The triangle breakpoints are `0.5`, `0.25`,
  `0.75`.
- Not confirmed from the binary: the numeric data constants (`DAT_*`) behind the saw wrap and the
  triangle gain. They are in the image's data section, not in the decompile text.
- Inline rectangle interpolation order in the analog kernel is not recovered (last-bit only,
  documented in `rectangle_native`).
- Held noise: the recurrence (`LCG`) and half-cycle grid are native. The reset seed is not:
  native uses the tick count (`0x1405f4510`), the repo folds the caller seed instead
  (`HELD_RESET_SEED_IS_NATIVE = false`). Repeated resets therefore differ from Kontakt.

### Admission (translator, `fn lfo`)

| Entry | Admitted | Refused (reported as `UnknownLaw`) |
|---|---|---|
| Zero Multi (5) | all weights zero | none |
| Digital sine-only (6) | fade 0, or retrigger + unsynced fade with the verified records | synced fade, alternate mode, multi-component |
| Legacy weighted sine (5) | sine-only, fade 0 | none |
| Multi (5) with components | fade 0, or unsynced fade (records[1] = -1) | Digital (6) with fade > 0 and components |
| Native waves (0..4) | any width in 0..1 (rect), held needs phase < 0.5 | other waveform ids |

Pulse width is refused outside the builder's `0.05..0.95` window for Multi, and the held noise
needs a start phase below one half.

## Step modulator (BArp32Step, 0x140a768e0)

- Render confirmed: table at `+0xe0` (32 f32), length `+0xd8` (1..32), one-shot `+0xdc`, phase
  `+0xb0` (binary64), step `+0xb8`. Free-run wraps by the length, one-shot sets the step to 0 and
  holds the last entry. `blocks/step_table.rs` matches this.
- End-of-call correction (0x140a768d0 caller path) is implemented as `StepTable::correct`.
- **Not modelled:** the per-lookup branch on `+0xdd` (the decompile routes through a different
  path when it is non-zero). The repo already lists this as unproved (`step_table.rs` header).
- **Not recovered:** how the stored Kontakt "Arp" fields become table, length, rate and mode. The
  writers of `+0xd8`/`+0xdc` are shared across many object classes; the loader of this object is
  not yet identified. The translator reports it as `UnknownLaw` (`library.rs` ~L1904).

## Gap table

| Item | Shared engine has | Missing / unproven | Evidence |
|---|---|---|---|
| Native waves sine/rect/tri/saw/random | yes (recovered tables and landmarks) | rect inline interp last bits; rate lane is an approximation | decomp + vector tests |
| Fade post stage | yes (`KontaktLfoFade`, `NativeFaded`) | per-sample vs 1/32 cadence; init from stored field; fade-off path | decomp (structure) |
| Synced fade (sync note on Multi/Digital) | no | verified only for the sine-only Digital case | translator refusal |
| Digital Multi with fade > 0 and components | no | law + admission | translator refusal |
| Multi analog / digital mixers | yes | data constants (`DAT_*`) unverified in binary | decomp (structure) + vector tests |
| Held noise reset seed | LCG grid | native tick-count reset seed | decomp + repo note |
| Step modulator render | yes | `+0xdd` per-lookup branch | decomp |
| Step modulator setup (stored fields to table/len/rate/mode) | no | loader of the object | translator refusal |

## Next steps (not done here)

1. Identify the BLfo setup and the BArp32Step loader (search by vtable/constructor, not by offset,
   because the offsets are shared) to close the setup and fade-init rows.
2. Wine-oracle render of one Multi and one Digital Multi at a known start phase and fade, to fix
   the per-sample vs 1/32 cadence and the delay-vs-fade field meaning.
3. Use the decompiled `+0xdd` branch to implement the per-lookup mutation, or record it as a
   measured non-goal.
