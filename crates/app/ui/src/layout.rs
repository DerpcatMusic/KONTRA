//! What the source left to the renderer, turned into pixels: grid
//! placement, grid page heights and stock control sizes, and a picture's
//! own size on the axes it does not stretch (ported from v2's `ir_view`).

use kontra_ui_model::{AssetKind, Interface, Kind, Orientation, Placement, Role, Source};

/// Kontakt's layout grid (`move_control`): column and row pitch, and the
/// first cell's corner; `set_ui_height` rows are [`GRID_ROW_HEIGHT`] tall.
const GRID: (i32, i32, i32, i32) = (92, 21, 66, 2);
const GRID_ROW_HEIGHT: u32 = 68;

/// A widget's size where its source left it to the host: Kontakt's stock sizes.
// ponytail: KSP stock sizes for every source; per-source tables once Falcon UIs arrive.
fn default_size(kind: &Kind) -> (u32, u32) {
    match kind {
        Kind::Knob { .. } => (85, 52),
        Kind::Table { .. } | Kind::Xy { .. } | Kind::MouseArea => (92, 92),
        Kind::Waveform | Kind::Wavetable { .. } => (184, 92),
        Kind::FileSelector { .. } => (184, 184),
        Kind::LevelMeter {
            orientation: Orientation::Vertical,
            ..
        } => (8, 92),
        Kind::Panel => (0, 0),
        _ => (85, 18),
    }
}

/// Resolves the pages and the widgets `changed` of `tree` in place: after a
/// build, every widget; after a refresh, the ones it rebuilt.
pub fn resolve(tree: &mut Interface, changed: impl IntoIterator<Item = usize>) {
    for p in &mut tree.pages {
        if let Some(rows) = p.height_rows.take() {
            p.size.height = f64::from(rows * GRID_ROW_HEIGHT);
        }
    }
    for n in changed {
        let Some(w) = tree.widgets.get_mut(n) else {
            continue;
        };
        // A label's text keeps its authored bounds even when its picture is smaller.
        let label_bounds =
            if matches!(w.kind, Kind::Label) && (!w.auto_size || w.default_axes != [false; 2]) {
                [
                    !w.default_axes[0] && w.rect.width > 0.,
                    !w.default_axes[1] && w.rect.height > 0.,
                ]
            } else {
                [false; 2]
            };
        if let Placement::Grid { column, row } = w.placement {
            w.rect.x = f64::from((column as i32 - 1) * GRID.0 + GRID.2);
            w.rect.y = f64::from((row as i32 - 1) * GRID.1 + GRID.3);
            w.placement = Placement::Pixels;
        }
        if w.auto_size {
            let (width, height) = default_size(&w.kind);
            let axes = if w.default_axes == [false; 2] {
                [true; 2]
            } else {
                w.default_axes
            };
            if axes[0] {
                w.rect.width = f64::from(width);
            }
            if axes[1] {
                w.rect.height = f64::from(height);
            }
            w.auto_size = false;
            w.default_axes = [false; 2];
        }
        if tree.source == Source::FalconLua {
            continue;
        }
        let meta = w
            .images
            .iter()
            .filter(|i| i.role != Role::Handle)
            .find_map(|i| match &tree.assets.get(i.asset.0)?.kind {
                AssetKind::Image(m) => m.size.map(|s| (s, m.stretch)),
                AssetKind::BitmapFont | AssetKind::TrueTypeFont => None,
            });
        if let Some((size, stretch)) = meta {
            if !stretch[0] && !label_bounds[0] {
                w.rect.width = size.width;
            }
            if !stretch[1] && !label_bounds[1] {
                w.rect.height = size.height;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kontra_ui_model::{
        Asset, AssetKind, ImageMeta, ImageUse, Page, PageRef, Rect, Role, Size, Widget,
    };

    use super::*;

    /// A picture takes its own size on an axis it does not stretch, whatever
    /// width the script gave the control; on an axis it stretches, the
    /// control keeps the script's width. Electric Mint's preset menu is the
    /// case that shows it: 171 px asked for, a 32 px picture, and the
    /// sidecar's `Horizontal Resizable` decides whether the arrow lands at
    /// the field's right end or under the field.
    #[test]
    fn a_control_keeps_its_width_only_along_an_axis_its_picture_stretches() {
        let menu = |stretch: [bool; 2]| {
            let mut tree = Interface {
                pages: vec![Page::default()],
                ..Interface::default()
            };
            tree.assets.push(Asset {
                path: "Resources/pictures/dropdown_topright.png".into(),
                kind: AssetKind::Image(ImageMeta {
                    frames: 1,
                    size: Some(Size {
                        width: 32.,
                        height: 21.,
                    }),
                    stretch,
                    ..ImageMeta::default()
                }),
            });
            let mut w = Widget::new(
                "$preset",
                PageRef(0),
                Rect::new(446, 32, 171, 21),
                Kind::Menu { items: Vec::new() },
            );
            w.images
                .push(ImageUse::new(kontra_ui_model::AssetRef(0), Role::Strip));
            tree.widgets.push(w);
            resolve(&mut tree, 0..1);
            tree.widgets.first().map(|w| (w.rect.width, w.rect.height))
        };
        assert_eq!(menu([true, false]), Some((171., 21.)), "stretched across");
        assert_eq!(
            menu([false, false]),
            Some((32., 21.)),
            "a picture that does not stretch takes its own width"
        );
    }

    #[test]
    fn grid_rows_stock_sizes_and_grid_places_become_pixels() {
        let mut tree = Interface {
            pages: vec![Page {
                height_rows: Some(3),
                ..Page::default()
            }],
            ..Interface::default()
        };
        let mut knob = Widget::new(
            "$knob",
            PageRef(0),
            Rect::default(),
            Kind::Knob {
                range: kontra_ui_model::Range::default(),
                display: kontra_ui_model::Display::default(),
            },
        );
        knob.auto_size = true;
        knob.placement = Placement::Grid { column: 2, row: 3 };
        tree.widgets.push(knob);
        resolve(&mut tree, 0..1);
        assert_eq!(tree.pages.first().map(|p| p.size.height), Some(204.));
        assert_eq!(
            tree.widgets.first().map(|w| w.rect),
            Some(Rect::new(158, 44, 85, 52))
        );
    }
}
