# Authored renderer checkpoint — 2026-10-08

W3 owns authored resources, pictures, fonts, NativeUI and pure painters. W1 owns
selection/publication/typed transport; W2 owns input; W8 owns loading and header
artwork. This checkpoint does **not** certify all R0–R12 acceptance conditions.

## Shared scanner evidence

Driver source: `tools/kontra-scan@2355155b56a5ae26add8bdbc287d46d42e7825f2`.
Frozen v2 binary SHA-256:
`f1105599c13b14410ed891b29a014b8af43596e238850bb8e4439596e96e013a`.
W3 after adapter: `5a1793a3` (before the subsequent waveform/text/header changes).
Each item is a fresh whole worker, Original, using the common CLI and note plan.

| Fixture | Frozen v2 Original | W3 Original | Frozen v2 peak MiB | W3 peak MiB |
|---|---|---|---:|---:|
| Conflux | missing-images | original-ok | 229.18 | 264.71 |
| Analog Strings | original-ok | original-ok | 1257.89 | 895.82 |
| Clarinet A | missing-images | original-ok | 140.43 | 154.17 |

These are a three-item shard, not a corpus result. Logs/JSON/screenshots are in
`~/.cache/kontakto-w3/{final-baseline-v2,after-final-scanner/scan}`. There is no
same-final-driver combined W3+W8 load measurement yet. Analog's editor-open
memory gate against v1 remains open; lower picture residency alone does not pass
it. Clarinet's increase over the already-low frozen v2 peak also remains open.
Phase RSS and toggle/reopen/cancel measurements remain required.

Conflux consumes its real legacy `.nui` graph. The initial Native page resolves
167/167 requests (134 modules and 33 images), decodes 33/33 images and validates
8 supplied fonts. Its cream ground fraction changes from 93.8985% to 0%, hence
non-ground pixels change from 6.1015% to 100%. Native page pixel BLAKE3:
`4d49a3b17c15f0ba74735be00495b87389bb4485ed40bf15717c2006e92049a1`.
This is our authored renderer, not a retained native Kontakt golden capture.
Native Edit/FX pages and source callback gestures remain to be checked.
Clarinet uses the shared UVI resource route; visible requested frames resolve
8/8 and one supplied font resolves. The requested 21/21 full image inventory is
still a separate acceptance condition.

## Macro-name regression

The six complete published `@Footer__Macro__Name__1..6` strings reach Native
primitives and final text geometry unchanged. Only lengths/equality/metrics are
recorded. Generic editable payload insets `[8,6]` survived `.pad(0)`, reducing
44 px fields to 28 px viewports. Removing them restores the full viewport.
The Native graph declares no font for these fields; v1's bounded caption fit
is used with the existing fallback face while unfocused. Authored fonts and
focused editing retain their specified size. All six seven-character captions
now have a complete advance of 44 px inside a 44 px viewport.

`native_saved_text_reaches_authored_field_without_host_insets` fails before the
inset fix and passes after the full fit fix. It requires a locally owned fixture
and `RUST_MIN_STACK=67108864`; no saved names or source bytes are logged.
`native-text-{red,green,font,properties,fit}.log` retain the numeric receipts.

Supplied Native fonts can also resolve their embedded family/full/PostScript
names, rather than requiring the family to equal a filename. Only bounded,
validated package font bytes provide this authority; no system-font discovery
or substitution is performed. Font bytes participate in the existing 64 MiB
picture/font LRU. Native modules/fonts have a separate 16 MiB bound and Native
images a 48 MiB LRU; generation/cancel and phase-memory acceptance still need
broader lifecycle tests.

## Remaining gates

- Full strips/states, handles, hide masks, authored caption placement and source
  colours; corner/alpha and bitmap-font pixel fixtures at device scales 1 and 2.
- Native page/gesture/source-binding checks, including the four classic aliases.
- Structured shared resource failures and reload retry; arbitrary image-format
  frame/window/downscale fidelity and cancellation.
- Real waveform slices/wavetable semantics, source meter colour/range states,
  file captions and geometry/device snapping.
- Final-driver v1 comparison, owner-resolved W8 loader integration, phase RSS,
  quick15 and corpus-wide Original OK/no-regression acceptance.

W8's header/source-accessor checkpoint `93390aeb` is imported without rewriting
his loader or authored decoder. The only additional non-owned implementation
edit is W8's prerequisite allocation-accounting test block in `src/plugin.rs`.
The root shots no-run gate and the synthetic Native source-edit/context/font
checks pass. The real macro regression passes in 45.25 seconds.

## Merged transport and binding check

W1 `4f1abcc7` is merged, retaining source-addressed typed snapshots and the
generation-bound waveform provider. The merged macro fixture passes again in
41.07 seconds: all six complete strings have 44 px advances/viewports and zero
editable insets. An explicitly unavailable supplied family reports the Native
font-service category; it does not silently use a different font.

The initial Native Edit graph reads values from `Edit__Synth__Src__WTSelect`,
`SPLSelect` and `WTShaper` parameters, all from KSP slot 2 (UI IDs 32809, 32820,
32815). Their three aliases are not read through `Parameter.value()` or
`ksp_control_property` in this active graph (extended probe: 127.33 seconds).
Picker writes and other states remain untested, so this does not classify
their complete interaction intent. The
`Edit__Synth__Shp__SelectAlias` state remains unclassified. This RAM-only probe
invokes the authored tab callback and records primitive kinds plus binding
metadata; it does not certify pointer gestures or native-host stacking.

Native parameter and meter lookup now uses the lowest KSP script slot for
duplicate identifiers, independent of publication order. The synthetic
unordered slots 4/2/3 regression fails with last-wins lookup and passes with
slot 2, matching the official [expose_controls contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands).
The shared scanner adapter observes successful Native consumption instead of
retaining its old hardcoded false value; the common CLI remains unchanged.

PNG, JPEG, WebP and SVG now select the same requested strip frame/window and
device-size preparation. Whole-image non-streamable views share their decoded
Arc; selected views retain only their bounded pixels. The synthetic check
covers both strip axes, frame clamping, window overflow, downscale, cancellation
and alpha preservation. The merged UI suite passes 94 tests with 11 ignored.
Whole-worker memory and corpus acceptance remain open as listed above.

The fresh common-driver **debug** capture at `d0354672` loads Conflux but aborts
the Native graph at its unchanged 250 ms time guard, before image decoding.
There is no updated Native screenshot from that run. Its fixed diagnostic is
`NativeUI graph, NativeUI time budget exceeded`, digest `669a58cce2375280`.
The optimized three-item results above apply only to their recorded earlier
adapter, not to the latest branch. No guard was relaxed for the fixture.

Native readback now applies only to the addressed source and gives its typed
snapshot precedence over scalar fallback. A conflicting scalar value formerly
replaced the synthetic typed text and rewrote other script slots; that check
fails before the fix and passes after it. Native meter maps use the existing
per-source `PartShared.widget_meters` provider. The only new non-owned seam
edit is the meter-map argument at the Native loop in `src/ui/part.rs`.
