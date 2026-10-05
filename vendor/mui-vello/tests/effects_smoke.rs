//! The retained GPU renderer on a real device. Skips, loudly, when the
//! machine has no wgpu adapter at all; a software adapter still runs it.
#![cfg(feature = "gpu-effects")]

use mui_scene::prelude::*;
use mui_scene::{Image, ResolvedScene};
use mui_vello::effects::{Budget, GpuRenderer};
use mui_vello::kurbo::Affine;
use std::sync::Arc;

const SIZE: [u32; 2] = [320, 200];
/// Far enough right that nothing lands on the target.
const AWAY: Affine = Affine::new([1., 0., 0., 1., 5000., 0.]);

fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
    pollster::block_on(async {
        let adapter =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env())
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .map_err(|e| eprintln!("SKIPPED: no wgpu adapter ({e})"))
                .ok()?;
        adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| eprintln!("SKIPPED: no wgpu device ({e})"))
            .ok()
    })
}

fn target(device: &wgpu::Device) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn welded() -> ResolvedScene {
    let plate = |id: &str| block(80., 60.).radius(12.).fill(Role::Primary).id(id);
    let root = row![plate("a"), plate("b")]
        .gap(16.)
        .weld(Weld::all().reach(24.).blend(60.))
        .id("join")
        .anchor(Align::Start, Align::Start);
    let spec = SceneSpec::new(root).offered(Size::new(320., 200.));
    resolve(&spec.weld_backend(mui_scene::WeldBackend::AnalyticGpu)).unwrap()
}

fn pictured(v: u8) -> (ResolvedScene, std::sync::Weak<[u8]>) {
    let image = Arc::new(Image::rgba(2, 2, vec![v; 16]).unwrap());
    let buffer = Arc::downgrade(&image.rgba);
    let root = block(40., 40.)
        .fill(Fill::Image(image, Fit::Fill))
        .id("img");
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(40., 40.))).unwrap();
    (scene, buffer)
}

async fn hybrid(device: &wgpu::Device, queue: &wgpu::Queue) -> GpuRenderer {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    GpuRenderer::new(device, queue, format, SIZE, Budget::default())
        .await
        .unwrap()
}

/// A weld that scrolls offscreen keeps its texture, so scrolling back neither
/// reallocates nor re-renders it. It used to be evicted the frame it left.
#[test]
fn a_weld_scrolled_away_and_back_is_not_rendered_again() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let scene = welded();
    let first = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!((first.texture_allocations, first.effect_draws), (1, 1));
    let away = r.render(&scene, AWAY, &view).unwrap();
    assert_eq!(away.effect_draws, 0);
    let back = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!(
        (back.texture_allocations, back.effect_draws),
        (0, 0),
        "scrolling back rebuilt the weld texture"
    );
}

/// Once the app drops an image, nothing on the GPU path keeps its buffer.
/// The global pixmap cache and the atlas ids used to hold a clone each, and
/// each waited for the other to let go: every morph bake leaked one.
#[test]
fn a_dropped_image_buffer_is_freed_on_the_gpu_path() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let (a, gone) = pictured(10);
    r.render(&a, Affine::IDENTITY, &view).unwrap();
    drop(a);
    let (b, kept) = pictured(20);
    r.render(&b, Affine::IDENTITY, &view).unwrap();
    assert!(
        gone.upgrade().is_none(),
        "the renderer kept the app's buffer"
    );
    assert!(kept.upgrade().is_some());
}

/// An unchanged scene into the same target costs no encode, no Vello
/// render and no present: the target already holds the frame. A new target
/// (a swapchain's next image) gets only the present pass.
#[test]
fn a_static_frame_neither_encodes_nor_renders() {
    let Some((device, queue)) = device() else {
        return;
    };
    let (view, other) = (target(&device), target(&device));
    let scene = welded();
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let first = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert!(first.encoded_scenes > 0 && first.renders > 0);
    for _ in 0..5 {
        let idle = r.render(&scene, Affine::IDENTITY, &view).unwrap();
        assert_eq!((idle.encoded_scenes, idle.renders), (0, 0));
    }
    let moved = r.render(&scene, Affine::IDENTITY, &other).unwrap();
    assert_eq!((moved.encoded_scenes, moved.renders), (0, 0));
}

fn readable(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The target's pixels, test-side only: the renderer never reads back.
fn pixels(device: &wgpu::Device, queue: &wgpu::Queue, t: &wgpu::Texture) -> Vec<u8> {
    let row = SIZE[0] * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIZE[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        t.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: None,
            },
        },
        t.size(),
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();

    buffer.slice(..).get_mapped_range().unwrap().to_vec()
}

fn at(p: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIZE[0] + x) * 4) as usize;
    [p[i], p[i + 1], p[i + 2], p[i + 3]]
}

/// A host texture named by `Image::texture` is painted from the GPU copy,
/// and a redraw of it shows after `set_texture` with the scene unchanged:
/// the spectrogram path, with no readback and no upload.
#[test]
fn a_host_texture_paints_and_refreshes_without_a_new_scene() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let host = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let fill = |rgba: [u8; 4]| {
        queue.write_texture(
            host.as_image_copy(),
            &rgba.repeat(16),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            host.size(),
        );
    };
    let image = Arc::new(Image::texture(7, 4, 4));
    let root = block(320., 200.)
        .fill(Fill::Image(image, Fit::Fill))
        .id("t");
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    fill([255, 0, 0, 255]);
    r.set_texture(7, &host);
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!(
        at(&pixels(&device, &queue, &out), 160, 100),
        [255, 0, 0, 255]
    );
    fill([0, 0, 255, 255]);
    r.set_texture(7, &host);
    let again = r.render(&scene, Affine::IDENTITY, &view).unwrap();
    assert_eq!((again.encoded_scenes, again.renders), (0, 1));
    assert_eq!(
        at(&pixels(&device, &queue, &out), 160, 100),
        [0, 0, 255, 255]
    );
}

/// A backdrop blurs what is under it: over a hard red/blue edge, the
/// pixels either side of it inside the panel mix, and those outside stay.
#[test]
fn a_backdrop_blurs_the_edge_under_it() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let half = |c: Color| block(160., 200.).fill(Fill::Color(c));
    let root = stack![
        row![half(Color::srgb(1., 0., 0.)), half(Color::srgb(0., 0., 1.))],
        block(320., 100.).backdrop_blur(8.).id("glass"),
    ]
    .id("root");
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let p = pixels(&device, &queue, &out);
    let (inside, outside) = (at(&p, 157, 75), at(&p, 157, 25));
    assert!(
        inside[0] < 250 && inside[2] > 5,
        "no blur at the edge: {inside:?}"
    );
    assert_eq!(outside, [255, 0, 0, 255], "the blur leaked past the panel");
}

/// The in-app Blur knob reaches MUI as a numeric Gaussian sigma: ten points
/// should spread an edge farther than one, rather than acting as on/off.
#[test]
fn backdrop_blur_strength_tracks_one_and_ten_points() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let scene = |sigma| {
        let half = |c: Color| block(160., 200.).fill(Fill::Color(c));
        let root = stack![
            row![half(Color::srgb(1., 0., 0.)), half(Color::srgb(0., 0., 1.))],
            block(320., 100.).backdrop_blur(sigma).id("glass"),
        ]
        .id("root");
        resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap()
    };
    let mut bleed = |sigma| {
        let scene = scene(sigma);
        r.render(&scene, Affine::IDENTITY, &view).unwrap();
        let p = pixels(&device, &queue, &out);
        (140..160).map(|x| u64::from(at(&p, x, 75)[2])).sum::<u64>()
    };
    let (one, ten) = (bleed(1.), bleed(10.));
    assert!(ten > one + 100, "1pt={one}, 10pt={ten}");
}

/// A custom outline's drop shadow is its path blurred through the backdrop
/// pass: shade just past the tip, falling off, none far away.
#[test]
fn a_custom_outline_shadow_blurs_on_the_gpu() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let diamond = |s: Size| {
        let p = mui_geometry::Point::new;
        let (w, h) = (s.width, s.height);
        mui_geometry::Path::default()
            .move_to(p(w / 2., 0.))
            .line_to(p(w, h / 2.))
            .line_to(p(w / 2., h))
            .line_to(p(0., h / 2.))
            .close()
    };
    let white = Fill::Color(Color::srgb(1., 1., 1.));
    let gem = block(100., 100.)
        .outline(diamond)
        .fill(white.clone())
        .shadow(Shadow::soft(8.))
        .id("gem");
    let root = stack![gem].square(200.).center().fill(white).id("root");
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(200., 200.))).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let p = pixels(&device, &queue, &out);
    // The tip is at (100, 150).
    let (near, far) = (at(&p, 100, 153)[0], at(&p, 100, 190)[0]);
    assert!(near < 250, "the custom-path shadow is dropped: {near}");
    assert!(near > 5, "that is a sharp black copy, not a blur: {near}");
    assert_eq!(far, 255, "the shadow reached far past its blur");
}

/// A weld gone from the scene gives its texture back after a few frames,
/// without waiting for budget pressure.
#[test]
fn a_removed_weld_frees_its_texture() {
    let Some((device, queue)) = device() else {
        return;
    };
    let view = target(&device);
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let first = r.render(&welded(), Affine::IDENTITY, &view).unwrap();
    assert!(first.resident_texture_bytes > 0);
    let (plain, _) = pictured(1);
    let mut last = first;
    for _ in 0..=mui_vello::effects::ABSENT_FRAMES {
        assert!(last.resident_texture_bytes > 0, "freed too early");
        last = r.render(&plain, Affine::IDENTITY, &view).unwrap();
    }
    assert_eq!(last.resident_texture_bytes, 0, "the weld texture stayed");
}

/// Most of its paint stays put from one call to the next: buttons with
/// shadows, strokes and labels, a translucent clipped panel at `panel`
/// opacity, and a floating tooltip when `tip`.
fn board(hot: Option<usize>, tip: bool, panel: f32, bg: Role) -> ResolvedScene {
    // One face, as an app holds it: a fresh `Font` is a different font.
    static FONT: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
    let button = |i: usize| {
        stack([text(format!("B{i}")).text_size(12.).fill(Role::Ink)])
            .size(60., 28.)
            .radius(8.)
            .fill(if hot == Some(i) {
                Role::Danger
            } else {
                Role::Primary
            })
            .stroke(Role::Ink)
            .stroke_width(1.)
            .shadow(Shadow::soft(4.))
            .id(format!("b{i}"))
    };
    let panel = stack([block(200., 30.).fill(Role::Success).offset(-20., 20.)])
        .size(150., 60.)
        .radius(12.)
        .clip()
        .fill(Role::Surface)
        .opacity(panel)
        .id("panel");
    let mut kids = vec![
        col([
            row((0..4).map(button)).gap(10.).fill(Gradient::linear(
                100.,
                [(0., Role::Surface), (1., Role::Warning)],
            )),
            panel,
        ])
        .gap(12.)
        .pad(10.),
    ];
    if tip {
        kids.push(
            stack([text("a tip").text_size(11.).fill(Role::Ink)])
                .size(60., 20.)
                .radius(4.)
                .fill(Role::Raised)
                .offset(70., 44.)
                .float()
                .id("tip"),
        );
    }
    let mut spec = SceneSpec::new(stack(kids).fill(bg).id("root")).offered(Size::new(320., 200.));
    spec.font = Some(
        FONT.get_or_init(|| Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
            .clone(),
    );
    resolve(&spec).unwrap()
}

/// `after` rendered over `before`'s frame, and by a renderer that never saw
/// `before`: both targets' pixels, and the stats of the changed frame.
fn damaged(
    before: &ResolvedScene,
    after: &ResolvedScene,
) -> Option<(Vec<u8>, Vec<u8>, mui_vello::effects::EffectStats)> {
    damaged_at(before, after, Affine::IDENTITY)
}

fn damaged_at(
    before: &ResolvedScene,
    after: &ResolvedScene,
    xf: Affine,
) -> Option<(Vec<u8>, Vec<u8>, mui_vello::effects::EffectStats)> {
    let (device, queue) = device()?;
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    r.render(before, xf, &view).unwrap();
    let stats = r.render(after, xf, &view).unwrap();
    let part = pixels(&device, &queue, &out);
    let mut fresh = pollster::block_on(hybrid(&device, &queue));
    fresh.render(after, xf, &view).unwrap();
    Some((part, pixels(&device, &queue, &out), stats))
}

fn same_pixels(part: &[u8], whole: &[u8]) {
    near_pixels(part, whole, 0, "");
}

/// No channel of `part` more than `within` levels from `whole`; a failure
/// says where the differing pixels are.
fn near_pixels(part: &[u8], whole: &[u8], within: u8, what: &str) {
    let mut off = 0;
    let mut worst = 0;
    let [mut x0, mut y0, mut x1, mut y1] = [u32::MAX, u32::MAX, 0, 0];
    for (i, (a, b)) in part.iter().zip(whole).enumerate() {
        if a != b {
            let px = (i / 4) as u32;
            let (x, y) = (px % SIZE[0], px / SIZE[0]);
            [x0, y0, x1, y1] = [x0.min(x), y0.min(y), x1.max(x), y1.max(y)];
            off += 1;
            worst = worst.max(a.abs_diff(*b));
        }
    }
    assert!(
        worst <= within,
        "{what}{off} channels differ from a whole render, by up to {worst}, \
         in pixels {x0}..={x1} x {y0}..={y1}"
    );
}

const AREA: u64 = SIZE[0] as u64 * SIZE[1] as u64;

/// One hovered button re-renders the box around it -- shadow, stroke and
/// label included -- and the frame matches a render of the whole.
#[test]
fn a_hover_renders_only_its_box_and_matches_a_whole_render() {
    // A fractional scale puts the edges between pixels, as on a HiDPI host.
    for xf in [Affine::IDENTITY, Affine::scale(1.37)] {
        let Some((part, whole, stats)) = damaged_at(
            &board(None, false, 0.6, Role::Background),
            &board(Some(1), false, 0.6, Role::Background),
            xf,
        ) else {
            return;
        };
        assert_eq!((stats.encoded_scenes, stats.renders), (1, 1));
        assert!(
            stats.rendered_pixels < AREA / 4,
            "{}",
            stats.rendered_pixels
        );
        // A part is drawn moved by whole tiles. At a fractional scale its
        // f32 coordinates round apart from the whole frame's by an ulp, and
        // the device's rasterizer turns that into a level or a few: Metal
        // lands 96 channels 1 off, llvmpipe 106 channels up to 9 off, on
        // the buttons' straight edges. At identity the move is exact. A box
        // that misses what changed shows the change itself, Primary against
        // Danger, far past this.
        near_pixels(
            &part,
            &whole,
            if xf == Affine::IDENTITY { 0 } else { 16 },
            &format!("at {xf:?}: "),
        );
    }
}

/// Two changes with an unchanged entry painted between them: the box covers
/// the two, not the one far away that sits between them in paint order.
#[test]
fn an_unchanged_entry_between_two_changes_is_not_damage() {
    let scene = |hot: bool| {
        let chip = |x: f64, id: &str| {
            block(20., 20.)
                .fill(if hot { Role::Danger } else { Role::Primary })
                .offset(x, 10.)
                .float()
                .id(id)
        };
        let far = block(100., 40.)
            .fill(Role::Warning)
            .offset(200., 150.)
            .float()
            .id("far");
        let spec = SceneSpec::new(
            stack([chip(10., "a"), far, chip(40., "c")])
                .fill(Role::Background)
                .id("root"),
        )
        .offered(Size::new(320., 200.));
        resolve(&spec).unwrap()
    };
    let Some((part, whole, stats)) = damaged(&scene(false), &scene(true)) else {
        return;
    };
    assert!(
        stats.rendered_pixels <= 64 * 48,
        "{}",
        stats.rendered_pixels
    );
    same_pixels(&part, &whole);
}

/// A float appended to the list, and taken away again, is damage too.
#[test]
fn a_tooltip_coming_and_going_renders_only_its_box() {
    let (plain, tip) = (
        board(None, false, 0.6, Role::Background),
        board(None, true, 0.6, Role::Background),
    );
    for (before, after) in [(&plain, &tip), (&tip, &plain)] {
        let Some((part, whole, stats)) = damaged(before, after) else {
            return;
        };
        assert!(
            stats.rendered_pixels < AREA / 4,
            "{}",
            stats.rendered_pixels
        );
        same_pixels(&part, &whole);
    }
}

/// A card that only moved keeps its paths and changes its offset: the
/// damaged box renders it at the new place, gradient, shadow and label
/// included, exactly as a whole render does.
#[test]
fn a_moved_card_renders_only_its_boxes_and_matches_a_whole_render() {
    let card = |x: f64| {
        let card = stack([text("moved").text_size(11.).fill(Role::Ink)])
            .size(70., 30.)
            .radius(6.)
            .fill(Gradient::linear(
                90.,
                [(0., Role::Primary), (1., Role::Warning)],
            ))
            .stroke(Role::Ink)
            .shadow(Shadow::soft(3.))
            .offset(x, 40.)
            .float()
            .id("card");
        let mut spec = SceneSpec::new(stack([card]).fill(Role::Background).id("root"))
            .offered(Size::new(320., 200.));
        spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
        resolve(&spec).unwrap()
    };
    let (before, after) = (card(20.), card(150.));
    let fill = |s: &ResolvedScene| {
        s.paint
            .iter()
            .find(|p| p.layer == mui_scene::Layer::Fill && &*p.key == "card")
            .map(|p| (p.path.clone(), p.offset))
            .unwrap()
    };
    let ((a, at_a), (b, at_b)) = (fill(&before), fill(&after));
    assert_eq!(*a, *b, "a move keeps the local path");
    assert!(at_b.x > at_a.x, "{at_a:?} {at_b:?}");
    for xf in [Affine::IDENTITY, Affine::scale(1.37)] {
        let Some((part, whole, stats)) = damaged_at(&before, &after, xf) else {
            return;
        };
        assert!(
            stats.rendered_pixels < AREA / 2,
            "{}",
            stats.rendered_pixels
        );
        same_pixels(&part, &whole);
    }
}

/// An opacity layer that changes repaints everything inside it, clipped
/// children included, and nothing else.
#[test]
fn a_fading_layer_renders_what_it_holds() {
    let Some((part, whole, stats)) = damaged(
        &board(None, false, 0.6, Role::Background),
        &board(None, false, 0.3, Role::Background),
    ) else {
        return;
    };
    assert!(
        stats.rendered_pixels < AREA / 2,
        "{}",
        stats.rendered_pixels
    );
    same_pixels(&part, &whole);
}

/// A change to most of the frame renders all of it, as does the first
/// frame and one after a resize.
#[test]
fn big_changes_the_first_frame_and_a_resize_render_everything() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let a = board(None, false, 0.6, Role::Background);
    let first = r.render(&a, Affine::IDENTITY, &view).unwrap();
    assert_eq!(first.rendered_pixels, AREA);
    let b = board(None, false, 0.6, Role::Surface);
    assert_eq!(
        r.render(&b, Affine::IDENTITY, &view)
            .unwrap()
            .rendered_pixels,
        AREA
    );
    r.resize([SIZE[0], SIZE[1] - 8]).unwrap();
    let c = board(Some(0), false, 0.6, Role::Surface);
    let resized = r.render(&c, Affine::IDENTITY, &view).unwrap();
    assert_eq!(resized.rendered_pixels, AREA - u64::from(SIZE[0]) * 8);
}

/// A backdrop blurs whatever is under it, so no change is local to it.
#[test]
fn a_change_under_a_backdrop_renders_everything() {
    let glass = |c: Color| {
        let root = stack![
            block(40., 40.).fill(Fill::Color(c)).id("swatch"),
            block(100., 100.)
                .backdrop_blur(4.)
                .offset(200., 0.)
                .id("glass"),
        ]
        .id("root");
        resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap()
    };
    let Some((part, whole, stats)) = damaged(
        &glass(Color::srgb(1., 0., 0.)),
        &glass(Color::srgb(0., 0., 1.)),
    ) else {
        return;
    };
    assert_eq!(stats.rendered_pixels, AREA);
    same_pixels(&part, &whole);
}

/// A hover above an unchanged backdrop reuses its cached blur and paints only
/// the row's damage; the patch must still match a fresh whole-frame render.
#[test]
fn a_hover_above_a_backdrop_reuses_blur_and_renders_only_its_box() {
    let glass = |hot: bool| {
        let popup = stack![
            block(40., 30.)
                .fill(if hot { Role::Danger } else { Role::Primary })
                .offset(20., 20.)
                .id("item")
        ]
        .size(100., 80.)
        .fill(Fill::Color(Color::srgba(0.1, 0.1, 0.1, 0.65)))
        .backdrop_blur(10.)
        .offset(200., 20.)
        .float()
        .id("popup");
        let root = stack![
            row![
                block(160., 200.).fill(Fill::Color(Color::srgb(1., 0., 0.))),
                block(160., 200.).fill(Fill::Color(Color::srgb(0., 0., 1.))),
            ],
            popup,
        ]
        .id("root");
        resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap()
    };
    let Some((part, whole, stats)) = damaged(&glass(false), &glass(true)) else {
        return;
    };
    assert!(stats.rendered_pixels > 0, "the popup row did not change");
    assert!(
        stats.rendered_pixels < AREA / 4,
        "rendered {} of {AREA} target pixels",
        stats.rendered_pixels
    );
    same_pixels(&part, &whole);
}

/// The present writes premultiplied alpha by default and straight alpha
/// for a surface that composites it (`PostMultiplied`): half-clear red is
/// `[128, 0, 0, 128]` one way and `[255, 0, 0, 128]` the other.
#[test]
fn the_present_writes_premultiplied_or_straight_alpha() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let root = block(320., 200.).fill(Fill::Color(Color::srgba(1., 0., 0., 0.5)));
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    // Vello keeps straight alpha in 8 bits: un-premultiplying rounds.
    let near = |a: [u8; 4], b: [u8; 4]| a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3);
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let premultiplied = at(&pixels(&device, &queue, &out), 160, 100);
    assert!(near(premultiplied, [128, 0, 0, 128]), "{premultiplied:?}");
    r.set_straight_alpha(true);
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let straight = at(&pixels(&device, &queue, &out), 160, 100);
    assert!(near(straight, [255, 0, 0, 128]), "{straight:?}");
}

/// Past the frame, the present writes clear. The renderer's texture only
/// grows, so a window dragged smaller once showed the larger frame's
/// pixels there, wherever the swapchain is larger than the frame (Linux
/// steps it) and the window is translucent.
#[test]
fn past_the_frame_the_present_writes_clear() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let root = block(320., 200.).fill(Fill::Color(Color::srgb(1., 0., 0.)));
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    r.resize([100, 80]).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let p = pixels(&device, &queue, &out);
    assert_eq!(at(&p, 50, 40), [255, 0, 0, 255], "the frame");
    assert_eq!(at(&p, 200, 150), [0, 0, 0, 0], "past the frame");
    assert_eq!(at(&p, 50, 150), [0, 0, 0, 0], "below the frame");
}

/// A frosted fill over nothing keeps its alpha: the backdrop blurs clear
/// into clear, so a translucent window shows the desktop through it rather
/// than black or a darkened tint.
#[test]
fn a_backdrop_over_nothing_stays_translucent() {
    let Some((device, queue)) = device() else {
        return;
    };
    let out = readable(&device);
    let view = out.create_view(&wgpu::TextureViewDescriptor::default());
    let mut r = pollster::block_on(hybrid(&device, &queue));
    let root = block(320., 200.)
        .fill(Fill::Color(Color::srgba(0., 0., 1., 0.5)))
        .backdrop_blur(8.)
        .id("glass");
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(320., 200.))).unwrap();
    r.render(&scene, Affine::IDENTITY, &view).unwrap();
    let px = at(&pixels(&device, &queue, &out), 160, 100);
    assert!(
        px[0] == 0 && px[1] == 0 && px[2].abs_diff(128) <= 1 && px[3].abs_diff(128) <= 1,
        "{px:?}"
    );
}
