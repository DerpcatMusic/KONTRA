# Solid G-EQ: script access to the knobs

Date: 2026-10-20. Target: `Kontakt8.exe` SHA256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

The gap this closes is the `Solid G-EQ: live control` row of
`docs/diagnostics/20261019-installed-gap-inventory.md` (59 entries, 3
instruments: Una Corda Pure, Felt, Cotton). The translator now binds the
module's engine parameters to the band stages, so a script's
`set_engine_par` writes move the EQ instead of being dropped. The coefficient
smoothing row is untouched and stays open.

## 1. The saved record

Module `0x44` (`BParFXSSLGEQ`) stores the knobs as plain normalized `f32` in
serialized order, with one-byte flags between them (reader
`0x140d029f0`; the record is `FFBFx8BFFBB` for v0x10 and four more fields for
v0x11):

| Field | v0x10 offset | v0x11 offset | Type |
|---|---:|---:|---|
| LF gain | 0 | 0 | f32 |
| LF frequency | 4 | 4 | f32 |
| LF bell | 8 | 8 | u8 |
| LMF gain, frequency, Q | 9, 13, 17 | 9, 13, 17 | f32 |
| HMF gain, frequency, Q | 21, 25, 29 | 21, 25, 29 | f32 |
| HF gain, frequency | 33, 37 | 33, 37 | f32 |
| HF bell | 41 | 41 | u8 |
| HP frequency | — | 42 | f32 |
| LP frequency | — | 46 | f32 |
| LP on | — | 50 | u8 |
| HP on | — | 51 | u8 |

Every knob is the native normalized `0..=1` control, not a physical unit:
`solid::coefficients` owns the spline tables that turn the knobs into biquads
(`crates/sampler-core/src/dsp/eq_family/solid.rs`). The engine laws are the
measured ones in `docs/re/kontakt-bindings/filters_eq.tsv`, all sixteen
`$ENGINE_PAR_SEQ_*` rows:

* twelve continuous knobs, law `identity` (`0 -> 0`, `1000000 -> 1`), so the
  written value *is* the stored knob;
* four switches, law `bool: stored=1 when value>=1` (LF bell, HF bell, LP,
  HP).

## 2. Why one config per band

The IR already had the mechanism, but it names knobs per *band*: a
`Processor::Eq` with `count == 1` and an `address` exposes
`Config::lanes()` (`ENGINE_PAR_FREQ1..3` for the legacy law, using
`Config::band`). Two constraints fix the shape:

* `lower_list_with_map` refuses `Processor::Eq(c) if c.count > 1` whenever a
  `processor_controls` binding reaches it: a live lane addresses one band, and
  a multi-band config has no band index to name;
* `Config::lanes()` describes the knobs of one band (`FREQn`, `BWn`, `GAINn`
  for the legacy law), because the legacy 3-band EQ is already translated as
  one config per band.

So `solid_geq` emits one one-band config per saved band, each carrying its
saved band index (`band`: LF 0, LMF 1, HMF 2, HF 3, HP 4, LP 5), and
`Config::lanes()` returns that band's `SEQ_*` lanes. The HP/LP bands carry a
frequency lane only.

The split cannot change the sound: `eq_family::lower` walks the bands in
signal order and emits `solid::sections(section, kind)` stages for each, so
five one-band configs produce exactly the stage sequence of one five-band
config. The test
`effects::tests::solid_geq_bands_are_live_and_the_split_is_audio_identical`
renders an addressed chain and an unaddressed one and asserts the outputs are
equal, then writes `ENGINE_PAR_SEQ_LF_GAIN` and asserts the output changes.

## 3. How the write reaches the DSP

* `Processor::lanes()` → `native_lanes` in `sampler-core/src/lower.rs` turns
  each lane into an `EngineParameterAddress` (parameter id from the
  `$ENGINE_PAR_SEQ_*` name, plus the slot's `group`/`slot`/`generic`), a
  `NativeControl` and an `EngineParameterBinding` with the identity law. Every
  chain's bindings are collected in `lower` (`native.into_values()`), so a
  script's `set_engine_par` resolves to the lane's control id.
* `lower_list_with_map` then binds each lane's role to the stages:
  `GainDecibels` → `EqBand::gain_db`, `Frequency` → `frequency`, `Quality` →
  `q`. It binds the lane to **every** stage of the band
  (`bind_role(stage, role, parameter) | found`), which is what a multi-section
  Solid band needs: the LF and HF shelves are three sections each, and each
  section's coefficient table reads the same three knobs.
* `live_stage` used to seed a *single* stage for a one-band `Eq` when a
  `processor_controls` binding reached it. That is right for the one-biquad
  legacy law and wrong for a multi-section band, so `Law::Solid` is now
  excluded there exactly like `Law::UviDigital`: the band lowers through
  `processors()` and every section is bound.
* The address itself is set by the translator (`Processor::at`) for every
  script-addressed module. That replaces the old blanket note: the module used
  to be reported as `live control` / "scripts cannot address Solid G-EQ knobs"
  whenever *any* script in the instrument wrote an effect slot, which is how
  the 59 entries were produced even for slots no script touches.

## 4. Recorded writes at load

`apply_writes` (the init-time pass over the script's recorded
`set_engine_par` calls) now calls `eq::solid_store` for module `0x44`: the
twelve knobs are written as `value / 1000000` (identity), the four switches as
`value >= 1`. Without it, a patch whose script sets a bell or a pass filter at
load would keep the saved topology while the script's value was dropped. The
write goes into the same bytes the reader parses, so the translated band, the
lane default and the saved record agree.

## 5. What is still not modelled

* **The four switches have no lane.** A bell or a pass enable changes the
  band's `Kind` (shelf ↔ peak) or adds a band, and `Kind` is a compile-time
  property of the stage, not a `ProcessorParameter`. A *live* switch write
  therefore has no engine binding; the runtime reports it through the
  engine-parameter outcome (`EngineParameterOutcome { result: Err }`) rather
  than dropping it silently.
* **The 20 ms coefficient smoothing** stays open (the other 59-entry row). The
  changed knobs recompute the band's biquad at the next block through the
  existing aux-key check in `eq_family/generic.rs`; the native ramps the
  coefficients instead.
* A knob written by a script before the patch is loaded is applied by
  `apply_writes` (§4) for group inserts and for every program rack, because
  both go through `apply_writes`.

## 6. Verification

```sh
cargo test -p sampler-ir --lib fx::eq
cargo test -p sampler-kontakt --lib solid_geq
cargo test -p sampler-core --lib eq_family
```

The corpus re-survey was run on 2026-10-20:

```sh
ALL=1 FILES=1 VALUES=all cargo run -p sampler-kontakt --release --example fx_survey -- /mnt/MAIN_STORAGE/Libraries/Kontakt
```

It reports `Solid G-EQ: live control` zero times, where the 2026-10-19 run
reported 59 entries over 3 instruments, and `Solid G-EQ: Solid G-EQ smoothing`
still 59. `docs/diagnostics/20261019-installed-gap-inventory.md` records the
same-tree totals for both runs.
