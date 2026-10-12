# Conflux NativeUI investigation — 2026-10-04

The Conflux 1.1 instrument uses the older Lua-based NativeUI package, not just classic KSP widgets. Rendering the exposed classic controls alone does not reconstruct its authored interface. Its private local package contains 128 `.nui` modules and 282 NativeUI resources; the instrument imports 92 groups, 1,985 zones and 1,962 samples with no missing sample references. Those counts establish import coverage, not playback equivalence.

## Implemented shared paths

The host now loads the authored module graph and artwork, runs a bounded editor-owned Lua VM, and bridges exposed controls from all KSP slots. Pointer gestures, text acceptance, menu visibility, indexed values/metadata, MIDI learning and meter updates reach their corresponding KSP/runtime paths. Queued edits carry an instrument epoch so a previous editor cannot edit a newly loaded instrument. Channel/CC assignments persist with the project and continue to work without an open editor.

Conditional expressions preserve Lua `nil`/`false` behavior, and child traversal preserves numeric siblings beyond gaps left by conditional children. ForEach expands siblings in declaration order. Component modifiers retain their own layout boundary; decorations have distinct graph paths. Fixed spacers hug their authored frame, stack content takes the remaining space after fixed siblings, and ZStack uses independent horizontal/vertical alignment. Decorations do not contribute to the foreground's measured size. Canvas paths, rings, WebP/SVG artwork and rotation render through MUI.

Conflux's authored footer specifies 16-by-40 vertical rectangular sliders with static `Bend` and `Mod` labels. It supplies a thin custom pitch handle and a zero-size mod handle. It does not specify numeric overlays on wheels. Changing it to round wheels or adding always-visible numbers would alter the library's design.

### NativeUI vector substitution boundary

NativeUI now has an authored opt-in visual receipt on an Image node:
`vector_recipe({version=1,role="slider",width=40,height=20,content=shape})`.
The admitted roles are `slider` and `knob`. `content` is an existing NativeUI
shape subtree, not a new drawing callback API. Lua's normal resolver expands it
at a separate visual path. The host requires version 1, exact original image
dimensions from retained metadata, and a shape-only graph with no Images,
interactive controls, gestures, or popovers. A single admission helper controls
both drawing and pixel demand, so unknown/mismatched receipts fall back to
Original. The actual Vectorized view flag selects this path; Original ignores it.

Only the Image's painting is replaced. Original intrinsic size, Image properties,
outer layout/decorations, graph identity and bound gestures stay on the same node.
Value/range/orientation/state behavior remains in the original authored controls
and Lua expressions; an annotation does not substitute a guessed control handler.
The replacement cannot reference Original image payloads through its primitive
graph. Metadata and Lua state survive when decoded and compressed pixels retire.

`authored_slider_recipe_preserves_pixels_pointer_ksp_and_releases_original_artwork`
passed the central validation run (0.39 seconds) with a real PNG-backed horizontal slider and an explicit rectangle-track
receipt and its original value-driven handle overlay. It compares Original and
Vector pixels/geometry, drives a pointer click to normalized .75 through an actual
exposed KSP slider (raw 75, one `on ui_control` callback), checks the handle moves,
and checks both pixel/payload owners expire while Lua remains usable. It also
restores Original drawing/demand for version and dimension mismatches and malformed
shape properties, child collections, or Canvas callbacks.

Unannotated library components still lack a semantic receipt: function expansion
drops originating component identity, and image names, colors or nearby labels
cannot establish it. Stock Conflux remains Original unless its author supplies
an admitted recipe. No image classifier or claim of full library vector coverage
is introduced by this path.

Hidden subtrees are excluded from NativeUI image demand. Drawing now follows
that ownership policy: hidden Images do not lease decoded pixels; known image
metadata still supplies exact intrinsic dimensions. An absent dimension record
on a hidden Image contributes zero intrinsic size while authored frame/layout
modifiers remain effective. Visible missing resources retain their existing
error. This prevents an undisplayed missing Image from replacing the whole
visible UI with an error; it does not infer dimensions for unavailable artwork.

## DSP evidence and remaining work

Per-event script modulation now reaches the supported voice targets, with release/pedal snapshots retaining the original event's values. Script source zero is used for aftertouch in this instrument. This does not admit every serialized modulation destination: wavetable/pan group parameters and LFO frequency/intensity routing remain separate gaps.

The native shared phase-form switch at `140567aa0` was inspected in a locally available Kontakt 8 binary. Its branch table, constants and math imports corroborate the implementation of bend/bend2, asymmetric/asymmetric2, PWM, flip, mirror, quantize, seesaw and exp/log phase transforms. The implementation retains the native asymmetric midpoint comparison, quantizer endpoint bias and rational bend2 mapping. Native disassembly and proprietary library assets remain outside this repository.

The native sample-form path is now traced and implemented for saturate18, fold19, blinds20–23 and wrap27. The oscillator at `0x140cd5af0` applies phase forms 1 then 2 (`0x140cd5c51`, `0x140cd5c6b`), reads/interpolates/morphs its tables through `0x140cd55c0`, then applies sample forms 1 then 2 (`0x140cd5d3f`, `0x140cd5d64`) through `0x140ce6eb0`. Both sample slots receive the original oscillator phase, before its advance, including when a phase form changes the read position. The independent unipolar preview routine `0x140532860` corroborates placement after morph.

For bipolar sample `x`, normalized amount `a` and original phase `p`, the traced laws are:

| Form | Native sample law |
| --- | --- |
| Saturate18 | Clamp `(1 + 8*a²)*x` to -1..1. |
| Fold19 | Set `v = (.25 + 3.75*a²)*x + .25`; output `4*(abs(v-roundf(v))-.25)`. The math import at `0x1443d107e` is `roundf`, not floor/truncation. |
| Blinds20–23 | With count2/4/6/8, attenuate by `1-a` when truncating `2*count*p` gives an odd integer; otherwise retain `x`. Helper `0x14054a600` proves the cell boundary and amount law. |
| Wrap27 | Subtract `a` below phase .5, add `a` at/above .5; add2 below -1 or subtract2 above1 once. Exact ±1 endpoints remain unchanged. |

Evidence comes from static local analysis of Kontakt8.13.1, PE image base `0x140000000`, SHA-256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`; no binary was activated or included in this repository. Bounded diagnostics are `/tmp/kontra-kontakt8-wavetable-sample.c`, `/tmp/kontra-kontakt8-wavetable-order.asm` and `/tmp/kontra-kontakt8-wavetable-read-blinds.c`. Staged PCM checks use independent constant-cycle expectations for nonlinear post-morph behavior, slot composition, original-phase boundaries and fragmented rendering; execution is pending the central validation run.

Sync1–6 (IDs1–3/24–26) still need their separate readout/quality path and remain unsupported. Randomized phase, inharmonic modes, FM/PM/RM and oscillator noise also remain unsupported. The table reader's quality and anti-aliasing are not established as Kontakt-equivalent. A working interface does not certify those DSP modes.

## Support evidence and verification scope

Both open and closed issues were retrieved from the public `DerpcatMusic/KONTRA` repository and the private `DerpcatMusic/buffr-support` repository used by the support endpoint. The public repository returned no non-PR issues; none of the 287 private issue bodies/titles matched KONTRA, KONTAKTO or Conflux. There was no uploaded macOS stack to attribute in those issue records. Issue comments and private object-storage attachments without a matching issue were not independently inventoried.

The supplied black-window report mentions Bitwig/X11/Vulkan and is a Linux report; it does not establish the cause of the separately reported macOS failures. The other supplied screenshot shows an initialized instrument with failed library UI and release-sample behavior, which are distinct from a native platform crash.

Verification for this implementation uses a Cargo build and production MUI renderer captures of the actual local Conflux package, with page selection dispatched through KSP. No tests were run, as requested. The renderer capture does not load the audio sample bank and reports zero RAM: it is UI evidence, not a sound check. There is no affected-macOS host run or matching Kontakt reference-audio comparison. NKS navigation remains unsupported, and the shipped `.nui` reference `MenuFilter` has no corresponding exposed control in its three scripts; that missing binding remains visible rather than receiving a guessed alias.

Contracts: [NI KSP UI commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands), [engine parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), [event commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands).
