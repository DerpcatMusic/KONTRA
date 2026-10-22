# Engine unit: Kontakt 8 Phaser filter (saved type 13), native located

Scope: Kontakt 8 Phaser filter only (saved filter type 13, `filter-type-ids.tsv`). Not the Legacy Phaser
effect (`k-timemachine` / legacy mod-fx notes). Status date 2026-10-22.

Outcome of this pass: the native object, its vtable, the process driver, the per-block kernel and the
coefficient law are located and read. The **host-side writer of the live Cutoff/Resonance lanes is not
located**, so the live lane law, its per-chunk cadence and a kemu vector set are **not** delivered. The
static path (`Processor::Phaser`, `phaser_filter`) is unchanged. No Wine run, no fresh Ghidra run. The
decompile export under `Research/KONTRA/source/kontakt8/` and the PE bytes (`pefile` + `capstone`
disassembly of a few functions, no execution) were the only sources.

## Located addresses

| item | address | evidence |
|---|---|---|
| Saved type 13 creator | `FUN_1407c9a70` (`BFilterPhaser`, object size `0x438`) | `0140970000.c:4637` (`case 0xd` in dispatcher `FUN_140973970`); also `0140970000.c:5700` (in `FUN_140978a00`) and `:6467` (in `FUN_140979b20`) |
| Vtable `BFilterPhaser` | `0x144f0fa38` | `docs/re/kontakt-rdata/vtables.tsv.gz`, `vtable_xrefs.tsv` (ctor `lea`) |
| Ctor constants | `+0x410..+0x430` | ctor immediates: 0.5, 1/6, 1/24, 1/120, 1/720, 1/5040, 1/40320, ln 2, 2 ln 2 (f32) |
| Slot 1 (rate constant) | `0x1408de8a0` | `+0x200 = 44011.976 / rate`; calls `FUN_140a9a300` first |
| Slot 3 (copy state) | `0x140837e40` | copies `+0x1dc` to `+0x1f8` and `+0x1dc`, and `+0x1e0`; shared with other BFilter classes |
| Slot 5 (reset) | `0x14094a720` | zeroes 16 x 0x20-byte blocks from `+0x21c`, calls `FUN_140ab71b0`, virtual slot 38 (`+0x130`), sets `+0x1e4 = +0x1e8 = -1.0f`, `+0x1a2 = 1` |
| Slot 23 (process driver) | `0x140ae9f90` (1792 B) | generic BFilterBase driver, identical in BFilter6PoleLP and BFilterPro52 |
| Slot 33 (block kernel) | `0x140909810` (398 B) | Phaser-specific; 4-stage allpass per channel |
| Slot 36 (coefficient + feedback) | `0x14095d770` (449 B) | Phaser-specific; `(this, cutoff, resonance)` |
| Slot 37 | `0x14087cfa0` (12 B) | `return value * 0.01f` (`DAT_1447148b4`); Phaser-specific, caller not located |
| Slot 32 | `0x140881930` (809 B) | vector loop over lane buffers; not read in full |
| Slots 45 / 46 | `0x1408efce0` / `0x1408e45d0` | predicates called by the driver (`+0x168`, `+0x170`); not read in full |

Slot ids are byte offset / 8 (`0x108` = 33, `0x120` = 36, `0x170` = 46). Slots not listed above equal the
BFilter6PoleLP / BFilterPro52 slots (generic BFilterBase).

## Object fields (as used by the located code)

| offset | meaning | writer / reader |
|---|---|---|
| `+0x14` | sample rate (float) | prepare (slot 1, `+0x200`) |
| `+0x124` | channel count (int) | kernel loop bound |
| `+0x20 + 8k`, `+0xa0 + 8k` | input and output pointers of channel k | kernel |
| `+0x208 + 32k` | channel k state: 4 x-state, 4 y-state floats | kernel |
| `+0x408` | allpass coefficient (float) | slot 36, kernel |
| `+0x40c` | feedback = `0.85 * clamp(resonance)` (float) | slot 36, kernel |
| `+0x410..+0x430` | Taylor and ln2 constants | ctor |
| `+0x1a1` | dirty flag (byte), cleared by the driver on entry | host writer NOT located |
| `+0x1bc` | chunk countdown (int), reset to `0x20` | driver |
| `+0x1c8`, `+0x1d0` | lane pointers (cutoff, resonance), may be null | driver; host writer NOT located |
| `+0x1dc` / `+0x1e0` | target cutoff / target resonance (float) | host writer NOT located; read by driver |
| `+0x1e4` / `+0x1e8` | last values passed to slot 36 (float; change detection) | slot 36, reset (-1) |
| `+0x1ec` | current resonance (float) | driver |
| `+0x1f0` | current cutoff (float) | driver |

## Native laws (read, not yet vector-verified)

### Coefficient and feedback (slot 36, `0x14095d770`)

- Change detection: recompute only when `|cutoff - stored(+0x1e4)| >= 1e-6` or
  `|res - stored(+0x1e8)| >= 1e-6` (`DAT_1447148f0`, f64 1e-6). Stores both, then recomputes.
- Cutoff clamped to `[0, 1]`. `v = 3c`, `square = 9c^2`, `cube = square*v`, `fourth = cube*3c`,
  `fifth = fourth*v`. Exponential term `1` when `|v| < 1e-5` (`0x3727c5ac`), else the Taylor sum
  `fifth*3c*(1/720) + fifth*(1/120) + cube*(1/6) + fourth*(1/24) + square*0.5 + v + 1`.
- Frequency `f = (e - 1) * 0.05431093007467753 * 0.33 + 0.02`; allpass coefficient `(1 - f) / (1 + f)` at `+0x408`.
- Feedback `+0x40c = clamp(res, 0, 1) * 0.85`.
- Rust `filter_coefficient` (`crates/sampler-core/src/dsp/timemod/mod.rs`) has the same operations and
  constants. One difference: Rust casts `(e - 1) * 0.0543...` to f32 before the `* 0.33 + 0.02` step, native
  keeps it in double and rounds once. The gap is one rounding step (about 1e-7 relative), well inside 1e-5.
  Static path left bit-identical in this pass.

### Kernel (slot 33, `0x140909810`)

- Per channel and stage k = 0..3, with `n` the sample index: `y_k[n] = c * (x_k[n] + y_k[n-1]) - x_k[n-1]`,
  where `x_k` is the stage input and `x_{k+1} = y_k`. The input of stage 0 is `x[n] + feedback * y_3[n-1]`.
- Output `= 0.5 * (y_3 + x) + previous output`, where `previous output` is the buffer value when the fifth
  argument is 0 (accumulate) and `0` when it is nonzero (overwrite). `0.5` is `DAT_14470bad0`.
- Rust `timemod` cascade is the same structure (`input = last * feedback + sig`, dry 0.5, wet 0.5).

### Driver cadence (slot 23, `0x140ae9f90`), partial

- On entry it reads and clears `+0x1a1`. If set, it copies the target and calls slot 36 with
  `(this, cutoff, resonance)` where `cutoff` is the lane pointer value at `+0x1c8` if non-null, else
  `+0x1dc`, and `resonance` is `+0x1e0`. (Ghidra does not show these float arguments; they are visible in
  the machine code at `140aea025..140aea06e` and `140aea2b3..140aea2ca`.)
- Work is split into chunks: `+0x1bc` is reset to `0x20`, each chunk renders `min(remaining, countdown)`
  frames through slot 33, and at the end of a chunk the driver can advance the lane ramp and call slot 36
  again.
- NOT read: the exact ramp step (a smoothing factor `DAT_144f38664` per chunk with a deadband), the stride
  of the lane buffer walk (`lVar15`), and the predicates in slots 45 and 46.

## Open questions (blocking the live-lane parity work)

1. The host-side writer of `+0x1dc`, `+0x1e0` and `+0x1a1` for the Cutoff and Resonance ENGINE_PAR lanes.
   Candidates to read next: the dispatcher `FUN_140973970` (live path), `FUN_140978a00`, `FUN_140979b20`.
   The generic setter `FUN_14087cb70` / `FUN_140838f30` is used by other BFilter classes, not this one.
2. Whether `+0x1c8` / `+0x1d0` point at one float or at a per-chunk array (the driver indexes with `lVar15`).
3. Slot 37 (`value * 0.01f`): if the live Cutoff value is 0..100 before slot 36, the Rust lane law needs a
   `0.01` factor. Find the caller of slot 37 for the Phaser (not in the generic `0x14087ce80` path, which
   calls slot 37 virtually on its own object).
4. Fifth argument of slot 33 (accumulate vs overwrite): which caller passes which value.
5. Driver ramp: the smoothing constant `DAT_144f38664` and the deadband constant, to be read from the PE
   with `pefile` before any port.
6. `+0x200` (slot 1) is not read by slot 36 or slot 33; it belongs to the generic cutoff mapping
   (`FUN_14087cb70`), which the Phaser does not use on the slot 36 path.

## Verified against the PE image (`pefile`, no execution)

| address | value | role |
|---|---|---|
| `0x1447149d0` | 3.0 (f32) | `v = 3c` |
| `0x1447149fc` | 9.0 (f32) | square factor |
| `0x144f388f0` | 0.05431093007467753 (f64) | frequency scale |
| `0x144f38968` | 0.32999999999999996 (f64) | frequency multiplier |
| `0x144f388b0` | 0.02 (f64) | frequency offset |
| `0x1447148a0` / `0x144f38d90` | +1e-5 / -1e-5 (f32) | Taylor threshold |
| `0x1447148f0` | 1e-6 (f64) | change-detection threshold |
| `0x144f389b0` | 0.85 (f64) | feedback gain |
| `0x14470bad4` | 1.0 (f32) | one |
| `0x1447148b4` | 0.01 (f32) | slot 37 scale |

These match the Rust constants in `filter_coefficient` and `Config::phaser_filter` (feedback 0.85, dry/wet 0.5).

## What is not verified

- No kemu vector set for this family (the harness was not written: the live writer is missing, so the
  object state cannot be driven the way the native host does).
- No Kontakt audio (Wine) for the live lanes. The static 8-step Wine measurement
  (`phaser_filter_matches_kontakt_measurements`, under 0.1 dB) is unchanged.
- The accumulate/overwrite fifth argument and the lane-pointer semantics above.

## Next step

Locate the live writer (open question 1), then write `tools/re/phaser_kemu_vectors.py` on the same pattern
as `formant_kemu_vectors.py`: vtable pointer at `obj+0`, constants at `+0x410`, prepare via slot 1, reset via
slot 5, the writer, the driver at `0x140ae9f90`, stereo output. Compare a pure-Python model of the driver ramp
to the vector before debugging Rust.
