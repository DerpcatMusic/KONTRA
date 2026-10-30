//! A KSP script's performance view as a `kontra_ui_model` tree, built once
//! per load from the declarations and the control store as `on init` left it
//! (07-APP.md §5.1). The UI draws from the tree and needs nothing else from
//! the engine until a control moves.

use std::collections::HashMap;

use kontra_engine::{ControlLook, Engine, Page};
use kontra_ksp::{Cells, Control, ControlKind, FIRST_UI_ID, SYMBOL_BASE, Script};

use crate::perf_view::{View, ViewControl};

mod keybed;
pub mod lua;
mod picture;
mod widget;
use kontra_ui_model as ui;
pub use picture::{Library, meta as picture_meta};
use widget::{Decl, widget};

use crate::script::LOOK;

/// Kontakt's instrument width in pixels when the script sets none.
const DEFAULT_WIDTH: u32 = 633;
/// Classic Kontakt frame: the wallpaper starts under the 68 px header.
const HEADER: u32 = 68;

/// What the tree needs from a compiled script once its program is in the
/// plan: the declared controls and the names behind its symbols.
#[derive(Clone, Debug)]
pub struct Face {
    controls: Vec<(Control, Box<str>, Cells)>,
    symbols: Vec<Box<str>>,
    /// The performance view the script loads, if any.
    view: Option<View>,
}

impl Face {
    #[must_use]
    /// `data` holds the script's library files; a `<name>.nckp` among
    /// them is its performance view.
    pub fn of(script: &Script, data: &[(String, Vec<u8>)]) -> Self {
        let controls = script
            .controls
            .iter()
            .filter_map(|c| {
                let v = script.variables.get(c.variable as usize)?;
                Some((c.clone(), v.name.clone(), v.cells))
            })
            .collect();
        Self {
            controls,
            symbols: script.symbols.clone(),
            view: data
                .iter()
                .find(|(name, _)| name.ends_with(".nckp"))
                .and_then(|(_, bytes)| crate::perf_view::parse(bytes)),
        }
    }

    /// The UI id of the control declared as `name` (`$Volume`), any case.
    #[must_use]
    pub fn control_id(&self, name: &str) -> Option<i32> {
        let (control, ..) =
            (self.controls.iter()).find(|(_, n, _)| n.eq_ignore_ascii_case(name))?;
        Some(FIRST_UI_ID + i32::try_from(control.variable).ok()?)
    }

    /// The first text cell of the control declared as `name` (`@field`).
    #[must_use]
    pub fn text_cell(&self, name: &str) -> Option<u32> {
        self.controls.iter().find_map(|(_, n, cells)| match cells {
            Cells::Text { base, .. } if n.eq_ignore_ascii_case(name) => Some(*base),
            _ => None,
        })
    }

    /// The name of parameter id `par`: a symbol, or a look parameter the
    /// script never named (`script::look_par`).
    fn par_name(&self, par: i32) -> Option<&str> {
        let n = usize::try_from(par.checked_sub(SYMBOL_BASE)?).ok()?;
        match self.symbols.get(n) {
            Some(s) => Some(s),
            None => LOOK.get(n - self.symbols.len()).copied(),
        }
    }
}

/// What a [`Live`] reads of a running script: the engine itself, or a copy
/// of those reads made on the audio thread (`crate::view::LookFrame`).
pub trait Looks {
    fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>>;
    fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64>;
    /// Text cell `n` of script `slot` (an `@` control's variable).
    fn script_text_in(&self, slot: u8, n: usize) -> Option<&str>;
    fn page(&self, slot: u8) -> Option<&Page>;
    /// The bitmap font behind font id `id` (`get_font_id`), by picture name.
    fn font(&self, slot: u8, id: i32) -> Option<&str>;
    /// The keyboard's keys as the scripts left them, by note.
    fn keys(&self) -> &[kontra_engine::Key];
    /// Whether the keyboard shows only the keys scripts press.
    fn pressed_support(&self) -> bool;
}

impl Looks for Engine {
    fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>> {
        Engine::control_look(self, slot, id)
    }

    fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        Engine::script_cell_in(self, slot, n)
    }

    fn script_text_in(&self, slot: u8, n: usize) -> Option<&str> {
        Some(self.vm(slot)?.text(n)?.as_str())
    }

    fn page(&self, slot: u8) -> Option<&Page> {
        Engine::page(self, slot)
    }

    fn font(&self, slot: u8, id: i32) -> Option<&str> {
        Engine::font(self, slot, id)
    }

    fn keys(&self) -> &[kontra_engine::Key] {
        Engine::keys(self)
    }

    fn pressed_support(&self) -> bool {
        Engine::key_pressed_support(self)
    }
}

/// `$CONTROL_PAR_*_COLOR` properties by colour slot.
type ColorSlot = fn(&mut ui::Colors) -> &mut Option<ui::Rgba>;
const COLORS: [(&str, ColorSlot); 10] = [
    ("$CONTROL_PAR_BG_COLOR", |c| &mut c.background),
    ("$CONTROL_PAR_ON_COLOR", |c| &mut c.on),
    ("$CONTROL_PAR_OFF_COLOR", |c| &mut c.off),
    ("$CONTROL_PAR_BAR_COLOR", |c| &mut c.bar),
    ("$CONTROL_PAR_PEAK_COLOR", |c| &mut c.peak),
    ("$CONTROL_PAR_OVERLOAD_COLOR", |c| &mut c.overload),
    ("$CONTROL_PAR_ZERO_LINE_COLOR", |c| &mut c.zero_line),
    ("$CONTROL_PAR_WAVE_COLOR", |c| &mut c.wave),
    ("$CONTROL_PAR_WAVE_CURSOR_COLOR", |c| &mut c.wave_cursor),
    ("$CONTROL_PAR_SLICEMARKERS_COLOR", |c| &mut c.slice_markers),
];

/// The parameters the tree carries; any other a script set is reported.
const MAPPED: &[&str] = &[
    "$CONTROL_PAR_VALUE",
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_MIN_VALUE",
    "$CONTROL_PAR_MAX_VALUE",
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_WIDTH",
    "$CONTROL_PAR_HEIGHT",
    "$CONTROL_PAR_HIDE",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_PICTURE_STATE",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_TEXTPOS_Y",
    "$CONTROL_PAR_VALUEPOS_Y",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR",
    "$CONTROL_PAR_AUTOMATION_NAME",
    "$CONTROL_PAR_SHORT_NAME",
    "$CONTROL_PAR_ALLOW_AUTOMATION",
    "$CONTROL_PAR_AUTOMATION_ID",
    "$CONTROL_PAR_SHOW_ARROWS",
    "$CONTROL_PAR_VERTICAL",
    "$CONTROL_PAR_DISABLE_TEXT_SHIFTING",
    "$CONTROL_PAR_ACTIVE_INDEX",
    "$CONTROL_PAR_BASEPATH",
    "$CONTROL_PAR_FILE_TYPE",
    "$CONTROL_PAR_COLUMN_WIDTH",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
    "$CONTROL_PAR_MOUSE_MODE",
    "$CONTROL_PAR_WT_VIS_MODE",
    "$CONTROL_PAR_WT_ZONE",
    "$CONTROL_PAR_PARALLAX_X",
    "$CONTROL_PAR_PARALLAX_Y",
    "grid_x",
    "grid_y",
    "table_steps_shown",
    "level_meter",
    "level_meter_group",
    "level_meter_generic",
];

/// The text style of each picture state, in [`ui::Widget::state_styles`] order.
const STATE_FONTS: [&str; 6] = [
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_FONT_TYPE_ON",
    "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
    "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
];

/// Carried, or accepted without effect: NKS properties map a control for
/// Komplete Kontrol hardware and have no view or sound of their own.
pub(crate) fn mapped(par: &str) -> bool {
    MAPPED.contains(&par)
        || STATE_FONTS.contains(&par)
        || COLORS.iter().any(|(c, _)| *c == par)
        || par.starts_with("$CONTROL_PAR_NKS_")
}

/// A control's parameters by name, as the script left them.
struct Props<'a> {
    ints: HashMap<&'a str, i32>,
    texts: HashMap<&'a str, &'a str>,
}

impl Props<'_> {
    fn int(&self, name: &str) -> Option<i32> {
        self.ints.get(name).copied()
    }
    fn text(&self, name: &str) -> Option<&str> {
        self.texts.get(name).copied()
    }
}

struct Builder<'p> {
    ui: ui::Interface,
    assets: HashMap<String, ui::AssetRef>,
    styles: HashMap<(i32, i32), ui::StyleRef>,
    picture: &'p dyn Fn(&str) -> Option<ui::ImageMeta>,
}

impl Builder<'_> {
    /// The picture asset for `name`; an empty name means the stock look.
    fn asset(&mut self, name: &str) -> Option<ui::AssetRef> {
        if name.is_empty() {
            return None;
        }
        let path = picture_path(name);
        if let Some(&a) = self.assets.get(&path) {
            return Some(a);
        }
        let a = ui::AssetRef(self.ui.assets.len());
        let meta = (self.picture)(&path).unwrap_or_default();
        self.ui.assets.push(ui::Asset {
            path: path.clone(),
            kind: ui::AssetKind::Image(meta),
        });
        self.assets.insert(path, a);
        Some(a)
    }

    /// The style of font id `font`: a factory font, or the bitmap font
    /// `bitmap` names (a `get_font_id` picture).
    /// The bitmap font asset of picture `name`.
    fn font_asset(&mut self, name: &str) -> ui::AssetRef {
        let path = picture_path(name);
        if let Some(&a) = self.assets.get(&path) {
            return a;
        }
        let a = ui::AssetRef(self.ui.assets.len());
        let kind = ui::AssetKind::BitmapFont;
        self.ui.assets.push(ui::Asset {
            path: path.clone(),
            kind,
        });
        self.assets.insert(path, a);
        a
    }

    fn style(&mut self, font: i32, align: i32, bitmap: Option<&str>) -> ui::StyleRef {
        if let Some(&s) = self.styles.get(&(font, align)) {
            return s;
        }
        let bitmap = bitmap.map(|name| self.font_asset(name));
        let styles = &mut self.ui.styles;
        *self.styles.entry((font, align)).or_insert_with(|| {
            // Kontakt's factory fonts carry their own colour and size, from
            // NI's font chart (KSP manual, control parameters).
            const COLORS: [u32; 26] = [
                0xfefefe, 0xfefefe, 0x373733, 0xcccccc, 0xe3c269, 0xe4d182, 0x7d3012, 0x853519,
                0x48443c, 0x000000, 0xd9d9d9, 0x898c8d, 0x1b1b1a, 0xd2dce1, 0xb9b9b9, 0x5f5f5f,
                0x000000, 0xfefefe, 0xfefefe, 0x000000, 0x7f7f7f, 0x7f7f7f, 0x000000, 0x7f7f7f,
                0xffffff, 0x0c2431,
            ];
            let factory = usize::try_from(font)
                .ok()
                .and_then(|f| COLORS.get(f).map(|&c| (f, c)));
            styles.push(ui::TextStyle {
                font: bitmap.map_or(ui::Font::Stock(font), ui::Font::Bitmap),
                size: factory
                    .filter(|(f, _)| matches!(f, 1 | 5 | 7 | 16 | 17 | 20))
                    .map(|_| 13.),
                color: factory.map_or(ui::Rgba::default(), |(_, c)| ui::Rgba::rgb(c)),
                align: match align {
                    0 => ui::Align::Left,
                    2 => ui::Align::Right,
                    _ => ui::Align::Center,
                },
            });
            ui::StyleRef(styles.len() - 1)
        })
    }

    fn unsupported(&mut self, widget: Option<usize>, feature: impl Into<String>, value: String) {
        self.ui.unsupported.push(ui::Unsupported {
            widget: widget.map(ui::WidgetRef),
            feature: feature.into(),
            value,
        });
    }
}

/// Library-relative path of a KSP picture name.
#[must_use]
pub fn picture_path(name: &str) -> String {
    format!("Resources/pictures/{name}.png")
}

/// KSP colours are `0xRRGGBB`, or `0xAARRGGBB` when the top byte is set.
fn color(v: i32) -> ui::Rgba {
    let v = v as u32;
    let a = (v >> 24) as u8;
    ui::Rgba {
        a: if a == 0 { 255 } else { a },
        ..ui::Rgba::rgb(v & 0xFF_FFFF)
    }
}

fn unit(n: i32) -> &'static str {
    // $KNOB_UNIT_* (constants.tsv).
    match n {
        1 => "dB",
        2 => "Hz",
        3 => "%",
        4 => "ms",
        5 => "oct",
        6 => "st",
        _ => "",
    }
}

/// Where a widget comes from: the script's control and/or the view's, by
/// index into [`Face`], and the id the script addresses it by.
#[derive(Clone, Copy, Debug)]
struct Key {
    id: i32,
    control: Option<usize>,
    view: Option<usize>,
}

/// The view's controls in its order (the script compiles only those it
/// names), then the script's own. Names match without regard to case, as
/// in Kontakt: Shreddage's view says `VibratoPopUp`, its script `VibratoPopup`.
fn keys(face: &Face) -> Vec<Key> {
    let script: HashMap<String, usize> = face
        .controls
        .iter()
        .enumerate()
        .map(|(n, (_, name, _))| (name.to_ascii_lowercase(), n))
        .collect();
    let id = |n: usize| {
        let (control, ..) = face.controls.get(n)?;
        Some(FIRST_UI_ID + i32::try_from(control.variable).ok()?)
    };
    let viewed = face.view.iter().flat_map(|v| &v.controls);
    let mut keys: Vec<Key> = viewed
        .clone()
        .enumerate()
        .filter_map(|(v, c)| {
            let control = script.get(&c.name.to_ascii_lowercase()).copied();
            Some(Key {
                // A view control the script never names: an id no control has.
                id: control.map_or_else(|| Some(-1 - i32::try_from(v).ok()?), id)?,
                control,
                view: Some(v),
            })
        })
        .collect();
    let in_view: std::collections::HashSet<String> =
        viewed.map(|v| v.name.to_ascii_lowercase()).collect();
    keys.extend(
        face.controls
            .iter()
            .enumerate()
            .filter(|(_, (_, name, _))| !in_view.contains(&name.to_ascii_lowercase()))
            .filter_map(|(n, _)| {
                Some(Key {
                    id: id(n)?,
                    control: Some(n),
                    view: None,
                })
            }),
    );
    keys
}

const UNTOUCHED: ControlLook<'static> = ControlLook {
    value: None,
    pars: &[],
    texts: &[],
    menu: &[],
    generation: 0,
};

fn decl<'a>(face: &'a Face, slot: u8, engine: &'a dyn Looks, key: Key) -> Option<Decl<'a>> {
    let view = key.view.and_then(|v| face.view.as_ref()?.controls.get(v));
    let control = key.control.and_then(|n| face.controls.get(n));
    let look = match control {
        Some(_) => engine.control_look(slot, key.id)?,
        None => UNTOUCHED,
    };
    Some(Decl {
        kind: view.map(|v| v.kind).or(control.map(|c| c.0.kind))?,
        params: view.map(|v| &*v.params).or(control.map(|c| &*c.0.params))?,
        view,
        name: view.map(|v| &*v.name).or(control.map(|c| &*c.1))?,
        cells: control.map_or(Cells::Const, |c| c.2),
        id: key.id,
        look,
    })
}

/// What a widget was last built from: when none of it moved, the widget
/// stays as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    generation: u32,
    value: Option<i64>,
    /// A hash of an array control's cells (a table's steps) or its text.
    cells: u64,
}

impl Seen {
    fn of(d: &Decl<'_>, slot: u8, engine: &dyn Looks) -> Self {
        let cells = match d.cells {
            Cells::Int { base, len } | Cells::Real { base, len } if len > 1 => (base..base + len)
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, c| {
                    let v = engine.script_cell_in(slot, c as usize).unwrap_or(0) as u64;
                    (h ^ v).wrapping_mul(0x100_0000_01b3)
                }),
            Cells::Text { base, .. } => (engine.script_text_in(slot, base as usize))
                .unwrap_or_default()
                .bytes()
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                    (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
                }),
            _ => 0,
        };
        Self {
            generation: d.look.generation,
            value: d.look.value,
            cells,
        }
    }
}

/// A script's UI tree kept current: built once, then [`Live::refresh`]
/// rebuilds only the widgets whose control the script or the user changed.
pub struct Live {
    pub tree: ui::Interface,
    keys: Vec<Key>,
    seen: Vec<Seen>,
    page: Option<Page>,
    assets: HashMap<String, ui::AssetRef>,
    styles: HashMap<(i32, i32), ui::StyleRef>,
}

/// Script `slot`'s performance view as `on init` left it. `picture` gives the
/// layout of a library-relative image path; `None` means one plain frame.
///
/// # Errors
/// When the script built a tree the model rejects (bad nesting).
pub fn interface(
    face: &Face,
    slot: u8,
    engine: &dyn Looks,
    picture: &dyn Fn(&str) -> Option<ui::ImageMeta>,
) -> Result<ui::Interface, ui::Error> {
    Live::new(face, slot, engine, picture).map(|l| l.tree)
}

impl Live {
    /// Builds the tree, as [`interface`].
    ///
    /// # Errors
    /// When the script built a tree the model rejects (bad nesting).
    pub fn new(
        face: &Face,
        slot: u8,
        engine: &dyn Looks,
        picture: &dyn Fn(&str) -> Option<ui::ImageMeta>,
    ) -> Result<Self, ui::Error> {
        let keys = keys(face);
        let decls: Vec<_> = keys
            .iter()
            .filter_map(|&k| decl(face, slot, engine, k))
            .collect();
        let keys = keys
            .into_iter()
            .filter(|&k| decl(face, slot, engine, k).is_some())
            .collect();
        let seen = decls.iter().map(|d| Seen::of(d, slot, engine)).collect();
        let mut bld = Builder {
            ui: ui::Interface {
                source: ui::Source::Ksp { slot },
                ..Default::default()
            },
            assets: HashMap::new(),
            styles: HashMap::new(),
            picture,
        };
        build(&mut bld, face, slot, engine, &decls)?;
        bld.ui.keybed = keybed::keybed(engine.keys());
        bld.ui.key_names = keybed::names(engine.keys());
        bld.ui.key_pressed = keybed::pressed(engine.keys(), engine.pressed_support());
        Ok(Self {
            tree: bld.ui,
            keys,
            seen,
            page: engine.page(slot).copied(),
            assets: bld.assets,
            styles: bld.styles,
        })
    }

    /// Rebuilds the widgets whose control changed since the last call, in
    /// place, and the page when it moved, and says which; an idle script
    /// changes nothing. Panel nesting is kept from the build.
    pub fn refresh(
        &mut self,
        face: &Face,
        slot: u8,
        engine: &dyn Looks,
        picture: &dyn Fn(&str) -> Option<ui::ImageMeta>,
    ) -> Changes {
        let page = engine.page(slot).copied();
        let page = (page != self.page).then(|| {
            self.page = page;
            page
        });
        let (keybed, names) = (keybed::keybed(engine.keys()), keybed::names(engine.keys()));
        let pressed = keybed::pressed(engine.keys(), engine.pressed_support());
        let keys = keybed != self.tree.keybed
            || names != self.tree.key_names
            || pressed != self.tree.key_pressed;
        if keys {
            self.tree.keybed = keybed;
            self.tree.key_names = names;
            self.tree.key_pressed = pressed;
        }
        let mut changed = Vec::new();
        for (n, (&key, seen)) in self.keys.iter().zip(&mut self.seen).enumerate() {
            let Some(d) = decl(face, slot, engine, key) else {
                continue;
            };
            let now = Seen::of(&d, slot, engine);
            if now != *seen {
                *seen = now;
                changed.push((n, d));
            }
        }
        if changed.is_empty() && page.is_none() {
            return Changes {
                keys,
                ..Changes::default()
            };
        }
        let mut bld = Builder {
            ui: std::mem::take(&mut self.tree),
            assets: std::mem::take(&mut self.assets),
            styles: std::mem::take(&mut self.styles),
            picture,
        };
        // The page moved (skin offset, wallpaper frame, size, title): only
        // it is rebuilt, the widgets keep their places.
        if let Some(page) = page {
            bld.ui.pages.clear();
            if let Some(page) = page {
                page_of(&mut bld, &page, face.view.as_ref());
            }
        }
        let mut out = Vec::with_capacity(changed.len());
        for (n, d) in changed {
            let props = props(face, &d);
            let mut w = widget(&mut bld, slot, &d, &props, engine);
            if let Some(old) = bld.ui.widgets.get_mut(n) {
                w.parent = old.parent;
                *old = w;
                out.push(ui::WidgetRef(n));
            }
        }
        (self.tree, self.assets, self.styles) = (bld.ui, bld.assets, bld.styles);
        Changes {
            page: page.is_some(),
            keys,
            widgets: out,
        }
    }
}

impl Live {
    /// What [`Live::refresh`] reads: the UI ids of the controls it shows,
    /// and the script cells of their arrays (a table's steps).
    #[must_use]
    pub fn reads(&self, face: &Face) -> (Vec<i32>, Vec<u32>, Vec<u32>) {
        let controls = self
            .keys
            .iter()
            .filter_map(|k| Some((k.id, face.controls.get(k.control?)?)));
        let mut ids = Vec::new();
        let (mut cells, mut texts) = (Vec::new(), Vec::new());
        for (id, (_, _, c)) in controls {
            ids.push(id);
            match *c {
                Cells::Int { base, len } | Cells::Real { base, len } if len > 1 => {
                    cells.extend(base..base + len);
                }
                Cells::Text { base, .. } => texts.push(base),
                _ => {}
            }
        }
        (ids, cells, texts)
    }
}

/// What one [`Live::refresh`] changed: the page itself, and the widgets it
/// rebuilt in place.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    pub page: bool,
    /// The keyboard's script colours, key names or pressed keys.
    pub keys: bool,
    pub widgets: Vec<ui::WidgetRef>,
}

impl Changes {
    /// Nothing to repaint.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.page && !self.keys && self.widgets.is_empty()
    }
}

fn build(
    bld: &mut Builder<'_>,
    face: &Face,
    slot: u8,
    engine: &dyn Looks,
    decls: &[Decl<'_>],
) -> Result<(), ui::Error> {
    if let Some(page) = engine.page(slot) {
        page_of(bld, page, face.view.as_ref());
    }
    let by_id: HashMap<i32, usize> = decls.iter().enumerate().map(|(i, d)| (d.id, i)).collect();
    let by_name: HashMap<String, i32> = decls
        .iter()
        .map(|d| (d.name.to_ascii_lowercase(), d.id))
        .collect();
    for (i, d) in decls.iter().enumerate() {
        let props = props(face, d);
        let mut w = widget(bld, slot, d, &props, engine);
        let parent = props.int("$CONTROL_PAR_PARENT_PANEL").or_else(|| {
            let name = d.view?.parent.as_deref()?;
            by_name.get(&name.to_ascii_lowercase()).copied()
        });
        if let Some(parent) = parent {
            match by_id.get(&parent) {
                Some(&p)
                    if p != i && decls.get(p).is_some_and(|d| d.kind == ControlKind::Panel) =>
                {
                    w.parent = Some(ui::WidgetRef(p));
                }
                _ => bld.unsupported(Some(i), "$CONTROL_PAR_PARENT_PANEL", parent.to_string()),
            }
        }
        for (&par, &value) in &props.ints {
            if !mapped(par) {
                bld.unsupported(Some(i), par, value.to_string());
            }
        }
        for (&par, &value) in &props.texts {
            if !mapped(par) {
                bld.unsupported(Some(i), par, value.into());
            }
        }
        bld.ui.widgets.push(w);
    }
    for skipped in face.view.iter().flat_map(|v| &v.skipped) {
        bld.unsupported(None, "performance view control", skipped.clone());
    }
    // Panels can be nested in any order; break a cycle rather than hand over
    // a malformed tree.
    if let Err(ui::Error::ParentCycle(at)) = bld.ui.validate()
        && let Some(w) = bld.ui.widgets.get_mut(at.0)
    {
        let parent = w.parent.take();
        bld.unsupported(
            Some(at.0),
            "$CONTROL_PAR_PARENT_PANEL",
            format!("cycle via {:?}", parent.map(|p| p.0)),
        );
    }
    bld.ui.validate()?;
    Ok(())
}

/// The page as init left it, over the performance view's when it loads one.
fn page_of(bld: &mut Builder<'_>, page: &Page, view: Option<&View>) {
    bld.ui.performance = page.performance || view.is_some();
    let or_view = |set: &str, view: Option<&String>| {
        let set = (!set.is_empty()).then(|| set.to_owned());
        set.or_else(|| view.cloned()).unwrap_or_default()
    };
    let mut background = ui::Background {
        offset_y: page.skin_offset,
        origin_y: HEADER,
        color: page
            .color
            .or(view.and_then(|v| v.color))
            .map(|c| ui::Rgba::rgb(c as u32)),
        frame: u32::try_from(page.wallpaper_frame).unwrap_or(0),
        ..Default::default()
    };
    let wallpaper = or_view(
        page.wallpaper.as_str(),
        view.and_then(|v| v.wallpaper.as_ref()),
    );
    background.image = bld.asset(&wallpaper);
    let icon = or_view(page.icon.as_str(), view.and_then(|v| v.icon.as_ref()));
    bld.ui.icon = bld.asset(&icon);
    bld.ui.icon_hidden = page.icon_hide & HIDE_WHOLE_CONTROL != 0;
    let height_px = page.height_px.or(view
        .and_then(|v| v.height)
        .filter(|_| page.height_rows.is_none()));
    let height_rows = height_px
        .is_none()
        .then(|| page.height_rows.map(|r| r.max(1) as u32))
        .flatten();
    // ponytail: 100 px when the script sets no height; Kontakt's own default may differ.
    let height = match (height_px, height_rows) {
        (Some(px), _) => px.max(1) as u32,
        (None, Some(_)) => 0,
        (None, None) => 100,
    };
    let width = page
        .width_px
        .or(view.and_then(|v| v.width))
        .map_or(DEFAULT_WIDTH, |w| w.max(1) as u32);
    bld.ui.pages.push(ui::Page {
        name: page.title.as_str().into(),
        size: ui::Size {
            width: f64::from(width),
            height: f64::from(height),
        },
        background,
        height_rows,
    });
}

/// `$HIDE_*` (constants.tsv, measured in Kontakt).
const HIDE_PART_BG: i32 = 1;
const HIDE_PART_TITLE: i32 = 2;
const HIDE_PART_VALUE: i32 = 4;
const HIDE_WHOLE_CONTROL: i32 = 16;

/// The view's properties, then what the script set over them.
fn props<'a>(face: &'a Face, d: &'a Decl<'a>) -> Props<'a> {
    let view = d.view.into_iter();
    let ints = view
        .clone()
        .flat_map(|v| &v.ints)
        .map(|(p, v)| (p.as_str(), *v));
    let texts = view
        .flat_map(|v| &v.texts)
        .map(|(p, t)| (p.as_str(), t.as_str()));
    let look = &d.look;
    Props {
        ints: ints
            .chain(
                look.pars
                    .iter()
                    .filter_map(|&(par, v)| Some((face.par_name(par)?, v))),
            )
            .collect(),
        texts: texts
            .chain(
                look.texts
                    .iter()
                    .filter_map(|(par, t)| Some((face.par_name(*par)?, t.as_str()))),
            )
            .collect(),
    }
}
