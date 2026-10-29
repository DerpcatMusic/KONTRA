//! A UVI script's own page as a `kontra_ui_model` tree: the widgets it made
//! as it loaded ([`kontra_lua::Ui`]), then kept current with what the engine
//! reads of them ([`WidgetLook`]). A widget's index is its number to the
//! engine ([`ui::Widget::source_id`]).

use std::collections::HashMap;

use kontra_engine::{Engine, Text, WidgetLook};
use kontra_lua::{Ui, Widget};
use kontra_ui_model as ui;

/// The tree of `page`; `picture` gives the layout of a script picture path.
#[must_use]
pub fn tree(page: &Ui, picture: &dyn Fn(&str) -> Option<ui::ImageMeta>) -> ui::Interface {
    let mut assets = HashMap::new();
    let mut tree = ui::Interface {
        source: ui::Source::FalconLua,
        performance: page.performance,
        ..Default::default()
    };
    let mut asset = |tree: &mut ui::Interface, path: &str, frames: u32| {
        let at = *assets.entry((path.to_owned(), frames)).or_insert_with(|| {
            let mut meta = picture(path).unwrap_or_default();
            // A strip's frames stack down; each is drawn at the widget's size.
            meta.frames = frames;
            meta.axis = ui::Orientation::Vertical;
            meta.size = meta.size.map(|s| ui::Size {
                width: s.width,
                height: (s.height / f64::from(frames)).floor(),
            });
            meta.stretch = [true; 2];
            tree.assets.push(ui::Asset {
                path: path.to_owned(),
                kind: ui::AssetKind::Image(meta),
            });
            tree.assets.len() - 1
        });
        let size = match tree.assets.get(at).map(|a| &a.kind) {
            Some(ui::AssetKind::Image(m)) => m.size,
            _ => None,
        };
        (ui::AssetRef(at), size)
    };
    let background = page.background.as_deref().map(|p| asset(&mut tree, p, 1).0);
    tree.pages.push(ui::Page {
        name: "Performance".into(),
        size: ui::Size {
            width: page.width,
            height: page.height,
        },
        height_rows: None,
        background: ui::Background {
            color: page.colour.as_deref().and_then(colour),
            image: background,
            ..Default::default()
        },
    });
    for (n, w) in page.widgets.iter().enumerate() {
        let mut out = widget(w, n);
        // Pictures by role: the first is the one drawn (see `swap`).
        let roles = [
            (
                w.strip.as_ref().map(|(p, f)| (p.as_str(), *f)),
                ui::Role::Strip,
            ),
            (w.image.as_deref().map(|p| (p, 1)), ui::Role::Background),
            (
                w.background.as_deref().map(|p| (p, 1)),
                ui::Role::Background,
            ),
            (w.normal.as_deref().map(|p| (p, 1)), ui::Role::Background),
            (w.pressed.as_deref().map(|p| (p, 1)), ui::Role::Pressed),
        ];
        let mut own = None;
        for (path, role) in roles {
            if let Some((path, frames)) = path {
                let (at, size) = asset(&mut tree, path, frames);
                own = own.or(size);
                out.images.push(ui::ImageUse::new(at, role));
            }
        }
        // An axis the script left open takes the picture's, else the kind's.
        match own {
            Some(size) => {
                if !w.sized[0] {
                    out.rect.width = size.width;
                }
                if !w.sized[1] {
                    out.rect.height = size.height;
                }
            }
            None if w.sized != [true; 2] => {
                out.auto_size = true;
                out.default_axes = [!w.sized[0], !w.sized[1]];
            }
            None => {}
        }
        // A widget says its name unless the script gave it words or a picture.
        if out.text.is_empty() && out.images.is_empty() {
            match out.kind {
                ui::Kind::Label | ui::Kind::Button { .. } => out.text.clone_from(&w.name),
                _ => {}
            }
        }
        swap(&mut out);
        tree.widgets.push(out);
    }
    tree
}

/// The model widget for `w`, number `n`, without its pictures.
fn widget(w: &Widget, n: usize) -> ui::Widget {
    let range = ui::Range {
        min: w.min,
        max: w.max,
        default: w.default.unwrap_or(w.value),
        step: w.integer.then_some(1.),
    };
    let kind = match w.kind.as_str() {
        "Knob" => ui::Kind::Knob {
            range,
            display: ui::Display {
                unit: w.unit.clone(),
                ..Default::default()
            },
        },
        "Slider" => ui::Kind::Slider {
            range,
            orientation: if w.vertical {
                ui::Orientation::Vertical
            } else {
                ui::Orientation::Horizontal
            },
        },
        "NumBox" => ui::Kind::ValueEdit {
            range,
            display: ui::Display::default(),
            arrows: false,
        },
        "Button" => ui::Kind::Button { momentary: true },
        "OnOffButton" => ui::Kind::Button { momentary: false },
        "Menu" | "MultiStateButton" => ui::Kind::Menu {
            items: (1..)
                .zip(&w.items)
                .map(|(value, text)| ui::MenuItem {
                    text: text.clone(),
                    value,
                    visible: true,
                })
                .collect(),
        },
        "Label" => ui::Kind::Label,
        "Image" => ui::Kind::Image,
        // ponytail: tables, XY pads, meters and wave views draw as empty
        // panels; give each its kind when a library's page needs it.
        _ => ui::Kind::Panel,
    };
    let [x, y, width, height] = w.rect;
    let mut out = ui::Widget::new(
        w.name.clone(),
        ui::PageRef(0),
        ui::Rect {
            x,
            y,
            width,
            height,
        },
        kind,
    );
    out.source_id = i32::try_from(n).ok();
    out.parent = w.parent.map(ui::WidgetRef);
    out.tooltip.clone_from(&w.tooltip);
    out.opacity = w.alpha.clamp(0., 1.) as f32;
    out.menu_cycle = w.kind == "MultiStateButton";
    out.colors.background = w.colours[0].as_deref().and_then(colour);
    out.colors.bar = w.colours[2].as_deref().and_then(colour);
    if matches!(out.kind, ui::Kind::Label | ui::Kind::Button { .. }) {
        out.text.clone_from(&w.text);
    }
    out.value = value_of(&out.kind, w.value);
    out.initial_value = w.value;
    out.hidden = !w.visible;
    out.enabled = w.enabled;
    out
}

/// A control's value as the editor keeps it: a menu's or button's whole.
fn value_of(kind: &ui::Kind, v: f64) -> Option<ui::Value> {
    match kind {
        ui::Kind::Knob { .. } | ui::Kind::Slider { .. } | ui::Kind::ValueEdit { .. } => {
            Some(ui::Value::Real(v))
        }
        ui::Kind::Menu { .. } | ui::Kind::Button { .. } => Some(ui::Value::Integer(v as i32)),
        _ => None,
    }
}

/// A button draws its pressed picture while on, its normal one while off.
fn swap(w: &mut ui::Widget) {
    let on = matches!(w.value, Some(ui::Value::Integer(v)) if v != 0);
    if let Some(at) = (w.images.iter()).position(|i| (i.role == ui::Role::Pressed) == on) {
        w.images.swap(0, at);
    }
}

/// Brings widget `n` to `look` and `text`; whether it changed.
pub fn apply(tree: &mut ui::Interface, n: usize, look: WidgetLook, text: Option<&str>) -> bool {
    let Some(w) = tree.widgets.get_mut(n) else {
        return false;
    };
    let before = (
        w.value.clone(),
        w.hidden,
        w.enabled,
        w.images.first().copied(),
    );
    w.value = value_of(&w.kind, look.value).or(w.value.take());
    w.hidden = !look.visible;
    w.enabled = look.enabled;
    swap(w);
    let mut changed = before
        != (
            w.value.clone(),
            w.hidden,
            w.enabled,
            w.images.first().copied(),
        );
    if let Some(text) = text.filter(|t| *t != w.text) {
        w.text = text.to_owned();
        changed = true;
    }
    changed
}

/// `RRGGBB` or `AARRGGBB`, with or without `#`.
fn colour(hex: &str) -> Option<ui::Rgba> {
    let hex = hex.trim_start_matches('#');
    let n = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(ui::Rgba::rgb(n)),
        8 => Some(ui::Rgba {
            a: (n >> 24) as u8,
            ..ui::Rgba::rgb(n & 0xFF_FFFF)
        }),
        _ => None,
    }
}

/// What the editor and a project save read of a script's own page (a Lua
/// one) through its engine.
pub trait Page {
    /// Widget `n` of script `slot`'s page, as it stands.
    fn widget(&self, slot: u8, n: u32) -> Option<WidgetLook>;
    /// Widget `n`'s text into `out`; false when it has none.
    fn widget_text(&self, slot: u8, n: u32, out: &mut Text) -> bool;
    /// The state script `slot` saves for a project, into `out` (a Lua
    /// script's `onSave`); its length.
    fn save_state(&mut self, slot: u8, out: &mut [u8]) -> Option<usize>;
}

impl Page for Engine {
    fn widget(&self, slot: u8, n: u32) -> Option<WidgetLook> {
        self.vm(slot)?.widget(n)
    }

    fn widget_text(&self, slot: u8, n: u32, out: &mut Text) -> bool {
        self.vm(slot).is_some_and(|vm| vm.widget_text(n, out))
    }

    fn save_state(&mut self, slot: u8, out: &mut [u8]) -> Option<usize> {
        self.vm_mut(slot)?.save_state(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_takes_its_pictures_and_a_button_shows_pressed_while_on() {
        let page = kontra_lua::compile(
            r#"setBackgroundColour("1a1f24")
               local p = Panel{"P", bounds={10, 10, 200, 100}}
               local k = p:Knob{"K", 0.5, 0, 1, pos={4, 4}}
               k:setStripImage("../art/k.png", 10)
               OnOffButton{"B", false, normalImage="off.png", pressedImage="on.png", size={20, 10}}"#,
            &[],
            &[],
        )
        .unwrap()
        .ui;
        let png = |_: &str| {
            Some(ui::ImageMeta {
                size: Some(ui::Size {
                    width: 40.,
                    height: 400.,
                }),
                ..Default::default()
            })
        };
        let mut tree = tree(&page, &png);
        let bg = tree.pages[0].background.color;
        assert_eq!(bg, Some(ui::Rgba::rgb(0x1a1f24)));
        let knob = &tree.widgets[1];
        assert_eq!(
            (knob.parent, knob.rect.width, knob.rect.height),
            (Some(ui::WidgetRef(0)), 40., 40.)
        );
        assert_eq!(tree.page_rect(ui::WidgetRef(1)).x, 14.);
        let shown = |t: &ui::Interface| t.widgets[2].images[0].role;
        assert_eq!(shown(&tree), ui::Role::Background);
        let on = WidgetLook {
            value: 1.,
            visible: true,
            enabled: true,
        };
        assert!(apply(&mut tree, 2, on, None));
        assert_eq!(shown(&tree), ui::Role::Pressed);
        assert!(!apply(&mut tree, 2, on, None));
    }
}
