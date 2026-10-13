# Conflux engine intent (2026-10-13)

Scope: installed Conflux 1.1.0, `Instruments/Conflux.nki`, 50 factory multis,
and 201 factory snapshots. All NKI slots and snapshot records were inspected.
The all-multi inspection was stopped by the coordinator under the later no-survey
rule; exhaustive multi coverage is not claimed. Engine behavior is separate from the NativeUI/Vector
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

## Fresh installed-source findings

The memory-only inspector on the base build read all five NKI slots. Slots 0/1
are empty; slot 2 is the 6,629-line main synth script, slot 3 is the 1,995-line
mono/portamento script, and slot 4 is a 38-line helper. All frontend stages and
all three active initializers succeed. Main has 279 UI handlers and note,
release, controller, poly-aftertouch, persistence, async, listener and PGS
handlers. The mono script has another 13 UI handlers, note/release/controller,
persistence and PGS handlers. The helper has note/release/listener callbacks.
Main issues 418 engine-parameter calls and six zone writes in the source.

Source selection (main slot 2, function starting at line 2613) reassigns zones
between the wavetable bank and playable source group and updates their low/high
keys using `ZONE_PAR_GROUP`, `ZONE_PAR_LOW_KEY` and `ZONE_PAR_HIGH_KEY`. All six
writes are frontend stubs. Selecting a wavetable consequently requires live
zone topology, not a scalar change to wavetable position. The sample selector
chooses groups using authored group gating/purge logic.

The main script's phase-form choices include sync, PWM, asymmetric/bend,
flip/seesaw, quantization, log/exp, wrapping, amplitude fold/mirror/saturation
and blinds. Their numeric identity differs from menu order. Primary shape mode
and amount are written at lines 2678/2683; second shape at 2697/2736. Audio-mod
controls issue WT_MOD_TYPE/WAVE/TUNE/TUNE_UNIT/AMOUNT; inharmonic controls issue
WT_INHARMONIC_MODE/INHARMONIC. Neither a complete oscillator nor correct table
selection can be inferred from an admitted saved phase-form subset.

Fresh translation retains 92 groups, 1,985 zones, 1,962 assets, 17 buses, 108
chains, 3,566 modulators, 2,724 routes and one decoded impulse. The NKI declares
keys 0..127 and velocities 1..127; script range/learning applies additional
musical filtering. Saved group inserts contain 91 filter objects (82 active,
9 bypassed). Instrument insert slot 3 is a bypassed convolution and slot 7 is
active module 0x13. Group/send/main ownership remains distinct from UI FX order.
