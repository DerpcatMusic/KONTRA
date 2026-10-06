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
    pub unsupported: Vec<Unsupported>,
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
    pub size: Size,
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
    pub rect: Rect,
    /// Stacking among siblings; higher draws later.
    pub z: i32,
    /// Hidden as a whole (`HIDE_WHOLE_CONTROL`, Lua `visible = false`).
    pub hidden: bool,
    /// Parts hidden while the control itself shows.
    pub hide: Parts,
    pub enabled: bool,
    pub kind: Kind,
    pub binding: Binding,
    /// Caption, button text or label text; lines split on `\n`.
    pub text: String,
    pub tooltip: String,
    /// Name the host shows for automation, when it differs from `text`.
    pub automation_name: Option<String>,
    pub style: Option<StyleRef>,
    /// Bitmaps the source draws this widget with, by role.
    pub images: Vec<ImageUse>,
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
            z: 0,
            hidden: false,
            hide: Parts::default(),
            enabled: true,
            kind,
            binding: Binding::None,
            text: String::new(),
            tooltip: String::new(),
            automation_name: None,
            style: None,
            images: Vec::new(),
        }
    }

    /// The image this widget uses in `role`, if any.
    pub fn image(&self, role: Role) -> Option<AssetRef> {
        self.images.iter().find(|i| i.role == role).map(|i| i.asset)
    }
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
    Knob { range: Range, display: Display },
    Slider { range: Range, orientation: Orientation },
    /// A latching on/off button (KSP `ui_button`, Lua `OnOffButton`) or a
    /// momentary one (Lua `Button`).
    Button { momentary: bool },
    /// KSP `ui_switch`: latching, drawn as a switch.
    Switch,
    Menu { items: Vec<MenuItem> },
    Label,
    ValueEdit { range: Range, display: Display },
    Table { columns: u32, range: Range, bipolar: bool },
    Xy { cursors: u32 },
    Waveform,
    Wavetable,
    LevelMeter { orientation: Orientation },
    FileSelector,
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
        Self { ratio: 1.0, unit: String::new() }
    }
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
}

/// Layout of an image, from source metadata (KSP picture `.txt`, Lua args).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageMeta {
    /// Animation frames stacked in the image; 1 for a still.
    pub frames: u32,
    /// Direction the frames are stacked in.
    pub axis: Orientation,
    pub alpha: bool,
    /// Nine-slice margins when the source stretches the image.
    pub stretch: Option<Margins>,
}

impl Default for ImageMeta {
    fn default() -> Self {
        Self { frames: 1, axis: Orientation::Vertical, alpha: true, stretch: None }
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
        Self { r: (packed >> 16) as u8, g: (packed >> 8) as u8, b: packed as u8, a: 255 }
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
        Self { x, y, width, height }
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
        for page in &self.pages {
            if let Some(a) = page.background.image {
                mark(a);
            }
        }
        let vector = presentation == Presentation::Vector;
        for w in &self.widgets {
            for i in &w.images {
                if !(vector && i.role.replaced_by_vector()) {
                    mark(i.asset);
                }
            }
        }
        if !vector {
            for s in &self.styles {
                if let Font::Bitmap(a) = s.font {
                    mark(a);
                }
            }
        }
        keep
    }

    /// Whether `widget` and every panel containing it are shown.
    pub fn visible(&self, widget: WidgetRef) -> bool {
        let mut at = Some(widget);
        // Bounded so a malformed (unvalidated) cycle cannot hang the caller.
        for _ in 0..=self.widgets.len() {
            let Some(w) = at.and_then(|w| self.widgets.get(w.0)) else { return true };
            if w.hidden {
                return false;
            }
            at = w.parent;
        }
        false
    }

    /// `widget`'s rectangle in page coordinates.
    pub fn page_rect(&self, widget: WidgetRef) -> Rect {
        let mut rect = self.widgets[widget.0].rect;
        let mut parent = self.widgets[widget.0].parent;
        for _ in 0..self.widgets.len() {
            let Some(p) = parent else { break };
            let p = &self.widgets[p.0];
            rect.x += p.rect.x;
            rect.y += p.rect.y;
            parent = p.parent;
        }
        rect
    }

    /// `page`'s widgets back to front: siblings by ([`Widget::z`], declaration
    /// order), each panel's contents straight after the panel.
    /// Requires a [validated](Self::validate) interface.
    pub fn draw_order(&self, page: PageRef) -> Vec<WidgetRef> {
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
            if let Font::Bitmap(a) = s.font {
                asset(a, false)?;
            }
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
        if let Some(w) = self.unsupported.iter().filter_map(|u| u.widget).find(|w| w.0 >= self.widgets.len()) {
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
            Self::MissingStyle(w) => write!(f, "widget {} names a text style that does not exist", w.0),
            Self::MissingParent(w) => write!(f, "widget {} names a widget that does not exist", w.0),
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
        Asset { path: path.into(), kind: AssetKind::Image(ImageMeta { frames, ..ImageMeta::default() }) }
    }

    /// Wallpaper, a panel with a background picture holding a strip knob, and a loose label.
    fn sample() -> Interface {
        let page = PageRef(0);
        let mut panel = Widget::new("$panel", page, Rect::new(10, 20, 200, 100), Kind::Panel);
        panel.images.push(ImageUse { asset: AssetRef(1), role: Role::Background });
        panel.z = 1;
        let mut knob = Widget::new(
            "$cutoff",
            page,
            Rect::new(5, 6, 40, 40),
            Kind::Knob { range: Range { max: 1_000_000.0, ..Range::default() }, display: Display::default() },
        );
        knob.parent = Some(WidgetRef(0));
        knob.binding = Binding::Control(ControlId(7));
        knob.images.push(ImageUse { asset: AssetRef(2), role: Role::Strip });
        let label = Widget::new("$title", page, Rect::new(0, 0, 100, 20), Kind::Label);
        Interface {
            source: Source::Ksp { slot: 0 },
            pages: vec![Page {
                name: "Main".into(),
                size: Size { width: 633, height: 400 },
                background: Background { image: Some(AssetRef(0)), ..Background::default() },
            }],
            widgets: vec![panel, knob, label],
            assets: vec![image("wallpaper.png", 1), image("panel_bg.png", 1), image("knob.png", 101)],
            styles: vec![],
            unsupported: vec![],
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
    fn panels_draw_before_children_and_by_z() {
        let ui = sample();
        // Label (z 0) before the panel (z 1); the knob straight after its panel.
        assert_eq!(ui.draw_order(PageRef(0)), [WidgetRef(2), WidgetRef(0), WidgetRef(1)]);
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
        ui.styles.push(TextStyle { font: Font::Bitmap(AssetRef(2)), size: None, color: Rgba::rgb(0), align: Align::Left });
        assert_eq!(ui.validate(), Err(Error::AssetKind(AssetRef(2))));
    }
}
