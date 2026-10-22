# Corpus gaps for the voice engine (survey 2026-10-22)

Source: `scratch/survey/full-20261022.jsonl` (`corpus-health --tier full`, 1395 instruments).

## Silent at a covered key (18)

- **15 instruments: "note suppressed by its script".** Dolce (violins, violas, celli, ensemble
  Spiccato/Staccato/Pizzicato/Energetic) and Pacific Ens Strings (Trills, Solo Harp Harmonic
  Pluck). The Dolce scripts take the tonal branch: `change_note(...)` shifts the event by the
  neighbour-borrow offset, then `ignore_event` discards the original, so key 60 has no sound by
  design (`Dolce - 07`, script line ~68795). The survey plays key 60 with default UI state and
  can't tell an intended remap from a dropped note. Not yet verified against Kontakt: the
  change_note-then-ignore_event path needs a Wine-oracle render at the remapped key.
- **3 Conflux multis (Ray Line, Symmetric Mindcontrol, Spiral Echoes):** no sound record, so
  the survey never rendered them. Not investigated.

## Missing the 64-frame deadline (51)

- All 51 are heavy stacks (peak voices 18–72 against a median of 2 across the corpus), mostly
  Areia legato and trill sets, plus 2 multis.
- Median block p50 is 0.059 ms (corpus 0.008 ms); max spikes reach 6.2 ms. The 349 stream
  underruns in these instruments are the likely cause of the spikes, not steady DSP cost.
- The survey ran at load averages of 7–24 on a shared machine with concurrent builds, so the
  spikes are not reproducible from this file. Re-run on a quiet machine before fixing.

## Keys beyond the runtime pitch range narrowed (93)

- Fixed in the resample PR: the runtime covered four octaves, so keys more than 48 semitones
  above a zone's root were dropped. The runtime now covers eight octaves (MAX_STEP 256). The
  Areia 04 Double Basses legato instrument goes from 144 narrowed zones to 0.
- Zones still narrowed: pitch beyond eight octaves. Not measured against Kontakt.

## MPE probes unexercised

- No investigation done in this pass: the survey does not drive MPE pitch or pressure responses.

## Follow-up (2026-10-22, second pass)

- **Dolce Staccato, Wine-oracle render** (`tools/kontakt-oracle/render.py`, notes 60/62/58/65 each
  1 s long at 0/3/6/9 s): Kontakt is silent during each held note and plays a burst 1 s after each
  note-on, i.e. at note-off, with a pitch matching the key (the 60 burst carries its octave
  harmonic, as does 58). KONTRA's `render-kontakt` is silent for the same four notes, also with
  `--no-scripts`. So the difference is not only the script branch. The instrument's zones or
  articulation selection give KONTRA nothing at these keys. Not yet fixed. Open: which zones
  Kontakt selects at note-off, and whether the default articulation state is active in KONTRA.
- **Conflux multis:** `render-kontakt` now accepts `file.nkm#N` (program N). Ray Line program 0
  ("Hill Spike") has no zones. Program 1 ("Conflux", 5855 zones) is silent at key 60, with 971
  unsupported items: 550 external modulation from RandomBipolar, 211 modulation of a modulator's
  intensity, 78 filter law. Those are the owning modulation and filter lanes. The survey's
  Conflux records stop at `started` (the PC crashed mid-run), so their sound check never ran.
- Not done: the 51-stack deadline re-run on a quiet machine, and the MPE probes.

## Dolce note-off burst: where it comes from (third pass, 2026-10-22)

- Kontakt's note-off burst is not a release-trigger zone. Every zone at key 60 (384 of them) has
  `trigger: Attack`, so no zone selection can answer a key-up. The instrument sets
  `NO_SYS_SCRIPT_RLS_TRIG`, so the script plays the release samples itself from `on release`.
- Oracle per-second peaks for the four-note file (`dolce07.wav`): 0.020 / 0.022 / 0.014 / 0.017 at
  1, 4, 7, 10 s, each exactly at a note-off. KONTRA's render (`--no-scripts` or not) peaks
  0.0002 at 0, 3, 6, 9 s, which is the attack noise floor of the amplitude envelope (15 s attack).
- KONTRA does run the on-release callback: `advance_release_stage` fires for both keys, and the
  release stage behaviour executes. The script's `play_note` never executes. The native `Play`
  instruction is not reached from the release, so the branch that plays the release samples is
  not taken. The branch is gated by developer/page/UI state (`$switch_developer_enable_performance`,
  `$page_current_developer`, `get_event_par($EVENT_ID, $EVENT_PAR_SOURCE)`, and the
  `%store_menu_developer_type_art` select). Which gate evaluates differently is open.
- Not the cause: group gain, output routing, sample decode (peak 0.042), controller 111 at 127
  (still 0.003 peak), velocity curve.
- Caveat on evidence: `message()` text is kept only up to 1000 messages per run, and the `NOTE
  RELEASE` message is lost behind the init messages, so message output cannot show the release
  branch. Use the Play instrumentation instead.
- Tooling trap: `sampler-native render-kontakt*` refuses to overwrite an existing output and exits
  with "File exists (os error 17)" with no other output. An old output path can look like a
  silent render. Check the exit status and the file's mtime.

## Dolce Staccato note-off burst: gate search (2026-10-22, fourth pass, no fix)

Root cause not established. No code change in this pass. Corrections to the third pass first.

- **The third-pass attribution to the `play_note` at script line 77297 is wrong for this
  instrument.** Persisted state: `%store_menu_developer_type_art = [1, 0]`, so articulation 0
  is `$ART__SHORT` (constants: LONG 0, SHORT 1, LEGATO 2, PORTATO 3). Line 77297 sits in the
  `case $ART__LONG` block (76962-77333). The SHORT block (77455-77490) only runs
  `note_off(%_event_id[...])` when CC64 < 64 and updates the labels. Nothing plays there.
- The tonal release path for this instrument is `on release` 76019 (`switch_developer_enable_performance`
  = 0, `page_current_developer` = TONAL, `get_event_par(..., SOURCE)` is -1 for host input, so
  the block runs) → select 76022 → SHORT 77455. The common block after the select (77491-77536)
  is choir-only. No `play_note` exists on the tonal SHORT release path.
- The performance-mode release (`switch_developer_enable_performance = 1`, 77739-77794) does
  play `play_note` (77757/77759), but the persisted value is 0 and no script line assigns it.
- SHORT note-on calls `reset_rls_trig_counter($EVENT_NOTE)` (line 70652). The note-on tonal path
  calls `change_note` + `ignore_event` (68793-68795) for keys in range.

Kontakt oracle (`dolce07.wav`, 48 kHz float, notes 60/62/58/65): output is exactly 0.0 through
1.00 s, first nonzero at ~1.02 s, peak 0.0199 at ~1.2 s, below 0.001 by 2.0 s. The burst starts
at note-off and is not a held-note tail.

Where the release sound would have to come from: the native release-trigger group path, not the
script. KONTRA gates it off as follows:

- `SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)` (Staccato script line 81) →
  `owns_release_triggers` (`crates/sampler-ksp/src/lib.rs:1753`) → `with_script_release_triggers`
  (`lib.rs:905`) → `run_release` with `musical = false`
  (`crates/sampler-core/src/prepare/selection.rs:1216`). Native release groups are silent.
- `crates/sampler-ksp/tests/groups.rs:409-424` encodes that reading ("native release groups do not
  fire"). It is the team's interpretation, not checked against Kontakt.
- Kontakt's generated system-script code (decompile bucket
  `Research/KONTRA/source/kontakt8/0140870000.c` lines ~4816-5022) emits the release-trigger logic only under
  `USE_CODE_IF_NOT(NO_SYS_SCRIPT_RLS_TRIG)`: the pedal exit, `disallow_group`/`allow_group` of
  `rlsTrigGroups`, and `_reset_rls_trig_counter` on note-on. That shows the condition removes the
  script-side gating. It does not show that native release groups stop sounding under it.

Still open (the exact gate is not yet pinned):

1. Whether the Staccato key-60 groups carry `release_trigger = true` and `rls_trig_counter > 0`
   (the release counter law in `crates/sampler-kontakt/src/library.rs:37-47`). Needs a per-group
   dump from the `.nki`; `object_survey` only aggregates.
2. Whether Kontakt sounds those native release groups when `NO_SYS_SCRIPT_RLS_TRIG` is set. The
   candidate fix is to keep native release playback and only drop the script-side gating. That
   contradicts `groups.rs:412`, so it needs an oracle check before any change.

Not done: no fix, no new Kontakt render, no code PR. The NI manual page
(`ksp-manual/en/advanced-concepts`) returned HTTP 403 to WebFetch, so the condition's documented
semantics are unverified.

## Dolce Staccato burst: release-trigger hypothesis falsified (2026-10-22, fifth pass)

Steps 1 and 2 of the follow-up, settled from data and the decompile. No fix.

- **Groups (`.nki` read via `sampler_kontakt::read_chunks`; the `inspect-kontakt-chunks` dump
  fails on this file with "Truncated at byte 39").** The instrument has 384 groups and 9600 zone
  records. All 384 groups have `release_trigger = false`, `release_trigger_note_monophonic = true`,
  `rls_trig_counter = 0`. Each key-60 zone (384) sits in its own group. So this instrument has no
  native release-trigger groups. The `NO_SYS_SCRIPT_RLS_TRIG` gate on `run_release` (fourth pass)
  cannot be the source of the burst.
- **Decompile (`Research/KONTRA/source/kontakt8/0140870000.c`, generator at `FUN_140876bd0`).**
  `NO_SYS_SCRIPT_RLS_TRIG` appears only as `USE_CODE_IF_NOT(NO_SYS_SCRIPT_RLS_TRIG)` around the
  generated system-script text for release triggers (~4816, 4929, 4940, 5003). When the condition
  is set, the generated release code is left out: the pedal exit, `disallow_group`/`allow_group`
  of the release groups, and `_reset_rls_trig_counter` on note-on. That means the script owns
  release handling. It matches KONTRA's reading (the flag silences the system's release path), so
  `owns_release_triggers` is not the defect. The `groups.rs:412` test is consistent with the
  decompile and is kept.
- **Where the sound comes from for SHORT.** The SHORT attack is `play_note` at script line 71673
  (`case $ART__SHORT` runs 70650-71704; `change_tune` by `2*$neighbor_borrow`). The release side
  for SHORT only calls `note_off` (77458). So the burst is a behaviour of the attack voice under
  `note_off`, not a release sample.
- **Discriminating facts.** Kontakt's held note is exactly 0.0 until note-off (not a low ramp).
  KONTRA's held note peaks at about 0.0002 (a ramp floor). A fix must reproduce the exact-zero hold
  and the post-note-off rise. Nothing has been verified on that route yet.

Open: the mechanism that makes a `note_off` on a silent attack voice produce a sound about 20 ms
later that peaks about 180 ms after that. The next probe should read the attack voice's envelope and
gain state at `note_off` in KONTRA (`crates/sampler-ksp` event state), not the release gate.

## Dolce Staccato burst: the mechanism is the cold-onset path; KONTRA never takes it (2026-10-22, sixth pass)

Probed KONTRA's `render-kontakt` (notes 60/62/58/65, 1 s each at 0/3/6/9 s, velocity 100) with
temporary eprintln probes (not committed; diff kept in scratch). Findings:

- **Admission.** All 216 source admissions of the Staccato sample are `resident = true`, `runway = 0`
  (`crates/sampler-core/src/stream.rs`, `check_source_ready`: a resident asset returns early). So no
  voice ever takes the cold path.
- **Note_off.** At frame 48128 (1.002 s) the key-60 family has 72 voices, all `held = false`,
  `runway = 0`, envelope level 0.01376. The release starts from that ramp level, so there is no burst.
- **Why Kontakt is exact 0.0.** The engine already has the cold-onset model. A cold voice
  (`Cursor::cold`, `cold_hold`, `render_starved`) is silent while its window is not resident, and
  `ownership.rs` releases a held voice with `release_onset`: release from sustain, and the clock
  waits for the first complete window (`pending_start_runway` 1019 frames, about 21 ms at 48 kHz).
  That is Kontakt's shape: exact silence during the hold, then a sustain-level burst about 20 ms
  after note_off. The 1019-frame runway matches the measured 1.02 s onset.
- **Conclusion.** The burst is produced by the existing cold-onset release path. KONTRA's offline
  render treats the Dolce sample as resident, so the path never fires and the attack ramp plays
  instead (about 0.0002). The gap is the residency/preload policy for this asset, not the
  envelope, the curve law, or the release gate. Envelope law and the `ATK_CURVE` coefficient match
  the documented converter (`docs/re/engine/k-mod-envelopes.md`).
- **Still open.** What makes Kontakt's window non-resident during the hold and resident at
  note_off (Kontakt's preload or stream policy for a sample at this start offset). The decompile
  bucket for the stream/preload path is the next thing to read. No engine fix is made: a residency
  policy change needs Kontakt evidence beyond this one oracle, and no vector test can be written
  until that policy is known.

## Dolce Staccato: preload-start residency hypothesis refuted (2026-10-22, seventh pass)

Tested the claim that a start offset past the DFD preload head makes the voice cold. It does not
hold for this instrument. No engine change.

- Program `dfd_channel_preload_size` = 61440 (`ni-file` Program params; decompile default 0xf000,
  readers around `0140570000.c:10935`, `0140580000.c:59`, `0140820000.c:7337`).
- All 384 key-60 zones: `sample_start` = 0, `num_frames` = 112800 or 113760 (stereo).
- `sample_start_mod_range` = 3841 frames (unit per `crates/sampler-kontakt/src/library.rs:3807`).
  The script's sample-start controller (251020, about 25%) gives an offset of about 960 frames,
  which is inside the 61440-byte head. Playback would be warm in a head-bounded model too.
- The script never sets a play position. Its sample-start controller feeds `ATTACK` (on note,
  line 65610), not the start.
- So a bounded head (head-policy PR #294, aaf2210) does not make this voice cold. The silence and
  the note_off burst still need another explanation. Open: the attack envelope and its
  ATK_CURVE/ATTACK law under `note_off`, and what the decompile says about attack-stage release.
