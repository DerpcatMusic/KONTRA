# The Multi LFO's saved fade cap, and the reports it causes

Status: open. No code changed for it. This records what the corpus and the
reader say, so a later pass does not repeat the search.

## The cascade

ANALOG STRINGS reports four `modulation of a module parameter` entries:

```
startPhase (<none>) destination slot 4 intensity 0
startPhase (<none>) destination slot 5 intensity 0.6099985837936401
```

They are not four independent gaps. The chain is:

1. Groups 39 `S10 L2 far` and 279 `S10 L2 far` each carry two Multi LFOs at
   internal slots 4 and 5, and an external `Constant` modulator at slots
   18 and 19 that addresses their `startPhase`.
2. Both LFOs are refused by the translator's waveform admission
   (`crates/sampler-kontakt/src/library.rs`, the `(5 | 6, Some([sine, pulse,
   triangle, saw, noise]))` arm) and each is reported as
   `LFO waveform (id, multi weights, pulse width)`:
   `5 Some([1.0, 1.0, -0.277, 1.0, 0.0]) 0.5`.
3. A refused modulator gets no runtime index (in
   `ir.source_indices.modulators` the group's slot-4 and slot-5 entries carry
   `runtime: None`; its slot-0..=3 and slot-7 entries do not).
4. `route()` binds `startPhase` only to a destination that has a runtime
   (`Target::SourcePhase`), so with no destination it falls through to
   `modulation of a module parameter`.

So one admission decision turns into four reports, and the two LFOs' own
volume routes (intensity 0.79) are lost with them. Fixing the admission
closes the waveform entries and these route entries together.

## Why the admission refuses

The saved LFOs differ from an admitted one only in the first initial value:

| Group 39 | `initial_values` | waveform | weights | admitted |
|---|---|---|---|---|
| slot 1 | `[0.0, 764.0, 0.5, 0.49902344]` | 5 | `[1,0,0,0,0]` | yes |
| slot 4 | `[5345.849, 24.0, 0.5, 0.5]` | 5 | `[1, 1, -0.277, 1, 0]` | no |
| slot 5 | `[5345.849, 24.0, 0.5, 0.50097656]` | 5 | `[1, 1, -0.277, 1, 0]` | no |

Every other guard in that arm passes: the weights are finite, the width is
0.5, the noise weight is zero, and the delay/frequency sync record is `-1`
(unsynchronized). The one failing guard is the fade bound,
`(0. ..=5000.).contains(&fade_ms)`, because the saved value is
`5345.849` ms.

## Where the 5000 ms comes from

`sampler_core::KontaktLfoFade::from_saved` also rejects `ms > 5000`. Its own
capacity note (`crates/sampler-core/src/voice_mod.rs`, the
`scoped_preparation_bytes` comment) gives the reason: the native fade is a
`/32` control-rate gain table of at most 60001 values, and `5000 ms` is
exactly `60001` ticks at the highest supported rate, `384 kHz`:

```
5000 ms * 384000 Hz / 32 * 0.001 = 60000 ticks
```

`5345.849` ms is therefore out of range only *at the top of the rate range*.
At 44.1 kHz it is `7366` ticks, far inside the table. The cap is a
rate-dependent capacity bound written as a rate-independent one.

The table itself is not incidental: the voice reads the prepared gains
by random index, `gains[age / 32]` (`crates/sampler-core/src/voice_mod.rs`,
the LFO arm of the value pass, and the onset read `gains[0]`), so the ramp
is materialized once per program rather than advanced per tick. That is why
the limit is a tick count and why the ramp cannot simply be evaluated on
demand: the iterative f32 product must stay bit-identical to the native
sequence.

One more consequence of the current split: a saved fade that passes the
translator and then exceeds the table at the host rate is a *lowering*
error (`from_saved(...).map_err(core(Stage::Modulation, owner))?`), not a
report. The translator's 5000 ms guard is what keeps that from happening,
which is why the two bounds have to be changed together.

## The open question that decides the fix

The two readers disagree about what that field means, and the answer decides
whether widening the bound is enough:

* `vendor/ni-file/src/kontakt/objects/lfo.rs` documents `initial_values` as
  **delay**, frequency, pulse width, start phase.
* `crates/sampler-kontakt/src/library.rs::lfo` reads `initial_values[0]` as
  the **fade-in** (`fade_ms`), builds `ir::Lfo { delay: Time::ZERO, fade_in:
  Milliseconds(fade_ms) }`, and for Multi/Digital Multi hands it to
  `LfoShape::NativeFaded`/`DigitalSine`, whose law is
  `sampler_core::KontaktLfoFade`.

A delay holds the cycle and then starts it; a fade scales the amplitude from
0.3 to 1 over the same interval. Both are plausible for a 5.3 s value, and
the corpus cannot tell them apart: no admitted LFO in the corpus carries both
a non-zero first value and a render we can compare.

What would settle it: one native render of a Multi LFO at 44.1 kHz with a
first value well past 5 s, watched for a held (delay) or rising (fade) start.
The existing `KontaktLfoFade` vectors in
`crates/sampler-core/src/v1_voice_controls.rs` cover the fade law itself, not
this field's meaning.

If it is a fade, the reader's bound can be restated as the tick bound
`ms * rate / 32 * 0.001 <= 60000` at the actual rate, which admits this pair
at every rate up to 359 kHz and keeps the table cap. If it is a delay, the
field should not reach `KontaktLfoFade` at all, `ir::Lfo::delay` should carry
it, and the Multi admission needs its own evidence.

## Reporting consequence

Until either answer is proved, the two reports are honest: the patch's
`startPhase` routes cannot bind because their destinations were not
translated. They are, however, *derived* reports — the root cause is the one
waveform entry, and a fix closes both rows.
