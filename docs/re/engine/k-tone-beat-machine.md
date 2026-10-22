# Kontakt 8 Tone Machine and Beat Machine: decompile findings

Status: **missing, candidate kernels located.** The process kernels are now
located through the cached vtable table (`docs/re/kontakt-rdata/vtables.tsv.gz`,
section "Vtable slots" below), but they are not ported: the Tone kernel is
three ~2 KB vectorised variants whose laws are not yet read, and the Beat
Machine has no audio kernel of its own (slice playback, see below). No DSP is
changed; both matrix rows stay `missing`.

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

## Vtable slots (cached rdata, `docs/re/kontakt-rdata/vtables.tsv.gz`)

Calibration: the known TM1 render (`0x140ce3600`, `time_machine.rs`) is slot 23
of `BTimeMachine` (`0x144fd06f0`). The S1200 and MP60 renders are slot 27 of
their vtables. So the render slot is per class; read it from each class's
own vtable.

| Class | vtable VA | Bases | Slot 2 (factory) | Process candidates |
| --- | --- | --- | --- | --- |
| `BToneMachine` | `0x144fd0bb0` | BGrainMachine, BMachineBase, BSoundObj | (`ToneMachineModule` slot 2 = `FUN_140d20640`, the 0x420 allocator) | slot 23 `FUN_140ce3ff0` (2197 B), slot 24 `FUN_140ce23e0` (2207 B), slot 25 `FUN_140cdd640` (2070 B) |
| `BTimeMachine` (TM1, reference) | `0x144fd06f0` | BGrainMachine, ... | n/a | slot 23 `FUN_140ce3600` (2448 B, the TM1 render) |
| `BTimeMachine2` (TM2, reference) | `0x144fd07d0` | BGrainMachine, ... | n/a | slots 23/24/25 `FUN_140ce2d20` (2208 B), `FUN_140ce0dd0` (2198 B), `FUN_140cdc4d0` (2056 B) |
| `BSlice` (Beat Machine) | `0x144f39f00` | BMachineBase, BSoundObj | (`SliceMachineModule` slot 2 = `FUN_140d20780`, the 0x90 allocator) | no slot above 189 B; slot 18 `FUN_140cdab80` (189 B), slot 12 `FUN_140ce6780` (174 B), slot 13 `FUN_140ce6210` (89 B) |
| `BWavetableMachine` | `0x144fd0c90` | BMachineBase, BSoundObj | n/a | slot 18 `FUN_140cdaf50` (332 B) |

Findings from these bodies (decompile, not yet ported):

- The three Tone candidates share one skeleton (same field offsets, same
  calls: `FUN_140883ce0` source length, `FUN_140883f10` block rounding,
  `FUN_14086e030` source pass). They differ in their arguments and locals.
  Which one runs for a normal Tone voice is not settled. The TM2 candidates have
  the same sizes within a few bytes, so the Tone kernel may be a TM2-family
  skeleton with grain constants (hypothesis, unconfirmed).
- Beat Machine slots 12 and 13 walk a table of up to 7 entries (stride `0x38`,
  one-based index) and test an entry's field at `+0x20`: values below 3 are
  skipped, value 4 marks a loop entry, and the entry's field at `+0x18` is a
  count (0 means unbounded). This is the sequencer that the script emitter
  (`0140870000.c:5446`) drives with `$loopIdx` / `$loopCount`.
- Beat Machine slot 18 derives a start and length from two float parameters
  (`+0x90`, `+0x94`) times a scale constant and a per-sample factor from the
  render context (`+0x27d00`), with rounding `0.5`. It also copies a flag
  byte from `+0x99`.

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

- The Tone render kernel as a ported law. Its process candidates are the
  vtable slots 23 to 25 above. Earlier guesses (`FUN_140cdc4d0`,
  `FUN_140cdcd10`, `FUN_140cdd640`) were by callee only and are superseded.
  `FUN_140cdb6f0` (3477 bytes) is the generic source render called from
  `0140870000.c:7620`, not a Tone-specific kernel.
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
