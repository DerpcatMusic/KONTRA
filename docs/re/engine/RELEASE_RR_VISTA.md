# Vista 3 Violins FFF Overlay: release tail silence (2026-10-24)

Question: does the gate-release group's `RoundRobin(1)` start test silence the release tails of keys 60/64/67?

## Result: round robin is not the cause

- The instrument has 62 round-robin tests in total, all `RoundRobin(1)` (dump: `format-census --slots` with
  `CENSUS_GROUPS=1`). Attack groups carry no start test. Release groups (`reldyn1`, `legreldyn1`) carry `RoundRobin(1)`.
- KONTRA cycle length: `native_rr_length` = max `RoundRobin` position = 1, so `tick % 1 + 1 == 1` for every note.
- Kontakt's binary law (docs/re/KONTAKT_START_CONDITIONS.md): stored 0 is the first match, cycle length is max stored
  + 1 = 1, one counter per instrument, advanced on ordinary note-ons only. Also every note matches a length-1 cycle.
- So the RR test passes for every release note in both implementations. The KONTRA RR code already matches the
  binary law: `keyswitch.rs` maps stored 0 to `RoundRobin(1)`, the counter is per plan (`lib.rs`, one increment per
  admitted ordinary note), and release selections commit without advancing it. No engine change.

## Remaining suspect: script-owned release triggers

- The instrument has one script (`scripts 1`). When a script binds `NO_SYS_SCRIPT_RLS_TRIG`, `owns_release_triggers`
  is set (`crates/sampler-ksp/src/lib.rs:1776`), `prepared.script_release_triggers` is true, and
  `run_release` sets `musical = false` (`crates/sampler-core/src/prepare/selection.rs:1234`), so native release
  groups are suppressed. That matches KONTRA's exact digital silence after each note-off.
- Not verified: whether this script binds that condition, and whether the script's `on release` plays the tails.
  The script text is not decoded by the tooling yet. This belongs with the script-side tail finding (wave74,
  "scripts on = silent within 80 ms of note-off"), so the fix is not made here.

## Oracle captures

The first capture (keys 60/64/67/60, holds 0.3 to 4 s) was removed with the scratch WAVs. Kontakt's tails were weak
(-51 to -72 dB in the first 0.5 s after note-off) and may include the sustain group's own release. Redo with the
script-ownership check before reading them as release-group output.
