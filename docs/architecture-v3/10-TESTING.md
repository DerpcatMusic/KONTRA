# 10 Testing: contracts, vectors, less test code

Today there are ~121k test lines, many of them tied to internals (45
`#[cfg(test)]` hooks inside `v2.rs` alone). Still, the probe in
`src/sound/v2.rs` (`audit_probe_dense_realtime_playback`) was the first
test to show that Areia dense chords are 0/64 audible. v3 tests behaviour
through public APIs, with three kinds of test.

## 1. Playback contracts (`tools/probe`, crate `kontra-probe`)

They run on the engine through `kontra-engine`'s public API at real-time
pacing and also offline, with the same results expected from both.

| Id | Contract | Measured as |
|---|---|---|
| C1 | **On time:** every voice's first output frame is at its scheduled sample | `start_error_frames` = 0 for all voices |
| C2 | **No dropouts** at the reference disk speed (and a degraded-disk run that only allows dropouts, never delays) | `dropout_frames` = 0 / C1 still holds |
| C3 | **Layers aligned:** all voices of one callback instant start in one batch | max start spread within a note = 0 |
| C4 | **Block-size independence:** 32/64/128/512/1024 and odd sizes render identical audio | null test < −120 dB |
| C5 | **Offline = real time:** same input in both modes renders identical audio | null test |
| C6 | **No double attack:** an ignored original attack produces no voice; one note on yields the expected voice count | voice log per note |
| C7 | **Script load:** 300 UI writes per note (Areia) don't delay or drop any note | C1 + C6 under that load |
| C8 | **Dense chords:** 64 steps × 4-note chords, all audible on the first block | 64/64 |
| C9 | **Start offsets** (S.Mod, play_note offset, CC/RTC → playPos) start from preload | C1 with offsets across the range |
| C10 | **CPU:** render time per block below budget at the reference polyphony | p99 < 50% of block time on `release` |

Fixtures:

- `native`-format instruments (06-FORMATS.md) built in-repo, so CI needs
  no vendor libraries. Each mimics a real pattern: Areia-style CC113 start
  plus a heavy UI script, Vista-style KeyPosition start, and mic layers.
- Real-library runs (Areia, Vista, Conflux, ...) are nightly and local,
  with the same contracts and `KONTRA_PROBE_NKI`-style env selection.

The ignored test in `src/sound/v2.rs` is the seed. Phase 1 moves it into
`kontra-probe` and turns its prints into assertions.

## 2. DSP vectors

- Each family folder has `vectors/*.json`: input reference, params, law,
  expected output and tolerance. The 5.9 MB of existing JSON/TSV vectors
  move to the family that proves them. Vectors that prove nothing (no
  native or closed-form source) get deleted.
- `module_tests!` (08-MACROS.md) adds the generic properties per module.

## 3. Unit tests

These cover only pure logic that is hard to reach through the API:
parsers, sema, law math and queue ordering. Each sits next to the code if
it's under ~150 lines, otherwise in `tests/`.

## 4. What gets deleted

- Tests that assert on private fields, counters of impossible states, or
  `cfg(test)` hooks. Each one is replaced by a contract or deleted with the
  code it tested.
- `src/ui/v2_tests.rs` (8k): keep the screenshot and interaction tests in
  `tools/shots`. Drop the rest.
- Duplicate per-format tests of shared DSP: the vectors live with the DSP
  family once.

Target: ~70k test lines. All contracts are green on fixtures in CI, and
on the four reference libraries nightly.
