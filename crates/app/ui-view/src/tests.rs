use std::time::Duration;

use kontra_ui_model::{
    Asset, AssetKind, AssetRef, ImageMeta, ImageUse, Page, Range, Rect, Rgba, Role, Size, Widget,
};
use mui::prelude::{Input, Size as ViewSize};

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
    let rgba = ui
        .scene()
        .map(|s| mui_headless::Raster::new(w, h).draw(s, 1.0));
    assert!(matches!(rgba, Some(Ok(_))), "painted on the CPU");
    rgba.and_then(Result::ok)
        .unwrap_or_default()
        .as_chunks::<4>()
        .0
        .to_vec()
}

#[test]
fn stock_atlas_label_uses_the_authored_left_inset() {
    let glyph = Arc::new(Image::rgba(1, 1, vec![255; 4]).unwrap());
    let glyphs: kontra_ui_pictures::Glyphs = vec![glyph; 256].into();
    let style = model::TextStyle {
        align: model::Align::Left,
        color: Rgba::rgb(0xff0000),
        ..model::TextStyle::default()
    };
    let label = stock::label(
        "A",
        &style,
        Some(0),
        (24., 8., 1., 1.),
        (13., 3.),
        (&glyphs, 3),
    )
    .unwrap();
    let pixels = paint(&mut Ui::default(), label, 24, 8);
    assert_eq!(pixels.iter().position(|p| *p == [255, 0, 0, 255]), Some(13));
}

#[test]
fn a_centred_atlas_label_sits_one_pixel_right_of_the_middle() {
    let glyph = Arc::new(Image::rgba(2, 1, vec![255; 8]).unwrap());
    let glyphs: kontra_ui_pictures::Glyphs = vec![glyph; 256].into();
    let style = model::TextStyle {
        align: model::Align::Center,
        color: Rgba::rgb(0xff0000),
        ..model::TextStyle::default()
    };
    // 20 px, 2 px each side and a 2 px glyph: the middle is x 9, Kontakt 10.
    let label = stock::label(
        "A",
        &style,
        Some(0),
        (20., 4., 1., 1.),
        (2., 2.),
        (&glyphs, 3),
    );
    let pixels = paint(&mut Ui::default(), label.unwrap(), 20, 4);
    assert_eq!(pixels.iter().position(|p| *p == [255, 0, 0, 255]), Some(10));
}

#[test]
fn fonts_1_and_13_count_an_unset_textpos_y_as_0_then_drop_2_px() {
    let (pixel, face) = (model::Font::Stock(0), model::Font::Stock(13));
    assert_eq!(super::atlas_top(&pixel, None), None);
    assert_eq!(super::atlas_top(&pixel, Some(0)), Some(0));
    assert_eq!(super::atlas_top(&face, None), Some(2));
    assert_eq!(super::atlas_top(&face, Some(0)), Some(2));
    assert_eq!(super::atlas_top(&face, Some(6)), Some(8));
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
fn a_kontra_page_draws_its_controls_on_the_wallpaper_without_their_pictures() {
    let (tree, wall, knob) = tree();
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    settle(&mut ui, &tree, &mut pictures, 1.);
    let kontra = Presentation::Kontra;
    let (el, _) = page_as(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[]),
        kontra,
    );
    let px = paint(&mut ui, el, 20, 10);
    let at = |x: usize, y: usize| px.get(y * 20 + x).copied();
    assert_eq!(at(0, 0), Some([0, 255, 0, 255]), "the wallpaper stays");
    assert!(!px.contains(&[255; 4]), "no knob strip");
    assert!(
        px.iter()
            .any(|p| ![[0, 255, 0, 255], [128, 128, 128, 255]].contains(p)),
        "a stock knob"
    );
}

/// A wordless button's picture is its label (an icon): KONTRA keeps it
/// rather than draw a blank stock button.
#[test]
fn a_kontra_page_keeps_a_wordless_buttons_picture() {
    let (mut tree, wall, knob) = tree();
    tree.widgets[0].kind = Kind::Button { momentary: false };
    assert!(tree.widgets[0].label_in_image());
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    settle(&mut ui, &tree, &mut pictures, 1.);
    let kontra = Presentation::Kontra;
    let (el, _) = page_as(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[]),
        kontra,
    );
    let px = paint(&mut ui, el, 20, 10);
    assert!(
        px.iter().any(|p| [[255; 4], [0, 0, 255, 255]].contains(p)),
        "its picture"
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
    // Half lit on its channel (left); the right is dark.
    let lit = |level| crate::Shown {
        level,
        ..Default::default()
    };
    let half = [lit(0.5), lit(0.)];
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
        (1., &[[lit(1.), lit(0.)]]),
    );
    assert_eq!(louder, 1);
}

/// A script value edit with no picture draws Kontakt's dark stock field
/// (Pacific's light font-13 words read on it); hiding its background
/// leaves the page showing through.
#[test]
fn an_unpictured_script_control_draws_its_stock_field() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
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
    assert_eq!(caption(&tree.widgets[0]), "Transpose 0");
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

/// A value edit narrower than its arrows still lays out: its words get no
/// room rather than less than none (Conflux's 6.6-point fine-tune fields,
/// whose refusal froze the whole editor on the old frame).
#[test]
fn a_value_edit_narrower_than_its_arrows_still_draws() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 20.,
            height: 10.,
        },
        ..Page::default()
    });
    let mut edit = Widget::new(
        "$fine",
        PageRef(0),
        Rect::new(0, 0, 6, 10),
        Kind::ValueEdit {
            range: Range::default(),
            display: model::Display::default(),
            arrows: true,
        },
    );
    edit.value = Some(model::Value::Integer(0));
    tree.widgets.push(edit);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    paint(&mut ui, el, 20, 10);
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
    assert_eq!(readout(&w), "C-1");
}

#[test]
fn a_script_switch_with_no_picture_draws_kontakts_stock_button() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
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

/// A switch picture of any frame count sits one pixel right and down in its
/// face, drawn whole (Kontakt 8, probed at 100 % and 150 %).
#[test]
fn a_switch_picture_sits_one_pixel_in() {
    let mut tree = Interface::default();
    let mut face = Page {
        size: Size {
            width: 8.,
            height: 4.,
        },
        ..Page::default()
    };
    face.background.color = Some(Rgba::rgb(0x808080));
    tree.pages.push(face);
    tree.assets.push(Asset {
        path: "sw.png".into(),
        kind: AssetKind::Image(ImageMeta {
            frames: 2,
            size: Some(Size {
                width: 4.,
                height: 2.,
            }),
            ..ImageMeta::default()
        }),
    });
    let mut switch = Widget::new("$s", PageRef(0), Rect::new(0, 0, 5, 3), Kind::Switch);
    let mut img = ImageUse::new(AssetRef(0), Role::Strip);
    img.inset = [1.0, 1.0];
    switch.images.push(img);
    switch.value = Some(model::Value::Integer(0));
    tree.widgets.push(switch);
    let red = png(4, 4, [255, 0, 0, 255].repeat(16).as_slice());
    let mut pictures = Pictures::new(Box::new(move |p| (p == "sw.png").then(|| red.clone())));
    let mut ui = crate::ui();
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 8, 4);
    let at = |x: usize, y: usize| px.get(y * 8 + x).copied();
    assert_eq!(at(0, 0), Some([128, 128, 128, 255]), "the corner is page");
    assert_eq!(at(1, 1), Some([255, 0, 0, 255]), "the picture starts 1 in");
    assert_eq!(at(4, 2), Some([255, 0, 0, 255]), "and runs its whole 4 x 2");
    assert_eq!(at(5, 2), Some([128, 128, 128, 255]), "the control clips it");
}

/// A label in a library picture font draws that font's glyph pixels, not
/// an outline face (Shreddage's RoadGeek FX labels).
#[test]
fn a_picture_font_label_draws_its_glyphs() {
    // 256 one-pixel glyphs under a row of red markers; 'A' is green.
    let mut rgba = vec![[255, 0, 0, 255]; 256];
    rgba.extend((0..256).map(|n| if n == 65 { [0, 255, 0, 255] } else { [0; 4] }));
    let font = png(256, 2, rgba.as_flattened());
    let mut tree = Interface::default();
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
        ..model::TextStyle::default()
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
fn kontra_draws_a_picture_slider_even_when_its_background_is_hidden() {
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
    let kontra = Presentation::Kontra;
    let (el, _) = page_as(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[]),
        kontra,
    );
    let pixels = paint(&mut ui, el, 20, 10);
    assert!(pixels.iter().any(|p| *p != [128, 128, 128, 255]));
}

#[test]
fn original_stock_knob_has_a_lit_upper_ring_and_shadow_below() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
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
        table_style: model::TableStyle::UVI,
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
fn label_caption_preserves_translator_normalized_text() {
    let mut widget = Widget::new("$label", PageRef(0), Rect::default(), Kind::Label);
    widget.text = "         Marcato Short".into();
    assert_eq!(plan::caption(&widget), "         Marcato Short");
}

#[test]
fn the_wheel_over_a_scrolling_region_never_scrolls_the_rack() {
    let at = |y, h| Rect {
        x: 0.,
        y,
        width: 100.,
        height: h,
    };
    let mut port = Widget::new("port", PageRef(0), at(0., 100.), Kind::Panel);
    port.viewport = Some([0, 0]);
    let mut rows = Widget::new("rows", PageRef(0), at(0., 300.), Kind::Panel);
    rows.parent = Some(model::WidgetRef(0));
    let tree = Interface {
        pages: vec![Page {
            size: Size {
                width: 200.,
                height: 200.,
            },
            ..Page::default()
        }],
        widgets: vec![port, rows],
        ..Interface::default()
    };
    let mut pictures = pictures(Vec::new(), Vec::new());
    let mut ui = Ui::default();
    // Far past the region's end: what is left over stays with it.
    for n in 0..12 {
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
        let rack = col![col![el, block(200., 600.)].scroll().h(200.).id("rack")];
        let input = Input {
            pointer: mui::prelude::PointerInput {
                pos: Some(Point::new(50., 50.)),
                ..Default::default()
            },
            wheel: Vec2::new(0., if n >= 2 { 60. } else { 0. }),
            ..Default::default()
        };
        assert!(
            ui.frame(rack, Some(ViewSize::new(200., 200.)), input, 1. / 60.)
                .is_ok()
        );
    }
    assert!(ui.scroll("kontra-v-0-0")[1] > 0., "the region scrolls");
    assert_eq!(ui.scroll("rack")[1], 0., "the rack does not");
}

/// A widget in a scrolling region sits where the script put it in that
/// region, the band a page shows above its y 0 counted once.
#[test]
fn a_widget_in_a_scrolling_region_keeps_its_place_under_the_band() {
    let at = |y, h| Rect {
        x: 0.,
        y,
        width: 100.,
        height: h,
    };
    let mut port = Widget::new("port", PageRef(0), at(10., 100.), Kind::Panel);
    port.viewport = Some([0, 0]);
    let mut go = Widget::new(
        "go",
        PageRef(0),
        at(0., 20.),
        Kind::Button { momentary: true },
    );
    go.parent = Some(model::WidgetRef(0));
    let tree = Interface {
        pages: vec![Page {
            size: Size {
                width: 200.,
                height: 200.,
            },
            background: model::Background {
                above: 4,
                ..model::Background::default()
            },
            ..Page::default()
        }],
        widgets: vec![port, go],
        ..Interface::default()
    };
    let mut pictures = pictures(Vec::new(), Vec::new());
    let mut ui = Ui::default();
    for _ in 0..2 {
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
        assert!(
            ui.frame(
                el,
                Some(ViewSize::new(200., 210.)),
                Input::default(),
                1. / 60.
            )
            .is_ok()
        );
    }
    let y = |id: &str| ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.y);
    assert_eq!(y(&surface(0, 1)), Some(14.));
}

#[test]
fn a_pixel_font_stem_at_150_percent_is_one_full_and_one_half_column() {
    // Font pixels 0 and 1 over three screen pixels: 0 fills the first and
    // half the second, 1 the other half and the third.
    let cols = crate::stock::area(2, 3, 1.5);
    let share = |i: usize, j: usize| {
        cols.get(i)
            .and_then(|c| c.iter().find(|&&(k, _)| k == j))
            .map_or(0., |&(_, w)| w)
    };
    assert!((share(0, 0) - 1.).abs() < 1e-9);
    assert!((share(1, 0) - 0.5).abs() < 1e-9);
    assert!((share(1, 1) - 0.5).abs() < 1e-9);
    assert!((share(2, 1) - 1.).abs() < 1e-9);
}

/// Kontakt's grey bare-label face is a script `ui_label`'s only: a
/// native_ui text on a KSP page (no UI id) draws none (Conflux's tabs and
/// captions turned into grey boxes, #1507).
#[test]
fn only_a_script_label_draws_kontakts_bare_label_face() {
    let tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    let mut label = Widget::new("Edit", PageRef(0), Rect::new(0, 0, 40, 12), Kind::Label);
    assert!(crate::stock::label_face(&tree, &label, (40., 12., 1.)).is_none());
    label.source_id = Some(7);
    assert!(crate::stock::label_face(&tree, &label, (40., 12., 1.)).is_some());
}

/// At 125 %, 150 % and 200 % a picture at a whole logical place paints its
/// device-scaled pixels as they are: its edges sit on whole pixels, so the
/// renderer never filters it a second time at a half-pixel offset.
#[test]
fn a_picture_lands_on_whole_pixels_at_every_zoom() {
    let row: Vec<u8> = (0..8)
        .flat_map(|i| if i % 2 == 0 { [255; 4] } else { [0, 0, 0, 255] })
        .collect();
    for scale in [1.25, 1.5, 2.] {
        let mut tree = Interface::default();
        let mut face = Page {
            size: Size {
                width: 12.,
                height: 4.,
            },
            ..Page::default()
        };
        face.background.color = Some(Rgba::rgb(0));
        tree.pages.push(face);
        tree.assets.push(Asset {
            path: "p.png".into(),
            kind: AssetKind::Image(ImageMeta {
                frames: 1,
                size: Some(Size {
                    width: 8.,
                    height: 2.,
                }),
                ..ImageMeta::default()
            }),
        });
        let mut label = Widget::new("$l", PageRef(0), Rect::new(1, 1, 8, 2), Kind::Label);
        label.images.push(ImageUse::new(AssetRef(0), Role::Strip));
        tree.widgets.push(label);
        let file = png(8, 2, &row.repeat(2));
        let mut pictures = Pictures::new(Box::new(move |p| (p == "p.png").then(|| file.clone())));
        let mut ui = crate::ui();
        settle(&mut ui, &tree, &mut pictures, scale);
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (scale, &[]));
        let w = (12. * scale).ceil() as usize;
        let px = paint(&mut ui, el, w as u16, (4. * scale).ceil() as u16);
        let (x0, y) = ((scale).round() as usize, (1.5 * scale) as usize);
        let want: Vec<u8> = Image::rgba(8, 2, row.repeat(2))
            .and_then(|i| kontra_ui_pictures::at(i, scale as f32))
            .map(|i| {
                i.rgba
                    .chunks(4)
                    .take(i.width as usize)
                    .map(|p| p[0])
                    .collect()
            })
            .unwrap_or_default();
        let got: Vec<u8> = (x0..x0 + want.len())
            .map(|x| px.get(y * w + x).map_or(0, |p| p[0]))
            .collect();
        assert!(!want.is_empty());
        assert_eq!(got, want, "at {scale}");
    }
}

/// At 50 % (the smallest editor size) a pixel font's two pixels share one
/// device pixel half and half: Kontakt's area law, read down as well as up.
#[test]
fn a_pixel_font_at_50_percent_averages_each_pair_of_pixels() {
    let cols = crate::stock::area(4, 2, 0.5);
    assert_eq!(cols, [vec![(0, 0.5), (1, 0.5)], vec![(2, 0.5), (3, 0.5)]]);
}

/// A stock XY pad as Kontakt 8 draws one (probe/xy.ksp at 100 %): the
/// frame line 7 in, the active cursor filled, the other hollow, y up.
#[test]
fn a_stock_xy_pad_draws_its_frame_and_cursors_where_kontakt_does() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 200.,
            height: 150.,
        },
        ..Page::default()
    });
    let mut pad = Widget::new(
        "?xy",
        PageRef(0),
        Rect::new(0, 0, 200, 150),
        Kind::Xy {
            cursors: 2,
            sensitivity: [None; 2],
            mouse_mode: None,
        },
    );
    pad.value = Some(model::Value::Reals(vec![0.2, 0.8, 0.8, 0.2]));
    tree.widgets.push(pad);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 200, 150);
    let at = |x: usize, y: usize| px.get(y * 200 + x).map(|p| p[0]);
    // Kontakt: the active cursor's square 38..50 by 28..40, filled #787878.
    assert_eq!(at(44, 34), Some(0x78));
    assert_eq!(at(38, 34), Some(0xFF));
    // The frame line, and the field inside it.
    assert_eq!(at(7, 75), Some(0xA6));
    assert_eq!(at(100, 75), Some(0x3F));
    // The second cursor (0.8, 0.2): its edge at 149, hollow inside.
    assert_eq!(at(149, 115), Some(0xFF));
    assert_eq!(at(155, 115), Some(0x3F));
}

/// A press on a cursor that does not move it edits nothing: its corner,
/// unrounded, gives back its values (200 by 150 at 0.5, where the drawn
/// corner rounds to 94, 69).
#[test]
fn an_xy_cursor_corner_gives_back_its_values() {
    let mut pad = Widget::new(
        "?xy",
        PageRef(0),
        Rect::new(0, 0, 200, 150),
        Kind::Xy {
            cursors: 1,
            sensitivity: [None; 2],
            mouse_mode: None,
        },
    );
    pad.value = Some(model::Value::Reals(vec![0.5, 0.5]));
    for art in [None, Some((2., 2.))] {
        let corner = xy::corner(&pad, 0, (200., 150.), art);
        let back = corner.map(|c| xy::value(c, (200., 150.), art));
        assert!(back.is_some_and(|(x, y)| (x - 0.5).abs() < 1e-9 && (y - 0.5).abs() < 1e-9));
    }
}

/// A meter's range narrows or inverts what it shows (`RANGE_MIN`/`MAX`,
/// default 0 to 1000000; probe/wave.ksp).
#[test]
fn a_meter_range_narrows_and_inverts_its_display() {
    assert!((crate::ranged(0.75, None) - 0.75).abs() < 1e-9);
    assert!((crate::ranged(0.75, Some([0, 500_000])) - 1.5).abs() < 1e-9);
    assert!((crate::ranged(0.75, Some([1_000_000, 0])) - 0.25).abs() < 1e-9);
    assert!((crate::ranged(0.75, Some([500_000, 1_000_000])) - 0.5).abs() < 1e-9);
}

/// A waveform sits in the stock rim on a #3F3F3F field under its
/// background at `BG_ALPHA`: red at 128 shows #9F1F1F (probe/wave.ksp).
#[test]
fn a_waveform_background_blends_over_its_field_at_its_alpha() {
    let mut w = Widget::new("$w", PageRef(0), Rect::new(0, 0, 20, 10), Kind::Waveform);
    w.colors.background = Some(Rgba::rgb(0xFF_0000));
    let wave = model::Waveform {
        zone: 0,
        flags: 0,
        cursor_us: 0,
        table: vec![],
        highlighted: None,
        midi_start_note: 60,
        peaks: vec![[0, 0]; 4],
        length_us: 1,
        xray: false,
        density: vec![],
        frames: vec![],
        frame: 0,
        alpha: [Some(128), None, None, None, None],
    };
    let img = crate::wave::raster(&wave, crate::wave::Look::of(&w), (20, 10, 1.));
    let px = |x: usize, y: usize| {
        let i = (y * 20 + x) * 4;
        img.as_ref()
            .and_then(|img| img.rgba.get(i..i + 4))
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
    };
    assert_eq!(px(0, 0), Some([0x2C, 0x2C, 0x2C, 255]));
    assert_eq!(px(1, 1), Some([0x39, 0x39, 0x39, 255]));
    assert_eq!(px(3, 2), Some([0x9F, 0x1F, 0x1F, 255]));
}

/// A wavetable frame drawn narrower than its points still shows a spike
/// that falls between two pixel edges, and a huge display rasterises
/// within its budget, stretched to its box.
#[test]
fn a_wavetable_keeps_its_spikes_and_its_raster_is_bounded() {
    let w = Widget::new(
        "$t",
        PageRef(0),
        Rect::new(0, 0, 64, 40),
        Kind::Wavetable {
            three_d: false,
            parallax: [0, 0],
        },
    );
    let mut cycle = vec![0i8; 256];
    if let Some(p) = cycle.get_mut(66) {
        *p = 127;
    }
    let wave = model::Waveform {
        zone: 0,
        flags: 0,
        cursor_us: -1,
        table: vec![],
        highlighted: None,
        midi_start_note: 60,
        peaks: vec![],
        length_us: 1,
        xray: false,
        density: vec![],
        frames: vec![cycle],
        frame: 0,
        alpha: [None; 5],
    };
    let look = crate::wave::Look::of(&w);
    let img = crate::wave::raster(&wave, look, (64, 40, 1.));
    // Point 66 of 255 lands in column 16; the top 0.9 of the half height
    // is lit there.
    let lit = |x: usize, y: usize| {
        let i = (y * 64 + x) * 4;
        img.as_ref()
            .and_then(|img| img.rgba.get(i))
            .is_some_and(|&r| r > 0x80)
    };
    assert!((3..8).any(|y| lit(16, y)), "the spike shows");
    let big = crate::wave::raster(&wave, look, (10_000, 10_000, 1.));
    let size = big.as_ref().map(|i| i.rgba.len() / 4);
    assert!(size.is_some_and(|n| n <= 2_000_000), "{size:?}");
}

#[test]
fn state_styles_select_on_hover_and_pressed_fonts_with_fallback() {
    let mut tree = Interface::default();
    tree.pages.push(Page {
        size: Size {
            width: 40.,
            height: 20.,
        },
        ..Page::default()
    });
    let mk_style = |rgb: u32| model::TextStyle {
        font: model::Font::Stock(1),
        size: Some(12.),
        color: Rgba::rgb(rgb),
        align: model::Align::Center,
        ..model::TextStyle::default()
    };
    // 0: off, 1: on, 2: off_pressed, 3: on_hover
    tree.styles.push(mk_style(0x11_1111));
    tree.styles.push(mk_style(0xFF_0000));
    tree.styles.push(mk_style(0x00_FF00));
    tree.styles.push(mk_style(0x00_00FF));

    let mut sw = Widget::new("$sw", PageRef(0), Rect::new(0, 0, 40, 20), Kind::Switch);
    sw.text = "ON".into();
    sw.style = Some(model::StyleRef(0));
    sw.state_styles = [
        Some(model::StyleRef(0)),
        Some(model::StyleRef(1)),
        Some(model::StyleRef(2)),
        None, // on_pressed falls back to state_styles[1] (on)
        None, // off_hover falls back to state_styles[0] (off)
        Some(model::StyleRef(3)),
    ];
    sw.value = Some(model::Value::Integer(0));
    tree.widgets.push(sw);

    let style_of =
        |tree: &Interface, pressed: &[model::WidgetRef], hovered: &[model::WidgetRef]| {
            plan::page(tree, PageRef(0), false, 1., (pressed, hovered))
                .and_then(|p| p.widgets.into_iter().next()?.style_ref)
        };
    let r0 = model::WidgetRef(0);
    assert_eq!(style_of(&tree, &[], &[]), Some(model::StyleRef(0)));
    assert_eq!(style_of(&tree, &[r0], &[]), Some(model::StyleRef(2)));
    assert_eq!(style_of(&tree, &[], &[r0]), Some(model::StyleRef(0)));

    tree.widgets[0].value = Some(model::Value::Integer(1));
    assert_eq!(style_of(&tree, &[], &[]), Some(model::StyleRef(1)));
    assert_eq!(style_of(&tree, &[r0], &[]), Some(model::StyleRef(1)));
    assert_eq!(style_of(&tree, &[], &[r0]), Some(model::StyleRef(3)));
}

#[test]
fn pictureless_switch_caption_uses_the_selected_state_size() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 80.,
            height: 40.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0)),
            ..Default::default()
        },
        ..Page::default()
    });
    for size in [Some(10.), Some(20.), None] {
        tree.styles.push(model::TextStyle {
            font: model::Font::Default,
            size,
            // Red words, told apart from the body $HIDE_PART_BG keeps.
            color: Rgba::rgb(0xff_0000),
            align: model::Align::Center,
            ..model::TextStyle::default()
        });
    }
    let mut sw = Widget::new("$sw", PageRef(0), Rect::new(0, 0, 80, 40), Kind::Switch);
    sw.text = "ON".into();
    sw.hide.background = true;
    sw.state_styles = [
        Some(model::StyleRef(0)),
        Some(model::StyleRef(1)),
        None,
        None,
        None,
        None,
    ];
    tree.widgets.push(sw);

    let mut pictures = Pictures::new(Box::new(|_| None));
    let mut ui = crate::ui();
    let mut ink_rows = |tree: &mut Interface, value, on_style| {
        if let Some(sw) = tree.widgets.first_mut() {
            sw.value = Some(model::Value::Integer(value));
            sw.state_styles[1] = Some(model::StyleRef(on_style));
        }
        let (el, _) = page(&mut ui, 0, tree, PageRef(0), &mut pictures, (1., &[]));
        paint(&mut ui, el, 80, 40)
            .as_chunks::<80>()
            .0
            .iter()
            .filter(|row| row.iter().any(|p| p[0] > 128 && p[1] < 64))
            .count()
    };
    let off = ink_rows(&mut tree, 0, 1);
    let on = ink_rows(&mut tree, 1, 1);
    let fallback = ink_rows(&mut tree, 1, 2);

    assert!(0 < off && off < on, "off={off}, on={on}");
    assert_eq!(fallback, off, "the unspecified size defaults to 10 points");
}

#[test]
fn pressed_button_shifts_caption_only_when_text_shift_on_press_is_enabled() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 40.,
            height: 20.,
        },
        ..Page::default()
    });
    let mut btn = Widget::new(
        "$b",
        PageRef(0),
        Rect::new(0, 0, 40, 20),
        Kind::Button { momentary: false },
    );
    btn.text = "GO".into();
    btn.text_shift_on_press = true;
    tree.widgets.push(btn);

    let r0 = model::WidgetRef(0);
    let plan_idle = plan::page(&tree, PageRef(0), false, 1., (&[], &[])).unwrap();
    assert!(!plan_idle.widgets[0].text_shift);
    let plan_held = plan::page(&tree, PageRef(0), false, 1., (&[r0], &[])).unwrap();
    assert!(plan_held.widgets[0].text_shift);

    tree.widgets[0].text_shift_on_press = false;
    let plan_disabled = plan::page(&tree, PageRef(0), false, 1., (&[r0], &[])).unwrap();
    assert!(!plan_disabled.widgets[0].text_shift);
}

#[test]
fn kontakt_table_honors_steps_shown_bipolar_colors_hide_bg_and_picture() {
    let mut tree = Interface {
        table_style: model::TableStyle::KONTAKT,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 40.,
            height: 20.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0x10_2030)),
            ..model::Background::default()
        },
        ..Page::default()
    });
    // 4-column bipolar table with steps_shown = 2: col 0 (+100) fills left half
    // above center, col 1 (-100) fills right half below center; cols 2..4 hidden.
    let mut table = Widget::new(
        "%t",
        PageRef(0),
        Rect::new(0, 0, 40, 20),
        Kind::Table {
            columns: 4,
            range: Range {
                min: -100.,
                max: 100.,
                default: 0.,
                step: Some(1.),
            },
            bipolar: true,
            cells: vec![100., -100., 100., 100.],
            steps_shown: Some(2),
        },
    );
    table.value = Some(model::Value::Integers(vec![100, -100, 100, 100]));
    table.colors.bar = Some(Rgba::rgb(0xFF_0000));
    table.colors.zero_line = Some(Rgba::rgb(0x00_FF00));
    table.hide.background = true;
    tree.widgets.push(table);

    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 40, 20);
    let at = |x: usize, y: usize| px.get(y * 40 + x).copied();

    // HIDE_PART_BG drops the frame and field: a column's empty part is the
    // bar colour at a quarter alpha over the page (the reference 8.13,
    // tools/ksp-ui tables page), and the gap after a column is the page.
    let dim = |p: Option<[u8; 4]>| {
        p.is_some_and(|[r, g, b, _]| {
            (70..=80).contains(&r) && (20..=28).contains(&g) && (32..=40).contains(&b)
        })
    };
    assert_eq!(at(5, 3), Some([0xFF, 0, 0, 255]), "col 0 positive bar");
    assert!(dim(at(5, 16)), "col 0 lower is dimmed bar: {:?}", at(5, 16));
    assert!(dim(at(25, 3)), "col 1 upper is dimmed bar: {:?}", at(25, 3));
    assert_eq!(at(25, 16), Some([0xFF, 0, 0, 255]), "col 1 negative bar");
    assert_eq!(at(19, 5), Some([0x10, 0x20, 0x30, 255]), "gap shows page");
    assert_eq!(at(19, 10), Some([0, 0xFF, 0, 255]), "bipolar zero line");

    // Adding a custom background picture draws both picture AND bars on top.
    tree.assets.push(Asset {
        path: "tbl.png".into(),
        kind: AssetKind::Image(ImageMeta {
            frames: 1,
            size: Some(Size {
                width: 40.,
                height: 20.,
            }),
            ..ImageMeta::default()
        }),
    });
    tree.widgets[0]
        .images
        .push(ImageUse::new(AssetRef(0), Role::Background));
    let blue = png(40, 20, [0, 0, 255, 255].repeat(40 * 20).as_slice());
    let mut pictures = Pictures::new(Box::new(move |p| (p == "tbl.png").then(|| blue.clone())));
    let mut ui = crate::ui();
    settle(&mut ui, &tree, &mut pictures, 1.);
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 40, 20);
    let at = |x: usize, y: usize| px.get(y * 40 + x).copied();
    assert_eq!(at(5, 3), Some([0xFF, 0, 0, 255]), "bar draws over picture");
    assert!(
        at(5, 16).is_some_and(|[r, _, b, _]| r > 50 && b > 150),
        "picture shows behind the dimmed column: {:?}",
        at(5, 16)
    );
}

#[test]
fn stock_menu_respects_menu_arrow_flag() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 40.,
            height: 20.,
        },
        ..Page::default()
    });
    let mut menu = Widget::new(
        "$m",
        PageRef(0),
        Rect::new(0, 0, 40, 20),
        Kind::Menu { items: Vec::new() },
    );
    menu.menu_arrow = false;
    tree.widgets.push(menu);

    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let without_arrow = paint(&mut ui, el, 40, 20);

    tree.widgets[0].menu_arrow = true;
    let (el, built) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(built, 1, "menu_arrow change invalidates memo");
    let with_arrow = paint(&mut ui, el, 40, 20);
    assert_ne!(without_arrow, with_arrow, "arrow renders only when enabled");
}

#[test]
fn style_font_and_value_y_invalidate_memo_and_table_columns_align_to_steps_shown() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 80.,
            height: 40.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0x10_2030)),
            ..model::Background::default()
        },
        ..Page::default()
    });
    tree.styles.push(model::TextStyle {
        font: model::Font::Stock(1),
        size: Some(12.),
        color: Rgba::rgb(0xFF_FFFF),
        align: model::Align::Center,
        ..model::TextStyle::default()
    });
    let mut label = Widget::new("$l", PageRef(0), Rect::new(0, 0, 40, 20), Kind::Label);
    label.text = "HI".into();
    label.style = Some(model::StyleRef(0));
    tree.widgets.push(label);

    let mut field = Widget::new(
        "$v",
        PageRef(0),
        Rect::new(40, 0, 40, 20),
        Kind::ValueEdit {
            range: Range {
                min: 0.,
                max: 100.,
                default: 50.,
                step: Some(1.),
            },
            display: model::Display::default(),
            arrows: false,
        },
    );
    field.value = Some(model::Value::Integer(50));
    field.value_y = Some(1);
    tree.widgets.push(field);

    // 4-column table with only 2 stored values and steps_shown = 8 (clamped to 4):
    // bars must occupy only the left half, leaving columns 2..4 empty.
    let mut tbl = Widget::new(
        "%t",
        PageRef(0),
        Rect::new(0, 20, 40, 20),
        Kind::Table {
            columns: 4,
            range: Range {
                min: 0.,
                max: 100.,
                default: 0.,
                step: Some(1.),
            },
            bipolar: false,
            cells: vec![100., 100.],
            steps_shown: Some(8),
        },
    );
    tbl.hide.background = true;
    tbl.colors.bar = Some(Rgba::rgb(0x00_FF00));
    tbl.value = Some(model::Value::Integers(vec![100, 100]));
    tree.widgets.push(tbl);

    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 80, 40);
    let at = |x: usize, y: usize| px.get(y * 80 + x).copied();
    assert_eq!(
        at(5, 30),
        Some([0, 0xFF, 0, 255]),
        "col 0 draws in first quarter"
    );
    assert_eq!(
        at(15, 30),
        Some([0, 0xFF, 0, 255]),
        "col 1 draws in second quarter"
    );
    // An empty column keeps only its dimmed cell (a quarter of the bar
    // colour over the page).
    assert!(
        at(25, 30).is_some_and(|[r, g, _, _]| r < 0x20 && (0x50..0x80).contains(&g)),
        "col 2 stays empty rather than stretching 2 bars across 4 columns: {:?}",
        at(25, 30)
    );

    // Changing only `style.font` (keeping StyleRef(0) and color/align/size identical)
    // must invalidate the label's memo.
    tree.styles[0].font = model::Font::Stock(12);
    let (_, built) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(built, 1, "style.font change invalidates memo");

    // Changing `value_y` on the ValueEdit must invalidate and move the painted pixels.
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let before = paint(&mut ui, el, 80, 40);
    tree.widgets[1].value_y = Some(6);
    let (el, built) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    assert_eq!(built, 1, "value_y change invalidates memo");
    let after = paint(&mut ui, el, 80, 40);
    assert_ne!(before, after, "value_y changes painted pixels");
}

/// A slider drawn with a filmstrip of square frames is a knob to its user
/// (Areia): KONTRA draws a dial. A long, thin strip stays a fader.
#[test]
fn kontra_draws_a_slider_with_a_rotary_strip_as_a_dial() {
    let (mut tree, ..) = tree();
    tree.widgets[0].kind = Kind::Slider {
        range: Range::default(),
        orientation: model::Orientation::Vertical,
    };
    let look = |tree: &Interface, bare| {
        let w = &tree.widgets[0];
        stock::Look::of(tree, w, 0.5, String::new(), String::new(), bare, false)
    };
    assert!(stock::rotary(&tree, &tree.widgets[0]), "4 by 4 frames");
    assert!(matches!(
        look(&tree, true),
        Some(stock::Look::Dial { turn: 500 })
    ));
    assert!(look(&tree, false).is_none(), "Original draws its strip");
    if let model::AssetKind::Image(m) = &mut tree.assets[1].kind {
        m.size = Some(Size {
            width: 4.,
            height: 40.,
        });
    }
    assert!(!stock::rotary(&tree, &tree.widgets[0]), "a fader's strip");
    assert!(look(&tree, true).is_none());
    if let model::AssetKind::Image(m) = &mut tree.assets[1].kind {
        (m.frames, m.size) = (
            1,
            Some(Size {
                width: 51.,
                height: 50.,
            }),
        );
    }
    assert!(
        stock::rotary(&tree, &tree.widgets[0]),
        "a one-frame ring is a knob too"
    );
}

/// A picture the script turns with a control's value (Widget::shows) is
/// that control's own graphic: KONTRA leaves it out, Original draws it.
#[test]
fn kontra_leaves_out_a_picture_that_shows_a_controls_value() {
    let (mut tree, ..) = tree();
    let mut cylinder = tree.widgets[0].clone();
    cylinder.kind = Kind::Label;
    cylinder.images[0].role = Role::Background;
    cylinder.shows = Some(model::WidgetRef(0));
    tree.widgets.push(cylinder);
    let drawn = |tree: &Interface, bare| {
        let page = plan::page(tree, PageRef(0), bare, 1., (&[], &[])).unwrap();
        page.widgets.iter().map(|w| w.index.0).collect::<Vec<_>>()
    };
    assert_eq!(drawn(&tree, false), [0, 1]);
    assert_eq!(drawn(&tree, true), [0]);
    tree.widgets[0].hidden = true;
    assert_eq!(
        drawn(&tree, true),
        [1],
        "with its control hidden, it shows the value"
    );
}

/// A picture KONTRA leaves out that spelled a menu's chosen item (a macro's
/// name) keeps that word in its place: nothing the Original says is lost.
#[test]
fn kontra_writes_the_word_a_left_out_picture_spelled() {
    let (mut tree, ..) = tree();
    let mut name = Widget::new(
        "$name",
        PageRef(0),
        Rect::new(0, 0, 1, 1),
        Kind::Menu {
            items: vec![model::MenuItem {
                text: "Rhythm".into(),
                value: 0,
                visible: true,
            }],
        },
    );
    name.hidden = true;
    tree.widgets.push(name);
    let mut word = tree.widgets[0].clone();
    word.kind = Kind::Label;
    word.images[0].role = Role::Background;
    word.shows = Some(model::WidgetRef(0));
    word.spells = Some(model::WidgetRef(1));
    tree.widgets.push(word);
    let page = plan::page(&tree, PageRef(0), true, 1., (&[], &[])).unwrap();
    let w = page.widgets.iter().find(|w| w.index.0 == 2).expect("kept");
    assert_eq!(w.words, "Rhythm");
    assert!(w.picture.is_none(), "its picture is still left out");
    let page = plan::page(&tree, PageRef(0), false, 1., (&[], &[])).unwrap();
    let w = page.widgets.iter().find(|w| w.index.0 == 2).expect("drawn");
    assert!(
        w.words.is_empty() && w.picture.is_some(),
        "Original draws the picture"
    );
}

/// Kontakt paints a label under a slider in the same layer (type rank), so
/// a label behind a knob's picture shows through its clear middle (Analog
/// Strings' "volume"). KONTRA's dial takes the slider's place in that order
/// and paints no face: the label still shows through.
#[test]
fn kontra_dial_leaves_what_paints_under_it_visible() {
    let (mut tree, wall, knob) = tree();
    tree.widgets[0].kind = Kind::Slider {
        range: Range::default(),
        orientation: model::Orientation::Vertical,
    };
    tree.widgets[0].rect = Rect::new(0, 0, 10, 10);
    // Declared after the slider, still painted before it.
    let mut under = Widget::new("$label", PageRef(0), Rect::new(0, 0, 10, 10), Kind::Label);
    under
        .images
        .push(ImageUse::new(AssetRef(0), Role::Background));
    tree.widgets.push(under);
    tree.z_order = model::ZOrder::Global; // KSP's
    assert_eq!(
        tree.draw_order(PageRef(0)),
        [model::WidgetRef(1), model::WidgetRef(0)]
    );
    tree.pages[0].background.image = None;
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    settle(&mut ui, &tree, &mut pictures, 1.);
    let kontra = Presentation::Kontra;
    let (el, _) = page_as(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[]),
        kontra,
    );
    let px = paint(&mut ui, el, 20, 10);
    let at = |x: usize, y: usize| px.get(y * 20 + x).copied();
    assert_eq!(
        at(5, 6),
        Some([0, 255, 0, 255]),
        "the label, through the dial"
    );
    assert!(
        (0..10).any(|x| at(x, 5).is_some_and(|p| p != [0, 255, 0, 255])),
        "the dial's ring"
    );
}

/// KONTRA's fader thumb is an outline, so a label painted under the
/// control (Analog Strings' "tune") stays readable through it.
#[test]
fn kontra_fader_thumb_leaves_what_paints_under_it_visible() {
    let (mut tree, wall, knob) = tree();
    tree.widgets[0].kind = Kind::Slider {
        range: Range::default(),
        orientation: model::Orientation::Horizontal,
    };
    tree.widgets[0].rect = Rect::new(0, 0, 20, 10);
    tree.widgets[0].value = Some(model::Value::Integer(0));
    if let model::AssetKind::Image(m) = &mut tree.assets[1].kind {
        m.size = Some(Size {
            width: 20.,
            height: 4.,
        });
    }
    let mut under = Widget::new("$label", PageRef(0), Rect::new(0, 0, 20, 10), Kind::Label);
    under
        .images
        .push(ImageUse::new(AssetRef(0), Role::Background));
    tree.widgets.push(under);
    tree.z_order = model::ZOrder::Global;
    tree.pages[0].background.image = None;
    let mut ui = crate::ui();
    let mut pictures = pictures(wall, knob);
    settle(&mut ui, &tree, &mut pictures, 1.);
    let kontra = Presentation::Kontra;
    let (el, _) = page_as(
        &mut ui,
        0,
        &tree,
        PageRef(0),
        &mut pictures,
        (1., &[]),
        kontra,
    );
    let px = paint(&mut ui, el, 20, 10);
    // The thumb sits at the left (value 0), 6 by 8 from y 1; its middle,
    // above the track, is the label's red row.
    assert_eq!(px.get(2 * 20 + 3).copied(), Some([255, 0, 0, 255]));
    assert!(
        (0..6).any(|x| px.get(20 + x).is_some_and(|p| *p != [255, 0, 0, 255])),
        "the thumb's outline"
    );
}

/// A button's and a menu's words follow TEXT_ALIGNMENT (the reference 8.13,
/// tools/ksp-ui fields page): left, centre and right start further right in turn.
#[test]
fn stock_button_and_menu_words_follow_their_text_alignment() {
    let first_ink = |kind: Kind, align: model::Align| {
        let mut tree = Interface {
            stock_skin: model::StockSkin::Kontakt,
            ..Interface::default()
        };
        tree.pages.push(Page {
            size: Size {
                width: 120.,
                height: 20.,
            },
            background: model::Background {
                color: Some(Rgba::rgb(0)),
                ..Default::default()
            },
            ..Page::default()
        });
        tree.styles.push(model::TextStyle {
            font: model::Font::Stock(0),
            color: Rgba::rgb(0xff_ffff),
            align,
            ..model::TextStyle::default()
        });
        let mut w = Widget::new("$w", PageRef(0), Rect::new(0, 0, 120, 20), kind);
        w.text = "Lowpass".into();
        w.value = Some(model::Value::Integer(0));
        w.style = Some(model::StyleRef(0));
        w.hide.background = true;
        w.menu_arrow = false;
        tree.widgets.push(w);
        let mut ui = crate::ui();
        let mut pictures = Pictures::new(Box::new(|_| None));
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
        let px = paint(&mut ui, el, 120, 20);
        (0..120)
            .find(|&x| (0..20).any(|y| px.get(y * 120 + x).is_some_and(|p| p[0] > 128)))
            .unwrap_or(usize::MAX)
    };
    let item = || model::MenuItem {
        text: "Lowpass".into(),
        value: 0,
        visible: true,
    };
    for kind in [
        Kind::Button { momentary: false },
        Kind::Menu {
            items: vec![item()],
        },
    ] {
        let [l, c, r] = [
            model::Align::Left,
            model::Align::Center,
            model::Align::Right,
        ]
        .map(|a| first_ink(kind.clone(), a));
        assert!(l < c && c < r && r < 120, "{kind:?}: {l} {c} {r}");
    }
}

/// A script label's face is black at half alpha over the page (the reference
/// 8.13, tools/ksp-ui text page): #777671 on the default page, dark red on red.
#[test]
fn a_label_face_darkens_what_lies_under_it() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 40.,
            height: 20.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0xff_3322)),
            ..Default::default()
        },
        ..Page::default()
    });
    let mut label = Widget::new("$l", PageRef(0), Rect::new(0, 0, 40, 20), Kind::Label);
    label.source_id = Some(1);
    tree.widgets.push(label);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 40, 20);
    let [r, g, b, _] = px.get(10 * 40 + 20).copied().unwrap_or_default();
    assert!((120..=132).contains(&r) && g < 32 && b < 24, "{r} {g} {b}");
}

/// A button's words in a chosen font with no pixel atlas draw as a label's
/// in that font (the reference 8.13, tools/ksp-ui fields page).
#[test]
fn a_chosen_font_draws_a_buttons_words_as_a_labels() {
    let ink = |kind: Kind| {
        let mut tree = Interface {
            stock_skin: model::StockSkin::Kontakt,
            ..Interface::default()
        };
        tree.pages.push(Page {
            size: Size {
                width: 72.,
                height: 20.,
            },
            background: model::Background {
                color: Some(Rgba::rgb(0)),
                ..Default::default()
            },
            ..Page::default()
        });
        tree.styles.push(model::TextStyle {
            font: model::Font::Stock(1),
            size: Some(12.6),
            weight: Some(700.),
            color: Rgba::rgb(0xff_ffff),
            align: model::Align::Left,
            ..model::TextStyle::default()
        });
        let mut w = Widget::new("$w", PageRef(0), Rect::new(0, 0, 72, 20), kind);
        w.text = "Lowpass 4".into();
        w.style = Some(model::StyleRef(0));
        w.hide.background = true;
        tree.widgets.push(w);
        let mut ui = crate::ui();
        let mut pictures = Pictures::new(Box::new(|_| None));
        let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
        // Where the white words sit: a button's grey body (kept under
        // $HIDE_PART_BG) only changes how their edges blend.
        let lit: Vec<(usize, usize)> = (paint(&mut ui, el, 72, 20).iter().enumerate())
            .filter(|(_, p)| p[0] > 240)
            .map(|(i, _)| (i % 72, i / 72))
            .collect();
        let xs = lit.iter().map(|p| p.0);
        let ys = lit.iter().map(|p| p.1);
        (xs.clone().min(), xs.max(), ys.clone().min(), ys.max())
    };
    let label = ink(Kind::Label);
    assert!(label.0.is_some());
    assert_eq!(ink(Kind::Button { momentary: false }), label);
}

/// A stock button keeps its body under $HIDE_PART_BG and its words under
/// $HIDE_PART_TITLE (the reference 8.13, tools/ksp-ui layers page button pb
/// and geometry page switches s1 and s2).
#[test]
fn a_stock_button_keeps_its_body_and_words_under_hide_parts() {
    let mut tree = Interface {
        stock_skin: model::StockSkin::Kontakt,
        ..Interface::default()
    };
    tree.pages.push(Page {
        size: Size {
            width: 60.,
            height: 20.,
        },
        background: model::Background {
            color: Some(Rgba::rgb(0)),
            ..Default::default()
        },
        ..Page::default()
    });
    let mut b = Widget::new(
        "$b",
        PageRef(0),
        Rect::new(0, 0, 60, 20),
        Kind::Button { momentary: false },
    );
    b.text = "pb".into();
    b.hide.background = true;
    b.hide.title = true;
    tree.widgets.push(b);
    let mut ui = crate::ui();
    let mut pictures = Pictures::new(Box::new(|_| None));
    let (el, _) = page(&mut ui, 0, &tree, PageRef(0), &mut pictures, (1., &[]));
    let px = paint(&mut ui, el, 60, 20);
    assert_eq!(px.get(10 * 60 + 50).map(|p| p[0]), Some(0x5f), "the body");
    assert!(px.iter().any(|p| p[0] > 200), "the words");
}

/// TEXTPOS_Y moves a stock menu's or button's words down by its value and a
/// value edit's by half, rounded up; unset and negative count as 0 (the
/// reference 8.13, tools/ksp-ui textpos page).
#[test]
fn stock_words_drop_by_textpos_y_never_above_their_place() {
    let drops: Vec<_> = [None, Some(0), Some(-4), Some(3), Some(6), Some(10)]
        .into_iter()
        .map(|y| (stock::text_drop(y), stock::half_drop(y)))
        .collect();
    assert_eq!(
        drops,
        [(0., 0.), (0., 0.), (0., 0.), (3., 2.), (6., 3.), (10., 5.)]
    );
}

/// A snapshot swap within the instrument hands the face a new copy of its
/// tree: nothing decodes again, and only the widgets whose script values
/// changed build. A second face of the same library (another preset, the
/// editor reopened) decodes nothing either, and a page hidden for a while
/// (another tab) comes back without building, input meanwhile or not.
#[test]
fn a_snapshot_swap_decodes_nothing_and_builds_only_what_changed() {
    let (tree, wall, knob) = tree();
    let mut ui = crate::ui();
    let mut store = pictures(wall.clone(), knob.clone());
    settle(&mut ui, &tree, &mut store, 1.);
    let decodes = kontra_ui_pictures::decodes();

    // The same snapshot again: a fresh copy, nothing changed.
    let same = tree.clone();
    assert_eq!(settle(&mut ui, &same, &mut store, 1.), 0);
    assert_eq!(kontra_ui_pictures::decodes(), decodes);

    // A face made anew over the same files decodes nothing; its first
    // frame, before the worker answers, builds the widgets without art.
    let mut again = pictures(wall, knob);
    settle(&mut ui, &same, &mut again, 1.);
    assert_eq!(
        kontra_ui_pictures::decodes(),
        decodes,
        "decoded once per process"
    );

    // The page leaves the frame (another tab) while input arrives, and
    // comes back.
    for buttons in [
        mui::prelude::Buttons::PRIMARY,
        mui::prelude::Buttons::default(),
    ] {
        let input = Input {
            pointer: mui::prelude::PointerInput {
                pos: Some(mui::prelude::Point::new(1., 1.)),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        };
        let other = block(40., 20.);
        assert!(
            ui.frame(other, Some(ViewSize::new(40., 20.)), input, 1. / 60.)
                .is_ok()
        );
    }
    assert_eq!(
        settle(&mut ui, &same, &mut again, 1.),
        0,
        "kept while hidden"
    );

    // A snapshot whose script set the knob elsewhere builds the knob only.
    let mut moved = same.clone();
    if let Some(w) = moved.widgets.first_mut() {
        w.value = Some(model::Value::Integer(0));
    }
    // Its old frame stands in until the new one decodes; then it builds.
    assert_eq!(settle(&mut ui, &moved, &mut again, 1.), 1);
}
