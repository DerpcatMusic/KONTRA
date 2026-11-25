//! Format-neutral description of a library's instrument interface.
//!
//! Source frontends (KSP `on init`, Creator Tools performance views, Falcon
//! Lua) emit an [`Interface`]; the product UI renders it with either the
//! library's own pictures or KONTRA's controls in the same places
//! ([`Presentation`]).
//!
//! Plain data: no I/O, no decoded pixels, no runtime handles, no dependencies.
//! Assets are referenced by library-relative path and loaded lazily by the
//! renderer. Values live in the headless control service; a widget only names
//! what it shows through its [`Binding`]. Meaning a frontend cannot express is
//! listed in [`Interface::unsupported`] rather than dropped.
//!
//! Geometry is in source pixels at 1x. A child's [`Widget::rect`] is relative
//! to its parent panel, as in KSP (`$CONTROL_PAR_PARENT_PANEL`) and Falcon.
#![forbid(unsafe_code)]

use std::fmt;

pub use kontra_ir as ir;

mod canvas;
mod preset_name;
mod scroll;
mod value;
pub use canvas::{Paint, Seg};
pub use preset_name::{
    displayed_preset_family, preset_family, preset_family_for, preset_file_name, preset_owner,
};

macro_rules! reference {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub usize);
    )*};
}

reference! {
    /// Index into [`Interface::pages`].
    PageRef,
    /// Index into [`Interface::widgets`].
    WidgetRef,
    /// Index into [`Interface::assets`].
    AssetRef,
    /// Index into [`Interface::styles`].
    StyleRef,
}

/// Stable identity of a headless control owned by the engine's control
/// service (`sampler_core::ControlId`'s value). Independent of widget order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlId(pub u128);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Interface {
    pub source: Source,
    /// The source explicitly declares an authored performance view.
    pub performance: bool,
    /// How widget z-layers resolve across panels (`Global` for KSP/Kontakt,
    /// `SiblingLocal` for hierarchical trees like Falcon Lua).
    pub z_order: ZOrder,
    /// How unstretched image sizes interact with authored widget bounds during
    /// layout resolution.
    pub picture_sizing: PictureSizing,
    /// Fallback visual style for controls without authored pictures.
    pub stock_skin: StockSkin,
    /// Typography layout, inset, and fitting policy for this interface.
    pub text_policy: TextPolicy,
    /// Default visual policy for [`Kind::Table`] widgets on this interface.
    pub table_style: TableStyle,
    pub pages: Vec<Page>,
    /// Declaration order; ties in [`Widget::z`] draw in this order.
    pub widgets: Vec<Widget>,
    /// Authored back-to-front traversal; `None` uses the source's default order.
    pub paint_order: Option<Vec<WidgetRef>>,
    pub assets: Vec<Asset>,
    pub styles: Vec<TextStyle>,
    /// The instrument's icon in the host's rack header (KSP `$INST_ICON_ID`).
    pub icon: Option<AssetRef>,
    /// The source hides the instrument icon (KSP `$INST_ICON_ID` `HIDE`).
    pub icon_hidden: bool,
    /// A legacy authored NativeUI entry point, consumed by the editor frontend.
    pub native_ui: Option<NativeUi>,
    /// The keys the source colours on the keyboard, by note, with the
    /// colour each draws (KSP `set_key_color`).
    pub keybed: Vec<(u8, Rgba)>,
    /// The keys the source names, by note (KSP `set_key_name`): shown when
    /// the pointer is over them.
    pub key_names: Vec<(u8, String)>,
    /// Set when the source shows only the keys it presses (KSP
    /// `set_key_pressed_support`): those keys, in place of the keys played.
    pub key_pressed: Option<Vec<u8>>,
    /// The last status text the source printed (KSP `message`).
    pub message: String,
    pub unsupported: Vec<Unsupported>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeUi {
    pub entry: String,
}

/// Which frontend produced the interface (diagnostic / telemetry tag only;
/// rendering and layout behavior are driven by explicit policy fields).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    /// No authored UI: generated from the instrument's controls.
    #[default]
    Generated,
    /// A KSP script's `on init` declarations, in script slot `slot`.
    Ksp { slot: u8 },
    /// A Creator Tools GUI Designer performance view (`.nckp`).
    PerformanceView,
    /// A Falcon / UVI Lua script.
    FalconLua,
}

/// How [`Widget::z`] orders widgets across container panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ZOrder {
    /// Siblings sort within their parent panel and children draw immediately
    /// after their parent (hierarchical UI trees such as Falcon Lua).
    #[default]
    SiblingLocal,
    /// Global Z layers across panels with Kontakt's type-priority tie-break.
    Global,
}

/// How an unstretched image's intrinsic size interacts with a widget's rect
/// during layout resolution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PictureSizing {
    /// Unstretched image dimensions override the widget's rect (Kontakt's rule).
    #[default]
    IntrinsicWhenUnstretched,
    /// Keep the widget's authored rect without overriding from image headers.
    PreserveAuthoredRect,
}

/// Stock fallback appearance for controls that lack authored pictures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StockSkin {
    /// No format-specific stock fallback skin.
    #[default]
    None,
    /// Kontakt's stock control skin (knobs, buttons, fields, menus, labels).
    Kontakt,
    /// Minimal vector fallback skin for generic/declarative UIs.
    Minimal,
}

/// How a multi-line label pitches consecutive lines relative to its font size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LinePitch {
    /// Fixed multiple of font size (Kontakt's measured `13/11`).
    Ratio(f64),
    /// Font's own line height (JUCE / Falcon Lua `em_per_height`).
    FontLineHeight,
}

impl Default for LinePitch {
    fn default() -> Self {
        Self::Ratio(13.0 / 11.0)
    }
}

/// Default vertical anchor for single-line text when [`Widget::text_y`] is unset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    /// Labels anchor to the top of their box; other controls center.
    #[default]
    Top,
    /// Single-line text centers vertically inside the widget (JUCE / Falcon Lua).
    Middle,
}

/// How single-line text fits when wider than its widget's room.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum TextFit {
    /// Step font size down (`1.0`, `0.9`, `0.8`, `0.75`) until it fits (Kontakt).
    #[default]
    StepDown,
    /// Horizontally condense down to `min_scale` before falling back (JUCE / Falcon Lua).
    Squash { min_scale: f32 },
    /// Render at authored size and clip to widget bounds.
    Clip,
}

/// Interface-level typography layout, inset, and fitting policy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextPolicy {
    pub line_pitch: LinePitch,
    pub vertical_align: VerticalAlign,
    /// Horizontal `[left, right]` inset in source pixels for [`Kind::Label`].
    pub label_inset: [f32; 2],
    /// Horizontal `[left, right]` inset in source pixels for general controls.
    pub control_inset: [f32; 2],
    /// Horizontal `[left, right]` inset in source pixels for [`Kind::Menu`].
    pub menu_inset: [f32; 2],
    /// Extra right inset on [`Kind::Menu`] proportional to widget height (to clear an arrow box).
    pub menu_arrow_height_factor: f32,
    pub fit: TextFit,
}

impl TextPolicy {
    /// Kontakt's measured typography policy (default).
    pub const KONTAKT: Self = Self {
        line_pitch: LinePitch::Ratio(13.0 / 11.0),
        vertical_align: VerticalAlign::Top,
        label_inset: [7.0, 7.0],
        control_inset: [2.0, 2.0],
        menu_inset: [2.0, 2.0],
        menu_arrow_height_factor: 0.0,
        fit: TextFit::StepDown,
    };

    /// Falcon / UVI (JUCE) typography policy.
    pub const FALCON_LUA: Self = Self {
        line_pitch: LinePitch::FontLineHeight,
        vertical_align: VerticalAlign::Middle,
        label_inset: [5.0, 5.0],
        control_inset: [2.0, 2.0],
        menu_inset: [6.0, 1.0],
        menu_arrow_height_factor: 1.0,
        fit: TextFit::Squash { min_scale: 0.7 },
    };
}

impl Default for TextPolicy {
    fn default() -> Self {
        Self::KONTAKT
    }
}

/// How [`Display::format_value`] renders numeric readouts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ValueFormat {
    /// Divide by `ratio`, format whole numbers without decimals or 2 decimal places, append `unit`.
    #[default]
    FixedPrecision,
    /// Trim trailing zeros up to 3 decimals and interpret Falcon/UVI unit names (`Hertz`, `Seconds`, `MidiKey`, ...).
    CompactTrimmed,
}

/// How a widget selects a frame from a strip [`ImageUse`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StripMode {
    /// Frame chosen continuously from the widget's normalized value (or on/off for toggles).
    #[default]
    Value,
    /// Kontakt 6-state interactive button/switch strip (off/on, pressed, hover).
    SixStateKontakt,
    /// 2-state on/off toggle strip without hover/press frames.
    TwoStateToggle,
    /// Always display this fixed frame index.
    Fixed(u32),
}

/// How [`Kind::Table`] separates adjacent columns.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableGap {
    /// 1 px gap when bars are at least 3 px wide, 0 px otherwise (Kontakt).
    Adaptive,
    /// Per-cell background gaps when inner edges are visible, else seamless overlap (UVI).
    CellEdges {
        gap_px: f32,
        seamless_overlap_px: f32,
    },
}

/// Visual rendering policy for [`Kind::Table`] widgets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TableStyle {
    /// Draw a 1 px framed border inset around the table background.
    pub frame_rim: bool,
    pub inner_edge: TableGap,
    /// Minimum pixel height for a bipolar bar sitting at zero.
    pub min_zero_bar_px: f32,
    /// Draw a 1 px horizontal zero reference line on bipolar tables.
    pub draw_zero_line: bool,
}

impl TableStyle {
    /// Kontakt's framed table policy (default).
    pub const KONTAKT: Self = Self {
        frame_rim: true,
        inner_edge: TableGap::Adaptive,
        min_zero_bar_px: 0.0,
        draw_zero_line: true,
    };

    /// Falcon / UVI frameless cell-edge table policy.
    pub const UVI: Self = Self {
        frame_rim: false,
        inner_edge: TableGap::CellEdges {
            gap_px: 1.0,
            seamless_overlap_px: 0.5,
        },
        min_zero_bar_px: 1.0,
        draw_zero_line: false,
    };
}

impl Default for TableStyle {
    fn default() -> Self {
        Self::KONTAKT
    }
}

/// Curve mapping a widget's value range onto a physical IR parameter range.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ParamMap {
    /// Pass the widget's value straight through to the IR parameter.
    #[default]
    Direct,
    /// Linearly map the widget's normalized `[0, 1]` position onto `[min, max]`.
    Linear { min: f32, max: f32 },
    /// Exponentially map `[0, 1]` onto `[min, max]` (e.g. filter cutoff Hz).
    Exponential { min: f32, max: f32 },
    /// Power-law map `[0, 1]` onto `[min, max]`: `min + (max - min) * t.powf(exponent)`.
    Power { min: f32, max: f32, exponent: f32 },
}

impl ParamMap {
    /// Converts this UI mapping into its canonical [`ir::Law`], or `None` for [`Self::Direct`].
    #[must_use]
    pub fn to_law(self) -> Option<ir::Law> {
        match self {
            Self::Direct => None,
            Self::Linear { min, max } => Some(ir::Law::Linear { lo: min, hi: max }),
            Self::Exponential { min, max } => {
                if min > 0.0 && max > 0.0 {
                    Some(ir::Law::Exp { lo: min, hi: max })
                } else {
                    Some(ir::Law::Linear { lo: min, hi: max })
                }
            }
            Self::Power { min, max, exponent } => {
                if exponent.is_finite() && exponent > 0.0 {
                    Some(ir::Law::Power {
                        lo: min,
                        hi: max,
                        exponent,
                    })
                } else {
                    Some(ir::Law::Linear { lo: min, hi: max })
                }
            }
        }
    }

    fn bounds(self) -> Option<(f32, f32)> {
        match self {
            Self::Direct => None,
            Self::Linear { min, max }
            | Self::Exponential { min, max }
            | Self::Power { min, max, .. } => Some((min, max)),
        }
    }
}

/// Direct, script-free binding from a UI widget to a format-neutral engine parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamBinding {
    pub scope: ir::Scope,
    pub addr: ir::ParamAddr,
    pub map: ParamMap,
}

fn finite_nonzero_span(span: f64) -> bool {
    span.is_finite() && span != 0.0
}

impl ParamBinding {
    #[must_use]
    pub const fn new(scope: ir::Scope, addr: ir::ParamAddr) -> Self {
        Self {
            scope,
            addr,
            map: ParamMap::Direct,
        }
    }

    #[must_use]
    pub const fn with_map(scope: ir::Scope, addr: ir::ParamAddr, map: ParamMap) -> Self {
        Self { scope, addr, map }
    }

    /// Maps a widget value (in its authored `range`, if any) to the physical IR parameter value.
    #[must_use]
    pub fn to_physical(&self, widget_value: f64, range: Option<Range>) -> f32 {
        let Some(law) = self.map.to_law() else {
            return widget_value as f32;
        };
        let t = match range {
            Some(r) if finite_nonzero_span(r.max - r.min) => {
                ((widget_value - r.min) / (r.max - r.min)).clamp(0.0, 1.0)
            }
            Some(r) if r.min.is_finite() && r.min == r.max => 0.0,
            _ => widget_value.clamp(0.0, 1.0),
        };
        if let Some((min, max)) = self.map.bounds() {
            if t == 0.0 {
                return min;
            }
            if t == 1.0 {
                return max;
            }
        }
        law.eval_f64(t) as f32
    }

    /// Inverse of [`Self::to_physical`]: maps a physical IR parameter value back to widget units.
    #[must_use]
    pub fn from_physical(&self, physical: f32, range: Option<Range>) -> f64 {
        let Some(law) = self.map.to_law() else {
            return f64::from(physical);
        };
        let t = if let Some((min, max)) = self.map.bounds() {
            let span = f64::from(max) - f64::from(min);
            if physical.is_finite() && finite_nonzero_span(span) {
                law.inverse_f64(f64::from(physical)).unwrap_or(0.0)
            } else {
                0.0
            }
        } else {
            0.0
        };
        let t = t.clamp(0.0, 1.0);
        match range {
            Some(r) if finite_nonzero_span(r.max - r.min) => {
                if t == 0.0 {
                    r.min
                } else if t == 1.0 {
                    r.max
                } else {
                    r.min + (r.max - r.min) * t
                }
            }
            Some(r) if r.min.is_finite() && r.min == r.max => r.min,
            _ => t,
        }
    }
}

/// One screen of the interface. KSP declares one; Falcon may declare several.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub name: String,
    /// Width, and height unless [`Page::height_rows`] is set.
    pub size: Size,
    /// Height in the source's grid rows (KSP `set_ui_height`); the renderer
    /// converts it with its grid geometry.
    pub height_rows: Option<u32>,
    pub background: Background,
}

impl Page {
    /// The height it shows: its own and the rows above its y 0.
    #[must_use]
    pub fn shown_height(&self) -> f64 {
        self.size.height + f64::from(self.background.above)
    }
}

impl Background {
    /// The wallpaper row at the page's top edge.
    #[must_use]
    pub fn top_row(&self) -> u32 {
        self.origin_y.saturating_sub(self.above)
    }
}

/// What is drawn behind every widget on a page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Background {
    pub color: Option<Rgba>,
    /// The wallpaper. Always kept, in both presentations.
    pub image: Option<AssetRef>,
    /// Vertical source offset into the wallpaper, in pixels (KSP skin offset).
    pub offset_y: i32,
    /// Source/profile header rows, independent of the script's skin offset.
    pub origin_y: u32,
    /// Rows the page shows above its y 0: the wallpaper's rows over
    /// `origin_y`, and whatever widget a script placed there.
    pub above: u32,
    /// The selected wallpaper strip frame.
    pub frame: u32,
}

/// Raw, immutable modifiers from the actual UI input edge. Sources decide
/// which platform key corresponds to their documented Command convention.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UiModifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiActivationOrigin {
    PointerDown,
    PointerUp,
    Keyboard,
    Accessibility,
}
/// A primary UI activation, distinct from a programmatic scalar value write.
/// This describes input; it is not a Core admission or execution receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiActivation {
    pub origin: UiActivationOrigin,
    pub modifiers: Option<UiModifiers>,
}
impl UiActivation {
    pub fn valid(self) -> bool {
        matches!(
            (self.origin, self.modifiers),
            (
                UiActivationOrigin::PointerDown
                    | UiActivationOrigin::PointerUp
                    | UiActivationOrigin::Keyboard,
                Some(_)
            ) | (UiActivationOrigin::Accessibility, None)
        )
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    /// Source identity: KSP variable (with type prefix) or Lua widget name.
    pub name: String,
    /// Source numeric identity where one exists (KSP `get_ui_id`).
    pub source_id: Option<i32>,
    pub page: PageRef,
    /// Containing panel; geometry is relative to it.
    pub parent: Option<WidgetRef>,
    /// The knob or slider whose value this picture shows: the source sets
    /// its frame or position from that value. [`Presentation::Kontra`]
    /// leaves it out while that control is shown; the control shows its
    /// value itself.
    pub shows: Option<WidgetRef>,
    /// The menu whose chosen item this picture spells: the source picks
    /// the picture by that menu's value. Where [`Presentation::Kontra`]
    /// leaves the picture out ([`Self::shows`]), it writes that item.
    pub spells: Option<WidgetRef>,
    /// Position and size in pixels. Position is ignored for
    /// [`Placement::Grid`]; size is ignored when [`Widget::auto_size`].
    pub rect: Rect,
    pub placement: Placement,
    /// The source did not size the widget; the renderer uses its default
    /// size for the kind and source.
    pub auto_size: bool,
    /// Default width/height independently. Explicit zero stays zero.
    pub default_axes: [bool; 2],
    /// Higher draws later: global layers for KSP, sibling layers for Lua.
    pub z: i32,
    /// Hidden as a whole (`HIDE_WHOLE_CONTROL`, Lua `visible = false`).
    pub hidden: bool,
    /// Parts hidden while the control itself shows.
    pub hide: Parts,
    pub enabled: bool,
    pub kind: Kind,
    pub binding: Binding,
    /// Caption, button text or label text; lines split on `\n` (KSP
    /// per-index label lines are joined with `\n`).
    pub text: String,
    /// Vertical offset of the text inside the widget, in pixels (KSP
    /// `TEXTPOS_Y`); `None` centres it.
    pub text_y: Option<i32>,
    /// Shift a button/switch caption while pressed (KSP's stock behavior).
    /// Sources without this behavior leave it false.
    pub text_shift_on_press: bool,
    /// Source-requested primary pointer activation at press instead of release.
    /// Keyboard and accessibility remain primary actions, not pointer edges.
    pub activate_on_press: bool,
    pub value_y: Option<i32>,
    /// Shown instead of the formatted value (KSP knob `LABEL`).
    pub value_text: Option<String>,
    pub tooltip: String,
    pub automation: Automation,
    /// How a drag changes the value; `None` for the renderer's default.
    pub drag: Option<Drag>,
    /// The source runs the control's callback once more when the pointer
    /// lets go, even when the value did not change (KSP `on ui_control`).
    pub reports_release: bool,
    /// Colours the source sets on its stock drawing; unset ones are the renderer's.
    pub colors: Colors,
    /// Authored separation between table bars. Unset preserves the stock drawing.
    pub table_inner_edge: Option<TableInnerEdge>,
    /// Optional per-widget table rendering policy overriding [`Interface::table_style`].
    pub table_style: Option<TableStyle>,
    /// Explicit supported table fill; absent retains the renderer's fallback.
    pub table_fill: Option<TableFill>,
    pub style: Option<StyleRef>,
    /// Off/on, off/on pressed, off/on hover; absent entries inherit `style`.
    pub state_styles: [Option<StyleRef>; 6],
    /// Bitmaps the source draws this widget with, by role.
    pub images: Vec<ImageUse>,
    /// Table cell or XY axis controls, in component order.
    pub components: Vec<ControlId>,
    /// Scalar state for controls without a numeric range (toggles/menus).
    pub initial_value: f64,
    pub opacity: f32,
    /// Degrees clockwise about its own centre (a `native_ui` knob pointer).
    pub rotation: f64,
    pub intercepts_mouse: bool,
    /// Clip children and offset their origin (UVI Viewport).
    pub viewport: Option<[i32; 2]>,
    /// Source response curve, e.g. UVI Exponential.
    pub mapper: Option<String>,
    /// MultiStateButton advances on click; Menu opens a choice list.
    pub menu_cycle: bool,
    /// Show the stock right-hand menu arrow; authored skins may hide it.
    pub menu_arrow: bool,
    /// Validated slash-path menu tree; leaf indices refer to Kind::Menu items.
    pub menu_hierarchy: Option<Vec<MenuNode>>,
    /// Explicit authored value-popup policy. Unset retains the source default.
    pub popup_display: Option<bool>,
    /// Even coordinate index of the manually selected XY cursor.
    pub active_index: Option<i32>,
    /// Current source value for typed widgets; numeric controls use their service.
    pub value: Option<Value>,
    pub waveform: Option<Waveform>,
    /// Unresolved Falcon display request; independent of a Kontakt zone descriptor.
    pub uvi_wave_view_request: Option<UviWaveViewRequest>,
    pub meter: Option<MeterAddress>,
    /// Exact Falcon element request. It requires its own resolved tap and must
    /// never fall back to an unrelated Kontakt/output meter address.
    pub uvi_meter_request: Option<UviMeterRequest>,
    /// Source display endpoints, including an inverted meter scale.
    pub meter_range: Option<[i32; 2]>,
}

impl Widget {
    /// A visible, enabled widget with no binding, text or images.
    pub fn new(name: impl Into<String>, page: PageRef, rect: Rect, kind: Kind) -> Self {
        Self {
            name: name.into(),
            source_id: None,
            page,
            parent: None,
            shows: None,
            spells: None,
            rect,
            placement: Placement::Pixels,
            auto_size: false,
            default_axes: [false; 2],
            z: 0,
            hidden: false,
            hide: Parts::default(),
            enabled: true,
            kind,
            binding: Binding::None,
            text: String::new(),
            text_y: None,
            text_shift_on_press: false,
            activate_on_press: false,
            value_y: None,
            value_text: None,
            tooltip: String::new(),
            automation: Automation::default(),
            drag: None,
            reports_release: false,
            colors: Colors::default(),
            table_inner_edge: None,
            table_style: None,
            table_fill: None,
            style: None,
            state_styles: [None; 6],
            images: Vec::new(),
            components: Vec::new(),
            initial_value: 0.0,
            opacity: 1.0,
            rotation: 0.,
            intercepts_mouse: true,
            viewport: None,
            mapper: None,
            menu_cycle: false,
            menu_arrow: true,
            menu_hierarchy: None,
            popup_display: None,
            active_index: None,
            value: None,
            waveform: None,
            uvi_wave_view_request: None,
            meter: None,
            uvi_meter_request: None,
            meter_range: None,
        }
    }

    /// The image this widget uses in `role`, if any.
    pub fn image(&self, role: Role) -> Option<AssetRef> {
        self.images.iter().find(|i| i.role == role).map(|i| i.asset)
    }

    /// A button or switch with no text: its picture carries the label (an
    /// icon, baked words) or is a deliberately invisible hit area, so a
    /// vector presentation keeps it rather than draw a blank box.
    pub fn label_in_image(&self) -> bool {
        matches!(self.kind, Kind::Button { .. } | Kind::Switch)
            && self.text.trim().is_empty()
            && self.image(Role::Strip).is_some()
    }

    /// The widget's numeric value range, when its kind has one.
    #[must_use]
    pub fn range(&self) -> Option<Range> {
        match self.kind {
            Kind::Knob { range, .. }
            | Kind::Slider { range, .. }
            | Kind::ValueEdit { range, .. }
            | Kind::Table { range, .. } => Some(range),
            Kind::Button { .. } | Kind::Switch => Some(Range {
                min: 0.0,
                max: 1.0,
                default: 0.0,
                step: Some(1.0),
            }),
            _ => None,
        }
    }
}

/// How a widget's position is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Placement {
    /// [`Widget::rect`]'s `x`/`y`.
    #[default]
    Pixels,
    /// A cell of the source's layout grid, 1-based (KSP `move_control`).
    Grid { column: u32, row: u32 },
}

/// How the host sees a control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Automation {
    /// Name in the host's automation list, when it differs from the text.
    pub name: Option<String>,
    /// Abbreviation for narrow host displays (KSP `SHORT_NAME`).
    pub short_name: Option<String>,
    /// Whether the host may automate it (KSP `ALLOW_AUTOMATION`).
    pub allowed: bool,
    /// Fixed automation slot (KSP `AUTOMATION_ID`, 0..=2047).
    pub id: Option<u32>,
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            name: None,
            short_name: None,
            allowed: true,
            id: None,
        }
    }
}

/// Source-owned widget state; arrays and strings retain their authored types.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Integer(i32),
    Real(f64),
    Text(String),
    Integers(Vec<i32>),
    Reals(Vec<f64>),
    /// Transient OS drop payload; never published as a widget value snapshot.
    DropPath {
        kind: DropKind,
        path: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropKind {
    Audio,
    Midi,
    Array,
}

/// A physical meter tap, using the source's group, effect slot and bus identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeterAddress {
    pub group: i32,
    pub slot: i32,
    pub channel: u8,
    pub bus: Option<i32>,
}

/// Authored Falcon element identity, independent of DSP packing or UI order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UviMeterScope {
    Program,
    Layer { ordinal: u32 },
    Keygroup,
    Oscillator,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UviMeterRequest {
    /// Canonical promoted-element ordinal exposed by Lua `__id`.
    pub element_id: u32,
    /// Actual XML NodeId from the same promoted element's retained source map.
    pub element_node: u32,
    pub scope: UviMeterScope,
    pub bus_id: u32,
    pub use_output: bool,
    /// Authored mono-sum request; no channel averaging law is implied here.
    pub display_as_mono: bool,
}

/// Attached source zone and waveform properties. Zone is the native ID, not
/// an index into a translated sample list; the importer resolves that identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waveform {
    pub zone: i32,
    pub flags: u32,
    pub cursor_us: i64,
    pub table: Vec<i32>,
    pub highlighted: Option<u32>,
    pub midi_start_note: u8,
    /// The zone's sample as columns of (lowest, highest) level, ±127 full
    /// scale; empty until the host read it.
    pub peaks: Vec<[i8; 2]>,
    /// The sample's length, for the cursor.
    pub length_us: i64,
    /// Drawn as the sample's line seen through (KSP `WF_VIS_MODE` X-ray).
    pub xray: bool,
    /// Per column of [`Waveform::peaks`], how many samples fall in each of
    /// [`WAVE_BINS`] bands, top (+1) to bottom (-1): the X-ray's ink.
    pub density: Vec<u16>,
    /// A wavetable's frames, each [`FRAME_POINTS`] levels, ±127 full scale.
    pub frames: Vec<Vec<i8>>,
    /// The frame the table plays: its group's position, nearest frame.
    pub frame: usize,
    /// Alphas the source set: background, wave, wave end, other frames,
    /// their end (KSP `BG_ALPHA`, `WAVE_ALPHA`, ..., 0 to 255).
    pub alpha: [Option<u8>; 5],
}

/// Bands a waveform column's samples are counted in.
pub const WAVE_BINS: usize = 64;
/// Points a wavetable frame is drawn through.
pub const FRAME_POINTS: usize = 256;

/// Exact authored WaveView display request. This does not assert that a
/// sample provider, oscillator binding or native paint law is available.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UviWaveViewRequest {
    pub sample: UviWaveViewSample,
    /// Assignment-time module context; never substitute a host CWD/search root.
    pub script_directory: Option<String>,
    pub binding: UviWaveViewBinding,
    pub colors: UviWaveViewColors,
}

/// Manual display sample, independent of the bound oscillator's own sample.
/// Unspecified leaves the native default unknown. Empty strings remain authored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UviWaveViewSample {
    #[default]
    Unspecified,
    /// An explicit sample=nil assignment, independent of an unset constructor.
    Cleared,
    /// Original authored path; resolve with its authenticated source context.
    Manual(String),
    /// The Lua source remains intact; do not invent a usable sample request.
    Unsupported { type_name: String },
}

/// A retained oscillator request, never a Kontakt zone ID or sample edit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UviWaveViewBinding {
    #[default]
    Unspecified,
    Oscillator {
        element_id: u32,
        element_node: u32,
    },
    Unsupported {
        type_name: String,
    },
}

/// Each unset color leaves that native default unknown. Invalid authored values
/// also retain a per-property finding; these are requests, not painted colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UviWaveViewColors {
    pub playback_cursor: Option<Rgba>,
    pub high_wave: Option<Rgba>,
    pub low_wave: Option<Rgba>,
    pub axis: Option<Rgba>,
    pub channel_separator: Option<Rgba>,
    pub sample_name: Option<Rgba>,
    pub sample_name_background: Option<Rgba>,
}

/// The drag gesture of a continuous control: which way it moves, and how
/// far the pointer travels to sweep the whole range. The translator sets
/// its format's law; the renderer only follows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    pub axis: Orientation,
    pub sweep: Sweep,
}

/// How far a drag travels to sweep a control's whole range, in view
/// points at 100 % (the renderer scales them with the view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sweep {
    /// A fixed distance, whatever the control's size.
    Points(f64),
    /// This many times the control's own length along the axis.
    Lengths(f64),
    /// This far per unit of the value.
    PerUnit(f64),
}

/// Colours of the stock drawing (KSP `*_COLOR` control parameters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Colors {
    pub background: Option<Rgba>,
    pub on: Option<Rgba>,
    pub off: Option<Rgba>,
    pub bar: Option<Rgba>,
    /// Authored menu border and right-hand arrow ink.
    pub outline: Option<Rgba>,
    pub arrow: Option<Rgba>,
    pub peak: Option<Rgba>,
    pub overload: Option<Rgba>,
    pub zero_line: Option<Rgba>,
    pub wave: Option<Rgba>,
    pub wave_cursor: Option<Rgba>,
    pub slice_markers: Option<Rgba>,
    /// A wave's gradient end, and a wavetable's other frames and their end.
    pub wave_end: Option<Rgba>,
    pub wavetable: Option<Rgba>,
    pub wavetable_end: Option<Rgba>,
}

/// Table separator policy. An unknown native default colour remains unset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableInnerEdge {
    pub visible: bool,
    pub color: Option<Rgba>,
}

/// Supported authored table background/bar drawing. Native gloss is uncalibrated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableFill {
    Solid,
}

/// Parts of a control hidden individually (KSP `HIDE_PART_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Parts {
    pub background: bool,
    pub value: bool,
    pub title: bool,
    pub unit: bool,
}

/// What a widget is, with the data only that kind has.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Container for other widgets; draws nothing of its own but its images.
    Panel,
    Knob {
        range: Range,
        display: Display,
    },
    Slider {
        range: Range,
        orientation: Orientation,
    },
    /// A latching on/off button (KSP `ui_button`, Lua `OnOffButton`) or a
    /// momentary one (Lua `Button`).
    Button {
        momentary: bool,
    },
    /// KSP `ui_switch`: latching, drawn as a switch.
    Switch,
    Menu {
        items: Vec<MenuItem>,
    },
    Label,
    /// `arrows`: step buttons beside the value (KSP `SHOW_ARROWS`).
    ValueEdit {
        range: Range,
        display: Display,
        arrows: bool,
    },
    /// `cells`: initial values, one per column (may be shorter than `columns`);
    /// `steps_shown`: number of leading columns displayed across the full widget
    /// width (KSP `set_table_steps_shown`); absent means all columns.
    Table {
        columns: u32,
        range: Range,
        bipolar: bool,
        cells: Vec<f64>,
        steps_shown: Option<u32>,
    },
    /// `sensitivity`: per-axis drag sensitivity in source units (KSP
    /// `MOUSE_BEHAVIOUR_X`/`_Y`); `mouse_mode`: the source's pointer mode,
    /// verbatim (KSP `MOUSE_MODE`), until its meanings are verified.
    Xy {
        cursors: u32,
        sensitivity: [Option<u32>; 2],
        mouse_mode: Option<i32>,
    },
    Waveform,
    /// `three_d`: every frame drawn, not only the current one (KSP
    /// `WT_VIS_MODE` `$NI_WT_VIS_3D`); `parallax`: how far the frames step
    /// apart across and down, millionths (KSP `PARALLAX_X`/`_Y`).
    Wavetable {
        three_d: bool,
        parallax: [i32; 2],
    },
    /// `channel`: the output channel it shows (0 left, 1 right; KSP
    /// `attach_level_meter`); `None` shows nothing. `bus`: the instrument bus
    /// it reads, `None` the part's output.
    LevelMeter {
        orientation: Orientation,
        channel: Option<u8>,
        bus: Option<u8>,
    },
    /// `base_path`: the folder it opens at (KSP `BASEPATH`); `files`: what it lists;
    /// `column_width`: list column width in pixels; `tops`: the first row
    /// each open column shows (the player's scrolling). The selected path
    /// is the widget's text.
    FileSelector {
        base_path: Option<String>,
        files: Files,
        column_width: Option<u32>,
        tops: Vec<u32>,
    },
    TextEdit,
    /// A static picture (Lua `Image`); KSP pictures on labels are [`Kind::Label`].
    Image,
    /// An invisible target files are dropped on. `takes`: how many audio,
    /// MIDI and array files it accepts (0 none, 1 one, more any number);
    /// `drag`: whether a drag passing over it calls back too, not only
    /// the drop.
    MouseArea {
        takes: [u16; 3],
        drag: bool,
    },
    /// A shape a Kontakt 8 `native_ui` page draws, filled with
    /// `colors.background`.
    Shape(Shape),
    /// What a Kontakt 8 `native_ui` page's painter drew, in page points.
    Canvas(Vec<Paint>),
}

/// A [`Kind::Shape`]'s outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// A rectangle with rounded corners of `radius` points.
    Rect { radius: f64 },
    /// An arc of a ring `thickness` points wide, inside the widget's bounds,
    /// from `start` degrees (0 at 3 o'clock, clockwise) over `sweep`.
    Ring {
        thickness: f64,
        start: f64,
        sweep: f64,
    },
}

impl Kind {
    /// Whether this kind can contain widgets.
    pub fn is_container(&self) -> bool {
        matches!(self, Self::Panel)
    }
}

/// A numeric value range, in the control's own units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
    pub default: f64,
    /// Smallest change; `None` for continuous.
    pub step: Option<f64>,
}

/// How a numeric value reads as text.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// Shown value = raw value / `ratio` (KSP display ratio); a value
    /// edit's 0 shows note names.
    pub ratio: f64,
    /// Unit suffix or Falcon `Unit` name; empty for none.
    pub unit: String,
    /// Readout formatting policy.
    pub format: ValueFormat,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            unit: String::new(),
            format: ValueFormat::FixedPrecision,
        }
    }
}

impl Display {
    /// Formats a readout only; the raw value and edit conversion remain unchanged.
    pub fn format_value(&self, value: f64, integer: bool) -> String {
        if self.format == ValueFormat::CompactTrimmed {
            // Port v1 4bffbb18:src/ui/uvi_instrument.rs::unit_text/number_text.
            let number = |value: f64, integer: bool| {
                if integer {
                    format!("{value:.0}")
                } else {
                    format!("{value:.3}")
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_owned()
                }
            };
            let (value, suffix) = match self.unit.as_str() {
                "Percent" => (value, "%"),
                "PercentNormalized" => (value * 100., "%"),
                "Seconds" if value.abs() < 1. => (value * 1000., "ms"),
                "Seconds" => (value, "s"),
                "MilliSeconds" if value.abs() > 1000. => (value / 1000., "s"),
                "MilliSeconds" => (value, "ms"),
                "Hertz" if value.abs() > 1000. => (value / 1000., "kHz"),
                "Hertz" => (value, "Hz"),
                "Decibels" => (value, "dB"),
                "LinearGain" if value <= 0. => return "-inf dB".into(),
                "LinearGain" => (20. * value.log10(), "dB"),
                "Pan" if value == 0. => return "Center".into(),
                "SemiTones" => (value, "st"),
                // The reference: "switches to semitones above 100 cents".
                "Cents" if value.abs() > 100. => (value / 100., "st"),
                "Cents" => (value, "ct"),
                "Megabyte" => (value, "MB"),
                // "display int value into MIDI value (ex C3)": middle C is C3.
                "MidiKey" => {
                    const NAMES: [&str; 12] = [
                        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                    ];
                    let key = value.round() as i32;
                    return format!(
                        "{}{}",
                        NAMES
                            .get(key.rem_euclid(12) as usize)
                            .copied()
                            .unwrap_or_default(),
                        key.div_euclid(12) - 2
                    );
                }
                // shortcut: the filter's own formatting is not calibrated against native readouts.
                "" | "Generic" | "UviFilter" | "Pan" => {
                    return number(value, integer);
                }
                suffix => (value, suffix),
            };
            return format!("{} {suffix}", number(value + 0., false));
        }
        let value = value / if self.ratio == 0. { 1. } else { self.ratio };
        let number = if value.fract() == 0. {
            format!("{value}")
        } else {
            format!("{value:.2}")
        };
        format!("{number} {}", self.unit).trim().to_owned()
    }
}

/// Which files a file selector lists (KSP `FILE_TYPE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Files {
    /// No `FILE_TYPE`: Kontakt 8 lists MIDI files, drawn with the audio icon
    /// (oracle probe fs3.ksp).
    #[default]
    Any,
    Audio,
    Midi,
    /// Saved arrays / script data (KSP `$NI_FILE_TYPE_ARRAY`).
    Data,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuItem {
    pub text: String,
    pub value: i32,
    pub visible: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuNode {
    pub label: String,
    pub item_index: Option<usize>,
    pub children: Vec<MenuNode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuHierarchyError {
    EmptySegment,
    DuplicatePath,
    LeafIsAlsoBranch,
    RendererLimit,
}

/// Falcon's documented slash paths, preserving original item indices/order.
/// Ambiguous native cases stay unmodeled rather than choosing a value silently.
pub fn menu_hierarchy(items: &[MenuItem]) -> Result<Vec<MenuNode>, MenuHierarchyError> {
    struct Node {
        label: String,
        item_index: Option<usize>,
        children: Vec<usize>,
    }
    fn materialize(at: usize, nodes: &mut [Node]) -> Option<MenuNode> {
        let node = nodes.get_mut(at)?;
        let label = std::mem::take(&mut node.label);
        let item_index = node.item_index;
        let children = std::mem::take(&mut node.children)
            .into_iter()
            .filter_map(|child| materialize(child, nodes))
            .collect();
        Some(MenuNode {
            label,
            item_index,
            children,
        })
    }
    let mut roots = Vec::new();
    let mut nodes = Vec::<Node>::new();
    let mut lookup = std::collections::HashMap::<(Option<usize>, &str), usize>::new();
    for (item_index, item) in items.iter().enumerate().filter(|(_, item)| item.visible) {
        let parts: Vec<_> = item.text.split('/').take(33).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(MenuHierarchyError::EmptySegment);
        }
        if parts.len() > 32 {
            return Err(MenuHierarchyError::RendererLimit);
        }
        let mut parent = None;
        for (depth, label) in parts.iter().enumerate() {
            let at = match lookup.get(&(parent, *label)) {
                Some(&at) => at,
                None => {
                    if nodes.len() >= 16_384 {
                        return Err(MenuHierarchyError::RendererLimit);
                    }
                    let at = nodes.len();
                    nodes.push(Node {
                        label: (*label).into(),
                        item_index: None,
                        children: Vec::new(),
                    });
                    lookup.insert((parent, *label), at);
                    if let Some(parent) = parent.and_then(|p| nodes.get_mut(p)) {
                        parent.children.push(at);
                    } else {
                        roots.push(at);
                    }
                    at
                }
            };
            let Some(node) = nodes.get_mut(at) else {
                break;
            };
            if depth + 1 == parts.len() {
                if node.item_index.is_some() {
                    return Err(MenuHierarchyError::DuplicatePath);
                }
                if !node.children.is_empty() {
                    return Err(MenuHierarchyError::LeafIsAlsoBranch);
                }
                node.item_index = Some(item_index);
                break;
            }
            if node.item_index.is_some() {
                return Err(MenuHierarchyError::LeafIsAlsoBranch);
            }
            parent = Some(at);
        }
    }
    Ok(roots
        .into_iter()
        .filter_map(|at| materialize(at, &mut nodes))
        .collect())
}

/// What a widget shows and edits.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Binding {
    /// Nothing: panels, static labels and images.
    #[default]
    None,
    /// A host-visible control in the headless control service.
    Control(ControlId),
    /// Script memory with no host control behind it (a `ui_table`'s cells, a
    /// label written at runtime), by script slot and variable name.
    Variable { script: u8, name: String },
    /// Direct binding to a format-neutral IR parameter (zero script VM overhead).
    Param(ParamBinding),
    /// A level meter's signal: an output bus, or the instrument when `None`.
    Meter { bus: Option<u32>, channel: u8 },
}

/// A bitmap a widget is drawn with, and what it is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageUse {
    pub asset: AssetRef,
    pub role: Role,
    /// A fixed frame (KSP `PICTURE_STATE`); `None` lets the value choose.
    pub frame: Option<u32>,
    /// Which of the widget's parts it draws: an XY pad cursor's index; 0
    /// for anything else.
    pub part: u32,
    /// Source-pixel `[x, y]` inset inside the widget bounds before drawing.
    pub inset: [f32; 2],
    /// How this image selects its active strip frame.
    pub strip_mode: StripMode,
}

impl ImageUse {
    pub const fn new(asset: AssetRef, role: Role) -> Self {
        Self {
            asset,
            role,
            frame: None,
            part: 0,
            inset: [0.0, 0.0],
            strip_mode: StripMode::Value,
        }
    }
}

/// What an image does for its widget; decides what [`Presentation::Kontra`] keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// Static artwork that is part of the picture (a panel or label
    /// background, a logo). Kept in both presentations.
    Background,
    /// Animation frames of a control's value (knob/slider/button/switch strip).
    Strip,
    /// A separately drawn moving part: slider handle, XY cursor, meter fill.
    Handle,
    Pressed,
    Hover,
    HoverPressed,
}

impl Role {
    /// Whether vector controls replace this image.
    pub fn replaced_by_vector(self) -> bool {
        !matches!(self, Self::Background)
    }
}

/// A file the interface draws with, loaded on demand by the renderer.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    /// Library-relative resource path, e.g. `"pictures/knob_big.png"`.
    pub path: String,
    pub kind: AssetKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AssetKind {
    Image(ImageMeta),
    /// A bitmap font (KSP `get_font_id` picture font).
    BitmapFont,
    TrueTypeFont,
}

/// Layout of an image, from source metadata (KSP picture `.txt`, Lua args).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageMeta {
    /// Animation frames stacked in the image; 1 for a still.
    pub frames: u32,
    /// Direction the frames are stacked in.
    pub axis: Orientation,
    pub alpha: bool,
    /// One frame's pixel size, from the image file's header. A control
    /// showing the image takes this size along each axis it does not
    /// stretch, whatever size the source gave it (Kontakt's rule).
    pub size: Option<Size>,
    /// Stretches to the control across, down (Kontakt's sidecar
    /// "Horizontal"/"Vertical Resizable").
    pub stretch: [bool; 2],
    /// Nine-slice fixed edges along the axes it stretches.
    pub margins: Margins,
}

impl Default for ImageMeta {
    fn default() -> Self {
        Self {
            frames: 1,
            axis: Orientation::Vertical,
            alpha: true,
            size: None,
            stretch: [false; 2],
            margins: Margins::default(),
        }
    }
}

/// Kontakt switch/button/dropdown strips: off, on, off pressed, on pressed,
/// off hover, on hover (NI Kontakt 4 Player Developer Guide §5.4.2).
/// A short legacy strip retains its off/on fallback; a held pointer wins over hover.
pub fn kontakt_picture_frame(on: bool, held: bool, hovered: bool, frames: usize) -> usize {
    let state = usize::from(on)
        + if frames >= 6 {
            if held {
                2
            } else if hovered {
                4
            } else {
                0
            }
        } else {
            0
        };
    state.min(frames.saturating_sub(1))
}

/// Fixed edges of a stretchable image, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Margins {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font: Font,
    /// Pixel size; `None` for the font's own.
    pub size: Option<f32>,
    /// Variable-font weight axis (`wght`, e.g. `600.0`, `700.0`); `None` uses the face default.
    pub weight: Option<f32>,
    /// Optional per-style horizontal `[left, right]` inset in source pixels, overriding [`TextPolicy`].
    pub inset: Option<[f32; 2]>,
    pub color: Rgba,
    pub align: Align,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font: Font::Default,
            size: None,
            weight: None,
            inset: None,
            color: Rgba::default(),
            align: Align::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Font {
    /// The renderer's interface face.
    Default,
    /// A source's numbered stock font (KSP `$CONTROL_PAR_FONT_TYPE` 0..=25).
    Stock(i32),
    /// A font named by the source (Lua font name).
    Named(String),
    /// A bitmap font asset. [`Presentation::Kontra`] draws [`Font::Default`] instead.
    Bitmap(AssetRef),
    File(AssetRef),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Opaque, from `0xRRGGBB`.
    pub const fn rgb(packed: u32) -> Self {
        Self {
            r: (packed >> 16) as u8,
            g: (packed >> 8) as u8,
            b: packed as u8,
            a: 255,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    fn valid(self) -> bool {
        self.width.is_finite() && self.height.is_finite() && self.width >= 0. && self.height >= 0.
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x: x as f64,
            y: y as f64,
            width: width as f64,
            height: height as f64,
        }
    }

    fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && Size {
                width: self.width,
                height: self.height,
            }
            .valid()
            && (self.x + self.width).is_finite()
            && (self.y + self.height).is_finite()
    }
}

/// Source UI meaning with no IR representation, e.g. an unknown control property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub widget: Option<WidgetRef>,
    /// Source feature, e.g. `"$CONTROL_PAR_MOUSE_BEHAVIOUR"`.
    pub feature: String,
    /// Authored value, verbatim.
    pub value: String,
}

/// How a library's interface is drawn: its two looks. Both lay out the
/// same controls in the same places on the same pages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Presentation {
    /// The library's own pictures, as its sampler draws them.
    #[default]
    Original,
    /// KONTRA's own controls on the library's wallpaper and
    /// [`Role::Background`] art.
    Kontra,
}

impl Presentation {
    pub const ALL: [Self; 2] = [Self::Original, Self::Kontra];
    pub const LABELS: [&str; 2] = ["Original", "KONTRA"];
}

// ---------------------------------------------------------------- queries

/// Documented Kontakt back-to-front type priority within one Z layer.
/// Image is the shared-IR representation of a picture label; panels paint no
/// pixels and retain their position before ordinary controls.
fn kontakt_paint_rank(kind: &Kind) -> u8 {
    match kind {
        Kind::Panel => 0,
        Kind::FileSelector { .. } => 1,
        Kind::Waveform => 2,
        Kind::Wavetable { .. } => 3,
        Kind::LevelMeter { .. } => 4,
        Kind::Label | Kind::Image | Kind::Shape(_) | Kind::Canvas(_) => 5,
        Kind::Knob { .. } => 6,
        Kind::Slider { .. } => 7,
        Kind::Switch => 8,
        Kind::Button { .. } => 9,
        Kind::ValueEdit { .. } => 10,
        Kind::Menu { .. } => 11,
        Kind::Table { .. } => 12,
        Kind::Xy { .. } => 13,
        Kind::TextEdit => 14,
        Kind::MouseArea { .. } => 15,
    }
}

impl Interface {
    /// Which assets `presentation` draws, indexed like [`Self::assets`].
    /// Everything else need not be decoded, or may be released.
    pub fn needed_assets(&self, presentation: Presentation) -> Vec<bool> {
        let mut keep = vec![false; self.assets.len()];
        let mut mark = |a: AssetRef| {
            if let Some(k) = keep.get_mut(a.0) {
                *k = true;
            }
        };
        for a in self
            .pages
            .iter()
            .filter_map(|p| p.background.image)
            .chain(self.icon)
        {
            mark(a);
        }
        let vector = presentation == Presentation::Kontra;
        for w in &self.widgets {
            for i in &w.images {
                if !(vector && i.role.replaced_by_vector() && !w.label_in_image()) {
                    mark(i.asset);
                }
            }
        }
        for s in &self.styles {
            match s.font {
                Font::File(a) => mark(a),
                Font::Bitmap(a) if !vector => mark(a),
                _ => {}
            }
        }
        keep
    }

    /// Whether `widget` and every panel containing it are shown.
    pub fn visible(&self, widget: WidgetRef) -> bool {
        let mut at = Some(widget);
        // Bounded so a malformed (unvalidated) cycle cannot hang the caller.
        for _ in 0..=self.widgets.len() {
            let Some(w) = at.and_then(|w| self.widgets.get(w.0)) else {
                return true;
            };
            if w.hidden {
                return false;
            }
            at = w.parent;
        }
        false
    }

    /// `widget`'s rectangle in page coordinates. Uses [`Widget::rect`] as
    /// is: resolve [`Placement::Grid`] and [`Widget::auto_size`] first.
    pub fn page_rect(&self, widget: WidgetRef) -> Rect {
        let Some(w) = self.widgets.get(widget.0) else {
            return Rect::default();
        };
        let (mut rect, mut parent) = (w.rect, w.parent);
        for _ in 0..self.widgets.len() {
            let Some(p) = parent else { break };
            let Some(p) = self.widgets.get(p.0) else {
                break;
            };
            rect.x += p.rect.x - f64::from(p.viewport.map_or(0, |v| v[0]));
            rect.y += p.rect.y - f64::from(p.viewport.map_or(0, |v| v[1]));
            parent = p.parent;
        }
        rect
    }

    /// `page`'s widgets back to front. Kontakt layers order by widget type,
    /// then declaration order (KSP Control Parameters, `$CONTROL_PAR_Z_LAYER`).
    /// Kontakt uses global layers; native graphs order siblings and draw each
    /// panel's contents straight after the panel.
    /// Requires a [validated](Self::validate) interface.
    pub fn draw_order(&self, page: PageRef) -> Vec<WidgetRef> {
        if let Some(order) = &self.paint_order {
            return order
                .iter()
                .copied()
                .filter(|w| self.widgets.get(w.0).is_some_and(|w| w.page == page))
                .collect();
        }
        if self.z_order == ZOrder::Global {
            let mut out: Vec<_> = self
                .widgets
                .iter()
                .enumerate()
                .filter(|(_, w)| w.page == page)
                .map(|(n, _)| WidgetRef(n))
                .collect();
            out.sort_by_key(|n| {
                let w = self.widgets.get(n.0);
                (w.map(|w| (w.z, kontakt_paint_rank(&w.kind))), n.0)
            });
            return out;
        }
        let mut children: Vec<Vec<WidgetRef>> = vec![Vec::new(); self.widgets.len() + 1];
        let root = self.widgets.len();
        for (n, w) in self.widgets.iter().enumerate() {
            if w.page == page
                && let Some(siblings) = children.get_mut(w.parent.map_or(root, |p| p.0))
            {
                siblings.push(WidgetRef(n));
            }
        }
        for c in &mut children {
            c.sort_by_key(|w| (self.widgets.get(w.0).map(|w| w.z), w.0));
        }
        let mut out = Vec::with_capacity(self.widgets.len());
        let mut stack: Vec<WidgetRef> = children
            .get(root)
            .map_or_else(Vec::new, |c| c.iter().rev().copied().collect());
        while let Some(w) = stack.pop() {
            out.push(w);
            stack.extend(children.get(w.0).into_iter().flatten().rev());
        }
        out
    }

    /// Checks every reference, panel nesting and asset kinds.
    pub fn validate(&self) -> Result<(), Error> {
        let asset = |a: AssetRef, image: bool| match self.assets.get(a.0) {
            None => Err(Error::MissingAsset(a)),
            Some(x) if matches!(x.kind, AssetKind::Image(_)) != image => Err(Error::AssetKind(a)),
            Some(_) => Ok(()),
        };
        for (n, p) in self.pages.iter().enumerate() {
            if !p.size.valid() {
                return Err(Error::PageGeometry(PageRef(n)));
            }
            if let Some(a) = p.background.image {
                asset(a, true)?;
            }
        }
        for (n, a) in self.assets.iter().enumerate() {
            if let AssetKind::Image(m) = a.kind
                && m.size.is_some_and(|s| !s.valid())
            {
                return Err(Error::AssetGeometry(AssetRef(n)));
            }
        }
        if let Some(order) = &self.paint_order {
            let mut seen = vec![false; self.widgets.len()];
            for &w in order {
                let at = seen.get_mut(w.0).ok_or(Error::MissingWidget(w))?;
                if *at {
                    return Err(Error::DuplicatePaintWidget(w));
                }
                *at = true;
            }
        }
        for s in &self.styles {
            if let Font::Bitmap(a) | Font::File(a) = s.font {
                asset(a, false)?;
            }
        }
        if let Some(a) = self.icon {
            asset(a, true)?;
        }
        for (n, w) in self.widgets.iter().enumerate() {
            let at = WidgetRef(n);
            if !w.rect.valid() {
                return Err(Error::WidgetGeometry(at));
            }
            if w.page.0 >= self.pages.len() {
                return Err(Error::MissingPage(at));
            }
            if w.style.is_some_and(|s| s.0 >= self.styles.len()) {
                return Err(Error::MissingStyle(at));
            }
            for i in &w.images {
                asset(i.asset, true)?;
            }
            if let Some(p) = w.parent {
                let parent = self.widgets.get(p.0).ok_or(Error::MissingParent(at))?;
                if !parent.kind.is_container() {
                    return Err(Error::ParentNotPanel(at));
                }
                if parent.page != w.page {
                    return Err(Error::ParentOnOtherPage(at));
                }
            }
            // A chain longer than the widget count must revisit a widget.
            let mut up = w.parent;
            for _ in 0..self.widgets.len() {
                match up {
                    Some(p) => {
                        up = self
                            .widgets
                            .get(p.0)
                            .ok_or(Error::MissingParent(at))?
                            .parent
                    }
                    None => break,
                }
            }
            if up.is_some() {
                return Err(Error::ParentCycle(at));
            }
            if !self.page_rect(at).valid() {
                return Err(Error::WidgetGeometry(at));
            }
        }
        if let Some(w) = self
            .unsupported
            .iter()
            .filter_map(|u| u.widget)
            .find(|w| w.0 >= self.widgets.len())
        {
            return Err(Error::MissingWidget(w));
        }
        Ok(())
    }
}

/// Why an [`Interface`] is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    PageGeometry(PageRef),
    WidgetGeometry(WidgetRef),
    AssetGeometry(AssetRef),
    DuplicatePaintWidget(WidgetRef),
    MissingAsset(AssetRef),
    /// An image slot names a font, or a font slot names an image.
    AssetKind(AssetRef),
    MissingPage(WidgetRef),
    MissingStyle(WidgetRef),
    MissingParent(WidgetRef),
    /// An [`Unsupported`] entry names a widget that does not exist.
    MissingWidget(WidgetRef),
    ParentNotPanel(WidgetRef),
    ParentOnOtherPage(WidgetRef),
    ParentCycle(WidgetRef),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PageGeometry(p) => write!(f, "page {} has invalid geometry", p.0),
            Self::WidgetGeometry(w) => write!(f, "widget {} has invalid geometry", w.0),
            Self::AssetGeometry(a) => write!(f, "asset {} has invalid geometry", a.0),
            Self::DuplicatePaintWidget(w) => write!(f, "widget {} repeats in paint order", w.0),
            Self::MissingAsset(a) => write!(f, "asset {} does not exist", a.0),
            Self::AssetKind(a) => write!(f, "asset {} is the wrong kind for its use", a.0),
            Self::MissingPage(w) => write!(f, "widget {} is on a page that does not exist", w.0),
            Self::MissingStyle(w) => {
                write!(f, "widget {} names a text style that does not exist", w.0)
            }
            Self::MissingParent(w) => {
                write!(f, "widget {} names a widget that does not exist", w.0)
            }
            Self::MissingWidget(w) => write!(f, "widget {} does not exist", w.0),
            Self::ParentNotPanel(w) => write!(f, "widget {}'s parent is not a panel", w.0),
            Self::ParentOnOtherPage(w) => write!(f, "widget {}'s parent is on another page", w.0),
            Self::ParentCycle(w) => write!(f, "widget {} is inside itself", w.0),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    #[test]
    fn kontakt_picture_strip_uses_all_six_responsive_states() {
        assert_eq!(
            [false, true]
                .into_iter()
                .flat_map(|on| {
                    [(false, false), (true, true), (false, true)]
                        .map(|(held, hover)| kontakt_picture_frame(on, held, hover, 6))
                })
                .collect::<Vec<_>>(),
            [0, 2, 4, 1, 3, 5]
        );
        for frames in 0..=2 {
            assert_eq!(
                kontakt_picture_frame(true, true, true, frames),
                frames.saturating_sub(1)
            );
            assert_eq!(kontakt_picture_frame(false, false, true, frames), 0);
        }
    }
    use super::*;

    #[test]
    fn value_format_policies_format_fixed_and_compact_readouts() {
        let display = Display {
            ratio: 100.,
            unit: "dB".into(),
            format: ValueFormat::FixedPrecision,
        };
        assert_eq!(display.format_value(123., true), "1.23 dB");
        assert_eq!(display.format_value(123., false), "1.23 dB");
        assert_eq!(
            Display {
                ratio: 0.,
                unit: String::new(),
                format: ValueFormat::FixedPrecision,
            }
            .format_value(1.125, false),
            "1.12"
        );
        let compact = Display {
            format: ValueFormat::CompactTrimmed,
            ..Display::default()
        };
        assert_eq!(compact.format_value(1.125, false), "1.125");
        assert_eq!(compact.format_value(1.6, true), "2");
    }

    #[test]
    fn falcon_units_read_as_the_reference_lists_them() {
        let at = |unit: &str, v: f64| {
            Display {
                ratio: 0.,
                unit: unit.into(),
                format: ValueFormat::CompactTrimmed,
            }
            .format_value(v, false)
        };
        assert_eq!(at("Seconds", 0.25), "250 ms");
        assert_eq!(at("Hertz", 2500.), "2.5 kHz");
        assert_eq!(at("Cents", 50.), "50 ct");
        assert_eq!(at("Cents", 250.), "2.5 st");
        assert_eq!(at("Megabyte", 12.), "12 MB");
        assert_eq!(at("MidiKey", 60.), "C3");
        assert_eq!(at("MidiKey", 21.), "A-1");
    }

    #[test]
    fn binding_uses_tiny_and_fixed_authored_widget_ranges() {
        let binding = ParamBinding::with_map(
            ir::Scope::Instrument,
            ir::ParamAddr::Volume,
            ParamMap::Linear { min: 0., max: 1. },
        );
        let tiny = Some(Range {
            min: 0.,
            max: 1e-16,
            ..Range::default()
        });
        assert_eq!(binding.to_physical(1e-16, tiny), 1.);
        assert_eq!(binding.from_physical(0.5, tiny), 5e-17);
        let fixed = Some(Range {
            min: 0.25,
            max: 0.25,
            ..Range::default()
        });
        assert_eq!(binding.to_physical(0.25, fixed), 0.);
        assert_eq!(binding.from_physical(0.75, fixed), 0.25);
        assert_eq!(binding.to_physical(0.25, None), 0.25);
    }

    #[test]
    fn binding_returns_declared_physical_endpoints_exactly() {
        for (map, end) in [
            (ParamMap::Linear { min: 1e20, max: 1. }, 1.),
            (
                ParamMap::Power {
                    min: 1e20,
                    max: 1.,
                    exponent: 2.,
                },
                1.,
            ),
            (
                ParamMap::Exponential {
                    min: 1e20,
                    max: -1.,
                },
                -1.,
            ),
            (ParamMap::Exponential { min: 1e20, max: 1. }, 1.),
        ] {
            let binding = ParamBinding::with_map(ir::Scope::Instrument, ir::ParamAddr::Volume, map);
            assert_eq!(binding.to_physical(0., None), 1e20);
            assert_eq!(binding.to_physical(1., None), end);
        }
    }

    #[test]
    fn invalid_power_exponents_are_linear_in_both_directions() {
        for exponent in [-1.0, 0.0, f32::INFINITY, f32::NAN] {
            let binding = ParamBinding::with_map(
                ir::Scope::Instrument,
                ir::ParamAddr::Volume,
                ParamMap::Power {
                    min: 0.0,
                    max: 2.0,
                    exponent,
                },
            );
            assert_eq!(binding.to_physical(0.25, None), 0.5);
            assert_eq!(binding.from_physical(0.5, None), 0.25);
        }
    }

    #[test]
    fn inverse_respects_distinct_tiny_endpoints() {
        for map in [
            ParamMap::Linear {
                min: 0.0,
                max: 1e-16,
            },
            ParamMap::Exponential {
                min: 1e-17,
                max: 1e-16,
            },
            ParamMap::Power {
                min: 0.0,
                max: 1e-16,
                exponent: 2.0,
            },
        ] {
            let binding = ParamBinding::with_map(ir::Scope::Instrument, ir::ParamAddr::Volume, map);
            assert_eq!(
                binding.from_physical(binding.to_physical(1.0, None), None),
                1.0
            );
            assert_eq!(binding.from_physical(f32::NAN, None), 0.0);
        }
    }

    #[test]
    fn param_binding_maps_direct_linear_exponential_and_power_curves() {
        let range = Some(Range {
            min: 0.0,
            max: 100.0,
            default: 50.0,
            step: None,
        });
        let direct = ParamBinding::new(ir::Scope::Instrument, ir::ParamAddr::Volume);
        assert!((direct.to_physical(-6.0, range) - -6.0).abs() < 1e-6);
        assert!((direct.from_physical(-6.0, range) - -6.0).abs() < 1e-6);

        let linear = ParamBinding::with_map(
            ir::Scope::Instrument,
            ir::ParamAddr::Pan,
            ParamMap::Linear {
                min: -1.0,
                max: 1.0,
            },
        );
        assert!((linear.to_physical(50.0, range) - 0.0).abs() < 1e-6);
        assert!((linear.from_physical(0.5, range) - 75.0).abs() < 1e-5);

        let exp = ParamBinding::with_map(
            ir::Scope::Group(0),
            ir::ParamAddr::FilterCutoff { insert: 0 },
            ParamMap::Exponential {
                min: 20.0,
                max: 20_000.0,
            },
        );
        let mid_hz = exp.to_physical(50.0, range);
        assert!((mid_hz - (20.0_f32 * 20_000.0_f32).sqrt()).abs() < 1e-2);
        assert!((exp.from_physical(mid_hz, range) - 50.0).abs() < 1e-4);

        let power = ParamBinding::with_map(
            ir::Scope::Group(0),
            ir::ParamAddr::FilterResonance { insert: 0 },
            ParamMap::Power {
                min: 0.0,
                max: 1.0,
                exponent: 2.0,
            },
        );
        assert!((power.to_physical(50.0, range) - 0.25).abs() < 1e-6);
        assert!((power.from_physical(0.25, range) - 50.0).abs() < 1e-4);
    }

    fn image(path: &str, frames: u32) -> Asset {
        Asset {
            path: path.into(),
            kind: AssetKind::Image(ImageMeta {
                frames,
                ..ImageMeta::default()
            }),
        }
    }

    /// Wallpaper, a panel with a background picture holding a strip knob, and a loose label.
    fn sample() -> Interface {
        let page = PageRef(0);
        let mut panel = Widget::new("$panel", page, Rect::new(10, 20, 200, 100), Kind::Panel);
        panel
            .images
            .push(ImageUse::new(AssetRef(1), Role::Background));
        panel.z = 1;
        let mut knob = Widget::new(
            "$cutoff",
            page,
            Rect::new(5, 6, 40, 40),
            Kind::Knob {
                range: Range {
                    max: 1_000_000.0,
                    ..Range::default()
                },
                display: Display::default(),
            },
        );
        knob.parent = Some(WidgetRef(0));
        knob.binding = Binding::Control(ControlId(7));
        knob.images.push(ImageUse::new(AssetRef(2), Role::Strip));
        let label = Widget::new("$title", page, Rect::new(0, 0, 100, 20), Kind::Label);
        Interface {
            source: Source::Ksp { slot: 0 },
            z_order: ZOrder::Global,
            stock_skin: StockSkin::Kontakt,
            pages: vec![Page {
                name: "Main".into(),
                size: Size {
                    width: 633.0,
                    height: 400.0,
                },
                background: Background {
                    image: Some(AssetRef(0)),
                    ..Background::default()
                },

                ..Page::default()
            }],
            widgets: vec![panel, knob, label],
            assets: vec![
                image("wallpaper.png", 1),
                image("panel_bg.png", 1),
                image("knob.png", 101),
            ],
            ..Interface::default()
        }
    }

    #[test]
    fn kontra_keeps_wallpaper_and_backgrounds_only() {
        let ui = sample();
        ui.validate().unwrap();
        assert_eq!(ui.needed_assets(Presentation::Original), [true, true, true]);
        assert_eq!(ui.needed_assets(Presentation::Kontra), [true, true, false]);
    }

    #[test]
    fn ksp_child_z_layer_crosses_parent_boundaries() {
        let mut ui = sample();
        ui.widgets[1].z = 5;
        ui.widgets[2].z = 2;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [WidgetRef(0), WidgetRef(2), WidgetRef(1)]
        );
        ui.widgets[1].z = 2;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [WidgetRef(0), WidgetRef(2), WidgetRef(1)],
            "a label remains behind a knob within the same Kontakt layer"
        );
        ui.widgets[0].hidden = true;
        assert!(
            !ui.visible(WidgetRef(1)),
            "global layers retain inherited hiding"
        );
    }

    #[test]
    fn kontakt_layers_use_native_type_priority_before_declaration_order() {
        let mut ui = sample();
        ui.widgets = vec![
            Widget::new(
                "menu",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Menu { items: vec![] },
            ),
            Widget::new(
                "slider",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Slider {
                    range: Range::default(),
                    orientation: Orientation::Vertical,
                },
            ),
            Widget::new(
                "first-label",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Label,
            ),
            Widget::new(
                "second-label",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::Label,
            ),
            Widget::new(
                "meter",
                PageRef(0),
                Rect::new(0, 0, 20, 20),
                Kind::LevelMeter {
                    orientation: Orientation::Vertical,
                    channel: None,
                    bus: None,
                },
            ),
        ];
        // NI's documented top-to-bottom priority is Menu, Slider, Label, Meter.
        // The two labels retain their declaration-order tie break.
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(4),
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0)
            ]
        );
        ui.widgets[4].z = 1;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0),
                WidgetRef(4)
            ]
        );
        ui.z_order = ZOrder::Global;
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [
                WidgetRef(2),
                WidgetRef(3),
                WidgetRef(1),
                WidgetRef(0),
                WidgetRef(4)
            ]
        );
    }

    #[test]
    fn sibling_local_panels_draw_before_children_and_by_z() {
        let mut ui = sample();
        ui.z_order = ZOrder::SiblingLocal;
        // Label (z 0) before the panel (z 1); the knob straight after its panel.
        assert_eq!(
            ui.draw_order(PageRef(0)),
            [WidgetRef(2), WidgetRef(0), WidgetRef(1)]
        );
        assert_eq!(ui.page_rect(WidgetRef(1)), Rect::new(15, 26, 40, 40));
    }

    #[test]
    fn hidden_panel_hides_children() {
        let mut ui = sample();
        assert!(ui.visible(WidgetRef(1)));
        ui.widgets[0].hidden = true;
        assert!(!ui.visible(WidgetRef(1)));
    }

    #[test]
    fn rejects_bad_nesting() {
        let mut ui = sample();
        ui.widgets[2].parent = Some(WidgetRef(1));
        assert_eq!(ui.validate(), Err(Error::ParentNotPanel(WidgetRef(2))));
        let mut ui = sample();
        ui.widgets[0].parent = Some(WidgetRef(0));
        assert_eq!(ui.validate(), Err(Error::ParentCycle(WidgetRef(0))));
        let mut ui = sample();
        ui.styles.push(TextStyle {
            font: Font::Bitmap(AssetRef(2)),
            size: None,
            color: Rgba::rgb(0),
            align: Align::Left,
            ..TextStyle::default()
        });
        assert_eq!(ui.validate(), Err(Error::AssetKind(AssetRef(2))));
    }

    #[test]
    fn authored_order_preserves_ids_and_explicit_empty_traversal() {
        let mut ui = sample();
        let declarations = ui.widgets.clone();
        ui.paint_order = Some(vec![WidgetRef(2), WidgetRef(0), WidgetRef(1)]);
        assert_eq!(ui.validate(), Ok(()));
        assert_eq!(ui.draw_order(PageRef(0)), ui.paint_order.clone().unwrap());
        assert_eq!(ui.widgets, declarations);
        ui.paint_order = Some(vec![]);
        assert!(ui.draw_order(PageRef(0)).is_empty());
        ui.paint_order = Some(vec![WidgetRef(1), WidgetRef(1)]);
        assert_eq!(
            ui.validate(),
            Err(Error::DuplicatePaintWidget(WidgetRef(1)))
        );
        ui.paint_order = Some(vec![WidgetRef(3)]);
        assert_eq!(ui.validate(), Err(Error::MissingWidget(WidgetRef(3))));
    }

    #[test]
    fn fractional_geometry_is_finite_through_parent_resolution() {
        let mut ui = sample();
        ui.widgets[0].rect.x = 10.25;
        ui.widgets[1].rect.x = 5.125;
        assert_eq!(ui.validate(), Ok(()));
        assert_eq!(ui.page_rect(WidgetRef(1)).x, 15.375);
        ui.widgets[1].rect.width = -0.25;
        assert_eq!(ui.validate(), Err(Error::WidgetGeometry(WidgetRef(1))));
        ui.widgets[1].rect.width = f64::NAN;
        assert_eq!(ui.validate(), Err(Error::WidgetGeometry(WidgetRef(1))));
        ui.widgets[1].rect.width = 40.;
        ui.pages[0].size.height = f64::INFINITY;
        assert_eq!(ui.validate(), Err(Error::PageGeometry(PageRef(0))));
        ui.pages[0].size.height = 400.;
        ui.widgets[0].rect.x = f64::MAX;
        ui.widgets[1].rect.x = f64::MAX;
        assert_eq!(ui.validate(), Err(Error::WidgetGeometry(WidgetRef(1))));
        ui.widgets[0].rect.x = 10.;
        ui.widgets[1].rect.x = 5.;
        if let AssetKind::Image(meta) = &mut ui.assets[0].kind {
            meta.size = Some(Size {
                width: f64::NAN,
                height: 10.,
            });
        }
        assert_eq!(ui.validate(), Err(Error::AssetGeometry(AssetRef(0))));
    }
}

mod publication;
pub use publication::InterfacePatch;
