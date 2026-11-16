# Voices: polyphony, stealing, mono lines, legato and glide

One design for how notes become voices. The formats translate into four IR
shapes. The engine plays them on the audio thread without allocating.

| IR | What it says | Engine |
|---|---|---|
| `Group.polyphony`, `Plan.voices` | most voices a group / the instrument holds | `admit.rs` |
| `ir::VoiceGroup` | a shared budget: `voices`, `steal`, `prefer_released`, `fade_ms`, `exclusive`, `per_note`, `legato`, `returns` | `voice_groups.rs`, `mono.rs` |
| `ir::Glide` | how a note's pitch arrives from the previous one: `scope`, `depth`, `seconds`, `per_semitone`, `legato`, `resume`, `curve` | `glide.rs` |
| scripts | `change_tune`, pitch-bend routes with lag | the script VM, routes |

## Polyphony and stealing

A voice group is a budget. With `per_note` it counts notes, not voices:
a note over budget steals *every* voice of the note it replaces together. The
victim is chosen by `steal` (oldest, newest, highest, lowest), released notes
first when `prefer_released`. Stolen voices fade over `fade_ms`. A note that
sounds several zones (layers, mic positions) is one unit. Counting voices
instead would let a mono layer with uneven zone counts keep two notes
sounding (#1560's CodeRabbit finding). Ownership is a stamp per note, so
there is no allocation. `exclusive` chokes the other voice groups of its set
(hi-hat open/closed).

## Mono lines: legato and last-note priority

A voice group with `legato` or `returns` keeps a *line* (`mono.rs`): the note
sounding it, and up to 8 older notes whose keys are still down (newest last).

- **New note, line busy, `legato`:** the new note *takes over* the sounding
  voices. They keep playing (no new attack), change owner, and move to the
  new pitch, sliding when the group glides. The new note's own zones in that
  group stay silent.
- **New note, line busy, no `legato`:** the new note starts its own voices.
  The budget (`voices` 1, `per_note`) fades the old note over `fade_ms`
  (retrigger).
- **Release of the line's note, `returns`:** the newest older key still down
  comes back. With `legato` it takes the voices back (and slides back).
  Otherwise its zones start again in that group alone. A note that only
  waits in a line stays alive (`waits_in_line`) until it comes back or its
  key goes up.

## Glide

`glide.rs` works out a note's slide once per scope, at its first voice:

- **From:** the scope's previous note. With `resume` it starts from where
  that note is sounding now, mid-slide or not. Otherwise it starts from that
  note's own key. The first note of a scope does not glide.
- **When:** always, or with `legato` only while another note of the scope is
  held.
- **Distance:** `(from − key) × depth` semitones.
- **Time:** `seconds`, or `seconds × |interval|` with `per_semitone`.
- **Shape:** `covered(p)` with `k = (1 − curve)/2` clamped to [1e-4, 0.9999],
  `r = (1 − k)/k`, `f(p) = (r^(2p) − 1)/(r² − 1)`. Curve 0 is linear in
  semitones; a positive curve leaves slowly and arrives fast.

The slide is read at each control step (256 frames). Between steps the voice
ramps its playback step frame by frame (`kontra_voice::Ramp`) to where the
slide will be at the next step. The cursor stays a closed form
(`step·k + per_frame·(r(r−1)/2 + held·frames)`), so the pitch moves every
frame and the result is identical whatever the host block size (tested at
32 and 384 frames).

## The laws

### UVI (Falcon / UVI Workstation)

From the Falcon manual (§ Portamento, falcon.txt:1086) and the unpacked
binary (r2 on `uvi.analysis-only.bin`). Spot-checked addresses:
`0x141389af7` (|Δkey| × time), `0x141389b61..b75` (k = (1−c)/2) and
`0x141cfa9dc` (steal fade 0.01 × rate).

| Layer `PlayMode` | IR |
|---|---|
| 0 Poly | no line, no glide |
| 1 Poly Portamento | glide, `resume` false: every new voice glides from the previous key |
| 2 Mono Retrigger | line, `returns`; a new note fades the old over 10 ms |
| 3 Mono Portamento | line, `legato`, `returns`; glide `legato`, `resume` |
| 4 Slide | as 3, and a fresh note after release glides too (glide not `legato`) |

`PortamentoTime` is `seconds` (default 0.03). `PortamentoMode` 1 is
proportional, i.e. `per_semitone`. `PortamentoCurve` is `curve`. The steal
fade is 10 ms, linear.

**Verified on AO *Coline MW*:**
- The program has a PlayMode 3 layer with PortamentoTime 4.8317 s, Mode 0
  and Curve 0.
- The phrase was 60 held, then 67 legato at 1.0 s held to 7.0 s. Only the
  gliding groups were rendered (`KONTRA_ONLY_GLIDE=1`).
- The voice's source-position slope (`KONTRA_POSITIONS=1`; the layer is an
  unpitched texture YIN cannot track) shows a single voice kept throughout.
- That voice moves 0 → 7 st at 1.4488 st/s, against 7/4.8317 = 1.4488 by
  the law, with no deviation from a line, starting at the note-on.
- On release it slides back toward the held 60 at the same rate.

### Kontakt

The Glide modulator (Modulation → Others → Glide), routed to the Source
module's pitch at an intensity. Measured in Kontakt 8.13 with
`tools/kontakt-oracle/probes/glide.lua` through
`tools/kontakt-oracle/gui_session.py` (no API adds a modulator). The probe is
a 261.6 Hz sine, glides of 4 and 12 semitones up and down, one voice at a
time, and a YIN pitch track. Results:

- **Linear in semitones** (within 0.02 st of a line).
- **Speed mode** (stored `mode` 1): the time is per semitone, a constant
  rate. 142.3 ms gives 140–144 ms a semitone; 418.7 ms gives 413–423.
- **Time mode** (stored `mode` 2): the time is fixed whatever the interval.
  A stored 1224 ms gives 1208 ms over 12 st and 1238 ms over 4 st; 47.5,
  142.3 and 418.7 ms agree within 4%.
- **The file stores milliseconds** in `Glide.values[0]`. A patch saved from
  the probe reads `mode: 2, values: [1224.2, -1.0]` (display "1.2k").
  Kontakt 8.13 stores this modulator under category 5, which the reader now
  accepts.
- **From the previous note's key**, never from the pitch it reached: a note
  played while the previous one was still gliding (at 67.96 on its way down
  to 60) started from 60. So `resume` is false.
- **Not legato-only:** it glides from a key released 0.5 s earlier. So
  `legato` is false.

| `$ENGINE_PAR_GLIDE_COEF` | 0 | 150000 | 300000 | 450000 | 600000 | 750000 | 900000 |
|---|---|---|---|---|---|---|---|
| display (ms) | 0.00 | 3.8 | 15.0 | 47.5 | 142.3 | 418.7 | 1.2k (1224.2 stored) |

Writing `$ENGINE_PAR_GLIDE_COEF_UNIT` from KSP changed nothing: it reads back
0 and the audio is the same. The mode is set in the editor.

**KONTRA on the same saved patch:** 9.9 st/s, so 60 → 72 takes 1.23 s.
Kontakt played the same patch at 9.93 st/s.

**Intensity, range and the previous note** (`probes/glide_intensity_groups.lua`,
`probes/glide_range.lua`, intensity set from KSP with
`$ENGINE_PAR_INTMOD_INTENSITY`, depth = (2x − 1)³):

| depth | 60 → 72 | slide |
|---|---|---|
| +1 | 28.9 st/s | 12 st in 415 ms |
| +0.5 | 14.3 st/s | 6 st in 420 ms |
| +0.25 | 7.04 st/s | 3 st in 426 ms |
| −0.5 | −14.4 st/s from 78 | 6 st, overshooting |
| −1 | −29.1 st/s from 84 | 12 st, overshooting |

- **Intensity** is a signed share of the interval over the same time
  (`depth`, negative from beyond the note).
- **At most 12 st:** jumps of 17, 19, 20, 24 and 38 st all slid 12 st in the
  same time (`range` 12). That the cap applies before the intensity is
  inferred; an intensity on a wider jump was not measured.
- **The previous note is the last one heard** (`from_heard`), even one a
  script ignored. The probe's trigger note 36, dropped by `ignore_event`, was
  where the first note slid from. "The first note does not glide" above was
  that artefact.
- **Every group glides from the instrument's previous note**, including one
  it never played (kept from it by `$EVENT_PAR_ALLOW_GROUP` or by its key
  range). So `scope` is 0.

Not measured: the cap in Speed mode, and a group without Glide next to one
with it. One unexplained miss: once in 24 transitions Kontakt did not glide
(Speed 142.3 ms, 72 → 60).

**Release triggers** (`probes/release_trigger.lua`, with Release Trigger
switched on in the editor; no Lua or KSP route sets it):
- Release voices count toward the instrument polyphony and the voice-group
  limit. They steal and can be stolen.
- A release still fires after its note's own voice was stolen.
- **Monophonic** (on by default with Release Trigger) is per key: playing the
  key again cuts its sounding release voice at the new note-on, and other
  keys' release voices sound on (`Group.repeat_cuts`).
- With t(ms) 0 and nothing routed, the release level is the same after
  holds of 0.1, 1 and 3 s.

**Which scripts hear a played note's release**
(`probes/release_callbacks.lua`, Kontakt 8.13, two script slots; each
`on release` marks the key it heard). The script that played the note runs
its own `on release` when the note ends by its duration, by its parent's
release (`-1`), or by a `note_off` sent from `on note`. A `note_off` it
sends from its own `on release` skips that script, and only later slots
hear it. KONTRA used to skip the player after any `note_off` of its own
(an inference from Pacific, #1210).

**Script offsets in DFD.** The KSP manual (`play_note()`) says that in DFD
mode a sample offset greater than the zone's S.Mod is ignored, and the sample
plays from its start (`Group.offset_limited`).

Scripted glides (`change_tune` steps from KSP, pitch-bend routes with lag)
stay script-driven. They need nothing from `ir::Glide`.

## Survey, and what won

| | time or rate | legato | shape | from |
|---|---|---|---|---|
| Surge | time, or constant rate | fingered = legato-only | log / lin / exp curve | the current glide position |
| sfizz | — (pitch CC smoothing only) | — | — | — |
| HISE | script `addPitchFade(ms, …)` | script decides | linear | the voice's pitch |
| UVI | time, or per semitone | per play mode | exponential-segment curve | current position (3/4) or previous key (1) |
| Kontakt | Time or Speed (per semitone) | no | linear | the previous key |

One IR shape covers all of them: a duration that can be per semitone, a
legato flag, a resume flag and a curve. What won, and why:

- **A per-frame step ramp with a closed-form cursor** won over per-block
  stepping (Surge, UVI's 32-sample blocks). It is sample-accurate,
  block-size independent and costs the same.
- **Glide on the voice's own step** won over a pitch modulator in the
  modulation graph. It needs no route, it composes with bend and tune, and
  legato take-over is just re-aiming the voice.
- **Note-counting voice groups** are the foundation. Stealing by note
  removes the uneven-layer bug, and mono lines are a voice group with a
  memory of held keys, not a separate mode.
