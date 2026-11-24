---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Real-library parity census

Plan: `docs/architecture-v3/13-IR-VNEXT.md`, lane L4. Follows
`wave3-parity.md`.

## Problem & Goal

- **Problem:** Parity with real libraries was judged from samples (18 of 51
  ANALOG STRINGS snapshots silent after #160), not measured over the
  libraries installed on the build host.
- **Goal:** One re-runnable census plays a held C3 on every installed
  Kontakt `.nki`/`.nksn` and UVI program, records loads / silence (RMS) /
  diagnostics / script faults, and ranks failure classes by instrument
  count. Fixes land root cause first, each with a real-library regression
  test, and the census is re-run before and after.

## Primary-Source & Version Proof

N/A — internal refactor: behaviour is taken from the libraries' own scripts and saved data and from the KSP manual text already in the repo; no external API or SDK assumption.

## Acceptance criteria

1. **Census.** `tools/parity/census.sh OUT` builds `kontra-inspect`, plays
   every instrument under `/data/main/Libraries/{Kontakt,UVI}`
   (`KONTRA_CENSUS_ROOTS` overrides) and writes `OUT/census-<date>.tsv`
   (instrument, loads, silent, rms, peak, classes) and a ranked summary.
   Resumable; `--summary` re-ranks without playing. `kontra-inspect play`
   reports `metrics.rms`. The held C3 starts after a 2 s pre-roll, as a
   player's first note comes after the instrument loaded: a script that
   mutes while it loads (UVI AUGMENTED ORCHESTRA's layers ignore notes until
   their `loadSample` pass, behind a `wait(50)`, ends) is not counted as
   silent.
2. **Snapshot recall.** A Kontakt snapshot loads as Kontakt loads it: its
   base, then the snapshot recalled onto it. A script with
   `set_snapshot_type` 1 or 3 (state kept) sees the values land again and
   its `on persistence_changed` run a second time (`ir::Instrument::recalled`,
   `Engine::new`). ANALOG STRINGS snapshots saved with the other bus pair
   live (Quarterstra: buses 1–2 at −120 dB, `$instrument_bus_offset_` 2)
   reach `switch_bus` and sound. Regression test
   `plugin/src/tests.rs::an_analog_strings_snapshot_on_its_other_buses_sounds`
   (skips without the library).
3. **Zones moved by `on init`.** A zone `set_zone_par` moves while the
   instrument loads (key or velocity range, group) plays where it moved
   from the first note: the engine's key index is rebuilt after init and
   after a recall (`Engine::zones_moved`). CONFLUX snapshots (Antarctica)
   sound. Regression tests `crates/app/host/tests/script.rs::a_zone_init_moves_plays_there_from_the_first_note`
   (no library) and `plugin/src/tests.rs::a_conflux_snapshot_plays_the_zone_its_init_moved`.

## Non-goals

- `on init` of a kept-state script still reads the snapshot's values,
  where Kontakt reads the base's own; no census instrument shows a
  difference.
- Load-time script work runs a fixed share per audio block, so a large
  host buffer delays when a heavy script settles (ANALOG STRINGS: ~3 s at
  512 frames). Unchanged here.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py --changed origin/main` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64, 1567 passed, 23 skipped) |
| Real-library regression | Validator/Runtime | linux-x86_64 | `build-slot cargo test --release -p kontra-plugin --lib snapshot` | AS and CONFLUX snapshot tests pass with the libraries mounted | PASS (2026-11-24, linux-x86_64) |
| Census before/after | Validator/Runtime | linux-x86_64 | `plugins/kontra/tools/parity/census.sh <dir>` (before: same scenario, main's engine) | Silent count drops; no new load errors | PASS (2026-11-24, linux-x86_64, 2481 instruments all load; silent 459 → 91, none newly silent; PluginArtifacts/kontra-parity/{before-main,after}/census-2026-11-24.tsv) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green | PENDING |

## Unverified Boundaries & Promotion Gate

- No Kontakt oracle render of a `.nksn` (the oracle loads `.nki` only): the
  second persistence-changed callback is inferred from the script
  (`$_snapshot_was_loaded := 1-$init_flag`) and from factory snapshots
  that otherwise cannot sound.
