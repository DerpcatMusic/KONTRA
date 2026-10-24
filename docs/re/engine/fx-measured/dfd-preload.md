# Dolce DFD channel preload window: does Kontakt floor the stream head on it?

Date: 2026-10-24. Branch `measure/dfd-preload` from `origin/main` (d46520826). Measurement only: no KONTRA head sizing change.

## Question

KONTRA reads `dfd_channel_preload_size` (`crates/sampler-kontakt/src/library.rs`, the "DFD preload window not applied to stream head" diagnostic) and does not use it for `head_frames`. Does Kontakt 8 treat the authored window as a floor on the head, so that start latency or head length changes with it, or does it ignore it for start timing?

## Conclusion

**Unknown.** Not settled by this run.

- The decisive test (same instrument, authored window changed, onset compared) could not be run. The changed-window copy is blocked (below).
- The one comparison that is possible (two authored windows on two different instruments) is confounded by articulation and gives no window-only effect.
- Start timing is measured here. Head length and memory are **not** measured (no method was run for them).

## Authored values (read, not guessed)

`Program::params().dfd_channel_preload_size` from `ni-file` via `sampler_kontakt::read_chunks`, all 77 NKIs under `Audio Imperia Dolce/Instruments/`:

| authored bytes | KiB | count | instruments |
|---|---|---|---|
| 61440 (`0xf000`, init default) | 60 | 66 | e.g. `01 7 1st Violins/Dolce - 03 ... - Sustained Con Sordino.nki`, `... - 04 ... - Harmonics.nki` |
| 73728 (`0x12000`) | 72 | 11 | `Dolce - 01 ... - Legato.nki` and `Dolce - 02 ... - Sustained.nki` in each section (1st Vln, 2nd Vln, Violas, Celli, Basses), and `06 Ensemble Patches/Dolce - 01 Ensemble - Sustained.nki` |

Full list: [dfd-preload-authored.tsv](dfd-preload-authored.tsv). 73728 = 12 × 6144, so it is on the `0x1800` grid the window law rounds to.

## Test matrix (what was run)

One `oracle-slot` session, `tools/oracle/capture.py --batch`, 6 s slots, default scenario (notes at 0.2 / 1.6 / 3.0 s after slot start, keys 60/64/67). Three repeats per instrument as three slots of the same session.

| instrument | authored bytes | changed bytes | repeat | note-1 onset, −20 dB (ms after note-on) | −30 dB (ms) | peak (dBFS) |
|---|---|---|---|---|---|---|
| 01 7 1st Vln - 02 Sustained | 73728 | not run | 1 | 220 | 195 | −29.3 |
| same | 73728 | not run | 2 | 155 | 135 | −29.4 |
| same | 73728 | not run | 3 | 155 | 135 | −29.4 |
| 01 7 1st Vln - 03 Sustained Con Sordino | 61440 | not run | 1 | 65 | 30 | −28.3 |
| same | 61440 | not run | 2 | 65 | 35 | −28.2 |
| same | 61440 | not run | 3 | 75 | 45 | −28.2 |

Raw: [dfd-preload-raw.tsv](dfd-preload-raw.tsv). Analysis script: [dfd-preload-onset.py](dfd-preload-onset.py).

- Onset = first 5 ms RMS frame that reaches sustain RMS (200–400 ms after note-on) minus 20 or 30 dB. Threshold dependent; for distribution checks only.
- **Notes 2 and 3 are invalid** (`-50` sentinel in the raw file). Note 1 of each slot decays from the previous slot's note, so the reference is contaminated; the detector returns the window start. Only note 1 of each slot is used.
- Onsets are 30–220 ms after note-on here. The "about 1.5 s" figure in the task brief is not what this setup shows; the slot-relative timing is the same across both instruments, so the difference is not from the timing scheme.

### Sensitivity / positive control

Not a window control (no changed copy exists). The detector separates the two instruments by about 90 ms at −20 dB, but the instruments differ in articulation, so this is a sensitivity indication only. Repeat spread: 65 ms for the 73728 instrument (155–220 at −20 dB) and 10 ms for the 61440 instrument (65–75). With n=3 per instrument the spread is not resolved against a real effect.

### Memory

Not measured. No method was run.

## Why the changed-window copy is blocked

Checked on `Dolce - 02 7 1st Violins - Sustained.nki` (scratch, nothing written to the library):

1. **Container.** The file is an NI `hsin` NIS container. The preset is an encrypted `EncryptionItem` (`vendor/ni-file/src/nis/properties/encryption_item.rs`); the Kontakt chunk stream is inside it. A raw byte search for the 73728 value finds nothing, so the field cannot be patched in place.
2. **Key.** `sampler_kontakt::library_key` finds the library keystream from the `.nicnt`. Reading works.
3. **Round trip.** `NIFile::read` then `NIFile::write` with no change reproduces the 2,944,774-byte file **byte-for-byte** (identical = true). So the writer does not corrupt an unmodified file.
4. **Modification.** Changing the field needs: decrypt the preset chunk stream, patch `Program` public `dfd_channel_preload_size` (fixed width, so no size change), re-encrypt with `SubtreeItem::write_with_key` (keystream, `compressed = true`), and replace the `EncryptionItem` subtree in the tree before `NIFile::write`. `ni-file` has the writer and the keystream but no API to replace the preset body. This is not implemented here, and no changed copy was produced.

So the change is **feasible in principle** (round trip is exact and the key is available), but it needs a new write path in `ni-file` plus a Kontakt load check. That is the blocker. It was not guessed around.

## What would settle it

1. Implement the encrypted-preset body replacement in `ni-file` (or use the existing `write_with_key` on a patched chunk stream), and verify with a no-op round trip (done, identical) and a read-back of the new value.
2. Make copies of one 73728 instrument at 61440 and 73728 (same bytes otherwise), 3 repeats each, in one `oracle-slot` capture; compare note-1 onsets at −20 and −30 dB. A floor on the head would move onset or head length with the window; no movement means ignored for start timing.
3. Head length: needs a Kontakt-side observable (stream head read size), not onset. Not measured here.
4. Memory: Kontakt's resident size at load for the two copies (`oracle-slot` probe), not measured here.

## Raw files

- `dfd-preload-raw.tsv`: per slot and note, onset at −20 and −30 dB, with authored bytes and peak.
- `dfd-preload-authored.tsv`: authored `dfd_channel_preload_size` for all 77 Dolce NKIs.
- `dfd-preload-onset.py`: the onset analysis (reads WAVs from a scratch directory; not run against committed WAVs since WAVs are deleted).

Related: [DOLCE_ONSET_START_RANGE.md](../DOLCE_ONSET_START_RANGE.md) (capture latency ~34 ms, Dolce is route-driven: Controller 113 SampleStart, not the random start law). `k-sampler-source.md` G2 records the window as unrecovered and unapplied.
