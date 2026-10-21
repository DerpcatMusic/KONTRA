# SMOOTH: reference input and UI behaviour (soothe3, static RE)

Status: **partial**. Recovered with evidence: the wheel message path, the
component vtable layout, the full tooltip text block, and a few constants.
Not recovered: wheel step, drag sensitivity, fine factor, reset target,
click-to-type, hit radius, tooltip timing, animation timings, right-click
menus, cursor shapes. The blockers are in section 6.

## Method

- Binary: `/tmp/opencode/rea/soothe3-restored.bin`, PE, image base `0x180000000`,
  sha256 `0143f03b7c7f1e41a51896f269a64c3dfc4c804e94ce8cccbeef9c98dbbcfa64` (sweep index meta). Addresses are VAs unless marked RVA.
- radare2 5.x: `aa`, then `pdg` (Ghidra) per function. Batch scripts were run
  with `r2 -i`; outputs live under `RunScratch/smooth-re-ui/` (not in the repo).
- Sweep index `index-default.sqlite`: vtables, vtable slots, and call sites
  (`sites.function_rva` maps a site to its enclosing function).
- Obfuscation: several input handlers flatten control flow and compute
  constants through MBA arithmetic. Ghidra fails on some outright
  (`fcn.180739410`: "Instruction overlaps instruction"). Pseudocode quality is
  the limit on every item marked "not recovered" below.

## 1. Class and vtable layout (recovered)

| Class | Vtable VA | Slots | Overrides vs `kali::Component` (VA 0x182073ab0) |
|---|---|---|---|
| `kali::Component` | `0x182073ab0` | 13 | base |
| `Knob` | `0x182085f10` | 13 | 2, 3, 4, 7, 11, 12 |
| `TextSlider` | `0x182087478` | 13 | 2, 3, 4, 7, 11, 12 |
| `Toggle` | `0x182087600` | 13 | 2, 3, 4, 7, 11, 12 |
| `ChoiceMenu` | `0x182087718` | 13 | (not diffed) |
| `TextInput` | `0x182086438` | 13 | (not diffed) |
| `Graph` | `0x18207c5a8` | 13 | 0, 1, 2, 3, 4, 7, 8 |
| `kali::Editor<visualisation_data>` | `0x18205a400` | 56 | slot 8 = wheel (below) |

Slot targets (Knob): 2 `fcn.18073dc10`, 3 `fcn.180739410`, 4 `fcn.18073d550`,
7 `fcn.18073dba0`, 11 `fcn.18073eef0`, 12 `fcn.18073ef40`.

- **Slot 2 is `update(Context&, Input const&, Output&)`.** The mangled lambda
  names `update@Soothe3PresetMenu`, `update@PresetConfirmationDialog` and
  `update@MainComponent` all take those three arguments.
- **Slot 7 is a hit test.** Knob slot 7 (`fcn.18073dba0`) is an axis-aligned
  rectangle test on fields `+0x4c..+0x58`, returning `this` or null. It has no
  hit radius beyond the component bounds.
- **Slot 4 is paint** (Knob `fcn.18073d550` draws with float geometry).
- **Slot 3 is probably the pointer handler** (Knob `fcn.180739410`, 12,340 bytes,
  Ghidra error). Inferred from size and from the Graph vtable, which puts
  `Graph::handle_interaction` in slot 1 (inferred from its mangled lambda names).
  Not confirmed.
- Slot 11 of Knob calls a shared relayout/state function; slot 12 is an
  obfuscation stub (no input meaning).

## 2. Mouse wheel (partly recovered)

Recovered:

- **Entry:** `kali::Editor` slot 8 = `fcn.180367590` (RVA `0x367590`). It copies
  8 bytes from the event at `arg2+0` and one flags byte from `arg2+8` into a
  9-byte payload. The 8 bytes are presumably the two `float32` deltas of a JUCE
  `MouseWheelDetails` (assumed, not confirmed). It then calls
  `fcn.180331100(host+0x300, kind=0xf, &payload)`, where `host = *(editor+0x1d0)`.
- **Transport:** `fcn.180331100` is a lock-free multi-producer ring: cells of
  `0x80` bytes, `cell.seq = (pos/cap)*2+1` when published, `*arg1` = capacity,
  head at `+0x40`.
- **Magnify shares kind 0xf.** `kali::Editor` slot 9 = `fcn.1803674f0`
  (RVA `0x3674f0`) pushes kind `0xf` with payload `(1 − scale, 1 − scale)`, where
  `scale` is the float argument. Any consumer of kind 0xf must handle both.
- The UI-side messages on a second host ring (`host+0x6b0 + 0x140`) are kinds
  `0xc`, `0xd`, `0xe` from `kali::UserInterface` slots 3/4/5 (`fcn.180563d30`,
  `fcn.180563dd0`, `fcn.180563e70`). Kinds `0xc` and `0xe` carry two floats
  each. Kind `0xd` carries two 32-bit values; their meaning is not established.

Not recovered:

- **The consumer of kind 0xf.** Searched without success:
  - Functions that load the host field `+0x1d0`: 106 candidates, of which 87
    were decompiled by `pdg` before the decompiler stalled on one of the rest.
    The queue writes seen are kinds `0xc`, `0xf`, `0x11`, `0x12`, `0x15`, `0x24`.
    No queue pop (the `(pos/cap)*2+2` release) was seen. The three `*2+2`
    matches are a vector-growth routine (`fcn.1818dc770`), not the queue.
    Coverage is therefore 87 of 106, not complete.
  - The editor destructor calls a virtual at `+0x260` on the host. The
    `Soothe3AudioProcessor` vtable (77 slots) has a stub at slot 76
    (`fcn.18034e890`). Whether that object is the host is **not confirmed**.
  - `case 0xf:` in `fcn.1805b3d20`: a DSP/graph data-write switch, not a queue.
  - Functions near the push (RVA `0x331000–0x336000`): no pop found.
- Therefore the wheel step per notch, the Shift/Ctrl/Alt factors, and what the
  wheel does over the graph (width vs depth) are **unknown**. The SMOOTH values
  (`STEP` 0.01, `FINE` 0.1 in `input.rs`; `FINE` 0.2 in `graph/gesture.rs`) are
  placeholders.

## 3. Drag, fine drag, reset, click-to-type (not recovered)

- `Knob` slot 3 (`fcn.180739410`) is the presumed drag/press handler. It is
  12,340 bytes of MBA-obfuscated code and Ghidra gives up. Its constant loads
  include `0.04` (`0x18209d09c`), `1.0`, `-16`, `24`, `14`, `0.83`. Their roles
  are unconfirmed.
- `TextSlider` slot 3 (`fcn.18079cfa0`) and `Graph` slot 1 (`fcn.180644730`,
  1,142 lines) are obfuscated too: 116 and 173 suspect constants.
- Event binding: `fcn.1806fad10` looks up an event for a control in a hash map
  keyed by FNV-1a of an 8-byte id (`arg3+0x68`). That is the binding layer.
  Not the drag maths.
- Click-to-type: `kali::TextInput` exists (vtable `0x182086438`). The
  entry format and the trigger gesture are unknown.
- Pixels per full range, the fine factor, and the double-click or Ctrl+click
  reset target are **unknown**.

## 4. Keyboard (partly recovered)

- The tooltip text confirms these gestures: "Double-click to select and close
  the menu" (preset list), "[Ctrl]+[Click] to select multiple categories",
  "double clicking on its background" (hide the side panel).
- Key-name strings (JUCE key table, present in the binary): `cursor left`,
  `cursor right`, `cursor up`, `cursor down`, `return`, `spacebar`, `backspace`,
  `escape`, `insert`, `delete`, `page up`, `page down`, `control`, `shift`,
  `option`, `command`, `alt + `, `shift + `, `ctrl + `. They come from the JUCE
  `KeyPress` text table, not from a kali hotkey table. Which keys SMOOTH's graph
  responds to is not confirmed.
- `Graph::handle_interaction` takes a `KeyPress` lambda, so the graph does
  handle keys. The arrow-key step is not recovered.

## 5. Tooltips (recovered text; timing not recovered)

- The tooltip strings sit in one contiguous run of `.rdata` text; for example
  `Adjust the dry/wet mix of the output signal.` is at VA `0x181fe3630`. The run
  runs through the preset, channel and graph strings, about 200 lines in total.
  `reference-ui-text.txt` holds the subset of tooltips that SMOOTH uses.
- Corrections applied in `tips.rs` this round (verbatim reference wording):
  `gear` → "Adjust plug-in settings and check for updates.";
  `help` → "Manual, support, and other helpful resources.";
  `side-panel` → "Hide the side panel. You can alternatively hide the side panel
  by double clicking on its background."
- Deliberately not changed:
  - `preset-prev`/`preset-next`: the reference says "in the current category".
    SMOOTH has no category navigation yet.
  - `preset-filter`: the reference filters by creator and favourite. SMOOTH's
    filter differs.
  - `wet-trim`: SMOOTH-specific parameter.
  - `mode`: the reference's bold markup is rendered as lower-case text in SMOOTH.
- Not recovered: tooltip delay, placement, fade. The constants `1500`
  (`0x18209d8b8`), `200` (`0x18209d818`) and `120` (`0x18209d7c8`) are used by
  unrelated calls (a notice and a layout), not confirmed as tooltip timings.

## 6. Animation timings, menus, cursors (not recovered)

- Float constants seen in paint code: `0.5`, `0.4`, `0.6`, `0.66`, `0.3`, `0.2`,
  `0.1`, `0.05`. None is tied to a fade, attack or curve smoothing, so none is
  used.
- Right-click menus: no kali menu strings were found. The strings `Cut`, `Copy`,
  `Paste`, `Delete`, `Select All` and `Default` belong to JUCE's `TextEditor` and
  property panel. Their use in soothe3 is not confirmed.
- Cursor shapes: not recovered.

## 7. What would settle the open items

1. Find the pop side of the editor ring. A release store `seq = (pos/cap)*2+2`
   (with a `LOCK`-prefixed compare) is the signature. Search the functions
   that call the `fcn.1803xxxx` queue helpers, or run the corpus job
   (`smooth/re-corpus`) to completion and grep for `(… / *arg1) * 2 + 2`.
2. For the Knob and TextSlider drag handlers, deobfuscate by emulation: r2 ESIL
   (`aezi`/`aesu`) on `fcn.180739410` with a synthetic `Input` (one 8-byte id,
   one drag delta), reading the value written back to the parameter array.
   Vary one input at a time to get the px-per-range constant.
3. Find the tooltip timer: `UTooltipData` sits in `std::optional` (type
   `_Ref_count_obj2@optional@UTooltipData`); the reader is the tooltip owner,
   which has not been located.

## Implementation status in SMOOTH

- `tips.rs`: three strings aligned to the reference; a unit test pins them.
- `input.rs`, `graph/gesture.rs`: the wheel, fine and key constants are
  placeholders that this recovery does not replace. They are labelled as such.
