# 14 Unified Library UI IR: One Format-Neutral UI Pipeline

> **Companion to [13-IR-VNEXT.md](13-IR-VNEXT.md) and [VISION.md](VISION.md).**
> Audio already follows `format → translator laws → neutral IR → one engine`.
> Library interfaces must follow the exact same rule:
> **`format UI → UI translator laws → neutral UI IR (kontra-ui-model) → one renderer/input (kontra-ui-view / kontra-ui)`**.

## 1. Diagnosis: Why Library UIs Feel Fragmented Today

`kontra-ui-model::Interface` was designed to be the format-neutral UI IR, but
format-specific rules leaked downstream into the layout, model, and renderer
layers via `Interface::source` (`Source::Ksp`, `Source::PerformanceView`,
`Source::FalconLua`). Meanwhile, new formats like DecentSampler (`<ui>`) have
no UI translator at all (`crates/format/decentsampler/src/translate.rs:139`
logs `element <ui>` as ignored), and Kontakt itself is split across three
frontends (`face/src/widget.rs`, `face/src/perf_view.rs`, `face/src/nui.rs`)
beside UVI's `face/src/lua.rs`.

### Complete Inventory of Vendor `Source::*` Leaks (Measured on `main`, 2026-11-23)

Every non-test `Source::` branch below violates [VISION.md](VISION.md) ("never
branch on the source format below the translators"):

| # | Location | Current Branch | Format Behavior Actually Being Encoded |
|---:|---|---|---|
| 1 | `crates/app/ui/src/layout.rs:76` | `if tree.source == Source::FalconLua` | Whether a non-stretching picture overrides the widget's rect (`PictureSizing::IntrinsicOverridesRect` vs `PreserveAuthoredRect`) |
| 2 | `crates/model/ui-model/src/lib.rs:730` | `if source == Source::FalconLua` | Numeric readout decimal formatting (`ValueFormat::CompactTrimmed` vs `FixedPrecision`) |
| 3 | `crates/model/ui-model/src/lib.rs:1275` | `matches!(self.source, Ksp \| PerformanceView)` | Z-layer stacking scope (`ZOrder::Global` vs `ZOrder::SiblingLocal`) |
| 4 | `crates/app/ui-view/src/plan.rs:234` | `matches!(source, Ksp)` | Tab expansion in labels (`\t` → `"  "` should happen in the KSP UI translator, not the renderer) |
| 5 | `crates/app/ui-view/src/plan.rs:282` | `let lua = matches!(source, FalconLua)` | Line pitch (`13/11` vs `1/em`), single-line vertical anchor (`Top` vs `Middle`), label/menu horizontal insets |
| 6 | `crates/app/ui-view/src/stock.rs:835` | `matches!(tree.source, Ksp)` | Unpictured fallback skin (`StockSkin::Kontakt` vs `StockSkin::Flat` / `None`) |
| 7 | `crates/app/ui-view/src/lib.rs:147` | `matches!(tree.source, Ksp \| PerformanceView)` | 6-state interactive strip tracking (should be driven by `ImageUse::frame_map` / `Widget::interactive_states`) |
| 8 | `crates/app/ui-view/src/lib.rs:334` | `matches!(tree.source, Ksp)` | Stock bitmap pixel-font atlas lookup (`Font::Stock(id)` already identifies a Kontakt stock font!) |
| 9 | `crates/app/ui-view/src/lib.rs:356` | `matches!(tree.source, Ksp)` | Stock Raldo outline font for `Font::Stock(23)` |
| 10 | `crates/app/ui-view/src/lib.rs:497` | `matches!(tree.source, Ksp)` | 1 px button/switch picture inset (`ImageUse::inset: [f32; 2]`) |
| 11 | `crates/app/ui-view/src/lib.rs:529` | `matches!(tree.source, FalconLua)` | `uvi: bool` table rendering branch (should be driven by `TableStyle`: inner-edge gap, border rim, zero-line rule) |
| 12 | `crates/app/ui-view/src/lib.rs:914,925` | `matches!(source, FalconLua \| Ksp)` | Horizontal text squash (`TextFit::Squash { min_x: 0.7 }` vs `TextFit::StepDown`) & variable font weight (`TextStyle::weight`) |

**Total vendor branches in UI core: 12 → Target: 0.**

## 2. Target Architecture: Shared UI IR (`kontra-ui-model` vNext)

Every library format compiles its UI into the same orthogonal IR types in
`kontra-ui-model`, keeping all product-specific conventions as **data** in
each format's UI translator:

```text
  Kontakt KSP + .nckp + native_ui ──┐
  UVI / Falcon Lua                  ├──▶  kontra-ui-model (Neutral UI IR)  ──▶  kontra-ui-view (pure draw)
  DecentSampler <ui> XML            │       • Widget tree + Panel/Viewport       kontra-ui::input (gestures)
  SFZ / HALion / Opus (future)      │       • Style / TextPolicy / FitMode       kontra-ui::layout (resolve)
  Native .kontra UI                 ┘       • ImageUse { role, inset, frames }   kontra-ui-pictures (decode)
                                            • Binding (Variable | ParamAddr)
```

### Key Orthogonal Primitives to Add to `kontra-ui-model`

1. **`Interface` Page-Level Policies (data, not `Source` enum checks):**
   - `z_order: ZOrder` (`Global` for KSP/PerformanceView; `Sibling` for Falcon/DecentSampler/native).
   - `picture_sizing: PictureSizing` (`IntrinsicWhenUnstretched` for KSP; `AuthoredRect` for Falcon/DecentSampler).
   - `stock_skin: StockSkin` (`Kontakt`, `Minimal`, `None`).
2. **`TextStyle` & `TextPolicy` (replaces all `lua`/`ksp` text branches):**
   - `weight: Option<f32>` (explicit `400`/`600`/`700`, replacing `stock_weight(id)` lookup in `ui-view`).
   - `line_pitch: LinePitch` (`Ratio(f32)` e.g. `13.0/11.0`, or `FontLineHeight` for JUCE/UVI).
   - `vertical_align: VerticalAlign` (`Top` vs `Middle`), replacing `label_from_top`.
   - `inset: [f32; 2]` (left/right horizontal text padding, replacing the `(6, height+1) / (5, 5) / (7, 7) / (2, 2)` hardcoded match).
   - `fit: TextFit` (`Squash { min_scale: f32 }` for JUCE/UVI; `StepDown` for Kontakt; `Clip`).
   - `value_format: ValueFormat` (moved onto `Display`, removing `source: Source` from `Display::format_value`).
3. **`ImageUse` Geometry & Frame Rules (replaces KSP-only strip/inset checks):**
   - `inset: [f32; 2]` (KSP button/switch sets `[1.0, 1.0]`; everyone else `[0.0, 0.0]`).
   - `strip_mode: StripMode` (`Value`, `SixStateKontakt`, `TwoStateToggle`, `Fixed(u32)`), replacing `matches!(tree.source, Ksp)` in `drawn_page` and `plan::frame`.
4. **`TableStyle` (unifies Kontakt and UVI tables into one drawer):**
   - `frame_rim: bool` (1 px border + inset for stock Kontakt tables; `false` for UVI/pictured/HIDE_PART_BG).
   - `inner_edge: TableInnerEdge` (gap width or seamless overlap).
   - `min_zero_bar_px: f32` (`1.0` for UVI bipolar zero-indicator; `0.0` for Kontakt).
5. **Unified Control Binding (`Binding::Param(ir::ParamAddr)` alongside `Binding::Variable`):**
   - Today `Binding` only models script variables (`KSP`/`Lua`). Adding `Binding::Param(ParamAddr)` lets **non-scripted formats** (DecentSampler `<ui>` bindings, SFZ CC/macro UIs, native `.kontra` panels) drive `Engine::set_param(ParamAddr)` directly through the exact same `kontra-ui-view` + `kontra-ui::input` pipeline with zero script VM overhead!

### How Reverse Engineering (`kuna` / `rea` / Oracle) Feeds the UI IR

When verifying subtle UI laws in Kontakt 8 or UVI Workstation/Falcon:
- Use `kuna` / `rea` on native binaries to confirm exact layout formulas, hit-test rules, font metrics, and strip-frame state tables.
- Record every verified law in the **format's UI translator** (`face/src/widget.rs`, `face/src/perf_view.rs`, `face/src/lua.rs`, or `format/decentsampler/src/ui.rs`) as data on `kontra-ui-model` — **never** as a `Source::*` branch in `ui-view` or `ui`.

## 3. Phased Migration Plan (Disjoint Lanes)

| Phase | Goal | Owned Paths | Target Metric |
|---|---|---|---|
| **U0** (this PR) | Fix immediate Kontakt UI rendering gaps (`state_styles`, `text_shift_on_press`, Kontakt `ui_table` `steps_shown`/`bipolar`/colors/picture/`HIDE_PART_BG`, `value_y`, `menu_arrow`) | `crates/app/ui-view` | All 5 Kontakt UI defect classes covered by pixel tests |
| **U1** | Add orthogonal UI IR policy fields (`ZOrder`, `PictureSizing`, `TextPolicy`, `ImageUse::{inset, strip_mode}`, `TableStyle`) to `kontra-ui-model`; populate from `face/{widget,perf_view,lua,nui}` | `crates/model/ui-model`, `crates/app/face` | Policy fields populated by every existing frontend |
| **U2** | Switch `kontra-ui-view`, `kontra-ui/src/layout.rs`, and `ui-model` methods onto policy fields; delete every `Source::*` branch outside translators | `crates/app/ui-view`, `crates/app/ui/src/layout.rs`, `crates/model/ui-model` | `Source::` branches in `ui-model` + `ui-view` + `ui`: **12 → 0** |
| **U3** | Add `Binding::Param(ParamAddr)` and implement DecentSampler `<ui>` translator (`<tab>`, `<labeled-knob>`, `<control>`, `<button>`, `<menu>`, `<label>`, `<image>`) + `.kontra` native UI schema | `crates/model/ui-model`, `crates/format/decentsampler`, `crates/app/host` | DecentSampler `<ui>` renders & drives parameters through the shared UI IR |
