---
status: implemented
project: plugins/kontra
owner: derpcat
created: 2026-11-23
updated: 2026-11-23
---

# KONTRA Kontakt UI & ni-file True Parity

## Problem & Goal

- **Problem:**
  1. **`ni-file` binary parity gaps (L4 & Fundamentals B P2-4):**
     - `BParArp` (chunk `0x09`, Step Modulator, v0x80/v0x81) has no hand reader (`KontaktObject::BParArp` is an empty unit variant and `InternalMod` returns `Modulator::Other { chunk_id: 0x09 }`).
     - `BParLFO` (chunk `0x08`) rejects legacy v0x70 in `Lfo::read_supported` (`parser_coverage.md` shows native `0x70-0x73`, hand reader `0x71-0x73`).
     - `BParGroupDynamics` (chunk `0x4A`, Fundamentals B P2-4) has no `Group::dynamics()` accessor or non-zero payload check (`has_nonzero()`), and does not enforce exact stream consumption.
     - `ModTarget.invert` (`+0x16`) is misnamed: Kontakt 8 decompilation (`0140920000.c:7098-7111`, Fundamentals Audit B Verified Solid #7) proved it is the UI `"tableOpen"` flag, not audio polarity inversion (`unknown_flags & 2`).
  2. **Kontakt UI / KSP UI visual & rendering parity gaps (`kontra-ui-view` / `kontra-ui`):**
     - `Widget::state_styles` (`FONT_TYPE`, `_ON`, `_OFF_PRESSED`, `_ON_PRESSED`, `_OFF_HOVER`, `_ON_HOVER`) are populated by `kontra-face` but completely ignored by `kontra-ui-view` (every button/switch always renders its base `w.style` regardless of state, hover, or press).
     - `Widget::text_shift_on_press` (`$CONTROL_PAR_DISABLE_TEXT_SHIFTING`) is populated by `kontra-face` but completely ignored by `kontra-ui-view` (pressed buttons/switches never shift captions by 1 px unless disabled).
     - Kontakt `ui_table` rendering in `kontra-ui-view` has five parity bugs: (a) ignores `steps_shown` (`set_table_steps_shown`) while `kontra-ui::input` uses it, causing visual/pointer mismatch; (b) ignores `bipolar` (draws negative-range tables from the bottom instead of the center zero line); (c) ignores `colors.bar`, `colors.background`, and `colors.zero_line`; (d) draws NO bars at all when a Kontakt table has a custom `PICTURE` or `HIDE_PART_BG` (because `stock::face` is gated behind `else if stock` after `picture` and `!w.hide.background`); and (e) omits `zero_line` and `steps_shown` from `ui.memo` deps.
     - `Widget::value_y` (`$CONTROL_PAR_VALUEPOS_Y`) and `Widget::text_y` on stock `ValueEdit`/`Knob`, `Widget::menu_arrow` on stock `Menu`, and `style.font` / `value_y` / `text_shift` in `ui.memo` deps are missing.
  3. **Workspace clippy gate blocker:** `crates/format/decentsampler/src/effects.rs:141` fails `cargo clippy -- -D warnings` with `clippy::indexing_slicing`.
- **Goal:**
  - Close all listed `ni-file` parser/accessor gaps with Kontakt 8.13.1 decompiled evidence (`FUN_140cff920`, `FUN_140d03660`, `FUN_140d035f0`, `0140920000.c`) and byte-exact roundtrip tests.
  - Close all listed Kontakt UI / KSP UI rendering parity gaps in `kontra-ui-view` with headless pixel-verified tests.
  - Fix the `decentsampler` clippy error so `matari check origin/main --project plugins/kontra` is green.
- **Plan Reference:** `plugins/kontra/docs/architecture-v3/13-IR-VNEXT.md` Lane L4 (`vendor/ni-file`), Fundamentals B P2-4 (`BParGroupDynamics` 0x4A), and `docs/architecture-v3/07-APP.md` §5 (Kontakt UI parity).

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** Zero changes to audio-thread hot paths; `ni-file` runs at load time only, and `kontra-ui-view` runs on the UI thread only.
- **Determinism & Compatibility:**
  - Every `ni-file` reader/writer preserves exact IEEE-754 float bits and unknown/reserved bytes on roundtrip, and rejects truncated, overlong, or out-of-range enum/bool bytes.
  - `ModTarget` serde deserialization keeps `#[serde(alias = "invert")]` on `table_open` for backward compatibility.
  - Zero breaking changes to `ni_file::kontakt::objects::Modulator` enum variants while peer agent `agy4` holds active leases on `crates/format/kontakt`.
- **Mirror Boundaries:** All changes live cleanly inside `plugins/kontra` with zero private monorepo paths or secrets.
- **UI Rendering Invariants:**
  - Idle UI frames rebuild 0 widgets (`ui.memo` deps stay exact and minimal — no spurious invalidations when nothing moved).
  - Existing Kontakt 8 oracle pixel tests in `kontra-ui-view` continue to pass unchanged.

### Non-Goals
- **Fundamentals B P0-9 (`fx_idx_amp_split_point`) & `crates/format/kontakt` / `crates/engine/*` / `crates/model/ir` translator/engine edits:** Actively leased and implemented by `agy4` on branch `kontra/amp-split-point` (PR #120). This spec strictly stays on disjoint paths (only updating the 1-line `ModTarget { table_open: false, .. }` test literal in `crates/format/kontakt/src/timed.rs:440` for the `ni-file` rename, and avoiding `vendor/ni-file/src/kontakt/objects/snapshot_group.rs`).
- **Translating `StepMod` or `BParGroupDynamics` inside `crates/format/kontakt/src/translate.rs`:** Deferred to a tiny follow-up once `agy4` releases `crates/format/kontakt`; this PR provides the verified `ni-file` APIs (`StepMod`, `InternalMod::step_mod()`, `Group::dynamics()`, `GroupDynamics::has_nonzero()`).

## API / Data / File Contract

### `ni-file` (`plugins/kontra/vendor/ni-file`)
- `pub struct StepMod` (`BParArp`, chunk `0x09`, versions `0x80` and `0x81`, verified against `FUN_140cff920` + `0140920000.c:295-771`):
  - Fields: `version: u16`, `snap_to_12: bool` (`+0xd1`), `num_steps: u32` (`+0xc`), `single_trigger: bool` (`+0xd0`), `steps: Vec<f32>` (up to 32 from `FUN_140cf0840`), `frequency: f32` (`+0x90`), `legacy_note_value: Option<f32>` (v0x80 `+0xb0`), `frequency_sync: Option<PackedRecord>` (v0x81 `FUN_140ab5da0` `+0x98`).
  - Methods: `TryFrom<&Chunk>`, `to_chunk(&self) -> Result<Chunk, Error>`, `write(&self, writer: impl Write) -> Result<(), Error>`.
  - `KontaktObject::BParArp(StepMod)` in `chunk.rs`.
  - `InternalMod::step_mod(&self) -> Result<Option<StepMod>, Error>` in `internal_mod.rs` (non-breaking companion to `InternalMod::params()`).
- `Lfo` (`BParLFO`, chunk `0x08`, `lfo.rs`):
  - `Lfo::read_supported` and `Lfo::to_chunk` support `v0x70` alongside `0x71..=0x73`, verified against `FUN_140d03660` (`public_len = 29 + usize::from(waveform == 5) * 20`, `records[0].values[0] = note_value_frequency`, `records[1] = LfoRecord { flag: true, values: [note_value_delay, 0.0, 0.0] }`, `trailing_flag = true`).
- `GroupDynamics` (`0x4A`, `native_modulators.rs`) & `Group` (`group.rs`):
  - `GroupDynamics::try_from` rejects trailing bytes after the 64 floats.
  - `GroupDynamics::has_nonzero(&self) -> bool` returns `true` if any of the 64 floats is non-zero or NaN.
  - `Group::dynamics(&self) -> Result<Option<GroupDynamics>, Error>` finds child `0x4A` and decodes it.
- `ModTarget` (`modulation.rs`):
  - Field `invert: bool` renamed to `table_open: bool` (`#[cfg_attr(feature = "serde", serde(alias = "invert"))]`).

### `kontra-ui-view` (`plugins/kontra/crates/app/ui-view`)
- `plan::Widget`:
  - `style_ref: Option<model::StyleRef>` + resolved `style: Option<&'a model::TextStyle>` selected from `w.state_styles` using Kontakt's 6-slot table (`[off, on, off_pressed, on_pressed, off_hover, on_hover]`) with fallback to `[on]` -> `[0]` -> `w.style`.
  - `text_shift: bool` = `held && w.text_shift_on_press && matches!(w.kind, Kind::Button { .. } | Kind::Switch)`.
- `ui-view/src/lib.rs`:
  - Tracks `pressed`/`hovered` for any visible widget with `strip || art || has_state_styles || w.text_shift_on_press`.
  - Applies `(1. * scale, 1. * scale)` text offset when `item.text_shift` is true.
  - Renders `Kind::Table` bars as an overlay layer (after `picture` if present, or after stock background when `!w.hide.background`), supporting `steps_shown`, `bipolar`, `colors.bar`, `colors.background`, and `colors.zero_line`.
  - Updates `ui.memo` `deps` to include `item.style_ref`, `item.text_shift`, `wd.value_y`, `wd.menu_arrow`, `wd.colors.zero_line`, and table `steps_shown`/`bipolar`.
- `ui-view/src/stock.rs`:
  - `Look::Button` supports `text_shift` offset.
  - `Look::Field` respects `wd.text_y` and `wd.value_y` when set.
  - `Look::Knob` respects `wd.text_y` and `wd.value_y` when set.
  - `Look::Menu` respects `wd.menu_arrow`.
  - `Kind::Slider` respects `wd.colors.bar` when set.

### `kontra-format-decentsampler` (`plugins/kontra/crates/format/decentsampler`)
- Replace `MUSICAL_BEATS[idx]` with `MUSICAL_BEATS.get(idx).copied().unwrap_or(1.0)` at `src/effects.rs:141`, and return `Result` from the non-`#[test]` `effects(...)` helper at `tests/decentsampler.rs:491` so `clippy::unwrap_used` is clean across `--all-targets`.

## Verification Matrix

Every row below must pass before marking the spec `implemented`:

| Check | Exact Command | Pass Criterion |
|---|---|---|
| `ni-file` tests | `cd plugins/kontra && build-slot cargo nextest run --manifest-path vendor/ni-file/Cargo.toml` | Exit 0, all roundtrip & negative tests pass |
| `kontra-ui-view` & `kontra-ui` tests | `cd plugins/kontra && build-slot cargo nextest run -p kontra-ui-view -p kontra-ui -p kontra-ui-pictures` | Exit 0, all new + existing pixel/input tests pass |
| `decentsampler` & `kontakt` tests | `cd plugins/kontra && build-slot cargo nextest run -p kontra-format-decentsampler -p kontra-format-kontakt` | Exit 0, zero regressions |
| Workspace clippy `-D warnings` | `cd plugins/kontra && build-slot cargo clippy --workspace --all-targets -- -D warnings` | Exit 0, zero warnings/errors |
| Formatting check (touched crates) | `cd plugins/kontra && cargo fmt -p kontra-ui-view -p kontra-format-decentsampler -p kontra-format-kontakt -- --check && cargo fmt --manifest-path vendor/ni-file/Cargo.toml -- --check` | Exit 0 |
| Full PR verification gate | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green |

## Task Decomposition

Split implementation into path-disjoint units suitable for `agent-lease`:

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | `ni-file`: `StepMod` (0x09), `Lfo` v0x70, `GroupDynamics` (0x4A) helpers, `ModTarget.table_open` | `plugins/kontra/vendor/ni-file` (excl. `snapshot_group.rs`), `plugins/kontra/crates/format/kontakt/src/timed.rs:440` (test literal) | — | `cd plugins/kontra && build-slot cargo nextest run --manifest-path vendor/ni-file/Cargo.toml` |
| 2 | `kontra-ui-view`: `state_styles`, `text_shift_on_press`, Kontakt `ui_table` parity, `value_y`/`menu_arrow`/memo deps | `plugins/kontra/crates/app/ui-view` | — | `build-slot cargo nextest run -p kontra-ui-view -p kontra-ui` |
| 3 | `decentsampler`: fix `clippy::indexing_slicing` at `effects.rs:141` | `plugins/kontra/crates/format/decentsampler` | — | `build-slot cargo clippy --workspace --all-targets -- -D warnings` |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` only (`matari affected origin/main` will report `plugins/kontra`).
- **Mirror Impact:** Subtree mirror (`DerpcatMusic/KONTRA`) — all changes stay inside `plugins/kontra` with zero private paths.
- **Versioning & Changelog:** No version bump needed for this internal parity PR.
