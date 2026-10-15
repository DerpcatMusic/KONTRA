//! Source-free request/painter effective-value regression. No production hooks.
use super::super::{ir_view::Assets, theme};
use super::*;
use moose::mui::mui::prelude::*;
#[allow(dead_code)]
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod instrument_fixture;
const COLORS: [[u8; 4]; 3] = [
    [19, 59, 113, 255],
    [79, 119, 173, 255],
    [139, 179, 233, 255],
];

#[test]
fn requested_frame_and_actual_painter_share_default_typed_and_committed_values() {
    let mut observed = Vec::new();
    let mut expected = Vec::new();
    for slider in [false, true] {
        for (name, bound, typed, readback, wanted) in [
            ("unbound-range-default", false, None, None, 2),
            ("bound-missing-readback", true, None, None, 2),
            ("unbound-typed-value", false, Some(1.), None, 1),
            (
                "committed-overrides-typed-default",
                true,
                Some(1.),
                Some(0.),
                0,
            ),
        ] {
            let directory = instrument_fixture::directory();
            let path = directory.path().join("authored.nki");
            std::fs::write(
                &path,
                instrument_fixture::base_instrument("Default frames", &[None; 5]),
            )
            .unwrap();
            let folder = directory.path().join("Resources/pictures");
            std::fs::create_dir_all(&folder).unwrap();
            let mut bytes = Vec::new();
            {
                let mut encoder = png::Encoder::new(&mut bytes, 32, 96);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                let rgba: Vec<_> = (0..96).flat_map(|y| COLORS[y / 32].repeat(32)).collect();
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&rgba)
                    .unwrap();
            }
            std::fs::write(folder.join("strip.png"), bytes).unwrap();
            let range = ir::Range {
                min: 0.,
                max: 2.,
                default: 2.,
                step: Some(1.),
            };
            let kind = if slider {
                ir::Kind::Slider {
                    range,
                    orientation: ir::Orientation::Horizontal,
                }
            } else {
                ir::Kind::Knob {
                    range,
                    display: Default::default(),
                }
            };
            let mut widget =
                ir::Widget::new("$value", ir::PageRef(0), ir::Rect::new(0, 0, 32, 32), kind);
            widget.binding = if bound {
                ir::Binding::Control(ir::ControlId(11))
            } else {
                ir::Binding::None
            };
            widget.initial_value = 0.;
            widget.value = typed.map(ir::Value::Real);
            widget.hide = ir::Parts {
                title: true,
                value: true,
                unit: true,
                ..Default::default()
            };
            widget
                .images
                .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
            let face = ir::Interface {
                source: ir::Source::Ksp { slot: 0 },
                performance: true,
                pages: vec![ir::Page {
                    size: ir::Size {
                        width: 32.,
                        height: 32.,
                    },
                    ..Default::default()
                }],
                widgets: vec![widget],
                assets: vec![ir::Asset {
                    path: "Resources/pictures/strip.png".into(),
                    kind: ir::AssetKind::Image(ir::ImageMeta {
                        frames: 3,
                        size: Some(ir::Size {
                            width: 32.,
                            height: 32.,
                        }),
                        ..Default::default()
                    }),
                }],
                ..Default::default()
            };
            face.validate().unwrap();
            let mut values = Values::default();
            if let Some(value) = readback {
                values.insert(ir::ControlId(11), value);
            }
            let mut worker = Preparation::new_for_program(&path, 0);
            let (capture, requests) = mpsc::sync_channel(1);
            worker.jobs = capture;
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            let batch = requests
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            assert_eq!(batch.len(), 1);
            let requested = batch[0].key.frame;
            // All painter frames use the real decoder for this authored strip.
            let mut source = Source::for_program(&path, 0).unwrap();
            let frames = (0..3)
                .map(|frame| {
                    source
                        .load_frame(&face.assets[0], frame, [32, 32], None, || false)
                        .unwrap()
                        .frames[0]
                        .clone()
                })
                .collect();
            let picture = Arc::new(Picture::new(frames));
            let mut assets = Assets::default();
            assets.sync(&face, ir::Presentation::Bitmap, |_| Some(picture.clone()));
            let mut ui = theme::ui();
            for _ in 0..3 {
                let root = super::super::ir_view::view(
                    &mut ui,
                    &face,
                    ir::PageRef(0),
                    &assets,
                    ir::Presentation::Bitmap,
                    1.,
                    &mut values,
                );
                ui.frame(root, Some(Size::new(32., 32.)), Input::default(), 1. / 60.)
                    .unwrap();
            }
            let rgba = super::super::tests::pixels(&ui, 32, 32);
            let at = (16 * 32 + 16) * 4;
            let painted: [u8; 4] = rgba[at..at + 4].try_into().unwrap();
            let label = format!("{slider}/{name}");
            observed.push((label.clone(), requested, painted));
            expected.push((label, wanted, COLORS[wanted]));
        }
    }
    assert_eq!(
        observed, expected,
        "request and actual CPU pixels must agree on effective control values"
    );
}

// Observe the public scene and complete CPU paint, rather than testing the
// effective-value helper against a second copy of its selection logic.
fn painted_scalar(
    face: &ir::Interface,
    picture: Option<Arc<Picture>>,
    values: &Values,
) -> (Vec<u8>, Vec<(String, [u64; 4])>, u64) {
    let mut assets = Assets::default();
    assets.sync(face, ir::Presentation::Bitmap, |_| picture.clone());
    let mut ui = theme::ui();
    let mut values = values.clone();
    for _ in 0..3 {
        let root = super::super::ir_view::view(
            &mut ui,
            face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Bitmap,
            1.,
            &mut values,
        );
        ui.frame(root, Some(Size::new(96., 48.)), Input::default(), 1. / 60.)
            .unwrap();
    }
    let scene = ui.scene().unwrap();
    let role = &scene
        .surface("ir-0")
        .unwrap()
        .semantics
        .as_ref()
        .unwrap()
        .role;
    let value = match role {
        A11y::Slider { value, .. } => *value,
        A11y::Toggle { on } => {
            if *on {
                1.
            } else {
                0.
            }
        }
        _ => panic!("scalar fixture must expose a numeric or toggle value"),
    };
    let mut geometry: Vec<_> = scene
        .surfaces()
        .map(|s| {
            (
                s.key.as_str().to_owned(),
                [
                    s.frame.x,
                    s.frame.y,
                    s.frame.size.width,
                    s.frame.size.height,
                ]
                .map(f64::to_bits),
            )
        })
        .collect();
    geometry.sort_by(|a, b| a.0.cmp(&b.0));
    (
        super::super::tests::pixels(&ui, 96, 48),
        geometry,
        value.to_bits(),
    )
}

fn scalar_cases() -> Vec<(
    &'static str,
    ir::Binding,
    Option<ir::Value>,
    Option<f64>,
    f64,
    usize,
)> {
    use ir::{Binding as B, Value as V};
    let bound = B::Control(ir::ControlId(11));
    let nan = f64::from_bits(0x7ff8_0000_0000_0123);
    vec![
        ("unbound-default", B::None, None, None, 2., 4),
        ("bound-missing-readback", bound.clone(), None, None, 2., 4),
        ("typed-real", B::None, Some(V::Real(1.)), None, 1., 3),
        (
            "typed-integer",
            bound.clone(),
            Some(V::Integer(-1)),
            None,
            -1.,
            1,
        ),
        (
            "committed-wins",
            bound.clone(),
            Some(V::Integer(1)),
            Some(-2.),
            -2.,
            0,
        ),
        (
            "committed-zero-wins",
            bound.clone(),
            Some(V::Real(1.)),
            Some(0.),
            0.,
            2,
        ),
        (
            "unbound-ignores-readback",
            B::None,
            Some(V::Integer(1)),
            Some(-2.),
            1.,
            3,
        ),
        (
            "variable-ignores-readback",
            B::Variable {
                script: 0,
                name: "$value".into(),
            },
            Some(V::Real(1.)),
            Some(-2.),
            1.,
            3,
        ),
        (
            "different-control-ignored",
            B::Control(ir::ControlId(12)),
            Some(V::Real(1.)),
            Some(-2.),
            1.,
            3,
        ),
        (
            "text-is-not-scalar",
            B::None,
            Some(V::Text("-2".into())),
            None,
            2.,
            4,
        ),
        (
            "integer-array-is-not-scalar",
            B::None,
            Some(V::Integers(vec![-2])),
            None,
            2.,
            4,
        ),
        (
            "real-array-is-not-scalar",
            B::None,
            Some(V::Reals(vec![-2.])),
            None,
            2.,
            4,
        ),
        (
            "typed-negative-zero",
            B::None,
            Some(V::Real(-0.)),
            None,
            -0.,
            2,
        ),
        (
            "committed-negative-zero",
            bound.clone(),
            Some(V::Real(1.)),
            Some(-0.),
            -0.,
            2,
        ),
        (
            "typed-nan-payload",
            B::None,
            Some(V::Real(nan)),
            None,
            nan,
            0,
        ),
        (
            "committed-nan-payload",
            bound.clone(),
            Some(V::Real(1.)),
            Some(nan),
            nan,
            0,
        ),
        (
            "typed-positive-infinity",
            B::None,
            Some(V::Real(f64::INFINITY)),
            None,
            f64::INFINITY,
            4,
        ),
        (
            "committed-negative-infinity",
            bound,
            Some(V::Real(1.)),
            Some(f64::NEG_INFINITY),
            f64::NEG_INFINITY,
            0,
        ),
    ]
}

fn scalar_face(kind: ir::Kind) -> ir::Interface {
    let mut widget = ir::Widget::new("$value", ir::PageRef(0), ir::Rect::new(7, 9, 72, 24), kind);
    widget.initial_value = -1.;
    widget.hide.title = true;
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        performance: true,
        pages: vec![ir::Page {
            size: ir::Size {
                width: 96.,
                height: 48.,
            },
            ..Default::default()
        }],
        widgets: vec![widget],
        ..Default::default()
    };
    face.validate().unwrap();
    face
}

fn scalar_range() -> ir::Range {
    ir::Range {
        min: -2.,
        max: 2.,
        default: 2.,
        step: Some(1.),
    }
}

fn patterned_strip() -> (tempfile::TempDir, PathBuf, ir::Asset) {
    let directory = instrument_fixture::directory();
    let path = directory.path().join("authored.nki");
    std::fs::write(
        &path,
        instrument_fixture::base_instrument("Scalar pixels", &[None; 5]),
    )
    .unwrap();
    let pictures = directory.path().join("Resources/pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 24, 120);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let rgba: Vec<_> = (0..120u8)
            .flat_map(|y| {
                (0..24u8)
                    .flat_map(move |x| [20 + (y / 24) * 40, 30 + x * 7, 40 + (y % 24) * 7, 255])
            })
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
    }
    std::fs::write(pictures.join("strip.png"), bytes).unwrap();
    let asset = ir::Asset {
        path: "Resources/pictures/strip.png".into(),
        kind: ir::AssetKind::Image(ir::ImageMeta {
            frames: 5,
            size: Some(ir::Size {
                width: 24.,
                height: 24.,
            }),
            ..Default::default()
        }),
    };
    (directory, path, asset)
}

#[test]
fn scalar_strip_edges_match_full_pixels_geometry_and_real_painter_values() {
    let (_directory, path, asset) = patterned_strip();
    let mut source = Source::for_program(&path, 0).unwrap();
    let frames = (0..5)
        .map(|frame| {
            source
                .load_frame(&asset, frame, [24, 24], None, || false)
                .unwrap()
                .frames[0]
                .clone()
        })
        .collect();
    let picture = Arc::new(Picture::new(frames));
    for slider in [false, true] {
        let kind = if slider {
            ir::Kind::Slider {
                range: scalar_range(),
                orientation: ir::Orientation::Horizontal,
            }
        } else {
            ir::Kind::Knob {
                range: scalar_range(),
                display: Default::default(),
            }
        };
        for (name, binding, typed, readback, expected, expected_frame) in scalar_cases() {
            let mut face = scalar_face(kind.clone());
            face.assets.push(asset.clone());
            face.widgets[0]
                .images
                .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
            face.widgets[0].binding = binding;
            face.widgets[0].value = typed;
            let values: Values = readback
                .into_iter()
                .map(|v| (ir::ControlId(11), v))
                .collect();
            let mut worker = Preparation::new_for_program(&path, 0);
            let (capture, requests) = mpsc::sync_channel(1);
            worker.jobs = capture;
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            let batch = requests
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap();
            assert_eq!(batch.len(), 1, "{slider}/{name}: exactly one requested key");
            assert_eq!(
                batch[0].key.frame, expected_frame,
                "{slider}/{name}: requested strip frame"
            );
            let actual = painted_scalar(&face, Some(picture.clone()), &values);
            assert_eq!(
                actual.2,
                expected.to_bits(),
                "{slider}/{name}: actual painter scalar bits"
            );
            let mut baseline = face.clone();
            baseline.widgets[0].binding = ir::Binding::Control(ir::ControlId(11));
            let baseline_values = Values::from([(ir::ControlId(11), expected)]);
            let reference = painted_scalar(&baseline, Some(picture.clone()), &baseline_values);
            assert_eq!(
                actual.0, reference.0,
                "{slider}/{name}: complete CPU pixels"
            );
            assert_eq!(
                actual.1, reference.1,
                "{slider}/{name}: complete scene geometry"
            );
        }
    }
}

#[test]
fn value_edit_keeps_real_text_pixels_geometry_and_scalar_bits_for_all_precedence_edges() {
    let kind = ir::Kind::ValueEdit {
        range: scalar_range(),
        display: Default::default(),
        arrows: true,
    };
    for (name, binding, typed, readback, expected, _) in scalar_cases() {
        let mut face = scalar_face(kind.clone());
        face.widgets[0].binding = binding;
        face.widgets[0].value = typed;
        let values: Values = readback
            .into_iter()
            .map(|v| (ir::ControlId(11), v))
            .collect();
        let actual = painted_scalar(&face, None, &values);
        assert_eq!(
            actual.2,
            expected.to_bits(),
            "{name}: ValueEdit actual scalar bits"
        );
        let mut baseline = face.clone();
        baseline.widgets[0].binding = ir::Binding::Control(ir::ControlId(11));
        let reference = painted_scalar(
            &baseline,
            None,
            &Values::from([(ir::ControlId(11), expected)]),
        );
        assert_eq!(
            actual.0, reference.0,
            "{name}: ValueEdit complete text pixels"
        );
        assert_eq!(
            actual.1, reference.1,
            "{name}: ValueEdit complete scene geometry"
        );
    }
}

#[test]
fn authored_default_and_typed_strip_changes_keep_one_read_and_decode() {
    let (_directory, path, asset) = patterned_strip();
    let mut face = scalar_face(ir::Kind::Knob {
        range: scalar_range(),
        display: Default::default(),
    });
    face.assets.push(asset);
    face.widgets[0]
        .images
        .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
    let mut worker = Preparation::new_for_program(&path, 0);
    let values = Values::default();
    for (typed, expected_frame, expected_work) in [
        (None, 4, 1),
        (Some(ir::Value::Integer(0)), 2, 2),
        (None, 4, 2),
    ] {
        face.widgets[0].value = typed;
        let start = std::time::Instant::now();
        loop {
            worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
            if worker.pending() == 0 {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(worker.failures().is_empty());
        assert_eq!(worker.wanted.len(), 1);
        assert_eq!(worker.wanted[0].frame, expected_frame);
        assert_eq!(
            (
                worker.scan.lookups,
                worker.scan.decodes,
                worker.scan.preparation_completed,
                worker.scan.strip_cache_hits
            ),
            (1, 1, expected_work, expected_work - 1),
            "one raw strip read/decode; one preparation per distinct demanded key"
        );
        for _ in 0..3 {
            assert!(
                worker
                    .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
                    .is_empty()
            );
            assert!(worker.unchanged());
        }
        assert_eq!(
            (
                worker.scan.lookups,
                worker.scan.decodes,
                worker.scan.preparation_completed,
                worker.scan.strip_cache_hits
            ),
            (1, 1, expected_work, expected_work - 1),
            "settled requests must add no reads or decodes"
        );
    }
}
