# Kontakt 8 classic modulation effects: Choral, Flair, Phasis, Vibrato/Chorus, Legacy Chorus, Legacy Flanger, Legacy Phaser

Status: Legacy Chorus, Legacy Flanger, Legacy Phaser and Choral are implemented
on the shared time-modulation core with recorded or independently computed
vectors. Flair, Phasis and Vibrato/Chorus are **not** implemented: their saved
fields are measured, but the core laws are not recovered yet. They stay
unsupported and report a reason (`unproved` in
`crates/sampler-kontakt/src/effects/timemod.rs`), so nothing is faked.

Blocked on the Kontakt 8 decompile export
(`/data/dev/workspace/Research/KONTRA/source/kontakt8/READY`, not present when
this unit was written; the directory is empty). Items marked **READY** below
need it.

## Shared engine coverage

| Engine piece | Where | Used by |
|---|---|---|
| Legacy modulated delay (4096-frame ring, 4-point Lagrange, base/excursion, feedback, optional second head with one-pole smoothing) | `crates/sampler-core/src/dsp/timemod/mod.rs`, `Native::LegacyModDelay` in `crates/sampler-ir/src/fx/timemod.rs` | Legacy Chorus (0x11), Legacy Flanger (0x12) |
| Four-stage legacy allpass cascade (`SweepLaw::Legacy`) | `dsp/timemod/mod.rs`, `Allpass` in `sampler-ir` | Legacy Phaser (0x14) |
| LFO: sine leapfrog, Hz or tempo-synced `Rate::Sync`, right-channel phase | `Lfo`, `Rate` in `sampler-ir` | all of the above |
| Voice bank (`BANK_VOICES` = 7 modulated lines, mode tables) | `dsp/timemod/voices.rs`, `Native::VoiceBank` | Choral (0x5d) |

Parameter mapping and unsupported reporting live in
`crates/sampler-kontakt/src/effects/timemod.rs` (`read`, `config`, `unproved`).

## Per module

### Legacy Phaser (0x14): implemented

- Saved order: Depth, Feedback, Speed, Phase, unit. Reader `FUN_140d01fa0` at
  +8/+14/+c/+10/+30 (`docs/re/kontakt-phaser.md`).
- Feedback stored × 0.85, Phase stored × 0.25 cycles, Depth feeds the
  `SweepLaw::Legacy` polynomial (factor 3, square 9, Taylor 1/2…1/720).
- Four stages per channel, input feedback from the final stage output.
- Speed: free Hz; tempo sync when unit > 9.999999717e-10, Hz = 1 / ((unit·60/tempo)·speed), speed ≥ 1, clamp 0..8 Hz, tempo < 0.1 BPM read as 120.
- Stereo spread: right channel starts at sin/cos(2π(1 − 0.25·Phase)).
- Evidence: `effects::tests::legacy_phaser_free_speed_phase_and_four_stage_vector`
  (independent f32 vector), `legacy_phaser_is_exact_and_reports_nothing`. Both
  pass on this branch.

Gap: the LFO is the shared sine leapfrog; the shape is confirmed by the
oscillator description in `kontakt-phaser.md` only. Not a gap in behaviour
today; add a shape selector only if a second product needs it.

### Legacy Flanger (0x12): implemented

- Saved order: Depth, Feedback, Speed, Phase, Color, unit.
- Color is the base delay in ms (0..5, enforced in `config`); excursion
  Depth × 50 frames; feedback stored × 0.95, subtracted from the ring write;
  Phase × 0.25 cycles; Speed through the shared sync getter.
- Evidence: `dsp::timemod::tests::legacy_flanger_native_vector_delay_and_negative_feedback`,
  `effects::timemod::tests::legacy_chorus_and_flanger_render_native_kemu_vectors`
  (kemu-rendered native f32 bits), partition-independence test.

Gap: the kemu vectors were rendered from a Kontakt 8.13.1 binary
under the old oracle, not from the current export. Re-check the feedback
sign and the 0.95 factor against the decompiled setter 0x1409637f0 and audio
0x140aa2350. Stereo spread here is only the 0.25-cycle right phase; confirm
there is no second spread control.

### Legacy Chorus (0x11): implemented, one open question

- Saved order: Depth, Speed, Phase, unit.
- Two heads (base, base >> 1) on LFOs 0.015 cycles apart, each one-pole
  smoothed with 0.95, summed. Excursion Depth × 75 frames. Phase × 0.25.
- **Open question:** the base delay is taken as `BParFXChorus +8 × rate ×
  0.001` and is 0 in practice, which makes the excursion clamp to −1 and
  Depth have no audible effect in the current mapping (see the comment at
  `effects/timemod.rs` config for Chorus). That contradicts the product
  intent of a Depth control and must be checked before anything else is
  built on it. It was reasoned from the container memset and kemu runs, not
  from a recovered store.
- Evidence: `legacy_chorus_and_flanger_render_native_kemu_vectors`.

Wine-oracle measurement (Kontakt 8.13.1 through `tools/kontakt-oracle/eqrender.py`,
`Effects/Chorus` INIT preset, Speed 0, Phase 0, impulse in, scratch renders
under `/data/dev/workspace/scratch/k-modfx-classic/`):

- A bypassed reference is silent in the wet path. With the chorus active the
  impulse response is two clean echoes: about 425 frames (8.85 ms) and 905
  frames (18.85 ms) at 48 kHz, equal level, with a fractional split
  (424/425 and 904/905) consistent with a second head exactly 480 frames
  (10 ms) later.
- Those echoes are not frozen by Speed 0: five identical Depth-0 steps moved
  the first echo 425 -> 457 -> 515 -> 536 -> 494 frames, so the modulation
  runs at a floor rate even at Speed 0.
- Depth 0 and Depth 1 produced the same echo series over the same step order
  (425, 458, 517, 536 in both runs), so Depth did not visibly scale the
  excursion in this measurement.

This contradicts the current mapping: the engine's base delay is 0 frames,
so Kontakt's ~8.85 ms echoes cannot be produced, and the kemu vectors were
rendered from that zero base. Those vectors must not be treated as Kontakt
truth until the base and LFO floor are explained. The Wine measurement is
the stronger evidence; the decompile is needed for the exact law.

READY: confirm the base-delay source (writer 0x140d104e0 / container slot
+0x58, audio 0x140aa0510). If the base is nonzero in the saved file, the
mapping is wrong and the Chorus vector must be re-rendered.

### Choral (0x5d): implemented as voice bank

- Saved: seven f32 (amount, rate, delay, voices, width, feedback, mix), two
  flags (invert, scatter), i32 mode (`0x51` version). Mode index
  `(int)(p9 × 4)` clamped to 0..3.
- Laws in `choral()`: amount d = a²·0.99 + 0.01, pitch ratio 2^(d·0.025);
  rate speed = r³·7.4 + 0.25 Hz; delay span = G0·(D·5 + 0.1) s; feedback
  g = fb^¼·0.999 with input gain (1 − min(g,.99)⁴)^¼.
- Four mode tables (`MODES`, 0x14193cd70), seven voices each.
- Evidence: `choral_laws_match_native_vectors`, `choral_renders_native_kemu_vectors`
  (two configurations, six samples each, f32 bits from kemu 8.13.1).

Gaps:
- Width, scatter and invert are applied through the voice bank but only the
  two kemu configurations cover them. Add one vector per flag only if a
  regression is found, not speculatively.
- READY: confirm mode 3 and 4 share a table as stated; the table is extracted
  from the old oracle.

### Flair (0x5c): unsupported

- Saved fields and most of the core law are extracted (`unproved`,
  `effects/timemod.rs`).
- Missing: the writer of the pitch note field (+0x40), the internal-rate
  resampler, and the random start phase.
- Public description (NI MOD PACK page): tape and pedal-style flanging,
  tempo-synced pitch modulation. Engine parameter names
  (`$ENGINE_PAR_FLAIR_*` in `crates/sampler-core/src/engine_parameter_names.rs`):
  Amount, Chord, Damping, Detune, Feedback, Invert phase, Mix, Mode, Offset,
  Pitch, Rate, Rate unit, Scanmode, Voices. Their ranges are not yet measured.
- Engine has no pitch-modulated flanger core. Do not map Flair onto Legacy
  Flanger: different law, different pitch path.

READY: recover pitch note writer, resampler and start phase. Then add a
Flair core config (flanger line plus pitch note path) to the shared engine,
not a Flair copy.

### Phasis (0x5b): unsupported

- Saved fields are measured in `docs/re/kontakt-bindings/fx_raum_delight_psyche_vibrato_phasis_replika.tsv`
  (version 0x51): Rate (+8), Rate unit table (+8 and +12, +13), Spread (+16),
  Feedback (+20), Mix (+24), Stereo (+36), Depth/Mod mix (+40), Notches
  table (+28), Invert phase (+45), Invert mod mix (+46), Ultra (+44).
- Notches table (`ENGINE_PAR_PHASIS_NOTCHES`, TSV rows): 0 for stored
  indices 0–2, 0.1…0.6 for 3–8, 0.8 at 10, 0 at 16 and above. Measured data,
  not yet a law.
- Missing: binding of saved fields to core parameter indices, and the core's
  oversampling resamplers (about 96 kHz internal).
- Engine has an allpass cascade (`SweepLaw::Sweep`, `Linear`) but no
  Phasis-specific law. Do not approximate it with the generic sweep; it
  would be a second bit-different phaser.

READY: extract the field-to-core binding and resampler, then add a Phasis
law selector to the allpass stage.

### Vibrato/Chorus (0x69): unsupported

- Saved fields are measured in the same TSV, but the core is a wave-digital
  LC ladder that models the delay line. Missing: that ladder.
- Engine has no LC ladder. The modulated delay line is the wrong shape:
  do not substitute it.

READY: extract the ladder equations and component laws; then add a ladder
law to the shared delay line.

## Gap table

| Module | Code | Engine status | What is missing | Evidence now | Needs READY |
|---|---|---|---|---|---|
| Legacy Phaser | 0x14 | implemented (SweepLaw::Legacy, 4 stages) | none verified; LFO shape from oscillator description | focused test passes | confirm sync fallback and shape |
| Legacy Flanger | 0x12 | implemented (LegacyModDelay) | none found; kemu-only vectors | 2 tests pass | re-check feedback sign, 0.95, setter 0x1409637f0 |
| Legacy Chorus | 0x11 | implemented on a zero base; contradicted by Kontakt | ~8.85 ms base, 10 ms second head, speed-floor LFO, Depth law | kemu vector test passes; Wine echoes measured | exact law from decompile |
| Choral | 0x5d | implemented (VoiceBank, 4 modes) | per-flag coverage | 2 kemu vectors pass | confirm mode table sharing |
| Flair | 0x5c | unsupported (reported) | pitch note writer, resampler, start phase, core | `unproved` | yes |
| Phasis | 0x5b | unsupported (reported) | field binding, oversampling resampler, core law | TSV measurements | yes |
| Vibrato/Chorus | 0x69 | unsupported (reported) | wave-digital LC ladder | TSV measurements | yes |

## Edge cases and checks

- Out-of-range values produce `Reason::InvalidValue` (Phaser, Chorus, Flanger
  range checks; Choral mode outside 0..3). Unsupported modules produce
  `Reason::NotModeled`.
- No allocation on the audio thread: all the modules above run from the
  pre-sized ring/bank.
- Tests: `cargo test -p sampler-kontakt --lib timemod` and
  `-p sampler-core --lib legacy_flanger`, both green on this branch.

## Confidence

- Phaser and Flanger: high for the vectors as written, medium for the
  kemu-derived parts until re-checked on the export.
- Chorus base delay: low. Flagged.
- Choral: medium-high on the two kemu vectors.
- Flair, Phasis, Vibrato/Chorus: no implementation; no confidence claim.
