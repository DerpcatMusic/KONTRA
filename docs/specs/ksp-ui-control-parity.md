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
| Kontakt KSP `get_control_par` / `set_control_par` | Kontakt 8.13 (Wine oracle) | https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/KSP_Reference_Manual.pdf | 2026-11-25 | measured reads/writes in `tools/kontakt-oracle/probes/ksp/control_pars_{1..10}.measured` (`ksp_probe.py`, `ANALOG STRINGS.nki`); par names and documented ranges from the manual |

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

- Text pars read the control's declared name where Kontakt does (`NAMED`:
  IDENTIFIER on every type; TEXT on knob, button, switch, label and value
  edit; AUTOMATION_NAME on a knob), until the script sets them.
- `move_control` with a cell below 1 keeps the position and sets
  `$HIDE_WHOLE_CONTROL`; cells from 1 move the control and clear that bit,
  leaving hide parts.

### Non-Goals
- Mouse/drag/wheel behaviour and drawing (next batches).

## API / Data / File Contract

- `ParLaw { par, kind, unset, write: ParWrite, read: ParRead }`;
  `ParWrite::{Keep, Byte, Flag, Within, AtLeast}`;
  `ParRead::{Stored, Unset, Opaque, Cell, Inherit}`.
- `crates/app/host/tests/oracle_probes.rs` replays every probe that has a
  `.measured` file and compares each value. Lines marked `unstable` are not
  compared.
- `tools/ksp-manual-coverage.py` counts a measured `.kspp` as test text.
- `ksp_probe.py`: `T <label> <expr>` records a string (saved with
  `save_array_str`, printed verbatim); `ORACLE_PROBE_NKR` lends the
  instrument a resource container; stderr names the oracle slot.

## Tasks

1. Probes 1–5 measured in Kontakt.
2. `ParLaw` table plus the store's write/read paths.
3. Replay test over every measured probe.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/ksp-ui-control-parity.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Oracle replay | Validator/Runtime | linux-x86_64 | `matari fast kontra -p kontra-host -- -E 'test(every_measured_probe_reads_as_kontakt_measured)'` | 282 values match, 9 unstable skipped | PASS (2026-11-25, linux-x86_64) |
| Host tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-host` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Probes compile | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-ksp -- -E 'test(every_oracle_probe_compiles)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | CI `verify` | Green | PENDING |

## Unverified Boundaries & Promotion Gate

- Knob WIDTH/HEIGHT read 87/18 with or without a picture (control_pars_9,
  `kp_knob` lent through `ORACLE_PROBE_NKR`). The probe cannot see whether
  the lent picture resolved; a shot of it would.
- An untouched wavetable's WAVE_END_COLOR/ALPHA read uninitialised memory
  (different across runs); those 9 lines are `unstable`, and KONTRA reads
  the inherited value.
- FILEPATH: Kontakt 8.13 keeps a path to an existing file whatever the
  BASEPATH (outside it, without its slash, in another case) and drops a
  missing one (`filepath_base.kspp`, not replayed). KONTRA keeps the
  manual's rule instead (kept under a non-empty BASEPATH, by prefix): its
  engine cannot see files. Matching needs the host to lend an existence
  check; none crosses into the engine today, and `set_control_par_str` may
  run on the audio thread, so it is left for a design of its own. With no basepath and an existing file nothing is measured yet.
- A button's and a knob's TEXT read their last line after a TEXTLINE, as a
  label's does (control_pars_11, replayed).
- Drags (tools/ksp-ui behaviour page, 100 %): knobs, plain sliders and
  horizontal `MOUSE_BEHAVIOUR` sliders match KONTRA within a unit
  (`a_drag_moves_each_control_as_far_as_kontakt_did`). A value edit moves 4
  units a pixel on 0..1000; KONTRA now sweeps it in 250 points, which also
  fits the earlier 150 % fact on 0..100, but other ranges are not measured:
  the rig loses those drags. A stock slider with a negative
  `MOUSE_BEHAVIOUR` sweeps in 2000/|m| × 15 px; KONTRA uses its 18-point
  height (open: one size measured). The wheel cannot be driven in the rig.
- Overlaps and hiding (tools/ksp-ui layers page): the click goes where
  KONTRA's draw order and visibility put it in every measured case, after
  a control that sets no Z_LAYER took its panel's (it kept its own). A
  child's own Z_LAYER inside a panel is not measured. Callback order
  (`ui_controls`, then `ui_control`; `persistence_changed` after init)
  matched already.
- `$CONTROL_PAR_NONE`, `_DND_BEHAVIOUR` and `_MIDI_EXPORT_AREA_IDX` measured
  0, 40 and 105; KONTRA keeps CONTROL_PAR constants opaque.
- `$CONTROL_PAR_WAVETABLE` in the coverage report is a wrapped table row of
  the manual (`_END_COLOR`), not a parameter.
