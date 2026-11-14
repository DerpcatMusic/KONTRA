---
title: "A dead-looking UVI control is often a measurement artifact, not a missing mechanism"
author: DerpcatMusic
date: 2026-11-14
branch: uvi/flagship-params
cost: { minutes: 932, tokens: 3473482 }
id: l-e2bea906
scope: repo
status: active
src: agent
touches:
  - tools/inspect/src/ui.rs
  - crates/app/host/tests/rack.rs
  - crates/app/host/tests/script.rs
  - crates/engine/effects/src/response.rs
  - crates/engine/engine/src/params.rs
  - crates/engine/engine/src/script.rs
  - crates/engine/vm/src/action.rs
  - crates/format/kontakt/src/impulse.rs
  - crates/format/uvi/src/program/routing.rs
  - crates/format/uvi/src/program/tree.rs
---

## Decided
Judge a control by a phase-free print (octave bands + L/R correlation) of fresh notes, two before and two after the move, plus the release tail, after letting delayed script work settle (640 ms).

## Tried and ruled out
- Waveform difference: VWinds humanizes every note (random detune, attack pitch), so the floor is about -5 dB and hides everything.
- Rendering right after the move: VWinds' room size waits 150 ms before it sets Time, so the change landed between the two "after" renders and read as noise (floor 14 dB).
- A held 320 ms note: a room's length, a reverb's decay and a release time only show in the tail.

## Watch out
- Restore each control to its preset value, or a muted room hides every room control after it.
- Seats sweep in panel order, so neighbouring seats compare against each other and sound alike. Use isolated moves off the preset seat.
- VWinds close-mic reverb (reverbAmount, reverbDecay, reverbModel) is silent in every shipped preset: closeMicsVol is 0 to 0.03. That is by design, not an engine gap.
- Many top-used VWinds controls (vibAmount, maxDetune, tuneOutAttack, rampAttack, naturalAir) only set script variables read per note or legato, so a single held note can't hear them.

In the code: tools/inspect/src/ui.rs (SETTLE_BLOCKS, FRESH_BLOCKS, print, print_distance); KONTRA_INSPECT_UI_ONLY / KONTRA_INSPECT_UI_LEAN.
