# Timing: how late the libraries sound, and auto-align

Orchestral samples keep the player's breath, bow or tongue before the beat, so a note
on the grid sounds late. Composers correct it by hand with a negative track delay
per articulation. This audit measures that lateness in the owner's libraries and
documents KONTRA's experimental **Auto-align timing**, which corrects it
automatically for each articulation.

Generated with `kontakto audit-latency <nki>...` (offline renders in KONTRA's own
engine; no audio written) and `kontakto audit-scripts` (script inventory: builtin
names and counts, no source). Libraries under `/mnt/MAIN_STORAGE/Libraries/Kontakt`.

## Method

- **Onset of a first note.** The instrument is loaded with its scripts, set to
  CC1 = 100 and CC11 = 127 and silenced. One note is played at the key with the most
  zones above the keyswitches, at velocities 48, 90 and 120 (the buckets ≤64,
  65–100 and 101+). The onset is the first 5 ms RMS window (1 ms hop) within
  −20 dB of the take's peak.
- **Legato transition.** A second note a major third up starts 1 s later, and the
  first is released 50 ms after that. The transition is when energy at the new
  note's first three partials (Goertzel, 40 ms Hann, 2 ms hop) comes within −20 dB of
  its peak. It is not reported when the old note leaks within 6 dB of that threshold.
- **Script delay.** The first voice is timed from the note-on in the same render.
  Every library starts a voice within 16 frames (one control block) for first notes
  and legatos. No script waits before playing, so all measured delay is in the samples.
- **Why −20 dB.** It lands within about 20 ms of the vendors' published figures for
  sharp attacks (Pacific marcato 53–60 ms vs 80 ms stated; Audio Imperia 117–132 ms vs
  125 ms). −12 dB fits slow swells better (Pacific marcato 76–80). One threshold cannot
  fit every attack. Vista is the outlier: 45 ms measured against 140 ms published.
  Its sustains start with a faint bow noise that −20 dB catches.

## Scripts (static inventory)

Counts are instruments whose scripts use the builtin. `wait` is almost everywhere, but
it sits in release, fade and listener paths, not before the note: the renders above
show voices starting within one block of note-on.

| Library | NKIs | play_note | wait | wait_ticks | wait_async | set_event_par(_arr) | listener callback |
|---|---:|---:|---:|---:|---:|---:|---:|
| Afflatus Chapter II Brass | 348 | 348 | 348 | 0 | 0 | 348 | 348 |
| Areia | 155 | 155 | 155 | 0 | 0 | 155 | 154 |
| CHORUS | 42 | 42 | 42 | 0 | 9 | 42 | 42 |
| Dolce | 77 | 77 | 77 | 0 | 0 | 77 | 77 |
| Pacific Ensemble Strings | 49 | 45 | 9 | 0 | 0 | 41 | 0 |
| Vista | 7 | 7 | 6 | 0 | 0 | 6 | 0 |
| Solo | 100 | 100 | 100 | 0 | 0 | 2 | 98 |
| Una Corda | 3 | 3 | 3 | 0 | 0 | 3 | 3 |

### Delay controls the libraries show (declared)

| Library | Control | Value |
|---|---|---|
| Pacific | "Sample Offset" | −180 ms on legato sustains, −80 ms on marcatos |
| Pacific spiccatos | "PLBK Offset" / "LV Offset" | −100 / −50 ms |
| Areia Core, CHORUS Traditional, Solo | unlabelled slider readout | −125 ms |
| Dolce Legato, Sustained | slider readout | 180 ms |
| Dolce Marcato | slider readout | 80 ms |
| Dolce Spiccato, CHORUS Energetic | slider readout | −80 ms |
| Audio Imperia (all) | "S. Start", "Pre Delay", "Smpl. Strt. Min/Max", "Fixed Sample Start" | Areia max 250; Dolce spiccato 20/100; Dolce Legato fixed start on, 180 |
| Afflatus | "Rel. Off." | 0 |
| Vista, Una Corda | none | |

KSP cannot report latency to a host, and Kontakt reports no instrument delay. These
panels are advice to the composer, or sample-start trims.

## Measured onsets

In ms after note-on, −20 dB, first note / legato transition. A range spans articulations
or velocities.

| Library, patch | First note | Legato | Velocity dependent | Declared / published |
|---|---|---|---|---|
| Vista 5 Violins | 45 (−12 dB: 153) | 196 (−12 dB: 242) | no | 140 constant |
| Pacific 16 Vln Legato Sustains | 197 | 168 | no | 180 |
| Pacific Spiccatos | 68–89 | | slightly | 100 |
| Pacific Marcatos | 53–60 (−12 dB: 76–80) | | no | 80 |
| Una Corda Pure | ~3 | | no | none |
| Areia Core: sustains | 113–140 | 108–234 | yes (legato) | 125; legato ~250 |
| Areia Core: pizz, Bartók | 117–121 | | no | 125 |
| Areia Core: spiccato fast / slow | 6–13 / 91–115 | | no | 125 |
| Areia Core: staccato, marcato, portato | 117–132, 99–147, 143–167 | | yes (marcato) | 125 |
| Areia Core: col legno, sul tasto, harmonics | 84–103, 140, 151 | | | 125 |
| CHORUS Traditional | sustains 139–189, staccato 126–136 | Legato rows silent | yes | 125 |
| CHORUS Energetic | 82–89 | | no | 80 |
| Dolce Sustained | 158 | 136 | no | 180 |
| Dolce Marcato | 36–51 | | yes | 80 |
| Dolce Spiccato | 27–52 | | yes | 80 |
| Solo Violin | 121–140 | 120 at v48, 218 at v90/120 | yes (legato) | 125 |
| Solo Trumpet | 106–122 | | no | 125 |
| Afflatus Mega Brass | 3–25 | not detectable | yes | none |
| Afflatus Solo Trumpet KS | 3–26 per articulation | 106 | yes | none |

Sample starts: Vista median 40215 frames into the file (the pad is trimmed away);
Areia and Solo 714; Pacific spiccatos 4800. Sample-start modulation maxima range
3841–187000 frames. KONTRA honours the static start; start modulation shifts attacks
the measurement cannot see (it measures at CC1 = 100).

Measuring takes 0.3–5.7 s per instrument, off the audio thread, after a
0.1–4.2 s load.

**Not measurable in KONTRA today:** the Audio Imperia legato patches (Dolce
Legato, Areia Performance Legato, CHORUS Legato rows) render silence. That is an engine
gap, not a timing fact. They get the declared figure if they show one, else 0.

## Public knowledge

- **Cinematic Studio Strings:** legato 333 / 250 / 100 ms by velocity (≤64, 65–100,
  101+), shorts 60, first notes about 100. **CSSS** is the same, with a Low Latency
  mode around 150. **CSB** 230 or 180 / 100. **CSW** 220 / 130 / 90. The velocity
  buckets here follow CSS.
- **Spitfire BBCSO:** 30–150 ms by articulation, with a Tightness control (CC18).
  **OT Tableau:** −180 / −250 / −160.
- **Audio Imperia (Nucleus, Areia):** a 125 ms pad on shorts and sustains; Areia
  legato about 250. **Vista:** 140 constant. **Pacific:** 180 on legato patches.
- **Hollywood Opus:** about 80. **Gabrielle Flute:** 50. **VSL Synchron:** none needed.
- **How composers deal with it:**
  - Negative track delay per articulation track: Studio One, Ableton, Reaper
    (media playback offset).
  - Cubase 15 Expression Maps "Attack Compensation", per articulation. A value-doubling
    bug was fixed in 15.0.6, and it cannot tell a legato first note from a transition.
  - Logic Scripter scripts. Dorico keyswitch ticks.
  - Plugins that fake latency (Latency Fixer, Voxengo Latency Delay) and CSS Delay Helper.
  - Bitwig has no track delay.
- **Constant or not:** shorts and first notes are close to constant per articulation.
  Legato transitions depend on velocity (speed) in CSS, Areia and Solo. Marcatos and
  shorts vary a little with velocity where softer layers have slower attacks.

## Auto-align timing (experimental)

App menu → Performance → **Auto-align timing**. Off by default.

- **Measure.** Each loaded part is measured once per instrument and program on a
  background thread (`kontra-timing`), in its own engine, as above. Results are saved
  with the part. Where nothing could be measured, the patch's one declared figure fills in.
- **Report.** The host is told one latency: the latest articulation of any part,
  capped at 500 ms (CLAP `latency.changed` and restart; VST3 `kLatencyChanged`).
  A new figure must hold for 1.5 s before it is sent, so measuring a rack does not
  restart the host once per part.
- **Hold back events, not audio.** Every host event for a part goes into a
  preallocated queue (2048 events per part). A note is held back by the reported
  latency less its own delay, so its attack lands on the grid. The delay is chosen
  from the articulation the note will play:
  - the keyswitch state (a keyswitch the router knows is taken over and replayed
    just before its note);
  - channel and velocity routing;
  - an articulation picked on the part's panel;
  - legato or first note (another note held);
  - the velocity bucket.
  Releases, per-note expression and controllers keep their distance to their note.
  All-notes-off goes after everything already held. Under a legato script (legatos
  more than 15 ms later than first notes) notes and releases stay in the order played,
  so a detached phrase does not become a legato.
- **Per part.** The part menu shows the part's lateness and where it comes from,
  per articulation, with "Play 10 ms earlier / later" (a manual figure for every note
  of the part), "As measured", "Exclude from alignment" (the part plays as late as the
  library does) and "Measure again". The part's facts line shows "−N ms".
- **Only while the transport plays.** While stopped nothing is held back, so live
  playing sounds as the library does. The reported latency stays the same, so the
  host is not restarted when the transport starts. Hosts differ in whether they
  delay live monitoring by it. When the transport stops, whatever is held plays
  at once.
- **No audio delay lines.** None of these libraries waits in its script before
  playing. A script that did would have that wait inside the measured onset, so the
  event hold already accounts for it, and the hold is never negative.

Code: `src/timing.rs` (measurement, `Holds`, `Scheduler`, `Align`),
`src/plugin.rs` (`align`, `plan`, the audio-thread hookup, `latency`),
`src/articulate.rs` (`Router::articulation_of`, `select`, `reach`), `src/ui/menu.rs`.

### Limits

- Live playing with auto-align on is heard late by the reported latency (up to
  500 ms). Record with it on; play in with it off or with "Only while the transport plays".
- One threshold (−20 dB) for every attack. Slow swells read early and faint noise
  reads early (Vista 45 vs 140 ms). Use the per-part figure where the ear disagrees.
- The manual figure is one number for the whole part. It replaces the per-articulation
  values rather than offsetting them.
- The measurement takes one key, CC1 = 100 and a major-third legato. Other keys,
  dynamics, sample-start modulation and legato intervals can differ. Where the scripts
  pick a legato speed from anything other than velocity, the speed is not seen.
- Articulations picked only by a script control the router does not know (no
  keyswitch or control in the part's list) use the instrument's as-loaded timing.
- The on-screen keyboard and audition are not held back.
- A latency change restarts the plugin in most hosts (1.5 s debounce).
- A part whose queue overflows (2048 events) plays the overflow at once.
- The Audio Imperia legato patches are silent in KONTRA, so they are not measured.
