# Library gaps (full-tier corpus survey, 2026-10-23)

Source: `tools/corpus-health/run.sh run --tier full --workers 3 --timeout 600 /data/main/Libraries`, run niced (`nice -n 10`), origin/main `3ab06104d` (survey worktree `survey-20261023`).
Raw: `scratch/survey/full-20261023-fresh.jsonl` (24 MB, kept). Summary: `full-20261023-fresh-summary.md`. Diff vs previous: `full-20261023-fresh-diff.txt`. KSP symbol mode: `full-20261023-ksp.jsonl`, `full-20261023-ksp.md`, `full-20261023-ksp-summary.md`.
Previous run: `full-20261022-fresh.jsonl` (origin/main 3485e567f). Earlier per-branch notes (2026-10-22/23) are in git history of this file; this run supersedes its tables.
Caveat: the machine was busy (load average 8 to 13). Load and deadline-miss timings are not valid evidence (see Deadline misses).

## Scoreboard

| | now (10-23) | previous (10-22) |
|---|---|---|
| items | 1395 (Kontakt 682, Kontakt multi 53, UVI 660) | 1395 |
| instruments that load | **1312** | 703 |
| instruments that sound | **1297** | 688 |
| instruments with no failure | **1295** | 646 |
| Kontakt single instruments loading | 682 / 682 | 681 / 682 |
| Kontakt multis loading | **53 / 53** | 22 / 53 |
| UVI programs loading | **577 / 660** (87%) | 0 / 660 |
| regressions (ok to failing) | **0** | (the 32 Conflux translate-stage regression is fixed) |
| fixed | 610 | |

Load by library (loads / items):
- UVI Augmented Orchestra 541 / 620. Remaining 79 are translate-stage refusals (see Load errors).
- UVI VWinds Clarinets 14 / 16, Double Reeds 13 / 15, Flutes 9 / 9.
- Kontakt, all clean at load: Afflatus Brass 348 / 348, Areia 155 / 155, Dolce 77 / 77, Conflux 51 / 51, Pacific Ensemble Strings 49 / 49, CHORUS 42 / 42, Performance Samples Vista 7 / 7, Una Corda 3 / 3, ANALOG STRINGS 1 / 1, Morphology Evolved 1 / 1.
- Shreddage E-Upright 1 / 1 loads but reports a script compile failure ("get_ui_id requires a UI control"), counted under Kontakt errors below.

Kontakt errors: 1 (script compile failure, Shreddage E-Upright). No Kontakt load failures.
Conflux: all 51 instruments and multis load (was 17 of 51 in the 10-22 run; the 32 "resident PCM over payload budget" translate-stage refusals are gone). Dark Context program 1 -11.94 dBFS (unchanged). Arps -13.14 to -15.39 dBFS (2.25 dB). Blessed Emotions -11.47 (unchanged). None of these moved by more than 20 dB.

## Load errors (UVI, 83 programs)

| programs | stage | cause |
|---|---|---|
| 79 | translate | `shared output connection has no supported exact law` (SignalConnection, per Program/Layer/Keygroup). Message truncated in JSONL. |
| 4 | translate | `shared Frequency connection has no unique real Freq control` (Clarinet/Contrabass family) |

The UVI load failures are not Kontakt failures: 83 of the 660 UVI programs do not load, all with an explicit refusal string (79 SignalConnection, 4 Frequency). The 577 loading UVI programs carry warnings (approximations), listed in Unsupported features below.

## Warnings versus gaps

Native script warnings (classified as warnings, not engine gaps):
- `find_group(): Group "..." was not found. Index will be invalid!` (229 + 229, Conflux and related). The script is the cause; the engine returns the same invalid index Kontakt does.
- `array index N out of bounds for !name / %name (length N); read returns zero or empty text` (77 + 54). Same class: script reads past its declared array.
- `$name is not in the performance view description; retained as an unbound handle` (20).
- `$name clamped to its declared range` (8).

Engine gaps (real behaviour not implemented): the rows marked gap in the table below.

## Stuck notes and pending script events

- `stuck_notes`: 0 on every row. `stuck_voices`: 0 on every row.
- `pending_script_events` > 0 on 36 rows (horizon hits after the 30 s settle). Not yet classified; see the Kontakt wave notes for the earlier metric fix. Treat as open until each is re-run with `corpus-health run --only`.

## Peak and tail anomalies (> 20 dB change versus 10-22)

Only one instrument changed by more than 20 dB:
- `Pacific - Ens Strings - 8 Basses - FX - Cluster Risers.nki`: -13.9 dBFS to -37.7 dBFS (-23.7 dB). Not investigated. Top priority for the next re-run: it is either a regression or a quieter program.

Conflux Dark Context, Arps and Blessed Emotions were checked (see above): no 20 dB change.

## Deadline misses and performance

- Deadline misses: 54 rows (10-22: 106). Largest: `Augmented Orchestra / 04 Hybrid / Asak Dancer.uvip` 125 misses (new in this run; `0 -> 125`), and `Reflex MW.uvip` 18.
- Load time median 526 to 1135 ms. Moves with machine load (see caveat), not a valid comparison.
- Peak heap median 159 to 402 MiB; Conflux programs rose from about 91 to about 172 to 188 MiB (they now load, so their samples are resident). Expected, not a leak. Max peak heap 715 to 716 MiB.
- Audio-thread allocations: 0.

## Unsupported features (top 40 by instruments)

Owner lanes (suggested): k-convolution, k-ksp-engine-params, k-mod-external, owning FX, UVI mapping (sampler-uvi, modulation), UVI sample loader. Counts overlap; do not sum them.

| # | instruments | libraries | kind | feature | owner lane |
|---:|---:|---:|---|---|---|
| 1 | 577 | 4 | approximation (warning) | Constant cadence (approximated) | UVI mapping |
| 2 | 550 | 4 | gap | lua setParameter ConstantModulation.Value | UVI mapping |
| 3 | 528 | 1 | gap (reason label is generic) | `module` (no specific detail in the record) | UVI mapping (loader must name it) |
| 4 | 432 | 1 | gap | lua setParameter SignalConnection.Ratio | UVI mapping |
| 5 | 416 | 5 | gap | Convolution: IR preserve length | k-convolution |
| 6 | 416 | 5 | gap | Convolution: IR latency compensation bypass | k-convolution |
| 7 | 349 | 2 | gap | script: polyphonic variables start at N; initializer ignored | k-ksp-engine-params |
| 8 | 271 | 3 | gap | script: polyphonic variable read outside a note callback | k-ksp-engine-params |
| 9 | 261 | 1 | gap | Program-level MultiLFO | UVI mapping |
| 10 | 243 | 1 | gap | lua setParameter LFO.Freq | UVI mapping |
| 11 | 242 | 1 | gap | Program-level LFO | UVI mapping |
| 12 | 229 | 2 | warning | script: find_group() Group "NCllChnswStbsBrNClsDynNRRN" not found | warning (not a gap) |
| 13 | 229 | 2 | warning | script: find_group() Group "NVlnsChnswStbsBrNFMpDynNRRN" not found | warning (not a gap) |
| 14 | 208 | 1 | gap | lua setParameter MultiLFO.Freq | UVI mapping |
| 15 | 187 | 1 | gap | lua setParameter MultiLFO.Smooth | UVI mapping |
| 16 | 159 | 1 | gap | lua setParameter StepEnvelope.Smooth | UVI mapping |
| 17 | 146 | 1 | gap | lua setParameter DAHDSR.DecayTime | UVI mapping |
| 18 | 142 | 1 | gap | lua setParameter DAHDSR.SustainLevel | UVI mapping |
| 19 | 120 | 1 | gap | lua setParameter LFO.SyncToHost | UVI mapping |
| 20 | 118 | 1 | gap | lua setParameter LFO.Retrigger | UVI mapping |
| 21 | 108 | 1 | gap | lua setParameter MultiLFO.Retrigger | UVI mapping |
| 22 | 106 | 1 | gap | WaveShaper mode | UVI mapping (lua) / owning FX |
| 23 | 105 | 3 | gap | keyswitch script (articulations not migrated to zone selectors) | k-ksp-engine-params |
| 24 | 103 | 1 | gap | lua setParameter MultiLFO.SyncToHost | UVI mapping |
| 25 | 97 | 1 | gap | lua setParameter WaveShaper.Amount | UVI mapping / owning FX |
| 26 | 93 | 1 | gap | lua setParameter MultiLFO.NoiseDepth | UVI mapping |
| 27 | 92 | 1 | gap | lua setParameter WaveShaper.Mix | UVI mapping / owning FX |
| 28 | 87 | 1 | gap | lua setParameter DAHDSR.ReleaseTime | UVI mapping |
| 29 | 78 | 1 | gap | lua setParameter Layer.PortamentoTime | UVI mapping |
| 30 | 77 | 1 | warning | script: array index out of bounds for !name | warning (not a gap) |
| 31 | 77 | 1 | gap | lua setParameter DAHDSR.ReleaseCurve | UVI mapping |
| 32 | 77 | 1 | gap | lua setParameter DAHDSR.DecayCurve | UVI mapping |
| 33 | 66 | 1 | gap | MultiLFO Smooth | UVI mapping |
| 34 | 62 | 1 | gap | unreadable initial sample | UVI sample loader |
| 35 | 59 | 1 | gap | lua setParameter MSN.Freq | UVI mapping |
| 36 | 58 | 1 | gap | lua setParameter WaveShaper.OutputGain | UVI mapping / owning FX |
| 37 | 57 | 1 | gap | lua setParameter StepEnvelope.NumSteps | UVI mapping |
| 38 | 57 | 1 | gap | lua setParameter DAHDSR.AttackTime | UVI mapping |
| 39 | 54 | 1 | warning | script: array index out of bounds for %name | warning (not a gap) |
| 40 | 52 | 2 | gap | script Unsupported: set_engine_par | k-ksp-engine-params |

Notes:
- `lua setParameter ...` rows are UVI program writes the lua-side mapper cannot yet bind; most are one library (Augmented Orchestra).
- Row 3 (`module`, 528) has no detail in the record. Fix the reason string before assigning work.
- Rows 1-2 and 4 are the largest, but they are one family (UVI modulation). The UVI load gap (83 programs) is separate.

## Silent instruments

15 silent at a covered key (Dolce 10, Pacific Ensemble Strings 5): note-on/selection suppressed by the script at key 60 (`note suppressed by its script`). Probe key 60 may be an artifact; check before fixing (k-voice-engine).

## KSP symbol usage (corpus-health ksp mode)

The mode ranks symbols by instruments that use them (SYMBOLS.tsv status). Caveat: the top rows are UI scaffolding (`set_control_par`, `$CONTROL_PAR_*`, `on init`), counted in nearly every instrument. They are status SIMPLIFIED (behaviour-compatible per the table), not gaps.

Non-full symbols that matter for the engine (instruments, occurrences):
- KSP-STUB: `$CONTROL_PAR_CUSTOM_ID` (1 instrument, 5 occurrences); `find_zone` (1, 1); `$CONTROL_PAR_DISABLE_TEXT_SHIFTING` (1, 2).
- KSP-UNSUPPORTED: `$ENGINE_PAR_SENDLEVEL_SMOOTHED_0` to `_7` (1 instrument each); `$MARK_10` (1, 3); `@syntax` (1, 1).
- KSP-SIMPLIFIED with the most instruments: `set_control_par` 682, `$CONTROL_PAR_HIDE` 682, `set_engine_par` 682 (870915 occurrences), `on init` 682, `on note` 682 (695 occurrences).

Read: the non-full engine symbols are rare (one instrument each). The engine-impacting work is the 40-row table above, not the symbol list.

Profiled 2026-10-22 (ANALOG STRINGS, 2 voices, 4875 blocks, quiet-ish host, niced): 5-8 misses per run, max 8.4-11.4 ms. Each spike is inside `render` (stream service < 0.02 ms, 0 minor faults, 0 allocations, 0 underruns). `corpus-health` now splits misses by involuntary context switches of the render thread into `deadline_misses` and `preempted_deadline_misses`. Preemption explains about 3 of 8; the rest have no preemption on the render thread, so the cause is still open (host CPU contention or cache/frequency effects are not separable without a perf trace). Treat `deadline_misses` as host-sensitive until that trace exists.

Fault split (2026-10-23, ANALOG STRINGS, quick tier, 3 runs, host load average 41-56 on 16 CPUs): `faulted_deadline_misses` (render-thread minor fault in the block, no preemption) is 0 in every run; the misses are 5-8 preempted and 1-2 unexplained. Render-thread page faults are ruled out as the spike cause. `stream.latency_p50_ms` is the load-time first-read probe, not note-on latency: 10.5 ms on a cold disk cache, 0.03 ms warm.

## Script diagnostics

Unsupported and Warning counts for the 10-23 run in the table above. Compile failures: 1 (Shreddage E-Upright).

## Deadline misses and quiet re-run

Re-run this when the machine is quiet (loadavg1 < 8). Timing numbers here are not evidence.

## Ranked gap lanes (owner suggestions)

| lane | gap rows | instruments (top) | priority |
|---|---|---|---|
| UVI modulation mapping (lua setParameter, LFO/MultiLFO/DAHDSR, ConstantModulation, Program-level LFO) | 1, 2, 3, 4, 9, 10, 11, 14-22, 24, 26-29, 31-33, 35, 37, 38 | 577 + 550 + 528 + 432 + ... (overlap, one family) | 1 |
| UVI load: translate refusals (SignalConnection, Frequency) | 83 failing programs | 83 | 2 |
| Convolution IR latency and preserve length | 5, 6 | 416 each | 3 |
| KSP script: polyphonic variables (initializer; read outside callback) | 7, 8 | 349, 271 | 4 |
| Keyswitch script migration | 23 | 105 | 5 |
| set_engine_par unsupported | 40 | 52 | 6 |
| WaveShaper (lua route and effect) | 22, 25, 27, 36 | 106, 97, 92, 58 | 7 |
| Silent at covered key | silent | 15 | 8 |
| UVI sample loader: unreadable initial sample | 34 | 62 | 9 |
| Pacific 8 Basses FX Cluster Risers level (-23.7 dB change) | anomaly | 1 | 10 |

## Re-run

Re-run after each merge wave: `nice -n 10 tools/corpus-health/run.sh run OUT.jsonl --tier full --workers 3 --timeout 600 /data/main/Libraries`. Compare with `tools/corpus-health/run.sh diff OLD NEW`. KSP symbol ranking: `run.sh ksp OUT.jsonl --md REPORT.md /data/main/Libraries`.
