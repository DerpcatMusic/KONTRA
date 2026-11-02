# Parity corpus — twenty short recordings KONTRA has to keep matching

Twenty cases, each a 2.5-second note (or chord) rendered by **real Kontakt 8.14.1** and committed
as a WAV beside the instrument path and the exact MIDI that produced it. `run.py` renders the same
MIDI in KONTRA and fails when the audio drifts outside the case's own threshold. `tools/verify.sh`
runs three of them (Glory Days Trumpet 1, Fredonia Grand Organ, Electric Mint) on every build.

It exists because every parity number in `STATUS.md` before this was measured **once, by hand**. A
hand measurement is a claim about one afternoon: nothing re-reads it, so nothing notices when a
change makes it false. The corpus turns those claims into files that fail a build.

```
tools/parity-corpus/
  cases.json          the twenty cases: instrument (relative to `library_root`), key, MIDI,
                      expectation, references, threshold, why
  kontakt/*.wav       the references, two takes per case, captured from real Kontakt
  run.py              the gate: render in KONTRA, align, compare, exit non-zero on drift
  README.md           this file
```

## The gate never needs Kontakt

References are files and `run.py` is standard-library Python. A machine with the sample library and
a `kontra-render` binary can run the whole corpus offline. Kontakt is only used to *create* a
reference — never to check one. `--capture` exists for that and is not for CI.

## The measurement

| | |
|---|---|
| format | stereo, 32-bit float, 48 kHz, raw and unnormalised — absolute level is the measurement |
| note | velocity 100 held **2.0 s**, plus a **0.5 s** tail |
| key | **derived per case**, not assumed: the median of the longest run of keys the instrument's own zones sound on at velocity 100 (see `key_note`) |
| alignment | the onset difference between the two captures centres a 1 ms-frame envelope cross-correlation, searched ±10 ms and applied before any comparison; the found lag must be within **1 ms** of the case's recorded offset |
| level | integrated RMS difference, Kontakt − KONTRA, in dB |
| band RMS | RMS of the 1/3-octave band differences (40 Hz … 16 kHz) after their median is removed, in dB — `tools/oracle/compare.py`'s timbre metric, reimplemented here |

Mono would have been smaller and cheaper, and it would have hidden exactly the bug the diffusion
lane found: the right channel reading the wrong lane. Stereo is the point.

The alignment is onset-centred on purpose. A whole-envelope correlation is fine for a sustained
instrument and useless for a pad whose attack ramp differs from ours: on `unity-nostalgic-synth` it
peaked at `r = 0.06`, which is no peak, and the search walked to its edge and would have compared
two notes 196 ms apart. The onset is the landmark both captures always share; the correlation only
refines it. `align_method` in the run output says which one a case used, and `lag_ms` is what it
found.

## What the threshold bounds — the one rule worth reading twice

`baseline` in each case is where KONTRA stood against reference take `a` when the case was
recorded. The threshold bounds **movement from that baseline**, not the absolute distance to
Kontakt:

| case | level | band | moved | limit | verdict |
|---|---|---|---|---|---|
| `glory-trumpet1-multi` | −0.03 dB | 0.03 dB | 0.00 / 0.00 | 0.50 / 1.00 | matches |
| `fredonia-grand-organ` | −15.97 dB | 24.01 dB | 0.00 / 0.00 | 0.50 / 1.00 | **finding** (unchanged) |

A regression gate can only be green on the day it is written if it asserts what it can actually
assert: that today's audio is still today's audio. So a case that already disagrees with Kontakt
passes, is labelled `finding (unchanged)` and prints its absolute delta (and `--verbose` prints all
25 bands) on every run. The disagreement is on the record; it is not a licence to widen a
threshold. When a fix moves a case, the baseline is re-recorded deliberately (`--record`, reviewed
in the diff) and the finding count is expected to fall.

The alternative — passing only when absolute agreement is inside the floor — was rejected because
it fails on the first day for ten of twenty cases and would be switched off within a week, which is
exactly how the hand-measured numbers this replaces came to be stale.

## Why the thresholds are what they are

Each case was captured **twice, in two separate oracle sessions**. The spread between those two
takes sets the threshold, with a floor of

- **0.5 dB** level and
- **1.0 dB** band RMS,

so a case whose Kontakt takes agree to 0.03 dB is still allowed 0.5 dB — a threshold that flatters
the engine is not a threshold. The floors are `tools/oracle-mcp/parity.py`'s limits, themselves
set at 4.2× and 2.6× the Kontakt-against-itself noise floor measured on 2026-10-30.

Measured spreads, 2026-11-01: seventeen of the nineteen cases that have references agree
take-to-take within 0.24 dB level and 0.39 dB band, because Kontakt's round robin is a per-session
*counter* — render 0 of one session is the same sample as render 0 of the next. Two cases do not
fit that: `vintage-organs-c3-jazz` (0.52 dB level, 4.30 dB band) and `session-horns-keyswitch`
(1.04 dB band) pick a different sample between sessions, so their thresholds are wider, and
honestly so. `electric-mint-melody`'s takes differ by 3 ms in onset, which is why the alignment
floor is what it is.

Kontakt's take-to-take spread is recorded per case under `threshold.from_spread` so a wide threshold
can always be traced to the instrument rather than hidden in the runner.

## Where KONTRA stands today (2026-11-01)

| case | key | level | band RMS | |
|---|---:|---:|---:|---|
| glory-trumpet1-multi | 70 | −0.03 dB | 0.03 dB | matches |
| fredonia-grand-organ | 66 | **−15.97 dB** | **24.01 dB** | finding: KONTRA is 16 dB loud on a pipe organ |
| electric-mint | 68 | +1.03 dB | 0.93 dB | finding |
| electric-mint-melody | 68 | +1.67 dB | 1.28 dB | finding |
| haunted-strings-2 | 60 | −0.47 dB | 0.28 dB | matches |
| cherry-electric-bass | 72 | −0.31 dB | 2.07 dB | finding (timbre) |
| soundiron-harpsichord | 67 | −0.19 dB | 1.00 dB | matches |
| glory-trombone1-chord | 58 | 0.00 dB | 0.24 dB | matches (4-note chord) |
| eminent-trombone | 55 | −0.12 dB | 0.10 dB | matches |
| eeb-rick-bridge | 62 | **−3.91 dB** | **23.06 dB** | finding: also sounds on only six keys |
| session-horns-keyswitch | 60 | +0.15 dB | 1.53 dB | finding |
| spitfire-horns-a4 | 57 | **−29.72 dB** | **15.07 dB** | finding: Kontakt really is at 0.0035 |
| studio-drummer-stadium | 54 | +2.24 dB | 0.98 dB | finding |
| vintage-organs-c3-jazz | 66 | −2.04 dB | 13.97 dB | finding |
| unity-nostalgic-synth | 72 | −0.16 dB | 1.99 dB | finding |
| lass-aleatoric-cellos | 60 | — | — | refused by both engines |
| garritan-flute1 | 60 | — | — | refused by KONTRA, sounds in Kontakt |
| evolve-piano-glissandos-02 | 60 | — | — | refused by KONTRA, silent in Kontakt |
| glass-steel-zero-zone | 60 | — | — | known gap: Kontakt −9.8 dBFS, KONTRA silent |
| scene-lotus | 60 | — | — | known gap: Kontakt −27.6 dBFS, KONTRA silent |

Ten findings, five clean, five negative. The findings are the corpus paying for itself on day one:
the largest is Fredonia, where a 3,927-zone organ with no round robin should be the most deterministic
case in the set and is 16 dB out instead.

**Cross-check against the hand measurement.** Rendered at key 60 (the note the old parity sweep
used) through the same pipeline, Electric Mint reads **−0.12 dB level / 0.97 dB band**, against
`STATUS.md`'s hand-measured −0.14 / 0.93 — so the corpus's ruler is the same ruler. At its own
derived key 68 it reads +1.03 / 0.93. Both are true: the difference is the note, and a 1.15 dB
key-dependence on one instrument is itself the kind of thing this corpus exists to keep an eye on.

## Adding a case

1. Resolve the `.nki` from the census (`/var/tmp/census/partA/shard-*.jsonl`) — never guess a
   path. Note the library, the refused/loaded state, and the zone counts.
2. Find the key: render each key the prepared zones cover, at velocity 100, and take the median of
   the longest run that sounds above −60 dBFS. Record it in `key` **and** in `key_note` with the
   run it came from.
3. Add the entry to `cases.json` with `expect` (`matches`, `expected-refusal`, `expected-silent`),
   an empty `threshold`, `align_ms: null`, and a **one-line `why`** saying what code path the case
   exists to catch. A case nobody can justify in one line is a case to delete. Write `instrument`
   **relative to `library_root`** (`Glory Days - Big Band Horns/Instruments/…nki`), never as an
   absolute path: a case that hardcodes one machine's library runs on that machine only.
4. `python3 tools/parity-corpus/run.py --capture --case <id>` — two oracle sessions, writes
   `kontakt/<id>-a.wav`, `-b.wav`, the measured threshold and the alignment offset.
5. Run it: `python3 tools/parity-corpus/run.py --case <id>`. If it drifts, that is a finding: say
   so in the commit message rather than widening the threshold.

Adding a case is also how the corpus grows past twenty without anyone re-measuring by hand: the
capture is the measurement, and it is two commands.

## Re-capturing one case

```bash
python3 tools/parity-corpus/run.py --capture --case electric-mint
```

Two sessions, ~80 s of Kontakt time. The oracle is a single consumer: if another session is up,
wait for it rather than racing. Each take is `oracle_up → oracle_load → oracle_render →
oracle_down`, and the capture fails loudly if `kontakt_leftovers` is non-empty or the display is
still up — a leaked Xvfb is a bug in the harness, not a footnote.

Re-capture when the *reference* is wrong (a Kontakt version change, a wrong key). Never re-capture
to make a KONTRA regression pass.

## The negative cases

Five cases are wrong on purpose, and each flips to a pass when the gap it documents closes:

- **expected-refusal** — KONTRA must refuse with the recorded reason (`lass-aleatoric-cellos`,
  `garritan-flute1`, `evolve-piano-glissandos-02`). If it starts rendering, the runner says `FIXED -
  now renders; re-capture and promote` and passes, so a fix never has to argue with the corpus.
  A refusal for a *different* reason still fails: a panic where a clean error used to be is drift.
- **known-gap** — the reference sounds and KONTRA does not (`glass-steel-zero-zone` at −9.8 dBFS,
  `scene-lotus` at −27.6 dBFS). Both pass while KONTRA is silent, name the gap and the reference
  level every run, and ask to be promoted the moment KONTRA makes the sound.
- **expected-silent** — no case uses it any more. It was written for `glass-steel-zero-zone` and
  `scene-lotus` on the census's evidence that they were silent; the oracle said otherwise, and the
  reference WAVs are what corrected it. An instrument Kontakt can play is not a silent instrument
  just because KONTRA cannot. The runner still honours the outcome for a case that genuinely needs
  it.

`lass-aleatoric-cellos` is refused by **both** engines (encrypted presets, no `.nicnt` key
material), so it has no reference WAV at all: `references: []` says so explicitly rather than
pointing at a file that does not exist.

## Size

`kontakt/` holds 38 references at 960,044 bytes each (2.5 s × 48 kHz × 2 channels × 4 bytes) —
**36.5 MB, committed on purpose**. That is the price of raw float stereo with a real tail, and it
is the corpus's whole point: the files are the measurement. If that ever needs to come down, drop
the tail to 0.25 s (17 MB) rather than quantising to 16-bit, which would change the thing being
measured. `lass-aleatoric-cellos` has no reference at all because both engines refuse it.

## Cost

Measured 2026-11-01 on this machine:

| | |
|---|---|
| capture, whole corpus | **~21 minutes** of one Kontakt consumer for 40 sessions (20 cases × 2 takes), ~31 s per session (launch 5 s, load 20 s on average, render 6 s, down 0.4 s) |
| capture, one case | ~62 s |
| refusals | the slowest: Kontakt spends its 60 s paint timeout refusing LASS |
| gate subset (3 cases) | **6.2 s**, no Kontakt, no display |
| whole corpus | ~29 s, no Kontakt, no display |

## Reading a failure

```
[FAIL] electric-mint        DRIFT    level +3.03dB band 1.12dB (moved +2.00/+0.19, limit 0.50/1.00)
       level moved +2.00 dB (baseline +1.03)
```

means the KONTRA render moved 2 dB away from where it stood when the case was recorded — further
than this instrument's own two takes are from each other, so the build changed the sound. That
example is a reference deliberately scaled by +2 dB, not a hypothetical. `--verbose` prints all 25
bands, which usually names the region: a shelf at 12 kHz is an amp/FX law, a dip at 500 Hz is a
filter, a broadband shift is a gain stage. `--json` writes the same rows for a report.

A `finding (unchanged)` row never fails the build. It is the list of places where KONTRA does not
yet match Kontakt, printed every run so it cannot be mistaken for parity.
