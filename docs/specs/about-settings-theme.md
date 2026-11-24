---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# About, Settings, theming, standalone app, retired names

Branch `kontra-about-settings-theme`, stacked on `kontra-restore-classic-controls` (#166).

## Primary-Source & Version Proof

N/A — internal refactor: editor views, theme tokens, build stamping and a
standalone binary built from KONTRA's own crates and the in-tree moose
`moose-standalone` host; no external API, protocol or SDK assumption.

## Problem & Goal

- **Problem:** About said `Version 3.0.0-dev` while releases are `v0.6.x`;
  it had no build time or changelog. Settings mixed centred and left-aligned
  headers, and its dropdown arrows were empty boxes (MUI's Material Symbols
  glyph, absent from the bundled Noto Sans). The colours were fixed, with a
  slight blue tint KONTRA ≤ v0.6.20 did not have. Latches and buttons still
  eased their set state. There was no standalone app. Old "Kontakto" naming
  and dead v2 material lingered.
- **Goal:** each acceptance criterion below holds and is tested.
- **Plan Reference:** user request (T3 thread), KONTRA UI lane of
  `docs/architecture-v3/13-IR-VNEXT.md`.

## Acceptance criteria

1. **Version:** `plugin/Cargo.toml` is `0.6.24`, the release after the
   newest KONTRA release `v0.6.23` (DerpcatMusic/KONTRA). Every release
   raises PATCH (`tools/version.py`); MINOR only for a `!` title. The
   monorepo's `v0.6.91` tag belongs to another plugin
   (`pure_va_dispersion_core`), not KONTRA. About, the CLAP/VST3 descriptors
   (crate version) and `matari version kontra` agree; nothing shows `3.0.0`.
2. **About:** a sheet about 64×46 text units (clamped to the window). Left:
   name, version, commit, build date and time (UTC, stamped by `build.rs`,
   `SOURCE_DATE_EPOCH` honoured), target triple, formats. Right: CHANGELOG.md
   embedded at build time, newest first, headings and bullets, scrolling.
3. **Settings:** sections Library, Interface, Appearance, MIDI & Output,
   About & diagnostics; every header left-aligned, every row label-left /
   control-right at one row height. Dropdown arrows are vector icons
   (`Icon::Down`/`Icon::Up`); no string in About or Settings draws `.notdef`.
4. **Theme:** the colour tokens (background, accent, secondary text,
   warning) are one process-wide `Look`; Settings → Appearance offers presets
   (Graphite default, Midnight, Ember, Moss), a tint slider and per-token
   editing with MUI's colour picker (hex field included). Edits apply on the
   frame they happen, in every open editor, and are saved as `theme.json`
   (token → `#rrggbb`) beside `prefs.json`. The default (Graphite) has zero
   chroma: neutral grey, as ≤ v0.6.20. UI scale stays in Settings.
5. **Precision:** no value easing on KONTRA controls: `kit::interactive` no
   longer springs paint, so latches and on/off buttons switch the frame they
   change. Knob, slider and wedge stay instant (#166). Layout glides
   (browser width, keyboard) and the tab underline are not values and stay.
   Meters are out of scope.
6. **Standalone:** `kontra-plugin` has a `standalone` feature and a
   `kontra-standalone` binary (`moose_standalone::run`).
   `matari build kontra --standalone` builds it; the monorepo
   `release-plugin.yml` builds and stages it for kontra; the mirror's
   `release.yml` puts it in the Linux and Windows archives.
7. **Retired names:** user-facing/project "Kontakto" references in live
   docs and tools read KONTRA; dead v2 material is removed (listed in the
   PR). Historical dated audit evidence keeps its original text. NI's
   Kontakt format names are untouched.

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** no audio-thread change. Theme reads take an
  uncontended `RwLock` on the UI thread only.
- **Determinism & Compatibility:** saved plugin state is unchanged; old
  `prefs.json` files load as before; a missing or invalid `theme.json` (or
  token) falls back to Graphite.
- **Mirror Boundaries:** everything stays under `plugins/kontra` except the
  monorepo build wiring (`tools/matari.py`, `.github/workflows/release-plugin.yml`).

### Non-Goals
- A light-mode theme, per-part colour editing, theme import/export.
- macOS standalone packaging inside the `.pkg` (the binary builds; the pkg
  layout is unchanged).
- Rewriting historical audit evidence or v2 design records.

## API / Data / File Contract

- `kontra_ui::app::Build { version, revision, built, target, formats, changelog }`
  (all `&'static str`), `kontra_plugin::BUILD`.
- `kontra_ui::app::changelog(&str) -> Vec<Release { heading, lines }>`.
- `kontra_ui::theme::{Look, TOKENS, look, set_look, sync, hex, parse_hex}`.
- `kontra_ui::app::Action::Theme(BTreeMap<String, String>)`.
- `Browser::{theme, set_theme}`; file `<config>/kontra-v3/theme.json`.
- Cargo: feature `standalone`, bin `kontra-standalone`.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/about-settings-theme.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64, checked=1 passed=1 failed=0) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `build-slot cargo nextest run -p kontra-ui-view -p kontra-ui -p kontra-host` + `kontra-plugin` fast suite | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64, 371/371 ui-view/ui/host + plugin version/contract/diagnostics, mui::audit at 900x600 & 1180x780, zero .notdef) |
| Standalone build | Compile/ABI | linux-x86_64 | `python3 tools/matari.py build kontra --standalone --debug --stage-dir ...` | Exit 0, `kontra-standalone` built & `--help` prints usage | PASS (2026-11-24, linux-x86_64, kontra-standalone built, staged, and --help verified) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green | PENDING (runs in CI `verify-heavy (plugins/kontra)`) |

## Unverified Boundaries & Promotion Gate

- The full PR gate runs in CI (`verify-heavy (plugins/kontra)`); promote to `implemented` once it passes with its run URL.
- Windows/macOS standalone builds are proven only by the mirror's release run.
