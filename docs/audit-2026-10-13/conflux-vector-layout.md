# Conflux Vector authored-layout gap

The existing full-editor test proves successful CPU layout and paint of two different UI representations. It does not prove authored layout correctness or Kontakt pixel parity. The Original screenshot renders the evaluated NativeUI composition; the Vector screenshot renders overlapping classic KSP control geometry, including multiple edit branches at once.

## Evidence

- Screenshots: current-ui-gates/0-Conflux-Original.png and 0-Conflux-Vector.png. Original is visibly coherent; Vector visibly overlaps and loses the native composition.
- Current initialized report: corpus-fresh-initialized/initialized-05.jsonl, receipt-05.json. Slot 2 has 378 widgets, 371 with parent links, and only 6 own-hidden. The edit/synth/mod parent panels are not hidden in that KSP snapshot. Many raw knobs have unsized axes. Initializer reports zero faults. UI report status is partial; its scope explicitly excludes native renderer, pixels, gestures and callbacks.
- src/ui/part.rs:598 evaluates native.view only for Original. Native authored size is consulted in this branch. Vector reaches ir_view::view_state at line 669 with the same selected KSP face. The source tab remains GUI in both full-editor screenshots; this is not an obvious different-tab selection.
- crates/sampler-ui-ir/src/lib.rs:60 and 65 stores the NativeUI entry string, not a projected native layout graph. Presentation::Vector at line 743 is described as retaining wallpaper/background art and drawing controls as vectors.
- crates/sampler-ksp/src/ui.rs:470..529 imports classic KSP geometry, default-axis intent and whole-control hide flags. Native graph visibility/state is not imported there.
- src/ui/ir_view.rs:606..689 defaults an unsized knob to 85x52 and resolves parents into page geometry. At line 787 it calls Interface::visible. That query at sampler-ui-ir/lib.rs:790 walks all ancestors and suppresses hidden descendants. Evidence does not support a dropped-parent-hide bug in the renderer.
- The 213 unsupported slot-2 UI properties in the report are NKS metadata (136 type, 30 style, 24 numeric-values, 23 string-values). They do not diagnose the NativeUI-to-Vector composition gap.

## Classification

Unsupported native authored composition projection in Vector is the supported explanation. Classic raw KSP control visibility and geometry cannot be assumed to equal the evaluated native composition. No narrow source-backed correction to hidden bits, source selection or renderer overlap policy is established. Hiding arbitrary controls, selecting only the visually pleasant branch, replacing Vector with a bitmap, or loosening visual tests would not establish the requested parity.

## Prioritized follow-up

1. Emit an explicit source-free UI diagnostic when Vector is selected for a native authored interface without native-graph projection. Include source slot, selected presentation, native-entry-present, projection capability, and unresolved layout/visibility scope. Keep CPU-paint success separate from authored visual correctness and native parity.
2. Capture the evaluated NativeUI scene semantics (bounds, hierarchy, clipping, z/order, conditional composition, parameter identity, state and resources) into a typed projection suitable for vector rendering. Preserve native events and bindings; do not infer visibility from variable names.
3. Establish a native Kontakt oracle for the same instrument, recalled state, selected page, zoom, viewport and resource-ready moment. Compare visual geometry/composition and interaction/readback in Play and Edit states. Existing KONTRA Original is a useful regression comparator, not native proof.

The source review made no renderer changes. Current CPU paint success does not establish authored visual correctness or native parity.
