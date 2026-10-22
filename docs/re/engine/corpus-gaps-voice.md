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
