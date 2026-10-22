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
- Working hypothesis (decompile): the follower samples the group's audio at current-block timing.
- Implemented timing (deviation, one-span delay): the voice keeps the mono of its previous rendered span
  (first 64 frames, `Voice::follow_audio`) and feeds it to `Inputs::audio` at the next modulation step.
  Current-block timing is not realizable without reordering render, because a voice's audio depends on
  its modulation. Native timing stays open until a Wine-oracle probe.
- Engine finding: `voice_mod::advance` runs before a voice renders its span, and the voice's own
  audio depends on its modulation. Current-block audio is therefore not available to the modulation
  step without changing render order. This needs a decision (one-block delay, or reordering render).
- NaN: the native clamp compares `x <= 0` and `1 <= x`, so NaN passes through. Implemented and tested.

## Glide setup (FUN_140cebba0, binary constants checked)

- start = -1 (+1 for a downward interval), step = 1 / (time_s * rate), gain = |interval| / 12.
- time: stored `BParGlide` float; multiplied by |interval| only for stored mode 1; time_ms * 0.001.
- Implemented as `GlideRamp::kontakt_glide` and executable as `ModSource::Glide` (see below).

## Follower as a source (executable)

- `ir::ModulationSource::Follower` -> `voice_mod::ModSource::Follower([attack, decay, gain, adaption])`.
- Per voice: `AdaptiveFollower` built at voice start from the stored fields at the voice's sample rate.
  Each evaluation feeds `Inputs::audio` (empty = hold) and reads the second state as a 0..=1 level.
- Test: `voice_mod::follower_source_tests` (rises on loud audio, falls over silence, holds with no audio).
- The Kontakt mapping now produces this source instead of an unsupported report.

## Glide as a source (executable)

- `ir::ModulationSource::Glide` (stored mode, time_ms from `BParGlide` `values[0]`) ->
  `voice_mod::ModSource::Glide`. Per voice: `GlideRamp` built at voice start from
  `kontakt_glide(mode, time_ms, interval, rate)`; each evaluation renders the ramp for the span and
  reads its last sample. Bipolar output (starts at -interval/12 octaves for an upward glide).
- Interval: semitones from the previous note this plan started (`VoiceModState::glide_interval`),
  set on `Inputs::interval` at voice start (`prepare/selection.rs`). First note: 0.
- Test: `voice_mod::glide_source_tests` (origin from the previous note, ramp to zero over 700 frames
  for mode 1, mirror for downward, silent for zero interval).

## Deviations and open TODOs (verify with the Wine oracle)

- TODO(oracle): timing of the follower's audio input. Implemented as one-span delay (the voice's
  previous rendered span, mono, first 64 frames). Native timing (current block or otherwise) is unproved.
- TODO(oracle): glide units. The ramp is octave-normalized (`|interval|/12`). A pitch route's depth
  scale against this output is not verified, so pitch depth through a glide is not yet native-parity.
- TODO(oracle): glide origin is per plan (instrument), not per Kontakt group, and does not check
  legato. Group and legato rules are unproved.

## Decompile evidence, final pass (Kontakt 8, read as data)

- Follower input (`FUN_140916350`, `0140910000.c`): the follower call
  `FUN_140ae3de0(modulator, param_1[idx*16+6], n)` at line 4373 runs before the source render that
  fills the same buffer (`FUN_1408e4110`, line ~4783). The buffer is a 4 KiB pool slot (`0x1000`
  stride, 1024 floats) in the 16-pointer per-slot table built by `FUN_1407f31f0` (`01407f0000.c:2571`).
  Ordering therefore confirms the one-span (previous-block) timing used by the engine. Still open:
  whether the buffer is mono pre-pan source output (we feed the voice's stereo mean after gain), and
  whether the native consumer reads the decimated ring cells or the last state.
- Glide start (`0140970000.c:7741-7753`): a glide starts when bit 4 of the modulator flags (`+0xa4`)
  is set; `FUN_140cebba0` gets `rec[0x458] - rec[0x454]` from a per-block record
  (`+0x2d7f8` table, index = position / 128). This is the interval. Our origin is per plan, not this
  record; record identity is unproven (`0x454`/`0x458` are written by several object types).
- Glide consumer: `FUN_140ceb090` (ramp render) has no direct caller in the export; it is reached
  through a virtual call. Its output units, and how a pitch route scales them, are unproven.
- External sources and routing lag/shape: no getter or lag law located in this pass. Status stays partial.

## Routing smoothing (final pass)

- Native compiler FUN_140873040 (`0140870000.c`, body at ~1413-1435, text emit ~1839-1870) reads the
  signed lag `+0xe` and computes, with constants from the binary: rate * |lag| / 32000 (32-frame cells).
- Positive lag: `1 - exp(-2 ln10 / cells)` per cell. The engine's per-chunk step at 64 frames equals
  two cells (`voice_mod::laws::native_lag_coefficient_per_cell`, test `route_lag_matches_the_native_compiler_laws`).
- Negative lag: `trunc(cells_samples * 0.25 + 0.5)` as a stored integer. The consumer that uses this count
  was not located. The lowering keeps negative lag unsupported.
- Still open: shape law (`+0x..` shaper tables) and the bypass flag semantics.

## RandomBipolar leads (decompile pass, 2026-10-22; no law yet)

Survey impact: `external modulation from RandomBipolar` is the largest modulation gap (550 rows in
`Conflux/Multis/Ray Line.nkm` alone, 51 instruments corpus-wide). Routes stay dropped (`UnknownLawOmitted`).

- Source type 13 name and class: `0140a70000.c:3388` ("Random Bipolar"), type 12 is "Random Unipolar"
  (`:3382`). Classification path: `FUN_140973970` (`0140970000.c:2395`), already in the code.
- **Retracted (2026-10-22): the "type 0xc object" lead is ZeroMQ, not a random source.** The 0x650-byte
  object at `FUN_141a00880` (`0141a00000.c:331`) is built by the zmq socket factory `FUN_141a0e2d0`
  (`0141a00000.c:10872`, switch on socket type; case 0xc = ZMQ_SERVER, case 0 = PAIR; vtables resolve to
  `zmq::server_t`). Its callers (`FUN_141a2f9b0`, `FUN_141a0f9f0`) and the strings in `0141a20000.c`
  (`D:\NIBuild\3rdparty\zeromq-v4.3.5-R2\src\socket_base.cpp`) confirm it. The `rand()` seed at `+0x648`
  is the zmq identity seed, and `+0x14c = 0xc` is the socket type. Radare2 on `0x141a00880` (`pd 90`) and the
  vtable blocks at `0x1478f5678` and `0x1478f57a8` show zmq methods only. Do not use this object as a lead.
- Real source names: `FUN_140a73e30` (`0140a70000.c:3370-3392`) names type 0xc "Random Unipolar" and 0xd
  "Random Bipolar". The `rand()` sites in the decompile cache (`0140780000`, `01405d0000`, `01425e0000`,
  `0141a20000`) are not a modulator producer: `FUN_14078bc80` is an event dispatcher. The start-criteria RNG
  is MT19937 (BASIC_BEHAVIORS row 3); whether RandomBipolar uses that stream is open.
- Next: find the voice-modulation evaluator that switches on source type 0xd and its per-block producer.
  Use radare2 one function at a time (`pd N @ addr`), not a re-decompile.
