# KONTRA roadmap

Reviewed 2026-10-23 against `main` (16f110c11). Each item was checked against code, tests and the [Kontakt 8 parity matrix](docs/re/engine/PARITY_MATRIX.md), not against earlier plans. PR numbers are in parentheses.

- **Done**: shipped in nightly builds, with tests or measurements behind it.
- **Partial**: a working subset. The limits are stated.
- **Not started**: no working implementation in the tree.
- **Parity**: a matrix row with native reference vectors (Kontakt 8 dumps, Wine oracle or the kemu oracle). Rows without them are partial, even when the DSP is written.
- **Approximate**: a documented approximation, such as an UnknownLaw or a load-report warning.

## Architecture

One engine with a universal, configurable set of sampler, synthesis, modulation and effect processing. Each sampler is an extension that reads its formats and maps laws and controls onto it. See [docs/ENGINE.md](docs/ENGINE.md).

| Sampler | Stage |
| --- | --- |
| Kontakt | In development |
| UVI / Falcon | In development |
| Opus / EastWest, HALion, HISE, other samplers | Planned |

## Shipped

Nightly alpha builds for Windows x64, macOS universal and Linux x64, as CLAP and VST3 plugins and a standalone build (#1, #2, #20, #36). Nightly notes come from accepted fix entries (#6, #45).

### Formats and loading

- **Kontakt, done**: NKI instruments, NKM multis, NKS/NIS wrappers, NKX/NKR archive resources, NCW and WAV/AIFF samples (mono and stereo). Load parity is measured against Kontakt 8.13.1: 0 mismatches over about 5.3M group, zone and loop values in 11 libraries (#60).
- **Kontakt, partial**: NKSN snapshots need their base NKI. Protected library access uses keys from local library metadata (partial). NKB bank program switching and multichannel sample files are not supported.
- **UVI / Falcon, partial**: UFS banks and programs load, including protected programs. Several UVI modulation connections are approximated rather than bound (#364, #374, #388, #391, #408, #413, #420, #423, #435, #438, #473).
- **Load report**: each load lists what loaded, what is missing with a reason, and runtime problems. Known limits are in the section below.

### Playback

- **Done**: key and velocity mapping, transposition, forward loops, alternating and reverse loops, AHDSR envelopes (#476, #479), resident and streamed PCM with budgeted heads (#294, #358).
- **Partial**: round robin, release triggers, stealing, legato and choke (matrix: partial). Flex and DBD envelopes. Loop crossfades.
- **Measured**: a 64-frame block at 48 kHz has a 1,333 µs deadline. See Performance below.

### Effects, filters and modulators

Counts are rows in the [parity matrix](docs/re/engine/PARITY_MATRIX.md) (155 rows).

| Family | Parity | Partial | Not started |
| --- | ---: | ---: | ---: |
| Filters (state-variable, ladder, legacy, Daft, Formant, PRO-53, Monark, Versatile, Dual SKF) | 43 | 16 | 0 |
| Stomps, lo-fi and tape | 5 | 10 | 1 |
| Amplifiers | 2 | 2 | 7 |
| Modulation effects | 2 | 5 | 3 |
| Dynamics, EQ, spatial, mangling and utilities | 0 | 17 | 5 |
| Delays, reverbs and convolution (2 rows unknown) | 0 | 7 | 0 |
| Sources (sampler, wavetable, Time Machine, Beat, S1200, MP60) | 0 | 10 | 1 |
| Modulation sources (envelopes, LFO, step, external) | 0 | 10 | 0 |
| Script and library loading | 0 | 4 | 0 |

- **Parity, shipped**: state-variable, ladder (including HQ controls, #279, #285), legacy and PRO-53 filters, Daft, Formant I/II (#120, #157, #172, #305, #307, #308), Big Fuzz, Cat, Cry Wah, Dirt, DStortion, ACBox, Cabinet, legacy Flanger and Phaser.
- **Partial, shipped with limits**: Monark, Versatile, Dual SKF, Surround Panner (#401, #415, #418), Stereo Tune (#387), Gater (#375), Replika Delay (#296, #302, #323, #326, #328), Kolor, Bite, Skreamer.
- **Modulators**: none is at parity yet. LFO, Multi Digital, step, AHDSR, DBD, Flex, follower and glide are partial (#122, #154, #174, #183, #203).
- **Not started**: 17 Kontakt rows, listed under Planned.

### Scripting

- **KSP, done**: scripts run natively in the plugin. Callbacks cover init, note, release, controller and UI. Covered: generated notes, waits, `find_group` lookup (#261), the MT19937 start draw (#274), polyphonic variable scope (#384), effect and filter type constants (#356, #430, #432), `set_control_par` rules (#400), keyswitch articulations migrated to zone selectors (#436).
- **KSP, partial**: the Kontakt 8.12 surface has 1,605 identifiers. Most are not yet implemented, and 42 have partial overrides ([KSP parity map](docs/architecture-v2/KSP_PARITY.md)). Persistence, async and some UI services are partial. KSP diagnostics name the failing operation and input (#81, #379, #399).
- **UVI Lua, partial**: UVI scripts run on the shared Luau runtime (#452) under a deterministic work budget. The API is a Lua 5.1-compatible approximation. Parameter writes for LFO, MultiLFO and DAHDSR are not bound (see Known limitations).

### Performance (measured)

Recorded baseline, commit `fbc205f7` (2026-10-07), AMD Ryzen 7 7800X3D, 16 cores, 48 kHz. Median and p99 block time against the 1,333 µs deadline at 64 frames:

| Scenario | Block | p50 | p99 | Deadline misses |
| --- | ---: | ---: | ---: | ---: |
| Dense strings, 1 thread | 64 frames | 36.7 µs | 448 µs | 20 / 6,000 blocks |
| Scripted legato | 64 frames | 39.5 µs | 389 µs | 1 / 6,000 |
| Streaming sweep | 64 frames | 4.6 µs | 15.1 µs | 5 / 8,250 |
| Convolution pads | 64 frames | 117 µs | 151 µs | 12 / 6,000 |

100 resident voices at 256 frames, one thread, share of the deadline ([docs/architecture-v2/CPU_EVIDENCE.md](docs/architecture-v2/CPU_EVIDENCE.md)): plain 0.31%, fractional 5.37%, filtered 9.10%, modulated 13.44%, three EQs plus stereo 17.18%.

This baseline is older than much of the performance work (#236 to #446) and has not been re-measured. Re-baselining is pending. The target of 300 voices at 1% of the meter is not achieved. The Kontakt and Falcon comparison on the same instruments is not measured.

### MPE and MIDI

- **Done**: MPE per-note pitch, pressure and timbre routing. Measured on 41 Kontakt instruments (up to five per library): pitch responds on 34, pressure on 40, timbre on 35 ([MPE_COVERAGE.md](docs/architecture-v2/MPE_COVERAGE.md)). Per-note bend sensitivity and pitch reset are covered by tests (#281, #333, #334, #337).
- **Not started**: MIDI-CI and SysEx. Full MPE on UVI instruments is not measured.

### Editor and platform

- **Done**: native editor with the MUI host and browser layout (#32, #33), Linux X11 gesture and click fixes (#79, #83), part-level layout fixes (#243, #254), macOS signed and notarized installer (#18–#25).

## In progress

- **Universal Luau script layer** (#452, #453, #460, #477, #478, #480, #482, #483, #485, #487). The Luau runtime exists, and a KSP-to-Luau front end covers UI controls, PGS keys and UI record builtins. No corpus script is fully in the subset yet: 0 of 696 extracted scripts. The interpreter stays the default because Luau is slower with worse tails on the measured per-event workload ([UNIVERSAL_SCRIPT.md](docs/architecture-v2/UNIVERSAL_SCRIPT.md)).
- **Format coverage**: UVI modulation mapping. This is the largest gap family and covers LFO, MultiLFO, DAHDSR and constant modulation.
- **Wow / Flutter** (#317, #322, #325, #463). Parameters are read and stored, and the Kontakt control laws are applied. The DSP kernel is not ported; the prepare step is blocked (#467, #469). Not shipped as processing.
- **Kolor shaper** (#367, #389, #398, #402, #414, #417, #419, #422, #462, #466, #470, #471). The dry path and several shaper cases are ported and checked against native vectors. Other cases are open. Matrix status: partial.
- **Time Machine and machine family** (#100, #102, #202, #223, #238, #253, #258, #286, #297). Beat slicing and machine controls are wired. Time Machine is partial. Time Machine 2 and Pro wait on Kontakt 8 READY (#102).
- **Replika Delay** (#296, #302, #316, #323, #326, #328, #348, #396, #425, #427, #428). Mode-1 chains and head kernels are being ported. Output and feedback are partial.
- **Parity matrix**: 81 partial rows and 17 missing rows. Owners update their rows in the same PR as the code.

## Planned

- **Full KSP parity** with the Kontakt 8.12 manual. It is a required product gate, and no licensed Kontakt differential run has been completed.
- **Kontakt DSP not started** (17 rows): Tone Machine, Supercharger GT, Bass Invader, Bass Pro, EP Preamps, HotSolo, Reverb Delight, Super Fast 100, Van51, Chainsaw, Flair, Phasis, Vibrato/Chorus, Beat Masher, Beat Slicer, Reverse Grain, Transpose Stretch.
- **Kontakt features not supported**: multiple active loops per zone, NKB bank program switching, multichannel sample files.
- **Output routing**: each instrument on its own DAW pair, with a nested per-instrument mixer. Rack buses exist; the nested mixer is not evidenced yet.
- **UI**: a vectorized UI mode (library artwork kept, controls drawn natively) and UI IR rendering for all KSP and Falcon UIs. The `sampler-ui-ir` crate exists; the full UI set does not.
- **Other samplers**: Opus / EastWest, HALion, HISE and others.
- **Release gate for v2**: v2 ships under the KONTRA name only after UI, DSP, UVI and scripting are complete and beat v1 on every metric. Nightly alpha builds continue until then.

## Known limitations

Dates matter. The corpus numbers are from the full-tier survey of 2026-10-23 (base `3ab06104d`, machine heavily loaded). The [library gap report](docs/re/engine/LIBRARY_GAPS.md) has the full table.

- **Corpus load (2026-10-23)**: 1,312 of 1,395 instruments and programs load, 1,297 sound, 1,295 have no failure. Kontakt: 682 of 682 single instruments and 53 of 53 multis load; one script compile failure (Shreddage E-Upright).
- **Silent at a covered key (2026-10-23)**: 15 instruments (Dolce 10, Pacific Ensemble Strings 5). Probe key 60 may be the cause.
- **UVI (2026-10-23)**: 577 of 660 programs load. The 83 refusals were 79 SignalConnection and 4 Frequency. #435 and #438 replaced those refusals with bindings or approximations after the survey base. A re-survey is needed for the new count.
- **UVI approximations (2026-10-23)**: "Constant cadence (approximated)" affects 577 programs. Lua `setParameter` gaps include `ConstantModulation.Value` (550), `SignalConnection.Ratio` (432), `LFO.Freq` (243) and `MultiLFO.Freq` (208).
- **Script warnings (2026-10-23)**: 685 instruments raise warnings. 52 hit unsupported script features (`set_engine_par` among them). Pending script events remain unclassified on 36 rows.
- **Level outlier (2026-10-23)**: Pacific Ensemble Strings "Cluster Risers" moved -23.7 dB against the previous run. It is not yet investigated.
- **Timing (2026-10-23, busy machine)**: 54 deadline-miss rows. Median load time 1,135 ms, p99 5,970 ms, max 8,875 ms. These are not valid timings.
- **Memory (2026-10-23)**: largest peak heap 716 MiB.
- **Audio thread**: the survey recorded 0 audio-thread allocations. The baseline at `fbc205f7` recorded 98 on the UVI scripted pad scenario. Re-measure before claiming zero.
- **Matrix coverage**: 52 parity rows, 81 partial, 17 missing, 3 unknown (saved-state round trips, PsycheDelay, Twin Delay), 1 not applicable (Pro EQ, not in Kontakt 8) and 1 owned by another lane.
- **KSP**: most of the 1,605 manual identifiers are not implemented. The default for each is missing.
- **Performance**: 300 voices at 1% of the meter not achieved. The Kontakt and Falcon comparison is not measured.

Feature inventory pages in [docs/FEATURES.md](docs/FEATURES.md) and [docs/COMPATIBILITY.md](docs/COMPATIBILITY.md) predate this review. The [website feature table](https://derpcatmusic.github.io/KONTRA/features.html) is generated from committed probes on each main commit.
