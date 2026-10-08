//! Checks and screenshots for the v2 views: the nested mixer, the load report
//! and the UI-IR renderer. Shots land in `artifacts/v2-ui/`.

use super::{ir_view, load_report as lr, mix_tree as mt, theme, tests::{Harness, pixels}};
use super::ir_view::Picture;
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
        Node::new(2, "Mics", Kind::Bus, Some(0)),
        Node::new(3, "Close", Kind::Bus, Some(1)),
        Node::new(4, "Tree", Kind::Bus, Some(1)),
        Node::new(5, "Room", Kind::Bus, Some(1)),
        Node::new(6, "Legato", Kind::Group, Some(0)),
        Node::new(7, "Shorts", Kind::Group, Some(0)),
        Node::new(8, "Spiccato", Kind::Group, Some(6)),
        Node::new(9, "Pizzicato", Kind::Group, Some(6)),
        Node::new(10, "Una Corda", Kind::Instrument, None),
    ];
    (n[0].output, n[9].output) = (mt::Output::Pair(0), mt::Output::Pair(1));
    n[0].inserts = vec!["EQ".into(), "Reverb".into()];
    n[4].output = mt::Output::Pair(5);
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
        why_silent: Some("key 60: 1122 zones rejected by group selection (script)".into()),
        faults: vec!["InvalidInput in script 1 on note".into()],
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
            lr::Runtime::ScriptBudget { overruns: 12 },
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
    Arc::new(Picture::new(frames))
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
fn both_modes(face: &ir::Interface, load: &mut dyn FnMut(&ir::Asset) -> Option<Arc<Picture>>, dir: &str, stem: &str) -> [usize; 2] {
    let face = ir_view::resolved(face);
    let page = &face.pages[0];
    let (w, h) = (page.size.width.clamp(1, 1200) as u16, ir_view::height(&face, ir::PageRef(0)).clamp(1, 900) as u16);
    let mut values = ir_view::Values::default();
    let mut assets = ir_view::Assets::default();
    let mut bytes = [0; 2];
    for (n, (p, mode)) in [(ir::Presentation::Bitmap, "bitmap"), (ir::Presentation::Vector, "vector")].into_iter().enumerate() {
        assets.sync(&face, p, &mut *load);
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
        both_modes(&face, &mut |_| None, "corpus", &path.file_stem().unwrap().to_string_lossy());
        drawn += 1;
    }
    eprintln!("drew {drawn} interfaces ({widgets} widgets) of {} scripts in both presentations", paths.len());
    assert!(drawn > 0);
}

/// A real instrument through the v2 Kontakt loader into UI IR, with the
/// library's own pictures, in both presentations; prints the decoded pixels
/// each keeps. Opt-in: `KONTRA_UI_IR_PATCH=/path/to/patch.nki`.
#[test]
#[ignore = "set KONTRA_UI_IR_PATCH to a locally owned instrument"]
fn ir_view_real_instrument_memory() {
    let patch = std::path::PathBuf::from(std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH"));
    // Only key 0's samples: the interfaces are what this measures.
    let options = sampler_kontakt::Options { keys: 0..=0, library: Some(patch.clone()), ..Default::default() };
    let loaded = sampler_kontakt::load(&patch, &options, |_| {}).unwrap();
    let mut face = loaded.interfaces.into_iter().max_by_key(|u| u.widgets.len()).expect("a script with an interface");
    // A wallpaper the saved state does not name can be given a stand-in to measure.
    if let (Some(a), Ok(n)) = (face.pages[0].background.image, std::env::var("KONTRA_UI_IR_WALLPAPER"))
        && face.assets[a.0].path.ends_with("/.png")
    {
        face.assets[a.0].path = format!("Resources/pictures/{n}.png");
    }
    let mut source = super::pictures::Source::of(&patch);
    if std::env::var_os("DUMP_WIDGETS").is_some() {
        let r = ir_view::resolved(&face);
        for (n, w) in r.widgets.iter().enumerate() {
            eprintln!("W{n} {} {:?} {:?} vis={} hide={:?} text={:?} val={:?} imgs={:?} parent={:?}", w.name, kind_name(&w.kind), r.page_rect(ir::WidgetRef(n)), r.visible(ir::WidgetRef(n)), w.hide, w.text, w.value_text, w.images.iter().map(|i| (r.assets[i.asset.0].path.clone(), i.role)).collect::<Vec<_>>(), w.parent);
        }
        eprintln!("PAGE {:?} {:?}", r.pages[0], r.unsupported);
    }
    let stem = Path::new(&patch).file_stem().unwrap().to_string_lossy().replace(' ', "_");
    let bytes = both_modes(&face, &mut |a| source.load(a), "real", &stem);
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

/// The editor with a real instrument in the rack, drawn from the v2 Kontakt
/// loader's data: its interface in both presentations, the mixer tree and
/// the load report. Opt-in: `KONTRA_UI_IR_PATCH=/path/to/patch.nki`.
#[test]
#[ignore = "set KONTRA_UI_IR_PATCH to a locally owned instrument"]
fn editor_with_a_real_instrument() {
    use crate::sound::{report::LoadReport, tree::MixTree};
    let patch = std::path::PathBuf::from(std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH"));
    let options = sampler_kontakt::Options { rate: 48000, keys: 0..=127, scripts: true, library: Some(patch.clone()), ..Default::default() };
    let loaded = sampler_kontakt::load(&patch, &options, |_| {}).unwrap();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part { path: patch.to_string_lossy().into_owned(), ..Default::default() });
    {
        let mut v = p.shared.view.lock().unwrap();
        if v.parts.is_empty() {
            v.parts.push(Default::default());
        }
        let part = &mut v.parts[0];
        part.active = loaded.instrument.name.clone();
        part.report = Some(Arc::new(LoadReport::of(&loaded.instrument, &patch, loaded.plan.sample_count())));
        part.tree = Some(Arc::new(MixTree::instrument(&loaded.instrument.name)));
        part.interfaces = loaded.interfaces.into();
        part.instrument = Some(Arc::new(loaded.instrument));
    }
    let stem = patch.file_stem().unwrap().to_string_lossy().replace(' ', "_");
    let mut h = Harness::new(&p, 1180., 780.);
    h.idle(40);
    let shot = |h: &Harness, name: &str| shoot(&h.ui, 1180, 780, &format!("app/{stem}-{name}.png"));
    if h.ui.scene().unwrap().surface("face-original-0").is_some() {
        h.press("face-original-0");
        h.idle(20);
        shot(&h, "rack-original");
        h.press("face-vector-0");
        h.idle(20);
        shot(&h, "rack-vector");
    } else {
        shot(&h, "rack");
    }
    for view in ["Articulations", "Mapping", "Sound", "Info"] {
        let id = format!("view-0-{view}");
        if h.ui.scene().unwrap().surface(&id).is_some() {
            h.press(&id);
            h.idle(4);
            shot(&h, &view.to_lowercase());
        }
    }
    h.press("tab-mixer");
    shot(&h, "mixer");
    h.press("tab-report");
    shot(&h, "report");
}

fn kind_name(k: &ir::Kind) -> String {
    let s = format!("{k:?}");
    s.chars().take(60).collect()
}

/// A part with keyswitched articulations: the list switches by tapping a
/// row's key, follows the keys as they are played, and marks them on the
/// keyboard. Shoots `artifacts/v2-ui/app/synthetic-{articulations,mapping}.png`.
#[test]
fn articulations_switch_by_their_keys() {
    use sampler_ir as sir;
    use std::sync::atomic::Ordering;
    let mut inst = sir::Instrument { name: "Strings".into(), ..Default::default() };
    for (n, name) in ["Legato", "Sustain", "Staccato", "Pizzicato", "Tremolo"].into_iter().enumerate() {
        inst.groups.push(sir::Group { name: name.into(), ..Default::default() });
        inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 0, alternatives: Default::default() });
        for (v, (lo, hi)) in [(1, 63), (64, 127)].into_iter().enumerate() {
            let mut z = sir::Zone::new(sir::AssetRef(0));
            z.group = Some(sir::GroupRef(n));
            z.keys = sir::KeyRange { low: 36 + v as u8 * 3, high: 84 - n as u8 * 4 };
            z.velocities = sir::VelocityRange { low: lo, high: hi };
            inst.zones.push(z);
        }
    }
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part { path: "/x/Strings.nki".into(), ..Default::default() });
    {
        let mut v = p.shared.view.lock().unwrap();
        if v.parts.is_empty() {
            v.parts.push(Default::default());
        }
        v.parts[0].active = "Strings".into();
        v.parts[0].instrument = Some(Arc::new(inst));
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.idle(4);
    h.press("view-0-Articulations");
    h.idle(2);
    let lit = |h: &Harness, n: usize| h.ui.scene().unwrap().surface(&format!("art-0-{n}")).is_some();
    assert!(lit(&h, 4), "every articulation has a row");
    while p.shared.keyboard.pop().is_some() {}
    h.press("art-0-2");
    h.idle(2);
    let sent: Vec<String> = std::iter::from_fn(|| p.shared.keyboard.pop()).map(|(slot, play)| format!("{slot} {play:?}")).collect();
    assert_eq!(sent, ["0 Note(26, 1)", "0 Note(26, 0)"], "a row taps its key and lets it go");
    p.shared.heard[27].store(90, Ordering::Relaxed);
    h.idle(2);
    p.shared.heard[27].store(0, Ordering::Relaxed);
    h.idle(2);
    shoot(&h.ui, 1180, 780, "app/synthetic-articulations.png");
    h.press("remap-0-Channel");
    h.idle(2);
    shoot(&h.ui, 1180, 780, "app/synthetic-articulations-channel.png");
    h.press("view-0-Mapping");
    h.press("map-group-0-1");
    h.idle(2);
    shoot(&h.ui, 1180, 780, "app/synthetic-mapping.png");
}

/// The part's performance line: the playing articulation with its keys, the
/// instrument volume, the dynamics controller it waits for (one click picks
/// where it starts, saved on the part for the next load) and MPE.
#[test]
fn the_performance_line_shows_articulation_volume_dynamics_and_mpe() {
    use sampler_ir as sir;
    let mut inst = sir::Instrument { name: "Strings".into(), ..Default::default() };
    for (n, name) in ["Legato", "Staccato"].into_iter().enumerate() {
        inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 0, alternatives: Default::default() });
    }
    inst.host_volume = Some(sir::HostVolume { controller: 7, saved: 0.5 });
    assert_eq!(super::part::volume_text(&inst).as_deref(), Some("CC7 -6.0 dB"));
    let mut report = crate::sound::report::LoadReport::default();
    report.decoded.dynamics = vec![(1, 0), (11, 127)];
    report.decoded.needs_controller = true;
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part { path: "/x/Strings.nki".into(), ..Default::default() });
    {
        let mut v = p.shared.view.lock().unwrap();
        if v.parts.is_empty() {
            v.parts.push(Default::default());
        }
        v.parts[0].active = "Strings".into();
        v.parts[0].instrument = Some(Arc::new(inst));
        v.parts[0].report = Some(Arc::new(report));
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.idle(4);
    let there = |h: &Harness, id: &str| h.ui.scene().unwrap().surface(id).is_some();
    for id in ["perf-art-0", "perf-vol-0", "perf-needs-0"] {
        assert!(there(&h, id), "{id}");
    }
    assert_eq!(p.selection.read().unwrap().parts[0].dynamics, -1, "Kontakt's own start until picked");
    h.press("dyn-0-64");
    h.idle(2);
    assert_eq!(p.selection.read().unwrap().parts[0].dynamics, 64);
    use super::part::badge_text;
    assert_eq!(badge_text("CC1", -1, 0, true), "Needs CC1");
    assert_eq!(badge_text("CC1", 64, 0, true), "CC1 starts at 64", "a picked start replaces the warning");
    assert_eq!(badge_text("CC1", -1, 100, false), "CC1 starts at 100");
    assert!(!there(&h, "mpe-0"), "MPE stays in the header's MIDI menu");
    shoot(&h.ui, 1180, 780, "app/synthetic-performance.png");
}

/// What an importer set in the IR (owner, driver, key policy) is the part's
/// until the player remaps it: opening the Articulations view must not write
/// a remap of its own over it.
#[test]
fn imported_switching_survives_the_first_ui_sync() {
    use sampler_ir as sir;
    let drivers = [sir::Driver::Keys, sir::Driver::Velocity, sir::Driver::Channel, sir::Driver::Controller, sir::Driver::Program];
    let owners = [sir::SwitchOwner::Native, sir::SwitchOwner::Behavior];
    let policies = [sir::SwitchKeys::Keep, sir::SwitchKeys::Play, sir::SwitchKeys::Swallow];
    for owner in owners {
        for driver in drivers {
            for keys in policies {
                let mut inst = sir::Instrument { name: "Imported".into(), ..Default::default() };
                inst.switching = sir::Switching { owner, driver, keys };
                for (n, name) in ["Sustain", "Staccato", "Tremolo"].into_iter().enumerate() {
                    inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 1, alternatives: Default::default() });
                }
                inst.assign_alternatives(32);
                let p = Arc::new(crate::plugin::SamplerParams::new());
                p.selection.write().unwrap().parts.push(crate::plugin::Part { path: "/x/Imported.nki".into(), ..Default::default() });
                {
                    let mut v = p.shared.view.lock().unwrap();
                    if v.parts.is_empty() {
                        v.parts.push(Default::default());
                    }
                    v.parts[0].instrument = Some(Arc::new(inst));
                }
                let mut h = Harness::new(&p, 1180., 780.);
                h.idle(4);
                h.press("view-0-Articulations");
                h.idle(3);
                let stored = p.selection.read().unwrap().parts[0].switching;
                assert_eq!(stored, 0, "{owner:?} {driver:?} {keys:?}: the view wrote {stored:#x} over the import");
            }
        }
    }
}

/// A remap or start value belongs to the instrument it was made on: replacing
/// the part's instrument leaves the new import's own switching in force.
#[test]
fn replacing_the_instrument_drops_the_old_remap_and_dynamics_start() {
    let mut part = crate::plugin::Part {
        path: "/x/Old.nki".into(),
        switching: 0x80 | (sampler_ir::Driver::Velocity as u8) << 1,
        dynamics: 64,
        ..Default::default()
    };
    super::replace_part(&mut part, "/x/New.nki".into());
    assert_eq!(part.switching, 0, "the old remap would override the import's own switching");
    assert_eq!(part.dynamics, -1);
}

#[test]
fn uvi_momentary_buttons_callback_once_per_click_or_keyboard_activation() {
    let xml = "<UVI4><Program Name='P'><EventProcessors><ScriptProcessor Name='S'><script><![CDATA[
        setSize(200,100)
        clicks=0
        local p=Panel{'P',bounds={0,0,200,100}}
        local b=p:Button{'Fire',bounds={10,10,100,25}}
        b.changed=function() clicks=clicks+1 end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut host = sampler_uvi::script::ScriptHost::new(xml, (), Default::default()).unwrap();
    let mut face = host.interface();
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let assets = ir_view::Assets::default();
    let tick = |ui: &mut Ui, face: &ir::Interface, values: &mut ir_view::Values,
                host: &mut sampler_uvi::script::ScriptHost, input| {
        let before = values.clone();
        let root = ir_view::view(ui, face, ir::PageRef(0), &assets, ir::Presentation::Vector, 1., values);
        ui.frame(root, Some(Size::new(200.,100.)), input, 1./60.).unwrap();
        for (&id,&value) in values.iter() {
            if before.get(&id).copied().unwrap_or(0.) != value { host.set_control(id,value).unwrap(); }
        }
    };
    tick(&mut ui,&face,&mut values,&mut host,Input::default());
    let at = Point::new(50.,20.);
    let pointer = |held| Input { pointer: PointerInput { pos:Some(at), buttons:if held {Buttons::PRIMARY}else{Buttons::default()}, ..Default::default() }, ..Default::default() };
    for _ in 0..5 { tick(&mut ui,&face,&mut values,&mut host,pointer(true)); }
    assert_eq!(host.global_text("clicks"),"0");
    tick(&mut ui,&face,&mut values,&mut host,pointer(false));
    for _ in 0..3 { tick(&mut ui,&face,&mut values,&mut host,Input::default()); }
    assert_eq!(host.global_text("clicks"),"1");
    ui.focus("ir-1");
    tick(&mut ui,&face,&mut values,&mut host,Input { keys:vec![KeyPress {key:Key::Enter,mods:Default::default()}], ..Default::default() });
    for _ in 0..3 { tick(&mut ui,&face,&mut values,&mut host,Input::default()); }
    assert_eq!(host.global_text("clicks"),"2");
    face.widgets[0].enabled=false;
    tick(&mut ui,&face,&mut values,&mut host,Input::default());
    assert!(ui.scene().unwrap().surface("ir-1").is_some());
}

/// Audit-only gesture probe: the same renderer and input loop as the editor.
fn audit_motion(face: &ir::Interface, target: usize, dx: f64, dy: f64) -> (f64, bool) {
    audit_motion_readback(face, target, dx, dy, false)
}

fn audit_motion_readback(face: &ir::Interface, target: usize, dx: f64, dy: f64, round_each_frame: bool) -> (f64, bool) {
    let assets = ir_view::Assets::default();
    let mut values = ir_view::Values::default();
    let ir::Binding::Control(control) = face.widgets[target].binding else { return (0., false) };
    let start = match &face.widgets[target].kind {
        ir::Kind::Knob { range, .. } | ir::Kind::Slider { range, .. } => (range.min + range.max) / 2.,
        _ => 0.,
    };
    values.insert(control, start);
    let mut ui = settle(f64::from(face.pages[0].size.width), f64::from(ir_view::height(face, ir::PageRef(0))), |ui| {
        ir_view::view(ui, face, ir::PageRef(0), &assets, ir::Presentation::Vector, 1., &mut values)
    });
    let id = format!("ir-{target}");
    let Some(surface) = ui.scene().unwrap().surface(&id) else {
        if dx == 0. { println!("AUDIT_MISS target={target} page={}", face.widgets[target].page.0); }
        return (0., false)
    };
    let at = Point::new(surface.frame.x + surface.frame.size.width / 2., surface.frame.y + surface.frame.size.height / 2.);
    let mut pressed = false;
    let steps = if round_each_frame { 30 } else { 1 };
    let events = [(at, false), (at, true)].into_iter()
        .chain((1..=steps).map(|n| (Point::new(at.x + dx * f64::from(n) / f64::from(steps), at.y + dy * f64::from(n) / f64::from(steps)), true)))
        .chain([(Point::new(at.x + dx, at.y + dy), false)]);
    for (point, down) in events {
        for _ in 0..2 {
            let el = ir_view::view(&mut ui, face, ir::PageRef(0), &assets, ir::Presentation::Vector, 1., &mut values);
            ui.frame(el, Some(Size::new(f64::from(face.pages[0].size.width), f64::from(ir_view::height(face, ir::PageRef(0))))), Input {
                pointer: PointerInput { pos: Some(point), buttons: if down { Buttons::PRIMARY } else { Buttons::default() }, ..Default::default() },
                ..Default::default()
            }, 1. / 60.).unwrap();
            pressed |= ui.get(id.as_str()).held;
            if round_each_frame { values.values_mut().for_each(|value| *value = value.round()); }
            if dx == 0. && point == at && down && !ui.get(id.as_str()).held {
                let winners: Vec<_> = ui.scene().unwrap().surfaces().filter(|s| ui.get(s.key.as_str()).held).map(|s| s.key.to_string()).collect();
                println!("AUDIT_OCCLUDED target={target} x={} y={} held={winners:?}", at.x, at.y);
            }
        }
    }
    (*values.get(&control).unwrap() - start, pressed)
}

#[test]
fn widget_negative_mouse_behaviour() {
    let script = sampler_ksp::compile("on init\n declare ui_slider $s(0,1000000)\n set_control_par(get_ui_id($s),$CONTROL_PAR_MOUSE_BEHAVIOUR,-1000)\nend on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    assert_eq!(face.widgets[0].drag.unwrap().axis, ir::Orientation::Vertical);
    let (vertical, pressed) = audit_motion(&face, 0, 0., -30.);
    let (horizontal, _) = audit_motion(&face, 0, 30., 0.);
    assert!(pressed);
    assert!(vertical > 0.);
    assert_eq!(horizontal, 0.);
}

#[test]
fn widget_passive_overlay_passes_knob() {
    let script = sampler_ksp::compile("on init\n declare ui_knob $k(0,1000000,1)\n declare ui_label $l(1,1)\n move_control_px($k,20,20)\n move_control_px($l,20,20)\n set_control_par(get_ui_id($l),$CONTROL_PAR_WIDTH,85)\n set_control_par(get_ui_id($l),$CONTROL_PAR_HEIGHT,52)\nend on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let (delta, captured) = audit_motion(&face, 0, 0., -30.);
    assert!(captured);
    assert!(delta > 0.);
}

#[test]
fn widget_integer_readback_retains_substeps() {
    let script = sampler_ksp::compile("on init\n declare ui_knob $k(0,2,1)\nend on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let (free, captured) = audit_motion_readback(&face, 0, 0., -60., false);
    let (rounded, _) = audit_motion_readback(&face, 0, 0., -60., true);
    assert!(captured && free > 0.5);
    assert!(rounded >= 1., "fractional drags must survive integer feedback: {rounded}");
}


#[test]
fn widget_menu_passive_value_is_unchanged() {
    let script = sampler_ksp::compile("on init\n declare ui_menu $m\n add_menu_item($m,\"Ten\",10)\n add_menu_item($m,\"Forty\",40)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_|None).unwrap());
    let ir::Binding::Control(control) = face.widgets[0].binding else { panic!("binding") };
    let mut values = ir_view::Values::from([(control, -1.)]);
    let assets = ir_view::Assets::default();
    settle(633., 100., |ui| ir_view::view(ui, &face, ir::PageRef(0), &assets, ir::Presentation::Vector, 1., &mut values));
    assert_eq!(values[&control], -1.);
}

#[test]
fn widget_disabled_knob_does_not_edit() {
    let script = sampler_ksp::compile("on init\n declare ui_knob $k(0,100,1)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_|None).unwrap());
    face.widgets[0].enabled = false;
    let (delta, captured) = audit_motion(&face,0,0.,-30.);
    assert_eq!(delta,0.);
    assert!(!captured);
}

#[test]
#[ignore = "requires locally owned Conflux NKI; counts and input only"]
fn widget_conflux_placement_and_capture() {
    use std::collections::BTreeSet;
    fn names(raw: &serde_json::Value, path: &str, out: &mut BTreeSet<String>) {
        for control in raw.as_array().into_iter().flatten() {
            let value = &control["value"];
            let id = value["common"]["id"].as_str().unwrap_or_default();
            let name = if path.is_empty() { id.to_owned() } else { format!("{path}_{id}") };
            let prefix = match control["index"].as_i64().unwrap() { 9 => '%',10 => '@', _ => '$' };
            out.insert(format!("{prefix}{name}"));
            names(&value["controls"],&name,out);
        }
    }
    let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
    let mut source = sampler_kontakt::read(&path).unwrap().instrument;
    let mut resources = sampler_kontakt::Resources::of(&path);
    for behavior in &source.behaviors {
        if let Some(name) = sampler_ksp::nckp::view_name(&behavior.source) {
            let bytes = resources.read(&format!("Resources/performance_view/{name}.nckp")).unwrap();
            let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let mut raw_names = BTreeSet::new();
            names(&raw["value"]["performanceView"]["controls"],"",&mut raw_names);
            let (parsed, skipped) = sampler_ksp::nckp::parse(&bytes).unwrap();
            let parsed_names = parsed.controls.iter().map(|c|c.name.clone()).collect::<BTreeSet<_>>();
            assert!(skipped.is_empty());
            assert_eq!(raw_names.len(),parsed_names.len());
            assert!(raw_names == parsed_names,"raw hierarchy and reader names disagree");
            fn raw_knobs(raw: &serde_json::Value) -> usize {
                raw.as_array().into_iter().flatten().map(|c| usize::from(c["index"] == 3) + raw_knobs(&c["value"]["controls"])).sum()
            }
            let knobs = raw_knobs(&raw["value"]["performanceView"]["controls"]);
            assert_eq!(knobs,parsed.controls.iter().filter(|c| c.kind == sampler_ksp::model::WidgetKind::Knob).count());
            println!("CONFLUX_NCKP raw={} parsed={} raw_knobs={knobs}",raw_names.len(),parsed_names.len());
        }
    }
    source.zones.clear();
    source.assets.clear();
    let loaded = sampler_kontakt::prepare(source,vec![],&sampler_kontakt::Options {library:Some(path), ..Default::default()}).unwrap();
    let face = ir_view::resolved(loaded.interfaces.iter().max_by_key(|f|f.widgets.len()).unwrap());
    assert_eq!(face.widgets.len(),378,"unresolved handles must not publish widgets");
    let all_knobs = face.widgets.iter().filter(|w| matches!(w.kind,ir::Kind::Knob{..})).count();
    let origins = face.widgets.iter().enumerate().filter(|(n,w)| face.visible(ir::WidgetRef(*n)) && matches!(w.kind,ir::Kind::Knob{..}|ir::Kind::Slider{..}) && face.page_rect(ir::WidgetRef(*n)).x == 0 && face.page_rect(ir::WidgetRef(*n)).y == 0).count();
    println!("CONFLUX_PLACEMENT widgets={} all_knobs={all_knobs} visible_knobs_at_origin={origins}",face.widgets.len());
    assert_eq!(origins,0);
    let mut count = [0;3];
    for (n, _) in face.widgets.iter().enumerate().filter(|(n,w)| face.visible(ir::WidgetRef(*n)) && matches!(w.kind,ir::Kind::Knob{..}|ir::Kind::Slider{..})) {
        let (delta,captured) = audit_motion(&face,n,0.,-100.);
        count[0] += 1;
        count[1] += usize::from(captured);
        count[2] += usize::from(delta>0.);
    }
    println!("CONFLUX_CAPTURE visible={} captured={} increase={}",count[0],count[1],count[2]);
    assert_eq!(count[0],count[1]);
    assert_eq!(count[0],count[2]);
}

#[test]
fn widget_menu_selects_semantic_value_and_value_edit_accepts_typing() {
    let script = sampler_ksp::compile("on init\n declare ui_menu $m\n add_menu_item($m,\"first\",7)\n add_menu_item($m,\"second\",23)\n declare ui_value_edit $v(0,100,10)\n move_control_px($v,100,30)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_|None).unwrap());
    let ir::Binding::Control(menu) = face.widgets[0].binding else {panic!()};
    let ir::Binding::Control(value) = face.widgets[1].binding else {panic!()};
    let mut values = ir_view::Values::from([(menu,99.),(value,30.)]);
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui:&mut Ui,values:&mut ir_view::Values,state:&mut ir_view::InputState,input:Input| {
        let el = ir_view::view_state(ui,"probe",&face,ir::PageRef(0),&assets,ir::Presentation::Vector,1.,values,state);
        ui.frame(el,Some(Size::new(633.,300.)),input,1./60.).unwrap();
    };
    let key = |key| Input{keys:vec![KeyPress{key,mods:Mods::default()}],..Default::default()};
    for _ in 0..4 {tick(&mut ui,&mut values,&mut state,Input::default());}
    assert_eq!(values[&menu],99.,"passive paint preserves unknown semantic values");
    ui.focus("probe-ir-0");
    tick(&mut ui,&mut values,&mut state,key(Key::Enter));
    for _ in 0..3 {tick(&mut ui,&mut values,&mut state,Input::default());}
    assert!(ui.scene().unwrap().surface("probe-ir-0-popup").is_some());
    ui.focus("probe-ir-0-item-1");
    tick(&mut ui,&mut values,&mut state,key(Key::Enter));
    for _ in 0..3 {tick(&mut ui,&mut values,&mut state,Input::default());}
    assert_eq!(values[&menu],23.);
    ui.focus("probe-ir-1");
    tick(&mut ui,&mut values,&mut state,key(Key::Enter));
    for _ in 0..3 {tick(&mut ui,&mut values,&mut state,Input::default());}
    assert!(ui.scene().unwrap().surface("probe-ir-1-type").is_some());
    tick(&mut ui,&mut values,&mut state,Input{keys:vec![KeyPress{key:Key::Char('a'),mods:Mods{ctrl:true,..Default::default()}}],..Default::default()});
    tick(&mut ui,&mut values,&mut state,Input{text:"3.7".into(),..Default::default()});
    tick(&mut ui,&mut values,&mut state,key(Key::Enter));
    for _ in 0..3 {tick(&mut ui,&mut values,&mut state,Input::default());}
    assert_eq!(values[&value],37.,"typed display units are converted to authored units");
    assert!(state.edits.iter().any(|e| e.widget == ir::WidgetRef(0) && e.value == ir::Value::Integer(23)));
    assert!(state.edits.iter().any(|e| e.widget == ir::WidgetRef(1) && e.value == ir::Value::Integer(37)));
}

#[test]
fn widget_generated_menu_popup_anchors_to_scene() {
    let script = sampler_ksp::compile("on init\n declare ui_menu $m\n add_menu_item($m,\"first\",7)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_|None).unwrap());
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui:&mut Ui, values:&mut ir_view::Values, state:&mut ir_view::InputState, input:Input| {
        let widget = ir_view::widget_state(ui,"generated",&face,ir::WidgetRef(0),&assets,ir::Presentation::Vector,1.,values,state,130.,30.).at(300.,100.);
        let mut layers = vec![widget];
        if let Some(popup) = ir_view::menu_popup(ui,"generated",&face,1.,values,state,500.,250.) {layers.push(popup);}
        let generated = stack(layers).w(500).h(250).id("generated-ir-view").at(40.,30.);
        ui.frame(stack![generated],Some(Size::new(633.,400.)),input,1./60.).unwrap();
    };
    for _ in 0..4 {tick(&mut ui,&mut values,&mut state,Input::default());}
    ui.focus("generated-ir-0");
    tick(&mut ui,&mut values,&mut state,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});
    for _ in 0..4 {tick(&mut ui,&mut values,&mut state,Input::default());}
    let scene = ui.scene().unwrap();
    let root = scene.surface("generated-ir-view").unwrap().frame;
    let popup = scene.surface("generated-ir-0-popup").unwrap().frame;
    assert_eq!((popup.x-root.x,popup.y-root.y),(300.,130.));
}

#[test]
fn widget_keyboard_uses_authored_step() {
    let script = sampler_ksp::compile("on init\n declare ui_knob $k(0,1000,1)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_|None).unwrap());
    let ir::Kind::Knob{range,..} = &mut face.widgets[0].kind else {panic!()};
    range.step=Some(7.);
    let ir::Binding::Control(control)=face.widgets[0].binding else {panic!()};
    let mut values=ir_view::Values::from([(control,140.)]);
    let assets=ir_view::Assets::default();
    let mut ui=settle(633.,100.,|ui|ir_view::view(ui,&face,ir::PageRef(0),&assets,ir::Presentation::Vector,1.,&mut values));
    ui.focus("ir-0");
    for input in [Input{keys:vec![KeyPress{key:Key::Up,mods:Mods::default()}],..Default::default()},Input::default(),Input::default()] {
        let el=ir_view::view(&mut ui,&face,ir::PageRef(0),&assets,ir::Presentation::Vector,1.,&mut values);
        ui.frame(el,Some(Size::new(633.,100.)),input,1./60.).unwrap();
    }
    assert_eq!(values[&control],147.);
}

#[test]
fn widget_ids_keep_two_instances_focus_and_capture_separate() {
    let script=sampler_ksp::compile("on init\n declare ui_knob $k(0,1000,1)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face=ir_view::resolved(&script.ui(&|_|None).unwrap());
    let ir::Binding::Control(control)=face.widgets[0].binding else {panic!()};
    let mut a=ir_view::Values::from([(control,500.)]);
    let mut b=a.clone();
    let mut sa=ir_view::InputState::default(); let mut sb=ir_view::InputState::default();
    let assets=ir_view::Assets::default();
    let mut ui=theme::ui();
    let tick=|ui:&mut Ui,a:&mut ir_view::Values,b:&mut ir_view::Values,sa:&mut ir_view::InputState,sb:&mut ir_view::InputState,input:Input| {
        let left=ir_view::view_state(ui,"part-a",&face,ir::PageRef(0),&assets,ir::Presentation::Vector,1.,a,sa);
        let right=ir_view::view_state(ui,"part-b",&face,ir::PageRef(0),&assets,ir::Presentation::Vector,1.,b,sb);
        ui.frame(row![left,right],Some(Size::new(1266.,100.)),input,1./60.).unwrap();
    };
    for _ in 0..4 {tick(&mut ui,&mut a,&mut b,&mut sa,&mut sb,Input::default());}
    let surface=ui.scene().unwrap().surface("part-a-ir-0").unwrap();
    let at=Point::new(surface.frame.x+surface.frame.size.width/2.,surface.frame.y+surface.frame.size.height/2.);
    for (pos,down) in [(at,false),(at,true),(Point::new(at.x,at.y-30.),true),(Point::new(at.x,at.y-30.),false)] {
        for _ in 0..2 {tick(&mut ui,&mut a,&mut b,&mut sa,&mut sb,Input{pointer:PointerInput{pos:Some(pos),buttons:if down {Buttons::PRIMARY}else{Buttons::default()},..Default::default()},..Default::default()});}
    }
    assert!(a[&control]>500.);
    assert_eq!(b[&control],500.);
    let was=a[&control];
    ui.focus("part-b-ir-0");
    tick(&mut ui,&mut a,&mut b,&mut sa,&mut sb,Input{keys:vec![KeyPress{key:Key::Up,mods:Mods::default()}],..Default::default()});
    for _ in 0..3 {tick(&mut ui,&mut a,&mut b,&mut sa,&mut sb,Input::default());}
    assert_eq!(a[&control],was);
    assert_eq!(b[&control],501.);
}
