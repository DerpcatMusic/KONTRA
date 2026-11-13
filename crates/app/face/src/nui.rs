//! A Kontakt 8 `native_ui` page in the tree: rendered over the script's
//! controls ([`kontra_lua::nui`]), laid out ([`layout`]) in place of their
//! own widgets, and kept current: a control's change renders again only
//! the component instances that read it, each back in its own room.
//!
//! Not copied: `Canvas` paths, `TextInput`, popovers. They lay out but
//! draw nothing.

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
        normalized: range && max > min,
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
    let (page, root) = match rendered {
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
    bld.ui.paint_order = Some((first..bld.ui.widgets.len()).map(ui::WidgetRef).collect());
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
        for (path, node) in &out {
            layout::splice(bld, self.first, self.size, path, node.as_ref());
        }
        self.slots = std::mem::take(&mut bld.nui);
        let all: Vec<_> = (self.first..bld.ui.widgets.len())
            .map(ui::WidgetRef)
            .collect();
        bld.ui.paint_order = Some(all.clone());
        all
    }
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
