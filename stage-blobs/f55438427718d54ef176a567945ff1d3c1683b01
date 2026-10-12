//! Format-neutral description of a library's instrument interface.
//!
//! Source frontends (KSP `on init`, Creator Tools performance views, Falcon
//! Lua) emit an [`Interface`]; the product UI renders it with either the
//! library's own bitmaps or its own vector controls ([`Presentation`]).
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
    pub pages: Vec<Page>,
    /// Declaration order; ties in [`Widget::z`] draw in this order.
    pub widgets: Vec<Widget>,
    pub assets: Vec<Asset>,
    pub styles: Vec<TextStyle>,
    /// The instrument's icon in the host's rack header (KSP `$INST_ICON_ID`).
    pub icon: Option<AssetRef>,
    /// The source hides the instrument icon (KSP `$INST_ICON_ID` `HIDE`).
    pub icon_hidden: bool,
    /// A legacy authored NativeUI entry point, consumed by the editor frontend.
    pub native_ui: Option<NativeUi>,
    pub unsupported: Vec<Unsupported>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeUi {
    pub entry: String,
}

/// Which frontend produced the interface.
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
    /// The selected wallpaper strip frame.
    pub frame: u32,
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
    pub value_y: Option<i32>,
    /// Shown instead of the formatted value (KSP knob `LABEL`).
    pub value_text: Option<String>,
    pub tooltip: String,
    pub automation: Automation,
    /// How a drag changes the value; `None` for the renderer's default.
    pub drag: Option<Drag>,
    /// Colours the source sets on its stock drawing; unset ones are the renderer's.
    pub colors: Colors,
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
    pub intercepts_mouse: bool,
    /// Clip children and offset their origin (UVI Viewport).
    pub viewport: Option<[i32; 2]>,
    /// Source response curve, e.g. UVI Exponential.
    pub mapper: Option<String>,
    /// MultiStateButton advances on click; Menu opens a choice list.
    pub menu_cycle: bool,
    /// Even coordinate index of the manually selected XY cursor.
    pub active_index: Option<i32>,
    /// Current source value for typed widgets; numeric controls use their service.
    pub value: Option<Value>,
    pub waveform: Option<Waveform>,
    pub meter: Option<MeterAddress>,
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
            value_y: None,
            value_text: None,
            tooltip: String::new(),
            automation: Automation::default(),
            drag: None,
            colors: Colors::default(),
            style: None,
            state_styles: [None; 6],
            images: Vec::new(),
            components: Vec::new(),
            initial_value: 0.0,
            opacity: 1.0,
            intercepts_mouse: true,
            viewport: None,
            mapper: None,
            menu_cycle: false,
            active_index: None,
            value: None,
            waveform: None,
            meter: None,
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
}

/// The drag gesture of a continuous control (KSP `MOUSE_BEHAVIOUR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drag {
    pub axis: Orientation,
    /// Source units: KSP's magnitude, larger is faster; travel is picture-relative.
    pub sensitivity: u32,
}

/// Colours of the stock drawing (KSP `*_COLOR` control parameters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Colors {
    pub background: Option<Rgba>,
    pub on: Option<Rgba>,
    pub off: Option<Rgba>,
    pub bar: Option<Rgba>,
    pub peak: Option<Rgba>,
    pub overload: Option<Rgba>,
    pub zero_line: Option<Rgba>,
    pub wave: Option<Rgba>,
    pub wave_cursor: Option<Rgba>,
    pub slice_markers: Option<Rgba>,
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
    /// `steps_shown`: value steps drawn as grid lines (KSP `set_table_steps_shown`).
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
    /// `view_mode`: the source's display mode, verbatim (KSP `WT_VIS_MODE`);
    /// `parallax`: 3D depth offset in pixels (KSP `PARALLAX_X`/`_Y`).
    Wavetable {
        view_mode: Option<i32>,
        parallax: [i32; 2],
    },
    LevelMeter {
        orientation: Orientation,
    },
    /// `base_path`: the folder it opens at (KSP `BASEPATH`); `files`: what it lists;
    /// `column_width`: list column width in pixels.
    FileSelector {
        base_path: Option<String>,
        files: Files,
        column_width: Option<u32>,
    },
    TextEdit,
    /// A static picture (Lua `Image`); KSP pictures on labels are [`Kind::Label`].
    Image,
    /// An invisible drag-and-drop / mouse target.
    MouseArea,
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
    /// Shown value = raw value / `ratio` (KSP display ratio).
    pub ratio: f64,
    /// Unit suffix, e.g. `"dB"`, `"%"`, `"Hz"`; empty for none.
    pub unit: String,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            unit: String::new(),
        }
    }
}

/// Which files a file selector lists (KSP `FILE_TYPE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Files {
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
    /// A level meter's signal: an output bus, or the instrument when `None`.
    Meter { bus: Option<u32>, channel: u8 },
}

/// A bitmap a widget is drawn with, and what it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageUse {
    pub asset: AssetRef,
    pub role: Role,
    /// A fixed frame (KSP `PICTURE_STATE`); `None` lets the value choose.
    pub frame: Option<u32>,
}

impl ImageUse {
    pub const fn new(asset: AssetRef, role: Role) -> Self {
        Self {
            asset,
            role,
            frame: None,
        }
    }
}

/// What an image does for its widget; decides what [`Presentation::Vector`] keeps.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    pub color: Rgba,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Font {
    /// The renderer's interface face.
    Default,
    /// A source's numbered stock font (KSP `$CONTROL_PAR_FONT_TYPE` 0..=25).
    Stock(i32),
    /// A font named by the source (Lua font name).
    Named(String),
    /// A bitmap font asset. [`Presentation::Vector`] draws [`Font::Default`] instead.
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
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

/// How controls are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Presentation {
    /// The library's own bitmaps.
    #[default]
    Bitmap,
    /// Wallpapers and [`Role::Background`] art only; controls are drawn as vectors.
    Vector,
}

// ---------------------------------------------------------------- queries

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
        let vector = presentation == Presentation::Vector;
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
        let mut rect = self.widgets[widget.0].rect;
        let mut parent = self.widgets[widget.0].parent;
        for _ in 0..self.widgets.len() {
            let Some(p) = parent else { break };
            let Some(p) = self.widgets.get(p.0) else {
                break;
            };
            rect.x = rect
                .x
                .saturating_add(p.rect.x)
                .saturating_sub(p.viewport.map_or(0, |v| v[0]));
            rect.y = rect
                .y
                .saturating_add(p.rect.y)
                .saturating_sub(p.viewport.map_or(0, |v| v[1]));
            parent = p.parent;
        }
        rect
    }

    /// `page`'s widgets back to front by ([`Widget::z`], declaration order).
    /// Kontakt uses global layers; native graphs order siblings and draw each
    /// panel's contents straight after the panel.
    /// Requires a [validated](Self::validate) interface.
    pub fn draw_order(&self, page: PageRef) -> Vec<WidgetRef> {
        if matches!(self.source, Source::Ksp { .. } | Source::PerformanceView) {
            let mut out: Vec<_> = self
                .widgets
                .iter()
                .enumerate()
                .filter(|(_, w)| w.page == page)
                .map(|(n, _)| WidgetRef(n))
                .collect();
            out.sort_by_key(|n| (self.widgets[n.0].z, n.0));
            return out;
        }
        let mut children: Vec<Vec<WidgetRef>> = vec![Vec::new(); self.widgets.len() + 1];
        let root = self.widgets.len();
        for (n, w) in self.widgets.iter().enumerate() {
            if w.page == page {
                children[w.parent.map_or(root, |p| p.0)].push(WidgetRef(n));
            }
        }
        for c in &mut children {
            c.sort_by_key(|w| (self.widgets[w.0].z, w.0));
        }
        let mut out = Vec::with_capacity(self.widgets.len());
        let mut stack: Vec<WidgetRef> = children[root].iter().rev().copied().collect();
        while let Some(w) = stack.pop() {
            out.push(w);
            stack.extend(children[w.0].iter().rev());
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
        for p in &self.pages {
            if let Some(a) = p.background.image {
                asset(a, true)?;
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
                    Some(p) => up = self.widgets[p.0].parent,
                    None => break,
                }
            }
            if up.is_some() {
                return Err(Error::ParentCycle(at));
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
    use super::*;

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
            pages: vec![Page {
                name: "Main".into(),
                size: Size {
                    width: 633,
                    height: 400,
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
    fn vector_keeps_wallpaper_and_backgrounds_only() {
        let ui = sample();
        ui.validate().unwrap();
        assert_eq!(ui.needed_assets(Presentation::Bitmap), [true, true, true]);
        assert_eq!(ui.needed_assets(Presentation::Vector), [true, true, false]);
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
            [WidgetRef(0), WidgetRef(1), WidgetRef(2)]
        );
        ui.widgets[0].hidden = true;
        assert!(
            !ui.visible(WidgetRef(1)),
            "global layers retain inherited hiding"
        );
    }

    #[test]
    fn lua_panels_draw_before_children_and_by_z() {
        let mut ui = sample();
        ui.source = Source::FalconLua;
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
        });
        assert_eq!(ui.validate(), Err(Error::AssetKind(AssetRef(2))));
    }
}

mod publication;
pub use publication::InterfacePatch;
