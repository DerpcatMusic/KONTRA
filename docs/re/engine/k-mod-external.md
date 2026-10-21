# Kontakt modulation: envelope follower, glide, external sources, routing

Status: **partial. Evidence gathered, implementation not started.** Scope: Kontakt 8 only.
Unit: `k-mod-external`. Matrix rows: `docs/re/engine/PARITY_MATRIX.md`, "Modulation sources".

## Evidence used

- Repo, Kontakt reader: `crates/sampler-kontakt/src/modulation_objects.rs:65-66` maps
  Glide to internal chunk `0x0B` and Envelope Follower to `0x0A`;
  `crates/sampler-kontakt/src/source_parameters.rs:196` keeps both as opaque `undecoded_object`;
  `crates/sampler-kontakt/src/library.rs:1889-1893` says "saved floats to builder controls" for the follower.
- Repo, IR: `crates/sampler-ir/src/lib.rs:1690-1720` `ModulationSource` has no follower or glide
  variant. `crates/sampler-ir/src/kontakt.rs:266-280` `ExternalSource` has 13 variants.
- Repo, routing: `crates/sampler-ir/src/lib.rs:1921-1943` documents invert, then shape, then smoothing.
  `crates/sampler-ir/src/kontakt.rs:287-288`: negative native lag has **no proved smoothing law**.
  `crates/sampler-kontakt/src/library.rs:6063` test: saved depth sign is independent of invert and
  source polarity.
- Repo, oracle notes: `docs/re/NATIVE_ORACLE.md:87` lists Glide 0x0B and follower 0x0A as never decoded.
- Decompile (`source/kontakt8/`, not READY, read as data only): `0140500000.c`, function
  `FUN_140507b10`, modulator type menu. The menu builds the entries "Flexible Envelope" (0x10201),
  "DBD" (0x10202), "Step Modulator" (0x10300), "Envelope Follower" (0x10400) and "Glide" (the call
  passes no id). This confirms the modulator names and the follower's type id. It does not give DSP laws.
- Public docs: no Kontakt 8 manual page for follower or glide was found by search. Not used.

## Per-module spec

### Envelope follower (chunk 0x0A, type 0x10400)

- Parameters: not decoded. The reader keeps the saved floats only, without names or units.
- Laws, attack/release, detector, scaling: **unknown**. Needs decompile of the chunk-0x0A
  processor (not located yet) or a Wine-oracle probe.
- Gap: no `ModulationSource` variant, no engine evaluator, no native vector.

### Glide (chunk 0x0B, "Glide" in the type menu)

- Parameters: the reader's `BParGlide` schemas (`docs/re/kontakt-catalog/reader_schemas.tsv:16-17`) exist
  but the modulator object itself is opaque.
- Behaviour: "pitch and time to ramp start, step and gain" (from the reader's note). Not verified.
- Gap: same as the follower. A glide here is **not** the sampler-uvi script glide
  (`crates/sampler-uvi/src/scripted.rs`); that one is a script command and not the Kontakt modulator.

### External sources (`ExternalSource`, 13 variants)

- Mapping from native sources exists: `crates/sampler-kontakt/src/modulation_objects.rs:84-96`.
- Engine evaluation per variant is not verified here. No vector, so status stays **partial**.
- Known gaps: the random-bipolar law (noted in `NATIVE_ORACLE.md:90`, from Conflux), and the
  release-velocity and release-counter laws, which are not confirmed.

### Routing (target identity, depth/polarity, shaping, smoothing, bypass)

- Target and depth mapping: present and tested (`library.rs:6063`).
- Shape and scale: the IR defines the order (invert, shape, smoothing). Engine law not confirmed.
- Smoothing: positive lag goes to the engine (`whole_voice.rs:293-309`). Negative lag is unproven.
- Bypass: `SourceSwitch` in the IR exists. Live behaviour not vectored.

## Gap table

| Item | Shared engine | Kontakt reader | Evidence | Missing |
| --- | --- | --- | --- | --- |
| Envelope follower | none | opaque | decomp type id only | detector, attack/release law, params, vector |
| Glide | none | opaque | decomp menu name only | ramp law, params, vector |
| External sources | IR enum (13) | mapped | repo only | per-source engine law and vectors |
| Routing depth/polarity | IR + engine | mapped, tested | repo test | shape, negative-lag smoothing, bypass vectors |

## Next steps

1. Locate the chunk-0x0A and 0x0B processors in the decompile (search for the chunk ids in the
   `0x0A`/`0x0B` dispatch, and the `BParGlide` reader at `0x140d034d0`). Record the laws here.
2. Add a `Follower` and a `Glide` source to the shared IR and engine, general form (detector
   and ramp cores), with null or recorded tests once the laws are confirmed.
3. Measure external sources and negative lag on the Wine oracle to get vectors.

## Follower input and timing (decompile, FUN_140916350 -> FUN_140ae3de0)

- The follower is processed per block inside the voice render (`FUN_140916350`), for each follower
  modulator whose object flags bit 5 is set. It reads the per-group buffer `param_1[idx*16+6]`
  and runs before `FUN_14088b480`.
- Working hypothesis: the follower samples the group's audio at current-block timing.
- Engine finding: `voice_mod::advance` runs before a voice renders its span, and the voice's own
  audio depends on its modulation. Current-block audio is therefore not available to the modulation
  step without changing render order. This needs a decision (one-block delay, or reordering render).
- NaN: the native clamp compares `x <= 0` and `1 <= x`, so NaN passes through. Implemented and tested.

## Glide setup (FUN_140cebba0, binary constants checked)

- start = -1 (+1 for a downward interval), step = 1 / (time_s * rate), gain = |interval| / 12.
- time: stored `BParGlide` float; multiplied by |interval| only for stored mode 1; time_ms * 0.001.
- Implemented as `GlideRamp::kontakt_glide`. The interval is a per-note runtime input (not yet wired).
