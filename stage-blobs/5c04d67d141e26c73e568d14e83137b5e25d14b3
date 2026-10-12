# Polyphony-sized stream page pool — REJECTED

The rejected 8192-only lead is restored. This trial changes only the size prepared by sampler-kontakt Streamed::new on the loader/control thread, keeping d75ae827's request horizon, lazy head policy, readers, runtime voice limits and growth behavior. The configured stream-policy floor is retained; an instrument's authored polyphony may require more than that floor. Each streaming voice has the existing three-page allowance. Multiplication is checked and allocation errors retain the existing load error path. A source rebuilt after a polyphony change prepares its replacement pool before runtime/audio ownership.

The numeric-only stream_pool_capacity example reads source metadata without writing sample, script or UI payloads. Both Areia instruments declare 1000 voices, versus 576 peak voices observed on the Full Ensemble audit chord. This gives 3000 pages × 4096 frames × 8 bytes = 93.75 MiB, a 69.75 MiB increase over the 256-voice/24 MiB floor. Piano declares 240 and ANALOG STRINGS 128, so both retain that floor. W8 was notified of this exact increase before sizing edits; non-stream NoteParams/voice/DSP reductions remain separate.

The factory fixture creates real streamed plans with absent, below-floor and above-floor authored limits, including 1000 voices. It checks the policy floor, report/cache byte counts, and unchanged lazy head residency across source rebuilds. The existing host fixture still verifies allocation-free early two-page requests, completion admission and page crossings. Both passed, as did the lazy-head budget fixture and root cargo test --no-run. The candidate and exact d75 production baseline use matched corpus/debug-line-table settings and Cargo.lock. Candidate SHA256: dbd5263ea5d85cb7d92d15df53505575fa3d621668c0b52d03aa3db2e23d198f. Baseline SHA256: 3fb5cea91933813574731dab75e3ce749a9512c7f21db6fcd7230f9b84e064d6.

The trial ran the same 64 original-instrument runs, including cold/warm Areia 16 Violins and Full Ensemble at 32/64/256. Measurements include CPU median/p99, deadlines, storage underruns, stream-capacity errors, audio heap calls, cache residency receipts and RSS. The prior Full Ensemble baseline loaded RSS was about 473 MiB; v1's coordinator-supplied reference is about 309 MiB. This trial must not gain reliability by accepting a memory loss.


The fixed pool is rejected; no source SHA was handed to W0. All64 event/render heap counters were zero and all32 cold receipts showed zero cached pages after eviction. Activity validation labels63 runs QUIET and the Full Ensemble256 warm candidate CONTENDED (unowned scanner PID1699040 at18:58:20 UTC). The raw result is retained as UNKNOWN. Sixteen attribution runs added14 QUIET and two CONTENDED baseline intervals (external buffr cargo builds). A final two-run warm pair is NOT USED at the coordinator's direction. Timing differences are reported, not separately accepted against an unmeasured per-cell A/A spread; repeated clear storage faults and the RSS loss suffice to reject this trial.

| Instrument | Block | Cache | Before median / p99 µs | After median / p99 µs | Underruns before→after | Deadlines before→after | Capacity errors before→after | Loaded RSS MiB before→after |
|---|---:|---|---:|---:|---:|---:|---:|---:|
| areia16 | 32 | warm | 219.184 / 319.596 | 213.994 / 297.526 | 0→0 | 9→9 | 0→0 | 512.30→579.82 |
| areia16 | 32 | cold | 221.224 / 296.465 | 213.264 / 273.015 | 9→0 | 9→9 | 0→0 | 512.10→579.68 |
| areia16 | 64 | warm | 281.395 / 410.118 | 288.825 / 421.018 | 0→0 | 4→3 | 0→0 | 511.93→579.74 |
| areia16 | 64 | cold | 287.015 / 409.828 | 286.116 / 406.467 | 0→0 | 3→4 | 0→0 | 512.47→579.72 |
| areia16 | 256 | warm | 1217.803 / 1363.466 | 1202.212 / 1349.075 | 0→0 | 1→1 | 0→0 | 512.07→579.70 |
| areia16 | 256 | cold | 1192.652 / 1323.955 | 1202.983 / 1393.497 | 0→0 | 1→1 | 0→0 | 512.11→579.80 |
| areiafull | 32 | warm | 457.189 / 512.089 | 471.139 / 603.902 | 24→0 | 76→110 | 1202→0 | 472.96→537.02 |
| areiafull | 32 | cold | 453.408 / 582.491 | 473.858 / 717.393 | 24→88 | 90→139 | 1228→0 | 473.14→537.18 |
| areiafull | 64 | warm | 611.562 / 824.496 | 625.432 / 983.248 | 24→0 | 13→11 | 507→0 | 473.15→536.77 |
| areiafull | 64 | cold | 613.841 / 768.594 | 626.741 / 861.456 | 24→0 | 11→10 | 513→0 | 472.73→537.23 |
| areiafull | 256 | warm | 2306.104 / 2591.019 | 2467.447 / 3363.324 (CONTENDED) | 24→0 | 1→1 | 257→0 | 472.85→536.97 |
| areiafull | 256 | cold | 2289.003 / 2568.078 | 2357.524 / 2709.801 | 24→0 | 1→1 | 262→0 | 472.89→536.88 |
| fx | 32 | warm | 41.731 / 88.941 | 43.011 / 86.682 | 0→0 | 1→1 | 0→0 | 986.55→985.59 |
| fx | 32 | cold | 42.661 / 91.942 | 45.860 / 91.472 | 0→0 | 1→1 | 0→0 | 986.41→985.20 |
| fx | 64 | warm | 48.571 / 88.151 | 46.011 / 87.741 | 0→0 | 0→0 | 0→0 | 985.71→985.60 |
| fx | 64 | cold | 48.791 / 95.612 | 48.381 / 89.002 | 5→0 | 0→0 | 0→0 | 985.75→985.45 |
| fx | 256 | warm | 181.643 / 252.205 | 185.704 / 272.185 | 0→0 | 0→0 | 0→0 | 985.76→984.16 |
| fx | 256 | cold | 184.663 / 252.324 | 166.213 / 239.805 | 0→5 | 0→0 | 0→0 | 985.43→983.91 |
| piano | 32 | warm | 21.361 / 39.151 | 18.100 / 37.871 | 0→0 | 0→0 | 0→0 | 218.80→218.62 |
| piano | 32 | cold | 21.340 / 37.151 | 18.300 / 35.960 | 0→0 | 1→0 | 0→0 | 218.76→218.59 |
| piano | 64 | warm | 26.661 / 47.551 | 25.710 / 48.541 | 0→0 | 0→0 | 0→0 | 218.79→218.65 |
| piano | 64 | cold | 29.050 / 47.641 | 25.921 / 44.341 | 0→0 | 0→0 | 0→0 | 218.82→218.58 |
| piano | 256 | warm | 114.592 / 147.123 | 114.032 / 144.363 | 0→0 | 1→1 | 0→0 | 218.97→218.71 |
| piano | 256 | cold | 114.892 / 157.203 | 114.202 / 151.773 | 0→0 | 1→1 | 0→0 | 218.95→218.68 |

For repeated64-frame piano/FX cells the CPU columns are medians of three per-run statistics; counters are totals. Other original cells have one pair. Initial Full Ensemble32 cold rose24→88 underruns. Clear attribution repeat4 was25→0; clear repeat5 was20→178, confirming greater capacity alone does not make cold storage reliable. Repeat5 median/p99 was481.639/856.886→484.759/653.772 µs, deadlines522→132, candidate capacity errors0. Repeat6 candidate had0 underruns, but its baseline was CONTENDED and cannot support an A/B claim. FX256 cold original0→5 did not repeat: three clear extra pairs baseline0/1/0 versus candidate0/0/0. The unchanged FX pool has no demonstrated candidate-specific disk fault; release storage tolerance remains zero.

Full Ensemble capacity errors reached zero on every candidate, but loaded RSS rose about473→537 MiB and final RSS481→545 MiB. Both exceed the coordinator's v1 Full Ensemble reference about309 MiB. W8's pending d75-based non-stream measurements remain separate; earlier savings from another UI baseline are not credited here.

Production sampler-kontakt stream.rs before cfg(test) is restored byte-identical to d75ae827. The authored-polyphony fixture is retained with an explicit ignore reason because its rejected production sizing was removed; the source and passing trial test remain reviewable in56f1822b. Existing lazy-head and allocation-free host fixtures remain active. Frozen before/after binaries, BUILD receipts, raw numeric results, cache receipts and timestamped activity timelines remain under ~/.cache/kontakto-fix-cpu (large data symlinked to target volume).

The coordinator superseded the physical-growth fallback: next work copies v1 0cb7a8a0's resident head preload,8192-frame per-voice ring and64-reader implementation. Acceptance requires quiet v1/d75 A/B on Full Ensemble and FX32/64/256 cold/warm, calibrated by A/A,0 storage underruns,0 capacity errors and RSS no worse than d75. No further pool repeats or requests; W0 owns the next quiet window.
