# Corpus health

`tools/corpus-health` loads and plays every installed instrument (Kontakt, Kontakt multis, UVI) and writes one JSON record per instrument.

Build once and run the binary, never `cargo run` inside a heavy slot:
`~/.cache/kontakto-heavy cargo build --profile corpus -p corpus-health` (the `corpus` profile keeps symbols so failures name `crate::module::fn`). `tools/corpus-health/run.sh <args>` does both.

## Tiers
| command | what | target |
|---|---|---|
| `corpus-health quick` | the ~90 ids in `quick.txt`: load + short render, then a diff against the previous quick run | < 3 min |
| `corpus-health run OUT.jsonl --tier parse` | parse and lower only, no audio, all instruments | < 3 min |
| `corpus-health run OUT.jsonl --tier full` | everything, incl. diagnose reload, controller pass, MPE probe on 1 in 8 | < 20 min |

Quick runs rotate `~/.cache/kontakto-corpus/quick/current.jsonl` to `previous.jsonl`. `quick --workers N --timeout S --baseline FILE`.
`corpus-health quick-list RUN.jsonl...` regenerates the quick list (3 per library, 2 per failure mode, regressions, 4 multis).
`corpus-health diff OLD NEW` prints regressions, fixes, new failures, stage changes and the perf delta.

## Workers
Pool sized by RAM (MemAvailable minus 12 GiB) and at most 12, scaled down by other tenants' load average. Each job runs under `catch_unwind` with a timeout (300 s parse, 600 s otherwise); a hung job is recorded as a timeout and replaced.

## Record
Failures carry `stage`, `where` (crate::module::fn) and `error`. Perf: `load_ms`, `phases_ms`, `peak_heap`, `audio_thread_allocs` (render only, expected 0), `script_inline_allocs`/`script_inline_ms` (UVI Lua runs inline in the harness). Silent notes carry `sound.why_silent` (`sampler_core::why_silent`); `unsupported_ranked` is `sampler_ir::rank_features`. Multis play up to 16 programs; empty programs (no zones) count as empty slots.

## Env
`CH_RESCAN` rescan instead of `~/.cache/kontakto-corpus/items.tsv` (24 h); `CH_QUICK_FILE` alternate list; `CH_MPE` MPE probe on all; `CH_NOMPE` none; `CH_STEAL`, `CH_NOSTREAM` runtime variants.
