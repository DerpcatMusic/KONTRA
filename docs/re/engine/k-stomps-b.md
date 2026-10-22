# Kontakt stomps: Fuzz, Kolor, Saturator, Skreamer, Skreamer Deluxe

Evidence labels:

- **documented**: named in the KSP constants or the NI manual.
- **measured**: read from the Kontakt 8 binary (decompile at
  `source/kontakt8`, function addresses given) or from a recorded vector.
- **unconfirmed**: assumed or not yet tied to behaviour.

## Shared engine

- Fuzz amount, bass and treble are live knobs of the shared color curve
  (`Curve::Fuzz`, `crates/sampler-core/src/dsp/color/mod.rs`). `amp::fuzz::process_live`
  rebuilds the native shaper from the current values each call and keeps its
  history in the shaper slot. A constant knob equals the static unit exactly
  (test `live_constant_knobs_equal_the_static_unit`). The old amp dispatch copy is gone.
- Live knobs use the generic lane path: `Config::lanes()` names
  `ENGINE_PAR_FUZZ_*`, `sampler-core/src/lower.rs` binds them to
  `ProcessorParameter::Fuzz{Amount,Bass,Treble}`.
- Init-time writes reach the saved record through `apply_live_field`
  (`crates/sampler-kontakt/src/effects/mod.rs`), shared with Skreamer.
- The root app's chain labels cover the new parameters (`src/ui/chain.rs`).

## Module table

| Module | Saved record (binary reader) | Shared core | Live controls | Evidence |
| --- | --- | --- | --- | --- |
| Saturator 0x1d | Shape signed [-1,1], algorithm 0/1/2 | Classic / Enhanced / Drums | `ENGINE_PAR_SHAPE`, physical slot | Shape law **measured** (`saturation-curves.tsv`, `kontakt-drive-parameters.md`) |
| Skreamer 0x21 | five floats, identity law | `Curve::Skreamer` / `LiveSkreamer` | `ENGINE_PAR_SK_*` wired | field order **measured** (`fx_dynamics_dist_tape.tsv`); moving-control PCM **unconfirmed** |
| Fuzz 0x6e | reader `FUN_140ac5910` (`0140ac0000.c`): floats at struct +0x08, +0x0c, +0x10 (amount, bass, treble), byte at +0x14 (mono) | `Curve::Fuzz` (`amp/fuzz.rs`, kemu vectors) | `ENGINE_PAR_FUZZ_{AMOUNT,BASS,TREBLE}` wired, `FUZZ_MONO` stored only | record layout **measured** (reader); names to fields **measured** (registrations `0140a80000.c:837`, id 0x1d8..0x1da, same order); live identity law **unconfirmed** |
| Skreamer Deluxe 0x78 | reader `FUN_140ac6040`: mode int at +0x1c (0..2), five floats at +0x08..+0x18, byte +0x20 | none (unmapped) | `ENGINE_PAR_SKDLX_*` | knob names **measured** (registrations `0140a80000.c:956`, ids 0xdd..0xe1: drive, tone, bass, mid, treble). The earlier mapping onto the Tubescreamer core (pot A/B and filter) is **wrong** for these names, so it was removed; the module is reported as not modeled |
| Kolor 0x79 | reader `FUN_140ac5a20` (`0140ac0000.c:3645`): mode enum (0..9, else throws) at +0x38; 11 floats at +0x08..+0x30; byte at +0x34 | none (no DSP) | `ENGINE_PAR_KOLOR_*` names, not live | mode table **measured** (`0140660000.c:1552`, `FUN_141ef65b0`); 11 registrations **measured** (`0140a80000.c:866`, ids 0xe4..0xee); float-to-knob tie **unconfirmed** (same registration order assumed); topology and DSP **not recovered** |

### Kolor mode table (measured)

| Selector | Mode | KSP constant |
| --- | --- | --- |
| 0 | Solid Console | NI_KOLOR_SOLID_CONSOLE |
| 1 | Analog Aura | NI_KOLOR_ANALOG_AURA |
| 2 | Harmonic Ctrl | NI_KOLOR_HARMONIC_CTRL |
| 3 | Full On | NI_KOLOR_FULL_ON |
| 4 | Super Amp | NI_KOLOR_SUPER_AMP |
| 5 | Smooth Fuzz | NI_KOLOR_SMOOTH_FUZZ |
| 6 | Tape | NI_KOLOR_TAPE |
| 7 | Crunch | NI_KOLOR_CRUNCH |
| 8 | Saturaver | NI_KOLOR_SATURAVER |
| 9 | Diode | NI_KOLOR_DIODE |

`sampler_ir::fx::kolor` holds these tables with their evidence levels.

## Kolor: what is still open

1. The stored-field tie: which of the 11 reader floats each registered name
   drives (the apply for module 0x79 has not been located; the registration order
   is the working assumption).
2. The DSP topology. Lambdas for the 13 smoothed knobs are bound in the Kolor
   constructor (`0141970000.c` around line 5950); the processing function is not
   yet located. Update: the per-block process is core vtable slot 19 = `14197a2c0`,
   reached from the wrapper driver `140900eb0`; see `k-effects-batch-a.md`. Kolor stays not modeled.
3. Boost and Mode are KSP constants without a registration in the 0xe4..0xee block.

## Measurement (Wine oracle)

`tools/kontakt-oracle/probe_fx.py` needs a factory `- INIT` preset per module.
This machine's Presets tree has no Fuzz, Kolor or Skreamer Deluxe folder, and the
decoder path it names (`/mnt/Windows11/...`) does not exist. So the binary was used
instead of the oracle for these modules.

## Remaining

1. Kolor DSP and the float-to-knob tie (above).
2. Skreamer Deluxe: find its apply/DSP, then a mapping or a Kolor-like skeleton.
3. Fuzz live identity law (binding sweep needs an INIT preset).
4. Saturator: internal-modulator 32-frame clock; Enhanced/Drums live Shape.
5. Migration of Saturator and Skreamer onto a shared nonlinear stage: not started.

## Tests

- Added: `live_constant_knobs_equal_the_static_unit` (Fuzz core),
  `addressed_fuzz_exposes_its_three_live_lanes_and_not_the_mono_flag` (IR),
  `tables_are_complete_and_match_the_reader` (Kolor).
- Removed: `tubescreamer_record_fields_land_on_their_core_indices` (its 0x78 mapping
  was wrong, see the table).
- Kept: the kemu Fuzz vectors, the Saturator table/drums tests, the Skreamer live tests.
