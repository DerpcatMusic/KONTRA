//! Checks and screenshots for the v2 views: the nested mixer, the load report
//! and the UI-IR renderer. Shots land in `artifacts/v2-ui/`.

use super::{ir_view, load_report as lr, mix_tree as mt, theme, tests::pixels};
use crate::artwork::Picture;
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::path::Path;
use std::sync::Arc;

/// Lays `build` out a few frames at `w`×`h` and returns the ui.
fn settle(w: f64, h: f64, mut build: impl FnMut(&mut Ui) -> El) -> Ui {
    let mut ui = theme::ui();
    for _ in 0..4 {
        let root = col![build(&mut ui)].w(w).h(h).fill(Role::Background);
        ui.frame(root, Some(Size::new(w, h)), Input::default(), 1. / 60.).unwrap();
    }
    ui
}

fn shoot(ui: &Ui, w: u16, h: u16, name: &str) {
    let path = Path::new("artifacts/v2-ui").join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    moose::core::screenshot::save_png(&path, &pixels(ui, w, h), u32::from(w), u32::from(h));
}

fn mixer_sample() -> mt::Tree {
    use mt::{Kind, Node};
    let mut n = vec![
        Node::new(1, "Vista Cellos", Kind::Instrument, None),
        Node::new(2, "Mics", Kind::Submix, Some(0)),
        Node::new(3, "Close", Kind::Mic, Some(1)),
        Node::new(4, "Tree", Kind::Mic, Some(1)),
        Node::new(5, "Room", Kind::Mic, Some(1)),
        Node::new(6, "Legato", Kind::Group, Some(0)),
        Node::new(7, "Shorts", Kind::Group, Some(0)),
        Node::new(8, "Spiccato", Kind::Group, Some(6)),
        Node::new(9, "Pizzicato", Kind::Group, Some(6)),
        Node::new(10, "Una Corda", Kind::Instrument, None),
    ];
    n[0].inserts = vec!["EQ".into(), "Reverb".into()];
    n[4].output = mt::Output::Host(5);
    n[4].output_set = true;
    n[4].gain_db = -6.;
    n[3].mute = true;
    n[2].gain_db = 2.5;
    n[9].pan = -0.3;
    mt::Tree { nodes: n }
}

#[test]
fn nested_mixer_strips_step_down_and_fold() {
    let mut tree = mixer_sample();
    tree.assign_outputs(8);
    let mut state = mt::State::default();
    let levels: mt::Levels = Arc::new(|n| [0.2 + 0.05 * n as f32, 0.18 + 0.05 * n as f32]);
    let ui = settle(1180., 520., |ui| mt::view(ui, &mut tree, &mut state, 8, 480., levels.clone()));
    let height = |id: &str| ui.scene().unwrap().surface(id).unwrap_or_else(|| panic!("{id}")).frame.size.height;
    let (top, mid, low) = (height("mt-strip-1"), height("mt-strip-2"), height("mt-strip-3"));
    assert!(top > mid && mid > low, "each level is shorter: {top} {mid} {low}");
    let bottom = |id: &str| {
        let f = ui.scene().unwrap().surface(id).unwrap().frame;
        f.y + f.size.height
    };
    assert!((bottom("mt-strip-1") - bottom("mt-strip-3")).abs() < 1., "strips share a baseline");
    assert_eq!(tree.nodes[0].output, mt::Output::Host(0));
    assert_eq!(tree.nodes[9].output, mt::Output::Host(1), "every instrument has its own pair");
    shoot(&ui, 1180, 520, "mixer-tree.png");

    state.folded.insert(1);
    let ui = settle(1180., 520., |ui| mt::view(ui, &mut tree, &mut state, 8, 480., levels.clone()));
    assert!(ui.scene().unwrap().surface("mt-strip-3").is_none(), "folding hides the subtree");
    assert!(ui.scene().unwrap().surface("mt-strip-10").is_some());
}

#[test]
fn load_report_shot() {
    let r = lr::Report {
        instrument: "Vista - 3 Cellos".into(),
        loaded: vec![
            lr::Loaded { area: lr::Area::Mapping, summary: "3 412 zones · 48 groups".into() },
            lr::Loaded { area: lr::Area::Samples, summary: "2.1 GB, 64 KB resident heads".into() },
            lr::Loaded { area: lr::Area::Scripts, summary: "3 of 4 running".into() },
            lr::Loaded { area: lr::Area::Interface, summary: "47 controls".into() },
        ],
        missing: vec![
            lr::Missing::ScriptError { script: "Legato".into(), line: 412, column: 9, message: "unknown variable $vel_curve".into() },
            lr::Missing::Sample { path: "Samples/Cello_C3_pp_rr1.ncw".into() },
            lr::Missing::Sample { path: "Samples/Cello_C3_pp_rr2.ncw".into() },
            lr::Missing::Sample { path: "Samples/Cello_D3_pp_rr1.ncw".into() },
            lr::Missing::Sample { path: "Samples/Cello_D3_pp_rr2.ncw".into() },
            lr::Missing::Effect {
                module: "Convolution".into(),
                location: "Insert 2, bus 1".into(),
                params: vec![("size".into(), "1.20".into()), ("mix".into(), "30 %".into())],
            },
            lr::Missing::Modulation { law: "Step modulator, retrigger off".into(), location: "Group 'Shorts', filter cutoff".into() },
            lr::Missing::ScriptBuiltin { script: "Main".into(), name: "load_ir_async".into(), line: 88, column: 5 },
        ],
        runtime: vec![
            lr::Runtime::ScriptBudget { script: "Legato".into(), overruns: 12, worst_ms: 2.4 },
            lr::Runtime::StreamUnderruns { count: 3 },
        ],
    };
    let mut state = lr::State::default();
    let ui = settle(760., 620., |ui| lr::view(ui, &mut state, &r));
    assert!(ui.scene().unwrap().surface("report-more-1").is_some(), "four missing samples collapse behind Show all");
    shoot(&ui, 760, 620, "load-report.png");
}

fn solid(w: u32, h: u32, rgba: [u8; 4]) -> Arc<Image> {
    Arc::new(Image::rgba(w, h, rgba.repeat((w * h) as usize)).unwrap())
}

fn picture(frames: Vec<Arc<Image>>) -> Arc<Picture> {
    Arc::new(Picture { frames, stretch: [false; 2], atlas: None })
}

/// A wallpaper, a background panel picture, a 64-frame knob strip and a
/// two-frame switch: vector mode keeps the first two only.
#[test]
fn ir_view_vector_mode_releases_control_bitmaps() {
    use ir::{AssetKind, ImageMeta, ImageUse, Kind, PageRef, Rect, Role, Widget};
    let page = PageRef(0);
    let image = |path: &str, frames| ir::Asset { path: path.into(), kind: AssetKind::Image(ImageMeta { frames, ..ImageMeta::default() }) };
    let mut panel = Widget::new("$panel", page, Rect::new(20, 20, 300, 120), Kind::Panel);
    panel.images.push(ImageUse { asset: ir::AssetRef(1), role: Role::Background });
    let range = ir::Range { min: 0., max: 100., default: 50., step: None };
    let mut knob = Widget::new("$cutoff", page, Rect::new(20, 20, 64, 64), Kind::Knob { range, display: ir::Display::default() });
    knob.parent = Some(ir::WidgetRef(0));
    knob.binding = ir::Binding::Control(ir::ControlId(1));
    knob.images.push(ImageUse { asset: ir::AssetRef(2), role: Role::Strip });
    let mut switch = Widget::new("$legato", page, Rect::new(120, 40, 80, 24), Kind::Switch);
    switch.parent = Some(ir::WidgetRef(0));
    switch.binding = ir::Binding::Control(ir::ControlId(2));
    switch.text = "Legato".into();
    switch.images.push(ImageUse { asset: ir::AssetRef(3), role: Role::Strip });
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        pages: vec![ir::Page {
            name: "Main".into(),
            size: ir::Size { width: 633, height: 300 },
            background: ir::Background { image: Some(ir::AssetRef(0)), ..Default::default() },
        }],
        widgets: vec![panel, knob, switch],
        assets: vec![image("wallpaper", 1), image("panel", 1), image("knob", 64), image("switch", 2)],
        ..Default::default()
    };
    face.validate().unwrap();
    let load = |a: &ir::Asset| {
        Some(match a.path.as_str() {
            "wallpaper" => picture(vec![solid(633, 300, [40, 52, 70, 255])]),
            "panel" => picture(vec![solid(300, 120, [20, 24, 30, 255])]),
            "knob" => picture((0..64).map(|n| solid(64, 64, [n * 4, 120, 200, 255])).collect()),
            _ => picture(vec![solid(80, 24, [60, 60, 60, 255]), solid(80, 24, [200, 200, 200, 255])]),
        })
    };
    let mut assets = ir_view::Assets::default();
    assets.sync(&face, ir::Presentation::Bitmap, load);
    let bitmap = assets.bytes();
    let mut values = ir_view::Values::default();
    let ui = settle(633., 300., |ui| ir_view::view(ui, &face, page, &assets, ir::Presentation::Bitmap, 1., &mut values));
    shoot(&ui, 633, 300, "ir-bitmap-synthetic.png");
    assets.sync(&face, ir::Presentation::Vector, load);
    let vector = assets.bytes();
    let ui = settle(633., 300., |ui| ir_view::view(ui, &face, page, &assets, ir::Presentation::Vector, 1., &mut values));
    shoot(&ui, 633, 300, "ir-vector-synthetic.png");
    assert_eq!(bitmap - vector, 64 * 64 * 4 * 64 + 80 * 24 * 4 * 2, "strips released");
    assert_eq!(vector, (633 * 300 + 300 * 120) * 4, "wallpaper and panel art kept");
    assert_eq!(values.get(&ir::ControlId(1)), Some(&50.), "a value starts at its default");
}

/// Builds the IR from the current KSP view of a real instrument and measures
/// both presentations. Opt-in: `KONTRA_UI_IR_PATCH=/path/to/patch.nki`.
#[test]
#[ignore = "set KONTRA_UI_IR_PATCH to a locally owned instrument"]
fn ir_view_real_instrument_memory() {
    let patch = std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH");
    let i = crate::import::read(Path::new(&patch)).unwrap();
    let view = super::tests::scripted(&i);
    let interface = view.interface.expect("the instrument has a script interface");
    let names = crate::artwork::picture_names(&interface).map(|n| n.into_owned()).collect::<Vec<_>>();
    let pictures = crate::artwork::pictures(&i.path, names.iter().map(String::as_str));
    let wallpaper = crate::artwork::performance(&i, Some(&interface)).ok().flatten();
    let face = from_v1(&interface, &pictures, wallpaper.is_some());
    face.validate().unwrap();
    let load = |a: &ir::Asset| if a.path == "@wallpaper" { wallpaper.clone() } else { pictures.get(&a.path).cloned() };
    let stem = Path::new(&patch).file_stem().unwrap().to_string_lossy().replace(' ', "_");
    let (w, h) = (face.pages[0].size.width as u16, face.pages[0].size.height as u16);
    let mut values = ir_view::Values::default();
    let mut assets = ir_view::Assets::default();
    let mut bytes = Vec::new();
    for (p, name) in [(ir::Presentation::Bitmap, "bitmap"), (ir::Presentation::Vector, "vector")] {
        assets.sync(&face, p, load);
        bytes.push(assets.bytes());
        let ui = settle(f64::from(w), f64::from(h), |ui| ir_view::view(ui, &face, ir::PageRef(0), &assets, p, 1., &mut values));
        shoot(&ui, w, h, &format!("ir-{name}-{stem}.png"));
    }
    let all: usize = pictures.values().chain(wallpaper.iter()).flat_map(|p| p.frames.iter()).map(|i| i.width as usize * i.height as usize * 4).sum();
    eprintln!(
        "{stem}: {} widgets, {} assets; decoded pixels resident: bitmap {} KiB, vector {} KiB ({} KiB in every picture the script names)",
        face.widgets.len(),
        face.assets.len(),
        bytes[0] / 1024,
        bytes[1] / 1024,
        all / 1024
    );
    assert!(bytes[1] <= bytes[0]);
}

/// The current KSP view as UI IR: every visible control at its absolute
/// place (panels resolved), label pictures as background art, every other
/// control picture as a strip.
// ponytail: test-only bridge over the v1 KSP runtime; the KSP frontend emits the IR directly.
fn from_v1(u: &crate::ksp::Interface, pictures: &std::collections::HashMap<String, Arc<Picture>>, wallpaper: bool) -> ir::Interface {
    use super::perf_view::{Kind as V1, layout, prop};
    let mut face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        pages: vec![ir::Page {
            name: u.title.clone(),
            size: ir::Size { width: u.width.max(1) as u32, height: u.height.max(1) as u32 },
            background: ir::Background {
                color: u.background_color.map(ir::Rgba::rgb),
                image: wallpaper.then_some(ir::AssetRef(0)),
                offset_y: u.skin_offset + super::perf_view::HEADER as i32,
            },
        }],
        ..Default::default()
    };
    if wallpaper {
        face.assets.push(ir::Asset { path: "@wallpaper".into(), kind: ir::AssetKind::Image(ir::ImageMeta::default()) });
    }
    let mut asset_of = std::collections::HashMap::new();
    for s in layout(u, pictures) {
        let c = &u.controls[s.control];
        let int = |k: &str| match c.properties.get(k) {
            Some(crate::ksp::Value::Int(n)) => Some(f64::from(*n)),
            Some(crate::ksp::Value::Real(r)) => Some(*r),
            _ => None,
        };
        let range = ir::Range {
            min: int("$CONTROL_PAR_MIN_VALUE").unwrap_or(0.),
            max: int("$CONTROL_PAR_MAX_VALUE").unwrap_or(1_000_000.),
            default: int("$CONTROL_PAR_DEFAULT_VALUE").unwrap_or(0.),
            step: Some(1.),
        };
        let kind = match s.kind {
            V1::Knob => ir::Kind::Knob { range, display: ir::Display::default() },
            V1::Slider => ir::Kind::Slider { range, orientation: ir::Orientation::Vertical },
            V1::Switch => ir::Kind::Switch,
            V1::Button => ir::Kind::Button { momentary: false },
            V1::Menu => ir::Kind::Menu {
                items: c.menu.iter().map(|(t, v)| ir::MenuItem { text: t.clone(), value: *v, visible: true }).collect(),
            },
            V1::Value => ir::Kind::ValueEdit { range, display: ir::Display::default() },
            V1::Label => ir::Kind::Label,
            V1::Table => ir::Kind::Table { columns: 1, range, bipolar: false },
            V1::TextEdit => ir::Kind::TextEdit,
            V1::FileSelector => ir::Kind::FileSelector,
            V1::Area => ir::Kind::MouseArea,
            V1::Meter => ir::Kind::LevelMeter { orientation: ir::Orientation::Vertical },
            V1::Waveform => ir::Kind::Waveform,
            V1::Other => ir::Kind::Xy { cursors: 1 },
        };
        let mut w = ir::Widget::new(c.variable.clone(), ir::PageRef(0), ir::Rect::new(s.x as i32, s.y as i32, s.w as u32, s.h as u32), kind);
        w.source_id = Some(c.id);
        w.z = s.z;
        w.text = prop(c, "$CONTROL_PAR_TEXT").to_owned();
        let control = ir::ControlId(face.widgets.len() as u128);
        w.binding = ir::Binding::Control(control);
        let name = prop(c, "$CONTROL_PAR_PICTURE");
        if let Some(p) = s.picture.as_ref() {
            let next = face.assets.len();
            let at = *asset_of.entry(name.to_owned()).or_insert(next);
            if at == next {
                face.assets.push(ir::Asset {
                    path: name.to_owned(),
                    kind: ir::AssetKind::Image(ir::ImageMeta { frames: p.frames.len() as u32, ..Default::default() }),
                });
            }
            let role = if s.kind == V1::Label { ir::Role::Background } else { ir::Role::Strip };
            w.images.push(ir::ImageUse { asset: ir::AssetRef(at), role });
        }
        face.widgets.push(w);
    }
    face
}
