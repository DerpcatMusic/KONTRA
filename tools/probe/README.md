# kontra-probe

Plays an instrument through the playback contracts C1–C10
(`docs/architecture-v3/10-TESTING.md` §1) on the real product path
(`V2Core`, host blocks, effects drained every block) and reports them.

```sh
kontra-probe <instrument> [--pattern live|dense|offsets|cc] [--contracts C1,C4,...] [--json out.json]
kontra-probe --fixture cc_start|key_position|dense|streamed [...]
kontra-probe --help
```

- `--pattern` is the pattern C1 and C6 measure, and the one the summary line
  describes. C4 and C5 always measure `live`, C8 always measures `dense`; each
  verdict records the pattern it measured.
- `--contracts` defaults to all ten. Unknown ids are an error.
- Exit 0: every selected contract passed or is unsupported. Exit 1: a selected
  contract failed. Exit 2: usage or load error. `--help` prints usage, exits 0.

Each verdict prints as `C1 FAIL [live] ...`: id, outcome, the pattern it was
measured on, then the detail.

`--fixture` plays a synthetic instrument instead of a file. It is generated
from WAVs under the probe binary's folder (`target/<profile>/kontra-probe-fixtures`)
by the library creator, so no vendor files or audio are committed.

| Fixture | Stands for | Not carried by the creator |
|---|---|---|
| `cc_start` | Areia: 3 mic groups x 4 round robins, round-robin script | CC113 start route (the offset is `play_note`'s start time instead) |
| `ui_load` | Areia's effect traffic: 300 `set_control_par` writes and one `message` per note | the 16,700-line `on note` |
| `key_position` | Vista: 2 velocity layers | KeyPosition/RTC start route (the offset is computed in KSP) |
| `dense` | 4-note chords on 1 mic x 4 round robins | none of the audited load; the chords pass |
| `streamed` | 2 s samples, past the head | cold disk reads (the files are in the page cache) |

The probe prints what is missing when a fixture is played. Failing contracts
are `#[ignore]`d in `tests/fixtures.rs` with the task that fixes them. Run
`cargo test -p kontra-probe -- --ignored` to see the audited bugs reproduce.

## Contracts

| Id | Status | Pattern | Measured as |
|---|---|---|---|
| C1 | measured | selected | real time: first audible frame minus scheduled frame, 0 for every note |
| C2 | unsupported | – | needs a degraded-disk fixture |
| C3 | unsupported | – | needs per-voice first output; only the mixed output is public |
| C4 | measured | live | offline: null test of each block size against 128, < −120 dB |
| C5 | measured | live | real time vs offline, null test < −120 dB |
| C6 | measured | selected | offline: voices per key of every note, equal on every note and > 0 (a chord counts per key) |
| C7 | measured | selected | C1 and C6 under the fixture declared UI load; fails when fewer script effects drain than the declared writes (writes lost); unsupported when no load is declared (real instruments declare none) |
| C8 | measured | dense | real time: every chord audible in its first block |
| C9 | measured | offsets | real time: CC113 0..126 on every note, first audible frame minus scheduled frame |
| C10 | unsupported | – | render time per block is not instrumented |

Null tests (C4, C5) refuse to pass without evidence. The reference render must
have a peak of at least 1e-4 (about −80 dBFS) and at least 32 of its 64 notes
audible. Otherwise the verdict is FAIL with `null not measured: ...`. The
detail of a passing or failing null states the reference's peak, RMS and
audible count, so a null against silence cannot pass.

C4 sweeps host block sizes 32, 64, 97, 128, 512 and 1024. The engine renders in
chunks of at most `MAX_BLOCK` (128) frames, so only 32, 64, 97 and 128 change
how the engine itself is driven. 512 and 1024 only test host event splitting
and block bookkeeping. The detail lists which sizes ran natively.

C6 on a failure lists every note with no voices: its step, keys, velocity sent,
and whether a zone of the loaded instrument plays those keys at that velocity.
"A zone plays every key" with no voices points at the engine; "no zone for
keys" points at the probe's note choice. The summary line gives the zone key
span and how many steps have a key no zone plays. Zone coverage ignores
conditions and articulation switches, so it is a necessary check, not a full
one. Plain audio files have no zone map.

Known limits of the first version:

- Onset is measured on the mixed output with a 1e-3 level threshold, not per
  voice. A release tail from an earlier note inside the step can count as an onset.
- Voices per note are peak active voices minus the count at step start, so
  tails of earlier notes can lower the count.
- `dropouts` counts underrun and dropped-command events, not frames.

## JSON report

`--json` writes one object:

```json
{
  "instrument": "path/to/file.nki",
  "rate": 48000.0,
  "report": {
    "pattern": "live",
    "mode": "realtime",
    "block": 128,
    "notes": [
      {
        "step": 0,
        "keys": [48],
        "velocity": 100,
        "scheduled": 204800,
        "first_audible": 204800,
        "voices": 1,
        "dropouts": 0
      }
    ]
  },
  "verdicts": [
    { "id": "C1", "pass": true, "detail": "worst start error 0 frames over 64 notes; 0 never audible", "pattern": "live" },
    { "id": "C4", "pass": false, "detail": "null not measured: reference silent (peak -inf dBFS, rms -inf dBFS, 0/64 notes audible)", "pattern": "live" },
    { "id": "C3", "pass": null, "detail": "unsupported: per-voice first output is not public; ...", "pattern": "live" }
  ]
}
```

- `report` is the run of `--pattern`. Verdicts carry their own `pattern`, which
  can differ from the report's (C4 and C5 are live, C8 is dense).
- `pattern`: `live`, `dense`, `offsets`, `cc`. `mode`: `realtime`, `offline`.
- `velocity`: MIDI velocity sent with the step's keys.
- `scheduled`, `first_audible`: absolute frames from the start of the run, which
  includes 400 blocks of settle. `first_audible` is `null` when the step never
  sounded. `keys` lists one chord per step.
- `pass`: `true`, `false`, or `null` when the contract is unsupported.
- The mixed audio is not written to JSON.
