//! Pure projection of a script page into the things the view draws. The
//! runtime supplies a settled interface; resolving it does no I/O and does
//! not depend on decoded pictures or the window.

use kontra_ui_model::{
    self as model, ImageUse, Interface, Kind, PageRef, Role, WidgetRef, kontakt_picture_frame,
};

use super::{image_asset, knob_name, readout, stock};

/// One page, including the source page's background and its visible controls.
pub(super) struct Page<'a> {
    pub source: &'a model::Page,
    pub size: model::Size,
    pub widgets: Vec<Widget<'a>>,
}

/// A picture's asset, geometry and selected animation frame.
pub(super) struct Picture<'a> {
    pub use_: ImageUse,
    pub path: &'a str,
    pub meta: model::ImageMeta,
    pub frame: usize,
    pub size: model::Size,
}

/// One visible widget in scaled view coordinates, in back-to-front order.
pub(super) struct Widget<'a> {
    pub index: WidgetRef,
    pub source: &'a model::Widget,
    pub rect: model::Rect,
    pub picture: Option<Picture<'a>>,
    /// A slider's handle, drawn over its picture at its value.
    pub handle: Option<Picture<'a>>,
    pub style: Option<&'a model::TextStyle>,
    pub position: f64,
    pub look: Option<stock::Look>,
    pub stock: bool,
    pub words: String,
}

/// Project `page` from `tree` using its resolved geometry. `bare` selects
/// the Vector presentation; the same bounds, stacking and visibility apply.
pub(super) fn page<'a>(
    tree: &'a Interface,
    page: PageRef,
    bare: bool,
    scale: f64,
    pressed: &[WidgetRef],
) -> Option<Page<'a>> {
    let source = tree.pages.get(page.0)?;
    let above = f64::from(source.background.above);
    let on_page = |r: model::Rect| {
        r.width > 0.
            && r.height > 0.
            && r.x < source.size.width
            && r.y < source.size.height
            && r.x + r.width > 0.
            && r.y + r.height > -above
    };
    let widgets = tree
        .draw_order(page)
        .into_iter()
        .filter_map(|index| {
            let raw = tree.page_rect(index);
            let w = tree.widgets.get(index.0)?;
            if !tree.visible(index) || (!on_page(raw) && tree.scroller(index).is_none()) {
                return None;
            }
            let picture = |use_: &ImageUse| {
                let (path, meta) = image_asset(tree, use_.asset)?;
                Some(Picture {
                    use_: *use_,
                    path,
                    meta,
                    frame: frame(
                        w,
                        use_,
                        meta.frames as usize,
                        pressed.binary_search(&index).is_ok(),
                    ),
                    size: {
                        // A handle keeps its own size to ride its track.
                        let size = match (use_.role, meta.size) {
                            (Role::Handle, Some(own)) => own,
                            _ => drawn(raw, meta),
                        };
                        model::Size {
                            width: size.width * scale,
                            height: size.height * scale,
                        }
                    },
                })
            };
            let (handles, faces): (Vec<&ImageUse>, Vec<&ImageUse>) =
                w.images.iter().partition(|u| u.role == Role::Handle);
            let handle = handles.first().filter(|_| !bare).and_then(|u| picture(u));
            let picture = faces.first().filter(|_| !bare).and_then(|u| picture(u));
            let style = w.style.and_then(|s| tree.styles.get(s.0));
            let position = w.position();
            let look = stock::Look::of(
                tree,
                w,
                position,
                readout(w, tree.source),
                knob_name(w),
                bare,
            );
            let stock = look.is_none() && stock::shows(tree, w, bare);
            let words = if look.is_some() {
                String::new()
            } else {
                caption(w, tree.source)
            };
            Some(Widget {
                index,
                source: w,
                rect: snapped(raw.x, raw.y + above, raw.width, raw.height, scale),
                picture,
                handle,
                style,
                position,
                look,
                stock,
                words,
            })
        })
        .collect();
    Some(Page {
        source,
        size: model::Size {
            width: source.size.width * scale,
            height: source.shown_height() * scale,
        },
        widgets,
    })
}

/// The frame `w` shows of a picture with `frames`: the script's
/// `PICTURE_STATE`, else the one its value selects.
fn frame(w: &model::Widget, u: &ImageUse, frames: usize, held: bool) -> usize {
    if let Some(f) = u.frame {
        return f as usize;
    }
    match (&w.kind, u.role) {
        (Kind::Knob { .. } | Kind::Slider { .. } | Kind::ValueEdit { .. }, Role::Strip) => {
            (w.position() * frames.saturating_sub(1) as f64).round() as usize
        }
        (Kind::Button { .. } | Kind::Switch, Role::Strip) => {
            kontakt_picture_frame(w.number() != 0., held, false, frames)
        }
        _ => 0,
    }
}

/// A picture takes its frame's own size along each axis it does not
/// stretch, whatever size the widget has (a label's text keeps its bounds).
fn drawn(r: model::Rect, m: model::ImageMeta) -> model::Size {
    let own = m.size.unwrap_or(model::Size {
        width: r.width,
        height: r.height,
    });
    model::Size {
        width: if m.stretch[0] { r.width } else { own.width },
        height: if m.stretch[1] { r.height } else { own.height },
    }
}

/// The words a widget shows: a label's or button's text, a menu's item,
/// a value edit's title and value.
pub(super) fn caption(w: &model::Widget, source: model::Source) -> String {
    if w.label_in_image() {
        return String::new();
    }
    if let Kind::ValueEdit { .. } = w.kind {
        let title = if w.hide.title { "" } else { w.text.trim() };
        return format!("{title} {}", readout(w, source)).trim().to_owned();
    }
    if w.hide.title {
        return String::new();
    }
    match &w.kind {
        // Kontakt's label tabs advance without drawing a missing-glyph box.
        Kind::Label if matches!(source, model::Source::Ksp { .. }) => w.text.replace('\t', "  "),
        Kind::Label | Kind::Button { .. } | Kind::Switch | Kind::TextEdit => w.text.clone(),
        Kind::Menu { items } => {
            let at = match w.value {
                Some(model::Value::Integer(v)) => v,
                _ => w.initial_value as i32,
            };
            items
                .iter()
                .find(|i| i.value == at)
                .map_or(String::new(), |i| i.text.clone())
        }
        _ => String::new(),
    }
}

/// Geometry and ink of a label, in view points. Explicit line breaks keep
/// their empty rows and use the source's measured line pitch. A Kontakt
/// label's words start at its top however tall it is (Areia's 18- and
/// 22-point labels both ink from 3.3 points down in Kontakt 8); anything
/// else's single line sits in its middle.
pub(super) struct Text {
    pub size: f64,
    pub line: f64,
    pub top: f64,
    /// Room left and right of the words.
    pub inset: (f64, f64),
    pub align: model::Align,
    pub color: Option<model::Rgba>,
    pub multiline: bool,
}

pub(super) fn text(
    words: &str,
    style: Option<&model::TextStyle>,
    widget: &model::Widget,
    (height, scale, em): (f64, f64, f64),
    source: model::Source,
) -> Text {
    let multiline = words.contains('\n');
    let lua = matches!(source, model::Source::FalconLua);
    let menu = lua && matches!(widget.kind, model::Kind::Menu { .. });
    let size = style.and_then(|s| s.size).map_or(super::TEXT, f64::from) * scale;
    // Lines a UVI size apart (JUCE's font height, `size` at `em` per unit), Kontakt's 13/11.
    let pitch = if lua { 1. / em } else { 13. / 11. };
    // JUCE fits one line by width only: it sits in the middle however tall it runs.
    let line = match (multiline, lua) {
        (true, _) => size * pitch,
        (false, true) => (size * 1.5).max(height),
        (false, false) => size * 1.25,
    };
    let (align, top) = placed(words, style, widget, (height, line, scale), !lua);
    let label = matches!(widget.kind, model::Kind::Label);
    Text {
        size,
        line,
        top: if lua { top } else { top.max(0.) },
        // A UVI label is JUCE's, inset 5 by its border; a UVI menu's sits
        // at 1 in a combo, clear of its h − 3 arrow box (JUCE V2 positionComboBoxText).
        inset: match (label, lua, menu) {
            (_, _, true) => (6. * scale, height + scale),
            (true, true, _) => (5. * scale, 5. * scale),
            (true, false, _) => (7. * scale, 7. * scale),
            (false, ..) => (2. * scale, 2. * scale),
        },
        align,
        color: style.map(|s| s.color),
        multiline,
    }
}

/// How a widget's words align across it, and where their first line's top
/// sits: a label's at its top when `label_from_top` (Kontakt), explicit
/// lines always at the top, anything else in its middle.
fn placed(
    words: &str,
    style: Option<&model::TextStyle>,
    widget: &model::Widget,
    (height, line, scale): (f64, f64, f64),
    label_from_top: bool,
) -> (model::Align, f64) {
    let label = matches!(widget.kind, model::Kind::Label);
    let fallback = if label {
        model::Align::Left
    } else {
        model::Align::Center
    };
    let from_top = words.contains('\n') || (label && label_from_top);
    let top = widget
        .text_y
        .map_or(if from_top { 0. } else { (height - line) / 2. }, |y| {
            f64::from(y) * scale
        });
    (style.map_or(fallback, |s| s.align), top)
}

/// One bitmap glyph and its page-local destination, in view points.
pub(super) struct Glyph {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
}

/// Places a picture font's glyphs by their own advances, including empty
/// explicit lines. `width_of` reads only the already-decoded font metrics.
pub(super) fn glyphs(
    words: &str,
    style: Option<&model::TextStyle>,
    widget: &model::Widget,
    (width, height): (f64, f64),
    (scale, line): (f64, f64),
    width_of: impl Fn(usize) -> Option<u32>,
) -> Vec<Glyph> {
    // Picture fonts are Kontakt's: a label starts at its top.
    let (align, top) = placed(words, style, widget, (height, line, scale), true);
    let advance = |c| width_of(kontra_ui_pictures::glyph(c)).map_or(0., |w| f64::from(w) * scale);
    let mut result = Vec::new();
    for (n, row) in words.split('\n').enumerate() {
        let wide: f64 = row.chars().map(advance).sum();
        let mut x = match align {
            model::Align::Left => 2. * scale,
            model::Align::Center => (width - wide) / 2.,
            model::Align::Right => width - 2. * scale - wide,
        };
        for c in row.chars() {
            let index = kontra_ui_pictures::glyph(c);
            let Some(w) = width_of(index) else { continue };
            let glyph_width = f64::from(w) * scale;
            result.push(Glyph {
                index,
                x,
                y: top + n as f64 * line,
                width: glyph_width,
            });
            x += glyph_width;
        }
    }
    result
}

/// A rectangular source piece and its destination, in view points.
pub(super) struct Slice {
    pub source: [u32; 4],
    pub destination: [f64; 4],
}

/// The whole picture or its nonempty nine-slice pieces.
pub(super) enum Slices {
    Whole,
    Pieces(Vec<Slice>),
}

pub(super) fn slices(
    meta: &model::ImageMeta,
    decoded: (u32, u32),
    (width, height): (f64, f64),
    scale: f64,
) -> Slices {
    let own = meta
        .size
        .map_or((f64::from(decoded.0), f64::from(decoded.1)), |s| {
            (s.width, s.height)
        });
    let m = meta.margins;
    let across = cuts(
        meta.stretch[0],
        (m.left, m.right),
        (own.0, decoded.0),
        width,
        scale,
    );
    let down = cuts(
        meta.stretch[1],
        (m.top, m.bottom),
        (own.1, decoded.1),
        height,
        scale,
    );
    if across.len() == 1 && down.len() == 1 {
        return Slices::Whole;
    }
    let mut pieces = Vec::new();
    let mut y = 0.;
    for &(sy, sh, dh) in &down {
        let mut x = 0.;
        for &(sx, sw, dw) in &across {
            pieces.push(Slice {
                source: [sx, sy, sw, sh],
                destination: [x, y, dw, dh],
            });
            x += dw;
        }
        y += dh;
    }
    Slices::Pieces(pieces)
}

/// One source axis, mapped to decoded pixels and destination points.
/// Fixed ends keep their size, shrinking together when the destination is
/// shorter than the two ends.
pub(super) fn cuts(
    on: bool,
    (lo, hi): (u32, u32),
    (own, decoded): (f64, u32),
    to: f64,
    scale: f64,
) -> Vec<(u32, u32, f64)> {
    let ends = f64::from(lo + hi);
    if !on || lo + hi == 0 || ends >= own {
        return vec![(0, decoded, to)];
    }
    let k = f64::from(decoded) / own.max(1.);
    let (a, b) = (
        (f64::from(lo) * k).round() as u32,
        (f64::from(hi) * k).round() as u32,
    );
    let mid = decoded.saturating_sub(a + b);
    let fit = (to / (ends * scale)).min(1.);
    let (dl, dh) = (f64::from(lo) * scale * fit, f64::from(hi) * scale * fit);
    vec![
        (0, a, dl),
        (a, mid, (to - dl - dh).max(0.)),
        (a + mid, b, dh),
    ]
    .into_iter()
    .filter(|&(_, n, d)| n > 0 && d > 0.)
    .collect()
}

/// Whether a stock label fits inside the same inset used by the text fallback.
pub(super) fn label_fits(
    text: &str,
    glyphs: &kontra_ui_pictures::Glyphs,
    width: f64,
    scale: f64,
) -> bool {
    // The pixel font spaces 3 and insets 5 a side.
    let (space, inset) = (3., 5.);
    let advance: f64 = text
        .chars()
        .map(|c| {
            if c == ' ' {
                space
            } else {
                glyphs
                    .get(kontra_ui_pictures::glyph(c))
                    .map_or(0., |g| f64::from(g.width))
            }
        })
        .sum();
    advance * scale <= (width - 2. * inset * scale).max(0.)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kontakt_label_starts_at_its_top_and_a_lua_one_sits_in_its_middle() {
        let label = model::Widget::new(
            "$label",
            PageRef(0),
            model::Rect::new(0, 0, 50, 22),
            model::Kind::Label,
        );
        let at = |source| text("Sordino", None, &label, (22., 1., 1.), source);
        assert_eq!(at(model::Source::Ksp { slot: 0 }).top, 0.);
        let lua = at(model::Source::FalconLua);
        assert_eq!(lua.top + lua.line / 2., 11.);
        let mut button = label.clone();
        button.kind = model::Kind::Button { momentary: false };
        assert!(
            text(
                "Go",
                None,
                &button,
                (22., 1., 1.),
                model::Source::Ksp { slot: 0 }
            )
            .top > 0.
        );
    }

    #[test]
    fn a_held_button_selects_its_pressed_picture_in_the_plan() {
        let mut tree = Interface {
            source: model::Source::Ksp { slot: 0 },
            ..Default::default()
        };
        tree.pages.push(model::Page {
            size: model::Size {
                width: 100.,
                height: 100.,
            },
            ..Default::default()
        });
        tree.assets.push(model::Asset {
            path: "button.png".into(),
            kind: model::AssetKind::Image(model::ImageMeta {
                frames: 6,
                ..Default::default()
            }),
        });
        let mut w = model::Widget::new(
            "$button",
            PageRef(0),
            model::Rect::new(1, 2, 40, 30),
            model::Kind::Button { momentary: false },
        );
        w.images
            .push(model::ImageUse::new(model::AssetRef(0), model::Role::Strip));
        tree.widgets.push(w);
        let frame = |pressed: &[WidgetRef]| {
            page(&tree, PageRef(0), false, 1., pressed)
                .and_then(|p| p.widgets.into_iter().next()?.picture.map(|p| p.frame))
        };
        assert_eq!(frame(&[]), Some(0));
        assert_eq!(frame(&[WidgetRef(0)]), Some(2));
    }

    /// A UVI label's lines are its authored size apart (JUCE's font
    /// height); a Kontakt one's keep Kontakt's measured 13/11.
    #[test]
    fn a_uvi_label_pitches_its_lines_at_its_size() {
        let w = model::Widget::new(
            "l",
            PageRef(0),
            model::Rect::new(0, 0, 80, 60),
            model::Kind::Label,
        );
        let style = model::TextStyle {
            font: model::Font::Default,
            size: Some(22.),
            color: model::Rgba::default(),
            align: model::Align::Left,
        };
        // 22 at 0.8 em per unit of height: authored 27.5, as `words_el` gives it.
        let line = |source| text("a\nb", Some(&style), &w, (60., 1., 0.8), source).line;
        assert_eq!(line(model::Source::FalconLua), 27.5);
        assert_eq!(line(model::Source::Ksp { slot: 0 }), 26.);
    }

    /// A UVI menu's words sit in JUCE's combo label: 1 + 5 in, clear of the
    /// h − 3 arrow box; one line is never fitted down by its height.
    #[test]
    fn a_uvi_menu_keeps_its_words_off_the_arrow() {
        let w = model::Widget::new(
            "m",
            PageRef(0),
            model::Rect::new(0, 0, 120, 20),
            model::Kind::Menu { items: Vec::new() },
        );
        let t = text("Mode", None, &w, (20., 1., 1.), model::Source::FalconLua);
        assert_eq!(t.inset, (6., 21.));
        assert!(t.line >= t.size * 1.25);
        let k = text(
            "Mode",
            None,
            &w,
            (20., 1., 1.),
            model::Source::Ksp { slot: 0 },
        );
        assert_eq!(k.inset, (2., 2.));
    }

    /// A page that shows rows above its y 0 draws a label placed there
    /// (Pacific's headings at y -5) in that band, and grows by it.
    #[test]
    fn a_label_over_y_0_draws_in_the_band_the_page_shows_above_it() {
        let mut tree = Interface::default();
        tree.pages.push(model::Page {
            size: model::Size {
                width: 100.,
                height: 50.,
            },
            background: model::Background {
                above: 4,
                ..Default::default()
            },
            ..Default::default()
        });
        tree.widgets.push(model::Widget::new(
            "$heading",
            PageRef(0),
            model::Rect::new(0, -5, 60, 18),
            model::Kind::Label,
        ));
        let p = page(&tree, PageRef(0), false, 2., &[]).unwrap();
        assert_eq!(p.size.height, 108.);
        assert_eq!(p.widgets.first().map(|w| w.rect.y), Some(-2.));
    }
}

/// A control's place in device pixels, its edges on whole pixels, so a
/// picture resampled to the device lands on the grid instead of being
/// filtered a second time at a half-pixel offset.
fn snapped(x: f64, y: f64, width: f64, height: f64, scale: f64) -> model::Rect {
    let (x0, y0) = ((x * scale).round(), (y * scale).round());
    model::Rect {
        x: x0,
        y: y0,
        width: ((x + width) * scale).round() - x0,
        height: ((y + height) * scale).round() - y0,
    }
}
