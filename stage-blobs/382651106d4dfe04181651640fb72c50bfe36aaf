# Shared UI services and source frontends

Required product scope, revised 2026-10-06. This separates implementation targets
from current evidence; none of the imported UI families is complete yet. The
production plugin remains on the old composition root. There is no v2 DAW build.

## Verified source distinctions

| Source family | Required frontend behavior | Shared native services | Current v2 evidence |
| --- | --- | --- | --- |
| No authored UI | Generate useful controls and sample editing from semantic metadata | Stable controls, edit admission, state, assets, meters | Typed headless scalar controls; renderer pending |
| Native KSP widgets | All widget types, source IDs, layout, accessibility and callbacks | Same controls and callback ownership | Knob/slider/button/switch declarations and integer state; partial per-control callbacks; renderer pending |
| Bitmap KSP | PNG plus companion metadata, sprite selection, wallpapers, geometry and interactions | Same KSP control values; shared image assets | Pending; v1 is reference material only |
| Creator Tools GUI Designer | `.nckp` hierarchy and resource/control bindings; `.nckc` reusable authoring components | Same KSP semantic controls and presentation nodes | Pending |
| Komplete UI | Typed Komplete Script, packages/components, reactive dependencies, layout/modifiers, fonts/assets and Kontakt bindings | Controls, state, async services and rendering primitives | Source architecture reviewed; runtime pending |
| Falcon/UVI | Lua language/API, program hierarchy, widgets, events, coroutines and asset services | Same control and musical owners; source-specific dispatch/profile | v1 reference reviewed; new frontend pending |

KSP bitmap skins and stock widgets share the KSP runtime. Creator Tools exports
performance views as `.nckp`; `.nckc` represents reusable controls/containers and is
not directly loaded by KSP. Creator Tools Lua edits an instrument through the
authoring tools; it is not the language of Komplete UI. These distinctions are
specified in [NI's Creator Tools overview](https://docs.native-instruments.com/ni-tech-manuals/creator-tools-manual/en/overview-of-creator-tools).

Komplete UI uses **Komplete Script**, with typed imperative logic and declarative,
reactive UI composition. Its Kontakt integration connects to KSP controls. Preserve
the authored `Resources/info/library.json` `kontaktTargetVersion` when selecting a
frontend profile; do not reinterpret every package as the newest language/runtime.
Sources: [language tour](https://developer.native-instruments.com/komplete-ui/docs/LanguageTour/),
[integration/versioning](https://developer.native-instruments.com/komplete-ui/docs/KontaktIntegration/).

Resource containers distinguish `pictures` from `komplete_scripts`. Browser tiles,
NKS/library metadata and branding are separate from the instrument's interactive
performance view. Source: [NI resource containers](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/resource-container)
(the KSP 8.12 page is pinned in the existing reference cache).

## Ownership and interchangeable presentation

1. Source frontends preserve authored hierarchy, identities, units, assets, language
   version and unsupported material. They lower to shared controls and execution
   services; they do not create their own note/voice lifecycle.
2. [Headless control state](CONTROL_STATE.md) owns musical values independently of
   any window. Source variable identity, persistent control ID, KSP `get_ui_id`
   numbering and compiled dense index are distinct mappings.
3. UI callbacks require an independent callback owner retaining its originating
   plan/script instance. Do not fabricate a MIDI note to execute a knob callback.
   Bounded locals, waits, cancellation, outcomes and generated-note relationships
   now work for plan-owned scalar handlers. Script-instance subdivision and
   generated-note services for non-note contexts remain open.
4. Presentation nodes reference semantic controls and assets. Stock, bitmap and
   native vector presentations can change without copying/resetting control values.
   Preserve explicit interaction contracts: momentary edges, drag gestures, focus,
   keyboard edits, accessibility and source callback order.
5. A different vector appearance is a presentation choice, not proof that an
   arbitrary reactive component can be translated losslessly. Keep source language
   behavior, layout constraints, input handling and semantic bindings intact; expose
   an explicit diagnostic for unsupported meaning.
6. The asset/render layer owns decoded images, fonts, sprite metadata and GPU
   resources away from audio. Waveform editing submits versioned semantic edits;
   drawing never mutates a live source cursor or owns sample lifetime.

Lua, KSP and Komplete Script need distinct language implementations. Share their
native services and ownership rather than force a common grammar or a vendor-shaped
VM into every module. Choose the Lua implementation against real coroutine/event/UI
fixtures, allocator and GC behavior, licensing and cancellation requirements. A
worker-only Lua runtime is insufficient evidence for synchronous musical callbacks;
its timing/dispatch contract must be demonstrated before claiming support.

## Completion order and gates

- Land stable controls, transactional edits/recall, bounded UI handoff and coherent
  capture. **Implemented for scalar native state**, including new KSP declarations.
- Plan-owned `on ui_control` dispatch is implemented on shared continuations. Add
  script-instance state, the remaining UI callback family, property/automation
  semantics and full typed widget values.
- Bind a new native UI to those services; implement the sample editor against the
  same owned asset/source model. Validate closed/reopened UI and presentation changes.
- Complete KSP widget/property/resource behavior and GUI Designer hierarchy import.
- Implement Komplete Script/reactivity/packages and its versioned Kontakt service
  bindings; verify representative authored components and nested reactive layouts.
- Implement Falcon Lua/hierarchy frontend against the same ownership and services.
- Replace the production composition root, validate Linux CLAP/Bitwig, and remove
  old execution paths. Full DSP, streaming and external-format requirements remain
  their own gates; a working editor does not close them.

Each source family requires readable authorized fixtures, interaction/state traces,
asset/error cases and executable audio/service evidence. V1 tests and open-source
samplers identify obligations but cannot establish Kontakt/Falcon fidelity.
