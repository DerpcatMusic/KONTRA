# kontra-probe

Plays an instrument through the playback contracts C1–C10
(`docs/architecture-v3/10-TESTING.md` §1) on the real product path
(`V2Core`, host blocks, effects drained every block) and reports them.

```sh
kontra-probe <instrument> [--pattern live|dense|offsets|cc] [--contracts C1,C4,...] [--json out.json]
```

- `--pattern` picks the report written to `--json` (default `live`). Contracts
  each run their own pattern.
- `--contracts` defaults to all ten. Unknown ids are an error.
- Exit 0: every selected contract passed or is unsupported. Exit 1: a selected
  contract failed. Exit 2: usage or load error.

## Contracts

| Id | Status | Measured as |
|---|---|---|
| C1 | measured | live, real time: first audible frame minus scheduled frame, 0 for every note |
| C2 | unsupported | needs a degraded-disk fixture |
| C3 | unsupported | needs per-voice first output; only the mixed output is public |
| C4 | measured | live, offline: null test of 32/64/512/1024/97 against 128, < −120 dB |
| C5 | measured | live: offline vs real-time null test, < −120 dB |
| C6 | measured | live, offline: voices per note, equal on every note and > 0 |
| C7 | unsupported | needs a script-load fixture (300 UI writes per note) |
| C8 | measured | dense, real time: every chord audible in its first block |
| C9 | unsupported | playPos is not observable; the offsets pattern only sends CC113 |
| C10 | unsupported | render time per block is not instrumented |

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
        "scheduled": 204800,
        "first_audible": 204800,
        "voices": 1,
        "dropouts": 0
      }
    ]
  },
  "verdicts": [
    { "id": "C1", "pass": true, "detail": "worst start error 0 frames over 64 notes; 0 never audible" },
    { "id": "C3", "pass": null, "detail": "unsupported: per-voice first output is not public; ..." }
  ]
}
```

- `pattern`: `live`, `dense`, `offsets`, `cc`. `mode`: `realtime`, `offline`.
- `scheduled`, `first_audible`: absolute frames from the start of the run, which
  includes 400 blocks of settle. `first_audible` is `null` when the step never
  sounded. `keys` lists one chord per step.
- `pass`: `true`, `false`, or `null` when the contract is unsupported.
- The mixed audio is not written to JSON.
