# Conflux engine intent (2026-10-13)

Scope: installed Conflux 1.1.0, `Instruments/Conflux.nki`, 50 factory multis,
and 201 factory snapshots. Engine behavior is separate from the NativeUI/Vector
layout work owned by `gpt/conflux-ui`. Library files remain read-only; script
inspection uses `tools/library-inspector.py` in RAM. No decrypted script or sample
payload belongs in this document. Runtime coverage and the ranked gap table will
be recorded separately; the feature inventory below is not a parity claim.

The installed `Documentation/ReadMe.pdf` was read in full. It contains version
history and a pointer to the online manual, rather than synthesis instructions.
Native documentation: [overview](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/overview),
[edit](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/edit-page.html),
[performance](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/performance-controls),
[assignments](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/modulation-assignment),
and [settings](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/settings-page).

## Playing and sound generation

- Play exposes six assignable macros, mod wheel, bend, pressure, velocity and
  note-random modulation. Output volume is stored per snapshot.
- Wavetable and sampled layers can be selected independently, tuned and blended.
  Wave position and a selectable shaper change the oscillator's timbre. Earlier
  installed-source audits identify 92 groups: one sampler, 90 Time Machine 2
  sources and one wavetable. This topology is not 92 simultaneous layers: script
  selection and group gating must decide the audible groups.
- The auxiliary source supports FM/PM/RM, oscillator mixing, or another shaper
  with inharmonic control. Pitch units and shape must survive preset recall.
- A selectable resonant filter can process either layer. Key tracking, frequency
  and resonance are independently editable.
- Two LFOs and amp/mod envelopes drive assigned destinations. LFO2 can affect
  LFO1's rate. Saved sources, live controls and source-to-source routes all matter.

## Motion and routing

Six macros and the exposed MIDI sources can drive multiple targets. Assignment
editing sets depth and polarity. The Animator moves macro values using up to 16
steps with tempo-linked rate, several reading orders, amount and note restart.
It is a modulation sequencer; the separate factory multi helper scripts may
create or transform MIDI notes. Those helpers must retain their physical script
slot and program ownership.

## Effects

Four selectable/bypassable FX positions shape tone and space. Their user-facing
choices include dynamics, degradation, radio/cabinet/tape coloration, filters/EQ,
chorus/flanger/phaser, frequency shift, tremolo/vibrato/wow, delays, reverbs and
impulse responses. Changing a choice requires the corresponding native engine
module and routing, rather than changing only its displayed name. Saved group,
insert, send and main racks must be inspected independently of these four UI slots.

## Performance setup and presets

Active range filters notes before synthesis; learning writes its bounds. Pitch
bend has independent up/down ranges. Velocity and aftertouch have bounds and
curves, with mono/poly pressure selection. Mono mode selects low/high/last-note
priority and legato/retrigger behavior. Glide has always/legato and time/rate
options. Factory snapshots restore sound and script state; multis combine
programs and key/channel arrangements. Loading a preset must replay the correct
persistence behavior before a note is accepted.

## UI reports (do not fix in this package)

The existing `conflux-vector-layout.md` identifies missing authored NativeUI scene
projection in Vector. `conflux-callback-gate.md` establishes that a full 256-effect
outbox suspends callbacks until the host drains it and renders again. It does not
establish a broken library callback. Both findings belong in the UI worker's
review, with engine callbacks audited separately.
