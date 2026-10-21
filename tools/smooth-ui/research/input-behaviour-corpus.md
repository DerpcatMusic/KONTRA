# SMOOTH input behaviour: corpus pass (soothe3 restored image)

Status: **round 4: wheel law and drag law recovered by emulation** (section 8).
Verified by running the image's own code: the value law (sink
`fcn.1806c30c0`, divisor 300 or 1000), the modifier byte that selects the
divisor (Shift only), the notch size under JUCE's Windows convention, and the
per-frame wheel handling (last delta wins). The drag law is recovered by code
reading of the same sink. **Not recovered:** reset target and Ctrl/Alt roles
beyond the wheel, click-to-type, hit radius, tooltip timing, animation
timings, menus, cursors, keyboard steps, and the graph band's own steps.

Image: `/tmp/opencode/rea/soothe3-restored.bin`, base `0x180000000`. Pseudocode
from the corpus (`RunScratch/smooth/re-corpus/index/annotated/<rva>.c`). Float
constants were read from the PE file (pefile), not from pseudocode. The
disassembly was checked with capstone. Addresses are VAs.

## 1. The Input struct is the wheel store (no copy needed)

The UI pump `kali::UserInterface` / `PluginUI::slot2` = `fcn.1806f5ae0`
(RVA `0x6f5ae0`) runs with `this` = `host+0x200`. It calls the root component's
update with `Input = this+0x280` (emulated in round 2, see
`input-behaviour-emulated.md`).

The wheel consumer `fcn.1806f4640` (kind `0xf` case, pcs `0x1806f4c2f..4c4b`)
stores the event into `this+0x338`, `+0x33c`, `+0x340`. Those are
`Input+0xb8`, `+0xbc`, `+0xc0`. So the round-2 question "where is the copy from
`UI+0x338` into `Input`" has a simple answer: the store **is** the Input
object. Nothing copies it. Round 2 missed it because it treated `this+0x280` and
`this+0x338` as separate objects.

Per-frame Input handling in the pump (`006f5ae0.c`, after the event pops):

- `*(this+0x338) = 0` runs at the end of the frame (`Input+0xb8`, wheel dx).
  The wheel delta is therefore per frame, not a running total.
- Double buffer: `this+0x369 = this+0x368`, `this+0x36b = this+0x36a`,
  `this+0x36d = this+0x36c`. These are `Input+0xe9/0xeb/0xed` (prev) from
  `Input+0xe8/0xea/0xec` (cur). The current byte at `Input+0xea` is read by the
  drag code (section 3).

| Field | Input offset | UI offset | Meaning |
|---|---|---|---|
| wheel dx | `+0xb8` float | `+0x338` | delta, per frame |
| wheel dy | `+0xbc` float | `+0x33c` | delta, per frame |
| wheel flag | `+0xc0` byte | `+0x340` | reversed-direction flag (see 2) |
| param array | `+0x0` pointer | `+0x280` | records of 0x28 bytes |
| changed bitset | `+0x30` | | |
| hover/press component pointers | `+0x150`, `+0x158`, `+0x160`, `+0x168` | | |
| position | `+0x70`, `+0x74` float | | |
| scale | `+0x80` float | | drag px scale |
| modifier-like byte | `+0xea` (cur), `+0xeb` (prev) | | see section 3 |

The flag at `+0xc0` (`UI+0x340`) is stored from the event byte at `+8`, and
the editor push copies the event as two `float32` deltas plus one byte. That
layout matches JUCE's `MouseWheelDetails` (`deltaX`, `deltaY`, then the
`isReversed` bool at `+8`). That reading is an inference from the layout. It
is not verified against JUCE source.

## 2. Wheel consumer: `fcn.1807b29b0` (RVA `0x7b29b0`)

Called by `Knob` update (`fcn.18073dc10`, slot 2): `func_0x1807b29b0(this+0x170,
this, Input, Output)`. The `this+0x170` object is a gesture
(`fcn.1807b2900`, the gesture constructor). Its first qword is the parameter
id, `+0xd0` is a float scale (default `0x3f800000` = 1.0), `+0xdc` the state
(2 = idle).

Verified sign rule (`007b29b0.c` lines 95–102):

```c
if (*(arg3 + 0xc0) == '\0') fVar18 = *0x18209d2b4;   // +1.0
else                        fVar18 = *0x18209d9c0;   // -1.0
fVar19 = *(arg1 + 0xd0) * *(arg3 + 0xbc) * fVar18;   // dy term
fVar18 = fVar18 * *(arg1 + 0xd0) * *(arg3 + 0xb8);   // dx term
```

- Sign: `+1` when the flag byte is 0, `-1` when it is nonzero. Constants at
  `0x18209d2b4` = 1.0f and `0x18209d9c0` = -1.0f.
- Scale: `gesture+0xd0`, default 1.0. Nothing in this function writes it
  (the writer was not located).
- **No Shift or Ctrl appears in the wheel path.** The only Input bytes read
  are `+0xc0` (sign) and `+0xea`/`+0xeb` in the common tail (section 3). Shift
  and Ctrl are not read here. The SMOOTH "fine = Shift or Ctrl" rule is
  therefore not supported for the wheel by this function.
- Gate: the wheel applies only when `Input+0x150 == owner` (the control is the
  hovered component). `fcn.1807b29b0` line ~421.
- Accumulate: `gesture+0xb8 += dx term`, `gesture+0xbc += dy term`
  (lines 453–454). The accumulator is reset at roll start (`*(arg1+0xb8) = 0`
  when the state changes, line ~452).
- Common tail: `fStack_e8 = gesture+0xb8 * -100.0f`,
  `fStack_e4 = gesture+0xbc * -100.0f` (`0x18209da68` = -100.0f). The pair is
  packed into a 0x40-byte context `S` (`iStack_118 = Input`, `S+0x30/0x34` = the
  px pair, `S+0x38 = 1`) and passed to the value call (`fcn.1807b3640`, line
  ~470).
- Rollback: if the call returns a valid value and it is `<= 0` or `>= 1.0`
  (`0x18209d2b4`), the accumulator is undone (lines 480–482). So the accumulator
  only keeps deltas that stay inside `(0, 1)`.

So for a wheel delta `dy` with flag 0, the px delta is `-100 * dy * scale`. The
value step, after the (unrecovered) law, is `100 * dy * scale / D` of the range,
with `D` the px-per-range divisor in section 4.

## 3. Drag scale and the common-tail gate

- Drag px scale: `fVar18 = gesture+0xd0`, times `Input+0x80` when
  `gesture+0xcc == 0` (`007b29b0.c` lines 297–300).
- Drag branch conditions use `Input+0xea == Input+0xeb` (same for `+0xe8`,
  `+0xec`). What `+0xea` represents is **not confirmed**. It is the byte the
  pump shifts into `+0xeb` every frame, so it is a current state (a button or a
  modifier).
- The value law (section 4) selects its divisor from `Input+0xea` as well.

## 4. Value law: confirmed in round 4 (section 8)

Sink: `fcn.1806c30c0` (RVA `0x6c30c0`) is the `slot2` of the vtable at
`0x182085ea0`. That vtable is the one stored by the Knob constructor
(`fcn.180737410`, `lea rax,[rip+0x194ea09]` at `0x180737490`) for the
std::function passed to the gesture constructor. The function reads its state
`S` through `r8`:

```
x  = (S+0x38 ? S+0x30*0.25 + S+0x34 : S+0x34)      // 0.25 at 0x18209d130
d  = ([*S + 0xea] != 0) ? 1000.0 : 300.0          // 0x18209d8a8 / 0x18209d844
v  = S+0xc - x / d                                  // S+0xc = value at roll start
v  = (v >= 1.0) ? 1.0 : v;  v = max(v, 0)           // 0x18209d2b4 = 1.0
return (v, valid=1)
```

With `S` from section 2 (`S+0x30 = -100*accX`, `S+0x34 = -100*accY`,
`S+0x38 = 1`), this gives `v = v0 + (100*accY + 25*accX) / d`. The divisor is
300 px per range normally and 1000 when `Input+0xea != 0`. For a vertical wheel
notch of `dy = 1`, the step would be 100/300 = 1/3 of the range.

**Settled (round 4, emulated):** `fcn.1807b3640` calls the std::function as
`vtbl[2](impl, out)`. It does not write `r8`, so the register still holds its own
third argument `S`, the context the consumer built. The emulated call reaches
`0x1806c30c0` with `rcx = impl`, `rdx = out`, `r8 = S`. The sink reads the
divisor from `Input+0xea` (`cmp byte ptr [rax+0xea], 0` on `rax = [S]`, the
Input), and the emulated sink returns `{v, valid = 1}` as the code above says.

Verified constants (read from the PE): `0.25` (`0x18209d130`), `1000.0`
(`0x18209d8a8`), `300.0` (`0x18209d844`), `1.0` (`0x18209d2b4`), `-100.0`
(`0x18209da68`), `-0.01` (`0x18209d974`, used in the graph branch), `0.04`
(`0x18209d09c`), `0.001` (`0x18209d03c`), `1e-5` (`0x18209d010`).

## 5. Other items checked this round

- Graph band wheel (`Graph` slot 1, `fcn.180644730`) uses the same gesture helper
  (`fcn.1807b29b0` handles band lists through `arg4+0x80`). Which parameter a
  wheel over a band changes is not confirmed.
- `Knob` slot 3 (`fcn.180739410`) is a 34-byte thunk. Its body is in other
  `.pdata` fragments. Not traced this round.
- Wheel units: the mapping from OS notch to JUCE `deltaY` was not found. A
  search for the `WM_MOUSEWHEEL` compare (`0x20a`) and for `120`, `1200`,
  `2048` float constants in the corpus found nothing. Without this, the size of
  one notch in SMOOTH units is unknown. The SMOOTH placeholder `STEP = 0.01` is
  not supported by this round's constants, and `100/300` per unit would be 33x
  larger. Neither value should be copied without the unit.

## 6. Consequences for SMOOTH (round 4)

- `input.rs`: `STEP = 60/256 * 100 / 300 = 0.078125` of the range per notch;
  `FINE = 300/1000 = 0.3`, selected by Shift only (Ctrl and Alt are not in the
  wheel law); `DRAG_PX = 300` and `DRAG_PX_FINE = 1000` physical px per range.
  The placeholders `PAGE` (0.1) and the Ctrl+click reset are removed: the
  reference has no evidence for them.
- The drag divisor is applied through `ui.drag_with_fine(.., fine = false)` with
  the px value chosen by Shift, so mui's own fine factor is not used. The px
  value is divided by `ui.scale()` because the reference's drag is
  `dy_physical / divisor` (section 8).
- The text slider now drags on the vertical axis like the knob. The reference's
  drag sink reads only the vertical term (section 8), so a horizontal text
  slider has no reference counterpart. Confirm on the reference's TextSlider
  before relying on this.
- `graph/gesture.rs`: `FINE` now equals `input::FINE`, and the wheel's fine key
  is Shift only. The band's width, depth and frequency steps stay placeholders:
  the band's own parameter normalisation is not recovered. Alt-to-depth is not
  in the reference's wheel path and needs its own check.

## 7. Next steps

Done in round 4: the value law link (section 4), the Input+0xea writer (section
8), the notch unit (section 8), and the drag law (section 8).

Open, in order:
1. Reset target: the reference's double-click (or other reset gesture) path. The
   `Knob` update's gate on `Input+0xe8` (Ctrl) and `Input+0xec` (Alt) is read by
   the drag branch: find what sets `gesture+0xca` and `gesture+0xcd`.
2. Keyboard steps: find the reference's arrow-key path (virtual-key compares in
   the kali keyboard handler), so that arrows and Home/End can be checked.
3. Band graph: the wheel's parameter for each band and its normalisation (the
   `fcn.180644730` band wheel).
4. Hit radius and click-to-type (the `TextInput` path), tooltip timing and
   animation timings.

Repro notes: the pseudocode excerpts above are from
`RunScratch/smooth/re-corpus/index/annotated/` (`007b29b0.c`, `007b2900.c`,
`006f5ae0.c`, `0073dc10.c`, `007b3640.c`, `006f4640.c`, `00366f30.c`). The
emulation scripts are `tools/smooth-core/research/re-tools/emulate/wheel_chain.py`
(run with the venv in `RunScratch/smooth/re-emulate`, `python -I`).

## 8. Round 4: emulated value law, modifier, notch, drag

Method: isolated-function emulation of the restored image (`wheel_chain.py`).
The image's own code runs for the Knob constructor, both Editor slots, the ring,
the UI consumer and the Knob update. Synthetic inputs are listed in the script.

**Value sink.** Emulated: `out.v = 0.578125` for one Windows notch with no
modifier, and `0.523438` with Shift, from a start value of 0.5. The arithmetic
is `v = clamp(v0 - x / d, 0, 1)` with `x = -100 * dy` for the wheel (`dy` is the
JUCE delta, `flag = 0`), and `d = 300` or `1000` on `Input+0xea`.

**Modifier byte.** Editor slot 34 (`fcn.180366f30`, JUCE modifier flags in
`arg2[0]`) pushes kind `0x12`. The consumer's case 0x12 writes four bytes of the
packed word into the Input: byte 0 to `+0xe8`, byte 1 to `+0xec`, byte 2 to
`+0xea`. Emulated: flag `0x01` sets `+0xea`, `0x02` sets `+0xe8`, `0x04` sets
`+0xec`. Under JUCE's `ModifierKeys` order (shift = 1, ctrl = 2, alt = 4, from
memory of the JUCE sources, not checked in the image) that makes `+0xea` the
Shift bit, `+0xe8` Ctrl and `+0xec` Alt. Confidence: high for the byte mapping
(emulated), medium-high for the JUCE bit order.

**Notch unit.** The reference's JUCE peer converts a Windows `WM_MOUSEWHEEL` to
`deltaY = clamp(0.5 * (short) HIWORD(wParam), -1000, 1000) / 256`, so one
120-unit notch is `60/256 = 0.234375`. From memory of `juce_win32_Windowing`,
not checked in the image: confidence about 75%. The X11 convention (50/256 per
button 4 or 5) is recalled at about 50%. The image has no emulated peer code for
either. The wheel law is linear in `dy`, so a different platform unit scales the
step directly.

**Per-frame wheel.** The consumer's case 0xf stores `dx` and `dy` (it does not
add them). Emulated with two wheel events in one consumer pass: only the second
delta (0.5) survives. The reference therefore uses the last wheel delta of each
frame, and drops the rest. mui queues every wheel event as its own input
(`vendor/mui/src/host.rs`, `Pending::Input`), so SMOOTH applies all of them.
That is a parity gap, and it lives in the shared vendor host layer.

**Fling gain.** `vendor/mui-baseview` (`notch_lines`) multiplies a fast run of
notches (gap under 90 ms, same direction) by up to 3x. The reference has no such
gain, so SMOOTH's control steps are up to 3x the reference's on a fast run. This
is a KONTAKTO vendor patch shared with the lists.

**Drag law.** Code reading, not emulated: the drag branch (`007b29b0.c`,
lines 297–340) sends `S+0x34 = dy * scale` with `S+0x38 = 0`, so the sink uses
`x = dy * scale` only. `scale = gesture+0xd0 * Input+0x80`, and `Input+0x80` is
`UI+0x300 = *(config+0x630)` (`006f5ae0.c:1290`), the device scale: the consumer
divides incoming positions by the same value. The drag is therefore
`v -= dy_physical / 300`, or `/ 1000` under Shift. The drag threshold is 1 px
from the press point (`sqrt(dx² + dy²) <= 1.0`). The gesture branch also has a
gate `gesture+0xca != 0 || Input+0xec == 0 || Input+0xe8 != 0`: with Alt held and
Ctrl not held the drag is skipped unless `gesture+0xca` is set. The meaning of
`gesture+0xca` is open (open item 1 in section 7).
