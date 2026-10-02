//! Render the supported native scene primitives; assets never use classic-picture guesses.
use super::Cx;
use crate::native_ui::{Node, Paint};
use moose::mui::mui::prelude::*;

pub(super) fn view(ui: &mut Ui, cx: &mut Cx, part: usize) -> Option<El> {
    let scene = cx.view.parts[part].native_scene.clone()?;
    let epoch = cx.view.parts[part].script_epoch;
    let snapshot = { scene.lock().unwrap().snapshot() };
    let root = match snapshot {
        Ok(root) => root,
        Err(error) => {
            let message = format!("Native UI scene unavailable: {error:#}");
            let mut view = cx.p.shared.view.lock().unwrap();
            if let Some(v) = view.parts.get_mut(part).filter(|v| v.script_epoch == epoch) {
                v.native_status = message.clone();
                v.native_scene = None;
            }
            drop(view);
            if let Some(i) = super::instrument::instrument_of(cx, part) {
                crate::diagnostics::resource(&i.path, "native_ui", &message);
            }
            return None;
        }
    };
    if let Err(error) =
        cx.p.shared
            .request_native_assets(part, epoch, root.asset_names())
    {
        let mut view = cx.p.shared.view.lock().unwrap();
        if let Some(v) = view.parts.get_mut(part).filter(|v| v.script_epoch == epoch) {
            v.native_status = error;
        }
    }
    let assets = scene.lock().unwrap().assets.clone();
    fn draw(
        ui: &mut Ui,
        cx: &mut Cx,
        part: usize,
        epoch: u64,
        scene: &std::sync::Arc<std::sync::Mutex<crate::native_ui::Scene>>,
        node: &Node,
        offered: (f64, f64),
        assets: &crate::native_assets::Assets,
    ) -> El {
        let natural = intrinsic(node, assets, offered);
        let (w, h) = (
            node.width.unwrap_or(natural.0),
            node.height.unwrap_or(natural.1),
        );
        let children = node
            .children
            .iter()
            .map(|child| {
                draw(ui, cx, part, epoch, scene, child, (w, h), assets)
                    .anchor(node.alignment.0, node.alignment.1)
            })
            .collect();
        let el = primitive(node, w, h, children, assets);
        let id = format!("native-ui-{part}-{}", node.id);
        if node.tap {
            let response = ui.get(id.as_str());
            if response.clicked || response.key_activated {
                let (edits, error) = {
                    let mut scene = scene.lock().unwrap();
                    match scene.tap(node.id) {
                        Ok(edits) => (edits, None),
                        Err(error) => (
                            scene.take_edits(),
                            Some(format!("Native UI gesture failed: {error:#}")),
                        ),
                    }
                };
                let mut failure = error;
                for edit in edits {
                    if !cx.p.shared.edit_control_slot(
                        part,
                        epoch,
                        edit.slot,
                        edit.control,
                        edit.value,
                    ) {
                        failure = Some(
                            "Native UI control edit rejected: stale source or full queue".into(),
                        );
                    }
                }
                if let Some(message) = failure {
                    let mut view = cx.p.shared.view.lock().unwrap();
                    if let Some(v) = view.parts.get_mut(part).filter(|v| v.script_epoch == epoch) {
                        v.native_status = message.clone();
                    }
                    drop(view);
                    if let Some(i) = super::instrument::instrument_of(cx, part) {
                        crate::diagnostics::resource(&i.path, "native_ui/gesture", &message);
                    }
                }
            }
            el.cursor(Cursor::Hand)
                .focusable()
                .a11y(A11y::Button)
                .named("Native UI tap target")
                .id(id)
        } else {
            el.id(id)
        }
    }
    let size = (root.width?, root.height?);
    Some(
        draw(ui, cx, part, epoch, &scene, &root, size, &assets)
            .clip()
            .shrink(0)
            .named("Native UI subset scene"),
    )
}

// Both native editor rendering and the authored headless gate use this mapping.
fn intrinsic(
    node: &Node,
    assets: &crate::native_assets::Assets,
    offered: (f64, f64),
) -> (f64, f64) {
    if let Paint::Asset(reference) = &node.paint
        && !reference.resizable
        && let Some(Ok(asset)) = assets.entries.get(&reference.resource)
    {
        return asset.size();
    }
    offered
}
fn primitive(
    node: &Node,
    w: f64,
    h: f64,
    children: Vec<El>,
    assets: &crate::native_assets::Assets,
) -> El {
    match &node.paint {
        Paint::Text(value) => text(value.clone())
            .text_size(node.text_size)
            .lines(1)
            .fill(Color::srgba(
                node.text_color[0], node.text_color[1], node.text_color[2], node.text_color[3],
            ))
            .w(node.width.map_or(Len::Auto, Len::Px))
            .h(node.height.map_or(Len::Auto, Len::Px)),
        Paint::Rectangle(color) => {
            block(w, h).fill(Color::srgba(color[0], color[1], color[2], color[3]))
        }
        Paint::Stack => stack(children)
            .w(node.width.map_or(Len::Auto, Len::Px))
            .h(node.height.map_or(Len::Auto, Len::Px)),
        Paint::Asset(reference) => match assets.entries.get(&reference.resource) {
            Some(Ok(asset)) => match asset.as_ref() {
                crate::native_assets::Asset::Vector(vector) => {
                    block(w, h).fill(Fill::Vector(vector.clone(), Fit::Fill))
                }
                crate::native_assets::Asset::Bitmap(image) => {
                    block(w, h).fill(Fill::Image(image.clone(), Fit::Fill))
                }
            },
            _ => block(w, h), // Pending/failed assets never become fabricated artwork.
        },
    }
}
#[cfg(test)]
pub(crate) fn paint_snapshot(node: &Node, offered: (f64, f64)) -> El {
    paint_snapshot_assets(node, offered, &crate::native_assets::Assets::default())
}
#[cfg(test)]
pub(crate) fn paint_snapshot_assets(
    node: &Node,
    offered: (f64, f64),
    assets: &crate::native_assets::Assets,
) -> El {
    let natural = intrinsic(node, assets, offered);
    let (w, h) = (
        node.width.unwrap_or(natural.0),
        node.height.unwrap_or(natural.1),
    );
    let children = node
        .children
        .iter()
        .map(|child| {
            paint_snapshot_assets(child, (w, h), assets).anchor(node.alignment.0, node.alignment.1)
        })
        .collect();
    primitive(node, w, h, children, assets).id(format!("native-snapshot-{}", node.id))
}
