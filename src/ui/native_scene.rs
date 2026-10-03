//! Render the supported native scene primitives; assets never use classic-picture guesses.
use super::Cx;
use crate::native_ui::{DropPayload, Node, Paint, Scene};
use crate::plugin::SamplerParams;
use std::sync::{Arc, Mutex};
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
    typed_drop(cx.p, ui, part, epoch, &scene);
    gesture(cx, part, epoch, &scene, |scene| {
        scene.hover(ui, &format!("native-ui-{part}"))
    });
    gesture(cx, part, epoch, &scene, |scene| {
        scene.pointer_gestures(ui, &format!("native-ui-{part}"))
    });
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
        let el = if node.hover || node.drop { el.tracks_pointer() } else { el };
        let id = format!("native-ui-{part}-{}", node.id);
        if node.tap {
            let response = ui.get(id.as_str());
            if response.key_activated {
                gesture(cx, part, epoch, scene, |scene| scene.tap(node.id));
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

// Lua stays on the editor thread; release its mutex before locking Shared or
// enqueueing exact-slot, epoch-checked KSP edits.
fn gesture(
    cx: &mut Cx,
    part: usize,
    epoch: u64,
    scene: &std::sync::Arc<std::sync::Mutex<crate::native_ui::Scene>>,
    run: impl FnOnce(&mut crate::native_ui::Scene) -> anyhow::Result<Vec<crate::native_ui::Edit>>,
) {
    let (edits, failure) = {
        let mut scene = scene.lock().unwrap();
        match run(&mut scene) {
            Ok(edits) => (edits, None),
            Err(error) => (scene.take_edits(), Some(format!("Native UI gesture failed: {error:#}"))),
        }
    };
    publish_edits(cx.p, part, epoch, scene, edits, failure);
}

fn current(p: &SamplerParams, part: usize, epoch: u64, scene: &Arc<Mutex<Scene>>) -> bool {
    p.shared
        .view
        .lock()
        .unwrap()
        .parts
        .get(part)
        .is_some_and(|view| {
            epoch != 0
                && view.script_epoch == epoch
                && view
                    .native_scene
                    .as_ref()
                    .is_some_and(|active| Arc::ptr_eq(active, scene))
        })
}

fn publish_edits(
    p: &SamplerParams,
    part: usize,
    epoch: u64,
    scene: &Arc<Mutex<Scene>>,
    edits: Vec<crate::native_ui::Edit>,
    mut failure: Option<String>,
) {
    for edit in edits {
        if !p
            .shared
            .edit_control_slot(part, epoch, edit.slot, edit.control, edit.value)
        {
            failure = Some("Native UI control edit rejected: stale source or full queue".into());
        }
    }
    if let Some(message) = failure {
        let mut view = p.shared.view.lock().unwrap();
        let path = view
            .parts
            .get_mut(part)
            .filter(|view| {
                view.script_epoch == epoch
                    && view
                        .native_scene
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, scene))
            })
            .and_then(|view| {
                view.native_status = message.clone();
                view.instrument.as_ref().map(|i| i.path.clone())
            });
        drop(view);
        if let Some(path) = path {
            crate::diagnostics::resource(&path, "native_ui/gesture", &message);
        }
    }
}

/// The editor's typed drag producer supplies its actual formats and optional
/// source identity. No library-specific internal clipboard format is invented.
pub(crate) fn typed_drop(
    p: &SamplerParams,
    ui: &mut Ui,
    part: usize,
    epoch: u64,
    scene: &Arc<Mutex<Scene>>,
) {
    let prefix = format!("native-ui-{part}");
    let Some((source, target)) = ui
        .dropped()
        .filter(|(_, target)| target.starts_with(&format!("{prefix}-")))
        .map(|(source, target)| (source.to_owned(), target.to_owned()))
    else {
        return;
    };
    let Some(payload) = ui.dropped_on::<DropPayload>(target.as_str()) else {
        return;
    };
    let accepted = ui.pointer().pos.is_some_and(|position| {
        drop_payload(
            p,
            ui,
            part,
            epoch,
            scene,
            &prefix,
            &target,
            position,
            payload.clone(),
            true,
        )
    });
    if !accepted {
        ui.start_drag(source, payload);
    }
}

pub(crate) fn drop_payload(
    p: &SamplerParams,
    ui: &Ui,
    part: usize,
    epoch: u64,
    scene: &Arc<Mutex<Scene>>,
    prefix: &str,
    target: &str,
    position: Point,
    payload: DropPayload,
    dropped: bool,
) -> bool {
    if !position.x.is_finite()
        || !position.y.is_finite()
        || ui.target_at(position).is_none_or(|(hit, _)| hit != target)
    {
        return false;
    }
    if !current(p, part, epoch, scene) {
        return false;
    }
    if let Some((origin_part, origin_epoch, origin_scene)) = &payload.origin {
        if !origin_scene
            .upgrade()
            .is_some_and(|origin| current(p, *origin_part, *origin_epoch, &origin))
        {
            return false;
        }
    }
    let (edits, failure) = {
        let mut scene = scene.lock().unwrap();
        let Some(owner) = scene.drop_target(ui, prefix, target, &payload) else {
            return false;
        };
        if !dropped {
            return true;
        }
        match scene.perform_drop(ui, prefix, owner, position, payload) {
            Ok(edits) => (edits, None),
            Err(error) => (
                scene.take_edits(),
                Some(format!("Native UI drop failed: {error:#}")),
            ),
        }
    };
    publish_edits(p, part, epoch, scene, edits, failure);
    true
}

pub(crate) fn files(
    p: &SamplerParams,
    ui: &Ui,
    position: Point,
    paths: &[std::path::PathBuf],
    dropped: bool,
) -> bool {
    let Some((target, _)) = ui.target_at(position) else {
        return false;
    };
    let Some(part) = target
        .strip_prefix("native-ui-")
        .and_then(|name| name.split('-').next())
        .and_then(|part| part.parse::<usize>().ok())
    else {
        return false;
    };
    let Some((epoch, scene)) = p
        .shared
        .view
        .lock()
        .unwrap()
        .parts
        .get(part)
        .and_then(|view| {
            view.native_scene
                .clone()
                .map(|scene| (view.script_epoch, scene))
        })
    else {
        return false;
    };
    let payload = match DropPayload::files(paths) {
        Ok(payload) => payload,
        Err(error) => {
            if dropped {
                publish_edits(
                    p,
                    part,
                    epoch,
                    &scene,
                    Vec::new(),
                    Some(format!("Native UI file drop failed: {error:#}")),
                );
            }
            return false;
        }
    };
    drop_payload(
        p,
        ui,
        part,
        epoch,
        &scene,
        &format!("native-ui-{part}"),
        target,
        position,
        payload,
        dropped,
    )
}

// An annular sector uses exact circular path segments, not sampled polygons.
// A positive degree sweep follows screen coordinates clockwise from 3 o'clock.
fn ring_path(size: Size, start: f64, angle: f64, thickness: f64) -> Option<Path> {
    use moose::mui::mui::geometry::{Arc as CircularArc, PathCommand as Cmd};
    let radius = size.width.min(size.height) / 2.;
    if radius <= 0. || angle == 0. || thickness == 0. { return None; }
    let center = Point::new(size.width / 2., size.height / 2.);
    let start = start.to_radians();
    let sweep = angle.to_radians();
    let point = |radius: f64, angle: f64| center + Vec2::new(angle.cos() * radius, angle.sin() * radius);
    let outer_start = point(radius, start);
    let outer_end = point(radius, start + sweep);
    let mut commands = vec![Cmd::MoveTo(outer_start), Cmd::ArcTo(CircularArc { center, radius, start_angle: start, sweep, to: outer_end })];
    let inner = (radius - thickness).max(0.);
    if inner > 0. {
        commands.push(Cmd::LineTo(point(inner, start + sweep)));
        commands.push(Cmd::ArcTo(CircularArc { center, radius: inner, start_angle: start + sweep, sweep: -sweep, to: point(inner, start) }));
    } else { commands.push(Cmd::LineTo(center)); }
    commands.push(Cmd::Close);
    Some(Path { commands })
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
        Paint::Ring { color, start, angle, thickness } => {
            let (color, start, angle, thickness) = (*color, *start, *angle, *thickness);
            // Ring was renamed Arc in NI's current API. Preserve authored
            // degree units and clockwise sweep; MUI owns aspect-fit layout.
            canvas(move |size| ring_path(size, start, angle, thickness).map_or_else(Vec::new, |path| vec![Draw::fill(path, Color::srgba(color[0], color[1], color[2], color[3]))]))
                .full().aspect_fit(1.)
        },
        Paint::Stack | Paint::HStack(_) | Paint::VStack(_) => {
            let el = match &node.paint {
                Paint::HStack(spacing) => row(children).gap(*spacing).align(node.alignment.1),
                Paint::VStack(spacing) => col(children).gap(*spacing).align(node.alignment.0),
                _ => stack(children),
            };
            el.w(node.width.map_or(Len::Auto, Len::Px))
                .h(node.height.map_or(Len::Auto, Len::Px)).pad(node.padding)
                // Native layout scopes have rectangular bounds. Their empty
                // frame area must not acquire MUI's themed rounded hit contour.
                .radius(0.)
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
    let el = el.min_size(Size::new(node.minimum[0].unwrap_or(0.),node.minimum[1].unwrap_or(0.)));
    let el = if node.maximum.iter().any(Option::is_some) {
        //4096 is the existing admitted scene bound, not a made-up infinite
        // pixel extent. The unspecified axis retains every admitted size.
        el.max_size(Size::new(node.maximum[0].unwrap_or(4096.),node.maximum[1].unwrap_or(4096.)))
    } else { el };
    // NI opacity applies to each painted descendant, not a flattened stack.
    // Carry the product through composition and let MUI blend each primitive.
    let el = if !matches!(node.paint, Paint::Stack | Paint::HStack(_) | Paint::VStack(_)) && node.opacity != 1. {
        el.opacity(node.opacity)
    } else { el };
    let el = el.offset(node.offset[0], node.offset[1]);
    let el = if let Some(rotation) = node.rotation { el.rotation(rotation) } else { el };
    let el = if node.disabled { el.disabled() } else { el };
    if node.hidden { el.opacity(0.).disabled() } else { el }
}
#[cfg(test)]
pub(crate) fn paint_snapshot(node: &Node, offered: (f64, f64)) -> El {
    { let _ = offered; scene_tree(node, &crate::native_assets::Assets::default()) }
}
pub(crate) fn scene_tree(node: &Node, assets: &crate::native_assets::Assets) -> El {
    scene_tree_at(node, assets, "native-snapshot")
}

pub(crate) fn scene_tree_at(node: &Node, assets: &crate::native_assets::Assets, prefix: &str) -> El {
    let children = node
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let anchor = node.anchor.or(child.anchor).unwrap_or(node.alignment);
            let el = scene_tree_at(child, assets, prefix).anchor(anchor.0, anchor.1);
            if node.background && index==0 { el.underlay() } else if node.overlay && index==1 { el.overlay() } else { el }
        })
        .collect();
    let el = primitive(node, children, assets);
    let el = if node.hover || node.drop { el.tracks_pointer() } else { el };
    let el = if node.tap { el.cursor(Cursor::Hand).focusable().a11y(A11y::Button).named("Native UI pointer target") } else { el };
    el.id(format!("{prefix}-{}", node.id))
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
