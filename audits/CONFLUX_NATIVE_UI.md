# Conflux NativeUI investigation — 2026-10-04

The Conflux 1.1 instrument uses the older Lua-based NativeUI package, not just classic KSP widgets. Rendering the exposed classic controls alone does not reconstruct its authored interface. Its private local package contains 128 `.nui` modules and 282 NativeUI resources; the instrument imports 92 groups, 1,985 zones and 1,962 samples with no missing sample references. Those counts establish import coverage, not playback equivalence.

## Implemented shared paths

The host now loads the authored module graph and artwork, runs a bounded editor-owned Lua VM, and bridges exposed controls from all KSP slots. Pointer gestures, text acceptance, menu visibility, indexed values/metadata, MIDI learning and meter updates reach their corresponding KSP/runtime paths. Queued edits carry an instrument epoch so a previous editor cannot edit a newly loaded instrument. Channel/CC assignments persist with the project and continue to work without an open editor.

Conditional expressions preserve Lua `nil`/`false` behavior, and child traversal preserves numeric siblings beyond gaps left by conditional children. ForEach expands siblings in declaration order. Component modifiers retain their own layout boundary; decorations have distinct graph paths. Fixed spacers hug their authored frame, stack content takes the remaining space after fixed siblings, and ZStack uses independent horizontal/vertical alignment. Decorations do not contribute to the foreground's measured size. Canvas paths, rings, WebP/SVG artwork and rotation render through MUI.

Conflux's authored footer specifies 16-by-40 vertical rectangular sliders with static `Bend` and `Mod` labels. It supplies a thin custom pitch handle and a zero-size mod handle. It does not specify numeric overlays on wheels. Changing it to round wheels or adding always-visible numbers would alter the library's design.

## DSP evidence and remaining work

Per-event script modulation now reaches the supported voice targets, with release/pedal snapshots retaining the original event's values. Script source zero is used for aftertouch in this instrument. This does not admit every serialized modulation destination: wavetable/pan group parameters and LFO frequency/intensity routing remain separate gaps.

The native shared phase-form switch at `140567aa0` was inspected in a locally available Kontakt 8 binary. Its branch table, constants and math imports corroborate the implementation of bend/bend2, asymmetric/asymmetric2, PWM, flip, mirror, quantize, seesaw and exp/log phase transforms. The implementation retains the native asymmetric midpoint comparison, quantizer endpoint bias and rational bend2 mapping. Native disassembly and proprietary library assets remain outside this repository.

Sync forms enter that phase switch but need their additional readout path. Fold, saturate, blinds and wrap use other processing paths. Randomized phase, inharmonic modes, FM/PM/RM and oscillator noise also remain unsupported. The table reader's quality and anti-aliasing are not established as Kontakt-equivalent. A working interface does not certify those DSP modes.

## Support evidence and verification scope

Both open and closed issues were retrieved from the public `DerpcatMusic/KONTRA` repository and the private `DerpcatMusic/buffr-support` repository used by the support endpoint. The public repository returned no non-PR issues; none of the 287 private issue bodies/titles matched KONTRA, KONTAKTO or Conflux. There was no uploaded macOS stack to attribute in those issue records. Issue comments and private object-storage attachments without a matching issue were not independently inventoried.

The supplied black-window report mentions Bitwig/X11/Vulkan and is a Linux report; it does not establish the cause of the separately reported macOS failures. The other supplied screenshot shows an initialized instrument with failed library UI and release-sample behavior, which are distinct from a native platform crash.

Verification for this implementation uses a Cargo build and production MUI renderer captures of the actual local Conflux package, with page selection dispatched through KSP. No tests were run, as requested. The renderer capture does not load the audio sample bank and reports zero RAM: it is UI evidence, not a sound check. There is no affected-macOS host run or matching Kontakt reference-audio comparison. NKS navigation remains unsupported, and the shipped `.nui` reference `MenuFilter` has no corresponding exposed control in its three scripts; that missing binding remains visible rather than receiving a guessed alias.

Contracts: [NI KSP UI commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands), [engine parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters), [event commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands).
