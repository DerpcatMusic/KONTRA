# Performance baseline

`crates/sampler-perf` plays fixed note schedules through real instruments and
times every render block against its deadline. Run it on an idle machine
(other agents' builds distort it) and verify each DSP change against the baseline.

```
cargo build --release -p sampler-perf
target/release/sampler-perf run --out new.json [--only NAME,..] [--seconds S]
target/release/sampler-perf compare docs/architecture-v2/perf-baseline.json new.json [--threshold 10]
```

`compare` exits 1 when a cell regresses: p50, p99, resident memory, or the
`cycles`, `instructions`, `cache-misses`, `branch-misses` counters rise by more than
the threshold (default 10%), or deadline misses, audio-thread allocations or stream
underruns rise at all. Library roots come from `KONTRA_KONTAKT_LIBRARIES` and
`KONTRA_UVI_LIBRARIES` (defaults `/mnt/MAIN_STORAGE/Libraries/{Kontakt,UVI}`).

## What it measures

Per scenario, block size (64, 128, 512 frames at 48 kHz) and render threads
(1, 2, 4, N; UVI plays on one): block time p50, p99 and max against the deadline,
deadline misses, `perf stat` user counters (cycles, instructions, IPC, cache and LLC
misses, branch misses, page faults, context switches), resident memory, bytes read
from disk (`read_bytes`, and `rchar` for cache hits), stream cache and underruns,
heap calls on the audio thread (a counting allocator, around each block), and idle
wakeups per second (voluntary context switches of all threads over 3 s with the
instrument loaded and nothing playing). Each cell loads a fresh runtime and renders
in real time (streamed pages arrive from reader threads), after one second of
silence to start threads. Results are JSON: `perf-baseline.json` is the baseline.

| Scenario | Instrument | Schedule |
|---|---|---|
| dense-strings | Vista 5 Violins (Kontakt, streamed, scripted) | 30-note chord every 0.5 s, held 1 s, 8 s |
| scripted-legato | Pacific 10 Cellos Legato Sustains (legato script) | legato line over two held notes |
| streaming-sweep | Areia 16 Violins Core Techniques (300 MB of heads) | a new key every 0.2 s over five octaves |
| convolution-pads | ANALOG STRINGS (an impulse response) | as dense-strings |
| uvi-scripted-pad | Augmented Orchestra PAD Angela (UVI script host) | four-note pad, 3 s |

## Baseline (commit see JSON, Ryzen 7 7800X3D, 16 threads, one run)

Idle wakeups: 0.33 per second for every loaded scenario (one parked-thread timer).
Audio-thread heap calls: 0 for every Kontakt scenario; 96-98 per 8 s for the UVI
script host (`sampler_uvi::scripted::Player`, not the core).

| Scenario, 1 thread | block | p50 us | p99 us | max us | misses |
|---|---|---|---|---|---|
| dense-strings | 64 | 36.7 | 448 | 15301 | 20 |
| dense-strings | 128 | 61.4 | 839 | 15669 | 14 |
| dense-strings | 512 | 186.8 | 15459 | 18920 | 12 |
| scripted-legato | 64 | 39.5 | 389 | 1355 | 1 |
| scripted-legato | 128 | 65.6 | 649 | 1316 | 0 |
| scripted-legato | 512 | 231.1 | 1224 | 2473 | 0 |
| streaming-sweep | 64 | 4.6 | 15 | 5794 | 5 |
| streaming-sweep | 128 | 5.4 | 157 | 232 | 0 |
| streaming-sweep | 512 | 8.8 | 539 | 6729 | 0 |
| convolution-pads | 64 | 117.1 | 151 | 35004 | 12 |
| convolution-pads | 128 | 221.4 | 310 | 35957 | 6 |
| convolution-pads | 512 | 907.3 | 3129 | 34362 | 5 |
| uvi-scripted-pad | 64 | 10.0 | 23 | 48 | 0 |
| uvi-scripted-pad | 128 | 18.2 | 36 | 59 | 0 |
| uvi-scripted-pad | 512 | 80.8 | 145 | 164 | 0 |

IPC is 2.2-2.7 on the Kontakt scenarios and 3.0-3.5 on UVI. Loads: Vista violins
0.9 s and +65 MB, Areia 79 s and +430 MB (300 MB of heads resident), ANALOG STRINGS
5.7 s and +269 MB, PAD Angela 10.9 s and +631 MB (decoded up front).

## Findings

- **Convolution first use.** Every convolution cell has one block of about 35 ms
  (max column; 12 of 64-frame blocks miss, from that one block and the blocks it
  delays). The cost is the first note, not steady state (p99 is 150-360 us at 64/128).
  Look for IR partition setup that runs on the audio thread at first use.
- **Chord onsets.** dense-strings has a 12-18 ms block at about every chord onset
  (thirty simultaneous note-ons, each running its script and starting several voices):
  p99 at 512 frames is 15 ms. Steady-state blocks are 37-190 us.
- **No thread scaling yet.** Per-block cost at 2, 4 and 16 threads equals one thread
  on these schedules: voices render in 20-200 us, below the cost of a worker handoff.
  Scaling needs a heavier voice count than a 30-note chord.
- **Stream underruns** appear at 2+ threads in dense-strings (148-502 per cell, one
  per voice that lost pages): the cell runs at the same speed, so this is the page
  pool under 30 simultaneous cold starts, not rendering speed. Underruns are
  counted by `compare` as a regression only when they rise.
- **The UVI script host allocates** on the audio thread (about 12 heap calls per
  second of audio).
- Numbers come from one run; run to run noise at p50 is a few percent, at p99 and max
  much more. Raise `--threshold` for p99 and max, or compare three runs.

Known harness limits: UVI programs are driven through their script host between
blocks (not sample-accurate events), and the scenario list is fixed in
`crates/sampler-perf/src/main.rs`.
