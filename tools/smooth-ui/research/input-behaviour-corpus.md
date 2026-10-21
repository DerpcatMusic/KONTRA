# SMOOTH input behaviour: corpus pass (soothe3 restored image)

Status: **partly recovered (round 3)**. Verified in this round: where the wheel
fields live (no copy exists), the wheel consumer and its scale constants, the
sign rule, the gating, and the accumulate-then-clamp structure. **Not
recovered:** the value law that turns the accumulated delta into a parameter
value (a candidate exists but its call link is broken, see section 4), the
OS-notch to JUCE `deltaY` unit, the modifier that selects the fine divisor,
drag/fine factors, reset target, click-to-type, hit radius, tooltip timing,
animation timings, menus, cursors.

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

## 4. Value law: candidate, not confirmed

Candidate: `fcn.1806c30c0` (RVA `0x6c30c0`) is the `slot2` of the vtable at
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

**Why this is not confirmed:** `fcn.1807b3640` calls the std::function as
`vtbl[2](impl, out)` with two arguments and never sets `r8`. The candidate reads
`S` from `r8`. Either this vtable is not the sink, or the call passes `S` in a
way the decompiler did not show. This must be settled by emulating
`fcn.1807b29b0` with a synthetic Input and gesture, and recording which function
the indirect call reaches and what `r8` holds there.

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

## 6. Consequences for SMOOTH

- `input.rs`: no constant changed this round. The wheel law now has its shape
  (sign from the flag, px per notch `100`, px per range `300`/`1000`) but the
  notch unit and the `Input+0xea` identity are open. Changing `STEP` or `FINE`
  before those are known would be a guess.
- `input.rs` `FINE`: the reference wheel path does not read Shift or Ctrl. The
  fine divisor depends on `Input+0xea`, which may be a modifier. Ratio 300:1000
  is 0.3, not 0.1.
- `graph/gesture.rs`: the band wheel parameter and the factors are still open.

## 7. Next steps (ordered)

1. Emulate `fcn.1807b29b0` with a synthetic `Input` (`+0xb8 = 0`, `+0xbc = 1.0`,
   `+0xc0 = 0`, `+0x150 = gesture owner`) and a gesture at `this+0x170`. Record
   every indirect call and the `r8` value at each. This settles the
   value-law link (section 4).
2. Find the writer of `Input+0xea` (the pump's per-event handler, or the
   Editor's modifier translation). This settles the fine divisor.
3. Find the JUCE peer's wheel-to-`deltaY` conversion by emulating the peer's
   `WM_MOUSEWHEEL` handler with a known `WPARAM`. This settles the notch size.
4. Only then change `input.rs` (`STEP`, `FINE`) and `graph/gesture.rs`.

Repro notes: the pseudocode excerpts above are from
`RunScratch/smooth/re-corpus/index/annotated/` (`007b29b0.c`, `007b2900.c`,
`006f5ae0.c`, `0073dc10.c`, `007b3640.c`). `fcn.1806c30c0` (section 4) has no
decompiled body in the corpus, so its excerpt comes from a capstone listing. The
constants come from the PE file (pefile `get_data`, VA minus base). The Knob
constructor (`fcn.180737410`) also failed in `pdg`, so its lines were read from
a capstone listing.
