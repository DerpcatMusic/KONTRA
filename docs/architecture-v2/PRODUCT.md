# KONTRA v2 product requirements

The user's stated goals for v2, recorded 2026-10-06. Every v2 agent works to this
document. [PLAN.md](PLAN.md) is the architecture contract; this file is what the
product must do. v1 (0.3.x) is not a compatibility target or a behavioural oracle.
Kontakt and Falcon behaviour is the reference.

## Lessons from v1 (0.3.152, commit d097c363)

Keep: the UI quality and the fact that library UIs loaded and displayed.

Must not repeat:
- **Script lag.** Library scripts (KSP, Falcon Lua) caused audible lag. Scripts run within a
  measured per-callback budget; heavy work never blocks the audio thread.
- **Sustain/sostenuto pedal bugs.** Pedals must be correct with scripts, MPE, channel
  reuse, voice stealing and release triggers. Regression tests cover each case on real instruments.
- **Libraries playing incorrectly.** Every known deviation is either fixed or listed in the
  load report (below). Nothing is silently wrong.

## Load any library

Kontakt (all container versions, monoliths, encrypted, NCW) and Falcon/UVI (UFS banks,
protected programs) from the user's own installed libraries, on Linux, Windows and macOS.

## Load report and logging

Each instrument load produces a structured report, shown in the UI and written to a log file:
- loaded: what was decoded, translated and is playing;
- missing: every feature not translated or not implemented, with a reason
  (unsupported DSP module and its parameters, unknown modulation law, script compile error with line:col,
  unsupported script builtin, missing sample file, access failure);
- runtime problems: script budget overruns, voice-capacity drops, streaming underruns, non-finite faults.

Reports are data (`sampler-ir`'s unsupported list grows into this), not log strings. Never
include key material.

## Expression

- **Full MPE on everything**, including instruments not authored for MPE: per-note pitch, pressure
  and timbre routed by default, configurable. Scripts can still consume events first.
- **Articulation switching migration**: keyswitched instruments can be driven by velocity
  ranges, MIDI channel, or CC/program instead of keys, generated automatically from the source.

## Output routing: a tree

```
Instrument (rack part)  ──► DAW output pair (1-2, 3-4, …), automatic, overridable
└─ Instrument mixer (nested, per instrument)
   ├─ Mic positions (close / room / surround …)
   ├─ Groups / articulation buses
   └─ any source bus, nested to arbitrary depth
```

- Each node: gain, pan, mute, solo, inserts, sends, and an output assignment (parent or a DAW pair).
- Default: each instrument gets its own DAW stereo pair automatically. No Kontakt-style manual batch ops.
- Source bus structure (Kontakt group/bus outputs, UVI layer/mic buses) is translated into this tree.

## UI

- The cleanest sampler UI. Study Koda Sampler, SINE Player and HISE for layout, browsing,
  articulation and mixer handling; adopt what's best and document why.
- **Mixer is nested**: child strips are slightly shorter than their parents, and a colour strip
  shows which parent they belong to. Collapse/expand per node.
- **Library UI loading**: every Kontakt script UI (all KSP UI types, performance views) and Falcon
  Lua UI is described by a **UI IR**: plain data (widgets, layout, bindings, assets), independent of
  the source format, rendered by our components.
- **Vectorized mode**: keep the library's background artwork, release all other bitmap assets
  (knob strips, sliders, buttons) from memory, and draw controls with our own vector components.
  Lower RAM, crisp at any scale. The original-bitmap mode remains available.

## Performance and memory

- Ultra performance: block-based, vectorized DSP; meet the 48 kHz/64-frame deadline at high polyphony
  with pitched voices, filters and modulation.
- Least memory: streaming with small resident heads, a global budget, eviction and purge;
  measured against Kontakt and Falcon on the same instruments.
- Claims need measurements. Comparative claims need matched-quality comparisons.
