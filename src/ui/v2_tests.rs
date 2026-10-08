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
        inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 0, alternatives: Default::default(), ..Default::default() });
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
    let source = p.shared.view.lock().unwrap().parts[0].instrument.clone().unwrap();
    let ids = crate::sound::articulation::identities(&source.articulations);
    let row_id = |n: usize| super::inside::row_id(0, &ids[n]);
    assert!(h.ui.scene().unwrap().surface(&row_id(4)).is_some(), "every articulation has a row");
    h.press(&format!("{}-name", row_id(2)));
    h.idle(2);
    assert_eq!(p.shared.articulation_edits.pop(), Some((0, 2)), "name selection targets source identity independently of remapped inputs");
    p.shared.heard[27].store(90, Ordering::Relaxed);
    h.idle(2);
    p.shared.heard[27].store(0, Ordering::Relaxed);
    h.idle(2);
    shoot(&h.ui, 1180, 780, "app/synthetic-articulations.png");
    h.press("art-driver-0");
    h.press("menu-item-2");
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
        inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 0, alternatives: Default::default(), ..Default::default() });
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
                    inst.articulations.push(sir::Articulation { name: name.into(), switch_keys: vec![24 + n as u8], default: n == 1, alternatives: Default::default(), ..Default::default() });
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

/// Interaction evidence from OUR editor, including its compact narrow layout.
#[test]
fn keyswitch_panel_edits_swaps_learns_reorders_and_keeps_source() {
    use crate::sound::articulation::{Input as Trigger, identities};
    use sampler_ir as sir;
    let mut inst = sir::Instrument { name: "Strings".into(), ..Default::default() };
    inst.articulations = ["Legato", "Sustain", "Staccato", "Pizzicato", "Tremolo"].into_iter().enumerate().map(|(n, name)| sir::Articulation { source: format!("axis:main:{name}"), name: name.into(), switch_keys: vec![24 + n as u8], default: n == 0, ..Default::default() }).collect();
    inst.assign_alternatives(32);
    inst.articulations[0].alternatives.controller = Some(sir::ControllerRange { controller: 12, low: 3, high: 3 });
    let source = Arc::new(inst);
    let ids = identities(&source.articulations);
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part { path: "/synthetic/Strings.nki".into(), ..Default::default() });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].instrument = Some(source.clone());
        view.parts[0].active = "Strings".into();
        view.parts[0].keys = (0..128).map(|key| crate::sound::KeyLook { color: (key == 24).then_some(0), ..Default::default() }).collect::<Vec<_>>().into();
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Articulations");
    let row = |n: usize| super::inside::row_id(0, &ids[n]);
    let cell = |n: usize| format!("{}-trigger", row(n));
    for n in 0..5 {
        let surface = h.ui.scene().unwrap().surface(&row(n)).unwrap();
        assert_eq!(surface.frame.size.height, 24., "one compact row");
    }
    h.type_into(&cell(0), "C#2");
    assert_eq!(p.selection.read().unwrap().parts[0].articulation_overlay.input(&ids[0], &source.articulations[0], sir::Driver::Keys), Trigger::Keys(vec![49]));
    assert!(p.shared.articulation_edits.pop().is_none(), "editing never auditions");
    h.type_into(&cell(1), "C#2");
    assert!(h.ui.scene().unwrap().surface("art-swap-0").is_some(), "conflict offers a swap");
    h.press("art-swap-0");
    let overlay = p.selection.read().unwrap().parts[0].articulation_overlay.clone();
    assert_eq!(overlay.input(&ids[0], &source.articulations[0], sir::Driver::Keys), Trigger::Keys(vec![25]));
    assert_eq!(overlay.input(&ids[1], &source.articulations[1], sir::Driver::Keys), Trigger::Keys(vec![49]));
    let header_text = |h: &Harness| h.ui.scene().unwrap().surfaces()
        .filter(|s| s.parent.as_ref().is_some_and(|p| p.as_str() == "perf-art-0"))
        .filter_map(|s| s.text_value.as_deref()).collect::<Vec<_>>().join(" ");
    assert_eq!(header_text(&h), "Articulation Legato · C#0", "header uses the row's effective trigger after a swap");
    for n in 3..5 {
        assert_eq!(overlay.input(&ids[n], &source.articulations[n], sir::Driver::Keys), Trigger::Keys(vec![24 + n as u8]), "swap preserves other rows");
        assert_eq!(h.ui.scene().unwrap().surface(&cell(n)).unwrap().text_value.as_deref(), Some(super::theme::note_name(24 + n as u8).as_str()));
    }
    h.type_into(&cell(2), "G#8");
    assert!(h.ui.scene().unwrap().surface(&format!("{}-edit", row(2))).is_some(), "invalid input stays editable");
    assert_eq!(p.selection.read().unwrap().parts[0].articulation_overlay.input(&ids[2], &source.articulations[2], sir::Driver::Keys), Trigger::Keys(vec![26]));
    // Escape without changing the source or inputs.
    h.ui.focus(format!("{}-edit", row(2)));
    h.tick(Input { keys: vec![KeyPress { key: Key::Escape, mods: Mods::default() }], ..Default::default() });
    h.idle(2);
    h.press(&format!("{}-more", row(2)));
    assert!(h.ui.scene().unwrap().surface("menu-item-0").is_some(), "row menu opened; focus {:?}", h.ui.focus_key());
    h.press("menu-item-0");
    assert!(h.ui.scene().unwrap().surface(&format!("{}-edit", row(2))).is_some(), "learn editor opened; focus {:?}", h.ui.focus_key());
    p.shared.record_learn(1, 0, 51); // Other part port is ignored.
    p.shared.record_learn(0, 0, 50); // On and off before a frame still learns.
    h.idle(2);
    assert_eq!(p.selection.read().unwrap().parts[0].articulation_overlay.input(&ids[2], &source.articulations[2], sir::Driver::Keys), Trigger::Keys(vec![50]));
    let before = p.selection.read().unwrap().parts[0].articulation_overlay.inputs.clone();
    h.press(&format!("{}-more", row(2)));
    let menu_frame = h.ui.scene().unwrap().surface("context-menu").unwrap().frame;
    let menu_just_opened = pixels(&h.ui, 1180, 780);
    h.idle(45);
    let menu_settled = pixels(&h.ui, 1180, 780);
    let sample = ((menu_frame.y as usize + 2) * 1180 + menu_frame.x as usize + 20) * 4;
    assert_eq!(&menu_just_opened[sample..sample+4], &menu_settled[sample..sample+4], "open menu is immediately opaque");
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(&Path::new(&dir).join("keyswitch-menu-open.png"), &pixels(&h.ui, 1180, 780), 1180, 780);
    }
    h.press("menu-item-5"); // Move down (rule occupies item 3).
    assert!(h.ui.scene().unwrap().surface("context-menu").is_none(), "closed menu has no surface");
    let just_closed = pixels(&h.ui, 1180, 780);
    h.idle(45);
    let settled = pixels(&h.ui, 1180, 780);
    // Below the list, where no row/focus animation occurs, a closing menu
    // used to leave its text painted as a fading ghost above the editor.
    for y in 360..445 {
        let range = (y * 1180 + 970) * 4 .. (y * 1180 + 1150) * 4;
        assert_eq!(&just_closed[range.clone()], &settled[range], "closed menu must disappear immediately, scanline {y}");
    }
    let overlay = p.selection.read().unwrap().parts[0].articulation_overlay.clone();
    assert_eq!(overlay.display_order(&source.articulations), vec![0, 1, 3, 2, 4]);
    assert_eq!(overlay.inputs, before);
    assert_eq!(*p.shared.view.lock().unwrap().parts[0].instrument.clone().unwrap(), *source);
    h.drag(&format!("{}-drag", row(4)), &row(0));
    let overlay = p.selection.read().unwrap().parts[0].articulation_overlay.clone();
    assert_eq!(overlay.display_order(&source.articulations), vec![4, 0, 1, 3, 2]);
    assert_eq!(overlay.inputs, before, "drag changes display order only");
    for n in 3..5 {
        assert_eq!(overlay.input(&ids[n], &source.articulations[n], sir::Driver::Keys), Trigger::Keys(vec![24 + n as u8]), "reorder preserves other rows");
        assert_eq!(h.ui.scene().unwrap().surface(&cell(n)).unwrap().text_value.as_deref(), Some(super::theme::note_name(24 + n as u8).as_str()));
    }
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(&Path::new(&dir).join("keyswitch-panel.png"), &pixels(&h.ui, 1180, 780), 1180, 780);
        let mut narrow = Harness::new(&p, 900., 640.);
        narrow.press("view-0-Articulations");
        moose::core::screenshot::save_png(&Path::new(&dir).join("keyswitch-panel-narrow.png"), &pixels(&narrow.ui, 900, 640), 900, 640);
    }
    for (driver, input, label) in [
        (1, Trigger::Velocity(Some((19, 36))), "19–36"),
        (2, Trigger::Channel(Some(7)), "ch 8"),
        (3, Trigger::Controller(Some((12, 3, 3))), "CC12 3"),
        (4, Trigger::Program(Some(90)), "prog 91"),
        (0, Trigger::Keys(vec![]), "—"),
    ] {
        {
            let mut selection = p.selection.write().unwrap();
            selection.parts[0].articulation_overlay.driver = Some(driver);
            selection.parts[0].articulation_overlay.set(&ids[0], input);
        }
        h.idle(3);
        assert_eq!(header_text(&h), format!("Articulation Legato · {label}"), "header follows the effective input family");
        assert_eq!(h.ui.scene().unwrap().surface(&cell(0)).unwrap().text_value.as_deref(), Some(label));
    }
}

#[test]
fn keyswitch_real_afflatus_panel_uses_normalized_rows_and_authored_colours() {
    use crate::sound::{CoreLoader, LoadRequest, v2::V2Loader};
    let path = Path::new("/mnt/MAIN_STORAGE/Libraries/Kontakt/Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki");
    if !path.is_file() { eprintln!("SKIP: Afflatus missing"); return; }
    let loaded = V2Loader.prepare(&LoadRequest { path: path.into(), sample_rate: 48000., ..Default::default() }, &mut |_| {}, &|| false).unwrap();
    let inst = loaded.instrument.unwrap();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts.push(crate::plugin::Part { path: path.to_string_lossy().into_owned(), ..Default::default() });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].active = inst.name.clone();
        view.parts[0].instrument = Some(inst.clone());
        view.parts[0].keys = loaded.scripts.keys();
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Articulations");
    for source in crate::sound::articulation::identities(&inst.articulations) {
        assert_eq!(h.ui.scene().unwrap().surface(&super::inside::row_id(0, &source)).unwrap().frame.size.height, 24.);
    }
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(&Path::new(&dir).join("keyswitch-afflatus.png"), &pixels(&h.ui, 1180, 780), 1180, 780);
    }
}

#[test]
fn keyswitch_replacing_a_preset_clears_its_user_overlay() {
    use crate::sound::articulation::Input as Trigger;
    let mut part = crate::plugin::Part { path: "/old.nki".into(), ..Default::default() };
    part.articulation_overlay.set("native:groups:0:rows:0:keys:24-24#0", Trigger::Keys(vec![49]));
    part.articulation_overlay.keep_originals = true;
    part.articulation_overlay.driver = Some(3);
    super::replace_part(&mut part, "/new.nki".into());
    assert_eq!(part.articulation_overlay, Default::default());
}

#[test]
fn v1_mixer_view_controls_are_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("tab-mixer");
    h.idle(4);
    for id in ["mix-narrow", "mix-wide", "mix-spectrum-off", "mix-spectrum-part", "mix-spectrum-master"] {
        assert!(h.ui.scene().unwrap().surface(id).is_some(), "v1 control missing: {id}");
    }
}
