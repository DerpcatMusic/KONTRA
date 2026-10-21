# Routing and utilities: Inverter, Send Levels, buses, scopes

Docs first: Kontakt 8 manual, Effect Reference (Inverter, Send Levels) and Classic View bus insert effects.
Decompile: `Research/KONTRA/source/kontakt8/` (Kontakt 8, Ghidra export, READY). Function addresses below are from that export.
Vectors: synthetic PCM in the focused tests; no native render yet, so nothing here is `parity`.

## Inverter (`0x1a`, record `0x60`)

- Parameters: `invert` (phase invert, both channels), `swap` (L/R swap). Two saved bytes.
- Reader `FUN_140d01db0` (`140d00000.c:1234`): accepts version `0x60` only; byte +0 → wrapper +8 (invert), byte +1 → wrapper +9 (swap). Anything else throws.
- Audio `FUN_140908e60` (`140900000.c:6590`): the swap runs only when the wrapper's channel count is 2. Invert is a sign-bit XOR on every sample of every channel (the `^` masks at +0x1c0). No gain or Output term is in this function: the Output knob reaches the signal elsewhere (measured in `docs/architecture-v2/KONTAKT_REFERENCE.md` §27).
- Law (the shared engine's `matrix()` in `effects/mod.rs`): `[[±1,0],[0,±1]]`, or the swapped form `[[0,±1],[±1,0]]`. Bit-exact to the XOR and the swap.
- Default: both flags off (identity). An Inverter that changes nothing still carries its Output gain; that path is covered by `color.rs`.

## Send Levels (`0x17`, records `0x50`/`0x51`)

- Parameters: one linear level per send slot; the send taps are the rack's ordered `SendTap`s.
- Engine behaviour (repo, not yet decompile-confirmed): levels are non-negative finite gains; a non-finite or negative level is diagnosed and set to 0; a tap at `position` feeds the send bus with its level, and its `bypass` drops that tap's dry contribution.
- Decompile: the `0x50`/`0x51` branches exist in `140d00000.c` (e.g. lines 44, 169, 268), but the function that reads the Send Levels record is not yet identified. The send-level constants and the list layout (`u32` count, `f32` levels, trailing `u32`) are repo evidence only.

## Instrument bus amplifier (`BInsertBus`)

- Saved record (`InsertBus` in `ni-file`): name, volume (f32), pan (f32), output (i32), unknown tail. The phase-invert and L/R-swap saved bytes are **not located** yet.
- Runtime fields (proven by the audio function): volume +0x24, saved pan +0x28, current pan +0x64, first-call flag +0x2e, channel count +0x20, invert flag +0x2c, swap flag +0x2d.
- Audio `FUN_140824910` (`140820000.c:3436`), in order:
  1. swap L/R if `+0x2d` and the channel count is 2 (`:3472`);
  2. sign-bit XOR on every channel if `+0x2c` (`:3543`);
  3. fader: per-sample multiply by volume at +0x24, with a one-pole ramp when the pan gain crosses a bound (`:3550–3600`);
  4. balance: `left = 1-clamp(pan,0,1)`, `right = clamp(pan+1,0,1)` (`docs/re/kontakt-bus-pan.md`).
- The engine now applies step 2 and the swap, in that order, before the fader and balance (`effects/mod.rs`, `instrument_buses_with_recipes`), through the same matrix as the Inverter. Tests: `instrument_bus_swap_and_invert_run_before_the_balance`, `instrument_bus_pan_only_uses_native_stereo_balance`.

## Effect slot ordering and scopes

- Scopes: `Scope::Voice` (group insert, per voice, no memory-heavy modules) and `Scope::Bus` (summed bus). Rack processors run in slot order; Send Levels flush the current chain and open a tap at that position.
- Racks: `instrument insert` feeds the instrument main; `instrument send` slots each run on their own bus fed by the Send Levels slot's level; a group `output` routed to an instrument bus runs before the bus amplifier.
- Evidence: the ordering is the repo's model (`instrument_buses_with_recipes`, `chain`); the decompile has not yet confirmed the per-slot scheduler, so ordering is `unknown` in the matrix.

## Gap table (shared engine vs Kontakt 8)

| Area | Shared engine now | Missing / unproven |
| --- | --- | --- |
| Inverter invert + swap | Matrix, reader flags, tests | Native render vector; mono path (swap skipped on non-stereo is the native law, the engine is stereo-only) |
| Send Levels | Taps, levels, bypass, bus feed | Reader function not identified in decompile; live tap binding (script/automation changes to a tap level or bypass do not reach the compiled tap, `KONTAKT_TOPOLOGY.md` T25); `dynamic Mix` excludes Send Levels (`effects/mod.rs` `SendLevels` arm) |
| Bus invert / swap | Audio stage in the bus chain (this PR) | Saved flag offsets in the `InsertBus` record (not located); the bus fader/pan live slew (`FUN_140824910` ramp, step ±20/sample-rate) |
| Bus pan | Static diagonal balance, native law | Slew ramp |
| Bus volume | Static fader, script `Mix` when dynamic | Ramp path above |
| Slot ordering | Rack order and scopes | Per-slot scheduler not confirmed by decompile |
| Bypass / wet-dry | `bypass` on taps and slots | Native wet/dry law per module not yet extracted |

## Evidence confidence

- Inverter reader and audio: decompile-confirmed (high).
- Bus audio order and flag offsets (+0x2c, +0x2d): decompile-confirmed (high); saved-record reader: not found (open).
- Send Levels reader and record layout: repo-only (medium); the decompile still needs to be searched for the `0x17` reader.
- Nothing is measured against a native render yet; the parity rows stay `partial`.
