//! A Kontakt 8 `native_ui` page in the tree: rendered over the script's
//! controls ([`kontra_lua::nui`]), laid out ([`layout`]) in place of their
//! own widgets, and kept current: a control's change renders again only
//! the component instances that read it, each back in its own room.

use kontra_ksp::ControlKind;
use kontra_lua::nui::{self, Control};
use kontra_ui_model as ui;

use super::{Builder, Decl, Face};

mod layout;
pub(super) use layout::Slot;

/// A page's modules (path under `native_ui/`, source) and its entry.
#[derive(Clone, Debug, Default)]
pub struct Native {
    modules: Vec<(String, String)>,
    entry: String,
}

impl Native {
    /// The page among a script's library files, if it loads one.
    pub(super) fn of(data: &[(String, Vec<u8>)]) -> Option<Self> {
        let (_, entry) = data.iter().find(|(n, _)| n == "native_ui.entry")?;
        let modules = data
            .iter()
            .filter_map(|(n, bytes)| {
                let path = n.strip_prefix("native_ui/")?;
                Some((path.to_owned(), String::from_utf8_lossy(bytes).into_owned()))
            })
            .collect();
        Some(Self {
            modules,
            entry: String::from_utf8_lossy(entry).into_owned(),
        })
    }
}

/// The name a tap area's widget carries, then its `start` and `complete`
/// callbacks' numbers (0 for none).
pub(super) const TAP: &str = "native_ui tap";

/// The name a canvas's widget carries, then its `paint` callback's number.
pub(super) const CANVAS: &str = "native_ui canvas";

/// Paints the page's canvases inside `range`, each in its size.
fn paint_range(page: &mut nui::Page, bld: &mut Builder<'_>, range: std::ops::Range<usize>) {
    let mut failed = None;
    if let Some(slice) = bld.ui.widgets.get_mut(range) {
        for w in slice {
            let Some(n) = (w.name.strip_prefix(CANVAS)).and_then(|n| n.trim().parse().ok()) else {
                continue;
            };
            match page.paint(n, [w.rect.width, w.rect.height]) {
                Ok(p) => w.kind = ui::Kind::Canvas(p),
                Err(e) => failed = Some(e),
            }
        }
    }
    if let Some(e) = failed {
        report(bld, &e);
    }
}

/// The page's id of a control: its script name without the sigil.
fn id(name: &str) -> &str {
    name.trim_start_matches(['$', '%', '@', '~', '?', '!'])
}

fn control(face: &Face, d: &Decl<'_>) -> Control {
    let range = matches!(
        d.kind,
        ControlKind::Knob | ControlKind::Slider | ControlKind::ValueEdit
    );
    let par = |i: usize| {
        range
            .then(|| d.params.get(i))
            .flatten()
            .map(|&v| f64::from(v))
    };
    let props = super::props(face, d);
    let text = |k: &str| props.text(k).map(str::to_owned);
    let entries = if d.look.menu.is_empty() {
        let menu = d.view.map(|v| v.menu.as_slice()).unwrap_or_default();
        menu.iter()
            .map(|(t, v, _)| (t.clone(), f64::from(*v)))
            .collect()
    } else {
        let menu = d.look.menu.iter();
        menu.map(|m| (m.text.as_str().to_owned(), f64::from(m.value)))
            .collect()
    };
    let (min, max) = match d.kind {
        ControlKind::Button | ControlKind::Switch => (0., 1.),
        _ => (par(0).unwrap_or(0.), par(1).unwrap_or(1_000_000.)),
    };
    Control {
        name: id(d.name).to_owned(),
        label: text("$CONTROL_PAR_TEXT").unwrap_or_else(|| id(d.name).to_owned()),
        value_text: text("$CONTROL_PAR_LABEL").unwrap_or_default(),
        min,
        max,
        default: min,
        // A value edit reads raw: a page's steppers read knobs 0..1 and
        // their value edits raw (`denorm`), both with an index.
        normalized: matches!(d.kind, ControlKind::Knob | ControlKind::Slider) && max > min,
        entries,
        text: d.text.map(str::to_owned),
    }
}

/// A page shown: its state, where its widgets start in the tree, its size
/// and where its instances were placed.
pub(crate) struct Shown {
    page: nui::Page,
    /// Each control's UI id, by the page's id.
    controls: Vec<(i32, String)>,
    first: usize,
    size: ui::Size,
    pub(super) slots: Vec<Slot>,
}

/// The controls in `decls` as the page reads them, and their raw values
/// (the page's parameters scale them, [`Control::normalized`]).
fn read(face: &Face, decls: &[Decl<'_>]) -> (Vec<Control>, Vec<(String, f64)>) {
    let controls: Vec<_> = decls.iter().map(|d| control(face, d)).collect();
    let values = decls
        .iter()
        .zip(&controls)
        .filter_map(|(d, c)| Some((c.name.clone(), d.look.value? as f64)))
        .collect();
    (controls, values)
}

fn report(bld: &mut Builder<'_>, e: &nui::Diagnostic) {
    let at = e.line.map_or_else(String::new, |l| format!(":{l}"));
    bld.unsupported(
        None,
        "native_ui",
        format!("{}{at}: {}", e.module, e.message),
    );
}

/// Renders `native` over the script's controls and appends what it draws
/// on the page, in place of the controls' own widgets. Whatever the page
/// does not cover is reported, and then it shows nothing of the page.
pub(super) fn face(
    bld: &mut Builder<'_>,
    face: &Face,
    native: &Native,
    decls: &[Decl<'_>],
) -> Option<Shown> {
    let (read, values) = read(face, decls);
    let controls = decls
        .iter()
        .zip(&read)
        .map(|(d, c)| (d.id, c.name.clone()))
        .collect();
    let rendered = nui::load(&native.modules, &native.entry).and_then(|mut page| {
        page.set_controls(&read)?;
        let root = page.render(&values)?;
        Ok((page, root))
    });
    let (mut page, root) = match rendered {
        Ok((page, Some(root))) => (page, root),
        Ok(_) => return None,
        Err(e) => {
            report(bld, &e);
            return None;
        }
    };
    let size = bld.ui.pages.first().map_or(PAGE, |p| p.size);
    for w in &mut bld.ui.widgets {
        w.hidden = true;
    }
    let first = bld.ui.widgets.len();
    bld.nui.clear();
    layout::lower(&root, size, bld);
    let end = bld.ui.widgets.len();
    paint_range(&mut page, bld, first..end);
    bld.ui.paint_order = Some(order(bld, first));
    Some(Shown {
        page,
        controls,
        first,
        size,
        slots: std::mem::take(&mut bld.nui),
    })
}

impl Shown {
    /// Whether a state the page keeps changed: [`Shown::refresh`] has
    /// something to render again even when no control moved.
    pub(super) fn dirty(&self) -> bool {
        self.page.dirty()
    }

    /// The user pressed (`value` 1) or let go (0) of tap area `w`: the
    /// page's own `start` or `complete` callback runs, and what it wrote
    /// comes back as (UI id, the control's value).
    pub(super) fn tap(&mut self, w: &ui::Widget, value: f64) -> Vec<(i32, i64)> {
        let mut words = w
            .name
            .strip_prefix(TAP)
            .unwrap_or_default()
            .split_whitespace();
        let (start, complete) = (words.next(), words.next());
        let Some(n) = (if value > 0. { start } else { complete }).and_then(|n| n.parse().ok())
        else {
            return Vec::new();
        };
        let at = [w.rect.width / 2., w.rect.height / 2.];
        let writes = (n > 0)
            .then(|| self.page.call(n, at, [w.rect.width, w.rect.height]).ok())
            .flatten()
            .unwrap_or_default();
        writes
            .into_iter()
            .filter_map(|(id, raw)| {
                let (ui_id, _) = self.controls.iter().find(|(_, name)| *name == id)?;
                Some((*ui_id, raw.round() as i64))
            })
            .collect()
    }

    /// Takes the controls in `decls` as they are now: the instances that
    /// read one that moved render again, each back in its room, and the
    /// page's widgets come back; nothing when nothing it reads moved.
    pub(super) fn refresh(
        &mut self,
        bld: &mut Builder<'_>,
        face: &Face,
        decls: &[Decl<'_>],
    ) -> Vec<ui::WidgetRef> {
        let (controls, values) = read(face, decls);
        let out = match self.page.update(&values, &controls) {
            Ok(out) if !out.is_empty() => out,
            Ok(_) => return Vec::new(),
            Err(e) => {
                report(bld, &e);
                return Vec::new();
            }
        };
        bld.nui = std::mem::take(&mut self.slots);
        let mut dirty_ranges: Vec<(usize, usize)> = Vec::new();
        let mut structural = false;
        for (path, node) in &out {
            let Some(spliced) = layout::splice(bld, self.first, self.size, path, node.as_ref())
            else {
                continue;
            };
            if spliced.shift != 0 || spliced.z_changed || path.is_empty() {
                structural = true;
            }
            paint_range(&mut self.page, bld, spliced.start..spliced.end);
            if path.is_empty() {
                dirty_ranges.clear();
                dirty_ranges.push((spliced.start, spliced.end));
                continue;
            }
            if spliced.shift != 0 {
                let mut adjusted = Vec::with_capacity(dirty_ranges.len() + 1);
                for (a, b) in dirty_ranges {
                    if b <= spliced.start {
                        adjusted.push((a, b));
                    } else if a >= spliced.old_end {
                        let na = (a as isize + spliced.shift).max(0) as usize;
                        let nb = (b as isize + spliced.shift).max(na as isize) as usize;
                        adjusted.push((na, nb));
                    }
                }
                dirty_ranges = adjusted;
            }
            if spliced.start < spliced.end {
                dirty_ranges.push((spliced.start, spliced.end));
            }
        }
        self.slots = std::mem::take(&mut bld.nui);
        if structural || bld.ui.paint_order.is_none() {
            bld.ui.paint_order = Some(order(bld, self.first));
        }
        let mut dirty: Vec<ui::WidgetRef> = dirty_ranges
            .into_iter()
            .flat_map(|(a, b)| (a..b).map(ui::WidgetRef))
            .collect();
        dirty.sort_unstable();
        dirty.dedup();
        dirty
    }
}

/// The page's widgets from `first` on, back to front: in place order, an
/// open popover's (`z` 1) over the rest.
fn order(bld: &Builder<'_>, first: usize) -> Vec<ui::WidgetRef> {
    let mut all: Vec<_> = (first..bld.ui.widgets.len()).map(ui::WidgetRef).collect();
    all.sort_by_key(|w| bld.ui.widgets.get(w.0).map_or(0, |w| w.z));
    all
}

/// Kontakt 8's instrument page when the script sets none.
const PAGE: ui::Size = ui::Size {
    width: 970.,
    height: 600.,
};

impl super::Live {
    /// The user pressed (`value` 1) or let go (0) of `widget`, a native_ui
    /// page's tap area: the controls the page set, as (UI id, value);
    /// `None` for any other widget.
    pub fn tap(&mut self, widget: usize, value: f64) -> Option<Vec<(i32, i64)>> {
        let w = (self.tree.widgets.get_mut(widget)).filter(|w| w.name.starts_with(TAP))?;
        // Held while pressed, as a momentary button is: its release is a
        // change the editor reports.
        w.initial_value = value;
        Some(self.native.as_mut()?.tap(w, value))
    }
}
