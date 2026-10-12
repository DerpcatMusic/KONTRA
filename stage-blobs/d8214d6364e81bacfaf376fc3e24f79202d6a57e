# Koda-style views for the live sound editor

Research notes (September 2026) on how comparable samplers show what an
instrument does, and which of their views KONTRA's sound editor
(`src/ui/editor.rs`, `src/ui/viz.rs`) adopts.

## What is out there

There is no "Koda Player" product. KODA Sampler (KODA Sampler Inc., early
access since NAMM 2026, VST3/AU/standalone) is one app that both builds
and plays libraries. Its public docs are mostly shortcut lists, so the
details below are thin. Anything marked unverified comes from forum posts
or could not be confirmed.

- **KODA views** ([ui-overview](https://kodasampler.com/docs/ui-overview)):
  - Play: an instrument rack with quick controls and a four-layer crossfader pad.
  - Mapping: a pitch × velocity grid with round-robin layers and a zone inspector.
  - Lane Edit / Zone Editor: sample waveforms showing offsets, crossfades and loops.
  - Keyboard Edit: key ranges drawn on the on-screen keys.
  - Structure: articulation, then variation, then group.
  - Setup: an AHDSR envelope.
  - Modulation: modulator graphs (envelopes, LFOs, tables) edited as nodes.
  - Routing: bus mixers.
  - Parameter: automation and CC maps.
  - A GUI editor, and pop-out windows for more than one monitor.
  - Unverified: an EQ or filter response curve with handles, rings on the
    modulated knobs, live voice indicators, and a compact/expanded toggle.
- **Sine Player** (Orchestral Tools): a Performance view with an articulation
  grid, a Mixer where each instrument unfolds to its mic positions, and
  Options. It draws no envelope or EQ graphics.
- **Kontakt 8**: a redesigned side pane and Conflux's graphical source-to-target
  links. Its AHDSR and filters are knobs; the Flexible Envelope has up to
  32 breakpoints.
- **Pro-Q pattern** (FabFilter; from memory, the help page was 404): one
  response curve over a log-frequency axis, one draggable node per band,
  and a pre/post spectrum behind it.
- **Vital**: envelope and LFO shapes with draggable points on a grid, and
  animated modulation rings on knobs.
- **Decent Sampler**: coloured key ranges on the keyboard and an XY pad bound
  to two parameters.

Sources: kodasampler.com, rekkerd.org and synthtopia.com (KODA early access),
the KVR forum thread t=627448, the Orchestral Tools help docs, the Native
Instruments Kontakt 8 manual, Sound On Sound's Kontakt 8 review, the Decent
Sampler developer guide and the Vital user guide.

## Adopted

1. **Response curve with a handle per band** (the Pro-Q pattern, and
   Koda's graph editing). A group's filters and EQ bands are drawn as one
   magnitude curve, computed from the same TPT SVF prototypes the voices
   run. Handles set cutoff/frequency (x) and resonance/gain (y); the wheel
   sets EQ bandwidth. This is the densest way to show a whole insert chain.
2. **Envelope graph with a handle per stage** (Koda Setup/Modulation and
   Vital). The curve is traced with the engine's own envelope, stage by
   stage. The handles are attack, curve, decay/sustain and release.
3. **Key × velocity zone map** (Koda Mapping). The watched group's zones,
   with every sounding voice as a dot.
4. **Live playheads**. None of the references confirm them. They are cheap
   here: the audio thread stores 16 voices' stage and level in atomics.
5. **Compact / expanded toggle.** Compact shows the curves alone; expanded
   adds each value as a number, with a reset per control.
6. **Library ghost** (our own addition). Once a value is overridden, the
   library's curve stays drawn faintly under the edited one, so "what I
   changed" can be read at a glance.
7. **Spectrum** (the Pro-Q pattern). Behind the response, the part's
   post-fader output; beside the mixer, the selected part's or the master's.
   The audio thread copies one strip into a lock-free ring only while a
   spectrum shows (`plugin::Scope`); the UI thread transforms it (realfft,
   4096 points, at most 30 times a second) into smoothed log bands with a
   falling peak hold.
8. **Modulation and effects lists** under the graphs (Koda's Modulation and
   Routing, read-only): each source to its target with its depth and, for
   the wheels, velocity and key, its value now; the group's inserts and the
   instrument's chains with bypass and whether KONTRA plays them. Wide mixer
   strips list the instrument's inserts.

## Not adopted (yet)

- **Modulation rings and routing graphs**: KONTRA models modulation as
  imported tables. Nothing edits them yet, so the lists above are read-only.
- **Bypass toggles**: an effect's bypass is shown, not switched; switching
  needs an engine path for it.
- **Gain reduction**: no compressor has DSP here yet.
- **Waveform and loop lane editing**: this is a builder's view, not a
  player's.
