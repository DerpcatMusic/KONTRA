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
        assets: &crate::native_assets::Assets,
    ) -> El {
        let children = node
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                // A placement guide also governs the transparent scope when it is
                // offered a box smaller than its intrinsic child (e.g. overlay).
                let anchor = node.anchor.or(child.anchor).unwrap_or(node.alignment);
                let el = draw(ui, cx, part, epoch, scene, child, assets)
                    .anchor(anchor.0, anchor.1);
                if node.background && index==0 { el.underlay() } else if node.overlay && index==1 { el.overlay() } else { el }
            })
            .collect();
        let el = primitive(node, children, assets);
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
    Some(
        draw(ui, cx, part, epoch, &scene, &root, &assets)
            .clip()
            .shrink(0)
            .named("Native UI subset scene"),
    )
}

// Both native editor rendering and the authored headless gate use this mapping.
fn primitive(
    node: &Node,
    children: Vec<El>,
    assets: &crate::native_assets::Assets,
) -> El {
    let el = match &node.paint {
        Paint::Text(value) => {
            let el = text(value.clone())
                .text_size(node.text_size)
                .text_weight(if node.text_bold { Weight::BOLD } else { Weight::REGULAR })
                .fill(Color::srgba(
                    node.text_color[0], node.text_color[1], node.text_color[2], node.text_color[3],
                ))
                .w(node.width.map_or(Len::Auto, Len::Px))
                .h(node.height.map_or(Len::Auto, Len::Px));
            if let Some(lines) = node.text_lines { el.lines(lines) } else { el }
        },
        Paint::Rectangle(color, radius) => block(0, 0)
            .w(node.width.map_or(Len::Pct(100.), Len::Px))
            .h(node.height.map_or(Len::Pct(100.), Len::Px))
            .radius(*radius)
            .fill(Color::srgba(color[0], color[1], color[2], color[3])),
        Paint::Stack | Paint::HStack(_) | Paint::VStack(_) => {
            let el = match &node.paint {
                Paint::HStack(spacing) => row(children).gap(*spacing).align(node.alignment.1),
                Paint::VStack(spacing) => col(children).gap(*spacing).align(node.alignment.0),
                _ => stack(children),
            };
            el.w(node.width.map_or(Len::Auto, Len::Px))
                .h(node.height.map_or(Len::Auto, Len::Px)).pad(node.padding)
        },
        Paint::Asset(reference) => {
            let asset = assets.entries.get(&reference.resource).and_then(|a|a.as_ref().ok());
            let size = asset.map_or((0.,0.),|a|a.size());
            let width = node.width.map_or(if reference.resizable { Len::Pct(100.) } else { Len::Px(size.0) },Len::Px);
            let height = node.height.map_or(if reference.resizable { Len::Pct(100.) } else { Len::Px(size.1) },Len::Px);
            let el = block(0,0).w(width).h(height);
            match asset {
            Some(asset) => match asset.as_ref() {
                crate::native_assets::Asset::Vector(vector) => {
                    el.fill(Fill::Vector(vector.clone(), Fit::Fill))
                }
                crate::native_assets::Asset::Bitmap(image) => {
                    el.fill(Fill::Image(image.clone(), Fit::Fill))
                }
            },
            None => el, // Pending/failed assets never become fabricated artwork.
            }
        },
    };
    // NI opacity applies to each painted descendant, not a flattened stack.
    // Carry the product through composition and let MUI blend each primitive.
    let el = if !matches!(node.paint, Paint::Stack | Paint::HStack(_) | Paint::VStack(_)) && node.opacity != 1. {
        el.opacity(node.opacity)
    } else { el };
    let el = el.offset(node.offset[0], node.offset[1]);
    let el = if node.disabled { el.disabled() } else { el };
    if node.hidden { el.opacity(0.).disabled() } else { el }
}
#[cfg(test)]
pub(crate) fn paint_snapshot(node: &Node, offered: (f64, f64)) -> El {
    { let _ = offered; scene_tree(node, &crate::native_assets::Assets::default()) }
}
pub(crate) fn scene_tree(node: &Node, assets: &crate::native_assets::Assets) -> El {
    let children = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let anchor = node.anchor.or(child.anchor).unwrap_or(node.alignment);
            let el = scene_tree(child, assets).anchor(anchor.0, anchor.1);
            if node.background && index==0 { el.underlay() } else if node.overlay && index==1 { el.overlay() } else { el }
        })
        .collect();
    primitive(node, children, assets).id(format!("native-snapshot-{}", node.id))
}

// Admission uses the same font/layout/asset adapter as editor rendering, not
// guessed root dimensions through transparent component wrappers.
pub(crate) fn validate_root(node: &Node, assets: &crate::native_assets::Assets) -> anyhow::Result<()> {
    static FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
    let font = FONT.get_or_init(|| Font::new(&include_bytes!("../../assets/NotoSans.ttf")[..]).expect("embedded native fallback font"));
    let spec = moose::mui::mui::scene::SceneSpec::new(scene_tree(node, assets))
        .font(font.clone());
    let scene = moose::mui::mui::scene::resolve(&spec).map_err(|error|anyhow::anyhow!("Native UI root layout: {error}"))?;
    let size = scene.layout.size;
    anyhow::ensure!(size.width.is_finite() && size.height.is_finite() && size.width>0. && size.height>0. && size.width<=4096. && size.height<=4096.,"Native UI measured root exceeds positive 4096px bounds: {}×{}",size.width,size.height);
    Ok(())
}
