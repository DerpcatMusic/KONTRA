---
title: "An invisible marker element must have a non-zero size or the scene forgets it"
author: DerpcatMusic
date: 2026-11-16
branch: ui/dnd-followups
id: l-marker-size-20261116
scope: repo
status: active
src: agent
touches:
  - crates/app/ui-view/src/lib.rs
  - crates/app/ui/src/input.rs
---

## Decided
Input that measures a widget from an invisible marker element (native-page hover areas, KSP mouse areas) gives the marker a tiny real size (0.01 pt), not 0 by 0 (#1598).

## Tried and ruled out
- A 0 by 0 block with an id: mui records no scene surface for an empty frame, so the lookup by id is always empty. The widget is silently dead in a real window while tests that drive the host directly still pass. Conflux's macro assign toggles never showed because of this.
- A full-size invisible element: it takes presses meant for controls above or below it, and mui has no pointer pass-through.

## Watch out
Any element input looks up by id through the scene must be wider and taller than 0. Test such paths through the face harness (crates/app/ui/tests/face_input.rs), not only the host.

In the code: crates/app/ui-view/src/lib.rs `MARK`; mui-scene `scene/walk.rs` `collapsed()`.
