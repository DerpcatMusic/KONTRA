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
