use std::time::Duration;

use kontra_ui_model::{
    Asset, AssetKind, AssetRef, ImageMeta, ImageUse, Page, Range, Rect, Rgba, Role, Size, Widget,
};
use mui::prelude::{Input, Size as ViewSize};
use mui::vello;

use super::*;

fn png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = e.write_header() {
        assert!(w.write_image_data(rgba).is_ok());
    }
    out
}

/// `el` painted on the CPU, as RGBA rows.
fn paint(ui: &mut Ui, el: El, w: u16, h: u16) -> Vec<[u8; 4]> {
    assert!(
        ui.frame(
            el,
            Some(ViewSize::new(f64::from(w), f64::from(h))),
            Input::default(),
            1. / 60.
        )
        .is_ok()
    );
    let mut ctx = vello::vello_cpu::RenderContext::new(w, h);
    let mut resources = vello::vello_cpu::Resources::default();
    if let Some(scene) = ui.scene() {
        let cpu = &mut vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        };
        assert!(vello::paint(cpu, scene, vello::kurbo::Affine::IDENTITY).is_ok());
    }
    ctx.flush();
    let mut pix = vello::vello_cpu::Pixmap::new(w, h);
    ctx.render(&mut pix, &mut resources);
    pix.take_unpremultiplied()
        .iter()
        .map(|p| [p.r, p.g, p.b, p.a])
        .collect()
}

/// A 20x10 page: grey, a wallpaper 10 px tall whose top 4 rows the skin
/// offset scrolls away, and a knob whose 2-frame strip shows its value.
fn tree() -> (Interface, Vec<u8>, Vec<u8>) {
    let red_green: Vec<u8> = (0..20 * 14)
        .flat_map(|i| {
            if i < 20 * 4 {
                [255, 0, 0, 255]
            } else {
                [0, 255, 0, 255]
            }
        })
        .collect();
    let blue_white: Vec<u8> = (0..4 * 8)
        .flat_map(|i| {
            if i < 4 * 4 {
                [0, 0, 255, 255]
            } else {
                [255; 4]
            }
        })
        .collect();
    let mut tree = Interface::default();
    let mut page = Page {
        size: Size {
            width: 20.,
            height: 10.,
        },
        ..Page::default()
    };
    page.background.color = Some(Rgba::rgb(0x808080));
    page.background.image = Some(AssetRef(0));
    page.background.offset_y = 4;
    tree.pages.push(page);
    tree.assets.push(Asset {
        path: "wall.png".into(),
        kind: AssetKind::Image(ImageMeta::default()),
    });
    tree.assets.push(Asset {
        path: "knob.png".into(),
        kind: AssetKind::Image(ImageMeta {
            frames: 2,
            size: Some(Size {
                width: 4.,
                height: 4.,
            }),
            ..ImageMeta::default()
        }),
    });
    let range = Range {
        min: 0.,
        max: 10.,
        ..Range::default()
    };
    let mut knob = Widget::new(
        "$knob",
        PageRef(0),
        Rect::new(2, 2, 30, 30),
        Kind::Knob {
            range,
            display: model::Display::default(),
        },
    );
    knob.images.push(ImageUse::new(AssetRef(1), Role::Strip));
    knob.value = Some(model::Value::Integer(10));
    tree.widgets.push(knob);
    (tree, png(20, 14, &red_green), png(4, 8, &blue_white))
}

fn pictures(wall: Vec<u8>, knob: Vec<u8>) -> Pictures {
    Pictures::new(Box::new(move |p| match p {
        "wall.png" => Some(wall.clone()),
        "knob.png" => Some(knob.clone()),
        _ => None,
    }))
}

/// Frames of `tree` until every picture they ask for is back, and how
/// many widgets they built.
fn settle(ui: &mut Ui, tree: &Interface, pictures: &mut Pictures, scale: f64) -> usize {
    let mut built = 0;
    for _ in 0..2000 {
        let (el, b) = page(ui, 0, tree, PageRef(0), pictures, (scale, &[]));
        assert!(
            ui.frame(
                el,
                Some(ViewSize::new(40., 20.)),
                Input::default(),
                1. / 60.
            )
            .is_ok()
        );
        built += b;
        pictures.poll();
        if pictures.pending() == 0 {
            let (el, b) = page(ui, 0, tree, PageRef(0), pictures, (scale, &[]));
            assert!(
                ui.frame(
                    el,
                    Some(ViewSize::new(40., 20.)),
                    Input::default(),
                    1. / 60.
                )
                .is_ok()
            );
            return built + b;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    built
}

#[test]
fn a_page_draws_its_wallpaper_from_the_skin_offset_and_a_knob_at_its_value() {
    let (mut tree, wall, knob) = tree();
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    // The first frame waits on no picture: it is the page colour.
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(
        paint(&mut ui, el, 20, 10).first(),
        Some(&[128, 128, 128, 255])
    );
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 20, 10);
    let at = |x: usize, y: usize| px.get(y * 20 + x).copied();
    assert_eq!(
        at(0, 0),
        Some([0, 255, 0, 255]),
        "the offset hides the red rows"
    );
    // The strip's frame size wins over the control's 30x30.
    assert_eq!(
        at(3, 3),
        Some([255; 4]),
        "value 10 of 0..10 is the last frame"
    );
    assert_eq!(at(7, 3), Some([0, 255, 0, 255]));
    if let Some(w) = tree.widgets.first_mut() {
        w.value = Some(model::Value::Integer(0));
    }
    // The shown frame stands in until the new one is decoded.
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(
        paint(&mut ui, el, 20, 10).get(3 * 20 + 3).copied(),
        Some([255; 4])
    );
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(
        paint(&mut ui, el, 20, 10).get(3 * 20 + 3).copied(),
        Some([0, 0, 255, 255])
    );
}

#[test]
fn a_vector_page_draws_its_controls_without_their_pictures() {
    let (tree, wall, knob) = tree();
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = vector_page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 20, 10);
    let at = |x: usize, y: usize| px.get(y * 20 + x).copied();
    assert_eq!(at(0, 0), Some([128, 128, 128, 255]), "no wallpaper");
    assert!(!px.contains(&[255; 4]), "no knob strip");
    assert!(
        px.iter().any(|p| *p != [128, 128, 128, 255]),
        "a stock knob"
    );
}

#[test]
fn an_idle_frame_builds_no_widget_and_a_moved_knob_builds_one() {
    let (mut tree, wall, knob) = tree();
    let mut second = tree
        .widgets
        .first()
        .cloned()
        .unwrap_or_else(|| Widget::new("", PageRef(0), Rect::default(), Kind::Label));
    second.rect = Rect::new(10, 2, 4, 4);
    tree.widgets.push(second);
    let mut pictures = pictures(wall, knob);
    let mut ui = crate::ui();
    settle(&mut ui, &tree, &mut pictures, 2.);
    assert_eq!(
        settle(&mut ui, &tree, &mut pictures, 2.),
        0,
        "nothing moved, nothing built"
    );
    if let Some(w) = tree.widgets.get_mut(1) {
        w.value = Some(model::Value::Integer(3));
    }
    assert_eq!(settle(&mut ui, &tree, &mut pictures, 2.), 1);
}

#[test]
fn a_skin_offset_runs_through_the_wallpaper_across_its_frames() {
    // Una Corda: four 569-pixel frames, a 68-pixel header, the second tab
    // at a skin offset of 1138: the page starts 68 rows into frame 2.
    assert_eq!(wallpaper((0, 68, 1138), 569., 4), [(2, -68.), (3, 501.)]);
    // No offset: frame 0 from the header down, frame 1 under it.
    assert_eq!(wallpaper((0, 68, 0), 569., 4), [(0, -68.), (1, 501.)]);
    // A picture state picks the frame; a one-frame file shows only it.
    assert_eq!(wallpaper((1, 0, 0), 500., 2), [(1, 0.)]);
    assert_eq!(wallpaper((0, 68, 0), 569., 1), [(0, -68.)]);
}

/// An attached level meter lights from the bottom with the part's peak; a
/// frame at the same level rebuilds nothing, so a silent part draws no new
/// frames.
#[test]
fn a_level_meter_lights_with_the_peak_and_idles_when_it_holds() {
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: Size {
            width: 4.,
            height: 10.,
        },
        ..Page::default()
    });
    let mut m = Widget::new(
        "$meter",
        PageRef(0),
        Rect::new(0, 0, 4, 10),
        Kind::LevelMeter {
            orientation: model::Orientation::Vertical,
            channel: Some(0),
            bus: None,
        },
    );
    m.colors.on = Some(Rgba::rgb(0xff0000));
    m.colors.off = Some(Rgba::rgb(0x0000ff));
    tree.widgets.push(m);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    // −27 dB on its channel (left) lights half the bar; the right is silent.
    let half = [2f64.powf(-4.5), 0.];
    let (el, built) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[half]));
    assert_eq!(built, 1);
    let pix = paint(&mut ui, el, 4, 10);
    assert_eq!(pix.get(4 * 2), Some(&[0, 0, 255, 255]));
    assert_eq!(pix.get(4 * 8), Some(&[255, 0, 0, 255]));
    let (_, again) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[half]));
    assert_eq!(again, 0);
    let (_, louder) = page(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[[1., 0.]]),
    );
    assert_eq!(louder, 1);
}

/// A script value edit with no picture draws Kontakt's dark stock field
/// (Pacific's light font-13 words read on it); hiding its background
/// leaves the page showing through.
#[test]
fn an_unpictured_script_control_draws_its_stock_field() {
    let mut tree = Interface {
        source: model::Source::Ksp { slot: 0 },
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 20.,
            height: 10.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0xeeeee4)),
            ..model::Background::default()
        },
        ..Page::default()
    });
    let mut edit = Widget::new(
        "$edit",
        PageRef(0),
        Rect::new(0, 0, 20, 10),
        Kind::ValueEdit {
            range: Range::default(),
            display: model::Display::default(),
            arrows: false,
        },
    );
    edit.text = "Transpose".into();
    edit.value = Some(model::Value::Integer(0));
    tree.widgets.push(edit);
    assert_eq!(caption(&tree.widgets[0], tree.source), "Transpose 0");
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 20, 10);
    let dark = px.get(20 + 2).is_some_and(|p| p[0] < 80);
    assert!(dark, "a stock field under the words");
    if let Some(w) = tree.widgets.first_mut() {
        w.hide.background = true;
    }
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(
        paint(&mut ui, el, 20, 10).get(20 + 2),
        Some(&[0xee, 0xee, 0xe4, 255])
    );
}

/// A resizable picture keeps its fixed ends and stretches the rest
/// (CHORUS's selected-row glow: 26 px, 12 px fixed each side, on a 260
/// point row); a control shorter than both ends shrinks them alike.
#[test]
fn a_resizable_picture_keeps_its_fixed_ends() {
    assert_eq!(
        cuts(true, (12, 12), (26., 26), 260., 1.),
        [(0, 12, 12.), (12, 2, 236.), (14, 12, 12.)]
    );
    assert_eq!(
        cuts(true, (12, 12), (26., 39), 390., 1.5),
        [(0, 18, 18.), (18, 3, 354.), (21, 18, 18.)],
        "decoded at 1.5x, drawn at 1.5 points a pixel"
    );
    assert_eq!(
        cuts(true, (12, 12), (26., 26), 12., 1.),
        [(0, 12, 6.), (14, 12, 6.)]
    );
    assert_eq!(cuts(false, (12, 12), (26., 26), 260., 1.), [(0, 26, 260.)]);
}

#[test]
fn a_note_names_value_edit_reads_its_note() {
    let mut w = Widget::new(
        "$ks",
        PageRef(0),
        Rect::new(0, 0, 70, 18),
        Kind::ValueEdit {
            range: Range::default(),
            display: model::Display {
                ratio: 0.,
                ..model::Display::default()
            },
            arrows: false,
        },
    );
    w.value = Some(model::Value::Integer(12));
    assert_eq!(readout(&w, model::Source::Ksp { slot: 0 }), "C-1");
}

#[test]
fn a_script_switch_with_no_picture_draws_kontakts_stock_button() {
    let mut tree = Interface {
        source: model::Source::Ksp { slot: 0 },
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 20.,
            height: 10.,
        },
        ..Page::default()
    });
    tree.widgets.push(Widget::new(
        "$s",
        PageRef(0),
        Rect::new(0, 0, 20, 10),
        Kind::Switch,
    ));
    let mut pictures = Pictures::new(Box::new(|_| None));
    let mut shade = |on: i32| {
        tree.widgets[0].value = Some(model::Value::Integer(on));
        let mut ui = crate::ui();
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
        paint(&mut ui, el, 20, 10).get(20 * 5 + 2).copied()
    };
    // Off: Kontakt's dark grey; on: its light face (measured on Pacific).
    assert_eq!(shade(0), Some([0x5f, 0x5f, 0x5f, 255]));
    assert!(shade(1).is_some_and(|p| p[0] > 0xe0));
}

/// A label in a library picture font draws that font's glyph pixels, not
/// an outline face (Shreddage's RoadGeek FX labels).
#[test]
fn a_picture_font_label_draws_its_glyphs() {
    // 256 one-pixel glyphs under a row of red markers; 'A' is green.
    let mut rgba = vec![[255, 0, 0, 255]; 256];
    rgba.extend((0..256).map(|n| if n == 65 { [0, 255, 0, 255] } else { [0; 4] }));
    let font = png(256, 2, rgba.as_flattened());
    let mut tree = Interface {
        source: model::Source::Ksp { slot: 0 },
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 8.,
            height: 1.,
        },
        ..Page::default()
    });
    tree.assets.push(Asset {
        path: "font.png".into(),
        kind: AssetKind::BitmapFont,
    });
    tree.styles.push(model::TextStyle {
        font: model::Font::Bitmap(AssetRef(0)),
        size: None,
        color: Rgba::rgb(0xffffff),
        align: model::Align::Left,
    });
    let mut label = Widget::new("$l", PageRef(0), Rect::new(0, 0, 8, 1), Kind::Label);
    label.text = "AA".into();
    label.style = Some(model::StyleRef(0));
    tree.widgets.push(label);
    let mut pictures = Pictures::new(Box::new(move |p| (p == "font.png").then(|| font.clone())));
    let mut ui = crate::ui();
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 8, 1);
    // Left-aligned two pixels in: one green pixel per 'A'.
    assert_eq!(px.get(2..4), Some(&[[0, 255, 0, 255]; 2][..]), "{px:?}");
}

#[test]
fn vector_draws_a_picture_slider_even_when_its_background_is_hidden() {
    let (mut tree, wall, knob) = tree();
    let widget = tree.widgets.first_mut().unwrap();
    widget.kind = Kind::Slider {
        range: Range {
            min: 0.,
            max: 1.,
            default: 0.,
            step: None,
        },
        orientation: model::Orientation::Vertical,
    };
    widget.hide.background = true;
    assert!(!stock::shows(&tree, &tree.widgets[0], false));
    assert!(stock::shows(&tree, &tree.widgets[0], true));
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    let (el, _) = vector_page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let pixels = paint(&mut ui, el, 20, 10);
    assert!(pixels.iter().any(|p| *p != [128, 128, 128, 255]));
}

#[test]
fn original_stock_knob_has_a_lit_upper_ring_and_shadow_below() {
    let mut tree = Interface {
        source: model::Source::Ksp { slot: 0 },
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 90.,
            height: 40.,
        },
        ..Page::default()
    });
    tree.widgets.push(Widget::new(
        "$gain",
        PageRef(0),
        Rect::new(0, 0, 38, 40),
        Kind::Knob {
            range: Range::default(),
            display: model::Display::default(),
        },
    ));
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let pixels = paint(&mut ui, el, 90, 40);
    let upper = pixels[4 * 90 + 19][0];
    let lower = pixels[34 * 90 + 19][0];
    assert!(upper > lower + 10, "upper ring {upper}, lower ring {lower}");
}

#[test]
fn uvi_table_draws_fractional_bars_and_repaints_after_a_script_write() {
    let mut tree = Interface {
        source: model::Source::FalconLua,
        ..Default::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 20.,
            height: 10.,
        },
        ..Default::default()
    });
    let mut w = Widget::new(
        "curve",
        PageRef(0),
        Rect {
            x: 0.,
            y: 0.,
            width: 20.,
            height: 10.,
        },
        Kind::Table {
            columns: 2,
            range: Range {
                min: 0.,
                max: 1.,
                default: 0.,
                step: None,
            },
            bipolar: false,
            cells: vec![0.25, 0.75],
            steps_shown: None,
        },
    );
    w.colors.background = Some(Rgba::rgb(0x000000));
    w.colors.bar = Some(Rgba::rgb(0xff0000));
    w.value = Some(model::Value::Reals(vec![0.25, 0.75]));
    tree.widgets.push(w);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let before = paint(&mut ui, el, 20, 10);
    assert_eq!(before.get(5 * 20 + 5), Some(&[0, 0, 0, 255]));
    assert_eq!(before.get(5 * 20 + 15), Some(&[255, 0, 0, 255]));
    tree.widgets[0].value = Some(model::Value::Reals(vec![0.75, 0.25]));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let after = paint(&mut ui, el, 20, 10);
    assert_eq!(after.get(5 * 20 + 5), Some(&[255, 0, 0, 255]));
    assert_eq!(after.get(5 * 20 + 15), Some(&[0, 0, 0, 255]));
}

/// Hovering a script control shows its `set_control_help` text.
#[test]
fn hovering_a_control_shows_its_help() {
    let (mut tree, wall, knob) = tree();
    if let Some(w) = tree.widgets.first_mut() {
        w.tooltip = "Filter cutoff".into();
    }
    let mut pictures = pictures(wall, knob);
    let mut ui = crate::ui();
    let hover = mui::prelude::PointerInput {
        pos: Some(mui::prelude::Point::new(8., 8.)),
        ..mui::prelude::PointerInput::default()
    };
    let mut tip = None;
    for dt in [0.016, 0.016, 1.0] {
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (2., &[]));
        let f = ui.frame(el, Some(ViewSize::new(40., 20.)), hover, dt);
        tip = f.ok().and_then(|f| f.tip.map(|(t, _)| t));
    }
    assert_eq!(tip.as_deref(), Some("Filter cutoff"));
}

#[test]
fn multiline_labels_draw_each_line_and_keep_empty_rows() {
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: Size {
            width: 80.,
            height: 80.,
        },
        ..Default::default()
    });
    let mut label = Widget::new("$list", PageRef(0), Rect::new(0, 0, 80, 80), Kind::Label);
    label.text = "ONE\n\nTWO\n\nTHREE".into();
    label.text_y = Some(0);
    tree.widgets.push(label);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let pixels = paint(&mut ui, el, 80, 80);
    let ink = |from: usize, to: usize| {
        pixels
            .iter()
            .skip(from * 80)
            .take((to - from) * 80)
            .filter(|p| p[0] > 30)
            .count()
    };
    assert!(ink(0, 13) > 5, "first line");
    assert_eq!(ink(13, 26), 0, "empty line");
    assert!(ink(26, 39) > 5, "second line");
    assert_eq!(ink(39, 52), 0, "empty line");
    assert!(ink(52, 65) > 5, "third line");
}

#[test]
fn kontakt_label_tabs_are_blank_advances() {
    let mut widget = Widget::new("$label", PageRef(0), Rect::default(), Kind::Label);
    widget.text = "\t\t\t   Marcato Short".into();
    assert_eq!(
        plan::caption(&widget, model::Source::Ksp { slot: 0 }),
        "         Marcato Short"
    );
    assert_eq!(
        plan::caption(&widget, model::Source::FalconLua),
        widget.text
    );
}

#[test]
fn a_meter_lights_as_kontakt_maps_its_peak() {
    // Linear in dB over nine octaves: 0 dB full, -27 dB half, -54 dB dark.
    let at = [1., 2_f64.powf(-4.5), 2_f64.powi(-9), 2.];
    assert_eq!(at.map(super::lit), [1., 0.5, 0., 1.]);
    assert_eq!(super::lit(0.5), 0.89);
}
