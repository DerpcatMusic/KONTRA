//! Pure projection of a script page into the things the view draws. The
//! runtime supplies a settled interface; resolving it does no I/O and does
//! not depend on decoded pictures or the window.

use kontra_ui_model::{
    self as model, ImageUse, Interface, Kind, PageRef, Role, WidgetRef, kontakt_picture_frame,
};

use super::{image_asset, knob_name, readout, stock, value};

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
    let on_page = |r: model::Rect| {
        r.width > 0.
            && r.height > 0.
            && r.x < source.size.width
            && r.y < source.size.height
            && r.x + r.width > 0.
            && r.y + r.height > 0.
    };
    let widgets = tree
        .draw_order(page)
        .into_iter()
        .filter_map(|index| {
            let raw = tree.page_rect(index);
            let w = tree.widgets.get(index.0)?;
            if !tree.visible(index) || !on_page(raw) {
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
            let position = position(w);
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
                rect: model::Rect {
                    x: raw.x * scale,
                    y: raw.y * scale,
                    width: raw.width * scale,
                    height: raw.height * scale,
                },
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
            height: source.size.height * scale,
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
            (position(w) * frames.saturating_sub(1) as f64).round() as usize
        }
        (Kind::Button { .. } | Kind::Switch, Role::Strip) => {
            kontakt_picture_frame(value(w) != 0., held, false, frames)
        }
        _ => 0,
    }
}

/// Where `w`'s value lies in its range, 0..1; a switch is 0 or 1.
fn position(w: &model::Widget) -> f64 {
    match &w.kind {
        Kind::Knob { range, .. } | Kind::Slider { range, .. } | Kind::ValueEdit { range, .. } => {
            let span = range.max - range.min;
            if span == 0. {
                0.
            } else {
                ((value(w) - range.min) / span).clamp(0., 1.)
            }
        }
        _ => f64::from(u8::from(value(w) != 0.)),
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
    pub inset: f64,
    pub align: model::Align,
    pub color: Option<model::Rgba>,
    pub multiline: bool,
}

pub(super) fn text(
    words: &str,
    style: Option<&model::TextStyle>,
    widget: &model::Widget,
    (height, scale): (f64, f64),
    source: model::Source,
) -> Text {
    let size = style.and_then(|s| s.size).map_or(super::TEXT, f64::from) * scale;
    let multiline = words.contains('\n');
    let line = size * if multiline { 13. / 11. } else { 1.25 };
    let lua = matches!(source, model::Source::FalconLua);
    let (align, top) = placed(words, style, widget, (height, line, scale), !lua);
    let label = matches!(widget.kind, model::Kind::Label);
    Text {
        size,
        line,
        top: top.max(0.),
        inset: if label { 7. } else { 2. } * scale,
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
    let advance = |c| width_of(crate::pictures::glyph(c)).map_or(0., |w| f64::from(w) * scale);
    let mut result = Vec::new();
    for (n, row) in words.split('\n').enumerate() {
        let wide: f64 = row.chars().map(advance).sum();
        let mut x = match align {
            model::Align::Left => 2. * scale,
            model::Align::Center => (width - wide) / 2.,
            model::Align::Right => width - 2. * scale - wide,
        };
        for c in row.chars() {
            let index = crate::pictures::glyph(c);
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
    glyphs: &crate::pictures::Glyphs,
    width: f64,
    scale: f64,
) -> bool {
    let advance: f64 = text
        .chars()
        .map(|c| {
            if c == ' ' {
                4.
            } else {
                glyphs
                    .get(crate::pictures::glyph(c))
                    .map_or(0., |g| f64::from(g.width))
            }
        })
        .sum();
    advance * scale <= (width - 14. * scale).max(0.)
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
        let top = |source| text("Sordino", None, &label, (22., 1.), source).top;
        assert_eq!(top(model::Source::Ksp { slot: 0 }), 0.);
        assert!(top(model::Source::FalconLua) > 0.);
        let mut button = label.clone();
        button.kind = model::Kind::Button { momentary: false };
        assert!(
            text(
                "Go",
                None,
                &button,
                (22., 1.),
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
}
