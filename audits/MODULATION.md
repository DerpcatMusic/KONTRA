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
  u8    flags          (0x10 always; 0x04/0x02 unknown)
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
| intensity | high (storage), medium (scale) | Always 0..=1 locally, never negative. `pitch` from pitch bend is 0.1667 in 214k records (0.1599 in 168), and 2/12 matches the usual ±2 semitone bend. `Group::pitch_bend_range()` therefore uses intensity × 12. |
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
| 8 | Release trigger counter | – | RTC_VOLUME/PITCH/ATTACK |
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

**There is no LFO (or any other modulator kind) anywhere in the corpus**, so the LFO
chunk layout cannot be discovered from local data and LFOs are not implemented. An
LFO would not sit in the `0x07` BParEnv wrapper, so its group fails with a
"modulation not imported" warning instead of being misread. No internal modulator
targets pitch or pan. Every internal volume target stores intensity 1, no invert and
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
pressure. Not applied: Script/RTC, random, release velocity, poly aftertouch,
`Unassigned`, and the remaining `Module` targets (effect parameters); no pan target
exists in the local corpus. Velocity → volume always goes through the shaper.

Envelope-time targets: 953 external assignments drive `ahdsr_attack` (VEL, PB, RTC,
KP) or `ahdsr_release` (PB), all in Pacific, CHORUS, Vista and Una Corda. The `slot` is
the internal-modulator slot (high: Pacific groups ordered FLEX, AHDSR address the
AHDSR as slot 1, groups with the AHDSR alone as slot 0). Their shapers map into
0.59..1 (Pacific VEL_ATTACK: velocity 0 → 1, 127 → 0.59; PB_ATTACK release: 0.81 at
rest, 1 at full bend), which reads naturally as a time factor, so they scale the
stored time. Whether Kontakt scales the time or the normalized knob value is
unverified. Solo has none; Vista's are RTC-driven (not modelled), so no validation
library exercises them.

The decoded `invert` flag stays **ignored**. Vista Full Strings Sustains is decisive:
its four CC100 crossfade layers (`susdyn1`-`4`) carry bell shapers peaking at
CC 0.26, 0.44, 0.78 and a ramp from 0.56, so they play upright and sum to about 1;
the `cl` (close) and `dc` mic copies of layer 3 store the identical shaper, one with
invert set and one without, and layer 1's `BALANCE_COMP` shaper likewise differs only
in the flag between siblings. No reading of the flag (after the shaper, before it,
or as a sign on intensity) makes both copies play the same, so it cannot be Kontakt's
volume inversion as stored. Afflatus sets it on rising velocity/CC shapers, Areia and
Solo almost never. Applying it silences or inverts layers that clearly play upright.
The GM reset defaults hold before any input (CC7 127, CC10 64, CC11 127, rest 0), and
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

## Corpus run (`kontakto inspect-mods` over every local NKI)

All 778 NKIs import with no modulation errors. Stored pitch-bend range is 2 semitones
in every group that has one (1.92 in 168 Dolce groups). `cc_volume` is CC 111/110
(Areia, Solo, Dolce, CHORUS), 11 (Afflatus, Pacific) or 100/101 (Vista, Pacific,
CHORUS). Most groups have a volume AHDSR; the ones without it (Vista, some Dolce,
CHORUS and Pacific groups) have none stored.

## Not verified

- How Kontakt combines intensity, invert and shaper for volume and pitch (the
  multiplicative model above is inferred from stored crossfades). No Kontakt
  reference renders are available.
- The attack and flex curve steepness, the flex level scale beyond 0/1 and its
  unknown index and tail, the envelope-time modulation law, 0x4A dynamics, the source
  module, internal-modulator bypass, the extra AHDSR flag and records, and the
  ext-mod unknown bytes and id.
- LFOs: none in the corpus, so neither their layout nor their behaviour.
