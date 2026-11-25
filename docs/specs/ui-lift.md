---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-25
updated: 2026-11-25
proof_scope: internal
evidence_freshness_days: 180
---

# UI lift: reflow, browser UX, the tester's list

## Problem & Goal

- **Problem:** A public tester (13 screenshots, Kontakt 7's browser as the
  reference) found the editor zoomed instead of reflowing, the sidebar fixed
  and cramped, the preset list unreadable (folders and presets alike, every
  folder open, no row padding), pill chips that read as generic, an oversized
  keyboard and a slow hover ease on library tiles.
- **Goal:** The chrome behaves like a DAW's or Kontakt's browser: text at the
  player's interface scale, sections that grow with the window, navigation by
  drilling into a library. Every PR shows the minimum window, the default and
  ~1.8× the default, before and after, with numbers.
- **Plan Reference:** UI lane beside `docs/architecture-v3/13-IR-VNEXT.md`;
  tester feedback 2026-11-25.

## Primary-Source & Version Proof

N/A — internal refactor: KONTRA's own chrome on MUI and moose-mui APIs that
already exist (`MuiEditor::fixed_zoom`, `user_zoom`, `fits`).

## Ground rules (every PR)

1. **Intrinsic UI.** No hardcoded sizes unless truly necessary; a
   container's minimum comes from its content. Lists of long names ellipsize
   and show the full name on hover.
2. **Spacing tokens.** Spacing, gaps, padding, corners, stroke widths and
   type come from `kontra_ui::theme` (`HAIR`, `NOTCH`, `MARK`, `TIGHT`,
   `SPACE`, `INSET`, `TEXT`, `SMALL`, `CONTROL`); a token times a factor is
   fine, a bare pixel count at a call site is not.
   `crates/app/ui/tests/tokens.rs` fails on one. MUI changes stay generic.
3. **Measure everything.** Shots at the minimum, default and ~1.8× window,
   before and after (`crates/app/ui/examples/app_shot.rs`; `KONTRA_SHOT_FIT=1`
   draws the retired fit-zoom model), linked from each PR. Numbers, not
   impressions.

## Invariants & Non-Goals

### Hard Invariants
- The library's own panel (Kontakt/UVI performance view) keeps its native
  aspect: fitted, centred, never stretched.
- Saved views from earlier builds load (new `SavedView` fields default).
- An idle editor draws no frames (`clicks_drive_the_editor_and_idle_costs_no_frames`,
  `mui_audit_passes_across_sizes_and_views`).

### Non-Goals
- Frame pacing, redraw cost and popup performance (the menu's frame rate):
  another lane owns them.
- Engine behaviour from the same thread (notes sustaining, "Reading
  instrument 0%" loads): landed separately in #342.
- Copying Kontakt pixel for pixel; the library panel's own drawing
  (`kontra-ui-view`), which keeps the library's pixel sizes.

## Acceptance criteria

### PR 1 — reflow, not zoom (`kontra-responsive-layout`)
- [x] R1. A resized window lays the chrome out at its size: at 2× the width
  the chrome's text height is unchanged, the browser is wider and its grid
  has more columns (`a_wider_window_reflows_and_keeps_the_text_size`).
- [x] R2. Text and controls follow the interface scale (Settings → Interface
  scale, 50–200 %, 100 % = the host's display scale), applied at once
  (`a_new_scale_zooms_and_asks_the_window_to_follow`).
- [x] R3. The sidebar resizes with a splitter (sidebar width up to half the
  window, double-click resets) and the width is saved with the view
  (`look::tests::the_view_comes_back_as_it_was_left`).
- [x] R4. The keyboard spans the width with more keys as it widens; the top
  bar sheds stats before Master, MIDI or the menu.
- [x] R5. The library panel keeps its aspect, centred, whole/half steps above 1:1.
- [x] R6. Spacing tokens in the theme, the chrome migrated, the guard in
  place (`tokens::the_chrome_takes_its_spacing_from_tokens`).

### PR 2 — browser UX
- [ ] B1. Drill-down: a library opens on its top folders; a folder opens on
  its subfolders and presets; a breadcrumb/back control walks up. No folder
  list is expanded all at once.
- [ ] B2. A folder row has an icon and a different weight and size from a
  preset row.
- [ ] B3. List rows have token padding.
- [ ] B4. Flat rectangular tag buttons replace the outlined pill chips:
  subtle fill, no outline, selection by fill.
- [ ] B5. Library tiles pad and align their text; tile size follows the
  content and the grid width.
- [ ] B6. The keyboard strip is thin, sized from tokens (Kontakt's strip as
  the reference).
- [ ] B7. Library tiles answer hover at once (no ease).

### PR 3 — the rest of the tester's list
- [ ] T1. The scan line never overlaps the format tags (screenshot 944c…).
- [ ] T2. "Everything is a lot more spacious": list and panel padding from
  tokens across the browser, rack and sheets.
- [ ] T3. Every remaining item, re-checked against a fresh screenshot and
  ticked here.
- Already landed: Settings folder list (#341), "Falcon / UVI" under the
  first tile row (#341).

## Measurements (PR 1)

Window sizes: before 590×390 (old minimum) / 1180×780 / 2124×1404, after
940×600 (new minimum) / 1180×780 / 2124×1404. Body text is `TEXT` = 12 pt
at 100 %.

| Window | Model | Body text | Browser | Tiles/row | Keyboard |
|---|---|---|---|---|---|
| minimum | before (zoom 0.5) | 6 px | 132 px | 2 | 7 oct |
| minimum | after | 12 px | 264 px | 2 | 6 oct |
| default | before (zoom 1) | 12 px | 264 px | 2 | 7 oct |
| default | after | 12 px | 264 px | 2 | 7 oct |
| 1.8× | before (zoom 1.8) | 21.6 px | 475 px | 2 | 7 oct |
| 1.8× | after | 12 px | 500 px | 3 | 10 oct |

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/ui-lift.md` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| UI reflow, audit, tokens | Compile/ABI | linux-x86_64 | `build-slot cargo nextest run -p kontra-ui --cargo-profile dsp-dev -E 'test(a_wider_window) \| test(mui_audit) \| test(the_chrome_takes)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Scale and saved view | Compile/ABI | linux-x86_64 | `build-slot cargo nextest run -p kontra-plugin --cargo-profile dsp-dev -E 'test(a_new_scale) \| test(look::)'` | Exit 0 | PASS (2026-11-25, linux-x86_64) |
| Shots | Hardware/OS-CI | linux-x86_64 | `build-slot cargo run -p kontra-ui --example app_shot --profile dsp-dev -- <dir>` | PNGs at three sizes, read by eye and table above | PENDING_OS_CI (linux) |

## Unverified Boundaries & Promotion Gate

- A host that refuses the resize a new interface scale asks for: the layout
  reflows into the old window (by design); not exercised in a real DAW here.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Reflow, tokens, guard | `crates/app/ui/**`, `plugin/src/editor.rs`, `plugin/src/look.rs`, `crates/app/host/src/state.rs` | — | rows above |
| 2 | Browser UX | `crates/app/ui/src/app/browser.rs`, `crates/app/ui/src/keyboard.rs`, `crates/app/ui/src/kit.rs` | 1 | B1–B7 tests + shots |
| 3 | Tester list | `crates/app/ui/**` | 2 | T1–T3 + shots |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` only.
- **Mirror Impact:** subtree mirror (`kontra`); nothing private.
- **Versioning & Changelog:** a CHANGELOG line per PR at release.
