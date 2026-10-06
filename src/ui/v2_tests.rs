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
    Arc::new(Picture { frames })
}

/// A wallpaper, a background panel picture, a 64-frame knob strip and a
/// two-frame switch: vector mode keeps the first two only.
#[test]
fn ir_view_vector_mode_releases_control_bitmaps() {
    use ir::{AssetKind, ImageMeta, ImageUse, Kind, PageRef, Rect, Role, Widget};
    let page = PageRef(0);
    let image = |path: &str, frames| ir::Asset { path: path.into(), kind: AssetKind::Image(ImageMeta { frames, ..ImageMeta::default() }) };
    let mut panel = Widget::new("$panel", page, Rect::new(20, 20, 300, 120), Kind::Panel);
    panel.images.push(ImageUse::new(ir::AssetRef(1), Role::Background));
    let range = ir::Range { min: 0., max: 100., default: 50., step: None };
    let mut knob = Widget::new("$cutoff", page, Rect::new(20, 20, 64, 64), Kind::Knob { range, display: ir::Display::default() });
    knob.parent = Some(ir::WidgetRef(0));
    knob.binding = ir::Binding::Control(ir::ControlId(1));
    knob.images.push(ImageUse::new(ir::AssetRef(2), Role::Strip));
    let mut switch = Widget::new("$legato", page, Rect::new(120, 40, 80, 24), Kind::Switch);
    switch.parent = Some(ir::WidgetRef(0));
    switch.binding = ir::Binding::Control(ir::ControlId(2));
    switch.text = "Legato".into();
    switch.images.push(ImageUse::new(ir::AssetRef(3), Role::Strip));
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        pages: vec![ir::Page {
            name: "Main".into(),
            size: ir::Size { width: 633, height: 300 },
            background: ir::Background { image: Some(ir::AssetRef(0)), ..Default::default() },
            ..Default::default()
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

const NO_LIMITS: sampler_ksp::Limits =
    sampler_ksp::Limits { source_bytes: usize::MAX, instructions: usize::MAX, variables: usize::MAX, array_cells: usize::MAX };

/// Renders `face` in both presentations, shooting each as `{stem}-{mode}.png`
/// under `dir`; returns the decoded bytes each keeps.
fn both_modes(face: &ir::Interface, load: &dyn Fn(&ir::Asset) -> Option<Arc<Picture>>, dir: &str, stem: &str) -> [usize; 2] {
    let face = ir_view::resolved(face);
    let page = &face.pages[0];
    let (w, h) = (page.size.width.clamp(1, 1200) as u16, page.size.height.clamp(1, 900) as u16);
    let mut values = ir_view::Values::default();
    let mut assets = ir_view::Assets::default();
    let mut bytes = [0; 2];
    for (n, (p, mode)) in [(ir::Presentation::Bitmap, "bitmap"), (ir::Presentation::Vector, "vector")].into_iter().enumerate() {
        assets.sync(&face, p, load);
        bytes[n] = assets.bytes();
        let ui = settle(f64::from(w), f64::from(h), |ui| ir_view::view(ui, &face, ir::PageRef(0), &assets, p, 1., &mut values));
        shoot(&ui, w, h, &format!("{dir}/{stem}-{mode}.png"));
    }
    bytes
}

/// Every script interface the KSP frontend emits from the extracted corpus
/// draws in both presentations. Opt-in: `KSP_CORPUS` (default ~/.cache/ksp-corpus).
#[test]
#[ignore = "needs the extracted KSP corpus (kept outside the repo)"]
fn ir_view_draws_the_ksp_corpus() {
    let dir = std::env::var("KSP_CORPUS").unwrap_or_else(|_| format!("{}/.cache/ksp-corpus", std::env::var("HOME").unwrap()));
    let mut paths: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "ksp")).collect();
    paths.sort();
    let (mut drawn, mut widgets) = (0, 0);
    for path in &paths {
        let source = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        let Ok(script) = sampler_ksp::compile(&source, 48000, NO_LIMITS, &[]) else { continue };
        let face = script.ui(&|_| None).unwrap();
        if face.widgets.is_empty() {
            continue;
        }
        widgets += face.widgets.len();
        both_modes(&face, &|_| None, "corpus", &path.file_stem().unwrap().to_string_lossy());
        drawn += 1;
    }
    eprintln!("drew {drawn} interfaces ({widgets} widgets) of {} scripts in both presentations", paths.len());
    assert!(drawn > 0);
}

/// A real instrument's scripts through the KSP frontend into UI IR, with the
/// library's own pictures, in both presentations; prints the decoded pixels
/// each keeps. Opt-in: `KONTRA_UI_IR_PATCH=/path/to/patch.nki`.
#[test]
#[ignore = "set KONTRA_UI_IR_PATCH to a locally owned instrument"]
fn ir_view_real_instrument_memory() {
    use crate::sound::CoreLoader;
    let patch = std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH");
    let request = crate::sound::LoadRequest { path: patch.clone().into(), sample_rate: 48000.0, ..Default::default() };
    let loaded = crate::sound::v2::V2Loader.prepare(&request, &mut |_| {}, &|| false).unwrap();
    let face = loaded.interfaces.iter().max_by_key(|u| u.widgets.len()).expect("a script with an interface").clone();
    let load = |a: &ir::Asset| crate::artwork::asset(Path::new(&patch), a);
    let stem = Path::new(&patch).file_stem().unwrap().to_string_lossy().replace(' ', "_");
    let bytes = both_modes(&face, &load, "real", &stem);
    eprintln!(
        "{stem}: {} widgets, {} assets ({} unsupported entries); decoded pixels resident: bitmap {} KiB, vector {} KiB",
        face.widgets.len(),
        face.assets.len(),
        face.unsupported.len(),
        bytes[0] / 1024,
        bytes[1] / 1024
    );
    assert!(bytes[1] <= bytes[0]);
}
