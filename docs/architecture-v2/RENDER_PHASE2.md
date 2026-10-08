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
