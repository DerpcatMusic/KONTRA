# Kontakt 8 Tone Machine and Beat Machine: decompile findings

Status: **missing, partial evidence.** No Tone Machine or Beat Machine render
kernel was recovered from the Kontakt 8 C export
(`Research/KONTRA/source/kontakt8/`, `READY` present). Function names there are
generic (`FUN_*`), and the render entry points are reached through vtable
slots whose targets are not named in the export. This unit records the
object layouts, dispatch and saved-field names that were confirmed, plus the
open leads. It changes no DSP. Both matrix rows stay `missing`.

Scope: Kontakt 8 only. Addresses are Kontakt 8.exe VAs. Decompile text is
read with `rg`; no C is pasted here.

## Evidence used

| Source (under `kontakt8/`) | What it gives |
| --- | --- |
| `0140d20000.c:653` `FUN_140d20640` | Lazy allocator for the Tone object: 0x420 bytes, then the Tone constructor |
| `0140cd0000.c:4553` `FUN_140cd94c0` | Tone constructor (vtable `BToneMachine::vftable`, embedded filters, zeroed state) |
| `0140d20000.c:700` `FUN_140d20780` | Beat Machine slice object: 0x90 bytes, default fields |
| `0140d20000.c:798` `FUN_140d208f0` | Machine id (group `+0xba8`) to vtable pointer; case 5 is the Beat Machine family |
| `0140870000.c:5446` | Internal-trigger beat-machine group rule (group `+0xba8 == 5`, flag byte `+0xc38 == 0`) |
| `0140920000.c:7498` | Stored-record parser: `"tone_machine"` selects mode 1 |
| `0140920000.c:7636..7690`, `0140930000.c:6583..6600` | Saved-record field names `formantShift`, `grainLength`, `dcFilter`, `hiQuality` |
| `01405f0000.c:7486..7540` | UI per-machine visibility; internal id 1 shows the "DC Filter" control |
| `0140580000.c:7363` | Undo label "Beat Machine activated" (the mode-change message only) |
| `0140cd0000.c:4551..4600` | `FUN_140cd94c0` body (see Tone object below) |

## Internal machine ids

The group field `+0xba8` is an internal id, not the stored source mode. The
stored-to-internal map is not yet confirmed for every id (see open items).

| Internal id (`+0xba8`) | Family seen in the export | Evidence |
| --- | --- | --- |
| 0, 4 | Shared vtable (sampler / DFD streaming family) | `FUN_140d208f0` cases 0 and 4 |
| 1 | Tone Machine: "DC Filter" control and its parameter ids | `01405f0000.c:7527`, case 1 |
| 5 | Beat Machine (internal-trigger group rule) | `0140870000.c:5446` |
| 9 | Checked in several places; repo maps stored 9 to Wavetable (`BWavetableMachine` ctor `0140cd0000.c:4597`) | `0140960000.c:5046`, `0140d10000.c:6410` (identity unconfirmed) |
| other | Default vtable | `FUN_140d208f0` default |

The UI switch (`01405f0000.c:7527`) for id 1 sets the "DC Filter" label on
the switch control with parameter id `0x312`, and writes `0x313` / `0x31e`
for the companion controls. These are UI-side ids; their DSP meaning is not
recovered.

## Tone Machine (internal id 1, stored mode 1)

### Object

- Allocation: 0x420 bytes (`FUN_140d20640`), zeroed, then `FUN_140cd94c0`.
  The Time Machine 1 object is 0x1d0 bytes (`FUN_140d205e0`), so the Tone
  object is a separate type, not the TM1 object.
- Constructor writes:
  - base fields: `+0x8 = -1`, `+0x10 = -1`, `+0x18` self-link, `+0x20 = 0`,
    `+0x4c = 1.0f`, vtable `BToneMachine::vftable`;
  - two 2-pole filter objects embedded at `+0xf0` (`BFilter2Pole`) and
    `+0x2f8` (16 state pairs, zeroed), then retyped as `BFilter2PoleHP`
    (the export shows both vtable stores; which one owns the object is not
    settled);
  - `+0x2cc`, `+0x2e4` zeroed; `+0xbc = -1`; floats `1.0` at `+0xd0` and
    `+0xe8`.
- Destructor `FUN_140cd9a80` (`0140cd0000.c:4843`): sets `BToneMachine`, calls
  the embedded filter teardown `FUN_140a87e40`, then base `BSoundObj`.

Filter objects: the constructor embeds `BFilter2Pole` and `BFilter2PoleHP`.
That is consistent with a DC-filter stage (a high-pass), but the coefficient
law is not read. The DC filter is switched by the parsed `dcFilter` flag.

### Saved-record fields (mode-1 branch of the stored reader)

The reader is the same one that recognises `tone_machine`. Field names in the
order the parser checks them:

| Saved name | Parser type | Target in the parser's record struct | Note |
| --- | --- | --- | --- |
| `formantShift` | float (`atof`) | `+0x64` | Formant control; law not recovered |
| `grainLength` | float (`atof`) | `+0x88` | Grain control; law not recovered |
| `dcFilter` | bool (compared with the literal `true`) | `+0x61` | Switch for the high-pass stage |
| `hiQuality` | next in the chain | not yet read | Quality flag; mapping not confirmed |

These are offsets in the parser's record struct, not the 0x420 DSP object.
Mapping the record to the Tone object is unproven; the repo's current
"Tone shares Time Machine 1 record" assumption is still unverified.

### Shared machine parameter names

`FUN_140d1ba90` (`0140d10000.c:7343`) names the controls of the machine family:
Speed, Smooth, Formant, Form 1, Phase, Ph Rand, Position, Inharm, Form 2,
Mod Amt, Mod Tune, Grain, Slice Atk/Rel, Transient. Which of them the Tone
object reads is not confirmed.

### Not recovered

- The Tone render kernel. Candidate render functions in `0140cd0000.c`
  (`FUN_140cdc4d0`, `FUN_140cdcd10`, `FUN_140cdd640`) were checked by callee and
  field use. `FUN_140cdcd10` calls the TM1 scheduler `FUN_140cda1b0`;
  `FUN_140cdc4d0` calls `FUN_140cd9d30` (1137 bytes, parameter-state prep). None
  is tied to the 0x420 object yet. `FUN_140cdb6f0` (3477 bytes) is the generic
  source render called from `0140870000.c:7620`, not a Tone-specific kernel.
- Grain size, overlap, alignment search and window (the stand-in constants in
  `time_machine.rs` remain UNMEASURED).
- Formant law, DC filter coefficients, `hiQuality` effect, and the `Ph Rand`,
  `Inharm` and `Mod` laws shared with the wavetable family.

## Beat Machine (internal id 5, stored mode 4)

### Slice object

- `FUN_140d20780` (`0140d20000.c:700`): 0x90-byte object, vtable `BSlice::vftable`,
  zeroed, then `+0x54 = 32` (dword), `+0x58 = 600` (dword), `+0x5c = 0` (word),
  `+0x8/+0xc = -1`, `+0x4c = 1.0f`. The meaning of 32 and 600 is not
  recovered (slice count or a timing value are both possible; not claimed).
- A separate object at `0140a70000.c:4607` stores the `BSlice` vtable at a
  different offset; the link between that object and the group is unproven.

### Internal-trigger rule (the only control-flow lead)

`0140870000.c:5446` runs when a group has internal id 5 and the flag byte
`+0xc38` is zero. The export shows that this path does not render audio. It
emits **script text** that disallows the group's notes and then steps through
slices: `$sliceIdx`, `$loopIdx`, `$loopCount`, `_num_slices(...)` and
`_slice_idx_loop_start(group, loop)` for up to 7 loops.

Inference (to be confirmed): the Beat Machine's timing is driven by generated
script over the slice table. If that holds, its parity depends on the script
engine (the parity workbench), not on a DSP kernel. The timing and playback
law are not recovered from this text alone.

### Not recovered

- The sequencer timing law and pattern source.
- The meaning of stored `slice_float_1`, `slice_float_2`, `slice_flag_1`
  (`crates/sampler-kontakt/src/library.rs` `machine()`).
- The slice-to-voice playback path (start, length, fade) and its render kernel.

## Gap table

| Module | Shared engine covers | Missing |
| --- | --- | --- |
| Tone Machine | Mode mapping and stand-in granular engine, reported `NotModeled` | Render kernel, grain/formant/DC laws, record-to-object mapping, `hiQuality` |
| Beat Machine | Stored mode reported; plays as sampler | Sequencer timing law, slice field meaning, slice playback path |

## Open items (for the next pass)

1. Find the vtable slot that runs for internal id 1 (the `BToneMachine::vftable`
   entries). Needs the vtable address from the Ghidra symbol table, not a
   binary session.
2. Confirm the stored-mode to internal-id map. The export groups internal id 4
   with the sampler family and uses id 5 for the Beat Machine rule, so stored
   Beat (4 in the repo) is not the identity map. Unresolved.
3. Oracle: one Wine dump of a Tone and a Beat Machine instrument (single session,
   nice 19) to fix the parameter laws.
