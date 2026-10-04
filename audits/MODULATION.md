# Group modulation, envelopes and zone fields

Decoded from every locally installed preset (781 files, 787 programs, 225,795 groups,
13.3 M zones) on 2026-09-29. The corpus came from Kontakt 5 to 7 (Solo, Vista, Pacific,
Areia, Dolce, CHORUS, Afflatus, Una Corda). No preset bytes, keys or sample data are
committed. Confidence levels:

- **high**: exact parse of every record plus an unambiguous semantic check
- **medium**: consistent across the corpus, but the meaning comes from names or ranges
- **low / unknown**: stored raw as `unknown_*`

Readers: `vendor/ni-file/src/kontakt/objects/{modulation,internal_mod,envelope}.rs`.
Engine view: `src/modulation.rs`. Inspect one preset with `kontakto inspect-mods <nki>`.

## Container layout (high)

Each group (0x04, v0x95) has the children `0x3B, 0x3C, 0x38, 0x4A`, in that order, in all
225,795 groups.

- `0x3B` (16 slots) and `0x3C` (32 slots) are parameter arrays (v0x10/0x12). The public
  data is one presence byte per slot, followed by the chunk when the byte is 1.
- `0x3B` slot → `0x0D` BParInternalMod (v0x80/0x81) → child `0x07` BParEnv (v0x90,
  public `u32` kind: 0 = AHDSR, 1 = flex) → child `0x3F` AHDSR (all groups) or `0x40`
  flex envelope (6,380 modulators in the 778 NKIs, decoded below).
- `0x3C` slot → `0x0C` BParExternalMod (v0x100–0x102).
- In both 0x0D and 0x0C, the fields are in the StructuredObject **private** data, and
  the public data is empty.

## Modulation target list (shared by 0x0D and 0x0C)

Every one of the 1,319,478 external and 229,184 internal records parses to the exact
byte length with this grammar:

```
u32 target_count (1 in every external record; 1–2 internal)
per target:
  str8  param          "volume" | "pitch" | "playPos" | "eqGain1" | "filterCutoff" | "ahdsr_attack" ...
  f32   intensity
  i16   unknown        (-1 in every record)
  u8    flags          (0x02 negative target depth; other bits retained)
  u16   lag            (0, 40, 120, 128, 250, 500)
  str8  target name    KSP find_target name; "<none>" or "" when unnamed
  u8    slot           ONLY when param is not volume/pitch/playPos
  u8    invert         0/1
per target, afterwards:
  u8    shaper kind    0 none | 1 table | 2 breakpoints
  kind 1: u8 enabled, 128 × f32
  kind 2: u8 enabled, u8 n, n × (f32 x, f32 y, f32 curve)
```

str8 is a `u32` length followed by bytes.

| Field | Confidence | Evidence |
|---|---|---|
| param | high | Readable ids. Only `volume`, `pitch` and `playPos` omit the slot byte. The rule is inferred from the parameter name, and every record parses exactly. |
| intensity | high (storage), medium (scale) | Stored as a magnitude with negative direction in target flag 0x02. `pitch` from pitch bend is 0.1667 in 214k records (0.1599 in 168), and 2/12 matches the usual ±2 semitone bend. `Group::pitch_bend_range()` therefore uses signed intensity × 12. |
| lag_ms | medium | The Kontakt manual gives Lag in ms. Values are round numbers: 250 on PB_PITCH, 500/120/128 on CC volume. |
| slot | medium | eqGain\* uses slots 0/2/4, filterCutoff 0/2/3/5 and ahdsr_attack/release 0/1. These match FX-chain positions and internal-modulator slots. |
| invert | medium | Always 0/1. It is set on 63,805 volume assignments. Its order relative to the shaper has not been verified. |
| shaper kind/data | high | Table = 128 floats; breakpoints go from x = 0 to 1. Only kinds 0/1/2 occur. |
| shaper enabled | medium | 2,552 breakpoint and 1,379 table shapers have enabled = 0. Nearly all of these are identity curves (the default); 12 are non-identity disabled curves. Only enabled shapers reach `ModAssignment::shaper`. |
| breakpoint curve | low | Values lie in -1..1 and are probably segment curvature. `ShaperCurve::evaluate` interpolates linearly and ignores them. |

## External modulation (0x0C) tail (after the target list)

```
str8  name                KSP find_mod name (VEL_VOLUME, PB_PITCH, CC_VOLUME ...)
u32   category            1 = source follows | 2 = no source (54,654 records, Areia/Solo)
category 1: u32 source    (+ payload, see below)
            [u8;4] unknown  (00000000, 7f000000, ffff0001 ...)
category 2: [u8;2] unknown
u32   unknown id
```

Source codes follow the source order in the Kontakt manual, counted from 1. The names
and payloads agree with that order:

| Code | Source | Payload | Evidence (name → code, records) |
|---|---|---|---|
| 1 | Pitch bend | – | PB_PITCH 213k, PB_ATTACK |
| 2 | Poly aftertouch | assumed none | not present |
| 3 | Mono aftertouch | assumed none | not present |
| 4 | MIDI CC | u8 CC number | CC_VOLUME = 11/7/110/111; CC_EQ_* = 1 (mod wheel) |
| 5 | Key position | – | KP_PITCH, BALANCE_*HIGHREG* |
| 6 | Velocity | – | VEL_VOLUME, VEL_EQ_*, VEL_ATTACK |
| 7 | Release velocity | assumed none | not present |
| 8 | Release trigger counter | – | RTC_VOLUME/PITCH/ATTACK, 2,648 records (played, see "Release-trigger counter") |
| 9 | Constant | – | CV_PITCH |
| 10/11 | Random uni/bipolar | assumed none | not present |
| 12 | From script | u32 (1/3/4, likely script slot) | KSP_PITCH, script-driven CC_VOLUME |

Confidence is high for codes 1, 4, 6 and 8, and medium for 5, 9 and 12. Payloads for
absent codes are assumptions. The exact-length check turns a wrong assumption into an
error, never a silent misread.

## Internal modulator (0x0D) tail

```
[u8;4] unknown flags   00000100 | 00000101 | 01000100 | 01000101 (byte 0 may be bypass: unverified)
u32   unknown id
str8  name            ENV_AHDSR | ENV_FLEX
u32   category        2 in every record
```

The fourth flag byte, the target flag bit 0x04 and the four trailing AHDSR booleans are
set together (21,622 records, Afflatus). Their meaning is unknown.

### Internal modulator census (high)

Every one of the 229,184 internal modulators in the 778 NKIs is an envelope:

| Modulator | Targets | Count | Libraries |
|---|---|---|---|
| ENV_AHDSR | volume | 220,758 | all |
| ENV_FLEX | volume | 6,380 | Dolce 2,856, Afflatus 2,192, Pacific 580, CHORUS 504, Vista 248 |
| ENV_AHDSR | filterCutoff | 563 | Dolce, Vista, Pacific, Una Corda |
| ENV_AHDSR | eqGain2 + eqGain3 | 556 | Dolce, Vista, Pacific |

This older corpus contained no LFOs or other source kinds. The later ANALOG STRINGS
audit below supersedes that observation: direct `0x08` LFO children import as opaque
sources without dropping their group's other modulation. No internal modulator in
this older corpus targets pitch or pan. Every internal volume target stores intensity 1, no invert and
no shaper (Afflatus, Vista, Solo checked), so intensity is not modelled for envelopes.

Two volume envelopes in one group occur in 2,332 groups: Afflatus (2,192, AHDSR then
flex, all `BRASS_Dyn*_Leg_*` legato-transition groups whose flex rises in 20 ms and
falls to 0 over 680 ms) and Pacific (140, either order). Playback multiplies the first
volume AHDSR and the first volume flex envelope; the Afflatus shape (a transition layer
that fades out while held) is what that product gives.

## AHDSR envelope (0x3F, unstructured, v0x11, 77 bytes)

`f32 attack_curve, f32 attack_ms, f32 decay_ms, f32 hold_ms, f32 release_ms, f32 sustain,
u8 unknown, 4 × (f32, f32, f32, u8) unknown`

| Field | Confidence | Evidence |
|---|---|---|
| attack_curve | high | Always -1..1. Values include ±0.33, 0.4 and 1.0. |
| attack_ms | high | Non-negative ms. Examples: 80–320 for strings, 20–40 for brass, 2500 for a pad. |
| sustain | high | 0..=1: 1.0 for volume, 0.25/0 for filter and EQ envelopes. |
| decay_ms | medium | Maximum 25000.04, the same ceiling as release. It is 25000 wherever sustain = 1 (left at maximum), and 104–213 ms on filter envelopes with sustain 0. |
| hold_ms | medium | Maximum 15000.02, a different ceiling from decay and release. Usually 0. |
| release_ms | medium | 120 ms (Dolce shorts), 350 ms (Dolce pizzicato), 376/500 ms (Areia legato), 850 ms (Pacific legato), 25000 (scripted-release patches). |

The decay/hold/release assignment rests on shared range ceilings and on musically
consistent release times. It has not been checked against Kontakt's UI.

Attack curve (engine): the attack glides `(1 - e^(-k t)) / (1 - e^(-k))` with
`k = 5 * curve`, linear at 0. The sign follows the Kontakt manual (positive convex,
negative concave), confidence high; the steepness 5 is a guess, confidence low.
`$ENGINE_PAR_ATK_CURVE` decodes as `2x - 1`, confidence high: Solo's scripts set
1000000, 750000 and 333333 where its presets store 1, 0.5 and -0.33. Solo stores
curve 1.0 on 16,156 of its 20,590 volume envelopes (125 ms attacks), so the curve is
audible there: a Solo Violin note is 7-12 dB louder over its first 10-40 ms than with
the linear attack, and the Solo Pads (-0.33, 2.5 s attack) 5-8 dB softer over its
first second.

## Flex envelope (0x40, unstructured, v0x11/v0x12)

```
u8 0, u16 version
u32 last          index of the last point (2, 3 or 4 locally)
u32 unknown       sustain - 1 in every record (loop start?)
u32 sustain       index of the point held while the key is down (last - 1 everywhere)
(last + 1) × (f32 time_ms, f32 level, f32 curve)
u16 unknown       v0x12 only (504 CHORUS records), always 1
f32 × 3 unknown   (-1, 0, 1) in every record
u8  unknown       0 or 1
```

All 6,380 records parse to the exact length.

| Field | Confidence | Evidence |
|---|---|---|
| layout, point count | high | exact length for all 6,380 records at every point count |
| time_ms | medium | 1-2000 ms; a delta from the previous point, not absolute: absolute readings would go backwards (140, 144, 664, 184), and deltas such as 438.687 + 185.313 sum to whole milliseconds |
| level | medium | only 0, 0.986 and 1; read as linear gain. The last point is 0 everywhere. Attack layers go 1, 1, 0 (fade out while held), sustain layers 0, 1, 1, 0 (delayed fade-in) |
| sustain | medium | always a point before the last, so every envelope has a release segment |
| curve | low | 0.05-0.999, 0.5 = linear. Read as the segment's bulge: above 0.5 bulges above the straight line (fast rise, slow fall), so rising segments at 0.57 are slightly convex and releases at 0.37 fall fast at first. Engine curve = `±(2c - 1)`, signed by the segment's direction, through the attack's glide law |
| unknown index, tail byte | - | if they were a loop enable and start, Vista's attack layers would re-trigger while held, and identical layers of different mic groups differ in the byte; playback ignores both |

Playback: the envelope starts at 0, glides to each point in turn, holds at the
sustain point until release, then glides from its current level through the remaining
points (a release before the sustain point jumps straight there) and ends the voice
after the last. A flex envelope with no AHDSR runs against a unity AHDSR (not the
engine's default attack/release). Vista's main sustain groups carry only a flex
envelope: their notes now start with the stored 72-140 ms attacks (attack layers) and
200-300 ms delayed swells (sustain layers) instead of instantly
(Vista 3 Cellos: -20 dB at 30 ms instead of 0 ms, -6 dB at 140 ms instead of 30 ms).

## Group fields

| Field | Finding |
|---|---|
| volume / tune | **Linear ratios.** Group volume clusters at 1.0, 0.5, 0.7071, 1.4142 (-6/-3/+3 dB). Group tune is 1.0, 0.5 or 0.25 (octaves); zone tune is 0.98–1.02; zone volume is 0.891/1.122 (±1 dB). The ni-file "semitones" comment was wrong and is fixed. The importer's multiplication is correct. |
| interp_quality | 0 in every group. Exposed raw. |
| voice_group_index | -1 = none. Exposed as `Option<u32>`. |
| 0x4A BParGroupDynamics | Unstructured v0x10, 256 bytes, **all zero in every group**. Undecodable from this corpus. Not exposed. |
| BParSrcMode 0x0E | **Not found** as a chunk anywhere. The group private data (1,722–2,121 bytes) contains 136 `(u32 8, u32 flags, u32 0)` records and a trailer with a byte that is 3 in Vista and 4 in the other libraries. That byte could be the source module, but nothing confirms it, so it is not exposed. |

## Zone fields (public data, v0x98–0x9a)

| Field | Finding | Confidence |
|---|---|---|
| 4 × i16 fades | Order: low velocity, high velocity, low key, high key (ni-file naming). Only the first is non-zero in practice: 79,729 zones (Afflatus), widths 4–21, always on zones whose low velocity is above 0 (for example 110–127 with fade 5). One zone has low-key fade 1. | medium (first field high) |
| sample_start_mod_range | Frames. It is -1 in 377k zones (the only negative value), which is exposed as `None`. Otherwise it is 0 or a frame count (for example 31,340 frames in the Vista Harp). The `playPos` target is read as the sample-start modulation driven by this range. | medium |
| 6 bytes (v ≥ 0x9a) | `00 01 ff ff ff ff` in all 13.2 M zones. Unknown and skipped. | – |
| loop mode | Only mode 1 occurs (2.6 M loops), with count 0 and tune 1.0; 84 loops set the alternate flag. The importer's "mode 2 = until release" is **unverified** because no local preset uses it. | low |

## Engine-facing API (`src/modulation.rs`, re-exported from `import`)

- `Group.volume_env: Option<Ahdsr>`: the first internal AHDSR whose targets include
  `volume`. `Group.flex_env: Option<FlexEnvelope>`: the first flex envelope on
  `volume`. Groups with further internal modulators produce a warning.
- `Group.mods: Vec<ModAssignment>`: one entry per external target. It holds `source`
  (`ModSource`), `target` (`ModTarget::{Volume, Pitch, SampleStart, Attack, Release,
  Module{param, slot}}`; `Attack`/`Release` are `ahdsr_attack`/`ahdsr_release` on the
  volume AHDSR's internal-modulator slot),
  `intensity`, `invert`, `lag_ms` and `shaper` (only when enabled), with `shape(x)`.
- `Group::modulation(source, &target)`, `velocity_to_volume()` (largest stored intensity,
  0.0 = velocity-insensitive), `pitch_bend_range()` (semitones, `None` = no pitch bend) and
  `cc_volume()` (first CC → volume).
- Many libraries (Areia, Dolce, Vista, Pacific) set these intensities to 0 and drive them
  from scripts at runtime. Stored values reflect the saved state, not what plays.

- `Group.modulators: Vec<Modulator>`: `find_mod` order (internal modulators in slot
  order, then external ones), with target names for `find_target`, the index of their
  first `Group.mods` entry (external only) and whether it is the imported volume AHDSR.

## Runtime modulation and engine parameters (`src/engine/params.rs`)

Each group's assignments become a `ModTable` at bank load: per-entry route, intensity,
lag and a 128-point shaper table, plus the list of voice-rate entries. Per voice and
block the engine reads each source (0..1), shapes it, applies a one-pole lag
(`1 - exp(-n / (lag_ms * rate))`, stepped per block) and combines:

| Target | Law | Confidence |
|---|---|---|
| Volume | multiplicative: each entry scales by `1 - |i| * (1 - v)`; negative `i` uses `1 - v` | medium: matches stored crossfade shapers (layers sum to about 1), unverified against Kontakt |
| Pitch | `12 * i * v` semitones, bend mapped to -1..1 | high for the 2-semitone stored range (i = 0.1667) |
| Sample start | `i * v * zone.start_mod` frames, fixed at note start | medium |
| Attack, Release (volume AHDSR) | time scaled by the volume law `1 - |i| * (1 - v)`, fixed at note start | low (see below) |

Sources: velocity, key, constant, MIDI CC (live, per channel), pitch bend, channel
pressure, release-trigger counter (see below). Not applied: Script, random, release velocity, poly aftertouch,
`Unassigned`, and the remaining `Module` targets (effect parameters); no pan target
exists in the local corpus. Velocity → volume always goes through the shaper.

Envelope-time targets: 953 external assignments drive `ahdsr_attack` (VEL, PB, RTC,
KP) or `ahdsr_release` (PB), all in Pacific, CHORUS, Vista and Una Corda. The `slot` is
the internal-modulator slot (high: Pacific groups ordered FLEX, AHDSR address the
AHDSR as slot 1, groups with the AHDSR alone as slot 0). Their shapers map into
0.59..1 (Pacific VEL_ATTACK: velocity 0 → 1, 127 → 0.59; PB_ATTACK release: 0.81 at
rest, 1 at full bend), which reads naturally as a time factor, so they scale the
stored time. Whether Kontakt scales the time or the normalized knob value is
unverified. Solo has none; Vista's are RTC-driven, so only the release-trigger
counter exercises them there.

The decoded `invert` flag stays **ignored**. Vista Full Strings Sustains is decisive:
its four CC100 crossfade layers (`susdyn1`-`4`) carry bell shapers peaking at
CC 0.26, 0.44, 0.78 and a ramp from 0.56, so they play upright and sum to about 1;
the `cl` (close) and `dc` mic copies of layer 3 store the identical shaper, one with
invert set and one without, and layer 1's `BALANCE_COMP` shaper likewise differs only
in the flag between siblings. No reading of the flag (after the shaper, before it,
or as a sign on intensity) makes both copies play the same, so it cannot be Kontakt's
volume inversion as stored. Afflatus sets it on rising velocity/CC shapers, Areia and
Solo almost never. Applying it silences or inverts layers that clearly play upright.
The legacy observer cache holds GM reset defaults before input (CC7 127, CC10 64,
CC11 127, rest0); default-less authored routes retain that behavior. Imported
native CC routes use their saved substitute/availability gate as documented below.
The
controllers scripts set while loading (Areia sets CC110-113 in
`on persistence_changed`) are applied on install and after resets.

`set_engine_par`/`get_engine_par` (queued with notes, applied at their frame, gain
changes ramped over the block by the voice gain ramp):

| Parameter | Address | Value law | Evidence |
|---|---|---|---|
| VOLUME | group, instrument (`-1`), bus (`generic 1000+b`) | gain = 3.9810717 * x^3 (x = v/1e6) | 630859 = 0 dB (0.005 dB off), Areia SENDLEVEL 396820 = stored 0.25 |
| PAN | same | 2x - 1 | NI docs |
| TUNE | group, instrument | ±36 semitones linear | NI docs, unverified |
| OUTPUT_CHANNEL | group | `-1` instrument, 1000+b bus b | Areia init |
| ATTACK, HOLD | group, volume-AHDSR slot | ms = 2 * ((1 + 7500.01)^x - 1) | Areia 465229 = stored 125.013 ms |
| ATK_CURVE | same | 2x - 1 | Solo 750000/333333 where presets store 0.5/-0.33 |
| DECAY, RELEASE | same | ms = 2 * ((1 + 12500.02)^x - 1) | 512668 = 250.001 ms, 1e6 = 25000.04 ms |
| SUSTAIN | same | linear | low |
| MOD_TARGET_INTENSITY | group, `find_mod` slot, target (generic, -1 = 0) | i = x^2 | Areia 704316 where presets store 0.4961 |
| MOD_TARGET_MP_INTENSITY | same | 2x - 1 | NI docs |
| EFFECT_BYPASS, SEND_EFFECT_BYPASS | FX slot (`group -1`, generic 0/1/2 or bus) | v != 0 | - |
| SEND/INSERT_EFFECT_OUTPUT_GAIN, SEND_EFFECT_DRY_LEVEL, SENDLEVEL_0..7 | same | volume law | Areia |

`find_mod`/`find_target` resolve by decoded names; indices are stable positions in
`Group.modulators` and its target list. Everything else (effect-specific
parameters, INTMOD_*, LFO_*) is stored by the KSP runtime, read back unchanged, and listed by
`ksp-run`. On init, writes go to a setup engine that answers reads and records them
(`Runtime::init_engine_pars`); the playing engine replays them when the scripts,
bank or effects are installed. Up to 4096 writes queue per render (Areia bursts reach
about 320 per note); overflow is counted as dropped commands.

## Release-trigger counter (RTC_VOLUME / RTC_PITCH / RTC_ATTACK)

Kontakt 5.6.8 manual, Source module (p. 222): "T (Time, only visible if Release
Trigger is activated): If you set this to a value other than 0, KONTAKT will count
from that value backwards in millisecond intervals when it receives a note, then stop
the timer and provide its current value as a modulation source when it receives the
corresponding note-off value. This way, you can make your Instrument respond to note
durations, for instance by reducing the volume of your release sample after longer
notes in order to make it fit a Sample with a natural decay." Modulation chapter:
"RLS Trig. Count: This value is generated for Groups that are being triggered on
release and indicates the time between the trigger and the release signal." KSP
reference: "reset_rls_trig_counter(<note>): Resets the release trigger counter (used
by the release trigger system script)"; no `$ENGINE_PAR` or `$EVENT_PAR` addresses
the counter or `T`.

`T` is the group's `rls_trig_counter` field (imported as `Group::release_counter_ms`).
Corpus (every local NKI, aggregate only):

- 2,648 RTC assignments (external source code 8) in 1,454 groups, all release-trigger
  groups with `T` != 0: CHORUS 756, Dolce 936, Pacific 836, Vista 120. No group with
  RTC has `T` = 0, and no non-release group has `T` != 0. `T` is 750 ms (1,016
  groups), 1,200 (80), 1,500 (328), 2,000 (18) or 4,000 (12); 2,604 more release
  groups keep a `T` without any RTC assignment.
- Targets: volume 868, `playPos` 1,414 (named `RTC_PITCH` in every case: the name is
  the preset author's, the target is sample start), `ahdsr_attack` 366. None targets
  pitch.
- Every one carries an enabled breakpoint shaper; lag is 0 in all. Intensity is 1 for
  most volume/attack entries (0 in 47 volume, 92 attack, 2 start entries) and
  0.16-0.71 for sample start. Invert is set on 855 of them (301 of 366 attack).

Mapping: the source value is the counter's remaining share,
`x = clamp((T - held_ms) / T, 0, 1)`, fixed when the release voice starts, where
`held_ms` runs from the key's note-on (or the last `reset_rls_trig_counter` for that
note) to its note-off. A sustain pedal defers the release sample but not the stop:
the counter stops at the key release, as the manual says. `x` then goes through the
shaper and the existing laws for volume, sample start and attack time. A key never
pressed (script-generated notes) reads `held_ms` = 0, so `x` = 1.

Why this reading (medium confidence):
- Direction: the manual counts down and reports the current value, and its own
  example (quieter release after longer notes) is what identity shaping gives with
  `x` = remaining share: a note held `T` or longer reads 0, which the volume law turns
  into `1 - |i|`.
- Scale relative to `T`: the stored shapers vary over the whole 0..1 range for every
  `T` (CHORUS `T` = 750 volume: 0.797 at x = 0, 0.79-0.82 at 0.5, 0.90 at 0.75, 1 at
  1), which a fixed millisecond scale would leave mostly unused for short `T`.
- Shaper shapes then read naturally: all 1,414 sample-start shapers fall toward
  x = 1 (most from 1 to 0), so a note released at once plays its release sample from
  (or nearest) the start and one held past `T` skips furthest into it, up to
  `i * start_mod`: the longer the note, the less of the release's onset replays.
  Volume shapers rise
  toward x = 1 in 468 entries (CHORUS, most Dolce and Pacific `T` = 750: releases
  after long notes about 2 dB quieter) and fall in 348 (Pacific `T` = 1,500: releases
  after short notes quieter).

Level effect (library audit, `audit-patch`, 0.7 s notes): applying the counter took
Pacific 16 Violins Legato Sustains from -34.6 to -38.5 dBFS, Dolce 2nd Violins -34.7 to
-36.4, CHORUS -8.6 to -9.2, Vista 5 Violins -32.9 to -33.1; with the counter source
dropped each returns to its old level (Dolce -34.8). In Pacific it is all sample start:
its release samples open with about 0.5 s of sustain at -33 dB (above the held note's
-38 dB) before the release decays, and the counter's RTC_PITCH (intensity 0.50, shaper
1 up to x = 0.44, 0.42 at x = 1) skips 0.55 of the 47,130-frame start range there.
Forcing that offset back to the old 2,780 frames gives -34.5. The old level replayed
the pre-roll after every note-off. Held 0.7 s of `T` = 1,500, both counter directions
give the same skip (shaper 0.98 against 1.0), so this level does not hinge on the
unverified direction.

Unverified in the original corpus audit: that the source is normalised by `T`
(rather than by a fixed range), the direction (no Kontakt render to compare),
millisecond quantisation of the counter,
and the invert flag, which stays ignored as for every other source. The counter is
historically kept per MIDI channel and key; `reset_rls_trig_counter` reset the
key on the script's channel. At that audit revision, `NO_SYS_SCRIPT_RLS_TRIG`
was not modelled, so key-down reset occurred even when the system release script
was bypassed. Current production handles that condition for native release
generation and adds exact-owner and
surviving-voice clocks; the native overlap correction below qualifies their
semantics. Tests recorded by the original audit:
`release_trigger_counter_scales_release_volume_by_held_time` and
`reset_rls_trig_counter_restarts_the_count` in `tests/playback.rs`, and
`release_counter_moves_the_release_sample_start` in `src/engine/params.rs`.

Bounded native correction (Kontakt 8.13.1, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`): the
reset builtin at `0x14079aa6e` validates note0..127 and calls `0x140957600`.
That helper writes a float at native object `+0x2d210 + note*4`; the supplied
context/channel argument is unused. Sample admission reads the same key cell at
`0x14084f85e`, using note getter `0x14075c940`, which also supplies sample-voice
pitch. The ordinary branch of the final Group-system note prelude emits
`_reset_rls_trig_counter($EVENT_NOTE)`; private native release notes skip that
reset. This establishes native key sharing in overlapping-note cases. KONTRA's
Compatibility exact-owner and surviving-voice clocks retain stronger event identity
and therefore differ from that native overlap policy. The anonymous no-attack fallback
must not be replaced by an Event-onset snapshot under a claim of native parity.

The scalar timing law is now bounded by the reset and processing accesses. Let
`F32` denote a single-precision rounding step, `R` the native float sample rate,
`q` the current event's first dword, and `N` the processing span's frame count:

```
reset(key): age[key] = F32(F32(F32(q >> 10) * -1000.0f) / R)
advance:   delta = F32((double(N) * 1000.0) * (1.0 / double(R)))
           age[key] = F32(age[key] + delta)     // all 128 keys
source:    x = F32(max(F32(F32(T) - age[key]), 0.0f) / F32(T))
```

The reset discards the ten low timestamp bits, giving an integer sample offset,
then retains fractional milliseconds as float; this is not an integer-millisecond
counter. The offset is relative to the current processing span, not an absolute
onset clock. At `0x1409068c0..8f1`, the wrapper refreshes its double reciprocal
`+0x2d9e0` from sample rate `+0x27d00`; `0x140906e6b` loads double1000.
At `0x14090742a..4eb`, it starts a pointer at object `+0x2c00c` and adds the
rounded delta to vectors at pointer `+0x1204`: precisely table `+0x2d210`.
Eight iterations advance sixteen keys each. The table continues advancing after
key-up; the admitted release source receives a scalar snapshot, rather than
stopping the table's shared key cell. Float accumulation depends on processing
span lengths; replacing it with an absolute integer Event clock would also
change its numerical law.

The source read at `0x14084f819..8a1` uses positive group `T` at `+0x678`,
subtracts the shared age, clamps only the lower bound, divides by `T`, and calls
`0x140cebb70`, which stores the scalar at source object `+0x1b8`. There is no
note-off-offset addition in that read. Its bounded caller chain is
`0x14094fe20` → `0x1408f4520` (call `0x140950c36`) → `0x14084f380`
(call `0x1408f4d84`). The top wrapper `0x140d38b13..d41` processes queued events
with `0x14094fe20` before invoking processing wrapper `0x140906000`, where the
table advances. It splits spans greater than1024 frames into bounded chunks.
Native production now owns these 128 key cells separately from Compatibility's
existing exact-owner, voice and held-key clocks. It uses the existing
`begin_audio_block` host scope and balanced chunks of at most1024 frames;
MIDI, timer and MAX_BLOCK subdivisions do not advance the cells independently.
Only Native adds these chunk boundaries to DSP scheduling. An ordinary final
system admission resets its final key before zone selection, including a
no-zone match; a private native release admission reads the shared key then,
including after a wait or pedal deferral. Explicit reset remains key-global
when automatic system reset is bypassed.

The local cells initialize deterministically to zero. The bounded native
constructor scan did not establish an allocator-independent initial table
value, so parity claims apply to keys seeded by ordinary admission or explicit
reset. Reset/input replacement closes an unfinished host window using only DSP
already rendered and retains the key cells. Native RTC with nonpositive `T`
retains the admission source's initialized unity value; Compatibility retains
its historical zero result. Negative ages can admit raw scalars greater than
one, which the owner preserves. KONTRA's existing normalized modulation shaper
still clamps that input to0..1: downstream native extrapolation/target behavior
is outside this scalar proof, so the PCM proof uses scalars within0..1 and
checks the greater-than-one raw scalar separately. The older medium-confidence
corpus arithmetic above is not a complete native render-equivalence proof.

The NI [Source/Sampler manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view)
describes corresponding note-off timing, while [KSP Advanced Concepts](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts)
specifies the key argument to reset; neither documents overlap sharing. No
anonymous Event-clock repair was made for this finding.

Level effect (`audits/LIBRARIES.md`, d9e3ae5 -> b9bf445, bisected to a270c54): Pacific
-34.6 -> -38.5 dB, Dolce -34.7 -> -36.4, CHORUS -8.6 -> -9.2, Vista -32.9 -> -33.1.
With the counter route dropped at HEAD they return to -34.6, -34.8, -8.6 and -32.9.
Pacific carries no RTC volume: its release groups (`reldyn1..4`, `T` = 1,500) start
from `0.5 * shape(x)` of a 47,130-frame start range, and that shaper stays in
0.42..1. Every counter reading therefore skips 10k..24k frames, plus about 4k from PB
and constant. Before a270c54 the route was dropped, so releases started about 4k
frames in and replayed about 0.5 s of onset that is louder than the sustains (-33 dB
per 100 ms against -38). No counter reading can reach that level, so the lower level
is a correction. For the audit's 700 ms and 1,000 ms notes, both directions give
shape >= 0.93 (about 26k frames).

## Corpus run (`kontakto inspect-mods` over every local NKI)

All 778 NKIs import with no modulation errors. Stored pitch-bend range is 2 semitones
in every group that has one (1.92 in 168 Dolce groups). `cc_volume` is CC 111/110
(Areia, Solo, Dolce, CHORUS), 11 (Afflatus, Pacific) or 100/101 (Vista, Pacific,
CHORUS). Most groups have a volume AHDSR; most of the ones without it (Vista, some
Dolce, CHORUS and Pacific groups) have a volume flex envelope instead.

## Internal pitch AHDSRs (2026-10-02)

The older envelope census above predates the ANALOG STRINGS library. Its actual
instrument contains 480 `Pitch_Envelope` AHDSRs plus three `ENV_AHDSR` pitch
AHDSRs. Their slotless `pitch` targets previously disappeared during import because
module-envelope targets required an FX slot. Import now retains these targets in
original order. Playback reuses the existing AHDSR DSP and adds `12 * depth * level`
semitones to each voice's existing resampler pitch, including target shaping and
the direction convention already used for internal filter envelopes. KSP target
intensity and envelope-stage addresses preserve the original modulator/target
indices; target depth changes reach active voices, stage settings reach new voices.

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
describes internal envelopes as modulation sources and AHDSR stage behavior. This
implementation uses the existing engine's per-block pitch control rate; sample
accurate pitch curves and Kontakt reference-render parity have not been verified.
Every one of Kontakt's sixteen internal-modulator slots fits in the preallocated
voice workspace; no pitch envelope is truncated. Artificial groups exceeding the
native slot count fail bank preparation explicitly. No allocation occurs when processing these envelopes.
LFO objects remain opaque, and the ambiguous external invert byte remains ignored.
Mixed volume/pitch envelopes and flexible pitch envelopes remain unsupported.

The explicit KSP `INTMOD_BYPASS` button now removes pitch modulation while its
AHDSR clock continues. Preset mode/bypass flag bytes remain undecoded. The original
NI-authored [Kontakt Script Language Manual](https://www.danielrdehaan.com/attachments/kontakt_script_language.pdf)
(pp. 64-65 and 87) documents internal modulator/assignment addressing and the bypass
button; that PDF is a verbatim historical manual hosted by a third party.

Legacy `INTMOD_INTENSITY` now routes internal AHDSR pitch targets using the inferred
bipolar cubic depth `((2 * value / 1000000) - 1)^3`, with cube-root readback. This
matches the [original developer's published pitch measurements](https://community.native-instruments.com/discussion/60/how-to-approaching-modulation-in-ksp):
1129961 gives 24 semitones, 1221125 gives 36, 1293701 gives 48, and 2000000 gives
324. Those points use the existing 12-semitone/unit pitch scale. The law is inferred
from primary experimental data and has not been calibrated against Kontakt renders;
legacy volume and other filter target scaling remains unsupported. The same
cubic law is also corroborated for legacy AHDSR filter-cutoff depth. Modern
`MOD_TARGET_MP_INTENSITY` uses cubic depth for independently verified pitch and
filter-cutoff targets; other signed target laws remain unchanged. Finite extended
pitch depths survive writes and readback instead of being clamped to one octave.

For modern cutoff, Conflux's base saved `$Edit__Mods__ModEnv__Cutoff` is 507160.
Its selected sound is group 2; internal source slot 1 is `MOD ENV`, and original
target 1 is `MOD ENV -> CUTOFF`, parameter `filterCutoff`, filter module slot 0.
The independently stored depth is 0.000002936503. The authored control callback
passes its raw knob value directly to `MOD_TARGET_MP_INTENSITY` for that named
target. Cubing the centered value 0.01432 reproduces the native magnitude;
the former linear conversion made it about 4,877 times larger. Cube-root readback
returns 507160. This corroborates the
[original developer's published cubic measurement](https://vi-control.net/community/threads/the-formula-for-getting-the-ksp-modulator-intensity-value-from-desired-intensity-percentage.86187/page-2)
(post 34, edited May 2025), without claiming Kontakt render calibration.
Both internal and external cutoff depth addresses use this law and retain their
normalized limits. Pitch alone permits extended modern depths. Unipolar depth,
other signed targets, source flags and shapers retain their
existing behavior. The three inspected Conflux snapshots are script-only and
provide no independent native target-depth records; their changed UI amounts
are not treated as matched saved-depth evidence.

### Saved target direction and signed intensity aliases

Independent native producer inspection establishes the target sign separately
from the serialized `invert` control. The signed setter at `1408cb735` converts
depth, writes its absolute magnitude through parameter 718, then writes the
negative comparison through parameter 1056. That parameter's setter at
`1408c3834` sets or clears bit 1 of the original target's flag byte. The shared
target reader `140d09d50` and writer `140d16040` identify the same byte decoded as
`ModTarget::unknown_flags`, independently of the invert byte. This closes the
previously uncertain 0x02 interpretation without using saved UI values as an
oracle. The inspected local executable's SHA-256 is
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.

Import applies this negative direction to the independently supported slotless
`pitch` and module `filterCutoff` targets in both AHDSR and external assignments.
The raw magnitude, flags and independent invert field remain unchanged in the
vendor reader/writer. Other target laws retain their existing behavior. In the
actual Contradiction snapshot, group 37's filter envelope has magnitude 1 and
flags 0x12; Beta Decay group 51 has magnitude 1 and flags 0x10. These now import
as -1 and +1 respectively. This does not establish either program's complete
sound parity or explain its loudness in isolation.

Native KSP registration assigns both `INTMOD_INTENSITY` and
`MOD_TARGET_MP_INTENSITY` the same public constant 449. For proven pitch and
cutoff routes they now resolve to the same physical address, signed cubic
conversion and readback, including external assignments. Other legacy target
laws remain rejected by the existing resolver guards. The authored saved-target
regression checks both source layouts, signs, target indices, raw preservation,
and separate invert state. The PCM alias gate also compares imported signed
records with physical-depth reference groups and both live setters, with finite
output and zero audio-thread allocations. These are engine causal checks,
not Kontakt reference renders.

Explicit `INTMOD_BYPASS` and modern `MOD_TARGET_INTENSITY` /
`MOD_TARGET_MP_INTENSITY` now also address decoded AHDSRs driving group filter,
EQ and supported insert-stage parameters. Original envelope and target indices
survive omitted unsupported targets. Bypass removes every contribution, including
a nonzero shaper intercept, while the existing envelope clock continues; a mixed
pitch/module envelope updates both prepared copies. Reenabling resumes its elapsed
phase. Filter/EQ coefficients retain the existing 32-frame control ticks. These
controls use the existing normalized module-offset law and modern intensity laws.
The [official KSP engine parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters)
defines modern bipolar depth with zero contribution at 500,000, positive maximum
at 1,000,000 and inverted maximum at zero; it also identifies `INTMOD_BYPASS`
as the internal modulator bypass control. Legacy filter-cutoff
`INTMOD_INTENSITY` is mapped; other legacy module laws remain unverified.
The separately verified native source-bypass mapping initializes supported
pitch/filter/EQ envelopes (see Native internal source switches below).
The authored `module_envelope_controls_preserve_targets_and_elapsed_clock_without_heap`
regression checks PCM bypass/resumption, clock continuity, live depth/readback,
mixed-target addressing, the independent Conflux modern cutoff depth, signed
PCM response, normalized bounds, finite output and allocation counts. Kontakt reference
render parity has not been established.

A bounded check against production revision `e68d28c` merged the actual Analog
Strings base instrument with INIT, Accordia and Analog Wave. Each retained all
483 native groups and original script callbacks. Bypass writes/readback address
480 distinct module-envelope sources; modern depth writes/readback address 959
of 960 filter/formant targets, leaving one unsupported opaque filter target.
The real Layer 2 filter-envelope button produced finite PCM differences in all
three presets (delta RMS 0.0505270, 0.0235627 and 0.0505270 respectively), with the
same voice gain, main envelope, pitch step and sample position in each pair.
This checks callback-to-native-filter routing with a controlled authored stimulus:
a generated sample, replacement one-zone-per-group mapping and appended helper
script set modern target depth, envelope stages and filter cutoff. Group buses
were flattened and program FX omitted. It does not validate the stock legacy
filter-depth knob, original sample-map playback or Kontakt audio parity. Actual
script execution was not allocation-instrumented; the zero-heap and continuous
clock claims come from the separate authored DSP regression. The ignored
`artifacts/combined-e68d28c/analog-filter-envelope/` evidence preserves the probe,
executable, six PCM files, native/control logs and hashed manifest.

A content-free raw-modulator census of the configured 782 presets (788 programs)
completed with zero failures: 226,278 groups, 223,743 AHDSRs, 6,884 flex envelopes,
2,400 opaque LFO objects, and 483 AHDSR pitch targets. The maximum occupied internal
slots was eight. Every group had at most one volume AHDSR, one volume flex, and one
pitch AHDSR; no flex pitch targets or mixed volume/pitch envelopes occurred. Further
same-kind volume envelopes are therefore an explicit remaining feature, rather than
an assumed source of this corpus's missing modulation.

Run `pitch_envelope_uses_ahdsr_and_script_addresses` for the focused DSP/script
regression. Run the ignored `analog_strings_pitch_envelopes_are_routed` with
`KONTRA_ANALOG_NKI` pointing to the installed instrument to verify its aggregate
483 pitch-envelope routes without publishing library content.

## Explicit AHD Only script control

`$ENGINE_PAR_ENV_AHD` addresses decoded AHDSR volume, pitch and module envelopes
through the existing stage parameter route. Its named runtime symbol is appended
so previous serialized parameter IDs remain stable. Boolean readback is 0/1.
The [official Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
describes AHD Only as a one-shot attack/hold/decay envelope completed independently
of the held key. The existing envelope kernel therefore uses a zero decay target,
ignores key release and finishes after decay. It retains the stored sustain and
release settings for ordinary AHDSR mode. As with the existing AHDSR stage knobs,
mode is captured when a new voice starts; this does not claim a Kontakt-calibrated
mid-note mode switch. Explicit stop/fade and sample playback endpoints remain
separate voice lifecycle operations.

Imported native mode/flag bytes remain neutral: byte-exact parser roundtrips do
not identify their AHD, bypass or retrigger semantics. No preset flag is silently
mapped by this control. LFO generation also remains unimplemented; the current
voice path has envelope states, while the existing oscillator code belongs to
chorus/phaser/reverb effects and does not identify native LFO fields. Documented
waveforms/freewheel/retrigger behavior alone cannot establish packed frequency
units, normalized transfer law, initial phase or multi-waveform coefficients.

The focused `scripted_ahd_only_ignores_early_release_and_finishes_without_heap`
regression checks named script initialization/readback, volume and pitch routing,
zero audio-heap writes/rendering, early key release versus holding, finite terminal
zero, block versus scalar versus skipped clocks, curved and zero-duration stages,
and restored ordinary sustain/release. Production compilation is pending when
this evidence note is authored. An isolated rustc 1.98.1 check of the same
pure-stdlib envelope kernel passes its scalar/skip/early-release cases. It is not
an actual-library playback or Kontakt reference-render comparison.

## Opaque LFO investigation (2026-10-02)

A second metadata-only census of the same 782 files and 788 programs completed
with zero failures. All 2,400 opaque sources were direct `0x08` BParLFO children
with structured-object version `0x71`, 67 public bytes, no private bytes, and no
child chunks. The Analog instrument had 271 distinct source payloads. Its source
records use layout id 5; this observation alone does not establish a waveform
enumeration or justify treating a stored frequency as Hz.

Stored nonzero target intensities show that this gap affects initial playback:

| Target | Nonzero assignments |
|---|---:|
| pitch | 63 |
| volume | 672 |
| pan | 364 |
| filterCutoff | 62 |
| filterQ | 18 |
| bitdepth | 1,104 |
| downsample | 1,084 |
| shaper | 831 |
| distortionIntensity | 557 |

The current importer preserves source and target names for `find_mod` and
`find_target`; the raw preset retains the complete chunks. Playback does not
prepare or apply an LFO source. This is independent of the pitch AHDSR repair.
The actual script changes LFO frequency, bypass, pulse width, five waveform mix
parameters, and assignment depth. It also addresses separate external modulators
named `Phase_P1_Retrigger` and `Phase_P2_Retrigger`, and their freewheel counterparts.
The native metadata contains 1,920 such external Constant assignments targeting
`startPhase` on internal-modulator slots. Their routes are retained as module
targets; playback does not apply them. Phase routing therefore needs to be
implemented as well as the source record.

The [NI modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
distinguishes tempo-synchronized frequency, freewheel and retrigger timing, and
normalized bipolar waveform mixtures. It also distinguishes the older Multi
LFO's analog-style ripples from Multi Digital's mathematical waveforms. A generic
oscillator would not establish compatibility with these records. Packed sync
fields, source flags, target scaling and smoothing, and the older waveform law
remain unresolved; no guessed source fields or audible LFO implementation were
added from this census.

The typed native LFO parser/writer now retains both structured and unstructured
serialization, packed records, and unknown float bits under neutral field names.
Metadata checks of the base instrument and three snapshots reencoded all 9,600
complete LFO chunks byte for byte. The snapshot payloads differed from the base
in 14, 200 and 623 chunks respectively, so this checks edited records as well as
unchanged ones. These checks establish serialization fidelity, not audio behavior.

### Frequency and clock boundaries

The actual script's tempo lookup has 8,382 entries: 22 rhythmic divisions at
381 integer BPM values from 20 through 400. Rhythmic duration and BPM determine
an expected cycle rate independently of the normalized engine value. The entries
reduce to 3,965 unique rate anchors; repeated expected rates always have the same
engine value. Observed values span 840 through 970180 and expected rates span
0.0104167 through 160 Hz, with strict monotonic ordering. This is evidence about
the library's intended rates, not a measurement of Kontakt oscillator output.

The anchors 599338 at 4 Hz and 669111 at 8 Hz determine an exponential candidate.
It misses the other anchors by up to 0.5315% (RMS 0.3775%). Exact interpolation
would reproduce supplied anchors, but does not determine behavior between them,
outside the observed range, or when stored sync settings apply. The proprietary
lookup was not copied into production code, and no fitted rate law was applied.

The [official KSP engine parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters)
lists separate frequency and fade-in unit parameters, as well as LFO phase and
normalization. The packed native records have not been correlated to these units.
In particular, stored words such as 12, 24, 48 and 96 cannot yet be treated as Hz
or note divisions. NI documents retrigger as restarting the waveform and freewheel
as running without reacting to notes, but the native phase origin, start phase
units, bypass clock, and waveform values at that origin are not established.

A further 782-file metadata census found lag zero on all 483 AHDSR pitch targets
and 480 LFO pitch targets. All 1,920 LFO assignments per other observed target
have nonzero lag. Future source playback therefore also needs independent target
smoothing. The external group-target census found no pan assignments and found
160 nonzero Constant assignments each for `loopStart` and `loopLength`, all with
lag zero. Their loop offset/range semantics remain unsupported.

## Not verified

- How Kontakt combines intensity, invert and shaper for volume and pitch (the
  multiplicative model above is inferred from stored crossfades). No Kontakt
  reference renders are available.
- The attack and flex curve steepness, the flex level scale beyond 0/1 and its
  unknown index and tail, the envelope-time modulation law, 0x4A dynamics, the source
  module, internal-modulator bypass, the extra AHDSR flag and records, and the
  ext-mod unknown bytes and id.
- LFOs: source identity, record dimensions, target routes, and actual script calls
  are observed; packed field meanings and playback behavior remain undecoded.


## Native internal source switches (2026-10-02)

The BParInternalMod v0x80/v0x81 typed reader and native XML importer
independently identify the four packed bytes after the target list. The reader
stores them at object offsets 0x549, 0x540, 0xab8 and 0x548 respectively.
The XML importer compares `routersOpen`, `bypass` and `retrigger` against `yes`
and writes offsets 0x549, 0x540 and 0xab8. Therefore byte 0 is router UI state,
byte 1 is source bypass, and byte 2 is retrigger. Byte 3 remains unnamed.
This mapping comes from exact same-object stores, not correlations between
preset values. The former byte-0 mode/bypass warning was incorrect.

Saved bypass metadata is now retained for independently supported internal
sources. Pitch/filter/EQ AHDSRs initialize their existing bypass state from
that metadata; their target depths and running envelope clocks remain separate.
Amplitude AHDSR/flex bypass needs independent native note-off/lifetime evidence
before it can safely control source termination. In particular, masking a
completed amplitude envelope could otherwise leave an indefinitely looping
released voice. No such amplitude lifetime override is accepted from the flag
mapping alone.

For LFO BParLFO v0x71, exact native XML-to-object stores and the typed binary
reader also identify delay/frequency/pulseWidth/startPhase at public offsets
4/8/12/16, normalization at 20, noteValue_frequency at 21, noteValue_delay at
34, and five waveform mixer levels at 47/51/55/59/63 (sine, rectangle,
triangle, sawtooth, random). Its enum maps 0..5 to sine, rectangle, triangle,
sawtooth, random, Multi. Remaining sync fields, freewheel/retrigger timing,
converter laws and the older Multi waveform law remain unresolved; no
oscillator, arbitrary frequency calibration or inferred flag-3 behavior is
applied. The 2,400-record Analog census has 2,180 sources with saved bypass,
so future LFO playback must retain that switch as well as target smoothing.
None of this establishes reference Kontakt sonic parity or resolves all six
reported Analog preset differences.

## LFO waveform schema and clock follow-up (2026-10-02)

The first BParLFO public integer is a waveform selector, not a layout ID.
The native typed reader uses the same packed prefix for versions 0x71 and
0x72: waveform types 0..4 have 47 public bytes, while types 5 and 6 append
five mix levels for 67 bytes. Version 0x73 appends one additional boolean
to either form. The shared decoder/writer now retains these exact forms,
rejects mismatched lengths and invalid boolean bytes, and leaves unknown
versions/types opaque. Type 6's name and the additional boolean's meaning
remain unassigned. One authored roundtrip gate covers all accepted
waveforms, versions and structured/unstructured envelopes, including
truncation, signed zero and NaN bit preservation. This broadens format
decoding; it does not enable an audible LFO source.

Independent native note-value frequency/time getters corroborate the
documented sync units: at tempo T, note value N and count C, synchronized
frequency is T/(60*N*C) Hz and synchronized fade-in is 60000*N*C/T ms.
An unsynchronized frequency remains Hz. Native preparation converts this
frequency to a source phase step using sample rate, and passes the saved
phase directly as cycles. The serialized sync fields at25/29/38/42 and
booleans33/46 are still retained neutrally: generic converter bounds and
integer-count controls occupy other native members and must not be
inferred from these serialized fields.

The [Kontakt modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
distinguishes legacy Multi's ripples from Multi Digital. The identified
legacy Multi processor uses interpolated waveform tables, signed mixer
levels and normalization by max(1, sum of absolute mixer levels). Its
source scheduling also compensates phase at block boundaries and accepts
frequency/phase modulation buffers. These details, plus group freewheel
ownership, prevent treating a generic per-voice digital oscillator as
verified playback. No proprietary waveform tables, arbitrary calibration,
source bypass clock law or amplitude-lifetime change is included here.


## Constant loop-boundary controls (2026-10-02)

The native target registry identifies `loopStart`/`loopLength` as destinations
7/8. The common target-output binder places their 32-frame control buffers at
voice-context offsets 0x2f10/0x3010; SamplerModule reads those same buffers and
passes them to the loop consumer. That consumer adds truncated float32
fractions of total physical sample frames to both loop bounds (`loopStart`)
or to the end (`loopLength`). Independent decoder-length and WAV byte-bound
setters identify its total-frame member. Its gating member is a signed sample
preload-byte budget, not a source-mode enum; serialized mode 0 produces zero
budget, while mode 3 prepares a nonzero budget and disables this consumer.
The exact named-mode registry bridge remains separate.

A stored UI/timing case and the original script inverse lookup independently
produce normalized writes 751086/14233. Cubic signed conversion yields
+0.1266360866/-0.9170098731, matching native magnitudes 0.1266360730/0.9170099497
within float32 precision. Thus loop targets share the signed cubic setter and
inverse-cube-root readback law; this is a matched writer record, not an
assumption from the pitch law. The independently traced target sign bit is
applied to these two destinations; the separate Invert button is retained.
The official [NI zone API](https://docs.native-instruments.com/ni-tech-manuals/kontakt-api-reference-manual/en/zone)
defines loop geometry in sample frames, but does not publish this normalized
modulation law. The producer/binder/consumer chain supplies that evidence.

Playback prepares all eligible Constant, zero-lag, unshaped and non-inverted
assignments, including initially zero depths. On held forward full-sample
loops in serialized Source mode 0, geometry updates at native 32-frame ticks.
Native sample bounds and minimum four-frame length are retained. The saved
crossfade is clamped **after** the offset, so moving a zero-start loop does
not erase its crossfade. Live edits preserve the fractional physical sample
cursor and reconfigure the existing streamer with the same map used by RAM
playback. Until-release paths keep their final seam and tail after note-off.

Cropped, reverse, alternating, other-source, shaped, inverted and lagged
assignments remain diagnosed. Behind-cursor seam transitions and invalid
signed bounds are deferred; each distinct edit is counted once in the voice
census and retried on eligible control ticks without audio allocation.
Edits after release retain the final geometry. No arbitrary assignment cap,
forced whole-sample RAM, new modulation framework or full Kontakt loop-parity
claim is introduced. The focused authored gate covers signed writer/readback,
physical bounds, nonaligned control fragments, fractional cursor continuity,
RAM/stream PCM equality, deferred retry and until-release lifetime with zero
audio-thread heap operations. Actual-library PCM validation remains separate.

## Saved sine-only Multi pitch playback (2026-10-02)

A first audible subset now prepares saved version0x71 Multi sources only:
retrigger enabled, zero phase, synchronized positive note value
and count>=1, sine weight magnitude<=1, and the other four weights zero.
Only ordinary sampler pitch targets with finite signed depth, zero lag,
no inversion and no enabled shaper are applied. External assignments to
the source's own parameters exclude it. Other states and live LFO timing engine
parameter writes retain explicit unsupported diagnostics. Live source bypass is
applied only to these prepared pitch LFOs, by original source slot. Admitted pitch sources
retain each original target index and signed depth. Signed legacy and modern
intensity aliases and unsigned target intensity address those individual routes,
update the prepared sum without allocation or phase reset, and share one physical
source-slot/target persistence record. Non-pitch targets and older metadata without
individual target identities remain unsupported.

Independent source scheduling identifies a sampleRate/32 source clock,
ceil(audioFrames/32) waveform points and correction for the final partial
control interval. The sine-only legacy Multi component has the opposite
sign from the simple sine source. Source bypass pauses its phase. The
pitch target binder is independently connected to the ordinary sampler's
pitch buffer: it linearly interpolates previous/current control points
per audio frame with a retained modulo32 offset, rather than holding
pitch for32 frames. KONTRA's command/voice-planning fragments are not native
source-dispatch blocks: source points are sampled at that retained note-clock
boundary, so 17/111 fragments and an unrelated eligible loop's 32-frame planner
splits cannot shift the oscillator. The native producer's partial-block phase
correction independently establishes actual elapsed-frame phase advancement;
arbitrary native host-block dispatch alignment remains unverified. The renderer applies that curve only to affected
voices; ordinary fixed-pitch lanes retain their existing fast path.
The oscillator continues through the existing release envelope and does
not extend voice lifetime. No amplitude bypass lifecycle is changed.

Sine values use the standard sine function rather than copying the
proprietary legacy waveform table; this is an explicit numerical
approximation. The authored gate independently generates control points
and the audio ramp, then compares actual PCM from a linear sample, both
bipolar halves, short fragments, retrigger, release retirement and zero
heap activity. The combined Constant-loop gate additionally compares
unchanged-loop eligibility against no control assignment, 128 versus 17/111
PCM, RAM versus bare-stream playback, and a command at frame141 split as
13/115 versus 13/19/96 with native-tick application and bounded release.
It does not establish full reference Kontakt sonic parity.
Free-running/shared ownership, other waveforms and Multi mixes, synchronized
fade-in, external phase/frequency modulation, lag and live legacy
frequency dispatch remain unsupported.

Recognized group-insert modulation targets use a separate slot namespace from
internal LFO sources. A saved `shaper` assignment to group effect slot7 therefore
does not invalidate the saved pitch LFO at internal slot7. The shared reader uses
the existing effect-parameter classifiers; unknown same-numbered module targets
still exclude the saved-only source clock with a diagnostic. The authored raw
fixture preserves the actual-shaped Constant zero-depth/15 ms lag assignment and
checks both the recognized insert case and conservative unknown-source rejection.

The native pitch pipeline interpolates normalized octave values before an
in-place exponential conversion to playback ratios. KONTRA follows that order
with standard floating-point math; it does not interpolate ratio endpoints or
copy the proprietary exponential approximation. A private optimized stdlib probe
measured about 6.1 ms per 1,048,576 planned frames for this subset on this host;
that is not a whole-engine performance or real-library timing claim. Cached
exponential recurrences have not been introduced.

### Saved legacy pitch LFO fade-in (2026-10-03)

Saved version0x71 sources additionally retain unsynchronized fade-in durations
in milliseconds, within the documented0..5000 ms range. The typed reader,
object copy, source initializer, reset and final waveform consumer independently
connect the saved delay to the source fade state. Duration becomes
`N = trunc(fade_ms * (sampleRate / 32) * .001)` control points. Reset starts
at `0.3f32`; the factor is `pow(1 + 1 / double(0.3f32), 1 / N)`, rounded to
f32. For exactly N points the source output is multiplied by `state - 0.3f32`,
then the f32 state is multiplied by the factor and clamped to0..1. Later
points pass through. The legacy reader explicitly seeds the mode false;
version0x73's final boolean enables a different clamp and remains unsupported.
This preserves the older curve instead of replacing it with a linear fade.
The oscillator continues throughout the fade; note retrigger restarts both
phase and fade, and bypass pauses their source-clock progression.

The selected saved Prelude192 value0.21523452 ms produces zero fade points
at48 kHz, Keytar123's2.3047996 ms produces three, and Keytar142's1.9533157 ms
produces two. These are raw snapshot properties, not post-callback readbacks.
Their onset timing does not establish a cause for longer quiet/thin reports.
The existing authored physical-cursor/PCM gate covers these values plus40 ms,
an independent closed-form fade reference, planner fragmentation, retrigger,
release retirement and zero heap activity. A separate stdlib-only Clock check
passes18 rate/duration cases with maximum analytic cursor error0.000019351
source frames; it is a standalone math check, not an engine PCM result.

Primary control semantics: [Kontakt modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation).
The mathematical legacy distinction comes from independent native dataflow,
not from interpreting the manual's general description as an exact curve.

### Saved legacy volume LFO fade-in and source ownership (2026-10-04)

The same admitted version0x71, retriggered sine-only Multi source now retains
its saved unsynchronized0..5000 ms fade for its one admitted volume target.
This reuses the pitch source's existing legacy recurrence. It does not change
the native volume target's polarity, bipolar-to-unipolar conversion or lag law.
At initial zero source fade gain, that target therefore receives bipolar zero
and converts it to0.5, before applying its authored intensity.

Static evidence was rechecked in Kontakt8.13.1's existing PE64 executable
(SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`,
image base `0x140000000`), using bounded local radare2/Ghidra analysis:

- RTTI `BLfoMultiWithTargets<0>` is at `0x14a399170`; its complete-object
  locator is `0x14987fb60` and vtable is `0x144729a78`. The waveform method
  at vtable offset `+0x30` is `0x140b07290`.
- `0x140b07290` fills the caller's float waveform buffer, applies Multi
  normalization when enabled, then scales that same completed buffer by
  source state `+0xf0` minus the0.3 constant. It updates/clamps the f32 state
  using factor `+0xf4` and decrements remaining count `+0xfc`. This source
  method has no pitch/volume target-family condition: fade is already present
  in the returned waveform, before a target consumes it. No proprietary
  waveform tables were copied; the previously documented std-sine boundary
  remains.
- Reset `0x1405f45a0` restores phase `+0xb0` from `+0xe8`, remaining count
  `+0xfc` from authored count `+0xf8`, and seeds `+0xf0` with `0x3e99999a`
  (0.3f32). Source state copy `0x1405e5020` copies phase, current fade state
  and remaining count together. These are source-owned clocks.

KONTRA deduplicates each sampled fade value by original internal source slot
within the retained32-frame control point. Shared pitch and volume consumers
therefore advance one Fade, while pitch-only planning copies produce the same
physical cursor. The fixed16-slot callback stack cache allocates no memory;
source bypass pauses phase/fade, mute continues their audio clock, and a fresh
voice resets phase/fade/target lag. Live timing/fade/intensity, synchronized fade,
freewheel, other waveforms and broader target configurations remain unsupported.

One staged complete Engine PCM gate uses a stereo ramp/constant, independent
closed-form source fade plus scalar volume-target reference, shared pitch+volume
versus volume-only output, shared/unshared lanes,128 versus1/7/31/128 fragments,
mid-tick script bypass/resume, mute/unmute, empty calls, retrigger, exact note-end
agreement and callback heap guards. It is staged for the parent's central
compile/test harness; no result is claimed here. The manual's source-wide
[Fade in control](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
supports the range and retrigger semantics; the ordering and legacy curve come
from the native waveform method above.

### Live bypass of prepared pitch LFOs

`$ENGINE_PAR_INTMOD_BYPASS` controls each admitted pitch source's prepared
bypass flag. The source slot is preserved independently from envelope indices
and insert-effect slots. Existing native edit storage seeds getters, applies
changes without audio-thread allocation, and restores edits before init getters.
Unprepared free-running or wider-wave sources remain unsupported; this does not
implement their clocks or broaden unadmitted target configurations.

Independent native source processing skips both waveform generation and target
binding for a bypassed source. The pitch consumer validates the current target
buffer before interpolating it: when no active source binds pitch, it uses base
pitch and leaves its previous/current interpolation state intact. With another
active source, only the remaining contribution enters the ordinary control-point
sum. Phase and legacy fade progression pause while bypassed; the separate
audio interpolation offset continues. KONTRA retains the existing note-clock
32-frame adapter and its documented native host-alignment boundary.

The focused gate uses an independent linear-source PCM reference for single and
mixed sources, mid-tick removal/resumption, paused phase/fade, all-source bypass,
13/19 versus128 fragments, release retirement, zero heap, readback, restoration
and a fresh load without prior edits. Actual-library parity remains a separate
validation task. The general parameter contract is documented in the
[Kontakt engine parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#modulation);
the clock and interpolation behavior is grounded in independent native dataflow.

## Saved CC defaults and accepted source availability (2026-10-04)

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation), Assignment controls, distinguishes an unknown controller position from a received zero. A saved CC default substitutes until the first controller value arrives; default−1 leaves the destination unmodulated. This gate precedes the shaper and smoothing, so a nonzero shaper intercept cannot activate an unreceived source.

Static evidence is from Kontakt8.13.1, image base`0x140000000`, SHA256`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`:

- `BParExternalMod` RTTI`0x14a3a1ed8`, vtable`0x144e74c60`: reader`0x140d09620` reads the first post-source word into`+0x558`; writer`0x140d15c10` writes it back. In the existing ni-file `ExternalModParams`, these are exactly the first two little-endian bytes of`unknown_source_data`. The two following source bytes remain opaque. The admitted saved versions remain0x100..0x104.
- Source setup at`0x1409766d1` selects internal source code4 (MIDI CC);`0x1409766da` sign-extends the saved word. A nonnegative default is normalized and installed through`0x140cebb70`; a negative default sets source byte`+0xa8` at`0x1409766fb`. The setter stores held value`+0x1b8` and clears`+0xa8`.
- Target dispatch at`0x140916619` tests that source byte and skips the route to`0x14091692b` when inactive. The accepted timeline setter`0x140cebc40` clears`+0xa8` before inserting a timestamped value. `BExtMod` RTTI`0x14a3af040`, vtable`0x144f17268`, waveform virtual`+0x30` (`0x140ceaf60`) fills held source samples and consumes that queue.

KONTRA now prepares `ModAssignment.cc_default: Option<i16>` only from this verified CC word. Negative words disconnect; admitted nonnegative defaults are0..127, and preparation rejects larger values explicitly. Absent metadata preserves the existing behavior of hand-authored/default-less routes. `MidiState.cc_received` records accepted physical or authored CC values independently of numeric observer caches, including accepted zero and canonical14 coarse/fine identities. Installing a new Bank clears only this receipt mask; residency upgrades retain it. Session reset clears it before authored init-controller replay. Existing controller-reset writes mark the actual supplied defaults as accepted, retaining untouched controllers and channel scope; this is the existing core receiver policy, not a separately established native CC121 first-seen rule. The distinct raw Group-start receiver and physical protocol decoder remain under their own owners.

Prepared voice values/targets/preview retain typed optional availability; external filter states retain their activity bit. Inactive routes skip shaping, lag progression, gain/pitch/time/start contributions and filter target binding, including native Ladder modulation mode. All storage is allocated in the existing worker-prepared pool with checked sizes; callback processing allocates nothing. Start-range preparation covers both the accepted seed and authored initial substitute. Frozen MPE CC74 records receipt separately from its numeric member value, so member reuse cannot activate an old unreceived tail; manager and exact-note brightness keep their existing ownership and precedence.

The existing core first-event positive-lag seed/recurrence is preserved. Native initialization of the first received target's lag state, external invert ordering and the opaque source flags remain unproved and are not changed by this repair. Existing MPE numeric composition is retained; this slice establishes its source availability rather than a new native composition law.

One full Engine PCM scenario is staged in`src/engine/controller_defaults_tests.rs`: imported raw defaults−1/0/127, nonidentity shapers, gain/pitch/start/native Ladder identity, physical channel scope, script filtering and authored control, canonical14 input, bank replacement versus residency upgrade, mute/release/exact completion and frozen MPE tails with exact-note brightness. It compares route-free or explicit destination/reference engines across128 and1/7/31/127 fragments and checks callback/render heap guards. This audit does not claim execution or actual-library parity; central compilation and execution belong to the root owner.

## Multi Digital source law (2026-10-04)

Bounded analysis of the same Kontakt8.13.1 executable closes the previously
unnamed waveform type6: factory `0x140605430` selects vtable `0x144729c08`
for that arm; its RTTI names `BLfoMultiDigitalWithTargets<0>`, and waveform
virtual `+0x30` is `0x140b078d0`. The [NI modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
independently distinguishes Multi Digital's arithmetic shapes from legacy
Multi's analog approximations and ripples. This proof names type6; it does
not replace type5's legacy table consumers with digital shapes.

The existing packed reader's public offsets retain phase cycles at16,
pulse width at12, normalization boolean at20 (`records[0].flag`), and signed
sine/rectangle/triangle/saw/random levels at47/51/55/59/63 (`trailing_values`).
The delay-record trailing flag at46 is not normalization or retrigger.
Source preparation `0x140605430` reads parameter IDs700..704 into source
`+0x128/+0x134/+0x130/+0x12c/+0x138`, respectively, and normalization into
`+0x13c`. Pulse width is clamped to f32 constants0.05 and0.95; saved phase
is copied directly, independently of the displayed degree unit.

For finite phase `p` in0..1 and duty `d`, the unmodulated Digital source
uses these components, in this accumulation order:

| Component | Bipolar source contribution |
| --- | --- |
| Sine | `-S(p) * sine` using the existing native interpolated sine table |
| Saw | Set `q=2*p`; subtract1 only if `q>1`; then `(2*q-1) * saw` |
| Triangle | `4*(p-0.25) * triangle` for `p<0.5`, otherwise `4*(0.75-p) * triangle` |
| Rectangle | `-rectangle` for `p<d`, otherwise `+rectangle`, including equality |
| Random | Held bipolar value times random level |

The double saw/triangle expressions become f32 before multiplication by
their f32 weights and sequential f32 accumulation. Saw's doubled phase
and strict comparison are present in assembly `0x140b07a17`..`0x140b07a42`;
they must not be silently replaced with a conventional one-cycle ramp.
Triangle arithmetic is at `0x140b07a4a`..`0x140b07a7f`, rectangle comparison
at `0x140b07a87`..`0x140b07ab0`. Normalization, when enabled, multiplies by
the reciprocal of `max(1, ((abs(random)+abs(rectangle))+abs(triangle)) +
(abs(saw)+abs(sine)))` at `0x140b07ea0`..`0x140b07eee`. When disabled it
leaves the sum unchanged; there is no final source clamp. The completed
buffer then enters common legacy fade `0x140b06c90` via the tail jump at
`0x140b07f5b`, before pitch or volume consumes it.

Shared reset `0x1405f45a0` restores double phase `+0xb0` from saved `+0xe8`,
resets fade/count, and sets random threshold `+0x120` to0.5. Digital advances
phase after producing each control point and subtracts1 only when phase
is strictly greater than1 (`0x140b07b45`..`0x140b07b4f`). Exactly1 therefore
survives: its saw/rectangle endpoints differ from phase0. The existing sine
adapter's modulo collapse must not hide this boundary when admitting these
additional shapes. Existing32-frame source-point scheduling, retained target
interpolation, shared original-slot ownership, retrigger and bypass/fade
rules remain the bounded clock contract; freewheel/shared-group clock,
external frequency/pulse modulation and synchronized fade are not newly proved.

Random initialization `0x1405ed740` seeds from `GetTickCount`, advances u32
state with `state*0x0bb38435+0x3619636b`, and interprets bits
`(state & 0x7fffff) | 0x40000000` as f32 before subtracting3. After a source
point, crossing the alternating0.5/1.0 threshold advances the same state
and changes the held value for the next point. Phase reset does not reset
that seed or held value. Random weight must remain zero in the proposed
deterministic saved Digital slice; a fabricated note-local random sequence
would not establish native parity.

The implementable extension is saved version0x71, retriggered type6 with
finite signed deterministic weights, optional normalization, saved duty and
phase, and the already admitted pitch/volume target and clock conditions.
It reuses the existing standard-sine approximation explicitly. Legacy
additional components still read cubic-interpolated4096-entry tables through
`0x140b08f90/0x140b09150/0x140b08c40`; none of those proprietary tables was
copied, and ideal digital replacements are not claimed equivalent. This
section records static source evidence only; implementation and PCM gates
belong to the coordinated core owner and central harness.

The bounded Digital consumer is now prepared as optional `DigitalLfo` metadata.
Absent metadata retains the prior legacy arithmetic and cached-source behavior.
Admission keeps version0x71, parent-wrapper retrigger, existing saved timing and
pitch/volume target restrictions; it additionally requires finite deterministic
weights within the existing signed -1..1 subset, zero random and finite pulse.
`records[1].flag` is public33, the separate frequency-sync record flag;
`trailing_flag` remains the opaque public46 delay-record flag. Neither is the
normalization bit. Normalization is exclusively public20 / `records[0].flag`.

Source rate virtual `0x1406068b0` writes the f32 source rate and its double
reciprocal at `+0xc0`. Factory/source-preview preparation `0x1406058cd` writes
`+0xb8` from promoted frequency times that reciprocal; Digital waveform
`0x140b07adc` independently consumes the same double step. This closes that
component setup, not every production frequency setter. The existing admitted
32-frame source clock is reused. One original-slot cache holds the completed
normalized/faded Digital point for both pitch and volume, and phase advances
once after that point. Planning uses copied clocks; mute, bypass and retrigger
retain the existing note/source owners. Runtime work uses fixed16-slot storage.

The strict single subtraction only establishes a bounded phase for a finite
source step in0..1. Both pitch-only and volume-only plans now validate that
geometry before source-state or PCM advancement; unsupported low public engine
rates report the existing checked `UnrepresentablePitch` source failure. There
is no global audio-rate floor or alternative modulo waveform. The normal
in-range recurrence and exact phase1 endpoint are unchanged.

One complete Engine PCM scenario is staged with independently recorded eighth-
cycle saw/triangle values, a scalar pulse/sine/normalization/fade/target reference,
and a separate plain Engine supplying only amplifier PCM. It covers exact
phase1 and duty equality, normalization on/off, zero/40ms fade, dual pitch/volume
source reuse, shared/unshared voices, fixed/1/7/31/127 fragments, mute, bypass,
live admitted pitch-depth retarget, release/retrigger and exact note ends. It
also stages pitch-only and volume-only low-rate checked failures and callback
allocation checks. These scenarios have not been executed by the source owner;
central compilation does not establish native waveform-table parity.

## Saved Flex v0x12 Loop and Oneshot switches (2026-10-04)

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation), Flexible Envelopes, distinguishes Oneshot, which completes the envelope despite key release, from Loop, which repeats its sustain section. Both enabled continue indefinitely. A loop covering one segment freezes at its end; a wider section repeats. These controls do not establish any private byte mapping by themselves.

Bounded static inspection of Kontakt8.13.1, image base`0x140000000`,
SHA256`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`,
closes the mapping independently:

- RTTI`BParEnv_FM7` is`0x14a3a1de8` (complete-object locator`0x149899638`,
  vtable`0x144e74940`). Type getter`0x140762600` returns chunk`0x40`;
  version getter`0x140551770` returns`0x12`. The legacy internal class name
  identifies this concrete Flex object; it is not an alias for another
  advertised envelope model.
- Archive reader`0x140cffef0` reads last-point index into object`+0x190`,
  the next word into`+0x18c`, and the following word into`+0x188`.
  After the point triples, version`0x12` reads two separate bytes into
  `+0x194/+0x195`, followed by the existing13-byte timing record. Versions
  `0x10/0x11` do not read these switches; they are not admitted from an
  inferred legacy default. Writer`0x140d0ffd0`, instructions
  `0x140d1006f..0x140d10088`, independently writes the two switch bytes
  separately and then writes the timing record. The timing record's final
  byte is not the Oneshot switch.
- `BParEnv` wrapper reader`0x140d05bd0` requires version`0x90`; kind1
  selects the concrete child at wrapper`+0x118`. Constructor
  `0x140703500..0x140703513` installs the FM7 vtable at this exact offset.
  Named XML importer writes `loopStart` to wrapper`+0x2a4`
  (`0x140924ace`), `loopEnd` to`+0x2a0` (`0x140924b11`),
  `loopEnabled` to`+0x2ac` (`0x140924b5c`) and `oneshotMode` to
  `+0x2ad` (`0x140924baa`). Subtracting the established child offset
  gives concrete object`+0x18c/+0x188/+0x194/+0x195`. Therefore the
  saved unknown index is loop start, saved sustain is loop end, and the
  v0x12 opaque u16 consists of Loop then Oneshot booleans.
- Internal-record constructor`0x1407b01dd` bases its working pointer at
  record`+0x65c`; it installs the wrapper at working-pointer`-0x64`
  (`0x1407b02a8`, record`+0x5f8`) and FM7 at working-pointer`+0xb4`
  (`0x1407b0342`, record`+0x710`). Independent object copy
  `0x1407f8ae3..0x1407f8b1b` transfers internal-record
  `+0x898/+0x89c/+0x8a0/+0x8a4/+0x8a5` into concrete FM7
  `+0x188/+0x18c/+0x190/+0x194/+0x195`. Source setup
  `0x140965390..0x1409653d0` transfers those same five fields into
  runtime loop-end`+0x2fc`, loop-start`+0x300`, last-point`+0x304`,
  Loop`+0x321`, and Oneshot`+0x322`.
- `BEnvelopeFM7` RTTI`0x14a3aaef0`, vtable`0x144e94ca0`, owns the
  running source. Release virtual`0x14096be80` ignores release when
  `+0x322` is nonzero; otherwise it sets remaining count`+0x30c` to1
  and next-point`+0x314` to loop-end+1. Segment method`0x140ae5450`
  wraps the next point to loop-start+1 only when Loop`+0x321` is
  nonzero. Without Loop it traverses all points and then marks completion
  at`+0x2f8`. Waveform virtual`0x140ae44e0` transfers that completion
  to source done byte`+0xb0`.
- Terminal compare`0x140ae546d..0x140ae546f` tests next-point against
  last-point without inspecting the final level. Terminal stores
  `0x140ae5471..0x140ae5483` set multiplier1, offset0, completion1
  and remaining count65535. Waveform`0x140ae45c1..0x140ae45ca`
  marks source done, and done virtual`0x1405edd60` returns that byte.
  Primary `BEnvelopeFM7WithTargets<0>` RTTI`0x14a3b2a60`,
  vtable`0x144f2ade8`, uses these same waveform/reset/release/done
  methods. A nonzero last point therefore completes the source; its
  numeric final level survives the remainder of that source buffer.
  Exact nonzero-start amplitude and sample-voice retirement are not
  established merely by this source flag.

The narrow implementable slice is v0x12 Oneshot enabled, Loop disabled,
unsynchronized default timing, with the already required uninverted unity
volume assignment, zero target lag, no enabled shaper, known wrapper and
retriggered active source geometry. Preserve the raw index and13/15-byte
tail through cache serialization. Require a zero final point for this
slice; unsupported nonzero endpoints retain metadata and diagnostics.
A prepared boolean may then suppress
Flex key-release transitions and the sustain freeze, allowing the existing
point clock to finish every point. Note ownership, physical release,
release-trigger callbacks, independent paired AHDSR processing and explicit
stop fades retain their existing owners. EOF may still end a finite source.
Loop, synchronized timing and non-retriggered shared source ownership need
separate admission; this slice must not infer them from the final tail byte.

This establishes saved-switch and lifecycle semantics, not exact native Flex
PCM. Native reset`0x14094ce70` initializes from the last point's level and
next-point-1; segment setup inserts a5ms synthetic initial segment (float
literal`0x144f38634`, bits`0x3ba3d70a`). Its curve and duration arithmetic
also differ from the existing KONTRA Flex consumer. The narrow switch slice
must retain and disclose that existing waveform boundary, leaving ordinary
non-Oneshot arithmetic unchanged. Complete serialized-source PCM, early
and duplicate release, fragments, mute/skip, EOF and callback heap checks
are required before verification; no build or scenario was executed for
this static evidence append.

Production now retains the raw switches/indices through cache serialization
and admits exactly the finite v0x12 Oneshot subset above via
`Group.native_flex_oneshot`. Prepared `Flex.one_shot` suppresses key-release
transitions and the ordinary sustain freeze, preserving every existing
non-Oneshot arithmetic operation. No loop or legacy default is inferred.
One staged complete Engine gate,
`saved_flex_oneshot_preserves_physical_release_and_complete_pcm_without_heap`,
uses serialized native wrapper/source geometry, raw/cache roundtrips and
unsupported mode/source/endpoint cases. It compares held and early/duplicate
released recordings with independently timed linear points and separate
unmodulated reference voices, including overlapping retriggers, pedals,
mute/skip, empty calls, sharing and irregular fragments, exact finite
retirement boundaries, finite EOF, paired AHDSR and explicit SoundOff.
Callback/render allocation guards are staged; no scenario execution is
claimed here. The native synthetic startup and curve arithmetic remain the
existing scalar fidelity boundary.

## Native Glide identity and unresolved note-history owner (2026-10-04)

The [NI modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation), Glide, describes a legato pitch transition that also works polyphonically. Time mode fixes transition duration; Speed mode fixes travel rate. Those controls do not identify private fields or establish which prior note qualifies.

Bounded inspection of the same Kontakt8.13.1 binary establishes:

- `BParGlide` RTTI `0x14a3a1eb8`, complete-object locator `0x149899940`,
  vtable `0x144e74b80`. Its `+8` type getter `0x140745d10` returns chunk
  `0x0B`; its `+0x10` version getter `0x140762b00` returns `0x51`.
  The version value must not be mistaken for the chunk identifier.
- Reader `0x140d034d0` accepts versions `0x50/0x51`. A saved u32 of1 or2
  becomes internal mode0 or1 at object `+8`; other values reject. The next
  32-bit scalar is stored at `+0x0c`. The next scalar is stored at `+0x28`
  and compared against a native threshold to derive byte `+0x18`.
  Version0x50 sets byte `+0x19` to1. Version0x51 additionally reads two
  scalars into `+0x34/+0x38` and a byte into `+0x19`. Writer
  `0x140d12460` independently writes mode1/2, these four scalars, and the
  final byte. It substitutes native literal `0x144714a54` for `+0x28`
  when `+0x18` is false. The focused binder/initializer follow-up below
  closes Time/Speed, coefficient, and the consumed sync fields; the two
  version0x51 scalars and final byte retain their unresolved raw meanings.
- Concrete copy `0x1407f8cc6..0x1407f8cf9` installs the same parameter
  vtable and copies source `+0xa70/+0xa74` to child `+8/+0x0c`, then
  copies its timing object from source `+0xa78` to child `+0x10`.
- `BGlide` RTTI `0x14a3aaed0`, locator `0x1498a1798`, vtable
  `0x144e94be8`, and `BGlideWithTargets<0>` RTTI `0x14a3b4068`, locator
  `0x1498b6a30`, vtable `0x144f2f360`, share waveform `0x140ceb090`.
  Targeted constructor `0x1407c0f50` allocates0x150 bytes and installs
  the latter vtable. It is reached for internal source kind5 through the
  target-count dispatch `0x1409741ed..0x140974289`.
- The waveform emits f32 state `+0xb8` multiplied by f32 scale `+0xc0`,
  then adds signed f32 step `+0xbc`. Once the state and step have crossed
  to the same nonzero sign, it zero-fills remaining output. The ordinary
  scalar tail is independently visible at `0x140ceb240..0x140ceb280`.
  This proves a bounded linear source recurrence, not its note initializer.
- Shared source setup `0x140976465..0x140976471` supplies the program
  audio rate times f32 `0.03125` to virtual `+8`. Glide rate virtual
  `0x1408dcd90` stores `+0x14` and clears `+0x58`; its alternate rate
  setter `0x1406068e0` stores `+0x14`. Reset `0x1405f44a0` only clears
  byte `+0xa1`. Source comparator `0x1405f06a0` compares `+8/+0x18`.
  Neither establishes a note-local reset of pitch difference.

Current `ni-file` `internal_mod.rs` recognizes only AHDSR/Flex/LFO children;
this child consequently remains `Other { chunk_id: 0x0B }`. KONTRA keeps its
modulator/target identity and unsupported diagnostic without preparing a
Glide clock. The follow-up below closes the initializer recurrence and
history pair writes. Complete routing identity, history reset/eligibility,
retrigger/sharing, and final target binding remain admission blockers.
Existing mono/polyphony-one policy, physical controller caches, library raw
receiver, keyswitch selection, and the ordinary envelope `glide()` helper are
not evidence for those owners.

The targeted state-store candidates `0x140971cbe/0x140971d3e` were excluded:
they adjust circular-buffer indices/masks over inline0xc0-byte DSP objects,
not the identified `BGlide` object. No source behavior was implemented from
those offsets. Static component evidence alone does not justify an arbitrary
pitch sweep. No build or test was executed during this audit.

### Glide binder, initializer, and history pair follow-up

The concrete child payload is retained without inventing timing fields:
v0x50 is LE u32 mode (1 Speed, 2 Time), f32 coefficient, then f32 sync note
value (12 bytes). V0x51 appends two f32 timing fields and one u8 timing flag
(21 bytes). Reader `0x140d034d0` stores these at parameter `+8/+0xc/+0x28`,
then `+0x34/+0x38/+0x19`; v0x50 sets the final flag to1. Sync enable is
derived from the note value exceeding f32 approximately1e-9. Writer
`0x140d12460` emits the v0x51 tail and saves f32 -1 when sync is disabled.
The two appended floats and flag remain opaque rather than inferred defaults.
Sibling source-reader tracing independently closes the parent geometry:
`BParInternalMod` vtable `0x144e74ce8`, private reader `0x140d09b70` through
`0x140a8492e`, retains saved category0..6 in `+0x53c`. Child reader
`0x140d062d0` case5 chooses Glide at `+0xa68`, its vtable `0x144e74b80`,
chunk getter0x0b, and direct reader `0x14093d6c0`. Glide is saved category5
with direct child0x0b. Category2 instead chooses envelope wrapper7 at
`+0x5f8`; wrapper reader `0x140d0d420` accepts only its own kinds0/1/2.
The runtime factory's Glide kind5 does not imply wrapper7 public kind5.

Internal kind5 dispatch `0x140977983..0x1409779b6` reaches the actual Glide
binder. Parameter `+0xa70` mode0 stores runtime byte `+0xb0=0` at
`0x140977a89`; mode1 stores `+0xb0=1` at `0x140977a2a`. Both pass the
f32 coefficient from parameter `+0xa74` through timing object `+0xa78`,
virtual `+0x28`, and store returned f32 timing at runtime `+0xb4`
(`0x140977a22/0x140977a81`). The nearby `+0xbc/+0xc0` stores at
`0x140977b0d/0x140977b4c` belong kind4 and are excluded. Source-kind
encoder `0x1406ddfc0` independently returns `parameter[+0xa70] | 0x10500`
for kind5, so its encoded source identity includes mode.

Timing copy `0x1407f7af0` installs vtable `0x144e74260`; its `+0x28`
wrapper `0x14072b380` calls `0x140a74bf0`. Disabled timing byte `+8`
returns the coefficient verbatim. Enabled timing uses ordered f32
`((note_value * 60) / BPM) * coefficient * 1000`, with coefficient
clamped to at least1 when timing byte `+0x20` is set, then the supplied
lower/upper bounds. BPM below f32 `0.1` substitutes120. Glide construction
`0x1407035ec/0x1407035f0` supplies bounds0 and2500ms. Saved parameter
`+0x28` is timing note value `+0x18`; the saved derived byte `+0x18`
becomes its enable byte `+8` (threshold approximately1e-9). Version0x51
saved `+0x34/+0x38` become timing `+0x24/+0x28`, and saved byte `+0x19`
becomes timing byte `+9`; this conversion does not consume them.

Initializer `0x140cebba0..0x140cebc37` reads those exact runtime mode/timing
fields and seeds the same state consumed by waveform `0x140ceb090`.
It clamps the supplied signed integer interval to `[-12,12]`. Let `d`
be the clamped interval, `a=abs(d)`, and `t=runtime[+0xb4]`. Mode0
multiplies `t` by `a`; mode1 leaves `t` unchanged. It stores output scale
`+0xc0 = f32(a) * f32(1/12)`, state `+0xb8=-1`, and step
`+0xbc = 1 / ((t * f32(0.001)) * source_rate[+0x14])`. Negative `d`
changes state to+1 and negates the step. Thus mode0 is Speed (duration
scales with interval), and mode1 is Time (fixed duration), as independently
described by the NI reference. This is the source law; it does not yet prove
the final pitch target's conversion. Literal values are read from
`0x144737724` (f32 `1/12`) and `0x1447148a4` (f32 `0.001`).

The exact direct call at `0x14097c539` passes
`container[+0x458] - container[+0x454]`. It fetches that container from
`program[+0x2d7f8]`, table `+0x18`, using the source-owning voice's
`+0x8d4 / 128` (negative maps to-1). Source objects there are copied from
the prepared `+0xd68` table, with16-byte entries; the initializer is reached
for a newly copied source with source flag `+0xa4 & 0x10`.

The container is specifically RTTI `BProgramContainer`, not an inferred
MIDI-channel cache: constructor `0x1407f9c10` installs vtable
`0x144e983d8`, locator `0x1498a50f8`, and at `0x1407f9c6e` initializes
the prior/current pair together to-1. Manager construction
`0x1407f2df0..0x1407f2e3b` builds64 such table entries. Native note
dispatch `0x1408f4520`, called at `0x140950c36`, fetches the same table
entry using event `+0x18 / 128`. Key accessor `0x14075c940` returns-1
when event byte `+0x3d` is nonzero; otherwise it returns event byte
`+0x41 & 127`. For a nonnegative key, `0x1408f4614..0x1408f4631`
sets prior=current key when prior is negative, otherwise copies old current
to prior, then stores the new current key. This happens before the per-group
script loop at `0x1408f476d`, and before the later key/velocity range checks.
The resulting history is shared by those downstream groups. The first
eligible update therefore yields interval0, rather than an invented prior key.

The final pitch destination is independently closed by the sibling target
trace. Registry `0x140a6d4a2/0x140a6d4a9/0x140a6d4b5` names pitch and
registers kind6. `BGlideWithTargets<0>` getter `0x140887570` returns its
one88-byte target at `+0xc8`; common factory `0x14097639b..0x1409763e3`
copies saved kind, slot, flags, and depth. Target expression builders
`0x140873040/0x1409608f0` use bipolar pitch template
`0x144f1db68` (`$out := $in + {} * $intensity`). Unshaped signal template
`0x144f1d7d0` is `$signal`, and zero lag omits the lag recurrence. Signed
operator `0x14071a7c0` tests context `+0x6d` bit2 at `0x14071a7d3`, then
flips the signal sign at `0x14071a7e3`; context is target entry `+0x10`,
and cloned saved flags propagate to entry `+0x7d` in `0x14097c350`.
The distinct unipolar operator `0x14071a760` applies1-x and must not
replace that bipolar sign law.

Common apply `0x1409519e0` uses destination
`context+0x2810+kind*0x100`, so pitch kind6 reaches `+0x2e10` and
scheduler `0x1409163d0`. Consumer `0x14091699f` retains modulo32 offset.
Interpolator `0x1408e4110` writes
`previous + (current-previous)*(offset/32)` to `context+0x1030`; its
multiplied-buffer branch is separate, while pitch uses overwrite. Source
rate is audio rate times f32 `0.03125` at `0x140976465..0x140976471`.
Voice reset clears previous/current `+0x3c8/+0x3cc` at `0x14094d70d`,
initialized flag `+0x3d0` at `0x14094d714`, and modulo offset `+0x500`
at `0x14094d79a`. First use seeds current from source point0; at offset0
previous and current both become point0. The first32 audio frames therefore
hold point0, then the next interval interpolates point0 to point1. There is
no initial zero-to-point0 ramp.

The pitch endpoints belong to the combined kind6 destination, not to each
Glide source. Scheduler `0x1409163d0` skips a bypassed source before its
waveform and common target apply, while another active pitch source keeps
the shared destination valid. An off-tick bypass therefore retains the old
combined endpoint until the next modulo32 boundary, then interpolates toward
the newly emitted total; restoring a source likewise interpolates the shared
total rather than a private stale endpoint. KONTRA's Glide clock retains only
per-slot level, step, and scale. Its points join LFO points in the existing
retained pitch clock before normalized-octave interpolation. With all pitch
sources bypassed, the destination contributes no pitch, source state pauses,
and the note offset and retained endpoints survive. Prepared Glide groups
use deterministic slot-order accumulation; the native prepared dispatch-list
order has not been independently equated with that order. LFO-only arithmetic
and the existing separate generic pitch-envelope path remain unchanged.
The staged exterior PCM fixture exercises two Glide sources with bypass at
frame16 and restoration at80, including complete note lifetime and fragmented
rendering. Compilation and execution remain the central validation owner's
responsibility; this review performed neither.

Ratio conversion `0x140832440` modifies that octave buffer in place, and
ordinary samplers `0x140889570/0x140865c70` multiply their nominal step by
it. Native conversion uses a polynomial/bit approximation with127,
approximately0.33971, and8388608; standard-library `pow2` is not claimed
to be bit-identical. The intended source subset retains that existing core
conversion boundary rather than calling source and sampler arithmetic exact
Kontakt PCM parity. It requires retrigger1, direct category5/child0x0b,
positive finite coefficient, disabled sync, known source flags, pitch-only
finite saved magnitudes and flags0x10/0x12, no independent invert, zero
lag, and no shaper. Unknown timing, shared-retrigger lifetime, and additional
target transforms remain explicit exclusions.

Packed destination creation is now closed. In ingress `0x14086c670`, the
automatic route builds `outer_container_index << 7` at `0x14086c73d`, adds
the selected inner program at `0x14086c765`, and stores that packed value in
event `+0x18` at `0x14086c9c4`. MIDI channel is separately stored at `+8`
(`0x14086c96d`). Thus the history belongs to the rack/program container,
shared across its inner programs, MIDI channels, and downstream groups.
Event-parameter channel setter `0x1409a92d3` changes `+8`, not `+0x18`.

Native NOTE literal7 is independently registered as documented in
`docs/CORE_NATIVE_CONTROLLER_EVIDENCE.md`. Its runtime setter
`0x1409a9258..0x1409a926e` saves the original note once at event `+0x50`,
then calls `0x14076a7a0`. That setter changes the mutable note byte `+0x41`
and negative-note flag `+0x3d`. The history dispatcher reads this note before
on-note callbacks: `change_note` affects playback, but does not recapture the
history pair. The initializer still reads `current-prior` from the live
container at source creation, rather than a played-voice note or an interval
snapshotted when the input arrived.

`ignore_event` validation at `0x140793d9a/0x140793db2` assigns AST command69
and names its callback restriction. Runtime `0x140798a24..0x140798a49`
indexes command-minus-one in RVA table `0x1407a0214`; entry68 selects
`0x14079aa26`. Its call at `0x14079aa42` reaches `0x140955a20`, which
stores callback index into the matching event's `+0x2c`
(`0x140955a89/0x140955aa9`). History dispatch excludes an event already
marked at entry (`0x1408f453d`), but the pair update precedes callbacks;
ignoring inside a callback does not undo it. Wait resumption is dispatch
kind2 calling `0x140798060`; first note dispatch is kind4 calling
`0x1408f4520`. Resuming or forwarding a callback must not observe again.

Generated `play_note` uses that same first-dispatch path. Its named branch
`0x14079ec78` allocates through `0x1408e2fc0` at `0x14079ed24`, into the
program's `+0x14048` pool. It sets packed destination `+0x18` at
`0x14079edcb`, source callback indices `+0x1c/+0x24` at
`0x14079edfa/0x14079ee01`, and note packet through `0x14076a370` at
`0x14079efd5`. Main dispatch `0x14094fe20` reads that pool's head at
`+0x14050`, enqueues due rows as kind4 through `0x1408e25b0`, and reaches
the same history writer. There is no physical-root-only gate on that writer.

Checked note-off path `0x1408f4160..0x1408f4516` does not write the pair.
Manager reset `0x1408669a0` calls container reset `0x14094ed20`, which clears
its inner programs through `0x140830880`; those checked paths retain
`+0x454/+0x458`. Container bind `0x140959e70` only changes its owner pointer.
Constructor initialization remains the proved reset. The Glide initializer
has no key-held or pedal test; pedal logic must not substitute a MIDI history
owner. This does not claim every unexamined native global reset path.

KONTRA retains one `NoteHistory` per Player. Runtime emits one ordered
`ObserveNote` command before the first accepted root/generated note callback,
and marks the event across waits and slot forwarding. This preserves history
when a callback ignores its note while keeping the live pair in order with
queued voice starts. Cancellation retains already-emitted observations;
bank/script/rate handoff drains surviving observations in their stable command
order before clearing the queue. Direct unscripted starts observe before zone
selection. No physical or mutable delivered-channel cache supplies the pair.
Observation commands have their own transient256-row quota and exactly256
additional prepared command slots, preserving the existing256 ordinary-start
quota. Overflow uses the existing dropped-command diagnostic and never
fabricates a history update. This bounded ordered-command bridge is separate
from the native engine's immediate first-dispatch history mutation; compact
timeline storage is a later ownership change, not part of the source law.
Public direct starts and unscripted input admission first consume preceding
due-at-zero writes and commands using the renderer's write-before-command
policy. Applied rows remain marked in the same queue, so the next render cannot
replay their starts or history observations. Future waited work remains future.
The input paths perform this admission before recording the incoming held root,
preserving the order of an earlier CC66 capture and a later note. The complete
PCM proof `glide_direct_admission_tests.rs` compares mixed queued/direct starts
and a waited child with a route-free Engine driven by independent tuning knots;
it is staged, not executed here. The 6144 prepared command rows reserve at least
27,746,304 bytes per Engine: each tagged row accommodates the Start variant's
4004-byte script-source array and 512-byte group mask, before scalar fields and
alignment. Static inspection of the supplied current CI rlib's
`RawVec<engine::script::Command>::grow_one` gives alignment 8 and element size
`0x12b0` (4784 bytes), hence 29,392,896 bytes (28.03125 MiB) for the 6144 rows.
The rows comprise 256 ordinary, 256 stop, 1280 freeze (1024 voices plus 256
starts), 256 observation and 4096 release entries. This is the compiled artifact's
layout, not a claim that Rust guarantees the enum's layout across toolchains.
That measurement describes the preceding inline queue. The compact
`commands.rs` owner now retains small timeline rows and a single prepared pool
of Start/Release snapshots. Both snapshots include all 1001 script values;
there is no claimed reduction to a 6 MiB worst-case reserve. Storage is reserved
off the audio thread and slots are initialized only when used. Applied rows
retain their unique private handles until removal; clear and every cancellation
retain return snapshots through the same owner. Timestamp insertion and quota
scans therefore move/read small rows, without changing snapshot values, admission
bounds, or source/audio arithmetic. Performance is not benchmarked here.
Retrigger-disabled shared-source lifetime and opaque synchronized timing remain
outside the intended strict Glide subset. Additional matching-offset candidates
were excluded: `0x1436ddb0c/0x1436ddb5f/0x1436ddb7d` scatter a strided float
array, and `0x140904db5` writes a stack temporary in parallel history buffers.

## Saved AHDSR AHD mode and retrigger boundary (2026-10-04)

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
defines AHD Only as an attack/hold/decay source which completes independently
of key duration. Retrigger enabled restarts on each accepted note; disabled
keeps the source position until the last held note is released. These are
separate switches and do not identify a generic envelope model by name alone.

Bounded inspection of the same installed Kontakt8.13.1 executable used above
closes the saved AHD byte through the actual source state:

- `BParEnv_AHDSR` type/version getters `0x1407625e0/0x1407626d0`
  return chunk0x3f/version0x11. Reader `0x140cffb40` consumes the six
  scalar floats then stores the next byte at concrete parameter `+0x100`.
  Its version0x10 branch stores the same byte, but this is not an admission
  of that older serialized timing layout. Writer `0x140d0fd50` writes the
  same six scalars then that byte before the timing records.
- XML parser `0x14092473b..0x14092477a` compares `noteOffLessMode`
  with `yes` and stores wrapper `+0x110`. The AHDSR child is wrapper
  `+0x10`, so this is the same concrete `+0x100` field. The internal
  parameter constructor puts that child at record `+0x608`, making the
  saved AHD field record `+0x708`.
- AHDSR source setup `0x1409653db..0x140965563` converts the four
  saved timing records, loads record `+0x708` at `0x14096555c`, then
  calls source virtual `+0xb0`. For the identified AHDSR target class,
  that entry is `0x1405f0980`, which stores runtime mode byte `+0x118`.
- Release `0x1406030a0` records note-off status but only schedules the
  release countdown `+0x10c` when mode `+0x118` is zero. In AHD mode,
  decay setup `0x140ae5e40` uses zero offset and unity scale, disregarding
  the stored sustain. Waveform `0x140ae4332..0x140ae4396` advances
  phase0/1/2 then jumps directly to phase5. Phase5 setup clears output
  scale; `0x140ae43a8..0x140ae43bb` sets done byte `+0xb0`, which
  done virtual `0x1405edd60` reads. Thus early note-off does not select
  the saved release stage, and natural AHD completion publishes zero.

Retrigger is independently identified by the native internal-modulator
reader `0x140d09be6..0x140d09c18`: the third saved switch is record
`+0xab8`. XML `0x14092c060..0x14092c0ac` names that same store
`retrigger`. Native source preparation `0x14088b16f..0x14088b186`
retains a state pointer at runtime `+0x50` when retrigger is disabled and
runtime flags `+0xa4` bit7 select the shared branch. The corresponding
source setup `0x140977c95..0x140977cbd` marks runtime `+0xa0`, and
`0x140977ee0..0x140977ef2` skips the source reset virtual on that
branch. Cleanup `0x140978404..0x14097842f` also tests this saved switch
before resetting its shared container. AHDSR state-copy virtual
`0x1405e4fc0` copies phase, current scalar, remaining count, release
countdown, scale/offset and note-off status. This confirms additional
shared ownership; it does not yet prove the complete last-held-note,
pedal, channel or group history policy. Retrigger0 remains unadmitted.

The current primary amplitude admission already consumes this saved AHD
flag with the finite native source kernel. Nonvolume pitch/module AHDSRs
retain the raw byte but prepare through generic `Ahdsr::from`, which
selects ordinary AHDSR. The next bounded slice is an explicit per-envelope
saved AHD admission marker, defaulting false for authored objects and
older caches. Its gate requires the existing v90-kind0/category2 wrapper,
v11 child, raw AHD byte1, four exact default timing records, known source
switches with retrigger1, and supported pitch/module routes. Both prepared
consumers receive the same marker; source bypass remains separate from the
running clock. Unsupported flags retain metadata and a diagnostic.

Production now retains `ModEnvelope::native_ahd_only` with a false serde
default and applies it to both prepared pitch/filter copies. The admitted
target subset is pitch, cutoff and Q with finite intensity, flags0x10,
default target metadata, no invert, lag or enabled shaper. Saved source
bypass0/1 is retained independently. Other source/target geometries keep
their raw metadata and explicit unsupported diagnostics.

This slice establishes saved mode/release semantics over the existing
generic pitch/module arithmetic. That arithmetic still differs from the
native finite-count source and its curve recurrence. Enabling the saved
mode must not claim native source timing, curve or complete Kontakt PCM
parity. Flex production is unchanged. The staged
`saved_mixed_ahd_preserves_early_release_and_complete_pcm_without_heap`
gate carries an authored saved mixed pitch/cutoff source through parser,
cache, preparation and complete PCM. Its independently deferred key-off
reference enables AHD through the existing explicit parameter control.
Scalar closed-form bounds describe the existing generic waveform only.
The scenario covers cache defaults, rejected flags/timing/wrappers,
physical key-up, sustain/sostenuto, source bypass, mute, overlap retrigger,
empty/irregular calls, shared rendering, natural amplitude end and sample
EOF. No build or test was executed by this evidence author; the central
compiler and execution gates remain owned by the root task.


## Known KSP listener initialization inputs (source audit, 2026-10-04)

The previously reported `set_listener($NI_SIGNAL_TIMER_MS,-1)` initialization input is supported in current source: `builtins::constant` resolves the signal to integer1 before compiler auto-symbol fallback (`src/ksp/builtins.rs:700`, `src/ksp/compile.rs:924–946`), and `src/ksp/calls.rs:911–961` normalizes this legacy MS-only sentinel to disabled0. Positive MS timers require at least1000µs; beat and transport signals retain their separate validation. This matches the [NI documented timing units and disable0 contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands), with −1 retained as the observed legacy compatibility input rather than a newly inferred general sentinel.

Saved script text/linked-resource fallback and active persistence rows share the same extraction predicate in ordinary and snapshot import (`src/import.rs:674–686`, `871–875`, `472`). Runtime listener generations and scheduling are independent for MS/beat (`src/ksp/runtime.rs:2864–2913`, `2975`); `$NI_SIGNAL_TYPE` reads the dispatched context (`src/ksp/vm.rs:1297`). Existing staged coverage includes initialization, transport, disable/re-enable (`src/ksp/tests.rs:3062–3120`) and complete Engine timer DSP (`src/engine/mod.rs:2432–2563`). This was a read-only source audit: no tests were executed and the original Strings High Bartok Pizzicato Col Legno 8va/HStr Power Sustains instrument files were unavailable, so actual library initialization remains unverified.


## Prepared LFO source ownership admission (2026-10-04)

`engine::lfo::validate_sources` now guards both `params::validate_modulation_capacity` before authored Group conversion and `VoiceRoutes::prepare` before any route allocation. The latter is shared by the ordinary builder and public consuming `Bank::with_prepared_settings` edits. Fixed sixteen-slot phase/fade/volume storage requires in-range slots, one pitch row and one volume row per source slot, and identical paired source phase, timing, sine/Digital waveform metadata, fade and bypass. Target depth/indices and volume intensity, polarity and lag are consumer state and are excluded from source equality. Nonfinite processing fields and negative signed volume lag reject preparation; Digital requires phase0..1 for its verified strict single-wrap geometry. Legacy finite sine normalization, frequency clamps, finite cycle-phase flexibility and nonpositive-fade behavior remain unchanged. No callback validation/allocation framework was added.

Concrete public failure paths before this guard were a consuming edit to slot16, which returned a Bank later indexed into sixteen-element source arrays, and a negative volume lag reaching the prepared-target `expect`. Duplicate pitch rows could advance one legacy phase twice, duplicate volume rows reused one lag state twice, and differing paired Digital records consumed whichever waveform populated the shared point cache first. The raw native importer already emits one coherent source copy per admitted consumer; indexed current consuming-edit callers are test fixtures, so these are confirmed public preparation edges, not an observed faulty library.

Staged preparation rejection covers slot bounds, duplicate rows, each shared source field family, nonfinite inputs, negative volume lag and Digital phase. The existing complete Engine PCM fade proof also passes one engine through a valid consuming edit: equivalent legacy phase1.25/count0.5/noteValue1/12/sine3 and unequal pitch versus unused volume-copy depth/target metadata retain the independent waveform oracle, fixed/irregular/shared render checks, mute/bypass/retrigger/release, exact ends and callback heap guard. No test was executed here.

A separate public seed-identity gap was found: consuming settings edits preserve authored `Bank.groups` and `InstrumentState.base`, while residency compatibility compares authored source records and route topology. An incoming prepared source customization could therefore be discarded as an unused fill seed. Successful `with_prepared_settings` now marks the incoming bank as explicitly edited, including no-op closures; such incoming seeds use full installation. The provenance check applies only to the incoming bank, so genuine worker fills preserve an earlier customized installation and its live edits. Valid prepared customization remains available without a new source-signature representation. A complete PCM scenario compares a prepared-only Digital source change with independent explicit installation; it is staged and unrun.

## Saved wider Flex sustain loop admission (2026-10-04)

The [NI Flex documentation](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) distinguishes a sustain section with an interior breakpoint from the single-segment hold, and says that release enters the section following sustain when Oneshot is disabled. Both Loop and Oneshot enabled remain a separate indefinite mode.

The installed Kontakt 8.13.1 binary and hash are the same primary artifact identified above. This slice reuses the closed FM7 v12 reader/XML/constructor/setup chain: reader `0x140cffef0` reads loop-end, loop-start and last-point indices at concrete `+0x188/+0x18c/+0x190`, followed by separate Loop/Oneshot bytes at `+0x194/+0x195`; record copy `0x1407f8ae3..0x1407f8b1b` and setup `0x140965390..0x1409653d0` place those fields at runtime `+0x2fc/+0x300/+0x304/+0x321/+0x322`. The parser reads precisely last-index-plus-one points, so the last serialized point is the saved native terminal. No flag is inferred from the opaque timing record.

A bounded `pdg` of segment initializer `0x140ae5450` closes the loop law. It reads current level at `+0x308`, computes the segment toward the selected target, then, when the selected index equals loop-end and Loop is nonzero, substitutes loop-start before writing next-index=`index+1` to `+0x314`. It does not reset current level to the stored loop-start level. Therefore the completed endpoint returns into the segment after loop-start from its current endpoint level. Release `0x14096be80..0x14096bea8` with Oneshot zero sets remaining-count to one and next-index to loop-end+one; it does not select loop-start. The existing terminal next-index-greater-than-last gate remains unchanged. The bounded source capture is `/tmp/kontra-native-flex-loop-proof.txt`; no native library or audio was executed.

Production admits only the known v90-kind1/category2 wrapper, v12 Loop=1/Oneshot=0, a wider section with at least one interior point, valid start/end indices, zero final level, default unsynchronized timing, active retriggered source, and one uninverted unity volume target with zero lag and no enabled shaper. A serde-default `Group.native_flex_loop` marker survives cache/import, feeds prepared `Flex.loop_start`, and participates in bank source identity. The shared Envelope endpoint transition selects start+one; `None` keeps the previous sustain/Oneshot behavior. Release retains the existing post-loop selection. Unknown modes, single-segment loops, Loop+Oneshot, bypass and retrigger-zero remain explicit unsupported diagnostics.

This is mode admission over the existing Flex scalar kernel. Native startup at five milliseconds, native point-rate/interpolation, curve recurrence and release-duration correction (including the one remaining source point after release) are still equivalence gaps. In particular, the existing kernel's full authored post-loop segment duration is not claimed to match native level-dependent release timing.

The staged, unexecuted `saved_flex_loop_preserves_return_release_and_complete_pcm_without_heap` gate passes actual raw objects through typed decoding, cache serialization, preparation and Engine. Its independent frame-domain linear oracle uses unequal loop-start/end levels 1/.75, repeated 96/144-frame sustain segments and a 192-frame post-loop segment. It compares full 1280-frame stereo PCM against manually computed source levels and a separately deferred physical release; an ordinary held-Flex engine must materially differ. Shared/rendered/muted clocks, irregular/empty fragments, sustain pedal, duplicate physical key-up, same-pitch exact-owner retrigger, terminal boundaries 905/1045, sample EOF and explicit SoundOff are covered. This gate checks the current kernel's mode wiring and lifetime, not native scalar timing parity. No Cargo/build/test was executed by this author; the root task owns the central gates.

Saved amplitude bypass was investigated but not admitted. Native scheduler `0x14091660f` gates source waveform/application on source `+0x0c`, so bypass skips waveform advancement as well as the target. Voice terminal scan `0x140946a88..0x140946b51` identifies target ID2 (volume), checks source done through virtual `+0x88`, and then requires source `+0xa4` bit1/bit2 before lifecycle completion. The exact class flag and bypass/note-off lifetime bridge remains open. A generic running-but-muted envelope would not represent the proven native scheduler behavior.

## Saved LFO frequency conversion and unsynchronized Hz (native closure, 2026-10-04)

The [Kontakt modulation reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) distinguishes synchronized note-value counts from unsynchronized frequency and retriggered from free-running phase. Exact timing below comes from the same installed Kontakt 8.13.1 binary; the manual's approximate frequency range is not used as an arithmetic constant.

The BParLFO v71 reader `0x140d03660` reads coefficient at concrete `+0x10` and frequency timing at `+0x20` through `0x140ab5da0`. That timing reader derives enabled byte `+8` from saved note-value `> f32(1e-9)`; its serialized trailing byte is the separate opaque `+9`, not the sync switch. Constructor `0x1407033e9..0x140703435` installs frequency timing vtable `0x144e74208` and bounds with exact float bits `0x3c2c0831` and `0x4355199a` (approximately .0105 and 213.1 Hz). Factory `0x140977ce4..0x140977d35` invokes its virtual `+0x28` (`0x14072b330`) then installs the resulting frequency through `0x14095ecd0`.

The called converter `0x140a736a0..0x140a736f5` returns the coefficient unchanged when synchronization is disabled. Enabled timing uses BPM with the native low-tempo fallback120, the default coefficient-minimum flag's `max(count,1)`, and ordered f32 `((note_value * 60) / BPM) * count`; its reciprocal is then clamped to the exact constructor bounds. Source rate installer `0x1406068b0` computes a double reciprocal of its f32 control rate and narrows to f32 at `+0xc0`. Frequency installer `0x14095ecd9..0x14095ecf9` multiplies frequency by that reciprocal in f32, widens the product to double, and stores step at `+0xb8`. Digital phase advancement must retain this product rounding; multiplying frequency and reciprocal as doubles changes phase.

Production reuses the existing source metadata and clocks. The known v71 retriggered sine-only Multi and deterministic Multi Digital admission now also accepts finite unsynchronized Hz within those native constructor bounds, retaining the saved raw note value and coefficient through cache. The existing default opaque timing flag, phase bounds, unsynchronized fade and target restrictions remain. Sync count admission still requires count at least1; no external frequency/phase assignment, shared free-running source or live timing control is admitted. The source frequency helper and Digital step implement the traced operations. Sine retains its existing analytic phase and sine kernel approximation; exact native analog-table phase/shape parity remains open. Native source dispatch order and shared free-running ownership are also unproved. The compact bounded disassembly is `/tmp/kontra-native-lfo-timing-review.txt`; no native audio or Cargo/test was executed by this author.

## Primary amplitude source bypass and release ownership (native closure, 2026-10-04)

The [KSP engine-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters) identifies `ENGINE_PAR_INTMOD_BYPASS` as the internal-source bypass control. It does not specify source clock or voice termination laws; the following are observations from the same installed Kontakt 8.13.1 binary, not inferred general envelope behavior.

The earlier class-flag gap is closed. AHDSR base constructor `0x1407f6b30` ORs source `+0xa4` with2 at `0x1407f6b5a`; Flex base constructor `0x1407f6bc0` does the same at `0x1407f6bea`. Their actual WithTargets0 constructors call those bases at `0x1407bee9e` and `0x1407c049e`, respectively, before installing the proven target-bearing vtables. Thus the bit1 gate in volume-source completion scan `0x140946ae8..0x140946b09` belongs to these admitted sources. That scan calls source done through virtual `+0x88` and retires a done source with volume targetID2; it does not exclude bypassed sources. Bypass cannot resurrect an already completed amplitude source.

Held bypass has a separate consumer law. Source setup copies saved record `+0x540` to source `+0x0c` at `0x1409651bb..0x1409651d0`. Scheduler `0x14091660f..0x140916613` skips waveform and target application for nonzero `+0x0c`. Target application `0x1409519e0` registers target2's point buffer for the current generation. Amplitude consumer `0x140916eed..0x140917126` loads unity at `0x140916f09..0x140916f19`, requires matching generation `+0x2a18` and a nonnull pointer `+0x2a10`, and otherwise jumps to `0x140917126`, bypassing the amplitude multiplication. The active path interpolates source points with state at voice `+0x3b0/+0x3b4` through `0x1408e3d40` or `0x1408e4110`; the absent-buffer path skips those calls. A lone bypassed primary source therefore contributes neutral amplitude while preserving both source and consumer interpolation state. There is no newly computed ramp toward unity in that path. Other simultaneously active target2 sources require their combined-buffer law to be considered separately.

Physical note-off is not skipped with the waveform. Native note-off funclet `0x14086afab..0x14086b313` dispatches source virtual `+0x80` at `0x14086b193..0x14086b1ad` regardless of source bypass, except for the separately unadmitted shared/retrigger-zero branch. It then scans envelope-class bit1/bit2 and volume targetID2. Only a source whose `+0x0c` is zero supplies amplitude release ownership (`0x14086b203..0x14086b213`). If none does, the voice examines native sample-loop records; a type1 record returns without the ordinary termination branch. After the terminating loop scan, an ordinary sampler source sets phase3 and termination `+0x8fc` at `0x14086b297..0x14086b29f`; source flag `0x1000` instead retains a distinct fallback phase with saved10ms at `0x14086b2bf..0x14086b2ec`. Those native loop/source classifications and their exact final-sample bridge must be mapped before claiming general bypassed-note-off support. Pausing the source indefinitely after physical key-up would contradict the ordinary termination branch.

Current import does not omit a saved bypassed primary AHDSR/Flex: it retains the raw envelope plus `Modulator.bypassed`, refuses native admission, and prepares the generic amplitude fallback without applying that saved switch. Its existing unsupported-source diagnostic exposes the gap; it is not saved bypass support. Live `INTMOD_BYPASS` resolution likewise excludes the primary amplitude envelope and Flex, so it currently cannot report a successful primary bypass write without DSP. A concrete next API would carry a proven primary source slot/bypass pair in prepared GroupSettings, resolve a separate amplitude bypass address only for the admitted source geometry, pause source and interpolation with neutral output in every rendered/skipped/lane path, retain done-source retirement, dispatch release while paused, and transfer release ownership to the verified sampler termination path. Unknown loop/special-source classifications must remain outside that successful API until mapped.

Bounded artifacts: `/tmp/kontra-native-envelope-ctors.txt`, `/tmp/kontra-native-envelope-off-function.txt`, `/tmp/kontra-native-amp-bypass-unity-interp.txt`, and `/tmp/kontra-native-amp-bypass-retire.txt`. This section is evidence only: no new amplitude bypass production/API was admitted, no native audio/library was executed, and no Cargo/build/test was run here.

The ordinary phase3 consumer is now closed as well: `0x140916c78` branches on phase3 to `0x140916c90..0x140916cd8`, sets voice `+0x510` zero and clears each output channel's0x1000-byte buffer, skipping the sampler render virtual call at `0x140916d42`. This path contains no invented release fade or interpolated amplitude tail. Before subsequent voice dispatch, the active-voice list at context `+0x11ed0` is walked by `0x140906c70..0x140906c8f`; a nonzero voice `+0x8fc` invokes the manager's retirement virtual `+0x70`. Therefore the ordinary unlooped bypassed-source note-off branch yields zero for the next native processing fragment and is retired before a subsequent dispatch. The native physical-event offset to scheduler-fragment boundary is not yet a verified sample-exact equation. Saved type1 loops, the `0x1000` special source and combined amplitude destinations remain excluded from this narrowed observation. Bounded captures are `/tmp/kontra-native-phase3-consumers.txt` and `/tmp/kontra-native-phase3-actual-retire.txt`.


### Saved primary bypass quarantine and native scheduling boundary (2026-10-04)

The direct release entry is `0x14086aea0`; the previously inspected `0x14086afab` is a chained-unwind span within that same routine. A bounded `.text` call scan found one direct caller, `0x140906926`. Its surrounding manager branch `0x140906905..932` tests voice `+0x509` pending release and compares remaining offset `+0x50c` with the current processing length in `r15d`: an offset at or beyond the length is decremented; an offset inside the fragment calls release before the later voice-retirement scan. Combined with `0x140906c70..c8f` and the phase3 zero-output branch, this establishes fragment-level stop dispatch, not a sample-accurate host note-off equation. The event-to-offset setter and native fragment scheduling quantum remain unadmitted. Capture: `/tmp/kontra-native-envelope-off-fragment-entry.txt`; candidate setter capture `/tmp/kontra-native-envelope-off-offset-setter.txt` is not a closed ordinary note-off bridge (the inspected `0x1408f4451` branch is gated by source flag `0x1000`).

The separate saved-loop mapping is now established by the native BLoop v60 reader/writer and voice preparation: native types `0/3/4/1` mean saved Inactive/UntilEnd/UntilRelease/OneShot, respectively (see `audits/COUNTED_LOOPS.md`). Type1 in the release scan is therefore a saved sample OneShot exemption, and cannot be generalized to ordinary looping or an arbitrary envelope lifetime. The bounded future live-amplitude bypass subset still requires ordinary sampler ownership and closed scheduling; no successful primary bypass getter/write or runtime DSP bypass is introduced here.

Until that law is admitted, bank construction excludes zones whose selected primary AHDSR or Flex source is saved bypassed, using the existing `UnsupportedSource` accounting. AHDSR keeps the existing `Modulator.volume_env` owner meaning. The new serde-default `Modulator.primary_flex` records only the first Flex actually selected into `Group.flex_env`; `Modulator.flex` continues identifying every Flex roster source and does not imply amplitude ownership. The diagnostic includes source kind, native slot/name, original zone ID, group, sample and the unsupported bypass/release reason. Existing envelope/modulator/group metadata stays in the imported instrument and cache, and valid siblings remain playable. An all-excluded bank fails with its existing no-playable-zones diagnostic. The same narrow owner predicate in `bank::play_map` prevents zone-edit preparation from assigning a valid zone into an excluded source group; direct resident and prepared-settings paths cannot recreate excluded zones.

A staged actual NKI/cache scenario in `src/creator/nki_envelope_bypass_tests.rs` writes and then structurally rewrites the native container, retaining primary AHDSR/Flex bypass, a preceding non-volume Flex, and a supported bypassed pitch AHDSR sibling. RAM, bare/cache, direct resident and explicit prepared-settings banks exclude exactly the two amplitude groups and preserve original group metadata. Complete stereo PCM, initial absence of voices/sound for the excluded keys, audible sibling playback and completed release-tail silence are compared against a separately retained sibling-only NKI bank across regular and irregular fragments. Initial zone-worker preparation rejects a sibling-to-excluded-group remap. This is staged source evidence and an unexecuted scenario, not a claim of native audio execution, runtime bypass parity or completed tests.

### Loaded BSample bypass release and event offset (native closure, 2026-10-04)

The preceding interpretation of sample flag `0x1000` as a special source mode is superseded by the actual object/voice bridge. Successful PCM storage creation `0x1408df2e0` and sample preparation `0x140865fb0` set object `+0x80` bit `0x1000`; the former initializes format, sample rate, channels and frames at `+0x1788/+0x178c/+0x1790/+0x1794`. The constructor at `0x1407fa0e3` installs the BSample vtable `0x144e94b88` (RTTI BSample descriptor `0x14a3a9f78`), clears `+0x80`, and initializes these same fields. Voice preparation `0x140968ca0` reads zone `+0xa8`, tests that object's readiness bit, requests loading when absent, and installs that same pointer in voice `+0x558` at `0x140968f35`. Consequently an ordinary loaded sampler voice has this bit; the immediate phase3 path describes missing/not-ready sample data, not the admitted loaded unlooped baseline. This correction does not change the existing quarantine.

For an unlooped loaded sample with no active amplitude-release owner, release entry `0x14086aea0` initializes the separate voice fade: current gain `+0x540` is copied to starting gain `+0x550`, delay `+0x538` receives pending note-off offset `+0x50c` minus voice onset `+0x170`, phase `+0x184` becomes1, duration `+0x53c` is10ms, remaining `+0x548` becomes−1, and stop `+0x52e` is enabled. Source release virtual `+0x80` was already dispatched even when the source is bypassed; a bypassed source does not supply the amplitude ownership gate. Saved sample OneShot and other loop cases retain their separately diagnosed boundaries.

The fade consumer `0x14086d3c0` closes the audio law. It emits the previous scalar gain unchanged for the delay; initialization uses `N=max(2,trunc(10.0f32*0.001f32*render_rate+0.5f32))` and linear step `(-1.0f32/(N−1))*current_gain`. Each fade frame emits the current gain first, then adds the step, clamps0..1, and decrements remaining. Exhaustion changes the voice phase to3 when stop is enabled, filling any remaining span with its resulting gain. The voice initializer sets both gain fields to1 at `0x14094d8c2..d3`. Main render caller `0x140917d80..dcf` supplies the voice's actual render span and multiplies the produced buffer into sample audio; its flat-gain branch is used while the delay exceeds that span. This arithmetic differs from the current generic Fade's update-before-emission, division byN, and forced endpoint; reusing that Fade would not establish native parity.

The release timestamp bridge is also closed for the native queue. Wrapper call sites `0x140d38b71/0x140d38d0f` call `0x14094fe20` with the same processing length subsequently passed into manager `0x140906000`. Queue dispatch represents offsets in1/1024-frame units: it compares the release record's DWORD0 against processing length times`0x400`, subtracting that amount when the record is outside the span. Eligible release queue kind1 invokes `0x1408f4160` at `0x140950d75`; that routine matches the bound note ID, reads DWORD0 shifted right10 into voice `+0x50c`, and sets pending release `+0x509` for ready samples. Manager entry `0x14090602a` saves incoming `r8d` as its processing length. Its pending-release branch `0x140906905..932` subtracts that exact length for a later offset, or calls release inside the span. Together with the fade delay/onset subtraction, this establishes the audio fade's sample offset without guessing a32-frame control tick.

Native logical retirement remains tied to processing spans: wrapper `0x140d38b36..88` uses one call for a frame count at most1024; `0x140d38b92..d3b` splits larger counts into balanced spans at most1024. Fade exhaustion sets phase3, terminal scan `0x140946b41..b51` marks voice `+0x8fc`, and manager retirement is a subsequent dispatch through the already traced `0x140906c70..c8f` list. There is therefore a proven exact fade audio endpoint and a processing-span-dependent physical retirement boundary, not a new fragment-independent native census promise. A successful live primary-source bypass API requires an explicit policy for that boundary as well as strict source/target/sample-loop admission. No runtime bypass API or quarantine removal is introduced by this evidence append.

Bounded captures: `/tmp/kontra-native-source-1000-writers.txt`, `/tmp/kontra-native-source-load-proof.txt`, `/tmp/kontra-native-bsample-voice-bridge.txt`, `/tmp/kontra-native-bsample-voice-prepare.c`, `/tmp/kontra-native-amplitude-fallback-fade.txt`, `/tmp/kontra-native-envelope-fade-offset-callers.txt`, `/tmp/kontra-native-noteoff-offset-setup.txt`, `/tmp/kontra-native-noteoff-dispatch-offset.txt`, and `/tmp/kontra-native-noteoff-queue-dispatch.c`. These are installed-native static observations; no native audio/library or Cargo/build/test was executed by this author.

### Strict live primary amplitude bypass (production staged, 2026-10-04)

The closed loaded-sample audio law is now wired for a deliberately narrow live scope. A serde-default `Group.native_primary_bypass_slot` carries raw-address proof that all source records decoded, exactly one actual native `volume` destination exists, and it is an already admitted active/retriggered primary AHDSR or Flex with no external assignments. Authored target labels are not evidence of parameter identity. Ordinary import and snapshot import retain this proof; old caches lack it. Prepared GroupSettings additionally require forward Source-mode0 sampler ownership, no wavetable or additional volume LFO, and actual group zones without legacy or active native sample-loop rows. ScriptSetup applies the same zone gate before getters/init writes. Existing saved-bypassed-primary quarantine stays in place, since the marker specifically proves an active saved source and permits a later live switch.

`PrimaryBypass` resolves only the proven original slot; script and live host address validation require the prepared capability before reporting a getter/write or queueing a change. Voice state carries the switch plus its separate fallback fade. Bypass renders neutral unity and skips both amplitude source clocks, including the native AHDSR interpolator, across rendered, muted and shared-lane paths. The active source's existing waveform/interpolation arithmetic is unchanged. Release still reaches the primary source; if the switch is bypassed at release, the separate10ms fade runs using the verified emit-current-before-advance f32 arithmetic. Source completion, source EOF and explicit stop ownership remain independent. Planning updates the switch metadata but does not advance source/fade state; failed admission cannot consume those clocks.

Zone remapping rejects moving a sample-loop zone into a group carrying this capability. Zone preload compatibility includes the immutable capability vector. Consuming prepared-settings edits cannot change the proven source slot, switch source kinds/Flex geometry, add amplitude LFOs or replace modulation-route ownership. Bank residency compatibility compares both the raw marker and the prepared slot; mutable live bypass stays in Engine-owned state. Native wrapper-span voice retirement is explicitly not reproduced: Engine retains its existing sample-exact host NOTE_END policy at the fade endpoint. This slice proves the admitted audio/release law and the project's physical-owner policy; it does not claim full native wrapper scheduling parity or close existing Flex startup/curve parity gaps.

The staged `engine/primary_amp_tests.rs` scenario reads actual raw native AHDSR/Flex objects with deliberately arbitrary target labels, cache-roundtrips the slot proof, and compares complete2048-frame stereo Engine PCM against separate dry engines and source-scalar references with independently handwritten480-frame fallback arithmetic. Off-tick pause/resume, long mute with exact silence, overlapping physical host owners, early/duplicate note-off deferred by sustain, independently deferred release, regular/irregular/empty fragments and shared playback are included. Engine endpoint assertions are1193/1333, separately documented from native wrapper retirement. Active source-owned release must materially differ. Source EOF, SoundOff, looped-bank/script getter rejection and prepared ownership rejection are also staged. This author has run no Cargo/build/test or native audio/library execution; central compilation/execution belongs to the root task.


## Retrigger-disabled Multi LFO shared owner and generation cache (bounded follow-up, 2026-10-04)

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) describes inactive Retrigger as freewheel without note resets. It does not define idle scheduling or the lifetime of shared control-point buffers. The following is static evidence from the same installed Kontakt8.13.1 binary, not an implemented free-running admission.

The common LFO base constructor `0x1405dbd30` sets runtime flags `+0xa4 |= 0xc1`, including shared-capable bit7. Saved Group consumer `0x1407b44c0..0x1407b4860` indexes saved group ID `+0xf4` into the selected native program's runtime group table `+0x11370` and passes that row to source factory `0x140973970`. That row owns source prototypes at `+0xd68 + original_slot*16`; voice preparation `0x14097c15f..0x14097c26b` copies those prototypes into voice sources at `+0x590`. The owner is therefore a prepared group and original source slot, with voice clones; it is not a MIDI-channel or individual-note phase owner.

Source setup `0x140977c95..0x140977cbd` marks `+0xa0` when saved retrigger `+0xab8` is zero and shared-capable bit7 is set. `0x14088b16f..0x14088b186` retains a shared buffer pointer at prototype `+0x50`. The buffer contains control points, count `+0x1000` and processing generation `+0x1004`. Scheduler `0x140916800..0x140916875` emits one block on a new generation, copies the resulting clone state back into its prototype through virtual `+0x98`, and records count/generation. Later voice consumers in the same generation reuse the cached block, select its tail when their requested point count is shorter, and copy prototype state into their clone through the same virtual. Thus advancing a shared scalar separately for each voice would be wrong.

Both identified classes, Multi5 vtable `0x144729a78` and Multi Digital6 vtable `0x144729c08`, use virtual `+0x98 = 0x1405e5020`: it copies double phase `+0xb0`, fade level `+0xf0` and fade count `+0xfc`. Their reset `+0x78 = 0x1405f45a0` restores saved phase `+0xe8`, saved fade count `+0xf8`, fade level .3 and related initialization fields. Voice construction first resets a clone, then the shared branch at `0x14097c4cb..0x14097c4e8` copies the prototype state, so a later note cannot be modeled as an independent saved-phase restart. Their release virtual `+0x80` is the empty `0x1404f99a0`, so that method does not clear LFO phase or select an envelope release stage. Prototype buffer cleanup `0x1409783f7..0x14097842f` skips the reset-container branch for shared-capable retrigger-disabled sources.

An important negative boundary is explicit. The generic first-consumer path `0x14097c4ad..0x14097c4c4` computes elapsed engine time from cached generation, divides by32 and calls prototype virtual `+0x48`. For BOTH Multi classes that virtual is also the empty `0x1404f99a0` (`ret 0`). Its presence is not evidence of phase advancement during an idle gap. Background dispatch with zero voices, native generation/fragment alignment and program/group replacement reset semantics remain open. The shared generation cache also prevents treating a moved per-note Clock alone as a complete freewheel implementation. Retrigger0 remains explicitly unsupported until those owner boundaries close.

Focused captures: `/tmp/kontra-native-lfo-shared-prepare-full.asm`, `/tmp/kontra-native-lfo-shared-owner.c`, `/tmp/kontra-native-lfo-shared-copy.c`, `/tmp/kontra-native-lfo-shared-bind.c`, `/tmp/kontra-native-lfo-shared-class.c`, and `/tmp/kontra-native-lfo-shared-factory.c`. No native audio/library execution, Cargo, or tests were run by this author.

The actual runtime constructor lane independently confirms fresh state: waveform6 branch `0x140975220` selects `0x1407c2110` for one target, and waveform5 branch `0x140975131` selects `0x1407c2d10`. Both allocate and explicitly zero-fill the full0x1c8 source before calling `0x1405dbd30` and installing the identified target-bearing vtable. Thus freshly allocated prototype phase `+0xb0` and mutable fade fields `+0xf0/+0xfc` begin zero, while subsequent saved setup writes reset-phase `+0xe8` and fade-count `+0xf8`. The shared branch skips its reset virtual; saved reset-phase must not be assumed to initialize that shared state. This is a construction observation, not proof of first audible global alignment or idle scheduling. Capture: `/tmp/kontra-native-lfo-shared-runtime-constructors.c`.


The bounded ordinary engine pass now closes the idle and generation boundary for this path. Main `0x140906000` serial prepass walks global active voice head `+0x11ed0` and `next +0x920`; a zero head jumps from `0x140906d16` to `0x140906e6b` without source rendering. A source-bearing oldest active voice whose shared buffer generation differs from engine `+0x1067c` runs full common voice/source processing through `0x1409462c0` and `0x140916350`. Later voices reuse that generation's buffer. Released-but-still-active voices remain in this owner list. The ordinary engine's virtual `+0x30` at `0x140907427` resolves through vtable `0x144e98c60` to `0x140821920`; its helper `0x140821780` updates meter/history snapshots, not Group LFO waveforms. `0x14088ade0` schedules pending note work, not prototype phase. The DSP sample counter `+0x27cd0` and worker generation words beginning `+0x1067c` still advance by fragment length at `0x140908869/0x1409088a0`. Combined with the two LFO classes' no-op catch-up virtual, this ordinary zero-active-voice path retains shared phase over idle gaps rather than advancing an invented silent oscillator.

Native generation alignment is a buffer/window law. Common entry `0x140916437..0x140916448` subtracts voice start-delay `+0x170` from the processing length; `0x1409166a8..0x1409166d2` emits the ceiling of that audible window divided by32 control points and records last padding. Same-generation consumers use the cached count difference to select a tail of the point buffer at `0x140916854..0x140916875`. A shared instrument-wide modulo32 scalar alone is not this law. The current admitted retriggered source adapter's fragment/host-alignment boundary remains explicit; no shared-source implementation or new native parity claim is made here.

Destructive clear removes the shared owner. Manager `0x1408669a0` calls each program-container clear `0x14094ed20`, whose `0x140830880` destroys the loaded BProgram entries. The independently resolved BProgram RTTI vtable `0x144e98338` destructor `0x14081e000` calls `0x140802aa0`, then `0x14094e7b0` destroys the Group vector `+0x11340..+0x11348`. BGroup vtable `0x144e97a98` destructor `0x14081dc90` calls `0x140801e00`, which destroys all50 prototype rows beginning `+0xd68` through `0x140803940`. New allocation therefore recreates zero mutable source state. In-place same-kind/same-target-count source preparation instead retains the prototype after checks at `0x140973cd4..0x140973ced`; the shared branch still skips reset after frequency/rate installation. This is object identity retention, not permission to infer every program-selection/UI replacement command.

The three linked owner lists must remain distinct: global active voices `+0x11ed0 / next +0x920`, Group active voices `+0x58 / next +0x1b0`, and Program active voices `+0x11390 / next +0x1a0`. Source `+0x58` is an observed admission counter with a first-consumer gate; ordinary retirement decrement has not been independently traced. Additional focused captures: `/tmp/kontra-native-lfo-shared-main-pass.asm`, `/tmp/kontra-native-lfo-shared-main-helpers.c`, `/tmp/kontra-native-lfo-shared-background-hook.c`, `/tmp/kontra-native-lfo-shared-background-values.c`, `/tmp/kontra-native-lfo-shared-clear.c`, `/tmp/kontra-native-lfo-shared-program-clear.c`, `/tmp/kontra-native-lfo-shared-program-group-clear.c`, and `/tmp/kontra-native-lfo-shared-group-clear.c`.


The voice-side consumer is separately closed by `0x1408e4110`. An uninitialized voice destination seeds current from point0; only `(voice_offset & 31)==0` transfers current to previous and consumes the next point. Its local buffer cursor begins0 for each supplied native processing window, while the voice modulo32 offset and previous/current endpoints persist. The offset0 first use therefore seeds both endpoints to the first point. A late voice uses the scheduler-selected shared-buffer tail with its own interpolation offset. Shared source phase/fade and per-voice destination interpolation are distinct owners; relocating the complete existing note Clock would combine them incorrectly. Capture: `/tmp/kontra-native-lfo-shared-consumer.c`.

The bounded edit path preserves an already produced shared waveform block. Raw setter row0x2a8 at `0x1408c2f67..0x1408c2f7e` changes the saved source bypass field `+0x540`, then common post-set call at `0x1408c3d43` invokes Group preparation `0x140973970`. In the examined same-kind/target-count preparation, generation `buffer+0x1004` is written only when a new buffer is allocated (`0x140977c8e`); the existing shared buffer is not invalidated by that path. Preparation refresh `0x140978290 -> 0x14088b060` reaches Group active clones, whose LFO virtual `+0x18 = 0x1405e4f50` calls `0x140836fe0` to copy source bypass and target depths/polarity, then timing configuration, without replacing mutable phase. Scheduler tests clone bypass before waveform request and target binding (`0x14091660f`); otherwise it applies target transforms after the source-generation cache branch (`0x1409168d9` onward). Consequently downstream depth changes do not require regenerating unweighted waveform points, and a resumed consumer in an already-produced generation reuses that block. This does not authorize generic cache invalidation for every setter, nor invent a progressive-MIDI subwindow generation. Captures: `/tmp/kontra-native-lfo-shared-parameter-setter.c`, `/tmp/kontra-native-lfo-shared-dispatch-setter.c`, `/tmp/kontra-native-lfo-shared-parameter-rows.asm`, `/tmp/kontra-native-lfo-shared-bind.c`.


Runtime common LFO frequency installation is independently visible at `0x14095ecd0..0x14095ecf9`: it performs `mulss` of the delivered f32 frequency and source `+0xc0`, then `cvtss2sd` and stores double phase step `+0xb8`. This is the same f32-product-then-widen boundary for the shared Multi prototypes, not an inference from the UI-preview factory. The admitted deterministic owner proposal can reuse that scalar law; fresh shared phase0 and skipped reset remain separate from a retriggered source's saved initial phase. Capture: `/tmp/kontra-native-lfo-shared-phase-step.c` plus independently decoded instructions at the stated span.


### Strict shared Digital owner (production staged, 2026-10-04)

A further bounded check closes zero fade and corrects the earlier truncated arithmetic boundary. `0x140b06c94` limits fade processing to mutable count `+0xfc`; fresh count0 takes `0x140b06e7c -> 0x140b06eec`, multiplying none of the raw waveform points even though fresh mutable level `+0xf0` is0. The subsequent common tail `0x140b06eec..0x140b06f31` still subtracts `padding(+0x5c) * double0.03125(at0x144f3a000) * doubleStep(+0xb8)` from phase `+0xb0`, then adds1 once if negative. The earlier capture ended before this tail; advancing shared phase by ceil(point count) without its correction would be wrong. This closure is independent of generic per-note Fade initialization.

Production now stages only saved v71 direct-category1 Digital6/retrigger0 with deterministic weights, zero fade, disabled synchronization, known opaque flags/default timing tails and the existing plain signed pitch/one-volume target subset. Shared admission excludes wavetable ownership. `PitchLfo.shared` is serde-defaultfalse, retaining older cache per-voice ownership. Fresh shared phase0 ignores reset-only saved start phase. The existing Digital waveform arithmetic is extracted verbatim into one evaluator; the existing retriggered source branch retains its arithmetic, while a shared View reads one unweighted source block for both pitch and volume consumers. No proprietary sine/exponential parity is claimed.

Worker preparation allocates sparse mutable rows keyed by Group/originalslot into InstrumentState, which follows the existing initial-state/retired-Bank off-thread handoff. Consuming prepared-settings edits rebuild those rows off-thread. Compatible residency fills preserve Engine-owned phase; changed immutable records or destructive Bank replacement install fresh rows. Ordinary session/rate reset and NoteOff do not fabricate oscillator resets. Idle with no successfully started consumer leaves phase unchanged. The existing balanced host span has a narrow getter for this actual cache consumer; a host span survives progressive128-frame rendering, MIDI and timers, and its end is an Engine render boundary even for a Generic bank explicitly carrying shared metadata.

Each progressive segment stages copied waveform points for its complete remaining native window. All contenders start admitted audio at the same Player.now; cold physical onset/counter timestamps cannot alter candidate count or phase. Planning reads the immutable candidate. A fixed successful-plan bitmap and minimum stable admission serial nominate the oldest successfully Started producer, including released/muted active tails. Nomination changes only identity, never points/count/phase after another plan has consumed them. Only that accepted nomination commits the next phase once. Failed/cold plans cannot consume the owner; a later segment restages from unchanged phase at its actual start. Voice storage compaction and lane order stay independent.

A committed generation retains waveform points through source bypass/depth changes, following the examined native cache path. Per-voice pitch/volume endpoints, modulo32 offset and volume-target lag remain separate. The View selects the cache tail from the full window's remaining point count, while the existing per-voice interpolator decides when to consume an endpoint. Beyond-window planning/countEnd/prefetch uses a bounded copied-phase forecast without producing a future generation; the next actual span is checked again. This is an explicit KONTRA progressive adapter policy, not proof that native same-voice midwindow rerender or arbitrary caller scheduling matches it. In particular a started native window generates its whole source block; abandoning a partly rendered KONTRA callback retains that already committed block advancement, unlike the separate RTC table's elapsed-frame close policy. This boundary is preserved explicitly rather than inventing a partial rewind after edits.

All actual preview/render/countEnd/prefetch callers share that source View. Existing native source/target addressing applies live bypass and depth downstream; live timing/retrigger/phase and wider shared sources remain unadmitted. The staged production slice has been source-reviewed and diff-checked here; no native audio/library, Cargo/build or tests were executed by this author. Independent full-PCM source-owner validation remains with the coordinated fixture and central execution owners.


### Shared Digital receiver suffix correction (2026-10-04)

The bounded native consumer closes a stronger rule than the earlier local
right-aligned lookup description. Scheduler `0x140916854..875` selects the
source-buffer suffix once for the complete supplied window:
`max(cachedCount - ceil(audibleWindowFrames / 32), 0)`. Target application
`0x14091691e -> 0x1409519e0` transforms that block. The scheduler retains
voice offset `+500` and writes `(oldOffset + audibleFrames) & 31` at
`0x140916985..99f`. Consumer `0x1408e4110` starts its local buffer cursor at
zero and consumes a point only at that voice's modulo-32 boundary. Initial
endpoint seeding reads point zero without advancing this cursor. It does not
reselect a suffix independently at each audio sample.

Production now retains receiver window identity, entry frame and entry
modulo in each existing copied/committed LFO Clock. It selects one suffix
from that entry (or a later first-produced cache base), then computes the
cursor from actual voice boundaries. For example, a voice entering a
193-frame window at frame257 with offset1 reads cache point0 at its first
boundary288; the former per-sample remaining-window calculation incorrectly
read point1. Source cache generation/count/points and producer nomination
are unchanged. Planning changes only copied receiver clocks; actual rendering
commits the actual receiver. Bounds are checked before accessing emitted
points.

The full-window consumer rule is native evidence. Mapping progressive MIDI
edits, cold readiness and bypass/resume fragments into the supplied host
window remains KONTRA's documented adapter policy: a retained window entry
and audio modulo continue through bypass, with skipped boundary points not
replayed; a first cache produced late begins at its actual cache base. The
existing pure copied beyond-window forecast remains speculative and cannot
advance the owner. This does not establish full native scheduling or sine
kernel parity. No tests or native audio were executed for this change.

Ordinary retirement's previously open admission-counter decrement is now
closed independently by the mono/release owner: engine vtable slot70
`0x14096a740`, invoked before the shared prepass at `0x140906c86`, walks voice
sources `+590/count810` and decrements/clamps prototype `+58` at
`0x14096ab31..3f`. Zero-group-voice buffer clearing explicitly skips shared
bit7/retrigger0 sources; retirement does not call source reset. This supports
retained shared phase through ordinary voice retirement without conflating
Group, Program and global active lists.

### Retained non-volume AHDSR bypass clocks (production staged, 2026-10-04)

The [KSP engine-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters) identifies internal-source bypass separately from target intensity. The source-clock law here comes from the installed Kontakt binary: the common scheduler tests source `+0x0c` at `0x14091660f..0x140916613` before either waveform generation or target application. That gate is common to target-bearing internal sources, rather than specific to amplitude. Physical release separately visits source virtual `+0x80` at `0x14086b193..0x14086b1ad`, including bypassed retriggered sources. Thus disconnecting a pitch/filter destination while continuing its AHDSR source recurrence was incorrect.

The prepared pitch source now returns before advancing its Envelope when bypassed. Both steady/shared Voice advancement paths consult the corresponding source switch as well. VoiceFilter retains its original source rows and routed target indices, skips only the bypassed envelope recurrence, and resumes that same state when enabled. It continues the independent sample cursor, mute processing, filter control grid and external-controller lag clocks; a disabled destination never removes a retained source row or changes another source's time. Release reaches the paused source and captures its existing level through the existing release kernel. This change does not alter primary amplitude bypass/fallback ownership, WNT completion or shared Retrigger0 admission.

The existing complete-Engine filter oracle is strengthened into a mixed pitch/cutoff/resonance source. A 37.3ms moving attack is bypassed at 941, physically released at 1003 while paused, and resumed at 1107. A separate Engine has no internal envelope: scalar source outputs drive explicit per-frame base tuning and filter knobs. Complete stereo PCM across regular/irregular fragments, mute/unmute, both amplifier placements, both filter types, the completed amplitude tail and zero remaining voices are staged. The saved-bypass module-control and pitch-address fixtures now require a paused source rather than elapsed-time advancement. The scalar reference verifies orchestration of the existing generic non-volume kernel; it does not newly establish native non-volume AHDSR curves/timing or filter interpolation parity. No Cargo/build/test or native audio/library execution was performed by this author.

A bounded live-retarget trace also found that AHDSR WithTargets0 vtable `0x144729108`, virtual `+0x18 = 0x1405e4ef0`, copies bypass and target depth/polarity and sets source `+0xa1`, without copying source stage/count/curve fields. Flex virtual `+0x18 = 0x140837d90` has a different refresh body. AHDSR parameter-modulation helper `0x140ae46d0` dereferences optional parameter pointers `+0x128..+0x148` and is requested at retrigger/release boundaries by `0x140ae40f0`; this is not yet proof that a raw KSP Attack/Hold/Decay/Sustain/Release/Curve setter updates the already-running source. The exact setter-row/rebuild branch remains the next bounded closure; no guessed live retarget implementation was introduced. Captures: `/tmp/kontra-native-envelope-live-refresh.txt`, `/tmp/kontra-native-ahdsr-stage-refresh.txt`, `/tmp/kontra-native-ahdsr-reset-refresh.txt`.

The direct AHDSR setter boundary is now narrowed further. Bounded raw setter rows `0x2aa..0x2af` at `0x1408c13e7..0x1408c156c` store concrete curve/attack/hold/decay/sustain/release fields `+0x624/+0x614/+0x618/+0x61c/+0x610/+0x620`; setup `0x1409653db..0x140965551` reads precisely these fields into AHDSR initializer `0x140ae5b20`. They return through common Group preparation. Active-voice refresh `0x14097be30` rebuilds source storage when source-kind/route geometry differs; its retained-kind path instead visits each existing source's virtual `+0x18`. The AHDSR implementation above leaves the clone's current recurrence, counts, sustain and release coefficient unchanged. Scheduler virtual `+0x90 = 0x14051b400` returns literal true; AHDSR virtual `+0x58 = 0x1405f61f0` only reports unreleased Sustain, and waveform `0x140ae40f0` does not copy prototype knobs. Therefore an ordinary direct waveform-knob write is not evidence that a running retriggered AHDSR should restart or retarget. Keeping the existing note-start capture is appropriate for this examined retained-kind path. External modulation of envelope parameters through the optional `+0x128..+0x148` pointers remains a separate source-binding contract; no new live AHDSR sync method or external envelope-parameter route is admitted. Additional captures: `/tmp/kontra-native-ahdsr-setter-rows.txt`, `/tmp/kontra-native-ahdsr-live-scheduler.txt`, `/tmp/kontra-native-ahdsr-getter-refresh.txt`.


### Compatibility Flex point-index ownership (production staged, 2026-10-04)

The public prepared `Flex` owns a boxed point vector and uses `usize` sustain/loop indices. Its private Envelope stage formerly stored the current point in `u8`: advancing past point255 overflowed, while release and loop successors narrowed larger valid indices to unrelated earlier points. These are Rust ownership defects independent of native waveform timing. The stage now stores the vector's own `usize` index and uses checked `get` throughout. Only the public sustain/loop successor additions saturate, so an impossible `usize::MAX` marker reaches the existing out-of-range completion path instead of arithmetic overflow. Ordinary progression advances an index already proved to exist in the owned vector; zero-length vectors and impossible successors complete without revisiting a point. One-shot still ignores release and traverses the actual vector to its terminal boundary. No stage law, coefficient, clock, sample cursor or previously supported PCM arithmetic changes.

The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) specifies a32-breakpoint native editor. That editor limit is not imposed on public Compatibility vectors or consuming prepared settings; no new native mode admission or native curve/startup equivalence is claimed. Malformed markers are contained safely, without claiming that arbitrary malformed geometry is a supported envelope shape.

The isolated `engine/flex_index_tests.rs` stages a complete1024-frame stereo Engine oracle from an authored260-point Group, its serialized cache roundtrip, Bank preparation and prepared-mode edits. Independent two-frame linear point arithmetic scales separate source-free Engines with patterned stereo samples. The comparison covers passing point255, sustain257, loop return from257 to256, early physical release before reaching257, captured-level release into258/259, a pedal-held physical owner, overlapping retrigger, one-shot traversal, shared/scalar consumers, regular/irregular fragments, empty fragments and a fully muted interval. Exact audio-voice endings are717/354 for released sources and520/837 for one-shot sources, followed by a zero PCM tail. The first one-shot Host owner remains physically held after audio completion520: `mark_host_notes` pins held owners, physical Off600 records `stop600`, and `host_note_end_at` then reports the maximum of audio completion and physical stop,600. The second Host owner was already released at350 and ends with its audio at837. The fixture checks held-owner ineligibility at520..600 and eligibility after actual Off600 separately from the audio census. Separate empty-vector/MAX-marker assertions check overflow containment only. These fixtures are staged, not executed; no Cargo/build/test or native audio/library execution was performed by this author.


### Saved v73 unsynchronized LFO fade clamp (production staged, 2026-10-04)

The existing native v71/v72 source fade and typed reader are extended only by the closed v73 appended boolean. Native BParLFO reader `0x140d0376c..0x140d03774` reads the v73 final byte into concrete `+0x1d`; the earlier reader path explicitly writes false at `0x140d038dc`. Runtime setup `0x140977de0..0x140977de7` copies the embedded saved byte `+0x56d` into source `+0xe5`. It is separate from waveform normalization, frequency synchronization, delay-record flags and retrigger. Source reset `0x1405f45a0` still initializes mutable fade level to exact f32`0.3`, copies prepared count`+0xf8` into remaining`+0xfc`, and resets phase independently.

The common completed-waveform fade `0x140b06c90` tests source`+0xe5` in both its unrolled body (`0x140b06cf4`) and scalar remainder (`0x140b06e95`). Both modes first multiply the point by current f32 state minus f32`0.3`, then multiply state by the prepared f32 factor. False clamps the new state to f32`0..1`; true clamps to exact f32 constants with bits`0x3e99999a` and`0x3fa66666` (`0.3..1.3`). Only the remaining N points are scaled; subsequent waveform points pass through. No linear fade replacement, extra output clamp or changed phase recurrence is introduced. The bounded artifact is `/tmp/kontra-native-lfo-v73-fade.txt`. The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) documents LFO fade-in as source onset behavior; no editor label or undocumented UI meaning is invented for this exact saved switch.

Production accepts the actual v73 direct-category1 BParLFO child with an explicitly decoded appended boolean, active/retrigger1/default source flags and default opaque timing tails, through the existing deterministic Multi/MultiDigital waveform, phase, frequency, unsynchronized0..5000ms fade and target gates. v73 shared/retrigger0, synchronized fade, unknown source flags/layout/tails and external source controls remain diagnosed and unadmitted. Existing v71 shared zero-fade admission is unchanged. `PitchLfo.extended_fade` is serde-defaultfalse and belongs to source equality; paired pitch/volume rows cannot disagree on the switch. Immutable Group source equality already includes entire LFO records, and consuming prepared settings retain their existing destructive-install ownership. `build.rs` hashes `src/modulation.rs` in the importer hash, and cache stamps include that hash, so previously skipped v73 sources are reparsed through the existing invalidation mechanism.

The existing actual-NKI timing fixture now stages one v73 fade scenario for false and true modes. It crosses creator-written NKI replacement, typed import, exact cache, resident Bank preparation and whole Engine PCM. An independent triangle/point clock and explicit scalar f32 fade recurrence drive per-frame tuning in a separate source-free Engine; the reference does not use production Fade/Clock. Complete stereo PCM checks fixed/irregular fragments, mid-tick bypass/resume, source-depth retarget, mute/unmute, tempo changes that must not alter unsynchronized fade, physical release and equal Host endings, plus audible mode differences and completed zero tail. Older serialized records missing the new field retain false. Paired prepared-source disagreement is rejected by the existing ownership guard. No Cargo/build/test or native audio/library execution was performed by this author; central execution owns the functional result.

A separate synchronized-fade investigation closes delay converter `0x14072b380 -> 0x140a74bf0`: ordered f32`((note_value*60)/BPM)*max(count,1)*1000`, BPM below f32`0.1` falling back to120, clamped0..5000ms. Setup prepares fade N/factor from that duration, while same-kind refresh `0x1405e4f50` copies factor/future count without replacing current level/remaining count. Group sync detection `0x1408224c6..0x1408224da` checks both frequency and fade sync, and pending preparation reaches `0x140973970` at `0x14096a193`. The actual transport-change dispatch into that pending pass remains open; synchronized fade is not admitted from the converter formula alone.

### Saved synchronized LFO timing — prepared source prototype and retained voice state (staged)

The native transport dispatch is closed by a bounded instruction-aligned capture of the installed Kontakt8.13.1 binary: wrapper transport update `0x140d31fe0` ends in a tail jump at `0x140d321ba` to `0x140982710`. That function reads raw transport BPM at `engine+0x11c00 -> +8`, subtracts the last globally refreshed BPM at `engine+0x11bf0`, and returns inside the inclusive f32`[-0.001,+0.001]` band. An outside-band change stores the new refresh BPM and tail-jumps at `0x140982741` to `0x14096a060`. Its Group pass calls sync detection `0x140822460` and Group preparation `0x140973970` at `0x14096a193`. Detection tests both LFO frequency sync `+0x578` and fade sync `+0x5b0`; fade synchronization does not depend on frequency synchronization. The captured threshold constants have bits`0xba83126f` and`0x3a83126f`. Artifact: `/tmp/kontra-native-lfo-sync-fade-tempo.txt`.

The BParLFO delay getter `0x14072b380 -> 0x140a74bf0` uses ordered f32`(((note_value*60)/BPM)*max(coefficient,1))*1000`, substitutes120 for BPM below f32`0.1`, and applies the delay object's0..5000ms bounds. Finite negative, zero and sub-one authored coefficients therefore remain valid synchronized counts and are clamped by the converter; they are not replaced by an invented admission rejection. Group setup `0x140977d3a..0x140977dce` uses current raw BPM, multiplies returned milliseconds by the source control rate and then f32`.001`, truncates N, and prepares its existing exponential factor. Reset `0x1405f45a0` copies prototype N`+0xf8` into mutable remaining`+0xfc` and initializes fade level`+0xf0` to exact f32`.3`. Same-kind source refresh `0x1405e4f50` copies factor`+0xf4` and future-reset N`+0xf8` while preserving mutable current level`+0xf0` and remaining`+0xfc`. Thus a tempo refresh changes subsequent multiplication; it does not recalculate the running voice's remaining duration or restart its fade.

Production admits this synchronized fade only for the existing native direct-category1/v71-or-v73, retrigger1 deterministic Multi/MultiDigital source with default opaque timing tails and existing admitted pitch/volume target geometry. Shared/retrigger0 synchronized fades and external source timing controls remain diagnosed. `LfoFadeSync{note_value,count}` is an optional serde-default source specification; `fade_ms` retains real unsynchronized milliseconds and is zero when the synchronized specification is present. Both source equality and paired pitch/volume preparation include the specification; ambiguous units, nonfinite geometry and invalid source indices fail the existing preparation gate. Missing metadata preserves older unsynchronized behavior. Existing importer-source hashing invalidates formerly skipped synchronized sources through the normal cache mechanism.

InstrumentState owns bank-prepared fixed-sixteen-slot timing rows alongside effective Group settings. Player retains raw transport BPM separately from its optional global native refresh BPM. The first valid transport seeds and refreshes that owner; this avoids inventing a native constructor BPM. Subsequent accepted global tempo changes refresh source prototypes; bank installation and existing admitted Group preparation hooks use current raw BPM even when it lies inside the global threshold, without resetting the global comparison owner. Audio-rate reset refreshes prototypes for the new source rate. The same row retains converted f32 frequency and native double phase step: `0x140977ce4..0x140977d35` loads current raw BPM and invokes the frequency getter, while `0x1406068b0` narrows the control-rate double reciprocal to f32 and `0x14095ecd0..0x14095ecf9` multiplies f32 frequency by that reciprocal before widening to double `+0xb8`. Same-kind `0x1405e4f50` copies this prepared step without replacing phase. Synchronized frequency consumers now use that prepared row, so inside-band raw changes do not silently retime the source; KSP and other raw-transport consumers retain the raw BPM. Unsynchronized frequency keeps its existing arithmetic. Exact Sine waveform/table parity remains unproved; its existing analytic approximation merely uses the same prepared frequency owner. A voice captures the prepared reset count at source creation, including when initially bypassed. Subsequent prepared-factor updates preserve that voice's level/countdown; bypass pauses source emissions without losing the retained state. Copied planning clocks borrow the same prepared row and cannot advance the committed source on a planning error. Unsynchronized v71/v73 fade arithmetic and shared zero-fade behavior remain unchanged. The existing source-planning gate also rejects a synchronized fade prototype N outside signed-i32 representability, matching the native CVTTSS2SI boundary without imposing an arbitrary global rate or count cap. Unsupported live fade timing/mode writes remain unsupported.

The staged actual-NKI/cache fullPCM scenario uses a v73 synchronized triangle source with count4/note1/8. The global owner is explicitly seeded at120 before the fixture changes raw BPM. Independent f32 conversion gives375 points at120BPM,374 at120.0005,450 at100, and449 at100.0005/100.002. Separate source-free Engine voices receive explicit per-frame tuning from an independently written triangle/fade recurrence. The timeline covers a retained active voice, a second voice born during bypass before a global tempo refresh, and a fresh voice after refresh; below-threshold raw changes, native f32-product-then-widen Digital phase step, Group bypass/depth preparation at current raw BPM, bank installation inside the global threshold, mute, physical Off, complete stereo PCM, exact Engine Host endings, completed zero tail, fixed/irregular fragments and zero callback heap are asserted. The oracle checks the prepared-owner/countdown law over the existing Engine32-frame source/interpolation adapter; it does not newly claim native wrapper-span or waveform-kernel PCM parity. No Cargo/build/test or native audio/library execution was performed by this author; root owns central compilation and merit checks.


### Forced-stop pre-amplifier send lifetime — functional checkpoint (2026-10-04)

The voice already retained the dying Fade's exclusive terminal count, but its raw interpolation and insert/send processing continued for the remaining host fragment. A SendLevels slot before the Amplifier could therefore feed a return after the forced-stop fade completed; unrelated Rack peer boundaries changed those extra source frames. Voice.render now captures the dying Fade's remaining count before advancing it and restricts interpolation, voice insert/send processing and dry accumulation to that prefix. Source gathering still uses the complete planned position buffer required by counted/wide/HQ windows. Source, envelope and gain recurrences are unchanged. A zero user gain remains an active pre-amplifier source. Ordinary AHDSR/Flex completion, sample EOF and native postmodule lifetime policy are not broadened by this fix; program return processors retain their independent tails.

Root reports compile-only gate 11493 GREEN (1m08s) and the unchanged actual Rack chronological PCM regression PASS (0.03s), including all output/reference PCM, meters, exact note endings and the late drain. Both fixed 128-frame and irregular partitions retained a measured FFT residual of 4.1909518e-10 at frame 1093; exact silence was observed from 1280 through 1408. The independent impulse-support endpoint and subsequent drain are checked separately from exact float silence. This checkpoint verifies Engine forced-stop/send ownership and partition stability, not new native filter-tail or wrapper-retirement parity. This author ran no Cargo or tests.


### Saved synchronized frequency coefficient floor — admission fix staged (2026-10-04)

Raw LFO admission formerly required a synchronized frequency coefficient at least one, even though the native timing converter and the production prepared frequency owner both floor it at one. A bounded recapture of the installed Kontakt binary closes the exact constructor/reader/getter chain: BParLFO construction at `0x1407033f3` invokes timing constructor `0x140a723e0`, which writes timing `+0x20 = 1` at `0x140a72408`. Typed timing reader `0x140ab5da0` reads the saved note value into `+0x18`, opaque floats into `+0x24/+0x28`, and the trailing byte into `+9`; it derives sync-enabled `+8` from note value greater than f32 1e-9 without replacing the constructor's `+0x20` floor flag. Frequency getter `0x14072b330` calls `0x140a736a0`, whose enabled path tests `+0x20` and performs MAXSS(coefficient, f32 one) at `0x140a736cb` before the ordered note-value/tempo/count duration, reciprocal and constructor-bound clamp. Captures: `/tmp/kontra-native-lfo-count-floor.asm`, `/tmp/kontra-native-lfo-count-floor-setup.asm`, `/tmp/kontra-native-lfo-count-floor-getter.asm`. The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) explains that synchronized Freq specifies how many selected note values span one cycle; the finite sub-one/negative saved coefficient floor is established by the actual converter, not inferred from the editor wording.

Production now admits finite synchronized coefficients below one only for the existing v71/v73 deterministic source subset with strict unstructured category1/direct08 ownership, retrigger1, default source/timing opaque fields and the plain supported pitch/volume target geometry. Unknown ownership, nonfinite coefficients, unsupported modes/targets and v72 remain diagnosed. Existing admission for coefficients at least one is unchanged. The typed source and cache retain the authored coefficient exactly, including signed zero; conversion belongs to the existing `PitchLfo.frequency` and bank/group-prepared timing owner, rather than rewriting saved metadata to one or adding a second timing policy. Existing source equality includes count, and importer-source hashing invalidates formerly skipped records through the normal cache stamp.

The new `engine::lfo::timing_tests::count_floor_tests::saved_sync_frequency_counts_below_one_reach_native_floor_and_whole_pcm_without_heap` is staged. Creator-written NKI is rewritten with literal synchronized coefficients 1, -3.25, negative zero, positive zero, .25 and .999, then passes actual parsing, cache restoration, Bank and Engine. A separate source-free Engine receives explicit per-frame tuning from an independent triangle/phase interpolation oracle using literal effective count one; it never invokes the production converter or reads the authored coefficient. The complete stereo PCM and exact Host end are checked for fixed/irregular fragments, global tempo refresh, source bypass/resume, depth update, mute/unmute, physical Off, completed zero tail and zero callback heap. All floor cases must produce the identical PCM and end as authored one; additional v73 false/true 40ms fade cases cover the same new frequency boundary through the retained fade source. Existing fixtures retain their prior helper/API and scenarios. This author ran no Cargo/build/tests; root owns central compilation and functional checks.


Root functional checkpoint for the synchronized frequency floor: compile-only gate 62354 GREEN (1m31s), followed by the registered actual NKI/cache-to-Engine complete PCM scenario PASS (0.17s). The check exercised authored count preservation, floor-equivalent output, global tempo refresh, bypass, fragments and callback heap; independent static review was clean. Converter evidence is also retained in ignored `artifacts/core-native/lfo-count-floor-converter.asm`, `lfo-count-floor-constructor-reader.asm` and `lfo-count-floor-getter.asm` so temporary-file cleanup cannot discard the captures. No tests were executed by this author.


### Shared v71 Digital synchronized frequency — prepared generation timing (staged)

The previously admitted strict shared Digital owner rejected synchronized frequency and its source staging supplied no prepared timing row. Native synchronized frequency uses the same prepared source step for this shared class: LFO same-kind virtual `+0x18` at `0x1405e4f50` copies double step `+0xb8` at `0x1405e4f65..0x1405e4f6c` plus future fade parameters, without copying phase `+0xb0`. The distinct state-copy virtual `+0x98` at `0x1405e5020` copies mutable phase/fade. Aligned setup beginning `0x140977c46` allocates a shared buffer only when absent and writes its generation `+0x1004` at `0x140977c8e` only on that allocation branch. LFO setup `0x140977cd6..0x140977d35` installs frequency converted from current raw BPM before the common shared-capable/retrigger0 branch. The aligned shared branch `0x140977ee0` bypasses reset of that source. Global refresh `0x140982710` and sync detection `0x140822460` retain the previously closed inclusive f32 ±.001 threshold and frequency-sync detection. Fresh captures are retained in ignored `artifacts/core-native/lfo-shared-sync-copy-refresh.asm` and `lfo-shared-sync-setup-aligned.asm`; an initially unaligned setup prefix was removed and replaced by the capture starting at the actual common branch target.

Production extends only the existing v71/direct-category1 deterministic Digital, retrigger0, zero-fade/default opaque source and plain pitch/volume target subset to synchronized frequency. Newly synchronized raw admission requires the known unstructured child; prior unsynchronized admission is unchanged. Existing synchronized count-at-least-one admission remains the raw shared boundary; the separate retriggered saved count-floor extension remains unchanged. Shared fade synchronization, v73/shared mode, external timing/phase controls and unrelated waveforms remain unsupported. Preparation validates finite timing alongside the existing shared source/paired-consumer geometry. Player forwards the InstrumentState-owned fixed timing row into Shared.stage; synchronized generations use its exact prepared double step, while unsynchronized step arithmetic is unchanged. Already committed generation points and their phase/step correction are not regenerated by a mid-window global or authored refresh. New windows and a resumed source with no committed candidate use the latest prototype; idle with no accepted consumer retains phase. No new owner, callback allocation, source waveform arithmetic, consumer interpolation law or AHDSR/provider/program-amplifier change is introduced. Source equality already includes shared/count/note metadata, and existing importer hashing invalidates previously skipped saved sources.

The registered `engine::lfo::timing_tests::shared_sync_tests::saved_shared_sync_uses_prepared_generation_step_and_keeps_overlap_idle_and_tempo_pcm_without_heap` is staged from creator-written actual NKI, a literal v71 synchronized/retrigger0 child, typed parsing and binary cache through Bank/Engine. Independent window-length phase integrals, cached-point suffix selection and per-receiver linear interpolation drive explicit tuning in a separate source-free Engine. Its 257-frame host windows cross overlapping notes, raw BPM changes below/above the global threshold inside an already generated cache, bypass/resume of that cache, authored depth preparation at current below-threshold raw BPM, a fully muted source interval, exact 240-frame physical Host choke endings, a zero-consumer idle gap, retained-phase restart and completed zero tail. Complete stereo PCM, exact Host endings, strict scalar retained phase at idle/final boundaries, fixed/irregular fragments and zero callback heap are asserted. This is an Engine shared-generation/prepared-step oracle over the admitted adapter, not a native host/audio execution or new wrapper-span parity claim. This author ran no Cargo/build/tests; root owns central verification.

### Authored/prepared envelope finiteness boundary (production staged, 2026-10-04)

The native-file AHDSR and Flex readers already call their scalar validators
(`vendor/ni-file/src/kontakt/objects/envelope.rs`, `EnvelopeAhdsr::try_from`
and `EnvelopeFlex::try_from`). Their range checks exclude nonfinite source
values. Public authored Group structs and `Bank::with_prepared_settings` can
bypass those readers, however; previously the common `VoiceRoutes::prepare`
validated LFO/Glide sources but allowed nonfinite envelope fields into DSP.

That existing preparation owner now checks effective primary/pitch AHDSRs,
Flex points, and retained routed filter-envelope rows through small immutable
source predicates. Both normal Bank builders and consuming prepared-setting
edits reach this owner before route/source commitment or PCM. The exact
all-field canonical `Ahdsr::UNITY` value remains valid: its intentional positive
infinite release holds the partner of a lone Flex until another voice owner
ends it. The pre-conversion authored AHDSR gate in
`params::validate_modulation_capacity` rejects raw nonfinite numeric fields,
including infinities that the existing sustain clamp could conceal. Near-UNITY
prepared values receive no exception. No scalar is clamped or rewritten, and
finite legacy negative/extreme timing behavior remains unchanged. Native
primary curves additionally retain the reader's verified `[-1,1]` domain;
Compatibility curve ranges and unsupported-source roster handling are unchanged.

`engine/envelope_validation_tests.rs` stages authored primary/pitch/filter and
prepared primary/Flex nonfinite rejection, Native curve-domain rejection,
finite extreme timing acceptance and exact UNITY preservation,
then a repaired source's complete stereo Engine PCM and ending against an
independently pre-shaped physical sample. The reference uses sustain1/4 and a
32-frame release endpoint interpolation, rather than a production Envelope or
Native clock. Solo/lane, irregular fragments and allocation-free rendering are
covered. Ordinary Native's source-endpoint observation is kept separate from
physical sample EOF and shared range-end completion. Diff checking passed;
this author did not execute Cargo, tests, scenarios or native emulation.

### Supplied PCM and retained Group FX finiteness (production staged, 2026-10-04)

Public `Bank::from_samples` previously bypassed decoded `SampleReader`'s
existing nonfinite-to-zero policy and could retain NaN/Infinity in `Pcm::F32`.
The supplied-sample boundary now rejects the first nonfinite sample with its
path, frame and channel before Builder/prepack commitment. Decoder and stream
policies are unchanged. The registered validation fixture corrupts right frame73
with NaN and either infinity, then repairs to actual complete PCM/end behavior.

`VoiceRoutes::prepare` now checks retained `GroupFilter` processed fields:
consumed `Unit::key` components, retained output gains, StereoModeller numbers,
each drive/dynamics family's actually consumed field indices, and retained send
tap numbers. Existing key clamps remain the owner of filter domains, including
finite zero-frequency EQ inputs that normalize to negative infinity and already
clamp to their low endpoint. Unused filter extras, field padding and opaque FX
records are outside this predicate; no finite value is changed or newly clamped.
The same Unit key predicate validates a copied prospective filter type before
`set_type` changes the shape, section counts or revision, so a rejected switch
cannot make previously unused corrupt metadata audible or report success.

The authored/prepared corrupt-gain cases repair to a retained identity
StereoModeller at gain0.5 and independently scaled physical reference PCM.
The short reference source explicitly compensates its existing last48-frame
EOF fade while preserving physical END160 and the original error tolerance.
A second staged actual Engine case admits unused NaN/Infinity filter extras,
rejects a live type2-to-Ladder type33 switch after73 frames, and compares complete
stereo PCM and endings with a separately authored unchanged-type control. This
tests atomic type rejection and current PCM preservation, not native filter
transfer equivalence. Solo/lane, irregular fragment and zero heap assertions
remain staged for root execution; this author ran no Cargo or tests.

The live GroupFilter setters' production callers already obtain finite scalars
from the integer parameter codec, including signed direct Ladder Gain. No
duplicate setter gate was added. The separate public per-key expression entry
could previously copy arbitrary nonfinite gain/tune/pan into active Voice
target/cursor state. `MidiState::set_expression` now rejects the entire triple
before replacement or receipt changes, preserving all existing finite ranges
and typed CC/pressure/bend members. A staged actual Engine timeline attempts
NaN gain, positive infinite pan and negative infinite tuning at frames73/149/223,
then applies a finite gain/pan/octave recovery at257 and Note Off320. Complete
stereo PCM/end353 matches an untouched known-control timeline across solo/lane
and irregular fragments without heap; a third held-control timeline verifies
the valid recovery actually changes PCM. No tests were executed by this author.

### Live admitted volume LFO target depth: native units and ordered polarity (staged)

The reachable KSP hole was `Address::resolve` consulting only pitch LFO/Glide
routes although a prepared `VolumeLfo` already retained its native target index.
An admitted volume route now resolves the existing three intensity aliases;
unknown, cutoff and Q targets still have no invented live consumer.

Kontakt 8.13.1's raw PE unit table at `149e58390`, stride64, closes the numeric
law independently of pitch destination inference. Row718 at`149e63710` stores
forward`1407b9e30` (`mulss x,x`) and inverse`1407b9e40` (`sqrtss`). Rows721/722
at`149e637d0/149e63810` store forward`1407b84b0` (`x*x*x`), inverse`1407b9e50`
(sign times power(abs,1/3)), and bipolar byte1 at+30. Adapter`140958a90`
centers normalized input as`2*x-1` before generic converter`140514930`.
Signed setter`1408cb735` converts once, forwards absolute physical magnitude to
parameter718 and forwards the strict-negative predicate to1056. Getter
`1408a4241` reads those two values, restores the sign, then inverse adapter
`140879000` applies the inverse and `(value+1)*0.5`. These tables and aligned
bounded captures are retained in ignored
`artifacts/core-native/lfo-volume-depth-conversion-rows.txt` and
`artifacts/core-native/lfo-volume-depth-set-get-copy.asm`.

The primary KSP reference describes `$ENGINE_PAR_MOD_TARGET_INTENSITY` as the
positive slider and `$ENGINE_PAR_MOD_TARGET_MP_INTENSITY` as the bipolar alias,
neutral500000; negative intensity's source polarity is distinct from the
separate Invert Source control. See the official [engine parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters).
The numeric square/cubic laws above come from the native table, not that prose.

`VolumeSourceIntensity` writes the prepared target magnitude only for unsigned
writes, and magnitude plus polarity for signed writes. Getter signedness chooses
which representation is returned. Original source slot and target ordinal are
preserved. Same-kind native copy`140836fe0` updates target+8 magnitude and flag+5
bit1 without replacing source phase or target recurrence; this change likewise
leaves Clock, lag, interpolation, committed shared points and source geometry
untouched. Native state keeps signed and unsigned writers separately ordered:
a signed negative write followed by an unsigned magnitude write must restore
both, whereas the two signed aliases still address one record. A repeated
writer value must regain its later order when the opposite writer is newer:
otherwise negative.125 followed by unsigned.25 could restore as negative.125
because a still-earlier unsigned.25 caused a false no-op. Capture compares the
two already-prepared record orders before skipping equal values. Existing loader
preparation already enumerates both forms; no realtime storage growth was added.

A staged actual Engine fixture,
`params::tests::volume_lfo_depth_tests::live_volume_lfo_depth_keeps_native_units_lag_phase_and_ordered_restore_in_complete_pcm`,
compares complete4096-frame patterned stereo PCM to a source-free dry Engine
multiplied by an exterior literal16-point triangle, native15ms lag recurrence
and authored physical magnitude/polarity changes. Exact1/16 phase steps isolate
the target setter law. Automation starts after137 frames; bypass733..941 pauses
the source and retained target, mute1107..1439 continues clocks, and fixed128 vs
irregular partitions must agree. Physical Off2048 drains the ordinary amplitude
tail; ordered restore tests last-negative then unsigned magnitude, plus init
getter visibility. It tests accepted API-to-existing-DSP orchestration, not a
new native waveform/tail parity claim. Production and fixture are staged; this
author executed no Cargo or tests.

Root functional checkpoint for live volume LFO depth: compiled gate85841 and
actual registered complete PCM/ordered-restore scenario PASS (0.03s).
Independent static review was clean, including the repeated unsigned-value
ordering correction. Wire coverage is the truthful Group serialization cache
roundtrip; existing saved LFO decoding is unchanged by this live setter slice.
No test execution was performed by this author.

### Strict shared v71 synchronized count floor (staged)

A remaining raw admission check rejected finite synchronized counts below1
for the already-supported shared/retrigger0 Digital source, despite preparing
that same source through the closed native floor-to-one converter. The primary
[LFO controls reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation)
defines synchronized frequency in note durations per cycle and Retrigger0 as a
continuing source. The minimum-count law comes from the actual native converter,
not an assumption about the editor's range.

The retained `lfo-count-floor-converter.asm` shows enabled timing+8, constructor
`140a723e0` initializes minimum-count flag+20=1, and converter
`140a736a0..140a736f5` applies `max(count,1)` before ordered f32
`((note_value*60)/BPM)*count`, reciprocal and frequency bounds. Saved BParLFO v71
coefficient+10 and frequency timing+20 retain their authored units independently
of the internal modulator's Retrigger flag. Setup`140977cd6..140977d35` converts
frequency before the common retrigger0 branch`140977ee0`. Same-kind copy
`1405e4f50` copies step+B8 without phase+B0; the existing prepared source owner
and committed generation cache therefore already implement this law. Captures
remain in ignored `artifacts/core-native/lfo-count-floor-*.asm` and
`lfo-shared-sync-*.asm`.

Production only allows the strict existing shared predicate to satisfy the
synchronized frequency-count floor guard. Finite validation, direct unstructured
v71 child, deterministic Digital waveform, Retrigger0, default opaque timing,
zero unsynchronized fade, original flags and plain pitch/volume targets remain
required. Retriggered admission and unsynchronized bounds are unchanged;
v72/v73 shared, random, shaped/inverted or external target/timing configurations
remain diagnosed. Raw coefficient bits, including negative zero, remain authored
through import/cache and prepared source equality. No Player, Voice, source
arithmetic, preparation owner, getters, producer nomination or cache lifetime
was changed.

The staged actual-NKI/cache Engine fixture
`engine::lfo::timing_tests::shared_sync_tests::saved_shared_sync_counts_below_one_keep_native_floor_prepared_generations_and_whole_pcm_without_heap`
imports literal coefficients1, -3.25, negative/positive zero, .25 and .999 with
Retrigger0. Each complete stereo output is compared to a source-free Engine
receiving independent literal effective-count-ONE tuning, and must equal the
separately authored count-one instrument. Existing257-frame generated windows,
overlapping second note, fixed/irregular fragments, inside-cache tempo changes,
bypass/resume, target depth refresh, muted source clocks, physical choke endings,
zero-consumer idle and retained phase are retained, with zero callback heap.
The earlier count-three shared proof now supplies explicit oraclecount3 and
otherwise has unchanged assertions. This author ran no Cargo/build/tests.

### Live native Digital LFO frequency — exact slider units and retained phase (staged)

The [official KSP engine-parameter reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters) names `ENGINE_PAR_INTMOD_FREQUENCY`; the [parameter-command reference](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands) establishes the continuous 0..1000000 KSP range and original modulator-slot address. The [Kontakt modulation manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation) distinguishes unsynchronized Hz from a synchronized count of selected note values per cycle. The exact slider laws below come from the installed Kontakt8.13.1 binary, not an inferred exponential curve.

Native registry `1409adf96..1409adfaa` associates `intmod_frequency` with parameter713. Its static conversion-table row at `149e635d0` identifies normalized forward `1407b8a30` and inverse `1407b8ac0`. The forward curve clamps its normalized input and evaluates the instruction-ordered f32 exponent/IEEE-bit approximation; the inverse uses its separately authored f32 mantissa correction. Bounded Unicorn execution of these actual PE functions yields KSP0 -> raw Hz bits3c2aa580 (.0104154348), KSP500000 ->3fbecec0 (1.4906845) -> getter500072, and KSP1000000 ->43552480 (213.142578). These live slider endpoints differ from the saved frequency constructor bounds; applying the saved bounds to live writes would alter the native parameter law. The inverse is deliberately not an identity slider roundtrip.

Actual getter case `14089cb10..14089cb6f` reads the authored source coefficient at concrete+560 and invokes `140879000` with frequency timing+570. Setter case `1408c18f0..1408c1948` invokes `140958a90` with that timing and stores the decoded coefficient at+560. With synchronization enabled and opaque timing byte+9 clear, generic setter `140514930` uses frequency timing virtual+40 `140753a70`: ordered f32 `12 - 11*x`, followed by imported CRT roundf. Generic getter `14088832a..140888368` uses virtual+38 `140753ab0`: `(12-raw_count)*f32(1/11)`, then explicit MAXSS zero and MINSS one. Consequently saved sub-one/negative authored counts remain preserved, while their normalized frequency getter reports1000000. Native KSP setter general branch `14078a82f..14078a848` first converts the integer to double, multiplies exact double1e-6 and narrows to f32. The normalized getter branch multiplies in f32 by1000000, adds f32.5 and truncates. Captures and executable codec landmarks are retained in ignored `artifacts/core-native/lfo-frequency-713-codec-set-get.asm` and `lfo-frequency-713-native-landmarks.json`; this is bounded native function execution, not native instrument audio.

The common post-set internal-source path `1408c3d16..1408c3d43` invokes Group preparation. Previously closed same-kind LFO copy `1405e4f50` replaces the prepared frequency step without replacing mutable phase, fade countdown/level or target lag. Production now resolves only already prepared retriggered deterministic Digital sources by their original source slot and generic−1. It writes the authored coefficient to both pitch and volume source copies before the existing `overrides::write_group` hook refreshes the fixed source timing row at current raw BPM. The authoritative baseline/effective settings and NativeState source address are reused; the persisted edit keeps the requested slider while getters evaluate the actual inverse of the current authored coefficient. Existing control-point timing, source pause and planning clocks are unchanged. Shared sources, analog-table sources, wavetable groups, missing original slots and source-unit/phase/fade setters remain unadmitted. The synchronized adapter does not establish arbitrary out-of-range i32 clamping: the new API explicitly rejects frequency values outside the documented 0..1000000 range before mutation/capture rather than inventing that behavior.

The isolated registered `engine::params::tests::lfo_frequency_tests::live_digital_frequency_uses_native_units_and_keeps_paired_phase_in_complete_pcm_without_heap` is staged. It preserves paired source identity through the existing Group wire serialization, then exercises actual KSP setters/getters in full6144-frame patterned stereo Engine PCM. A separate source-free Engine receives exterior per-frame pitch plus independently retained volume lag/interpolation, driven by literal native coefficient/step receipts and triangle geometry. The scenario covers progressed retargeting, frequency changes while source-bypassed, a current-raw120.0005 authored refresh inside the global tempo threshold, mute/unmute with source progression, fixed/irregular fragments, physical release/completed zero tail, callback heap and one original-slot NativeState edit/restore. Out-of-range writes are inserted in the same PCM timeline and must preserve settings, phase trajectory, edit revision and current getter; legal unsynchronized setter500000 must read back500072. This fixture proves the accepted parameter/source orchestration and existing deterministic waveform, not generic LFO/native instrument dispatch parity or newly parsed NKI coverage. No Cargo/build/tests were run by this author; root owns the central gate and functional check.

Root checkpoint68590 compiled the registered retriggered Digital frequency proof,
then the full6144-frame actual KSP/PCM/ordered-restore case passed (0.05s).
The two immediate invalid-callback persistence inspections refresh one
worker-prepared snapshot in place inside the allocation guard; creating a fresh
snapshot there was fixture allocation, corrected before this functional check.
No production heap exemption or PCM assertion was removed.

### Live strict shared synchronized Digital frequency (staged)

The same native parameter713 synchronized coefficient codec now reaches the
already admitted direct v71, Retrigger0, zero-fade Digital source. The primary
KSP frequency/original-slot and Kontakt synchronized-count references above
apply unchanged. Same-kind native copy `1405e4f50` replaces the prepared step
without copying mutable phase; the native shared setup initializes generation
only when allocating its block storage. Existing committed generation ownership
therefore remains the boundary: an authored frequency write refreshes timing at
current raw BPM but does not replace cached points, captured forecast step or
committed phase. The next generated buffer consumes the refreshed row.

`Address::resolve` and `params::write` reuse the existing LFO source validator
for fixed source slots, deterministic shared zero-fade geometry and paired-copy
identity. The live shared extension additionally requires synchronized timing
and the original nonextended fade mode. Prospective writes require count1..12;
saved finite counts below1 remain authoritative/readable and use the already
closed floor-to-one converter. Unsynchronized shared frequency, v73 shared,
analog/random sources, unknown waveform/timing geometries and live phase/fade
writes remain unsupported. No Player, Voice, Bank, source clock, cache owner,
serialization field or NativeState key was added or changed.

The staged actual-NKI/cache test is
`engine::lfo::timing_tests::shared_sync_tests::frequency_tests::saved_shared_sync_live_frequency_keeps_committed_points_and_next_generation_pcm_without_heap`.
Literal native steps were captured by bounded execution of actual PE timing
getter `140a736a0`; receipts are retained in ignored
`artifacts/core-native/lfo-shared-live-frequency-native-steps.json`. A fresh
creator NKI with saved shared Digital count3 is parsed, cached and loaded into
Engine. Actual KSP callbacks write count1 at301, count12 at799, count6 while idle
at1409, and count2 at1901. Each immediate getter is inspected with in-place
persistence refresh. Cached points, generation, current/next phase and captured
step must remain exactly unchanged by each write. Count12 preparation at raw
83.0005 is checked independently even though that row is superseded before a
new receiver; the current771 generation keeps its count1 points.

A source-free reference Engine receives exterior triangle/interpolation tuning
from literal native step receipts over3072 stereo frames. The shared generations
at514 and2056 expose changed frequency without a phase reset; the second owner
at37 reads the same source trajectory, and the fresh owner at1542 resumes the
idle retained phase with count6/raw90.0005. The proof retains declared257-frame
host windows, fixed/irregular subdivision, source bypass/resume, muted source
progression, inside-band raw tempo changes, zero callback heap, exact physical
owner endings975/975/2296 and completed zero tail. This tests the accepted local
shared consumer policy with the native prepared step law, not independently
measured Kontakt receiver scheduling or native instrument audio. Production and
cfg are staged; root executes all Cargo/build/functional checks.

The first root functional attempt after compiled gate20062 stopped at the new
fixture's pre-render baseline assertion: controller callbacks queue parameter
writes, so baseline/effective settings still held count3 immediately after the
callback. The fixture now retains the immediate pending KSP getter check, then
checks the applied authored/effective count and prepared timing after the next
real render interval. Its cached generation/points/phase/step invariant spans
that actual application. Literal future-generation tuning and complete PCM
assertions are unchanged. This is a cfg timing correction, not a DSP change;
functional revalidation remains pending root execution.

### Live strict shared unsynchronized Digital frequency (staged, 2026-10-05)

The strict shared v71 Digital, zero-fade owner already renders saved Hz sources,
but its live frequency address rejected them. Native parameter713 uses the same
unsynchronized codec as the previously admitted retriggered source. Timing
getter `140a736a0..140a736f5` returns the authored coefficient directly when
timing byte+8 is clear; the synchronized constructor bounds are bypassed.
Frequency setter `14095ecd0..14095ed8d` multiplies that f32 coefficient by the
source's f32 control-rate reciprocal, widens the product and stores double step
at+B8. It writes no phase+B0. The fresh bounded capture is retained in ignored
`artifacts/core-native/lfo-shared-unsync-frequency-step.asm`; previously retained
same-kind copy and shared setup captures establish step copy without phase copy
and generation initialization only when allocating the shared buffer.

Production admits the existing nonextended shared geometry through its original
frequency slot/generic−1. Prepared unsynchronized Hz must be finite and positive;
raw saved import keeps its constructor-range guard. This distinction is needed
because actual native slider endpoints are Hz bits3c2aa580 and43552480, slightly
outside the saved constructor bounds. Codec receipts and exact control-rate
steps remain in `lfo-frequency-713-native-landmarks.json`. The current committed
generation retains its points, captured step and phase correction; the next
generation uses the updated coefficient. Bypass, idle and receiver ownership
use the existing shared implementation. No new source state, waveform law,
serialization field, phase setter or shared fade mode is added. Synchronized
shared writes still require count1..12, and out-of-range KSP units still reject
before mutation. A separately authored actual NKI/cache, live-KSP, complete PCM
proof is being staged; no native instrument audio or new native receiver
scheduling parity is claimed. Root owns compilation and functional execution.
