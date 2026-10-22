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
| `0140cd0000.c:5701` | Slot 18 `FUN_140cdab80`: crossfade frames from `+0x90`/`+0x94` and `+0x27d00` (Beat Machine) |
| `0140d10000.c:7372..7398` | Machine init `FUN_140d1ba90`: `Slice Atk` to `+0x90`, `Slice Rel` to `+0x94`, `Transient` to `+0x9c` |
| `0140960000.c:4300` | `FUN_140967c60`: sets the render rate `+0x27d00` (Hz; 44100 below 10) and `1000 / rate` |
| `01408e0000.c:3903` | Call with the 44100 default rate |

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

Status update: the render kernel (slot 23), its grain scheduler and the mark search are
ported in `crates/sampler-core/src/source/tone_machine.rs`; the source pass, mark table and
parameter setters are still open. See `k-tone-machine-kernel.md`.

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
  zeroed, then `+0x54 = 32` and `+0x58 = 600` (dwords), `+0x8/+0xc = -1`, `+0x4c = 1.0f`.
  Slot 18 overwrites `+0x54` and `+0x58` with the crossfade frame counts (below), so
  32 and 600 are only the frame values before the first slot-18 call.
- A separate object at `0140a70000.c:4607` stores the `BSlice` vtable at a
  different offset; the link between that object and the group is unproven.

### Stored controls (decompile, resolved this pass)

- The machine init `FUN_140d1ba90` (`0140d10000.c:7372..7398`) fills the control
  struct by name through `FUN_14087d160` (a getter of the control's stored value):
  `Slice Atk` goes to `+0x90`, `Slice Rel` to `+0x94` (both floats), `Transient` to
  `+0x9c` (int). The record order is the same as the control order, so the stored
  `slice_float_1` is **Slice Atk** and `slice_float_2` is **Slice Rel**, both in
  milliseconds (the Kontakt manual describes Slice Atk/Rel as the crossfade times
  between slices, p. 205). The same convention holds for Time Machine 1
  (`machine_float_1` = Smooth, `machine_float_2` = Grain), see `time_stretch_writes.rs`.
  Evidence: decompile plus that convention; no capture isolates the two values yet.
- `slice_flag_1` is copied to `+0x99` and then to `+0x50` by slot 18. No control name in
  the init list maps to `+0x99` (the init writes a u16 `1` at `+0x98`, so the byte is 0).
  Its UI meaning is **not recovered**.
- The render-context field `+0x27d00` is the **render sample rate in Hz** (float).
  Setter `FUN_140967c60` (`0140960000.c:4300`) stores its argument there (and at
  `+0x116c`), falls back to the constant 44100 when the value is below 10
  (`.rdata` 10.0 at `0x144714a00`; default 44100 at `0x144f38d58`, also passed from
  `01408e0000.c:3903`), and derives `1000 / rate` (ms per sample, a double at `+0x12158`).

### Crossfade law (slot 18, `FUN_140cdab80`, `0140cd0000.c:5701`)

- Inputs: source object `+0x90` (Slice Atk, ms), `+0x94` (Slice Rel, ms), `+0x99`
  (flag), render context `+0x27d00` (rate).
- Outputs on the slice object: `+0x54` = attack frames, `+0x58` = release frames,
  `+0x50` = flag, `+0x38` = a call result from the source player (not ported).
- `frames = (int)(ms * 0.001f * rate + 0.5f)`, with the `.rdata` constants
  `0x3a83126f` (0.001f) and `0x3f000000` (0.5f). The port's earlier names "slice start
  and end" were wrong: these are crossfade lengths.

### Slice sequencer (slots 12, 13) ported, not wired

`crates/sampler-core/src/source/beat_slices.rs` ports the sequencer and the slot-18
law (now named `attack` / `release` and `crossfade`):

- Slot 12 (reset): skips entries with kind below 3, then sets the loop count from the
  first active entry (`+0x18`; 0 means unbounded). `+0x88` is written only when the
  walk advances. Position: `+0x74 = (int)pos` when the flag at `+0x50` is clear, else
  `+0x6c = 0`.
- Slot 13 (settle): skips inactive entries from the current index; an entry of kind 4
  sets the loop count to 1.
- The table walk and the internal-trigger script (`0140870000.c:5446`, which uses
  `_num_slices`, `_slice_idx_loop_start`, `_slice_loop_count`, `_slice_length`, and
  `play_note(-1,-1,0,...)` with event parameters `1000 + zone` and `1000 + slice`)
  agree with the KSP manual names `num_slices_zone`, `zone_slice_idx_loop_start/end`,
  `zone_slice_loop_count`, `zone_slice_length` (KSP manual, Zone and Slice Functions).

Loader: the Kontakt loader reads `slice_float_1`, `slice_float_2` and `slice_flag_1` from the
mode-4 record into `ir::Group::beat_slices` (`crates/sampler-ir/src/lib.rs`, `BeatSlices`;
`crates/sampler-kontakt/src/library.rs`, `beat_slices_from_record`).

### Zone slice markers (chunk 0x35) and zone grid

A zone with a grid stores its slice boundaries in a structured child chunk `0x35` of the
zone (`vendor/ni-file/src/kontakt/objects/zone_slices.rs`, `ZoneSlices`). Public payload, from
the capture (grid fixed, 120 BPM, four 1/16 slices, 48 kHz):

- `u8` (0), `i32` selected (-1), `u32` count (5)
- `u32` grid length in frames (24000 = 4 x 6000); present only when count > 0 (inferred
  from the plain zone, count 0, payload 9 bytes)
- `count` records of `u32` frame, `f32` frame (equal up to rounding, meaning open), `u32`
  extra (8 on the first record, 0 after)

Frames: 0, 6000, 12000, 18000, 24000, so the four slices are consecutive pairs. The loader
stores them per zone in `ir::Playback::beat_grid` (`BeatGrid`: up to nine markers, the
native eight-slice table; `tempo_scale` from the zone private float at byte 12). A Beat Machine
zone without a grid, or with more than nine markers, is reported and plays as a sampler.
The layout is confirmed for one grid only.

### Zone playback rate (private float at byte 12)

The zone private block holds a float at byte offset 12: 0.25 in the grid-fixed zone and 1.0
in the plain zone (the same in the sampler-fixed file). It matches the decompiled zone
`tempoScaling` (`+0x7a8`, default `0x3f800000`, written at `0140860000.c:9276-9278` and
multiplied into the grid factor `FUN_14087d740`, `0140870000.c:9390`). The loader keeps it as
`tempo_scale`; lowering folds it into the zone transposition (12 * log2 of the rate), so the
voice reads the sample at that step. The grid factor's constants are not resolved, so the
rule "rate = stored value" is verified for this one grid only.

### Voice (lowering and playback) — implemented

`sampler-core` plays a Beat Machine zone as slices (`source/beat_slices.rs`, `Player`):

- The sequencer (`Sequencer`, slots 12/13/18) holds one active entry per slice. Each slice
  plays for its cell: its source length at the output rate. The next slice starts when the
  cell ends (slices in order, no loops, count 1). The read position jumps to the slice start.
- Each slice fades in over the attack and out over the release (`Sequencer::crossfade`, frames
  at the output rate). The fade shape is linear: the native shape is not recovered.
- Loops are ignored on Beat Machine zones (ponytail: a Beat zone with loops is not seen yet).
- Cursor reuse is disabled for beat cursors (`source/reuse.rs`).
- Tests: `beat_slices` (player order, cell length at 44.1 kHz, fades) and
  `source::tests::beat_zone_plays_grid_slices_in_order_at_the_stored_rate` (four synthetic
  tones, zero-crossing rate within one crossing per 0.1 s window).

### Verification against the captured wav (one session)

Reference: `beat_machine_v2.wav` and `beat_machine_4slices.wav` (note 60, velocity 100, 1/16 at
120 BPM, sample `beat_4slices_120bpm_48k_f32.wav`). Render: `sampler-native render-kontakt`
of the saved `beat_fixed.nki`, `--no-scripts`, note 60 for 0.8 s.

| Check | Oracle | Render | Tolerance | Result |
| --- | --- | --- | --- | --- |
| Slot rates (tone x 0.25), slices 1-3 (50, 80, 110 Hz) | 50.02, 79.98, 109.90 Hz | 49.94, 80.00, 109.96 Hz | 0.5 % | pass (render within 0.12 %) |
| Slot boundaries (onset at 0, cells 125 ms) | changes near 100, 240, 350 ms | same within one 20 ms step | 20 ms | pass |
| Slice 4 (560 Hz) | silent after 380 ms | 139.91 Hz, full level | — | fail (decay, below) |
| Level (peak, first 100 ms) | 0.136 RMS | 0.161 RMS | — | fail, 1.5 dB; the plain sampler render of the same zone is at our level, so the gap is in the shared level path |
| Amplitude over time | flat to ~100 ms, then falls to ~0 by 350 ms (tau about 0.1 s) | flat | — | fail, not modelled |

The source tones are flat (RMS 0.35 in each cell), and the NKI amplitude envelope has sustain
1.0, so neither explains the fall. Its cause is open.

### Second oracle session (retrigger, grid, BPM) — not conclusive

`tools/kontakt-oracle/probes/beat_machine_session2.lua` was run once (`session2_grid_retrigger.wav`
in the artifacts folder). It asked for three zones (1/16 at 120 BPM, 1/16 at 90 BPM, 1/8 at
100 BPM) and timed notes by KSP timer chains. Only the first note of the first zone sounded.
The later notes and the other two zones did not appear. The probe design cannot separate a
timer that does not re-arm from a silent second note, and standalone Kontakt sends MIDI to the
selected instrument only. The grid law (rate and slot spacing for other grids and BPMs) and the
second-trigger silence are therefore still open. A corrected probe would use one instrument with
three zones (keys 0-59, 60-84, 85-127, root keys 48, 72, 96) and MIDI timing from a file.

### Not recovered

- The grid factor constants behind the rate rule (other grids, auto grid, other BPM).
- The amplitude fall after about 100 ms in the oracle, and the 1.5 dB level gap.
- The second-trigger silence (session 2 did not test it).
- The meaning of `slice_flag_1` (`+0x99`) and of the float copy in the `0x35` records.
- The source player call from slot 18 (`+0x38`) and the fade shape beyond the linear attack
  and release.

## Native Tone vector (oracle, default parameters)

Captured once with `tools/kontakt-oracle/render.py` (Kontakt 8 under Wine). The
probe is in the same folder. Group playback mode `tone_machine` was accepted
(readback `tone_machine`); all Tone controls at defaults; input a 220 Hz sine at
0.5 amplitude; the note is triggered by the oracle's MIDI note.

- File: `/data/dev/workspace/PluginArtifacts/kontra-oracle/tone-machine/tone_default.wav`
  (float32, 2 channels, 48 kHz, 6.112 s; SHA-256
  `6d942a83f13cbd36e5fc0986f0bbee0c2ab51d8b5c8a974e3996de72e615486c`).
- Level: peak 0.192, RMS 0.0174 (the stimulus peak is 0.5, so the default path
  lowers level; the cause is not characterised). First non-zero frame 50062 (1.043 s).

This vector is the baseline for a Tone kernel port. No port exists yet, so no
comparison has been run. The slot-23 kernel's SIMD branch and its object
layout are still open.

## Native Beat Machine capture (oracle, default parameters)

Probe: `tools/kontakt-oracle/probes/beat_machine.lua`, run once with
`nice -n 19 tools/kontakt-oracle/render.py tools/kontakt-oracle/probes/beat_machine.lua OUT.wav 6`
(one Kontakt 8 session under Wine, Xvfb stopped afterwards; the DAW hosts were not touched).
The probe writes its own sample: 0.5 s mono float at 48 kHz, four 1/16-note slices at 120 BPM
(sines of 200, 320, 440, 560 Hz, 25/40/55/70 whole cycles per slice). The zone is set with
`set_zone_grid(inst, zone, "fixed", 120.0)` (Lua API; Fix mode, default width 1/16),
and group 0 with `set_group_playback_mode(inst, 0, "beat_machine")`. Readbacks: grid
`fixed` at 120.0, mode `beat_machine`, script accepted (`probe.log`).

- Files (`/data/dev/workspace/PluginArtifacts/kontra-oracle/beat-machine/`):
  `beat_machine_4slices.wav` (float32, stereo, 48 kHz, 9.1 s, L = R, SHA-256
  `10ccf8a679a228983a0fd4ee7a0143fe5a944f07df5750d5aa4340873dd36a2c`),
  `beat_machine_probe.lua`, `probe.log`.
- Trigger: the MIDI note render.py sends about 1 s into the recording starts a KSP timer.
  The KSP then plays note 60 at +0.1 s and again at +2.0 s. The first burst starts
  about 1.03 s into the recording.
- Measured (analytic signal, instantaneous frequency and amplitude):
  - Four slices in order. Frequencies about 50, 80, 110 and 140 Hz, which are the source
    frequencies 200, 320, 440 and 560 Hz times **0.25**. So every slice plays at rate 0.25.
  - Onsets about 1.03, 1.15, 1.28 and 1.41 s: spacing 0.125 s, which is one 1/16 at 120 BPM.
  - Each slice is audible for about 0.12 s, so only about a quarter of its source material is
    heard. The slice is cut at the slot end.
  - Silence after about 1.45 s.
  - Peak 0.192 for a 0.5 amplitude input (gain 0.384, -8.3 dB). The Tone capture above has
    the same peak; the cause is not characterised (same open question as the Tone note).
  - No second burst at +2.0 s. The probe's first note has no note-off (duration -1),
    so the second note may be ignored while held. Not explained.
- Candidate law (unverified, fits both numbers): rate = zone length / one host bar.
  The zone is 0.5 s (one beat at 120 BPM); one 4/4 bar at 120 BPM is 2.0 s, so 0.25.
  Slot spacing = bar / 16 = 0.125 s. The manual says the Beat Machine should be enabled
  with the Use Beat Machine button, not by switching the source mode, and that the Speed
  readout then shows a percentage. The probe switched the mode directly, so the speed
  state may differ from the normal path.
- Next single experiment (not run): change the grid BPM or host tempo and see whether the
  rate and the slot spacing move as the candidate predicts. Also enable the Beat Machine
  through the Sync / Slice path if the probe can drive it.

## Gap table

| Module | Shared engine covers | Missing |
| --- | --- | --- |
| Tone Machine | Mode mapping and stand-in granular engine, reported `NotModeled` | Render kernel, grain/formant/DC laws, record-to-object mapping, `hiQuality` |
| Beat Machine | Stored mode and controls decoded; zone grid (chunk 0x35, `Playback::beat_grid`) and rate (private float 12) parsed; zone played as sliced cells with the sequencer, linear fades, at the stored rate (one capture, rate within 0.2 %) | Oracle amplitude fall after about 100 ms and 1.5 dB level gap; second trigger; grid factor (other grids); `slice_flag_1` meaning; auto grid |

## Open items (for the next pass)

1. Find the vtable slot that runs for internal id 1 (the `BToneMachine::vftable`
   entries). Needs the vtable address from the Ghidra symbol table, not a
   binary session.
2. Confirm the stored-mode to internal-id map. The export groups internal id 4
   with the sampler family and uses id 5 for the Beat Machine rule, so stored
   Beat (4 in the repo) is not the identity map. Unresolved.
3. Oracle, Beat Machine (one corrected session): a single instrument with three zones
   (keys 0-59 root 48, 60-84 root 72, 85-127 root 96; grids 1/16 at 120, 1/16 at 90,
   1/8 at 100), notes sent by a MIDI file so each note-on and note-off is timed. This tests
   the rate rule on other grids, the retrigger and hold behaviour, and the second-trigger
   silence. Session 2 (KSP timers) did not produce them.
4. Oracle, amplitude: the fall after about 100 ms and the 1.5 dB level gap (compare the
   plain sampler render of the same zone, which is at the render level).
5. `slice_flag_1` (`+0x99`): find the control or the writer of that byte.
6. Grid factor constants behind the rate rule (`FUN_14087d740`), and the zone grid mode
   (Fix/Auto/none) from the zone private block.
