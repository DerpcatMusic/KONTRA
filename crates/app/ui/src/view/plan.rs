//! Pure projection of a script page into the things the view draws. The
//! runtime supplies a settled interface; resolving it does no I/O and does
//! not depend on decoded pictures or the window.

use kontra_ui_model::{self as model, ImageUse, Interface, PageRef, WidgetRef};

use super::{caption, drawn, frame, image_asset, knob_name, position, readout, stock};

/// One page, including the source page's background and its visible controls.
pub(super) struct Page<'a> {
    pub source: &'a model::Page,
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

/// One visible widget in page coordinates, in back-to-front order.
pub(super) struct Widget<'a> {
    pub index: WidgetRef,
    pub source: &'a model::Widget,
    pub rect: model::Rect,
    pub picture: Option<Picture<'a>>,
    pub style: Option<&'a model::TextStyle>,
    pub position: f64,
    pub look: Option<stock::Look>,
    pub stock: bool,
    pub words: String,
}

/// Project `page` from `tree` using its resolved geometry. `bare` selects
/// the Vector presentation; the same bounds, stacking and visibility apply.
pub(super) fn page(tree: &Interface, page: PageRef, bare: bool) -> Option<Page<'_>> {
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
            let rect = tree.page_rect(index);
            let w = tree.widgets.get(index.0)?;
            if !tree.visible(index) || !on_page(rect) {
                return None;
            }
            let picture = w.images.first().filter(|_| !bare).and_then(|use_| {
                let (path, meta) = image_asset(tree, use_.asset)?;
                Some(Picture {
                    use_: *use_,
                    path,
                    meta,
                    frame: frame(w, use_, meta.frames as usize),
                    size: drawn(rect, meta),
                })
            });
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
                rect,
                picture,
                style,
                position,
                look,
                stock,
                words,
            })
        })
        .collect();
    Some(Page { source, widgets })
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
