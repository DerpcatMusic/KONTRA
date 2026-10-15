//! Source-free Face publication/resource-retention oracles. No plugin or audio host.
use super::super::theme;
use super::*;
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[allow(dead_code)]
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod instrument_fixture;

const SIZE: [u16; 2] = [192, 128];
const FONT: ir::AssetRef = ir::AssetRef(3);

fn image(path: &Path, width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) {
    let mut pixels = Vec::new();
    for y in 0..height {
        for x in 0..width {
            pixels.extend(pixel(x, y));
        }
    }
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf, ir::Interface) {
    // The serialized test runner supplies TMPDIR on the dev drive. No vendor
    // source/artwork is read, and none of these ephemeral images is committed.
    let directory = instrument_fixture::directory();
    let root = directory.path().join("Resources");
    std::fs::create_dir_all(root.join("pictures")).unwrap();
    std::fs::create_dir_all(root.join("fonts")).unwrap();
    let path = directory.path().join("authored.nki");
    std::fs::write(
        &path,
        instrument_fixture::base_instrument("UI asset retention", &[None; 5]),
    )
    .unwrap();
    image(&root.join("pictures/wallpaper.png"), 192, 128, |x, y| {
        [30 + (x / 8) as u8, 40 + (y / 8) as u8, 70, 255]
    });
    image(&root.join("pictures/art.png"), 16, 12, |x, y| {
        [180, 50 + x as u8, 30 + y as u8, 255]
    });
    image(&root.join("pictures/strip.png"), 8, 32, |_, y| {
        [20 + (y / 8) as u8 * 60, 170, 40, 255]
    });
    std::fs::write(root.join("fonts/authored.ttf"), theme::NOTO_SANS).unwrap();

    let mut panel = ir::Widget::new(
        "$panel",
        ir::PageRef(0),
        ir::Rect::new(8, 6, 176, 116),
        ir::Kind::Panel,
    );
    panel.source_id = Some(100);
    let mut label = ir::Widget::new(
        "$label",
        ir::PageRef(0),
        ir::Rect::new(4, 4, 144, 22),
        ir::Kind::Label,
    );
    label.source_id = Some(101);
    label.parent = Some(ir::WidgetRef(0));
    label.text = "Preset A".into();
    label.style = Some(ir::StyleRef(0));
    let mut knob = ir::Widget::new(
        "$knob",
        ir::PageRef(0),
        ir::Rect::new(4, 34, 16, 16),
        ir::Kind::Knob {
            range: ir::Range {
                min: 0.,
                max: 3.,
                default: 0.,
                step: Some(1.),
            },
            display: Default::default(),
        },
    );
    knob.source_id = Some(102);
    knob.parent = Some(ir::WidgetRef(0));
    knob.binding = ir::Binding::Control(ir::ControlId(11));
    knob.images
        .push(ir::ImageUse::new(ir::AssetRef(2), ir::Role::Strip));
    let mut menu = ir::Widget::new(
        "$menu",
        ir::PageRef(0),
        ir::Rect::new(36, 30, 112, 24),
        ir::Kind::Menu {
            items: [
                ("First", 10, true),
                ("Second", 40, true),
                ("Hidden", 90, false),
            ]
            .into_iter()
            .map(|(text, value, visible)| ir::MenuItem {
                text: text.into(),
                value,
                visible,
            })
            .collect(),
        },
    );
    menu.source_id = Some(103);
    menu.parent = Some(ir::WidgetRef(0));
    menu.binding = ir::Binding::Control(ir::ControlId(12));
    menu.initial_value = 10.;
    menu.style = Some(ir::StyleRef(0));
    let mut art = ir::Widget::new(
        "$art",
        ir::PageRef(0),
        ir::Rect::new(4, 66, 32, 24),
        ir::Kind::Label,
    );
    art.source_id = Some(104);
    art.parent = Some(ir::WidgetRef(0));
    art.images
        .push(ir::ImageUse::new(ir::AssetRef(1), ir::Role::Background));
    let mut alternate = ir::Widget::new(
        "$alternate",
        ir::PageRef(0),
        ir::Rect::new(45, 62, 100, 22),
        ir::Kind::Label,
    );
    alternate.source_id = Some(105);
    alternate.parent = Some(ir::WidgetRef(0));
    alternate.text = "Alternate".into();
    alternate.style = Some(ir::StyleRef(0));
    alternate.hidden = true;
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        performance: true,
        pages: vec![ir::Page {
            size: ir::Size {
                width: SIZE[0].into(),
                height: SIZE[1].into(),
            },
            background: ir::Background {
                image: Some(ir::AssetRef(0)),
                ..Default::default()
            },
            ..Default::default()
        }],
        widgets: vec![panel, label, knob, menu, art, alternate],
        assets: vec![
            ir::Asset {
                path: "Resources/pictures/wallpaper.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    size: Some(ir::Size {
                        width: 192.,
                        height: 128.,
                    }),
                    ..Default::default()
                }),
            },
            ir::Asset {
                path: "Resources/pictures/art.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    size: Some(ir::Size {
                        width: 16.,
                        height: 12.,
                    }),
                    ..Default::default()
                }),
            },
            ir::Asset {
                path: "Resources/pictures/strip.png".into(),
                kind: ir::AssetKind::Image(ir::ImageMeta {
                    frames: 4,
                    size: Some(ir::Size {
                        width: 8.,
                        height: 8.,
                    }),
                    ..Default::default()
                }),
            },
            ir::Asset {
                path: "Resources/fonts/authored.ttf".into(),
                kind: ir::AssetKind::TrueTypeFont,
            },
        ],
        styles: vec![ir::TextStyle {
            font: ir::Font::File(FONT),
            size: Some(12.),
            color: ir::Rgba::rgb(0xffffff),
            align: ir::Align::Left,
        }],
        ..Default::default()
    };
    (directory, path, face)
}

fn variant(base: &ir::Interface) -> ir::Interface {
    let mut b = base.clone();
    b.widgets[0].rect.x += 3.;
    b.widgets[0].rect.y += 2.;
    b.widgets[1].text = "Preset B".into();
    b.widgets[1].tooltip = "Changed presentation".into();
    b.widgets[2].initial_value = 3.;
    b.widgets[2].value_text = Some("Last frame".into());
    b.widgets[3].initial_value = 40.;
    b.widgets[3].menu_arrow = false;
    b.widgets[3].rect.width = 124.;
    if let ir::Kind::Menu { items } = &mut b.widgets[3].kind {
        items[1].text = "Second preset".into();
        items[2].visible = true;
    }
    b.widgets[4].hidden = true;
    b.widgets[5].hidden = false;
    b.widgets[5].enabled = false;
    b.widgets[5].opacity = 0.75;
    b.widgets[5].z = 5;
    b.styles[0].color = ir::Rgba::rgb(0xffd080);
    b
}

fn ready(face: &mut Face) {
    let started = Instant::now();
    loop {
        face.assets.prepare_for_program(
            &face.path,
            face.program,
            &face.face,
            face.page,
            face.presentation,
            1.,
            &face.values,
        );
        if face.assets.pending() == 0 {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "synthetic art did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        face.assets.failures().is_empty(),
        "all authored fixture resources are valid"
    );
}

fn io(face: &Face) -> (usize, usize, usize, usize) {
    let scan = face.assets.scan();
    (scan.lookups, scan.encoded_bytes, scan.decodes, scan.fonts)
}

fn frame(ui: &mut Ui, face: &mut Face) {
    for _ in 0..3 {
        let root = ir_view::view_state(
            ui,
            "retention",
            &face.face,
            face.page,
            &face.assets,
            face.presentation,
            1.,
            &mut face.values,
            &mut face.input,
        );
        ui.frame(
            root,
            Some(Size::new(SIZE[0].into(), SIZE[1].into())),
            Input::default(),
            1. / 60.,
        )
        .unwrap();
    }
}

fn oracle(face: &mut Face, current: &ir::Interface, ui: &mut Ui, context: &str) -> Vec<u8> {
    assert_eq!(
        face.sources[face.shown].interface(&face.from[face.shown]),
        current,
        "complete raw presentation"
    );
    assert_eq!(
        face.face,
        ir_view::resolved(current),
        "complete resolved presentation"
    );
    let mut reference = Face::new_for_program(
        &face.path,
        face.generation,
        vec![current.clone()].into(),
        0,
        face.presentation,
        face.program,
    );
    // Production reads committed scalar/typed controls separately from the
    // structural Interface publication. Both painters must see the same input.
    reference.values.clone_from(&face.values);
    reference.input.values.clone_from(&face.input.values);
    ready(&mut reference);
    let mut fresh = theme::ui();
    frame(ui, face);
    frame(&mut fresh, &mut reference);
    assert_eq!(
        face.values, reference.values,
        "{context}: effective scalar state"
    );
    assert_eq!(
        face.input.values, reference.input.values,
        "{context}: effective typed state"
    );
    let geometry = |ui: &Ui| {
        let mut rectangles: Vec<_> = ui
            .scene()
            .unwrap()
            .surfaces()
            .filter(|s| s.key.as_str().starts_with("retention"))
            .map(|s| (s.key.as_str().to_owned(), s.frame))
            .collect();
        rectangles.sort_by(|a, b| a.0.cmp(&b.0));
        rectangles
    };
    assert_eq!(
        geometry(ui),
        geometry(&fresh),
        "actual retained/fresh surface geometry"
    );
    let pixels = super::super::tests::pixels(ui, SIZE[0], SIZE[1]);
    let expected = super::super::tests::pixels(&fresh, SIZE[0], SIZE[1]);
    let changed: Vec<_> = pixels
        .chunks_exact(4)
        .zip(expected.chunks_exact(4))
        .enumerate()
        .filter_map(|(at, (a, b))| (a != b).then_some(at))
        .collect();
    if !changed.is_empty()
        && let Some(root) = std::env::var_os("KONTRA_TEST_SCRATCH")
    {
        let root = std::path::PathBuf::from(root);
        for (name, rgba) in [("retained", &pixels), ("fresh", &expected)] {
            image(
                &root.join(format!("{context}-{:?}-{name}.png", face.presentation)),
                SIZE[0].into(),
                SIZE[1].into(),
                |x, y| {
                    let at = (y as usize * SIZE[0] as usize + x as usize) * 4;
                    rgba[at..at + 4].try_into().unwrap()
                },
            );
        }
    }
    let first = changed.first().map(|&at| {
        (
            at % SIZE[0] as usize,
            at / SIZE[0] as usize,
            &pixels[at * 4..at * 4 + 4],
            &expected[at * 4..at * 4 + 4],
        )
    });
    assert!(
        changed.is_empty(),
        "{context}/{:?}: actual retained/fresh CPU pixels differ: count={}, first={first:?}, hashes={}/{}",
        face.presentation,
        changed.len(),
        blake3::hash(&pixels),
        blake3::hash(&expected)
    );
    pixels
}

#[test]
fn presentation_a_b_a_reuses_assets_while_committed_values_menus_and_layout_change() {
    for presentation in [Presentation::Bitmap, Presentation::Vector] {
        let (_directory, path, a) = fixture();
        let b = variant(&a);
        assert!(same_bindings(&a, &b));
        let bases: Arc<[ir::Interface]> = vec![a.clone()].into();
        let mut face = Face::new_for_program(&path, 7, bases.clone(), 0, presentation, 0);
        face.values
            .extend([(ir::ControlId(11), 0.), (ir::ControlId(12), 10.)]);
        ready(&mut face);
        let worker = face.assets.worker_identity();
        let warm_io = io(&face);
        let font = face.assets.font(&FONT).unwrap().id();
        let wallpaper = face.assets.wallpaper(ir::AssetRef(0)).unwrap().frames[0].clone();
        let art = face.assets.get(ir::AssetRef(1)).unwrap().frames[0].clone();
        let expected_images = if presentation == Presentation::Bitmap {
            3
        } else {
            2
        };
        assert_eq!(
            warm_io.0,
            expected_images + 1,
            "one lookup per image/font resource"
        );
        assert_eq!((warm_io.2, warm_io.3), (expected_images, 1));
        let mut ui = theme::ui();
        let a_pixels = oracle(&mut face, &a, &mut ui, "initial-a");

        let live_a = face.values.clone();
        face.publish(
            bases.clone(),
            vec![ir::InterfacePatch::between(&a, &b)].into(),
            0,
        );
        assert_eq!(
            face.values, live_a,
            "graph publication alone must not overwrite live user values"
        );
        // The same extend boundary as part::interface's committed readback.
        face.values
            .extend([(ir::ControlId(11), 3.), (ir::ControlId(12), 40.)]);
        ready(&mut face);
        assert_eq!(face.generation, 7);
        assert!(Arc::ptr_eq(&face.from, &bases));
        assert_eq!(face.assets.worker_identity(), worker);
        assert_eq!(
            io(&face),
            warm_io,
            "a new strip frame uses the resident decoded strip, not another read/decode"
        );
        assert_eq!(face.assets.font(&FONT).unwrap().id(), font);
        assert!(Arc::ptr_eq(
            &wallpaper,
            &face.assets.wallpaper(ir::AssetRef(0)).unwrap().frames[0]
        ));
        assert!(!face.face.visible(ir::WidgetRef(4)));
        assert!(face.face.visible(ir::WidgetRef(5)));
        assert_eq!(face.face.widgets[3].initial_value, 40.);
        assert!(
            oracle(&mut face, &b, &mut ui, "b") != a_pixels,
            "the preset must visibly change"
        );

        let completed = face.assets.scan().preparation_completed;
        let live_b = face.values.clone();
        face.publish(bases, Arc::from([]), 0);
        assert_eq!(
            face.values, live_b,
            "graph reversion alone must preserve current live controls"
        );
        face.values
            .extend([(ir::ControlId(11), 0.), (ir::ControlId(12), 10.)]);
        ready(&mut face);
        assert_eq!(face.assets.worker_identity(), worker);
        assert_eq!(io(&face), warm_io);
        assert_eq!(
            face.assets.scan().preparation_completed,
            completed,
            "warm A reuses prepared keys as well as encoded resources"
        );
        assert_eq!(face.assets.font(&FONT).unwrap().id(), font);
        assert!(Arc::ptr_eq(
            &art,
            &face.assets.get(ir::AssetRef(1)).unwrap().frames[0]
        ));
        assert!(
            oracle(&mut face, &a, &mut ui, "return-a") == a_pixels,
            "exact authored A returns"
        );
    }
}

#[test]
fn structural_generation_reuses_valid_art_but_resource_revision_rebuilds_once() {
    use crate::sound::resource_revision::ResourceRevision;
    let (_directory, path, a) = fixture();
    let resources = sampler_kontakt::Resources::of(&path);
    let revision = Arc::new(ResourceRevision::capture(&path, 0, &resources).unwrap());
    let mut face =
        Face::new_for_program(&path, 4, vec![a.clone()].into(), 0, Presentation::Bitmap, 0);
    face.resource_revision = Some(revision.clone());
    ready(&mut face);
    let worker = face.assets.worker_identity();
    let warm_io = io(&face);
    let art = face.assets.get(ir::AssetRef(1)).unwrap().frames[0].clone();
    let font = face.assets.font(&FONT).unwrap().id();
    face.values.insert(ir::ControlId(999), 123.);
    face.input
        .values
        .insert(ir::WidgetRef(999), ir::Value::Text("stale owner".into()));
    let mut structural = variant(&a);
    let mut appended = structural.widgets[5].clone();
    appended.name = "$new_owner".into();
    appended.source_id = Some(106);
    appended.binding = ir::Binding::Control(ir::ControlId(99));
    appended.rect.y += 22.;
    structural.widgets.push(appended);
    assert!(!same_bindings(&a, &structural));
    assert!(face.shares_resources(&path, 0, &Some(revision.clone())));
    face.refresh_generation(5, vec![structural.clone()].into(), 0, Presentation::Bitmap);
    assert_eq!(face.generation, 5);
    assert!(
        face.values.is_empty() && face.input.values.is_empty(),
        "old owner drafts/values cannot publish into the fresh owner"
    );
    face.values
        .extend([(ir::ControlId(11), 3.), (ir::ControlId(12), 40.)]);
    ready(&mut face);
    assert_eq!(face.assets.worker_identity(), worker);
    assert_eq!(io(&face), warm_io);
    assert_eq!(face.assets.font(&FONT).unwrap().id(), font);
    let mut ui = theme::ui();
    oracle(&mut face, &structural, &mut ui, "structural");
    // The same asset descriptor is insufficient authority after its file changes.
    image(
        &path.parent().unwrap().join("Resources/pictures/art.png"),
        16,
        12,
        |_, _| [10, 40, 230, 255],
    );
    let changed_resources = sampler_kontakt::Resources::of(&path);
    let changed = Arc::new(ResourceRevision::capture(&path, 0, &changed_resources).unwrap());
    assert_ne!(revision, changed);
    assert!(!face.shares_resources(&path, 0, &Some(changed.clone())));
    assert!(!face.shares_resources(&path, 0, &None));
    assert!(!face.shares_resources(&path, 1, &Some(revision)));
    // Keep the retired owner alive: accidental pointer reuse cannot satisfy this
    // assertion, and its completed results have a different private receiver.
    let retired = face;
    let mut fresh =
        Face::new_for_program(&path, 6, vec![a.clone()].into(), 0, Presentation::Bitmap, 0);
    fresh.resource_revision = Some(changed);
    ready(&mut fresh);
    assert_ne!(
        fresh.assets.worker_identity(),
        retired.assets.worker_identity()
    );
    let updated = fresh.assets.get(ir::AssetRef(1)).unwrap().frames[0].clone();
    assert!(!Arc::ptr_eq(&art, &updated));
    assert_eq!(&updated.rgba[..4], &[10, 40, 230, 255]);
    assert_eq!(io(&fresh).0, 4);
    assert_eq!((io(&fresh).2, io(&fresh).3), (3, 1));
    let settled = (io(&fresh), fresh.assets.scan().preparation_completed);
    for _ in 0..3 {
        ready(&mut fresh);
    }
    assert_eq!(
        (io(&fresh), fresh.assets.scan().preparation_completed),
        settled,
        "changed authority settles once, not on every presentation refresh"
    );
    let mut ui = theme::ui();
    oracle(&mut fresh, &a, &mut ui, "new-authority");
    assert_eq!(
        &art.rgba[..4],
        &[180, 50, 30, 255],
        "retired immutable pixels are not modified in place"
    );
}
