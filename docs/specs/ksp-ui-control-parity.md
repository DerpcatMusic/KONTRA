---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: external-api
evidence_freshness_days: 180
---

# KSP control parameters read and write as Kontakt measures

## Problem & Goal

- **Problem:** `get_control_par` / `set_control_par` in KONTRA stored any
  value and read it back. Kontakt does not: it clamps, masks, ignores
  out-of-range writes, ties grid cells to pixels and reads per-type defaults.
  Scripts that read a par back (layout code, colour code) then took other
  branches than in Kontakt.
- **Goal:** every `$CONTROL_PAR_*` read and write that oracle probes measure
  behaves as Kontakt 8.13 does, declared once as a `ParLaw` row in
  `crates/app/host/src/script.rs` (`LAWS`) and applied by
  `crates/engine/script-ui/src/controls.rs`.
- **Plan Reference:** [14-UNIFIED-UI-IR.md](../architecture-v3/14-UNIFIED-UI-IR.md),
  script-UI store; [13-IR-VNEXT.md](../architecture-v3/13-IR-VNEXT.md) KSP lane.

## Primary-Source & Version Proof

| Standard / Vendor API | Pinned Version / Spec Rev | Primary-Source URL | Verified Date | Key Invariants / Deprecations Checked |
|---|---|---|---|---|
| Kontakt KSP `get_control_par` / `set_control_par` | Kontakt 8.13 (Wine oracle) | https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/KSP_Reference_Manual.pdf | 2026-11-25 | measured reads/writes in `tools/kontakt-oracle/probes/ksp/control_pars_{1..5}.measured` (`ksp_probe.py`, `ANALOG STRINGS.nki`); par names and documented ranges from the manual |

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| Kontakt (oracle host) | 8.13 | `tools/kontakt-oracle` | `get_control_par`, `set_control_par`, `move_control`, `set_knob_unit` |

## Invariants & Non-Goals

### Hard Invariants
- A law is data (one `LAWS` row); `controls.rs` has no par-specific branch
  except grid cell ↔ pixel and grid size ↔ pixel size.
- Laws apply only to controls; a variable id (kind 0) stores as before.
- The face still reads GRID_X/Y as 1-based cells and pixel sizes it was given.

### Non-Goals
- String pars (LABEL, HELP, IDENTIFIER, …): KSP cannot measure string
  length, so they need the ksp-ui trace (next batch).
- Mouse/drag/wheel behaviour and drawing.

## API / Data / File Contract

- `ParLaw { par, kind, unset, write: ParWrite, read: ParRead }`;
  `ParWrite::{Keep, Byte, Flag, Within, AtLeast}`;
  `ParRead::{Stored, Unset, Opaque, Cell, Inherit}`.
- `crates/app/host/tests/oracle_probes.rs` replays every probe that has a
  `.measured` file and compares each value. Lines marked `unstable` are not
  compared.
- `tools/ksp-manual-coverage.py` counts a measured `.kspp` as test text.

## Tasks

1. Probes 1–5 measured in Kontakt.
2. `ParLaw` table plus the store's write/read paths.
3. Replay test over every measured probe.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/ksp-ui-control-parity.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Oracle replay | Validator/Runtime | linux-x86_64 | `matari fast kontra -p kontra-host -- -E 'test(every_measured_probe_reads_as_kontakt_measured)'` | 181 values match, 9 unstable skipped | PASS (2026-11-25, linux-x86_64) |
| Host tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-host` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Probes compile | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-ksp -- -E 'test(every_oracle_probe_compiles)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | CI `verify` | Green | PENDING |

## Unverified Boundaries & Promotion Gate

- Knob WIDTH/HEIGHT read 87/18 whatever was written; measured only on a knob
  without a picture. A knob with a picture may read its written size.
- An untouched wavetable's WAVE_END_COLOR/ALPHA read uninitialised memory
  (different across runs); those 9 lines are `unstable`, and KONTRA reads
  the inherited value.
- `move_control` with one coordinate 0 and the other non-zero, and grid
  sizes 0 or negative, are not measured.
- String pars are not measured.
