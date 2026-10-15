//! Authored synthetic picture/text geometry; no installed-library artwork.
use super::*;

const RED: [u8; 4] = [220, 20, 30, 255];
const GREEN: [u8; 4] = [20, 220, 30, 255];
const BLUE: [u8; 4] = [20, 30, 220, 255];

fn fixture(
    source: ir::Source,
    size: [u32; 2],
    stretch: [bool; 2],
    control: [f64; 2],
) -> (Interface, Assets) {
    let mut label = ir::Widget::new(
        "$picture",
        PageRef(0),
        ir::Rect {
            x: 2.,
            y: 2.,
            width: control[0],
            height: control[1],
        },
        Kind::Label,
    );
    let mut picture = ir::ImageUse::new(ir::AssetRef(0), Use::Background);
    picture.frame = Some(1);
    label.images.push(picture);
    let face = Interface {
        source,
        pages: vec![ir::Page {
            size: ir::Size {
                width: 40.,
                height: 24.,
            },
            background: ir::Background {
                color: Some(ir::Rgba::rgb(0)),
                ..Default::default()
            },
            ..Default::default()
        }],
        widgets: vec![label],
        assets: vec![ir::Asset {
            path: "authored.png".into(),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                frames: 2,
                size: Some(ir::Size {
                    width: f64::from(size[0]),
                    height: f64::from(size[1]),
                }),
                stretch,
                margins: ir::Margins {
                    top: 1,
                    bottom: 1,
                    left: 1,
                    right: 1,
                },
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    let mut rgba = Vec::new();
    for row in 0..size[1] {
        for _ in 0..size[0] {
            rgba.extend(if row == 0 {
                RED
            } else if row + 1 == size[1] {
                GREEN
            } else {
                BLUE
            });
        }
    }
    let unused = Image::rgba(
        size[0],
        size[1],
        [250, 250, 0, 255].repeat((size[0] * size[1]) as usize),
    )
    .unwrap();
    let wanted = Image::rgba(size[0], size[1], rgba).unwrap();
    let picture = Arc::new(Picture::new(vec![Arc::new(unused), Arc::new(wanted)]));
    let mut assets = Assets::default();
    assets.sync(&face, Presentation::Bitmap, |_| Some(picture.clone()));
    (resolved(&face), assets)
}

fn render(face: &Interface, assets: &Assets, scale: f64) -> Vec<u8> {
    let mut ui = super::super::theme::ui();
    for _ in 0..2 {
        let root = view(
            &mut ui,
            face,
            PageRef(0),
            assets,
            Presentation::Bitmap,
            scale,
            &mut Values::default(),
        );
        ui.frame(
            root,
            Some(Size::new(40. * scale, 24. * scale)),
            Input::default(),
            1. / 60.,
        )
        .unwrap();
    }
    super::super::tests::pixels(&ui, (40. * scale) as u16, (24. * scale) as u16)
}

fn pixel(pixels: &[u8], scale: usize, x: usize, y: usize) -> [u8; 4] {
    let at = (y * 40 * scale + x) * 4;
    pixels[at..at + 4].try_into().unwrap()
}

#[test]
fn intrinsic_label_picture_keeps_native_axes_and_authored_text_bounds() {
    for source in [ir::Source::Ksp { slot: 0 }, ir::Source::PerformanceView] {
        let (face, assets) = fixture(source, [12, 8], [false, false], [6., 3.]);
        assert_eq!(face.widgets[0].rect, ir::Rect::new(2, 2, 6, 3));
        for scale in [1usize, 2] {
            let pixels = render(&face, &assets, scale as f64);
            assert_eq!(pixel(&pixels, scale, 3 * scale, 2 * scale), RED);
            assert_eq!(pixel(&pixels, scale, 10 * scale, 7 * scale), BLUE);
            assert_eq!(
                pixel(&pixels, scale, 10 * scale, 9 * scale + scale - 1),
                GREEN
            );
            assert_eq!(pixel(&pixels, scale, 14 * scale, 7 * scale), [0, 0, 0, 255]);
        }
    }
}

#[test]
fn label_picture_mixed_axes_keep_nine_slice_edges_and_lua_policy() {
    for stretch in [[true, false], [false, true], [true, true]] {
        let (face, assets) = fixture(ir::Source::Ksp { slot: 0 }, [6, 6], stretch, [12., 10.]);
        assert_eq!(face.widgets[0].rect, ir::Rect::new(2, 2, 12, 10));
        let width = if stretch[0] { 12 } else { 6 };
        let height = if stretch[1] { 10 } else { 6 };
        for scale in [1usize, 2] {
            let pixels = render(&face, &assets, scale as f64);
            assert_eq!(pixel(&pixels, scale, 3 * scale, 2 * scale), RED);
            assert_eq!(
                pixel(&pixels, scale, 3 * scale, (2 + height) * scale - 1),
                GREEN
            );
            assert_eq!(
                pixel(&pixels, scale, (2 + width) * scale, 4 * scale),
                [0, 0, 0, 255]
            );
        }
    }
    let (face, assets) = fixture(ir::Source::FalconLua, [12, 8], [false, false], [6., 3.]);
    assert_eq!(
        label_background_size(&face, WidgetRef(0), ir::AssetRef(0), [6., 3.], 1.),
        [6., 3.]
    );
    let pixels = render(&face, &assets, 1.);
    assert_eq!(pixel(&pixels, 1, 10, 7), [0, 0, 0, 255]);
}

#[test]
fn one_pixel_label_background_does_not_collapse_or_fill_the_text_box() {
    let (mut face, assets) = fixture(
        ir::Source::Ksp { slot: 0 },
        [1, 1],
        [false, false],
        [32., 12.],
    );
    face.widgets[0].text = "Wide label".into();
    for scale in [1usize, 2] {
        let pixels = render(&face, &assets, scale as f64);
        let picture_pixels = pixels.chunks_exact(4).filter(|p| *p == RED).count();
        assert_eq!(picture_pixels, scale * scale);
        assert!(
            pixels
                .chunks_exact(4)
                .enumerate()
                .any(|(i, p)| i % (40 * scale) > 4 * scale && p != [0, 0, 0, 255]),
            "text must still draw beyond the tiny picture"
        );
        assert_eq!(face.widgets[0].rect, ir::Rect::new(2, 2, 32, 12));
    }
}

#[test]
fn background_contrast_sampling_uses_the_actual_picture_footprint() {
    let (mut face, mut assets) = fixture(
        ir::Source::Ksp { slot: 0 },
        [12, 8],
        [false, false],
        [6., 3.],
    );
    face.widgets.push(ir::Widget::new(
        "$foreground",
        PageRef(0),
        ir::Rect::new(10, 7, 1, 1),
        Kind::Knob {
            range: Default::default(),
            display: Default::default(),
        },
    ));
    assets.sync(&face, Presentation::Bitmap, |_| unreachable!());
    let white = Arc::new(Image::rgba(12, 8, [255, 255, 255, 255].repeat(12 * 8)).unwrap());
    assets
        .loaded
        .insert(0, Some(Arc::new(Picture::new(vec![white.clone(), white]))));
    assert!(
        light_under(&face, &assets, WidgetRef(1)),
        "picture extends beyond label text/control bounds"
    );
    if let ir::AssetKind::Image(meta) = &mut face.assets[0].kind {
        meta.size = Some(ir::Size {
            width: 1.,
            height: 1.,
        });
    }
    assert!(
        !light_under(&face, &assets, WidgetRef(1)),
        "one-pixel art does not fill the larger label text box"
    );
}
