//! Checks and screenshots for the v2 views: the nested mixer, the load report
//! and the UI-IR renderer. Shots land in `artifacts/v2-ui/`.

use super::ir_view::Picture;
use super::{
    ir_view, load_report as lr, mix_tree as mt,
    tests::{Harness, pixels},
    theme,
};
use moose::mui::mui::prelude::*;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Lays `build` out a few frames at `w`×`h` and returns the ui.
fn settle(w: f64, h: f64, mut build: impl FnMut(&mut Ui) -> El) -> Ui {
    let mut ui = theme::ui();
    for _ in 0..4 {
        let root = col![build(&mut ui)].w(w).h(h).fill(Role::Background);
        ui.frame(root, Some(Size::new(w, h)), Input::default(), 1. / 60.)
            .unwrap();
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
    let ui = settle(1180., 520., |ui| {
        mt::view(ui, &mut tree, &mut state, 8, 480., levels.clone())
    });
    let height = |id: &str| {
        ui.scene()
            .unwrap()
            .surface(id)
            .unwrap_or_else(|| panic!("{id}"))
            .frame
            .size
            .height
    };
    let (top, mid, low) = (
        height("mt-strip-1"),
        height("mt-strip-2"),
        height("mt-strip-3"),
    );
    assert!(
        top > mid && mid > low,
        "each level is shorter: {top} {mid} {low}"
    );
    let bottom = |id: &str| {
        let f = ui.scene().unwrap().surface(id).unwrap().frame;
        f.y + f.size.height
    };
    assert!(
        (bottom("mt-strip-1") - bottom("mt-strip-3")).abs() < 1.,
        "strips share a baseline"
    );
    shoot(&ui, 1180, 520, "mixer-tree.png");

    state.folded.insert(1);
    let ui = settle(1180., 520., |ui| {
        mt::view(ui, &mut tree, &mut state, 8, 480., levels.clone())
    });
    assert!(
        ui.scene().unwrap().surface("mt-strip-3").is_none(),
        "folding hides the subtree"
    );
    assert!(ui.scene().unwrap().surface("mt-strip-10").is_some());
}

#[test]
fn load_report_shot() {
    let r = lr::Report {
        instrument: "Vista - 3 Cellos".into(),
        why_silent: Some("key 60: 1122 zones rejected by group selection (script)".into()),
        faults: vec!["InvalidInput in script 1 on note".into()],
        loaded: vec![
            lr::Loaded {
                area: lr::Area::Mapping,
                summary: "3 412 zones · 48 groups".into(),
            },
            lr::Loaded {
                area: lr::Area::Samples,
                summary: "2.1 GB, 64 KB resident heads".into(),
            },
            lr::Loaded {
                area: lr::Area::Scripts,
                summary: "3 of 4 running".into(),
            },
            lr::Loaded {
                area: lr::Area::Interface,
                summary: "47 controls".into(),
            },
        ],
        missing: vec![
            lr::Missing::ScriptError {
                script: "Legato".into(),
                line: 412,
                column: 9,
                message: "unknown variable $vel_curve".into(),
            },
            lr::Missing::Sample {
                path: "Samples/Cello_C3_pp_rr1.ncw".into(),
            },
            lr::Missing::Sample {
                path: "Samples/Cello_C3_pp_rr2.ncw".into(),
            },
            lr::Missing::Sample {
                path: "Samples/Cello_D3_pp_rr1.ncw".into(),
            },
            lr::Missing::Sample {
                path: "Samples/Cello_D3_pp_rr2.ncw".into(),
            },
            lr::Missing::Effect {
                module: "Convolution".into(),
                location: "Insert 2, bus 1".into(),
                params: vec![
                    ("size".into(), "1.20".into()),
                    ("mix".into(), "30 %".into()),
                ],
            },
            lr::Missing::Modulation {
                law: "Step modulator, retrigger off".into(),
                location: "Group 'Shorts', filter cutoff".into(),
            },
            lr::Missing::ScriptBuiltin {
                script: "Main".into(),
                name: "load_ir_async".into(),
                line: 88,
                column: 5,
            },
        ],
        runtime: vec![
            lr::Runtime::ScriptBudget { overruns: 12 },
            lr::Runtime::StreamUnderruns { count: 3 },
        ],
    };
    let mut state = lr::State::default();
    let ui = settle(760., 620., |ui| lr::view(ui, &mut state, &r));
    assert!(
        ui.scene().unwrap().surface("report-entry-1").is_some(),
        "missing samples have an expandable summary"
    );
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
    let image = |path: &str, frames| ir::Asset {
        path: path.into(),
        kind: AssetKind::Image(ImageMeta {
            frames,
            ..ImageMeta::default()
        }),
    };
    let mut panel = Widget::new("$panel", page, Rect::new(20, 20, 300, 120), Kind::Panel);
    panel
        .images
        .push(ImageUse::new(ir::AssetRef(1), Role::Background));
    let range = ir::Range {
        min: 0.,
        max: 100.,
        default: 50.,
        step: None,
    };
    let mut knob = Widget::new(
        "$cutoff",
        page,
        Rect::new(20, 20, 64, 64),
        Kind::Knob {
            range,
            display: ir::Display::default(),
        },
    );
    knob.parent = Some(ir::WidgetRef(0));
    knob.binding = ir::Binding::Control(ir::ControlId(1));
    knob.images
        .push(ImageUse::new(ir::AssetRef(2), Role::Strip));
    let mut switch = Widget::new("$legato", page, Rect::new(120, 40, 80, 24), Kind::Switch);
    switch.parent = Some(ir::WidgetRef(0));
    switch.binding = ir::Binding::Control(ir::ControlId(2));
    switch.text = "Legato".into();
    switch
        .images
        .push(ImageUse::new(ir::AssetRef(3), Role::Strip));
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        pages: vec![ir::Page {
            name: "Main".into(),
            size: ir::Size {
                width: 633,
                height: 300,
            },
            background: ir::Background {
                image: Some(ir::AssetRef(0)),
                ..Default::default()
            },
            ..Default::default()
        }],
        widgets: vec![panel, knob, switch],
        assets: vec![
            image("wallpaper", 1),
            image("panel", 1),
            image("knob", 64),
            image("switch", 2),
        ],
        ..Default::default()
    };
    face.validate().unwrap();
    let load = |a: &ir::Asset| {
        Some(match a.path.as_str() {
            "wallpaper" => picture(vec![solid(633, 300, [40, 52, 70, 255])]),
            "panel" => picture(vec![solid(300, 120, [20, 24, 30, 255])]),
            "knob" => picture(
                (0..64)
                    .map(|n| solid(64, 64, [n * 4, 120, 200, 255]))
                    .collect(),
            ),
            _ => picture(vec![
                solid(80, 24, [60, 60, 60, 255]),
                solid(80, 24, [200, 200, 200, 255]),
            ]),
        })
    };
    let mut assets = ir_view::Assets::default();
    assets.sync(&face, ir::Presentation::Bitmap, load);
    let bitmap = assets.bytes();
    let mut values = ir_view::Values::default();
    let ui = settle(633., 300., |ui| {
        ir_view::view(
            ui,
            &face,
            page,
            &assets,
            ir::Presentation::Bitmap,
            1.,
            &mut values,
        )
    });
    shoot(&ui, 633, 300, "ir-bitmap-synthetic.png");
    assets.sync(&face, ir::Presentation::Vector, load);
    let vector = assets.bytes();
    let ui = settle(633., 300., |ui| {
        ir_view::view(
            ui,
            &face,
            page,
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
        )
    });
    shoot(&ui, 633, 300, "ir-vector-synthetic.png");
    assert_eq!(
        bitmap - vector,
        64 * 64 * 4 * 64 + 80 * 24 * 4 * 2,
        "strips released"
    );
    assert_eq!(
        vector,
        (633 * 300 + 300 * 120) * 4,
        "wallpaper and panel art kept"
    );
    assert_eq!(
        values.get(&ir::ControlId(1)),
        Some(&50.),
        "a value starts at its default"
    );
}

const NO_LIMITS: sampler_ksp::Limits = sampler_ksp::Limits {
    source_bytes: usize::MAX,
    instructions: usize::MAX,
    variables: usize::MAX,
    array_cells: usize::MAX,
};

#[test]
fn authored_uvi_value_units_match_v1_readouts_without_changing_raw_values() {
    // v1 4bffbb18:src/ui/uvi_instrument.rs::documented_used_units_format_without_rescaling_values_or_edits.
    for (unit, raw, expected) in [
        ("Percent", 25., "25 %"),
        ("PercentNormalized", 0.375, "37.5 %"),
        ("Seconds", 0.25, "250 ms"),
        ("Seconds", 1., "1 s"),
        ("MilliSeconds", 1000., "1000 ms"),
        ("MilliSeconds", 1250., "1.25 s"),
        ("Hertz", 1000., "1000 Hz"),
        ("Hertz", 1250., "1.25 kHz"),
        ("Decibels", -60., "-60 dB"),
        ("LinearGain", 0., "-inf dB"),
        ("LinearGain", 1., "0 dB"),
        ("LinearGain", 0.5, "-6.021 dB"),
        ("Pan", 0., "Center"),
        ("SemiTones", -3., "-3 st"),
    ] {
        for kind in ["Knob", "NumBox"] {
            let source = format!(
                "<UVI4><Program Name='P'><EventProcessors><ScriptProcessor><script><![CDATA[setSize(160,120); {kind}{{'Readout',{raw},-10000,10000,unit=Unit.{unit},bounds={{10,10,140,90}},showLabel=false}}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
            );
            let host =
                sampler_uvi::script::ScriptHost::new(&source, (), Default::default()).unwrap();
            let face = host.interface();
            let mut oracle = face.clone();
            oracle.widgets[0].value_text = Some(expected.into());
            let render = |f: &ir::Interface| {
                let mut values = ir_view::Values::default();
                let assets = ir_view::Assets::default();
                let ui = settle(160., 120., |ui| {
                    ir_view::view(
                        ui,
                        f,
                        ir::PageRef(0),
                        &assets,
                        ir::Presentation::Bitmap,
                        1.,
                        &mut values,
                    )
                });
                assert_eq!(values.values().copied().collect::<Vec<_>>(), vec![raw]);
                pixels(&ui, 160, 120)
            };
            assert!(
                render(&face) == render(&oracle),
                "{kind} {unit} readout must match v1"
            );
            assert_eq!(host.control_values()[0].1, raw);
        }
    }
}

#[test]
fn authored_uvi_value_captions_overlay_skin_without_changing_geometry() {
    let host = sampler_uvi::script::ScriptHost::new("<UVI4><Program Name='P'><EventProcessors><ScriptProcessor><script><![CDATA[setSize(160,120); local k=Knob{'Readout',0.375,0,1,bounds={10,10,140,90},showLabel=false,displayText='Authored caption'}; k:setStripImage('synthetic.png',1)]]></script></ScriptProcessor></EventProcessors></Program></UVI4>", (), Default::default()).unwrap();
    let face = host.interface();
    assert_eq!(face.widgets[0].rect, ir::Rect::new(10, 10, 140, 90));
    let mut hidden = face.clone();
    hidden.widgets[0].hide.value = true;
    let render = |f: &ir::Interface| {
        let mut values = ir_view::Values::default();
        let mut assets = ir_view::Assets::default();
        assets.sync(f, ir::Presentation::Bitmap, |_| {
            Some(picture(vec![solid(32, 32, [40, 50, 60, 255])]))
        });
        let ui = settle(160., 120., |ui| {
            ir_view::view(
                ui,
                f,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Bitmap,
                1.,
                &mut values,
            )
        });
        let frame = ui.scene().unwrap().surface("ir-0").unwrap().frame;
        assert_eq!(frame.size, Size::new(140., 90.));
        pixels(&ui, 160, 120)
    };
    assert!(
        render(&face) != render(&hidden),
        "authored showValue must paint its caption over the strip"
    );
    assert_eq!(host.control_values()[0].1, 0.375);
}

/// Renders `face` in both presentations, shooting each as `{stem}-{mode}.png`
/// under `dir`; returns the decoded bytes each keeps.
fn both_modes(
    face: &ir::Interface,
    load: &mut dyn FnMut(&ir::Asset) -> Option<Arc<Picture>>,
    dir: &str,
    stem: &str,
) -> [usize; 2] {
    let face = ir_view::resolved(face);
    let page = &face.pages[0];
    let (w, h) = (
        page.size.width.clamp(1, 1200) as u16,
        ir_view::height(&face, ir::PageRef(0)).clamp(1, 900) as u16,
    );
    let mut values = ir_view::Values::default();
    let mut assets = ir_view::Assets::default();
    let mut bytes = [0; 2];
    for (n, (p, mode)) in [
        (ir::Presentation::Bitmap, "bitmap"),
        (ir::Presentation::Vector, "vector"),
    ]
    .into_iter()
    .enumerate()
    {
        assets.sync(&face, p, &mut *load);
        bytes[n] = assets.bytes();
        let ui = settle(f64::from(w), f64::from(h), |ui| {
            ir_view::view(ui, &face, ir::PageRef(0), &assets, p, 1., &mut values)
        });
        shoot(&ui, w, h, &format!("{dir}/{stem}-{mode}.png"));
    }
    bytes
}

/// Every script interface the KSP frontend emits from the extracted corpus
/// draws in both presentations. Opt-in: `KSP_CORPUS` (default ~/.cache/ksp-corpus).
#[test]
#[ignore = "needs the extracted KSP corpus (kept outside the repo)"]
fn ir_view_draws_the_ksp_corpus() {
    let dir = std::env::var("KSP_CORPUS")
        .unwrap_or_else(|_| format!("{}/.cache/ksp-corpus", std::env::var("HOME").unwrap()));
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "ksp"))
        .collect();
    paths.sort();
    let (mut drawn, mut widgets) = (0, 0);
    for path in &paths {
        let source = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        let Ok(script) = sampler_ksp::compile(&source, 48000, NO_LIMITS, &[]) else {
            continue;
        };
        let face = script.ui(&|_| None).unwrap();
        if face.widgets.is_empty() {
            continue;
        }
        widgets += face.widgets.len();
        both_modes(
            &face,
            &mut |_| None,
            "corpus",
            &path.file_stem().unwrap().to_string_lossy(),
        );
        drawn += 1;
    }
    eprintln!(
        "drew {drawn} interfaces ({widgets} widgets) of {} scripts in both presentations",
        paths.len()
    );
    assert!(drawn > 0);
}

/// A real instrument through the v2 Kontakt loader into UI IR, with the
/// library's own pictures, in both presentations; prints the decoded pixels
/// each keeps. Opt-in: `KONTRA_UI_IR_PATCH=/path/to/patch.nki`.
#[test]
#[ignore = "set KONTRA_UI_IR_PATCH to a locally owned instrument"]
fn ir_view_real_instrument_memory() {
    let patch = std::path::PathBuf::from(
        std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH"),
    );
    // Only key 0's samples: the interfaces are what this measures.
    let options = sampler_kontakt::Options {
        keys: 0..=0,
        library: Some(patch.clone()),
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(&patch, &options, |_| {}).unwrap();
    let mut face = loaded
        .interfaces
        .into_iter()
        .max_by_key(|u| u.widgets.len())
        .expect("a script with an interface");
    // A wallpaper the saved state does not name can be given a stand-in to measure.
    if let (Some(a), Ok(n)) = (
        face.pages[0].background.image,
        std::env::var("KONTRA_UI_IR_WALLPAPER"),
    ) && face.assets[a.0].path.ends_with("/.png")
    {
        face.assets[a.0].path = format!("Resources/pictures/{n}.png");
    }
    let mut source = super::pictures::Source::of(&patch);
    if std::env::var_os("DUMP_WIDGETS").is_some() {
        let r = ir_view::resolved(&face);
        for (n, w) in r.widgets.iter().enumerate() {
            eprintln!(
                "W{n} {} {:?} {:?} vis={} hide={:?} text={:?} val={:?} imgs={:?} parent={:?}",
                w.name,
                kind_name(&w.kind),
                r.page_rect(ir::WidgetRef(n)),
                r.visible(ir::WidgetRef(n)),
                w.hide,
                w.text,
                w.value_text,
                w.images
                    .iter()
                    .map(|i| (r.assets[i.asset.0].path.clone(), i.role))
                    .collect::<Vec<_>>(),
                w.parent
            );
        }
        eprintln!("PAGE {:?} {:?}", r.pages[0], r.unsupported);
    }
    let stem = Path::new(&patch)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .replace(' ', "_");
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
    let patch = std::path::PathBuf::from(
        std::env::var_os("KONTRA_UI_IR_PATCH").expect("KONTRA_UI_IR_PATCH"),
    );
    let options = sampler_kontakt::Options {
        rate: 48000,
        keys: 0..=127,
        scripts: true,
        library: Some(patch.clone()),
        ..Default::default()
    };
    let loaded = sampler_kontakt::load(&patch, &options, |_| {}).unwrap();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: patch.to_string_lossy().into_owned(),
            ..Default::default()
        });
    {
        let mut v = p.shared.view.lock().unwrap();
        if v.parts.is_empty() {
            v.parts.push(Default::default());
        }
        let part = &mut v.parts[0];
        part.active = loaded.instrument.name.clone();
        part.report = Some(Arc::new(LoadReport::of(
            &loaded.instrument,
            &patch,
            loaded.plan.sample_count(),
        )));
        part.tree = Some(Arc::new(MixTree::instrument(&loaded.instrument.name)));
        part.interfaces = loaded.interfaces.into();
        part.instrument = Some(Arc::new(loaded.instrument));
    }
    let stem = patch
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .replace(' ', "_");
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
    let mut inst = sir::Instrument {
        name: "Strings".into(),
        ..Default::default()
    };
    for (n, name) in ["Legato", "Sustain", "Staccato", "Pizzicato", "Tremolo"]
        .into_iter()
        .enumerate()
    {
        inst.groups.push(sir::Group {
            name: name.into(),
            ..Default::default()
        });
        inst.articulations.push(sir::Articulation {
            name: name.into(),
            switch_keys: vec![24 + n as u8],
            default: n == 0,
            alternatives: Default::default(),
            ..Default::default()
        });
        for (v, (lo, hi)) in [(1, 63), (64, 127)].into_iter().enumerate() {
            let mut z = sir::Zone::new(sir::AssetRef(0));
            z.group = Some(sir::GroupRef(n));
            z.keys = sir::KeyRange {
                low: 36 + v as u8 * 3,
                high: 84 - n as u8 * 4,
            };
            z.velocities = sir::VelocityRange { low: lo, high: hi };
            inst.zones.push(z);
        }
    }
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/x/Strings.nki".into(),
            ..Default::default()
        });
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
    let source = p.shared.view.lock().unwrap().parts[0]
        .instrument
        .clone()
        .unwrap();
    let ids = crate::sound::articulation::identities(&source.articulations);
    let row_id = |n: usize| super::inside::row_id(0, &ids[n]);
    assert!(
        h.ui.scene().unwrap().surface(&row_id(4)).is_some(),
        "every articulation has a row"
    );
    h.press(&format!("{}-name", row_id(2)));
    h.idle(2);
    assert_eq!(
        p.shared.articulation_edits.pop(),
        Some((0, 2)),
        "name selection targets source identity independently of remapped inputs"
    );
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
    let mut inst = sir::Instrument {
        name: "Strings".into(),
        ..Default::default()
    };
    for (n, name) in ["Legato", "Staccato"].into_iter().enumerate() {
        inst.articulations.push(sir::Articulation {
            name: name.into(),
            switch_keys: vec![24 + n as u8],
            default: n == 0,
            alternatives: Default::default(),
            ..Default::default()
        });
    }
    inst.host_volume = Some(sir::HostVolume {
        controller: 7,
        saved: 0.5,
    });
    assert_eq!(
        super::part::volume_text(&inst).as_deref(),
        Some("CC7 -6.0 dB")
    );
    let mut report = crate::sound::report::LoadReport::default();
    report.decoded.dynamics = vec![(1, 0), (11, 127)];
    report.decoded.needs_controller = true;
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/x/Strings.nki".into(),
            ..Default::default()
        });
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
    assert_eq!(
        p.selection.read().unwrap().parts[0].dynamics,
        -1,
        "Kontakt's own start until picked"
    );
    h.press("dyn-0-64");
    h.idle(2);
    assert_eq!(p.selection.read().unwrap().parts[0].dynamics, 64);
    use super::part::badge_text;
    assert_eq!(badge_text("CC1", -1, 0, true), "Needs CC1");
    assert_eq!(
        badge_text("CC1", 64, 0, true),
        "CC1 starts at 64",
        "a picked start replaces the warning"
    );
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
    let drivers = [
        sir::Driver::Keys,
        sir::Driver::Velocity,
        sir::Driver::Channel,
        sir::Driver::Controller,
        sir::Driver::Program,
    ];
    let owners = [sir::SwitchOwner::Native, sir::SwitchOwner::Behavior];
    let policies = [
        sir::SwitchKeys::Keep,
        sir::SwitchKeys::Play,
        sir::SwitchKeys::Swallow,
    ];
    for owner in owners {
        for driver in drivers {
            for keys in policies {
                let mut inst = sir::Instrument {
                    name: "Imported".into(),
                    ..Default::default()
                };
                inst.switching = sir::Switching {
                    owner,
                    driver,
                    keys,
                };
                for (n, name) in ["Sustain", "Staccato", "Tremolo"].into_iter().enumerate() {
                    inst.articulations.push(sir::Articulation {
                        name: name.into(),
                        switch_keys: vec![24 + n as u8],
                        default: n == 1,
                        alternatives: Default::default(),
                        ..Default::default()
                    });
                }
                inst.assign_alternatives(32);
                let p = Arc::new(crate::plugin::SamplerParams::new());
                p.selection
                    .write()
                    .unwrap()
                    .parts
                    .push(crate::plugin::Part {
                        path: "/x/Imported.nki".into(),
                        ..Default::default()
                    });
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
                assert_eq!(
                    stored, 0,
                    "{owner:?} {driver:?} {keys:?}: the view wrote {stored:#x} over the import"
                );
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
    assert_eq!(
        part.switching, 0,
        "the old remap would override the import's own switching"
    );
    assert_eq!(part.dynamics, -1);
}

/// Interaction evidence from OUR editor, including its compact narrow layout.
#[test]
fn keyswitch_panel_edits_swaps_learns_reorders_and_keeps_source() {
    use crate::sound::articulation::{Input as Trigger, identities};
    use sampler_ir as sir;
    let mut inst = sir::Instrument {
        name: "Strings".into(),
        ..Default::default()
    };
    inst.articulations = ["Legato", "Sustain", "Staccato", "Pizzicato", "Tremolo"]
        .into_iter()
        .enumerate()
        .map(|(n, name)| sir::Articulation {
            source: format!("axis:main:{name}"),
            name: name.into(),
            switch_keys: vec![24 + n as u8],
            default: n == 0,
            ..Default::default()
        })
        .collect();
    inst.assign_alternatives(32);
    inst.articulations[0].alternatives.controller = Some(sir::ControllerRange {
        controller: 12,
        low: 3,
        high: 3,
    });
    let source = Arc::new(inst);
    let ids = identities(&source.articulations);
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/synthetic/Strings.nki".into(),
            ..Default::default()
        });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].instrument = Some(source.clone());
        view.parts[0].active = "Strings".into();
        view.parts[0].keys = (0..128)
            .map(|key| crate::sound::KeyLook {
                color: (key == 24).then_some(0),
                ..Default::default()
            })
            .collect::<Vec<_>>()
            .into();
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
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .input(&ids[0], &source.articulations[0], sir::Driver::Keys),
        Trigger::Keys(vec![49])
    );
    assert!(
        p.shared.articulation_edits.pop().is_none(),
        "editing never auditions"
    );
    h.type_into(&cell(1), "C#2");
    assert!(
        h.ui.scene().unwrap().surface("art-swap-0").is_some(),
        "conflict offers a swap"
    );
    h.press("art-swap-0");
    let overlay = p.selection.read().unwrap().parts[0]
        .articulation_overlay
        .clone();
    assert_eq!(
        overlay.input(&ids[0], &source.articulations[0], sir::Driver::Keys),
        Trigger::Keys(vec![25])
    );
    assert_eq!(
        overlay.input(&ids[1], &source.articulations[1], sir::Driver::Keys),
        Trigger::Keys(vec![49])
    );
    let header_text = |h: &Harness| {
        h.ui.scene()
            .unwrap()
            .surfaces()
            .filter(|s| {
                s.parent
                    .as_ref()
                    .is_some_and(|p| p.as_str() == "perf-art-0")
            })
            .filter_map(|s| s.text_value.as_deref())
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert_eq!(
        header_text(&h),
        "Articulation Legato · C#0",
        "header uses the row's effective trigger after a swap"
    );
    for n in 3..5 {
        assert_eq!(
            overlay.input(&ids[n], &source.articulations[n], sir::Driver::Keys),
            Trigger::Keys(vec![24 + n as u8]),
            "swap preserves other rows"
        );
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface(&cell(n))
                .unwrap()
                .text_value
                .as_deref(),
            Some(super::theme::note_name(24 + n as u8).as_str())
        );
    }
    h.type_into(&cell(2), "G#8");
    assert!(
        h.ui.scene()
            .unwrap()
            .surface(&format!("{}-edit", row(2)))
            .is_some(),
        "invalid input stays editable"
    );
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .input(&ids[2], &source.articulations[2], sir::Driver::Keys),
        Trigger::Keys(vec![26])
    );
    // Escape without changing the source or inputs.
    h.ui.focus(format!("{}-edit", row(2)));
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::Escape,
            mods: Mods::default(),
        }],
        ..Default::default()
    });
    h.idle(2);
    h.press(&format!("{}-more", row(2)));
    assert!(
        h.ui.scene().unwrap().surface("menu-item-0").is_some(),
        "row menu opened; focus {:?}",
        h.ui.focus_key()
    );
    h.press("menu-item-0");
    assert!(
        h.ui.scene()
            .unwrap()
            .surface(&format!("{}-edit", row(2)))
            .is_some(),
        "learn editor opened; focus {:?}",
        h.ui.focus_key()
    );
    p.shared.record_learn(1, 0, 51); // Other part port is ignored.
    p.shared.record_learn(0, 0, 50); // On and off before a frame still learns.
    h.idle(2);
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .input(&ids[2], &source.articulations[2], sir::Driver::Keys),
        Trigger::Keys(vec![50])
    );
    let before = p.selection.read().unwrap().parts[0]
        .articulation_overlay
        .inputs
        .clone();
    h.press(&format!("{}-more", row(2)));
    let menu_frame = h.ui.scene().unwrap().surface("context-menu").unwrap().frame;
    let menu_just_opened = pixels(&h.ui, 1180, 780);
    h.idle(45);
    let menu_settled = pixels(&h.ui, 1180, 780);
    let sample = ((menu_frame.y as usize + 2) * 1180 + menu_frame.x as usize + 20) * 4;
    assert_eq!(
        &menu_just_opened[sample..sample + 4],
        &menu_settled[sample..sample + 4],
        "open menu is immediately opaque"
    );
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(
            &Path::new(&dir).join("keyswitch-menu-open.png"),
            &pixels(&h.ui, 1180, 780),
            1180,
            780,
        );
    }
    h.press("menu-item-5"); // Move down (rule occupies item 3).
    assert!(
        h.ui.scene().unwrap().surface("context-menu").is_none(),
        "closed menu has no surface"
    );
    let just_closed = pixels(&h.ui, 1180, 780);
    h.idle(45);
    let settled = pixels(&h.ui, 1180, 780);
    // Below the list, where no row/focus animation occurs, a closing menu
    // used to leave its text painted as a fading ghost above the editor.
    for y in 360..445 {
        let range = (y * 1180 + 970) * 4..(y * 1180 + 1150) * 4;
        assert_eq!(
            &just_closed[range.clone()],
            &settled[range],
            "closed menu must disappear immediately, scanline {y}"
        );
    }
    let overlay = p.selection.read().unwrap().parts[0]
        .articulation_overlay
        .clone();
    assert_eq!(
        overlay.display_order(&source.articulations),
        vec![0, 1, 3, 2, 4]
    );
    assert_eq!(overlay.inputs, before);
    assert_eq!(
        *p.shared.view.lock().unwrap().parts[0]
            .instrument
            .clone()
            .unwrap(),
        *source
    );
    h.drag(&format!("{}-drag", row(4)), &row(0));
    let overlay = p.selection.read().unwrap().parts[0]
        .articulation_overlay
        .clone();
    assert_eq!(
        overlay.display_order(&source.articulations),
        vec![4, 0, 1, 3, 2]
    );
    assert_eq!(overlay.inputs, before, "drag changes display order only");
    for n in 3..5 {
        assert_eq!(
            overlay.input(&ids[n], &source.articulations[n], sir::Driver::Keys),
            Trigger::Keys(vec![24 + n as u8]),
            "reorder preserves other rows"
        );
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface(&cell(n))
                .unwrap()
                .text_value
                .as_deref(),
            Some(super::theme::note_name(24 + n as u8).as_str())
        );
    }
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(
            &Path::new(&dir).join("keyswitch-panel.png"),
            &pixels(&h.ui, 1180, 780),
            1180,
            780,
        );
        let mut narrow = Harness::new(&p, 900., 640.);
        narrow.press("view-0-Articulations");
        moose::core::screenshot::save_png(
            &Path::new(&dir).join("keyswitch-panel-narrow.png"),
            &pixels(&narrow.ui, 900, 640),
            900,
            640,
        );
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
        assert_eq!(
            header_text(&h),
            format!("Articulation Legato · {label}"),
            "header follows the effective input family"
        );
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface(&cell(0))
                .unwrap()
                .text_value
                .as_deref(),
            Some(label)
        );
    }
}

#[test]
fn keyswitch_real_afflatus_panel_uses_normalized_rows_and_authored_colours() {
    use crate::sound::{CoreLoader, LoadRequest, v2::V2Loader};
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki",
    );
    if !path.is_file() {
        eprintln!("SKIP: Afflatus missing");
        return;
    }
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: path.into(),
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    let inst = loaded.instrument.unwrap();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].active = inst.name.clone();
        view.parts[0].instrument = Some(inst.clone());
        view.parts[0].keys = loaded.scripts.keys();
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Articulations");
    for source in crate::sound::articulation::identities(&inst.articulations) {
        assert_eq!(
            h.ui.scene()
                .unwrap()
                .surface(&super::inside::row_id(0, &source))
                .unwrap()
                .frame
                .size
                .height,
            24.
        );
    }
    if let Ok(dir) = std::env::var("KONTRA_KEYSWITCH_SHOTS") {
        std::fs::create_dir_all(&dir).unwrap();
        moose::core::screenshot::save_png(
            &Path::new(&dir).join("keyswitch-afflatus.png"),
            &pixels(&h.ui, 1180, 780),
            1180,
            780,
        );
    }
}

#[test]
fn keyswitch_replacing_a_preset_clears_its_user_overlay() {
    use crate::sound::articulation::Input as Trigger;
    let mut part = crate::plugin::Part {
        path: "/old.nki".into(),
        ..Default::default()
    };
    part.articulation_overlay.set(
        "native:groups:0:rows:0:keys:24-24#0",
        Trigger::Keys(vec![49]),
    );
    part.articulation_overlay.keep_originals = true;
    part.articulation_overlay.driver = Some(3);
    super::replace_part(&mut part, "/new.nki".into());
    assert_eq!(part.articulation_overlay, Default::default());
}

fn keyswitch_learn_fixture() -> (
    Arc<crate::plugin::SamplerParams>,
    Arc<sampler_ir::Instrument>,
) {
    let source = Arc::new(sampler_ir::Instrument {
        name: "MIDI learn fixture".into(),
        articulations: vec![sampler_ir::Articulation {
            source: "axis:main:Sustain".into(),
            name: "Sustain".into(),
            switch_keys: vec![24],
            default: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection.write().unwrap().parts = (0..2)
        .map(|n| crate::plugin::Part {
            path: format!("/synthetic/learn-{n}.nki"),
            ..Default::default()
        })
        .collect();
    p.shared.ensure_parts(2);
    for part in &mut p.shared.view.lock().unwrap().parts {
        part.active = source.name.clone();
        part.instrument = Some(source.clone());
    }
    (p, source)
}

fn keyswitch_begin_learn(h: &mut Harness, slot: usize) {
    let row = super::inside::row_id(slot, "axis:main:Sustain#0");
    h.press(&format!("view-{slot}-Articulations"));
    h.press(&format!("{row}-more"));
    h.press("menu-item-0");
    assert!(
        h.ui.scene()
            .unwrap()
            .surface(&format!("{row}-edit"))
            .is_some()
    );
}

#[test]
fn keyswitch_learn_escape_wins_over_a_note_received_in_the_same_frame() {
    let (p, source) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::Escape,
            mods: Mods::default(),
        }],
        ..Default::default()
    });
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "Escape must cancel the pending learned note"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
    assert_eq!(
        *p.shared.view.lock().unwrap().parts[0]
            .instrument
            .clone()
            .unwrap(),
        *source
    );
}

#[test]
fn keyswitch_learn_port_change_retires_a_pending_note() {
    let (p, source) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared.record_learn(0, 0, 62);
    p.selection.write().unwrap().parts[0].port = 1;
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "a pending note belongs to the retired MIDI port"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
    assert_eq!(
        *p.shared.view.lock().unwrap().parts[0]
            .instrument
            .clone()
            .unwrap(),
        *source
    );
}

#[test]
fn keyswitch_learn_channel_change_retires_a_pending_note() {
    let (p, _) = keyswitch_learn_fixture();
    p.selection.write().unwrap().parts[0].channel = 5;
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared.record_learn(0, 5, 62);
    p.selection.write().unwrap().parts[0].channel = 6;
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "a pending note belongs to the retired MIDI channel"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_filters_port_channel_and_invalid_notes() {
    let (p, _) = keyswitch_learn_fixture();
    {
        let mut selection = p.selection.write().unwrap();
        selection.parts[0].port = 2;
        selection.parts[0].channel = 5;
    }
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    let learned = p.shared.learned_note.load(Ordering::Relaxed);
    for (port, channel, key) in [(1, 5, 62), (2, 4, 62), (2, 16, 62), (2, 5, 128)] {
        p.shared.record_learn(port, channel, key);
    }
    h.idle(3);
    assert_eq!(p.shared.learned_note.load(Ordering::Relaxed), learned);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty()
    );
    assert_ne!(p.shared.learn_target.load(Ordering::Relaxed), 0);
    p.shared.record_learn(2, 5, 62);
    h.idle(3);
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs["axis:main:Sustain#0"]
            .keys,
        Some(vec![62])
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_omni_accepts_any_valid_channel() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared.record_learn(0, 15, 62);
    h.idle(3);
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs["axis:main:Sustain#0"]
            .keys,
        Some(vec![62])
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_conflict_cancel_keeps_both_assignments() {
    let (p, source) = keyswitch_learn_fixture();
    let mut instrument = (*source).clone();
    instrument.articulations.push(sampler_ir::Articulation {
        source: "axis:main:Legato".into(),
        name: "Legato".into(),
        switch_keys: vec![25],
        ..Default::default()
    });
    let source = Arc::new(instrument);
    p.shared.view.lock().unwrap().parts[0].instrument = Some(source.clone());
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared.record_learn(0, 0, 25);
    h.idle(3);
    assert!(h.ui.scene().unwrap().surface("art-swap-0").is_some());
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty()
    );
    h.press("art-cancel-0");
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty()
    );
    assert_eq!(
        *p.shared.view.lock().unwrap().parts[0]
            .instrument
            .clone()
            .unwrap(),
        *source
    );
}

#[test]
fn keyswitch_learn_has_one_owner_across_parts() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    keyswitch_begin_learn(&mut h, 1);
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    let selection = p.selection.read().unwrap();
    assert!(
        selection.parts[0].articulation_overlay.inputs.is_empty(),
        "starting another part's learn cancels the former owner"
    );
    assert_eq!(
        selection.parts[1].articulation_overlay.inputs["axis:main:Sustain#0"].keys,
        Some(vec![62])
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_close_releases_the_request_and_keeps_the_overlay() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    let learned = p.shared.learned_note.load(Ordering::Relaxed);
    let mut editor = super::editor(p.clone());
    editor.close();
    drop(editor);
    assert_eq!(
        p.shared.learn_target.load(Ordering::Relaxed),
        0,
        "closed editor cannot keep learning host notes"
    );
    p.shared.record_learn(0, 0, 62);
    assert_eq!(p.shared.learned_note.load(Ordering::Relaxed), learned);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty()
    );
}

#[test]
fn keyswitch_learn_leaving_the_panel_cancels_the_hidden_owner() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    h.press("view-0-Info");
    assert_eq!(
        p.shared.learn_target.load(Ordering::Relaxed),
        0,
        "hidden panel must stop recording notes"
    );
    p.shared.record_learn(0, 0, 62);
    h.press("view-0-Articulations");
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty()
    );
}

#[test]
fn keyswitch_learn_replacement_cancels_even_when_source_ids_match() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared.view.lock().unwrap().parts[0].generation += 1;
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "a pending note belongs to the retired load generation"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_pending_replacement_does_not_write_the_new_overlay() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    super::replace_part(
        &mut p.selection.write().unwrap().parts[0],
        "/synthetic/replacement.nki".into(),
    );
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "old-view learn cannot alter a replacement waiting for its loader"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn keyswitch_learn_clear_is_an_explicit_cancellation() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    let row = super::inside::row_id(0, "axis:main:Sustain#0");
    h.press(&format!("{row}-more"));
    h.press("menu-item-1");
    assert_eq!(
        p.shared.learn_target.load(Ordering::Relaxed),
        0,
        "Clear trigger cancels its pending learn"
    );
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert_eq!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs["axis:main:Sustain#0"]
            .keys,
        Some(vec![])
    );
}

#[test]
fn keyswitch_learn_loader_epoch_cancels_before_new_view_publication() {
    let (p, _) = keyswitch_learn_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    keyswitch_begin_learn(&mut h, 0);
    p.shared
        .part(0)
        .unwrap()
        .generation
        .fetch_add(1, Ordering::AcqRel);
    p.shared.record_learn(0, 0, 62);
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .articulation_overlay
            .inputs
            .is_empty(),
        "loader retirement takes effect before its new view is published"
    );
    assert_eq!(p.shared.learn_target.load(Ordering::Relaxed), 0);
}

#[test]
fn v1_mixer_view_controls_are_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("tab-mixer");
    h.idle(4);
    for id in [
        "mix-narrow",
        "mix-wide",
        "mix-spectrum-off",
        "mix-spectrum-part",
        "mix-spectrum-master",
    ] {
        assert!(
            h.ui.scene().unwrap().surface(id).is_some(),
            "v1 control missing: {id}"
        );
    }
}

fn v1_menu_fixture() -> (Arc<crate::plugin::SamplerParams>, Harness) {
    let p = editor_fixture();
    let instrument = p.shared.view.lock().unwrap().parts[0].instrument.clone();
    p.shared.view.lock().unwrap().parts[1].instrument = instrument;
    {
        let mut selection = p.selection.write().unwrap();
        selection.parts = (0..2)
            .map(|n| crate::plugin::Part {
                path: format!("/synthetic/Part{n}.nki"),
                collapsed: true,
                ..Default::default()
            })
            .collect();
        selection.order = vec![0, 1];
    }
    let h = Harness::new(&p, 1180., 780.);
    (p, h)
}

fn v1_strip_menu(h: &mut Harness, id: u64) {
    h.press("tab-mixer");
    h.idle(3);
    let at = super::tests::center(&h.ui, &format!("mt-name-{id}"));
    for buttons in [
        Buttons::default().set(Button::Secondary, true),
        Buttons::default(),
    ] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(2);
}

#[test]
fn v1_menu_rack_edit_sound_selects_the_requested_part() {
    let (p, mut h) = v1_menu_fixture();
    h.press("more-1");
    h.press("menu-item-0");
    assert_eq!(
        p.shared.editor_watch.load(Ordering::Relaxed),
        1,
        "Edit sound opens the Sound tab for this part"
    );
    assert_eq!(p.shared.selected.load(Ordering::Relaxed), 1);
    assert!(!p.selection.read().unwrap().parts[1].collapsed);
}

#[test]
fn v1_menu_mixer_edit_sound_selects_the_requested_part() {
    let (p, mut h) = v1_menu_fixture();
    h.press("collapse-1");
    h.press("view-1-Sound");
    h.press("sound-tab-1-Effects");
    h.press("collapse-1");
    v1_strip_menu(&mut h, 131072);
    h.press("menu-item-5");
    assert_eq!(p.shared.editor_watch.load(Ordering::Relaxed), 1);
    assert_eq!(p.shared.selected.load(Ordering::Relaxed), 1);
}

#[test]
fn v1_menu_route_part_and_host_bus_use_existing_output_picker() {
    for (id, host_bus) in [(65536, false), (1, true)] {
        let (p, mut h) = v1_menu_fixture();
        v1_strip_menu(&mut h, id);
        h.press("menu-item-3");
        h.press(&format!("mt-pick-{id}-4"));
        let selection = p.selection.read().unwrap();
        if host_bus {
            assert_eq!(selection.bus(0).port, 4, "bus route maps to host 9/10");
        } else {
            assert_eq!(
                (selection.parts[0].output, selection.parts[0].output_manual),
                (4, true)
            );
        }
    }
}

#[test]
fn v1_ram_and_disk_readouts_are_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let h = Harness::new(&p, 1180., 780.);
    shoot(&h.ui, 1180, 780, "settings-telemetry.png");
    for id in ["readout-ram", "readout-disk"] {
        assert!(
            h.ui.scene().unwrap().surface(id).is_some(),
            "v1 readout missing: {id}"
        );
    }
}

#[test]
fn v1_mixer_aux_and_host_bus_controls_are_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    {
        let mut selection = p.selection.write().unwrap();
        selection.parts = vec![crate::plugin::Part {
            path: "/synthetic/Piano.nki".into(),
            output: 1,
            aux: 0,
            ..Default::default()
        }];
        selection.order = vec![0];
    }
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("tab-mixer");
    h.idle(4);
    for id in ["mt-aux-65536", "mt-send-65536", "mt-strip-1", "mt-strip-2"] {
        assert!(
            h.ui.scene().unwrap().surface(id).is_some(),
            "v1 mixer control missing: {id}"
        );
    }
    h.press("mt-out-2");
    h.press("mt-pick-2-3");
    assert_eq!(
        p.selection.read().unwrap().bus(1).port,
        3,
        "bus remaps to host 7/8"
    );
    h.press("mt-aux-65536");
    h.press("menu-item-4");
    assert_eq!(p.selection.read().unwrap().parts[0].aux, 2);
    assert!(
        h.ui.scene().unwrap().surface("mt-strip-3").is_some(),
        "send destination has a strip"
    );
    let at = super::tests::center(&h.ui, "mt-name-2");
    for buttons in [
        Buttons::default().set(Button::Secondary, true),
        Buttons::default(),
    ] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(2);
    h.press("menu-item-0");
    assert!(h.ui.scene().unwrap().surface("mt-rename-2").is_some());
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::Char('a'),
            mods: Mods {
                ctrl: true,
                ..Default::default()
            },
        }],
        ..Default::default()
    });
    h.tick(Input {
        text: "Piano dry".into(),
        ..Default::default()
    });
    h.press("mt-rename-2");
    assert_eq!(p.selection.read().unwrap().bus(1).name, "Piano dry");
    p.selection.write().unwrap().bus_mut(1).gain = -9.;
    h.idle(2);
    let at = super::tests::center(&h.ui, "mt-name-2");
    for buttons in [
        Buttons::default().set(Button::Secondary, true),
        Buttons::default(),
    ] {
        h.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons,
                ..Default::default()
            },
            ..Default::default()
        });
    }
    h.idle(2);
    h.press("menu-item-1");
    let selection = p.selection.read().unwrap();
    let bus = selection.bus(1);
    assert_eq!(
        (bus.gain, bus.port, bus.name.as_str()),
        (0., 3, "Piano dry"),
        "reset preserves name and host routing"
    );
    let mut bytes = Vec::new();
    use moose::core::custom_state::{StateCursor, StateField};
    selection.write_field(&mut bytes);
    let restored = crate::plugin::Selection::read_field(&mut StateCursor::new(&bytes)).unwrap();
    assert!(restored == *selection);
    shoot(&h.ui, 1180, 780, "settings-routing.png");
}

#[test]
fn v1_sample_folder_creator_is_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("app-menu");
    assert!(
        h.ui.scene()
            .unwrap()
            .surfaces()
            .any(|s| s.text_value.as_deref() == Some("Create library from folder…")),
        "v1 library creator menu is missing"
    );
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
    let tick = |ui: &mut Ui,
                face: &ir::Interface,
                values: &mut ir_view::Values,
                host: &mut sampler_uvi::script::ScriptHost,
                input| {
        let before = values.clone();
        let root = ir_view::view(
            ui,
            face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            values,
        );
        ui.frame(root, Some(Size::new(200., 100.)), input, 1. / 60.)
            .unwrap();
        for (&id, &value) in values.iter() {
            if before.get(&id).copied().unwrap_or(0.) != value {
                host.set_control(id, value).unwrap();
            }
        }
    };
    tick(&mut ui, &face, &mut values, &mut host, Input::default());
    let at = Point::new(50., 20.);
    let pointer = |held| Input {
        pointer: PointerInput {
            pos: Some(at),
            buttons: if held {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    for _ in 0..5 {
        tick(&mut ui, &face, &mut values, &mut host, pointer(true));
    }
    assert_eq!(host.global_text("clicks"), "0");
    tick(&mut ui, &face, &mut values, &mut host, pointer(false));
    for _ in 0..3 {
        tick(&mut ui, &face, &mut values, &mut host, Input::default());
    }
    assert_eq!(host.global_text("clicks"), "1");
    ui.focus("ir-1");
    tick(
        &mut ui,
        &face,
        &mut values,
        &mut host,
        Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Default::default(),
            }],
            ..Default::default()
        },
    );
    for _ in 0..3 {
        tick(&mut ui, &face, &mut values, &mut host, Input::default());
    }
    assert_eq!(host.global_text("clicks"), "2");
    face.widgets[0].enabled = false;
    tick(&mut ui, &face, &mut values, &mut host, Input::default());
    assert!(ui.scene().unwrap().surface("ir-1").is_some());
}
#[test]
fn uvi_scene_culls_offscreen_controls_without_dropping_the_model() {
    use sampler_ui_ir::{self as ir, Interface, Kind, Page, PageRef, Rect, Widget};
    let mut face = Interface {
        source: ir::Source::FalconLua,
        ..Default::default()
    };
    face.pages.push(Page {
        size: ir::Size {
            width: 200,
            height: 100,
        },
        ..Default::default()
    });
    for i in 0..7000 {
        face.widgets.push(Widget::new(
            format!("w{i}"),
            PageRef(0),
            Rect::new(0, if i == 0 { 0 } else { 10000 }, 20, 20),
            Kind::Label,
        ));
    }
    let mut ui = theme::ui();
    let root = ir_view::view(
        &mut ui,
        &face,
        PageRef(0),
        &ir_view::Assets::default(),
        ir::Presentation::Bitmap,
        1.,
        &mut ir_view::Values::default(),
    );
    ui.frame(
        root,
        Some(Size::new(200., 100.)),
        Input::default(),
        1. / 60.,
    )
    .unwrap();
    assert_eq!(face.widgets.len(), 7000);
    assert!(
        ui.scene().unwrap().surface("ir-0").is_none(),
        "visible passive label must not regain a named hit target"
    );

    assert!(ui.scene().unwrap().surface("/ir-0").is_some());
    assert!(ui.scene().unwrap().surface("/ir-1").is_none());
}

/// Audit-only gesture probe: the same renderer and input loop as the editor.
fn audit_motion(face: &ir::Interface, target: usize, dx: f64, dy: f64) -> (f64, bool) {
    audit_motion_readback(face, target, dx, dy, false)
}

fn audit_motion_readback(
    face: &ir::Interface,
    target: usize,
    dx: f64,
    dy: f64,
    round_each_frame: bool,
) -> (f64, bool) {
    let assets = ir_view::Assets::default();
    let mut values = ir_view::Values::default();
    let ir::Binding::Control(control) = face.widgets[target].binding else {
        return (0., false);
    };
    let start = match &face.widgets[target].kind {
        ir::Kind::Knob { range, .. } | ir::Kind::Slider { range, .. } => {
            (range.min + range.max) / 2.
        }
        _ => 0.,
    };
    values.insert(control, start);
    let mut ui = settle(
        f64::from(face.pages[0].size.width),
        f64::from(ir_view::height(face, ir::PageRef(0))),
        |ui| {
            ir_view::view(
                ui,
                face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Vector,
                1.,
                &mut values,
            )
        },
    );
    let id = format!("ir-{target}");
    let Some(surface) = ui.scene().unwrap().surface(&id) else {
        if dx == 0. {
            println!(
                "AUDIT_MISS target={target} page={}",
                face.widgets[target].page.0
            );
        }
        return (0., false);
    };
    let at = Point::new(
        surface.frame.x + surface.frame.size.width / 2.,
        surface.frame.y + surface.frame.size.height / 2.,
    );
    let mut pressed = false;
    let steps = if round_each_frame { 30 } else { 1 };
    let events = [(at, false), (at, true)]
        .into_iter()
        .chain((1..=steps).map(|n| {
            (
                Point::new(
                    at.x + dx * f64::from(n) / f64::from(steps),
                    at.y + dy * f64::from(n) / f64::from(steps),
                ),
                true,
            )
        }))
        .chain([(Point::new(at.x + dx, at.y + dy), false)]);
    for (point, down) in events {
        for _ in 0..2 {
            let el = ir_view::view(
                &mut ui,
                face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Vector,
                1.,
                &mut values,
            );
            ui.frame(
                el,
                Some(Size::new(
                    f64::from(face.pages[0].size.width),
                    f64::from(ir_view::height(face, ir::PageRef(0))),
                )),
                Input {
                    pointer: PointerInput {
                        pos: Some(point),
                        buttons: if down {
                            Buttons::PRIMARY
                        } else {
                            Buttons::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                1. / 60.,
            )
            .unwrap();
            pressed |= ui.get(id.as_str()).held;
            if round_each_frame {
                values.values_mut().for_each(|value| *value = value.round());
            }
            if dx == 0. && point == at && down && !ui.get(id.as_str()).held {
                let winners: Vec<_> = ui
                    .scene()
                    .unwrap()
                    .surfaces()
                    .filter(|s| ui.get(s.key.as_str()).held)
                    .map(|s| s.key.to_string())
                    .collect();
                println!(
                    "AUDIT_OCCLUDED target={target} x={} y={} held={winners:?}",
                    at.x, at.y
                );
            }
        }
    }
    (*values.get(&control).unwrap() - start, pressed)
}

#[test]
fn widget_negative_mouse_behaviour() {
    let script = sampler_ksp::compile("on init\n declare ui_slider $s(0,1000000)\n set_control_par(get_ui_id($s),$CONTROL_PAR_MOUSE_BEHAVIOUR,-1000)\nend on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    assert_eq!(
        face.widgets[0].drag.unwrap().axis,
        ir::Orientation::Vertical
    );
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
    let script = sampler_ksp::compile(
        "on init\n declare ui_knob $k(0,2,1)\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let (free, captured) = audit_motion_readback(&face, 0, 0., -60., false);
    let (rounded, _) = audit_motion_readback(&face, 0, 0., -60., true);
    assert!(captured && free > 0.5);
    assert!(
        rounded >= 1.,
        "fractional drags must survive integer feedback: {rounded}"
    );
}

#[test]
fn widget_menu_passive_value_is_unchanged() {
    let script = sampler_ksp::compile("on init\n declare ui_menu $m\n add_menu_item($m,\"Ten\",10)\n add_menu_item($m,\"Forty\",40)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let ir::Binding::Control(control) = face.widgets[0].binding else {
        panic!("binding")
    };
    let mut values = ir_view::Values::from([(control, -1.)]);
    let assets = ir_view::Assets::default();
    settle(633., 100., |ui| {
        ir_view::view(
            ui,
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
        )
    });
    assert_eq!(values[&control], -1.);
}

#[test]
fn widget_disabled_knob_does_not_edit() {
    let script = sampler_ksp::compile(
        "on init\n declare ui_knob $k(0,100,1)\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    face.widgets[0].enabled = false;
    let (delta, captured) = audit_motion(&face, 0, 0., -30.);
    assert_eq!(delta, 0.);
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
            let name = if path.is_empty() {
                id.to_owned()
            } else {
                format!("{path}_{id}")
            };
            let prefix = match control["index"].as_i64().unwrap() {
                9 => '%',
                10 => '@',
                _ => '$',
            };
            out.insert(format!("{prefix}{name}"));
            names(&value["controls"], &name, out);
        }
    }
    let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
    let mut source = sampler_kontakt::read(&path).unwrap().instrument;
    let mut resources = sampler_kontakt::Resources::of(&path);
    for behavior in &source.behaviors {
        if let Some(name) = sampler_ksp::nckp::view_name(&behavior.source) {
            let bytes = resources
                .read(&format!("Resources/performance_view/{name}.nckp"))
                .unwrap();
            let raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let mut raw_names = BTreeSet::new();
            names(
                &raw["value"]["performanceView"]["controls"],
                "",
                &mut raw_names,
            );
            let (parsed, skipped) = sampler_ksp::nckp::parse(&bytes).unwrap();
            let parsed_names = parsed
                .controls
                .iter()
                .map(|c| c.name.clone())
                .collect::<BTreeSet<_>>();
            assert!(skipped.is_empty());
            assert_eq!(raw_names.len(), parsed_names.len());
            assert!(
                raw_names == parsed_names,
                "raw hierarchy and reader names disagree"
            );
            fn raw_knobs(raw: &serde_json::Value) -> usize {
                raw.as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| usize::from(c["index"] == 3) + raw_knobs(&c["value"]["controls"]))
                    .sum()
            }
            let knobs = raw_knobs(&raw["value"]["performanceView"]["controls"]);
            assert_eq!(
                knobs,
                parsed
                    .controls
                    .iter()
                    .filter(|c| c.kind == sampler_ksp::model::WidgetKind::Knob)
                    .count()
            );
            println!(
                "CONFLUX_NCKP raw={} parsed={} raw_knobs={knobs}",
                raw_names.len(),
                parsed_names.len()
            );
        }
    }
    source.retain_zones(|_| false);
    source.assets.clear();
    let loaded = sampler_kontakt::prepare(
        source,
        vec![],
        &sampler_kontakt::Options {
            library: Some(path),
            ..Default::default()
        },
    )
    .unwrap();
    let face = ir_view::resolved(
        loaded
            .interfaces
            .iter()
            .max_by_key(|f| f.widgets.len())
            .unwrap(),
    );
    assert_eq!(
        face.widgets.len(),
        378,
        "unresolved handles must not publish widgets"
    );
    let all_knobs = face
        .widgets
        .iter()
        .filter(|w| matches!(w.kind, ir::Kind::Knob { .. }))
        .count();
    let origins = face
        .widgets
        .iter()
        .enumerate()
        .filter(|(n, w)| {
            face.visible(ir::WidgetRef(*n))
                && matches!(w.kind, ir::Kind::Knob { .. } | ir::Kind::Slider { .. })
                && face.page_rect(ir::WidgetRef(*n)).x == 0
                && face.page_rect(ir::WidgetRef(*n)).y == 0
        })
        .count();
    println!(
        "CONFLUX_PLACEMENT widgets={} all_knobs={all_knobs} visible_knobs_at_origin={origins}",
        face.widgets.len()
    );
    assert_eq!(origins, 0);
    let mut count = [0; 3];
    for (n, _) in face.widgets.iter().enumerate().filter(|(n, w)| {
        face.visible(ir::WidgetRef(*n))
            && matches!(w.kind, ir::Kind::Knob { .. } | ir::Kind::Slider { .. })
    }) {
        let (delta, captured) = audit_motion(&face, n, 0., -100.);
        count[0] += 1;
        count[1] += usize::from(captured);
        count[2] += usize::from(delta > 0.);
    }
    println!(
        "CONFLUX_CAPTURE visible={} captured={} increase={}",
        count[0], count[1], count[2]
    );
    assert_eq!(count[0], count[1]);
    assert_eq!(count[0], count[2]);
}

#[test]
fn widget_menu_selects_semantic_value_and_value_edit_accepts_typing() {
    let script = sampler_ksp::compile("on init\n declare ui_menu $m\n add_menu_item($m,\"first\",7)\n add_menu_item($m,\"second\",23)\n declare ui_value_edit $v(0,100,10)\n move_control_px($v,100,30)\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let ir::Binding::Control(menu) = face.widgets[0].binding else {
        panic!()
    };
    let ir::Binding::Control(value) = face.widgets[1].binding else {
        panic!()
    };
    let mut values = ir_view::Values::from([(menu, 99.), (value, 30.)]);
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui: &mut Ui,
                values: &mut ir_view::Values,
                state: &mut ir_view::InputState,
                input: Input| {
        let el = ir_view::view_state(
            ui,
            "probe",
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            values,
            state,
        );
        ui.frame(el, Some(Size::new(633., 300.)), input, 1. / 60.)
            .unwrap();
    };
    let key = |key| Input {
        keys: vec![KeyPress {
            key,
            mods: Mods::default(),
        }],
        ..Default::default()
    };
    for _ in 0..4 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    assert_eq!(
        values[&menu], 99.,
        "passive paint preserves unknown semantic values"
    );
    ui.focus("probe-ir-0");
    tick(&mut ui, &mut values, &mut state, key(Key::Enter));
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    assert!(ui.scene().unwrap().surface("probe-ir-0-popup").is_some());
    ui.focus("probe-ir-0-item-1");
    tick(&mut ui, &mut values, &mut state, key(Key::Enter));
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    assert_eq!(values[&menu], 23.);
    ui.focus("probe-ir-1");
    tick(&mut ui, &mut values, &mut state, key(Key::Enter));
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    assert!(ui.scene().unwrap().surface("probe-ir-1-type").is_some());
    tick(
        &mut ui,
        &mut values,
        &mut state,
        Input {
            keys: vec![KeyPress {
                key: Key::Char('a'),
                mods: Mods {
                    ctrl: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
    );
    tick(
        &mut ui,
        &mut values,
        &mut state,
        Input {
            text: "3.7".into(),
            ..Default::default()
        },
    );
    tick(&mut ui, &mut values, &mut state, key(Key::Enter));
    for _ in 0..3 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    assert_eq!(
        values[&value], 37.,
        "typed display units are converted to authored units"
    );
    assert!(
        state
            .edits
            .iter()
            .any(|e| e.widget == ir::WidgetRef(0) && e.value == ir::Value::Integer(23))
    );
    assert!(
        state
            .edits
            .iter()
            .any(|e| e.widget == ir::WidgetRef(1) && e.value == ir::Value::Integer(37))
    );
}

#[test]
fn widget_generated_menu_popup_anchors_to_scene() {
    let script = sampler_ksp::compile(
        "on init\n declare ui_menu $m\n add_menu_item($m,\"first\",7)\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui: &mut Ui,
                values: &mut ir_view::Values,
                state: &mut ir_view::InputState,
                input: Input| {
        let widget = ir_view::widget_state(
            ui,
            "generated",
            &face,
            ir::WidgetRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            values,
            state,
            130.,
            30.,
        )
        .at(300., 100.);
        let mut layers = vec![widget];
        if let Some(popup) =
            ir_view::menu_popup(ui, "generated", &face, 1., values, state, 500., 250.)
        {
            layers.push(popup);
        }
        let generated = stack(layers)
            .w(500)
            .h(250)
            .id("generated-ir-view")
            .at(40., 30.);
        ui.frame(
            stack![generated],
            Some(Size::new(633., 400.)),
            input,
            1. / 60.,
        )
        .unwrap();
    };
    for _ in 0..4 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    ui.focus("generated-ir-0");
    tick(
        &mut ui,
        &mut values,
        &mut state,
        Input {
            keys: vec![KeyPress {
                key: Key::Enter,
                mods: Mods::default(),
            }],
            ..Default::default()
        },
    );
    for _ in 0..4 {
        tick(&mut ui, &mut values, &mut state, Input::default());
    }
    let scene = ui.scene().unwrap();
    let root = scene.surface("generated-ir-view").unwrap().frame;
    let popup = scene.surface("generated-ir-0-popup").unwrap().frame;
    assert_eq!((popup.x - root.x, popup.y - root.y), (300., 130.));
}

#[test]
fn widget_keyboard_uses_authored_step() {
    let script = sampler_ksp::compile(
        "on init\n declare ui_knob $k(0,1000,1)\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let ir::Kind::Knob { range, .. } = &mut face.widgets[0].kind else {
        panic!()
    };
    range.step = Some(7.);
    let ir::Binding::Control(control) = face.widgets[0].binding else {
        panic!()
    };
    let mut values = ir_view::Values::from([(control, 140.)]);
    let assets = ir_view::Assets::default();
    let mut ui = settle(633., 100., |ui| {
        ir_view::view(
            ui,
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
        )
    });
    ui.focus("ir-0");
    for input in [
        Input {
            keys: vec![KeyPress {
                key: Key::Up,
                mods: Mods::default(),
            }],
            ..Default::default()
        },
        Input::default(),
        Input::default(),
    ] {
        let el = ir_view::view(
            &mut ui,
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
        );
        ui.frame(el, Some(Size::new(633., 100.)), input, 1. / 60.)
            .unwrap();
    }
    assert_eq!(values[&control], 147.);
}

#[test]
fn widget_ids_keep_two_instances_focus_and_capture_separate() {
    let script = sampler_ksp::compile(
        "on init\n declare ui_knob $k(0,1000,1)\nend on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let ir::Binding::Control(control) = face.widgets[0].binding else {
        panic!()
    };
    let mut a = ir_view::Values::from([(control, 500.)]);
    let mut b = a.clone();
    let mut sa = ir_view::InputState::default();
    let mut sb = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut ui = theme::ui();
    let tick = |ui: &mut Ui,
                a: &mut ir_view::Values,
                b: &mut ir_view::Values,
                sa: &mut ir_view::InputState,
                sb: &mut ir_view::InputState,
                input: Input| {
        let left = ir_view::view_state(
            ui,
            "part-a",
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            a,
            sa,
        );
        let right = ir_view::view_state(
            ui,
            "part-b",
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            b,
            sb,
        );
        ui.frame(
            row![left, right],
            Some(Size::new(1266., 100.)),
            input,
            1. / 60.,
        )
        .unwrap();
    };
    for _ in 0..4 {
        tick(&mut ui, &mut a, &mut b, &mut sa, &mut sb, Input::default());
    }
    let surface = ui.scene().unwrap().surface("part-a-ir-0").unwrap();
    let at = Point::new(
        surface.frame.x + surface.frame.size.width / 2.,
        surface.frame.y + surface.frame.size.height / 2.,
    );
    for (pos, down) in [
        (at, false),
        (at, true),
        (Point::new(at.x, at.y - 30.), true),
        (Point::new(at.x, at.y - 30.), false),
    ] {
        for _ in 0..2 {
            tick(
                &mut ui,
                &mut a,
                &mut b,
                &mut sa,
                &mut sb,
                Input {
                    pointer: PointerInput {
                        pos: Some(pos),
                        buttons: if down {
                            Buttons::PRIMARY
                        } else {
                            Buttons::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        }
    }
    assert!(a[&control] > 500.);
    assert_eq!(b[&control], 500.);
    let was = a[&control];
    ui.focus("part-b-ir-0");
    tick(
        &mut ui,
        &mut a,
        &mut b,
        &mut sa,
        &mut sb,
        Input {
            keys: vec![KeyPress {
                key: Key::Up,
                mods: Mods::default(),
            }],
            ..Default::default()
        },
    );
    for _ in 0..3 {
        tick(&mut ui, &mut a, &mut b, &mut sa, &mut sb, Input::default());
    }
    assert_eq!(a[&control], was);
    assert_eq!(b[&control], 501.);
}

/// Drives the real renderer, admits its edits, then paints native readback.
struct NativeGesture<'a> {
    face: ir::Interface,
    script_ui: &'a mut crate::sound::ScriptUi,
    runtime: &'a mut sampler_core::Runtime,
    ui: Ui,
    values: ir_view::Values,
    state: ir_view::InputState,
    assets: ir_view::Assets,
}
impl<'a> NativeGesture<'a> {
    fn id(&self, n: usize) -> sampler_core::ControlId {
        let ir::Source::Ksp { slot } = self.face.source else {
            panic!("not KSP")
        };
        self.runtime
            .widget_id(
                self.runtime.active_plan(),
                slot,
                self.face.widgets[n].source_id.unwrap(),
            )
            .unwrap()
    }
    fn read(&self, n: usize) -> sampler_core::WidgetValue {
        self.runtime
            .widget_value(self.runtime.active_plan(), self.id(n), 0)
            .unwrap()
    }
    fn sync(&mut self) {
        for (n, w) in self.face.widgets.iter().enumerate() {
            let Some(ui_id) = w.source_id else { continue };
            let ir::Source::Ksp { slot } = self.face.source else {
                unreachable!()
            };
            let plan = self.runtime.active_plan();
            let Ok(id) = self.runtime.widget_id(plan, slot, ui_id) else {
                continue;
            };
            let Ok(value) = self.runtime.widget_value(plan, id, 0) else {
                continue;
            };
            let value = match value {
                sampler_core::WidgetValue::Integer(v) => ir::Value::Integer(v as i32),
                sampler_core::WidgetValue::Real(v) => ir::Value::Real(v),
                sampler_core::WidgetValue::Text(v) => ir::Value::Text(v.as_str().to_owned()),
                sampler_core::WidgetValue::DropPath { .. } => {
                    panic!("drop payload is not a readback value")
                }
            };
            if let ir::Binding::Control(c) = w.binding {
                let scalar = match value {
                    ir::Value::Integer(v) => f64::from(v),
                    ir::Value::Real(v) => v,
                    _ => continue,
                };
                self.values.insert(c, scalar);
            } else {
                self.state.values.insert(ir::WidgetRef(n), value);
            }
        }
    }
    fn tick(&mut self, input: Input) {
        let el = ir_view::view_state(
            &mut self.ui,
            "native-probe",
            &self.face,
            ir::PageRef(0),
            &self.assets,
            ir::Presentation::Vector,
            1.,
            &mut self.values,
            &mut self.state,
        );
        self.ui
            .frame(
                el,
                Some(Size::new(
                    f64::from(self.face.pages[0].size.width),
                    f64::from(ir_view::height(&self.face, ir::PageRef(0))),
                )),
                input,
                1. / 60.,
            )
            .unwrap();
        let pending = std::mem::take(&mut self.state.edits);
        let plan = self.runtime.active_plan();
        for edit in pending {
            let id = self.id(edit.widget.0);
            let value = match edit.value {
                ir::Value::Integer(v) => sampler_core::WidgetValue::Integer(i64::from(v)),
                ir::Value::Real(v) => sampler_core::WidgetValue::Real(v),
                ir::Value::Text(v) => {
                    sampler_core::WidgetValue::Text(sampler_core::Text::try_new(&v).unwrap())
                }
                _ => panic!("unexpected array edit"),
            };
            let interaction = sampler_core::WidgetInteraction {
                index: edit.index,
                cursor: edit.cursor,
                event: edit.event,
                mouse_over: edit.mouse_over,
                modifiers: u8::from(edit.mods.shift)
                    | u8::from(edit.mods.ctrl) << 1
                    | u8::from(edit.mods.alt) << 2,
                ..Default::default()
            };
            let context = sampler_core::ControlContext {
                performance: self.runtime.performance(0).unwrap(),
                origin: sampler_core::ChannelAddress {
                    protocol: sampler_core::Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                },
                channels: 1,
            };
            self.runtime
                .invoke_widget(
                    context,
                    plan,
                    None,
                    &[sampler_core::WidgetEdit {
                        id,
                        index: edit.index,
                        value,
                        interaction,
                    }],
                )
                .unwrap();
        }
        self.runtime.render(&mut [[0.; 2]; 16]).unwrap();
        let mut changed = false;
        self.runtime.drain_effects(|effect| {
            if let Some(instance) = effect.instance {
                changed |= self.script_ui.apply(usize::from(instance.0), effect);
            }
            true
        });
        if changed {
            if let Some(face) = self
                .script_ui
                .interfaces()
                .into_iter()
                .find(|f| f.source == self.face.source)
            {
                self.face = ir_view::resolved(&face);
            }
        }
        self.sync();
    }
    fn settle(&mut self) {
        for _ in 0..4 {
            self.tick(Input::default());
        }
    }
    fn center(&self, n: usize) -> Point {
        let s = self
            .ui
            .scene()
            .unwrap()
            .surface(&format!("native-probe-ir-{n}"))
            .unwrap();
        Point::new(
            s.frame.x + s.frame.size.width / 2.,
            s.frame.y + s.frame.size.height / 2.,
        )
    }
    fn hit_point(&mut self, n: usize) -> Option<Point> {
        let id = format!("native-probe-ir-{n}");
        let frame = self.ui.scene().unwrap().surface(&id).unwrap().frame;
        for y in [0.5, 0.1, 0.9] {
            for x in [0.5, 0.1, 0.9] {
                let p = Point::new(
                    frame.x + frame.size.width * x,
                    frame.y + frame.size.height * y,
                );
                self.pointer(p, false);
                if self.ui.get(id.as_str()).hovered {
                    return Some(p);
                }
            }
        }
        println!(
            "CONFLUX_OCCLUDED widget={n} name={} parent={:?} enabled={} z={} hide={:?} frame={frame:?}",
            self.face.widgets[n].name,
            self.face.widgets[n].parent,
            self.face.widgets[n].enabled,
            self.face.widgets[n].z,
            self.face.widgets[n].hide
        );
        None
    }
    fn pointer(&mut self, p: Point, down: bool) {
        for _ in 0..2 {
            self.tick(Input {
                pointer: PointerInput {
                    pos: Some(p),
                    buttons: if down {
                        Buttons::PRIMARY
                    } else {
                        Buttons::default()
                    },
                    ..Default::default()
                },
                ..Default::default()
            });
        }
    }
    fn key(&mut self, key: Key, mods: Mods) {
        self.tick(Input {
            keys: vec![KeyPress { key, mods }],
            ..Default::default()
        });
        self.tick(Input::default());
    }
    fn restore(&mut self, n: usize, value: sampler_core::WidgetValue) {
        let value = match value {
            sampler_core::WidgetValue::Integer(v) => ir::Value::Integer(v as i32),
            sampler_core::WidgetValue::Real(v) => ir::Value::Real(v),
            sampler_core::WidgetValue::Text(v) => ir::Value::Text(v.as_str().to_owned()),
            sampler_core::WidgetValue::DropPath { .. } => {
                panic!("drop payload cannot be restored as state")
            }
        };
        self.state.edits.push(ir_view::Edit {
            widget: ir::WidgetRef(n),
            index: 0,
            value,
            mods: Mods::default(),
            mouse_over: false,
            cursor: 0,
            event: 0,
        });
        self.settle();
    }
    fn changed_and_retained(&mut self, n: usize, before: sampler_core::WidgetValue, family: &str) {
        let after = self.read(n);
        assert_ne!(
            before, after,
            "{family} gesture did not change native widget {n}"
        );
        self.settle();
        assert_eq!(
            self.read(n),
            after,
            "{family} native readback was not retained for widget {n}"
        );
        println!("CONFLUX_GESTURE_PASS family={family} widget={n}");
    }
}

#[test]
#[ignore = "real-library gesture gate; set KONTRA_AUDIT_WIDGET_PATCH"]
fn widget_conflux_native_gestures_and_readback() {
    let Some(path) = std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").map(std::path::PathBuf::from)
    else {
        return;
    };
    if !path.exists() {
        return;
    }
    let mut source = sampler_kontakt::read(&path).unwrap().instrument;
    source.retain_zones(|_| false);
    source.assets.clear();
    let loaded = sampler_kontakt::prepare(
        source,
        vec![],
        &sampler_kontakt::Options {
            library: Some(path),
            ..Default::default()
        },
    )
    .unwrap();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 16);
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    let mut script_ui = crate::sound::ScriptUi {
        views: loaded.scripts,
        resources: loaded.resources,
        ..Default::default()
    };
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut scalar_unresolved = 0;
    let mut main_drag = 0;
    for authored in loaded.interfaces {
        let face = ir_view::resolved(&authored);
        if face.source == (ir::Source::Ksp { slot: 2 }) {
            assert_eq!(face.widgets.len(), 378);
            assert_eq!(
                face.widgets
                    .iter()
                    .enumerate()
                    .filter(|(n, w)| face.visible(ir::WidgetRef(*n))
                        && matches!(w.kind, ir::Kind::Knob { .. }))
                    .count(),
                45
            );
        }
        let mut h = NativeGesture {
            face: face.clone(),
            script_ui: &mut script_ui,
            runtime: &mut runtime,
            ui: theme::ui(),
            values: Default::default(),
            state: Default::default(),
            assets: Default::default(),
        };
        h.sync();
        h.settle();
        for (n, w) in face
            .widgets
            .iter()
            .enumerate()
            .filter(|(n, _)| face.visible(ir::WidgetRef(*n)))
        {
            if !h.face.visible(ir::WidgetRef(n)) {
                continue;
            }
            let original = if matches!(
                w.kind,
                ir::Kind::Knob { .. }
                    | ir::Kind::Slider { .. }
                    | ir::Kind::Button { .. }
                    | ir::Kind::Switch
                    | ir::Kind::Menu { .. }
                    | ir::Kind::ValueEdit { .. }
                    | ir::Kind::TextEdit
            ) {
                h.read(n)
            } else {
                continue;
            };
            if matches!(w.kind, ir::Kind::TextEdit) {
                scalar_unresolved += 1;
                println!(
                    "CONFLUX_TYPED_BINDING source={:?} ui_id={:?} name={} binding={:?} typed_readback={}",
                    face.source,
                    w.source_id,
                    w.name,
                    w.binding,
                    matches!(h.read(n), sampler_core::WidgetValue::Text(_))
                );
            }
            match &w.kind {
                ir::Kind::Knob { range, .. } | ir::Kind::Slider { range, .. } => {
                    let before = h.read(n);
                    let scalar = match before {
                        sampler_core::WidgetValue::Integer(v) => v as f64,
                        sampler_core::WidgetValue::Real(v) => v,
                        _ => panic!(),
                    };
                    let direction = if scalar >= range.max { -1. } else { 1. };
                    let p = h.center(n);
                    let horizontal = w
                        .drag
                        .is_some_and(|d| d.axis == ir::Orientation::Horizontal);
                    let q = Point::new(
                        p.x + if horizontal { 100. * direction } else { 0. },
                        p.y - if horizontal { 0. } else { 100. * direction },
                    );
                    h.pointer(p, false);
                    h.pointer(p, true);
                    h.pointer(q, true);
                    h.pointer(q, false);
                    h.changed_and_retained(n, before, "drag");
                    let after = match h.read(n) {
                        sampler_core::WidgetValue::Integer(v) => v as f64,
                        sampler_core::WidgetValue::Real(v) => v,
                        _ => panic!("scalar drag needs numeric readback"),
                    };
                    assert!(
                        (after - scalar) * direction > 0.,
                        "native drag moved against authored direction"
                    );
                    *counts.entry("drag").or_default() += 1;
                    if face.source == (ir::Source::Ksp { slot: 2 }) {
                        main_drag += 1;
                    }
                    let before = h.read(n);
                    let scalar = match before {
                        sampler_core::WidgetValue::Integer(v) => v as f64,
                        sampler_core::WidgetValue::Real(v) => v,
                        _ => panic!(),
                    };
                    h.pointer(p, false);
                    h.tick(Input {
                        pointer: PointerInput {
                            pos: Some(p),
                            ..Default::default()
                        },
                        wheel: Vec2::new(0., if scalar >= range.max { 120. } else { -120. }),
                        ..Default::default()
                    });
                    h.settle();
                    h.changed_and_retained(n, before, "wheel");
                    let after = match h.read(n) {
                        sampler_core::WidgetValue::Integer(v) => v as f64,
                        sampler_core::WidgetValue::Real(v) => v,
                        _ => panic!("scalar wheel needs numeric readback"),
                    };
                    assert!(
                        (after - scalar) * if scalar >= range.max { -1. } else { 1. } > 0.,
                        "native wheel moved against authored direction"
                    );
                    *counts.entry("wheel").or_default() += 1;
                }
                ir::Kind::Button { .. } | ir::Kind::Switch => {
                    let before = h.read(n);
                    let p = h
                        .hit_point(n)
                        .expect("button/switch must have an exposed hit region");
                    h.pointer(p, false);
                    h.pointer(p, true);
                    if matches!(w.kind, ir::Kind::Button { momentary: true }) {
                        let held = h.read(n);
                        assert_ne!(held, before, "momentary press did not change native value");
                        h.pointer(p, true);
                        assert_eq!(h.read(n), held, "held state was not retained");
                        h.pointer(p, false);
                    } else {
                        h.pointer(p, false);
                        h.changed_and_retained(n, before, "click");
                    }
                    *counts.entry("button/switch click").or_default() += 1;
                }
                ir::Kind::Menu { items } => {
                    let before = h.read(n);
                    let current = match before {
                        sampler_core::WidgetValue::Integer(v) => v,
                        _ => panic!(),
                    };
                    let Some((at, _)) = items
                        .iter()
                        .enumerate()
                        .find(|(_, i)| i.visible && i64::from(i.value) != current)
                    else {
                        continue;
                    };
                    let Some(p) = h.hit_point(n) else { continue };
                    h.pointer(p, false);
                    h.pointer(p, true);
                    println!(
                        "MENU_CAPTURE widget={n} enabled={} held={} winners={:?}",
                        w.enabled,
                        h.ui.get(format!("native-probe-ir-{n}")).held,
                        h.ui.scene()
                            .unwrap()
                            .surfaces()
                            .filter(|s| h.ui.get(s.key.as_str()).held)
                            .map(|s| s.key.to_string())
                            .collect::<Vec<_>>()
                    );
                    h.pointer(p, false);
                    h.settle();
                    let s =
                        h.ui.scene()
                            .unwrap()
                            .surface(&format!("native-probe-ir-{n}-item-{at}"))
                            .unwrap_or_else(|| {
                                panic!(
                                    "menu {n} enabled={} popup missing after pointer click",
                                    w.enabled
                                )
                            });
                    let p = Point::new(
                        s.frame.x + s.frame.size.width / 2.,
                        s.frame.y + s.frame.size.height / 2.,
                    );
                    h.pointer(p, false);
                    h.pointer(p, true);
                    h.pointer(p, false);
                    h.settle();
                    h.changed_and_retained(n, before, "menu click");
                    *counts.entry("menu click").or_default() += 1;
                }
                ir::Kind::ValueEdit { range, display, .. } => {
                    let before = h.read(n);
                    let current = match before {
                        sampler_core::WidgetValue::Integer(v) => v as f64,
                        _ => panic!(),
                    };
                    let step = range.step.unwrap_or(1.);
                    let next = if current + step <= range.max {
                        current + step
                    } else {
                        current - step
                    };
                    h.ui.focus(format!("native-probe-ir-{n}"));
                    h.key(Key::Enter, Mods::default());
                    h.settle();
                    h.key(
                        Key::Char('a'),
                        Mods {
                            ctrl: true,
                            ..Default::default()
                        },
                    );
                    h.tick(Input {
                        text: format!(
                            "{}",
                            next / if display.ratio == 0. {
                                1.
                            } else {
                                display.ratio
                            }
                        ),
                        ..Default::default()
                    });
                    h.key(Key::Enter, Mods::default());
                    h.settle();
                    h.changed_and_retained(n, before, "value typing");
                    *counts.entry("value typing").or_default() += 1;
                }
                ir::Kind::TextEdit => {
                    let before = h.read(n);
                    h.ui.focus(format!("native-probe-ir-{n}"));
                    h.key(
                        Key::Char('a'),
                        Mods {
                            ctrl: true,
                            ..Default::default()
                        },
                    );
                    h.tick(Input {
                        text: "W2 gesture probe".into(),
                        ..Default::default()
                    });
                    h.key(Key::Enter, Mods::default());
                    h.settle();
                    h.changed_and_retained(n, before, "text typing");
                    *counts.entry("text typing").or_default() += 1;
                }
                _ => continue,
            }
            h.restore(n, original);
        }
    }
    println!("CONFLUX_NATIVE_GESTURES {counts:?} scalar_unresolved_typed={scalar_unresolved}");
    assert_eq!(main_drag, 45);
    assert_eq!(counts.get("drag").copied().unwrap_or(0), 48);
    assert_eq!(counts.get("wheel").copied().unwrap_or(0), 48);
    assert_eq!(scalar_unresolved, 6);
    for family in [
        "wheel",
        "button/switch click",
        "menu click",
        "value typing",
        "text typing",
    ] {
        assert!(
            counts.get(family).copied().unwrap_or(0) > 0,
            "missing visible family gesture: {family}"
        );
    }
}

#[test]
fn widget_text_draft_preserves_focus_and_refreshes_native_readback() {
    let script = sampler_ksp::compile(
        "on init declare ui_text_edit @t end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let mut ui = theme::ui();
    let assets = ir_view::Assets::default();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    state
        .values
        .insert(ir::WidgetRef(0), ir::Value::Text("initial".into()));
    let tick = |ui: &mut Ui,
                state: &mut ir_view::InputState,
                values: &mut ir_view::Values,
                input: Input| {
        let el = ir_view::view_state(
            ui,
            "text",
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            values,
            state,
        );
        ui.frame(el, Some(Size::new(633., 100.)), input, 1. / 60.)
            .unwrap();
    };
    for _ in 0..4 {
        tick(&mut ui, &mut state, &mut values, Input::default());
    }
    ui.focus("text-ir-0");
    tick(
        &mut ui,
        &mut state,
        &mut values,
        Input {
            keys: vec![KeyPress {
                key: Key::Char('a'),
                mods: Mods {
                    ctrl: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
    );
    tick(
        &mut ui,
        &mut state,
        &mut values,
        Input {
            text: "typing".into(),
            ..Default::default()
        },
    );
    state
        .values
        .insert(ir::WidgetRef(0), ir::Value::Text("callback".into()));
    tick(&mut ui, &mut state, &mut values, Input::default());
    let enter = || Input {
        keys: vec![KeyPress {
            key: Key::Enter,
            mods: Mods::default(),
        }],
        ..Default::default()
    };
    tick(&mut ui, &mut state, &mut values, enter());
    for _ in 0..3 {
        tick(&mut ui, &mut state, &mut values, Input::default());
    }
    assert_eq!(
        state.edits.last().unwrap().value,
        ir::Value::Text("typing".into()),
        "readback must preserve a focused draft"
    );
    state
        .values
        .insert(ir::WidgetRef(0), ir::Value::Text("callback".into()));
    for _ in 0..3 {
        tick(&mut ui, &mut state, &mut values, Input::default());
    }
    ui.focus("text-ir-0");
    tick(&mut ui, &mut state, &mut values, enter());
    for _ in 0..3 {
        tick(&mut ui, &mut state, &mut values, Input::default());
    }
    assert_eq!(
        state.edits.last().unwrap().value,
        ir::Value::Text("callback".into()),
        "unfocused draft must refresh after callback or rejected admission"
    );
}

#[test]
fn widget_file_drop_uses_hit_order_namespace_and_atomic_path_limits() {
    let script = sampler_ksp::compile(
        "on init declare ui_mouse_area $drop end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    face.widgets[0].rect = ir::Rect::new(0, 0, 100, 100);
    let assets = ir_view::Assets::default();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let mut ui = theme::ui();
    let mut frame = |ui: &mut Ui, face: &ir::Interface| {
        let el = ir_view::view_state(
            ui,
            "part-a",
            face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
            &mut state,
        );
        ui.frame(el, Some(Size::new(400., 200.)), Input::default(), 1. / 60.)
            .unwrap();
    };
    frame(&mut ui, &face);
    let at = Point::new(50., 50.);
    let paths = vec![
        "/tmp/a.WAV".into(),
        "/tmp/a.mid".into(),
        "/tmp/a.nka".into(),
    ];
    let (widget, edits) = ir_view::file_drop(&ui, "part-a", &face, at, &paths, true)
        .expect("MouseArea takes OS drop");
    assert_eq!(widget, ir::WidgetRef(0));
    assert_eq!(edits.len(), 3);
    for (index, edit) in edits.iter().enumerate() {
        assert_eq!(edit.index, index as u32);
        assert_eq!(edit.event, 5);
        assert!(edit.mouse_over);
    }
    assert!(
        matches!(&edits[0].value,ir::Value::DropPath {kind:ir::DropKind::Audio,path} if path=="/tmp/a.WAV")
    );
    assert!(matches!(
        edits[1].value,
        ir::Value::DropPath {
            kind: ir::DropKind::Midi,
            ..
        }
    ));
    assert!(matches!(
        edits[2].value,
        ir::Value::DropPath {
            kind: ir::DropKind::Array,
            ..
        }
    ));
    assert_eq!(
        ir_view::file_drop(&ui, "part-a", &face, at, &paths, false)
            .unwrap()
            .1[0]
            .event,
        4
    );
    assert!(ir_view::file_drop(&ui, "part-b", &face, at, &paths, true).is_none());
    assert!(
        ir_view::file_drop(
            &ui,
            "part-a",
            &face,
            at,
            &vec!["/tmp/a.wav".into(); 33],
            true
        )
        .is_none()
    );
    assert_eq!(
        ir_view::file_drop_target(&ui, "part-a", &face, at),
        Some(ir::WidgetRef(0)),
        "invalid batch still targets a widget and must veto rack fallback"
    );
    assert!(
        ir_view::file_drop(&ui, "part-a", &face, at, &["/tmp/unknown.exe".into()], true).is_none()
    );
    let long = std::path::PathBuf::from(format!(
        "/tmp/{}.wav",
        "x".repeat(sampler_core::TEXT_CAPACITY)
    ));
    assert!(ir_view::file_drop(&ui, "part-a", &face, at, &[long], true).is_none());
    let mut passive = ir::Widget::new(
        "label",
        ir::PageRef(0),
        ir::Rect::new(0, 0, 100, 100),
        ir::Kind::Label,
    );
    passive.z = 10;
    face.widgets.push(passive);
    frame(&mut ui, &face);
    assert!(ir_view::file_drop(&ui, "part-a", &face, at, &paths, true).is_some());
    face.widgets[1].kind = ir::Kind::Button { momentary: false };
    frame(&mut ui, &face);
    assert!(ir_view::file_drop(&ui, "part-a", &face, at, &paths, true).is_none());
    face.widgets.pop();
    face.widgets[0].enabled = false;
    frame(&mut ui, &face);
    assert!(ir_view::file_drop(&ui, "part-a", &face, at, &paths, true).is_none());
}

#[test]
fn widget_xy_modes_preserve_active_cursor_and_relative_axis_scaling() {
    fn tick(
        ui: &mut Ui,
        face: &ir::Interface,
        state: &mut ir_view::InputState,
        p: Point,
        down: bool,
    ) {
        for _ in 0..2 {
            let el = ir_view::view_state(
                ui,
                "xy-test",
                face,
                ir::PageRef(0),
                &ir_view::Assets::default(),
                ir::Presentation::Vector,
                1.,
                &mut ir_view::Values::default(),
                state,
            );
            ui.frame(
                el,
                Some(Size::new(400., 200.)),
                Input {
                    pointer: PointerInput {
                        pos: Some(p),
                        buttons: if down {
                            Buttons::PRIMARY
                        } else {
                            Buttons::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                1. / 60.,
            )
            .unwrap();
        }
    }
    let script = sampler_ksp::compile(
        "on init declare ui_xy ?pad[4] end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    let values = vec![0.25, 0.25, 0.75, 0.75];
    face.widgets[0].rect = ir::Rect::new(0, 0, 100, 100);
    face.widgets[0].active_index = Some(2);
    face.widgets[0].value = Some(ir::Value::Reals(values.clone()));
    face.widgets[0].kind = ir::Kind::Xy {
        cursors: 2,
        sensitivity: [Some(500), Some(500)],
        mouse_mode: Some(0),
    };
    let mut ui = theme::ui();
    let mut state = ir_view::InputState::default();
    tick(&mut ui, &face, &mut state, Point::new(25., 75.), false);
    tick(&mut ui, &face, &mut state, Point::new(25., 75.), true);
    assert!(
        state.edits.is_empty(),
        "mode0 ignores inactive cursor clicks"
    );
    tick(&mut ui, &face, &mut state, Point::new(25., 75.), false);
    tick(&mut ui, &face, &mut state, Point::new(75., 25.), true);
    assert!(state.edits.iter().all(|e| e.cursor == 2));
    state.edits.clear();
    tick(&mut ui, &face, &mut state, Point::new(85., 5.), true);
    assert!(
        matches!(&state.values[&ir::WidgetRef(0)],ir::Value::Reals(v) if (v[2]-0.8).abs()<1e-9 && (v[3]-0.85).abs()<1e-9 && v[..2]==values[..2])
    );
    assert_eq!(
        state.edits.iter().map(|e| e.index).collect::<Vec<_>>(),
        [2, 3]
    );
    tick(&mut ui, &face, &mut state, Point::new(85., 5.), false);
    face.widgets[0].kind = ir::Kind::Xy {
        cursors: 2,
        sensitivity: [Some(500), Some(500)],
        mouse_mode: Some(1),
    };
    state = Default::default();
    ui = theme::ui();
    tick(&mut ui, &face, &mut state, Point::new(10., 90.), false);
    tick(&mut ui, &face, &mut state, Point::new(10., 90.), true);
    assert!(
        matches!(&state.values[&ir::WidgetRef(0)],ir::Value::Reals(v) if *v==values),
        "mode1 press emits callback without a jump"
    );
    assert_eq!(state.edits.len(), 2);
    state.edits.clear();
    tick(&mut ui, &face, &mut state, Point::new(20., 70.), true);
    assert!(
        matches!(&state.values[&ir::WidgetRef(0)],ir::Value::Reals(v) if (v[2]-0.8).abs()<1e-9 && (v[3]-0.85).abs()<1e-9)
    );
    face.widgets[0].kind = ir::Kind::Xy {
        cursors: 1,
        sensitivity: [Some(1), Some(1)],
        mouse_mode: Some(2),
    };
    face.widgets[0].active_index = None;
    face.widgets[0].value = Some(ir::Value::Reals(vec![0.25, 0.25]));
    state = Default::default();
    ui = theme::ui();
    tick(&mut ui, &face, &mut state, Point::new(10., 20.), false);
    tick(&mut ui, &face, &mut state, Point::new(10., 20.), true);
    assert!(
        matches!(&state.values[&ir::WidgetRef(0)],ir::Value::Reals(v) if *v==vec![0.1,0.8]),
        "mode2 absolute position ignores sensitivity"
    );
}

#[test]
fn widget_table_fast_stroke_edits_crossed_columns_as_one_frame_batch() {
    let script = sampler_ksp::compile(
        "on init declare ui_table %table[8](1,1,100) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    face.widgets[0].rect = ir::Rect::new(0, 0, 80, 100);
    let mut ui = theme::ui();
    let mut state = ir_view::InputState::default();
    let mut tick = |p: Point, down: bool, state: &mut ir_view::InputState| {
        for _ in 0..2 {
            let el = ir_view::view_state(
                &mut ui,
                "table-test",
                &face,
                ir::PageRef(0),
                &ir_view::Assets::default(),
                ir::Presentation::Vector,
                1.,
                &mut ir_view::Values::default(),
                state,
            );
            ui.frame(
                el,
                Some(Size::new(400., 200.)),
                Input {
                    pointer: PointerInput {
                        pos: Some(p),
                        buttons: if down {
                            Buttons::PRIMARY
                        } else {
                            Buttons::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                1. / 60.,
            )
            .unwrap();
        }
    };
    tick(Point::new(5., 80.), false, &mut state);
    tick(Point::new(5., 80.), true, &mut state);
    state.edits.clear();
    tick(Point::new(75., 10.), true, &mut state);
    let indices = state
        .edits
        .iter()
        .map(|e| e.index)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(indices, (0..8).collect());
    assert!(state.edits.iter().all(|e| e.cursor == 7 && e.event == 2));
    assert!(
        matches!(&state.values[&ir::WidgetRef(0)],ir::Value::Reals(v) if *v==vec![20.,30.,40.,50.,60.,70.,80.,90.])
    );
}

#[test]
fn widget_ksp_global_z_hit_order_crosses_parent_boundaries() {
    let script=sampler_ksp::compile("on init declare ui_panel $p declare ui_knob $child(0,100,1) declare ui_knob $other(0,100,1) end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    for w in &mut face.widgets {
        w.rect = ir::Rect::new(10, 10, 80, 80);
    }
    face.widgets[0].rect = ir::Rect::new(10, 10, 100, 100);
    face.widgets[1].rect = ir::Rect::new(0, 0, 80, 80);
    face.widgets[1].parent = Some(ir::WidgetRef(0));
    face.widgets[1].z = 9;
    face.widgets[2].z = 5;
    let (delta, captured) = audit_motion(&face, 1, 0., -30.);
    assert!(
        captured && delta > 0.,
        "child with higher global layer captures over later foreign sibling"
    );
    face.widgets[0].hidden = true;
    let (delta, captured) = audit_motion(&face, 2, 0., -30.);
    assert!(
        captured && delta > 0.,
        "hidden parent removes child hit target"
    );
}

#[test]
fn widget_authored_label_overflow_owns_nested_wheel_and_yields_at_boundary() {
    let script = sampler_ksp::compile(
        "on init declare ui_label $label(1,1) end on",
        48000,
        sampler_ksp::Limits::LIBRARY,
        &[],
    )
    .unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    face.widgets[0].rect = ir::Rect::new(0, 0, 120, 60);
    face.widgets[0].text = (0..50)
        .map(|n| format!("Label line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let assets = ir_view::Assets::default();
    let mut tick = |ui: &mut Ui, input: Input| {
        let authored = ir_view::view_state(
            ui,
            "label-test",
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
            &mut state,
        );
        let el = col![authored, block(200., 500.)]
            .w(200.)
            .h(120.)
            .scroll()
            .id("label-parent");
        ui.frame(el, Some(Size::new(200., 120.)), input, 1. / 60.)
            .unwrap();
    };
    for _ in 0..4 {
        tick(&mut ui, Input::default());
    }
    let wheel = |delta: f64| Input {
        pointer: PointerInput {
            pos: Some(Point::new(50., 30.)),
            ..Default::default()
        },
        wheel: Vec2::new(0., delta),
        ..Default::default()
    };
    tick(&mut ui, wheel(50.));
    for _ in 0..20 {
        tick(&mut ui, Input::default());
    }
    assert!(
        ui.scroll("/label-test-ir-0")[1] > 0.,
        "authored label scrolls its own overflow"
    );
    assert_eq!(ui.scroll("label-parent"), [0., 0.]);
    tick(&mut ui, wheel(-30.));
    for _ in 0..20 {
        tick(&mut ui, Input::default());
    }
    assert_eq!(ui.scroll("/label-test-ir-0")[1], 20.);
    assert_eq!(ui.scroll("label-parent"), [0., 0.]);
    ui.set_scroll("/label-test-ir-0", [0., 100000.]);
    for _ in 0..20 {
        tick(&mut ui, Input::default());
    }
    let end = ui.scroll("/label-test-ir-0");
    tick(&mut ui, wheel(50.));
    for _ in 0..20 {
        tick(&mut ui, Input::default());
    }
    assert!((ui.scroll("/label-test-ir-0")[1] - end[1]).abs() < 1e-6);
    // One native-layout tick may settle a subpixel content extent at the boundary.
    tick(&mut ui, wheel(50.));
    for _ in 0..20 {
        tick(&mut ui, Input::default());
    }
    assert!(
        ui.scroll("label-parent")[1] > 0.,
        "exhausted label yields wheel to parent"
    );
}

#[test]
#[ignore = "real-library footer text gate; set KONTRA_AUDIT_WIDGET_PATCH"]
fn widget_conflux_footer_text_reaches_typed_publication_without_truncation() {
    let Some(path) = std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").map(std::path::PathBuf::from)
    else {
        return;
    };
    let mut source = sampler_kontakt::read(&path).unwrap().instrument;
    let saved = source
        .behaviors
        .iter()
        .flat_map(|b| b.state.iter())
        .filter_map(|(name, value)| {
            if let sampler_ir::Saved::Text(value) = value {
                Some((name.trim_start_matches('@').to_owned(), value.clone()))
            } else {
                None
            }
        })
        .collect::<std::collections::HashMap<_, _>>();
    source.retain_zones(|_| false);
    source.assets.clear();
    let loaded = sampler_kontakt::prepare(
        source,
        vec![],
        &sampler_kontakt::Options {
            library: Some(path.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 16);
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    let mut script_ui = crate::sound::ScriptUi {
        views: loaded.scripts,
        resources: loaded.resources,
        ..Default::default()
    };
    let controls = loaded
        .interfaces
        .iter()
        .flat_map(|face| {
            face.widgets
                .iter()
                .enumerate()
                .map(move |(n, w)| (face.source, n, w.clone()))
        })
        .collect();
    let face = loaded
        .interfaces
        .into_iter()
        .find(|f| f.source == (ir::Source::Ksp { slot: 2 }))
        .unwrap();
    let mut h = NativeGesture {
        face: face.clone(),
        script_ui: &mut script_ui,
        runtime: &mut runtime,
        ui: theme::ui(),
        values: Default::default(),
        state: Default::default(),
        assets: Default::default(),
    };
    h.sync();
    let mut count = 0;
    for (n, w) in face
        .widgets
        .iter()
        .enumerate()
        .filter(|(_, w)| w.name.starts_with("@Footer__Macro__Name__"))
    {
        let sampler_core::WidgetValue::Text(actual) = h.read(n) else {
            panic!("footer needs typed text")
        };
        let published = h.state.values.get(&ir::WidgetRef(n));
        let expected = saved
            .get(w.name.trim_start_matches('@'))
            .expect("footer name has saved state");
        let authored = match &w.value {
            Some(ir::Value::Text(s)) => Some(s),
            _ => None,
        };
        println!(
            "FOOTER_TEXT widget={n} saved_bytes={} saved_chars={} runtime_bytes={} runtime_chars={} authored_matches={} typed_matches={}",
            expected.len(),
            expected.chars().count(),
            actual.as_str().len(),
            actual.as_str().chars().count(),
            authored.is_some_and(|s| s == expected),
            matches!(published,Some(ir::Value::Text(s)) if s==actual.as_str())
        );
        assert!(
            actual.as_str() == expected,
            "footer native readback differs from complete saved text"
        );
        assert!(
            matches!(published,Some(ir::Value::Text(s)) if s==actual.as_str()),
            "typed publication lost footer text"
        );
        count += 1;
    }
    assert_eq!(count, 6);
    let package = std::sync::Arc::new(
        super::native_runtime::Package::load(&path)
            .unwrap_or_else(|_| panic!("native package load failed; private details omitted")),
    );
    let session = super::native_runtime::Session::new(
        package,
        &face.native_ui.as_ref().unwrap().entry,
        controls,
    )
    .unwrap_or_else(|_| panic!("native session init failed; private details omitted"));
    session.update_view(&face, &h.values, &h.state.values, &h.state.meters);
    let graph = session
        .render()
        .unwrap_or_else(|_| panic!("native graph render failed; private details omitted"));
    fn texts(node: &mlua::Table, out: &mut Vec<(String, String)>) {
        let kind = node.get::<String>("kind").unwrap_or_default();
        if matches!(kind.as_str(), "Text" | "TextInput") {
            let props = node.get::<mlua::Table>("props").unwrap();
            out.push((kind, props.get::<String>("text").unwrap_or_default()));
        }
        if let Ok(children) = node.get::<mlua::Table>("children") {
            for child in children.sequence_values::<mlua::Table>() {
                texts(&child.unwrap(), out);
            }
        }
        if let Ok(modifiers) = node.get::<mlua::Table>("modifiers") {
            for modifier in modifiers.sequence_values::<mlua::Table>() {
                let modifier = modifier.unwrap();
                let kind = modifier.get::<String>("name").unwrap_or_default();
                if matches!(kind.as_str(), "background" | "overlay") {
                    if let Ok(child) = modifier.get::<mlua::Table>("value") {
                        texts(&child, out);
                    }
                } else if kind == "popover" {
                    if let Ok(value) = modifier.get::<mlua::Table>("value") {
                        if let Ok(child) = value.get::<mlua::Table>("content") {
                            texts(&child, out);
                        }
                    }
                }
            }
        }
    }
    let mut labels = Vec::new();
    texts(&graph, &mut labels);
    let mut matched = 0;
    for w in face
        .widgets
        .iter()
        .filter(|w| w.name.starts_with("@Footer__Macro__Name__"))
    {
        let expected = saved.get(w.name.trim_start_matches('@')).unwrap();
        let matches = labels
            .iter()
            .filter(|(_, text)| text == expected)
            .collect::<Vec<_>>();
        println!(
            "FOOTER_NATIVE_TEXT ui_id={:?} chars={} matching_text_nodes={} kinds={:?}",
            w.source_id,
            expected.chars().count(),
            matches.len(),
            matches.iter().map(|(kind, _)| kind).collect::<Vec<_>>()
        );
        assert!(
            !matches.is_empty(),
            "complete footer text missing from rendered native graph"
        );
        matched += 1;
    }
    assert_eq!(matched, 6);
}

#[test]
fn v1_sound_editor_controls_are_reachable() {
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let mut instrument = sampler_ir::Instrument {
        name: "Editor fixture".into(),
        ..Default::default()
    };
    instrument.groups.push(sampler_ir::Group {
        name: "Sustain".into(),
        ..Default::default()
    });
    let mut zone = sampler_ir::Zone::new(sampler_ir::AssetRef(0));
    zone.group = Some(sampler_ir::GroupRef(0));
    instrument.zones.push(zone);
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/generated/editor.nki".into(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].instrument = Some(Arc::new(instrument));
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Sound");
    for id in [
        "edit-group-prev",
        "edit-group-next",
        "edit-scope-all",
        "edit-scope-one",
        "edit-compact",
        "edit-expanded",
        "edit-envelope",
    ] {
        assert!(
            h.ui.scene().unwrap().surface(id).is_some(),
            "missing v1 editor control {id}"
        );
    }
}

fn editor_fixture() -> Arc<crate::plugin::SamplerParams> {
    use sampler_core::{ControlValue, Envelope, Prepared};
    use sampler_ir as I;
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let mut i = I::Instrument {
        name: "Native editor".into(),
        ..Default::default()
    };
    i.groups = vec![
        I::Group {
            name: "Sustain".into(),
            ..Default::default()
        },
        I::Group {
            name: "Legato".into(),
            ..Default::default()
        },
    ];
    i.source_indices.groups = vec![Some(I::GroupRef(0)), Some(I::GroupRef(1))];
    i.modulators.push(I::Modulator {
        scope: I::Scope::Voice,
        source: I::ModulationSource::Envelope(I::Envelope {
            attack: I::Time::Milliseconds(10.),
            decay: I::Time::Milliseconds(200.),
            sustain: 0.5,
            release: I::Time::Milliseconds(300.),
            ..Default::default()
        }),
    });
    for g in 0..2 {
        let mut z = I::Zone::new(I::AssetRef(0));
        z.group = Some(I::GroupRef(g));
        z.amplitude = Some(I::ModulatorRef(0));
        i.zones.push(z);
    }
    let native = Envelope::new(480, 0, 9600, 0.5, 14400).unwrap();
    let plan = Prepared::new(48000, vec![], vec![], 0)
        .unwrap()
        .with_groups(2, vec![])
        .unwrap()
        .with_group_envelope_parameters(0, 0, 2, native)
        .unwrap()
        .with_group_envelope_parameters(1, 1, 7, native)
        .unwrap();
    let atoms = p.shared.part(0).unwrap();
    *atoms.engine_bindings.lock().unwrap() = plan.engine_parameter_bindings().into();
    *atoms.controls.lock().unwrap() = plan
        .controls()
        .iter()
        .map(|c| {
            let ControlValue::Real(v) = c.default else {
                panic!("real native lane")
            };
            crate::plugin::ControlCell::new(sampler_ui_ir::ControlId(c.id.0), v)
        })
        .collect();
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/generated/editor.nki".into(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[0].instrument = Some(Arc::new(i));
    p
}

#[test]
fn v1_editor_typed_native_values_and_state_round_trip() {
    use crate::sound::edits::{Edits, Override, Param};
    use moose::prelude::*;
    let p = editor_fixture();
    let atoms = p.shared.part(0).unwrap();
    let i = p.shared.view.lock().unwrap().parts[0]
        .instrument
        .clone()
        .unwrap();
    let model = super::editor_model::Model::new(
        &i,
        0,
        &Edits::default(),
        &atoms.engine_bindings.lock().unwrap(),
        &atoms.control_values(),
        48000.,
    );
    for (param, text, native) in [
        (Param::Attack, "250 ms", 0.25),
        (Param::Release, "1.5 s", 1.5),
        (Param::Sustain, "-6 dB", 0.501187),
    ] {
        let n = model.typed(param, text).unwrap();
        assert!((model.display(param, n) - native).abs() < 0.0001, "{text}");
    }
    assert_eq!(model.typed(Param::Attack, "fast"), None);
    assert!(model.display(Param::Sustain, model.typed(Param::Sustain, "-inf").unwrap()) < 0.00001);
    let mut e = Edits::default();
    e.set(Override {
        group: None,
        param: Param::Attack,
        offset: 0.1,
    });
    e.set(Override {
        group: Some(1),
        param: Param::Attack,
        offset: 0.2,
    });
    assert!((e.offset(1, Param::Attack) - 0.3).abs() < 1e-6);
    let mut state = p.selection.read().unwrap().clone();
    state.parts[0].group = 1;
    state.parts[0].edits = e;
    assert_eq!(
        crate::plugin::Selection::deserialize(&state.serialize())
            .unwrap()
            .parts[0]
            .edits,
        state.parts[0].edits
    );
}

#[test]
fn v1_editor_scope_reset_compact_and_group_navigation_work() {
    use crate::sound::edits::{Override, Param};
    let p = editor_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Sound");
    for param in Param::ENVELOPE {
        assert!(
            h.ui.scene()
                .unwrap()
                .surface(&format!("edit-envelope-value-{param:?}"))
                .is_some()
        );
    }
    h.press("edit-group-next");
    assert_eq!(p.selection.read().unwrap().parts[0].group, 1);
    h.press("edit-group-prev");
    assert_eq!(p.selection.read().unwrap().parts[0].group, 0);
    h.press("edit-compact");
    assert!(
        h.ui.scene()
            .unwrap()
            .surface("edit-envelope-value-Attack")
            .is_none()
    );
    h.press("edit-expanded");
    p.selection.write().unwrap().parts[0].edits.set(Override {
        group: None,
        param: Param::Attack,
        offset: 0.1,
    });
    h.idle(3);
    assert!(
        h.ui.scene()
            .unwrap()
            .surface("edit-envelope-reset-Attack")
            .is_some()
    );
    h.idle(60);
    shoot(&h.ui, 1180, 780, "settings-editor.png");
    h.press("edit-envelope-reset-Attack");
    assert!(p.selection.read().unwrap().parts[0].edits.0.is_empty());
}

#[test]
fn v1_editor_graph_drag_wheel_fine_and_typed_readout_work() {
    use crate::sound::edits::Param;
    let p = editor_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Sound");
    h.idle(60);
    let model = || {
        let atoms = p.shared.part(0).unwrap();
        let i = p.shared.view.lock().unwrap().parts[0]
            .instrument
            .clone()
            .unwrap();
        let e = p.selection.read().unwrap().parts[0].edits.clone();
        super::editor_model::Model::new(
            &i,
            0,
            &e,
            &atoms.engine_bindings.lock().unwrap(),
            &atoms.control_values(),
            48000.,
        )
    };
    let handle = |h: &Harness| {
        let m = model();
        let env = m.playing.envelope.as_ref().unwrap();
        let shape = super::viz::envelope_over(env, super::viz::envelope_width(env));
        let at = super::viz::envelope_handles(&shape, env)[0].at;
        let r =
            h.ui.scene()
                .unwrap()
                .surface("edit-envelope")
                .unwrap()
                .frame;
        Point::new(
            r.x + theme::SPACE + f64::from(at[0]) * (r.size.width - 2. * theme::SPACE),
            r.y + theme::SPACE + (1. - f64::from(at[1])) * (r.size.height - 2. * theme::SPACE),
        )
    };
    let pointer = |at: Point, down: bool, shift: bool| Input {
        pointer: PointerInput {
            pos: Some(at),
            buttons: if down {
                Buttons::PRIMARY
            } else {
                Buttons::default()
            },
            mods: Mods {
                shift,
                ..Default::default()
            },
        },
        ..Default::default()
    };
    let at = handle(&h);
    h.tick(pointer(at, true, false));
    h.tick(pointer(Point::new(at.x + 40., at.y), true, false));
    h.tick(pointer(Point::new(at.x + 40., at.y), false, false));
    h.idle(3);
    let normal = p.selection.read().unwrap().parts[0]
        .edits
        .get(None, Param::Attack);
    assert!(normal > 0.01, "drag edits the all-groups layer");
    h.press("edit-reset-part");
    h.press("edit-scope-one");
    let at = handle(&h);
    h.tick(pointer(at, true, true));
    h.tick(pointer(Point::new(at.x + 40., at.y), true, true));
    h.tick(pointer(Point::new(at.x + 40., at.y), false, true));
    h.idle(3);
    let fine = p.selection.read().unwrap().parts[0]
        .edits
        .get(Some(0), Param::Attack);
    assert!(
        (fine / normal - 0.1).abs() < 0.03,
        "Shift scales drag to a tenth: {fine}/{normal}"
    );
    let at = handle(&h);
    h.tick(Input {
        pointer: PointerInput {
            pos: Some(at),
            ..Default::default()
        },
        wheel: Vec2::new(0., -1.),
        ..Default::default()
    });
    h.idle(3);
    assert!(
        p.selection.read().unwrap().parts[0]
            .edits
            .get(Some(0), Param::Attack)
            > fine,
        "wheel edits nearest handle"
    );
    let at = super::tests::center(&h.ui, "edit-envelope-value-Attack");
    for down in [true, false, true, false] {
        h.tick(pointer(at, down, false));
    }
    h.idle(2);
    let edit = "edit-envelope-value-Attack-edit";
    assert!(
        h.ui.scene().unwrap().surface(edit).is_some(),
        "double-click opens typed readout"
    );
    h.ui.focus(edit);
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::Char('a'),
            mods: Mods {
                ctrl: true,
                ..Default::default()
            },
        }],
        ..Default::default()
    });
    h.tick(Input {
        text: "250 ms".into(),
        ..Default::default()
    });
    h.tick(Input {
        keys: vec![KeyPress {
            key: Key::Enter,
            mods: Mods::default(),
        }],
        ..Default::default()
    });
    h.idle(3);
    let m = model();
    assert!(
        (m.playing.envelope.unwrap().attack - 0.25).abs() < 0.0001,
        "typed input changes the same offset layer"
    );
}

#[test]
fn sound_editor_close_and_drop_release_probe_owner() {
    use std::sync::atomic::Ordering::Relaxed;
    let p = editor_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Sound");
    for close in [true, false] {
        assert_eq!(
            p.shared.editor_watch.load(Relaxed),
            0,
            "the visible Sound editor owns the probe"
        );
        let mut editor = super::editor(p.clone());
        if close {
            editor.close();
        }
        drop(editor);
        assert_eq!(
            p.shared.editor_watch.load(Relaxed),
            usize::MAX,
            "closing or dropping the editor stops audio-thread probe publication"
        );
        h.idle(3);
        assert_eq!(
            p.shared.editor_watch.load(Relaxed),
            0,
            "reopening restores the visible editor's probe"
        );
    }
}

#[test]
fn v1_editor_has_one_selected_owner_across_rack_parts() {
    let p = editor_fixture();
    let i = p.shared.view.lock().unwrap().parts[0].instrument.clone();
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/generated/second.nki".into(),
            ..Default::default()
        });
    p.shared.view.lock().unwrap().parts[1].instrument = i;
    let mut h = Harness::new(&p, 1180., 1000.);
    h.press("view-0-Sound");
    h.press("view-1-Sound");
    assert!(
        h.ui.scene().unwrap().surface("edit-open-0").is_some(),
        "the first part yields the single v1 editor"
    );
    assert!(h.ui.scene().unwrap().surface("edit-open-1").is_none());
    h.press("edit-open-0");
    assert!(h.ui.scene().unwrap().surface("edit-open-1").is_some());
    assert!(
        h.ui.scene()
            .unwrap()
            .surface("edit-envelope-value-Attack")
            .is_some(),
        "returning to the first part restores its native values"
    );
}

#[test]
fn v1_editor_zero_offset_keeps_exact_native_graph_and_reset() {
    use crate::sound::edits::{Edits, Override, Param};
    let p = editor_fixture();
    let atoms = p.shared.part(0).unwrap();
    let i = p.shared.view.lock().unwrap().parts[0]
        .instrument
        .clone()
        .unwrap();
    let bindings = atoms.engine_bindings.lock().unwrap();
    let values = atoms.control_values();
    let mut edits = Edits::default();
    let original = super::editor_model::Model::new(&i, 0, &edits, &bindings, &values, 48000.);
    let env = original.base.envelope.as_ref().unwrap();
    assert_eq!(
        env.attack_shape,
        sampler_ir::Curve::Linear,
        "zero offset preserves the exact native linear curve"
    );
    assert_eq!(
        env.attack, 0.01,
        "graph uses the native frame count without a normalized round trip"
    );
    assert!(
        env.trace(10)[0]
            .iter()
            .enumerate()
            .all(|(n, v)| (*v - n as f32 / 10.).abs() < 1e-6)
    );
    edits.set(Override {
        group: None,
        param: Param::Curve,
        offset: 0.1,
    });
    let changed = super::editor_model::Model::new(&i, 0, &edits, &bindings, &values, 48000.);
    assert!(changed.playing.envelope.as_ref().unwrap().attack_shape != env.attack_shape);
    assert_eq!(
        changed.base.envelope.as_ref().unwrap().attack_shape,
        env.attack_shape
    );
    edits.reset(Param::Curve);
    let reset = super::editor_model::Model::new(&i, 0, &edits, &bindings, &values, 48000.);
    assert!(reset.playing.envelope == original.base.envelope);
}

#[test]
fn v1_auto_align_settings_and_manual_part_timing_are_reachable() {
    let p = editor_fixture();
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("app-menu");
    let pick = |h: &mut Harness, label: &str| {
        let scene = h.ui.scene().unwrap();
        let text = scene
            .surfaces()
            .find(|s| s.text_value.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing v1 setting {label}"))
            .frame;
        let id = scene
            .surfaces()
            .find(|s| {
                s.key.to_string().starts_with("menu-item-")
                    && s.frame.y <= text.y
                    && s.frame.y + s.frame.size.height >= text.y + text.size.height
            })
            .unwrap()
            .key
            .to_string();
        h.press(&id);
    };
    pick(&mut h, "Auto-align timing");
    assert!(p.selection.read().unwrap().auto_align);
    h.press("app-menu");
    pick(&mut h, "Only while the transport plays");
    assert!(p.selection.read().unwrap().align_transport_only);
    h.press("more-0");
    pick(&mut h, "Play 10 ms earlier");
    assert_eq!(
        p.selection.read().unwrap().parts[0].timing.override_ms,
        Some(10.)
    );
    h.press("more-0");
    pick(&mut h, "Play 10 ms later");
    assert_eq!(
        p.selection.read().unwrap().parts[0].timing.override_ms,
        Some(0.)
    );
    h.press("more-0");
    pick(&mut h, "As measured");
    assert_eq!(
        p.selection.read().unwrap().parts[0].timing.override_ms,
        None
    );
    h.press("more-0");
    pick(&mut h, "Exclude from alignment");
    assert!(p.selection.read().unwrap().parts[0].timing.exclude);
    p.selection.write().unwrap().parts[0].timing.source = "stale".into();
    h.press("more-0");
    pick(&mut h, "Measure again");
    assert!(
        p.selection.read().unwrap().parts[0]
            .timing
            .source
            .is_empty()
    );
    shoot(&h.ui, 1180, 780, "settings-timing.png");
}

#[test]
fn widget_mouse_area_press_release_reaches_native_callback_without_drop_configuration() {
    let instrument=sampler_ir::Instrument {behaviors:vec![sampler_ir::Behavior {name:String::new(),language:sampler_ir::Language::Ksp,source:"on init declare ui_mouse_area $area declare $calls declare $event declare $over end on on ui_control($area) inc($calls) $event := $NI_MOUSE_EVENT_TYPE $over := $NI_MOUSE_OVER_CONTROL end on".into(),slot:Some(0),state:vec![],requires:vec![]}],..Default::default()};
    let loaded = sampler_kontakt::prepare(instrument, vec![], &Default::default()).unwrap();
    let limits = sampler_core::Limits::for_plan(&loaded.plan, 16, 16);
    let mut runtime = sampler_core::Runtime::new(loaded.plan, limits).unwrap();
    let mut script_ui = crate::sound::ScriptUi {
        views: loaded.scripts,
        resources: loaded.resources,
        ..Default::default()
    };
    let mut face = loaded.interfaces.into_iter().next().unwrap();
    face.widgets[0].rect = ir::Rect::new(0, 0, 100, 100);
    let mut h = NativeGesture {
        face,
        script_ui: &mut script_ui,
        runtime: &mut runtime,
        ui: theme::ui(),
        values: Default::default(),
        state: Default::default(),
        assets: Default::default(),
    };
    h.sync();
    h.settle();
    let plan = h.runtime.active_plan();
    let cell = |h: &NativeGesture<'_>, index| {
        h.runtime
            .script_cell(plan, sampler_core::ScriptInstanceId(0), index)
            .unwrap()
    };
    h.pointer(Point::new(50., 50.), false);
    h.pointer(Point::new(50., 50.), true);
    assert_eq!(cell(&h, 1), 1, "MouseArea press must run ui_control once");
    assert_eq!(cell(&h, 2), 0);
    assert_eq!(cell(&h, 3), 1);
    h.pointer(Point::new(50., 50.), true);
    assert_eq!(cell(&h, 1), 1, "holding must not repeat press");
    h.pointer(Point::new(150., 150.), false);
    assert_eq!(
        cell(&h, 1),
        2,
        "captured release outside must run ui_control once"
    );
    assert_eq!(cell(&h, 2), 1);
    assert_eq!(cell(&h, 3), 0);
    assert_eq!(
        h.read(0),
        sampler_core::WidgetValue::Integer(0),
        "button metadata must retain the MouseArea handle value"
    );
}

#[test]
fn widget_missing_feedback_keeps_current_authored_value_separate_from_reset_default() {
    let script=sampler_ksp::compile("on init declare ui_knob $k(0,2,1) $k := 1 set_control_par(get_ui_id($k),$CONTROL_PAR_DEFAULT_VALUE,0) end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let mut face = ir_view::resolved(&script.ui(&|_| None).unwrap());
    face.widgets[0].rect = ir::Rect::new(0, 0, 100, 100);
    assert_eq!(face.widgets[0].value, Some(ir::Value::Integer(1)));
    let ir::Binding::Control(control) = face.widgets[0].binding else {
        panic!()
    };
    let mut ui = theme::ui();
    let mut values = ir_view::Values::default();
    let mut state = ir_view::InputState::default();
    let mut tick =
        |p: Point, down: bool, values: &mut ir_view::Values, state: &mut ir_view::InputState| {
            for _ in 0..2 {
                let el = ir_view::view_state(
                    &mut ui,
                    "missing-feedback",
                    &face,
                    ir::PageRef(0),
                    &Default::default(),
                    ir::Presentation::Vector,
                    1.,
                    values,
                    state,
                );
                ui.frame(
                    el,
                    Some(Size::new(200., 200.)),
                    Input {
                        pointer: PointerInput {
                            pos: Some(p),
                            buttons: if down {
                                Buttons::PRIMARY
                            } else {
                                Buttons::default()
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    1. / 60.,
                )
                .unwrap();
            }
        };
    tick(Point::new(50., 50.), false, &mut values, &mut state);
    assert_eq!(
        values[&control], 1.,
        "missing readback must seed current authored value, not reset default"
    );
    assert!(
        state.edits.is_empty(),
        "passive paint must not produce a widget edit"
    );
    tick(Point::new(50., 50.), true, &mut values, &mut state);
    tick(Point::new(50., 30.), true, &mut values, &mut state);
    assert!(
        state.edits.is_empty(),
        "sub-step drag must stay at current1, never jump to reset0"
    );
    tick(Point::new(50., -50.), true, &mut values, &mut state);
    assert_eq!(
        values[&control], 2.,
        "unrounded accumulator must eventually cross a step from authored current value"
    );
}

#[test]
fn native_ladder_editor_uses_hertz_for_readout_typing_and_graph() {
    use crate::sound::edits::{Edits, Param};
    use sampler_core::{
        ControlId, EngineParameterAddress, EngineParameterBinding, EngineParameterLaw,
    };
    use sampler_ir as I;
    let owner = I::SlotAddress {
        group: 0,
        slot: 3,
        generic: -1,
    };
    let mut instrument = I::Instrument::default();
    instrument.groups.push(I::Group {
        chain: Some(I::ChainRef(0)),
        ..Default::default()
    });
    instrument.chains.push(I::Chain {
        scope: I::Scope::Group(I::GroupRef(0)),
        pre_amplitude: vec![I::Processor::LadderLP4(I::LadderLP4 {
            address: Some(owner),
            gain: 0.,
            cutoff: 0.5,
            resonance: 0.2,
            record_version: 0x92,
        })],
        post_amplitude: vec![],
    });
    let bindings =
        ["ENGINE_PAR_CUTOFF", "ENGINE_PAR_RESONANCE"].map(|name| EngineParameterBinding {
            address: EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id(name).unwrap(),
                group: owner.group,
                slot: owner.slot,
                generic: owner.generic,
            },
            control: ControlId(if name == "ENGINE_PAR_CUTOFF" { 10 } else { 11 }),
            law: EngineParameterLaw::Linear { low: 0., high: 1. },
        });
    let model = super::editor_model::Model::new(
        &instrument,
        0,
        &Edits::default(),
        &bindings,
        &[(ir::ControlId(10), 0.5), (ir::ControlId(11), 0.2)],
        48000.,
    );
    let param = Param::Cutoff(3);
    let expected = 2f32.powf((1481.8816 + 575. * 0.5) / 60. - 20.);
    assert!(
        (model.display(param, 0.5) - expected).abs() < 0.2,
        "native normalized ladder cutoff must display in hertz"
    );
    let typed = model.typed(param, "1 kHz").unwrap();
    assert!(
        (model.display(param, typed) - 1000.).abs() < 0.5,
        "typing hertz must invert the native cutoff law"
    );
    let handle = super::viz::filter_handles(&model.playing).remove(0);
    assert!(
        (0.0..=1.0).contains(&handle.at[0]),
        "ladder handle belongs on the audible frequency axis"
    );
    assert!(
        model.playing.magnitude(100.) > model.playing.magnitude(10000.) * 10.,
        "the graph must include the native LP4 response"
    );
    let expected_scale = 1000f32.log2() / (575. / 60.);
    assert!(
        (handle.x.unwrap().1 - expected_scale).abs() < 0.001,
        "graph drag follows native frequency octaves"
    );
    let mut edits = Edits::default();
    edits.set(crate::sound::edits::Override {
        group: None,
        param,
        offset: 0.2,
    });
    let changed = super::editor_model::Model::new(
        &instrument,
        0,
        &edits,
        &bindings,
        &[(ir::ControlId(10), 0.5), (ir::ControlId(11), 0.2)],
        48000.,
    );
    assert_eq!(changed.base.magnitude(1000.), model.base.magnitude(1000.));
    assert!(
        changed.playing.magnitude(1000.) > model.playing.magnitude(1000.) * 2.,
        "graph follows the native edit layer"
    );
    let mut with_gain = bindings.to_vec();
    with_gain.push(EngineParameterBinding {
        address: EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_GAIN").unwrap(),
            group: 0,
            slot: 3,
            generic: -1,
        },
        control: ControlId(12),
        law: EngineParameterLaw::SignedNormalized,
    });
    let scripted = super::editor_model::Model::new(
        &instrument,
        0,
        &Edits::default(),
        &with_gain,
        &[
            (ir::ControlId(10), 0.5),
            (ir::ControlId(11), 0.2),
            (ir::ControlId(12), 0.5),
        ],
        48000.,
    );
    assert!(
        scripted.base.magnitude(100.) > model.base.magnitude(100.) * 1.8,
        "graph includes the current native gain lane"
    );
    // Kontakt's group insert rack is a shared voice chain referenced by zones.
    instrument.groups[0].chain = None;
    instrument.chains[0].scope = I::Scope::Voice;
    let mut zone = I::Zone::new(I::AssetRef(0));
    zone.group = Some(I::GroupRef(0));
    zone.chain = Some(I::ChainRef(0));
    instrument.zones.extend([zone.clone(), zone]);
    let voice = super::editor_model::Model::new(
        &instrument,
        0,
        &Edits::default(),
        &bindings,
        &[(ir::ControlId(10), 0.5), (ir::ControlId(11), 0.2)],
        48000.,
    );
    assert_eq!(
        voice.display(param, 0.5),
        model.display(param, 0.5),
        "native zone voice-chain cutoff uses the same Hz law"
    );
    assert_eq!(
        voice.base.magnitude(1000.),
        model.base.magnitude(1000.),
        "a shared voice chain appears once, not once per zone"
    );
}

#[test]
fn native_voice_group_filter_graphs_deduplicate_and_keep_control_owners() {
    use crate::sound::edits::{Edits, Override, Param};
    use sampler_core::{EngineParameterAddress, EngineParameterBinding, EngineParameterLaw};
    use sampler_ir as I;
    let mut i = I::Instrument::default();
    i.groups.push(I::Group {
        chain: Some(I::ChainRef(1)),
        ..Default::default()
    });
    let filter = |hz| {
        I::Processor::Filter(I::Filter {
            kind: I::FilterKind::LowPass { poles: 2 },
            cutoff: I::Frequency::Hertz(hz),
            resonance: I::Resonance::Q(std::f64::consts::FRAC_1_SQRT_2),
        })
    };
    for (scope, hz) in [
        (I::Scope::Voice, 1000.),
        (I::Scope::Group(I::GroupRef(0)), 2000.),
    ] {
        i.chains.push(I::Chain {
            scope,
            pre_amplitude: vec![filter(hz)],
            post_amplitude: vec![],
        });
    }
    let mut zone = I::Zone::new(I::AssetRef(0));
    zone.group = Some(I::GroupRef(0));
    zone.chain = Some(I::ChainRef(0));
    i.zones.extend([zone.clone(), zone]);
    let key = "native-group-filter".to_string();
    let control = sampler_core::lower::ir_control_id(&key);
    i.controls.push(I::Control {
        key,
        label: String::new(),
        value: I::ControlValue::Continuous {
            min: 20.,
            max: 20000.,
            default: 2000.,
            unit: I::ControlUnit::Hertz,
        },
        automation: I::Automation::None,
    });
    i.processor_controls.push(I::ProcessorControl {
        control: I::ControlRef(0),
        chain: I::ChainRef(1),
        index: 0,
        parameter: I::ProcessorParameter::Cutoff,
        ramp: I::Time::Milliseconds(0.),
    });
    let law = EngineParameterLaw::Exponential {
        low: 20.,
        high: 20000.,
    };
    let binding = EngineParameterBinding {
        control,
        law,
        address: EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),
            group: 0,
            slot: 4,
            generic: -1,
        },
    };
    let values = [(ir::ControlId(control.0), 4000.)];
    let model =
        super::editor_model::Model::new(&i, 0, &Edits::default(), &[binding], &values, 48000.);
    let magnitude = |cutoff| {
        sampler_core::Biquad::new(
            48000,
            sampler_core::FilterKind::LowPass,
            cutoff,
            std::f64::consts::FRAC_1_SQRT_2,
        )
        .unwrap()
        .magnitude(3000.) as f32
    };
    assert!(
        (model.base.magnitude(3000.) - magnitude(1000.) * magnitude(4000.)).abs() < 1e-6,
        "shared zone chain appears once; group control updates its own filter after voice filters"
    );
    let mut edits = Edits::default();
    edits.set(Override {
        group: None,
        param: Param::Cutoff(4),
        offset: 0.1,
    });
    let changed = super::editor_model::Model::new(&i, 0, &edits, &[binding], &values, 48000.);
    let cutoff = law.decode(law.encode(4000.) + 100000);
    assert!(
        (changed.playing.magnitude(3000.) - magnitude(1000.) * magnitude(cutoff)).abs() < 1e-6,
        "native edit changes the intended group filter, preserving voice-chain ownership"
    );
}

/// Sound's Mapping is the same read-only IR view as the chrome shortcut.
#[test]
fn mapping_sound_tabs_preserve_zone_identity_and_ir() {
    use sampler_ir as sir;
    let mut inst = sir::Instrument {
        name: "Layered strings (synthetic RR fixture)".into(),
        ..Default::default()
    };
    inst.assets.push(sir::Asset {
        location: sir::AssetLocation::Path("Samples/Cello_C3_rr1.wav".into()),
        encoding: sir::Encoding::Wav,
        root_key: Some(60),
        loops: vec![],
    });
    for name in ["Sustain", "Shorts"] {
        inst.groups.push(sir::Group {
            name: name.into(),
            ..Default::default()
        });
    }
    inst.sequences.push(sir::Sequence {
        policy: sir::SequencePolicy::RoundRobin,
        takes: 2,
        counter: sir::CounterScope::Key,
    });
    for n in 0..3 {
        let mut z = sir::Zone::new(sir::AssetRef(0));
        z.group = Some(sir::GroupRef(n / 2));
        z.keys = sir::KeyRange { low: 48, high: 72 };
        z.velocities = sir::VelocityRange { low: 1, high: 127 };
        if n < 2 {
            z.selection = Some(sir::Selection {
                sequence: sir::SequenceRef(0),
                take: sir::Take::Index(n as u32),
            });
        }
        inst.zones.push(z);
    }
    let source = Arc::new(inst);
    let before = (*source).clone();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/x/Layered strings.nki".into(),
            ..Default::default()
        });
    {
        let mut v = p.shared.view.lock().unwrap();
        v.parts.resize_with(1, Default::default);
        v.parts[0].active = "Layered strings (synthetic RR fixture)".into();
        v.parts[0].instrument = Some(source.clone());
    }
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Sound");
        assert!(
            ui.ui.scene().unwrap().surface("sound-tabs-0").is_some(),
            "Sound exposes its shared tab strip"
        );
        ui.press("sound-tab-0-Mapping");
        assert!(ui.ui.scene().unwrap().surface("map-0").is_some());
        ui.press("map-group-0-0");
        ui.press("map-zone-0-1");
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some(),
            "second RR zone retains its own identity"
        );
        assert!(ui.ui.scene().unwrap().surface("map-zone-0-0").is_some());
        assert!(
            ui.ui.scene().unwrap().surface("map-zone-0-2").is_none(),
            "group filter excludes other groups"
        );
        assert_eq!(
            p.shared
                .editor_watch
                .load(std::sync::atomic::Ordering::Relaxed),
            usize::MAX,
            "mapping does not arm audio editor probes"
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = std::path::PathBuf::from(dir).join(format!("mapping-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        ui.press("view-0-Mapping");
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some(),
            "legacy chrome route shares selection"
        );
    }
    assert_eq!(
        *source, before,
        "mapping selection never edits the worker IR"
    );
}

#[test]
fn mapping_waveform_worker_reads_falcon_without_original_waveform_widget() {
    use moose::prelude::BackgroundTask;
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("Cello_C3.wav");
    let mut writer = hound::WavWriter::create(
        &wav,
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..48000 {
        writer
            .write_sample(((n as f64 * 0.0288).sin() * 20000. * (1. - n as f64 / 60000.)) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
    let mut alternate = hound::WavWriter::create(
        dir.path().join("Cello_G3.wav"),
        hound::WavSpec {
            channels: 1,
            sample_rate: 48000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..24000 {
        alternate.write_sample(10000i16).unwrap();
    }
    alternate.finalize().unwrap();
    let path = dir.path().join("Layered strings.uvip");
    std::fs::write(&path,r#"<UVI4><Program Name="Layered strings (synthetic Falcon fixture)"><Layers><Layer Name="Strings"><Keygroups><Keygroup Name="Sustain" LowKey="48" HighKey="84" LowVelocity="1" HighVelocity="127"><Oscillators><SamplePlayer SamplePath="Cello_C3.wav" BaseNote="60" FineTune="-12" Gain="0.8"><PlaybackOptions Start="2000" Stop="45000"><Loop Start="12000" End="34000" Type="0"/></PlaybackOptions></SamplePlayer><SamplePlayer SamplePath="Cello_G3.wav" BaseNote="67" FineTune="12" Gain="0.7"/></Oscillators></Keygroup><Keygroup Name="Shorts" LowKey="55" HighKey="79" LowVelocity="70" HighVelocity="127"><Oscillators><SamplePlayer SamplePath="Cello_C3.wav" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.display().to_string(),
            ..Default::default()
        });
    for _ in 0..200 {
        crate::plugin::Load.run(&p);
        if p.shared
            .view
            .lock()
            .unwrap()
            .parts
            .first()
            .is_some_and(|v| !v.loading && v.instrument.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let (inst, epoch) = {
        let v = p.shared.view.lock().unwrap();
        let v = &v.parts[0];
        (
            v.instrument
                .clone()
                .unwrap_or_else(|| panic!("{}", v.status)),
            v.generation,
        )
    };
    assert_eq!(inst.source, sampler_ir::SourceFormat::Uvi);
    assert_eq!(inst.zones.len(), 3);
    assert_eq!(
        crate::sound::waveform::source_ids(&inst),
        vec![1, 2, 3],
        "Falcon player source identities match these fixture zones"
    );
    let part = p.shared.part(0).unwrap();
    let envelope = (0..200)
        .find_map(|_| {
            let e = part.zone_waveform(1, epoch, 512);
            if e.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            e
        })
        .expect("Mapping resolves full admitted PCM without Original widgets");
    assert_eq!(envelope.frames, 48000);
    assert_eq!(envelope.sample_rate, 48000);
    assert!(envelope.peaks.iter().any(|(lo, hi)| hi - lo > 0.3));
    assert!(
        part.zone_waveform(1, epoch + 1, 512).is_none(),
        "stale load cannot request a waveform"
    );
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Sound");
        ui.press("sound-tab-0-Mapping");
        ui.press("map-group-0-0");
        ui.press("map-zone-0-0");
        for _ in 0..15 {
            ui.idle(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(ui.ui.scene().unwrap().surface("map-wave-0").is_some());
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-zoom-in-0")
                .is_some(),
            "the selected sample has source-frame zoom and pan controls"
        );
        ui.press("map-wave-zoom-in-0");
        ui.press("map-wave-pan-right-0");
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains("View 24000–48000 frames"),
            "wave buttons change the actual source viewport"
        );
        let zoomed = (0..200)
            .find_map(|_| {
                let e = part.zone_waveform_window(1, epoch, 512, Some((24000, 48000)));
                if e.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                e
            })
            .unwrap();
        assert_eq!(zoomed.range, (24000, 48000));
        assert_eq!(zoomed.frames, 48000);
        assert!(
            part.zone_waveform_window(1, epoch + 1, 512, Some((24000, 48000)))
                .is_none()
        );
        ui.press("map-zone-0-1");
        for _ in 0..100 {
            ui.idle(1);
            if ui
                .ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .is_some_and(|s| s.contains("View 0–24000 frames"))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains("View 0–24000 frames"),
            "selecting another admitted sample resets the old 48k-frame viewport"
        );
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some()
        );
        ui.press("map-zone-0-0");
        ui.press("map-wave-fit-0");
        ui.idle(10);
        let at = super::tests::center(&ui.ui, "map-wave-0");
        ui.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(0., -120.),
            ..Default::default()
        });
        ui.idle(3);
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains("View 12000–36000 frames"),
            "wheel zoom is anchored to the pointer's exact source frame"
        );
        ui.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(50., 0.),
            ..Default::default()
        });
        ui.idle(3);
        assert!(
            !ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains("View 12000–36000 frames"),
            "horizontal scrolling pans the source window"
        );
        ui.press("map-wave-fit-0");
        for _ in 0..20 {
            ui.idle(1);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains("View 0–48000 frames")
        );
        let scene = ui.ui.scene().unwrap();
        let status = scene.surface("map-wave-status-0").unwrap().frame;
        let keys = scene.surface("keys").unwrap().frame;
        assert!(
            status.y + status.size.height <= keys.y - 24.,
            "all inspector details fit above the keyboard at {w}×{h}: {} vs {}",
            status.y + status.size.height,
            keys.y - 24.
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = std::path::PathBuf::from(dir).join(format!("mapping-falcon-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        // Holding the existing audition control emits an onset, then a balanced release.
        let at = super::tests::center(&ui.ui, "map-audition-0");
        ui.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons: Buttons::PRIMARY,
                ..Default::default()
            },
            ..Default::default()
        });
        ui.idle(1);
        assert!(matches!(p.shared.keyboard.pop(),Some((0,crate::plugin::Play::Note(_,v))) if v>0));
        ui.tick(Input::default());
        ui.idle(2);
        assert!(matches!(
            p.shared.keyboard.pop(),
            Some((0, crate::plugin::Play::Note(_, 0)))
        ));
        let held = || Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons: Buttons::PRIMARY,
                ..Default::default()
            },
            ..Default::default()
        };
        ui.tick(held());
        ui.tick(held());
        assert!(matches!(p.shared.keyboard.pop(),Some((0,crate::plugin::Play::Note(_,v))) if v>0));
        drop(ui);
        assert!(
            matches!(
                p.shared.keyboard.pop(),
                Some((0, crate::plugin::Play::Note(_, 0)))
            ),
            "closing the UI releases its held audition"
        );
    }
}

#[test]
fn mapping_kontakt_worker_probe_skips_when_library_is_missing() {
    use moose::prelude::BackgroundTask;
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki",
    );
    if !path.exists() {
        eprintln!("SKIP Mapping Kontakt probe: fixture library absent");
        return;
    }
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.display().to_string(),
            ..Default::default()
        });
    for _ in 0..600 {
        crate::plugin::Load.run(&p);
        if p.shared
            .view
            .lock()
            .unwrap()
            .parts
            .first()
            .is_some_and(|v| !v.loading && v.instrument.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (inst, epoch) = {
        let v = p.shared.view.lock().unwrap();
        let v = &v.parts[0];
        (
            v.instrument
                .clone()
                .unwrap_or_else(|| panic!("{}", v.status)),
            v.generation,
        )
    };
    assert!(matches!(
        inst.source,
        sampler_ir::SourceFormat::Kontakt { .. }
    ));
    assert!(!inst.articulations.is_empty());
    let source_ids = crate::sound::waveform::source_ids(&inst);
    let zone = inst
        .zones
        .iter()
        .position(|z| (z.keys.low..=z.keys.high).contains(&60))
        .expect("musical mapping");
    let part = p.shared.part(0).unwrap();
    let envelope = (0..400)
        .find_map(|_| {
            let e = part.zone_waveform(source_ids[zone], epoch, 512);
            if e.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            e
        })
        .expect("Kontakt physical source identity resolves its full waveform");
    assert!(envelope.frames > 0 && envelope.peaks.iter().any(|(lo, hi)| hi - lo > 0.0001));
    let before = (*inst).clone();
    let mut ui = Harness::new(&p, 1180., 780.);
    ui.press("view-0-Sound");
    ui.press("sound-tab-0-Mapping");
    assert!(ui.ui.scene().unwrap().surface("map-0").is_some());
    assert_eq!(*inst, before);
}

#[test]
fn mapping_audition_editor_close_releases_held_note() {
    for close in [true, false] {
        let p = Arc::new(crate::plugin::SamplerParams::new());
        p.shared.press_key(0, 60, 80);
        assert_eq!(
            p.shared.keyboard.pop(),
            Some((0, crate::plugin::Play::Note(60, 80)))
        );
        let mut editor = super::editor(p.clone());
        if close {
            editor.close();
        }
        drop(editor);
        assert_eq!(
            p.shared.played[60].load(std::sync::atomic::Ordering::Relaxed),
            0,
            "native close/drop releases Mapping audition even when the build closure is retained"
        );
        assert_eq!(
            p.shared.keyboard.pop(),
            Some((0, crate::plugin::Play::Note(60, 0)))
        );
    }
}

#[test]
fn v1_eq_handles_use_each_band_gain_and_graph_drag_scale() {
    use crate::sound::edits::{Edits, Override, Param};
    use sampler_core::{EngineParameterBinding, EngineParameterLaw};
    use sampler_ir as I;
    let mut i = I::Instrument::default();
    i.groups.push(I::Group {
        chain: Some(I::ChainRef(0)),
        ..Default::default()
    });
    let filter = |kind, hz| {
        I::Processor::Filter(I::Filter {
            kind,
            cutoff: I::Frequency::Hertz(hz),
            resonance: I::Resonance::Q(1.),
        })
    };
    i.chains.push(I::Chain {
        scope: I::Scope::Group(I::GroupRef(0)),
        pre_amplitude: vec![
            filter(I::FilterKind::LowPass { poles: 2 }, 500.),
            filter(
                I::FilterKind::Peak {
                    gain: I::Gain::Decibels(6.),
                },
                1000.,
            ),
            filter(
                I::FilterKind::Peak {
                    gain: I::Gain::Decibels(-6.),
                },
                2000.,
            ),
        ],
        post_amplitude: vec![],
    });
    let mut bindings = Vec::new();
    let mut values = Vec::new();
    for (band, hz, db, range) in [(0, 1000., 6., 18.), (1, 2000., -6., 24.)] {
        for (param, parameter, law, native) in [
            (
                Param::Freq(3, band),
                I::ProcessorParameter::Cutoff,
                EngineParameterLaw::Exponential {
                    low: 20.,
                    high: 20000.,
                },
                hz,
            ),
            (
                Param::Bandwidth(3, band),
                I::ProcessorParameter::Resonance,
                EngineParameterLaw::Exponential {
                    low: 0.1,
                    high: 10.,
                },
                1.,
            ),
            (
                Param::Gain(3, band),
                I::ProcessorParameter::Gain,
                EngineParameterLaw::DecibelGain {
                    low_db: -range,
                    high_db: range,
                },
                10f64.powf(db / 20.),
            ),
        ] {
            let key = format!("eq-handle-{}", bindings.len());
            let control = sampler_core::lower::ir_control_id(&key);
            let reference = I::ControlRef(i.controls.len());
            i.controls.push(I::Control {
                key,
                label: String::new(),
                value: I::ControlValue::Continuous {
                    min: law.decode(0),
                    max: law.decode(1000000),
                    default: native,
                    unit: I::ControlUnit::None,
                },
                automation: I::Automation::None,
            });
            i.processor_controls.push(I::ProcessorControl {
                control: reference,
                chain: I::ChainRef(0),
                index: usize::from(band) + 1,
                parameter,
                ramp: I::Time::Milliseconds(0.),
            });
            bindings.push(EngineParameterBinding {
                control,
                law,
                address: param.address(0),
            });
            values.push((ir::ControlId(control.0), native));
        }
    }
    let make =
        |edits: &Edits| super::editor_model::Model::new(&i, 0, edits, &bindings, &values, 48000.);
    let model = make(&Edits::default());
    let handles = super::viz::filter_handles(&model.playing);
    for (band, db, range) in [(0, 6., 18.), (1, -6., 24.)] {
        let param = Param::Gain(3, band);
        let handle = handles
            .iter()
            .find(|h| h.x.unwrap().0 == Param::Freq(3, band))
            .unwrap();
        assert!(
            (handle.at[1] - super::viz::db_y(db)).abs() < 1e-5,
            "each EQ handle shows its own gain, independent of other bands and serial filters"
        );
        assert!(
            (handle.y.unwrap().1 - 60. / (2. * range)).abs() < 1e-5,
            "vertical graph motion must use the admitted gain range"
        );
        assert_eq!(handle.wheel, Some(Param::Bandwidth(3, band)));
        let typed = model.typed(param, &format!("{db} dB")).unwrap();
        assert!((model.display(param, typed) - db).abs() < 0.001);
        let mut edits = Edits::default();
        edits.set(Override {
            group: None,
            param,
            offset: handle.y.unwrap().1 * 0.1,
        });
        let changed = make(&edits);
        let moved = super::viz::filter_handles(&changed.playing);
        let moved = moved
            .iter()
            .find(|h| h.x.unwrap().0 == Param::Freq(3, band))
            .unwrap();
        assert!(
            (moved.at[1] - handle.at[1] - 0.1).abs() < 1e-5,
            "dragging up one tenth of the graph raises the band's gain by 6 dB"
        );
        assert!(
            changed.base == model.base,
            "player edits preserve the script-set base"
        );
        edits.reset(param);
        assert_eq!(super::viz::filter_handles(&make(&edits).playing), handles);
    }
}

#[test]
fn mapping_navigation_reaches_late_takes_and_restores_fitted_keys() {
    use sampler_ir as sir;
    let mut inst = sir::Instrument {
        name: "Synthetic 96-take mapping".into(),
        ..Default::default()
    };
    inst.sequences.push(sir::Sequence {
        policy: sir::SequencePolicy::RoundRobin,
        takes: 96,
        counter: sir::CounterScope::Key,
    });
    for n in 0..96 {
        let mut z = sir::Zone::new(sir::AssetRef(0));
        z.selection = Some(sir::Selection {
            sequence: sir::SequenceRef(0),
            take: sir::Take::Index(n),
        });
        inst.zones.push(z);
    }
    let source = Arc::new(inst);
    let before = (*source).clone();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: "/x/Synthetic 96-take mapping.nki".into(),
            ..Default::default()
        });
    {
        let mut v = p.shared.view.lock().unwrap();
        v.parts.resize_with(1, Default::default);
        v.parts[0].active = "Synthetic 96-take mapping".into();
        v.parts[0].instrument = Some(source.clone());
    }
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Mapping");
        assert!(
            ui.ui.scene().unwrap().surface("map-zoom-in-0").is_some(),
            "mapping can zoom a full keyboard"
        );
        ui.press("map-zoom-in-0");
        ui.press("map-pan-right-0");
        assert!(
            ui.ui.scene().unwrap().surface("map-scale-0-0").is_none(),
            "zoom and pan move away from MIDI 0"
        );
        let map = ui.ui.scene().unwrap().surface("map-0").unwrap().frame;
        let at = Point::new(map.x + 0.5, map.y + map.size.height / 2.);
        let held = || Input {
            pointer: PointerInput {
                pos: Some(at),
                buttons: Buttons::PRIMARY,
                ..Default::default()
            },
            ..Default::default()
        };
        ui.tick(held());
        ui.tick(held());
        assert_eq!(
            p.shared.keyboard.pop(),
            Some((0, crate::plugin::Play::Note(64, 64))),
            "zoomed hit testing auditions the shown key and velocity"
        );
        ui.tick(Input::default());
        ui.idle(2);
        assert_eq!(
            p.shared.keyboard.pop(),
            Some((0, crate::plugin::Play::Note(64, 0)))
        );
        ui.press("map-fit-0");
        assert!(
            ui.ui.scene().unwrap().surface("map-scale-0-0").is_some(),
            "Fit restores the full authored range"
        );
        assert!(ui.ui.scene().unwrap().surface("map-scale-0-120").is_some());
        ui.press("map-stack-next-0");
        ui.press("map-stack-next-0");
        assert!(
            ui.ui.scene().unwrap().surface("map-zone-0-80").is_some(),
            "later takes are directly reachable"
        );
        let at = super::tests::center(&ui.ui, "map-stack-0");
        ui.tick(Input {
            pointer: PointerInput {
                pos: Some(at),
                ..Default::default()
            },
            wheel: Vec2::new(0., 352.),
            ..Default::default()
        });
        ui.idle(90);
        let scene = ui.ui.scene().unwrap();
        let list = scene.surface("map-stack-0").unwrap().frame;
        let take = scene.surface("map-zone-0-80").unwrap().frame;
        assert!(
            take.y >= list.y - 0.5 && take.y + take.size.height <= list.y + list.size.height + 0.5,
            "scroll brings take 81 inside the visible list at {w}×{h}"
        );
        ui.press("map-zone-0-80");
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-80")
                .is_some()
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = std::path::PathBuf::from(dir).join(format!("mapping-stack-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        ui.press("map-stack-prev-0");
        assert!(ui.ui.scene().unwrap().surface("map-zone-0-32").is_some());
        assert_eq!(
            ui.ui.scroll("map-stack-0"),
            [0., 0.],
            "page changes reset the old list scroll"
        );
        let scene = ui.ui.scene().unwrap();
        let map = scene.surface("map-0").unwrap().frame;
        let tick = scene.surface("map-scale-0-60").unwrap().frame;
        assert!(
            (tick.x - map.x - map.size.width * 60. / 128.).abs() < 1.,
            "octave labels align with exact map cells"
        );
    }
    assert_eq!(
        *source, before,
        "navigation leaves authored ranges and sequence order untouched"
    );
}

#[test]
fn mapping_analog_waveform_worker_probe_skips_only_when_library_is_missing() {
    use moose::prelude::BackgroundTask;
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    );
    if !path.exists() {
        eprintln!("SKIP Analog Mapping waveform probe: library absent");
        return;
    }
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.display().to_string(),
            ..Default::default()
        });
    for _ in 0..600 {
        crate::plugin::Load.run(&p);
        if p.shared
            .view
            .lock()
            .unwrap()
            .parts
            .first()
            .is_some_and(|v| !v.loading && v.instrument.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let (inst, epoch) = {
        let v = p.shared.view.lock().unwrap();
        let v = &v.parts[0];
        (
            v.instrument
                .clone()
                .unwrap_or_else(|| panic!("Analog load: {}", v.status)),
            v.generation,
        )
    };
    let ids = crate::sound::waveform::source_ids(&inst);
    let part = p.shared.part(0).unwrap();
    assert!(!inst.zones.is_empty());
    let first = 0;
    let envelope = (0..600)
        .find_map(|_| {
            let e = part.zone_waveform(ids[first], epoch, 512);
            if e.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            e
        })
        .expect("Analog selected zone admits full sample peaks");
    assert!(
        envelope.frames > 0
            && envelope.sample_rate > 0
            && envelope.peaks.iter().any(|(a, b)| b - a > 0.00001)
    );
    let high = envelope.frames.min(1024);
    let view = (0..600)
        .find_map(|_| {
            let e = part.zone_waveform_window(ids[first], epoch, 64, Some((0, high)));
            if e.is_none() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            e
        })
        .expect("Analog zoomed peaks");
    assert_eq!(view.range, (0, high));
    assert_eq!(view.frames, envelope.frames);
    assert!(
        part.zone_waveform_window(ids[first], epoch + 1, 64, Some((0, high)))
            .is_none()
    );
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Sound");
        ui.press("sound-tab-0-Mapping");
        for _ in 0..100 {
            ui.idle(1);
            if ui
                .ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .is_some_and(|s| s.contains(&format!("View 0–{} frames", envelope.frames)))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-wave-status-0")
                .unwrap()
                .tip
                .as_deref()
                .unwrap()
                .contains(&format!("View 0–{} frames", envelope.frames))
        );
        let scene = ui.ui.scene().unwrap();
        let status = scene.surface("map-wave-status-0").unwrap().frame;
        let keys = scene.surface("keys").unwrap().frame;
        let part = scene.surface("part-0").unwrap().frame;
        let rack = scene.surface("rack-view").unwrap().frame;
        assert!(
            status.size.height >= 10.
                && status.y + status.size.height
                    <= (part.y + part.size.height - 1.)
                        .min(keys.y - 24.)
                        .min(rack.y + rack.size.height),
            "authored header rows leave room for every inspector detail at {w}×{h}: bottom {}, height {}, part {}, keys {}, rack {}",
            status.y + status.size.height,
            status.size.height,
            part.y + part.size.height - 1.,
            keys.y - 24.,
            rack.y + rack.size.height
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = std::path::PathBuf::from(dir).join(format!("mapping-analog-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        ui.press("keyboard-toggle");
        ui.idle(30);
        let scene = ui.ui.scene().unwrap();
        let status = scene.surface("map-wave-status-0").unwrap().frame;
        let rack = scene.surface("rack-view").unwrap().frame;
        assert!(
            status.size.height >= 10. && status.y + status.size.height <= rack.y + rack.size.height,
            "inspector layout also works with the keyboard hidden"
        );
    }
    assert!(
        p.shared.keyboard.pop().is_none(),
        "waveform inspection dispatches no instrument audio"
    );
}

fn mapping_linked_fixture() -> (
    Arc<crate::plugin::SamplerParams>,
    Arc<sampler_ir::Instrument>,
) {
    use sampler_ir as I;
    let mut inst = I::Instrument {
        name: "Linked groups (synthetic metadata fixture)".into(),
        ..Default::default()
    };
    for (n, (group, art)) in [("Close room", "Sustain"), ("Far room", "Tremolo")]
        .into_iter()
        .enumerate()
    {
        inst.groups.push(I::Group {
            name: group.into(),
            start: vec![I::GroupStart {
                slot: 0,
                test: I::StartTest::Key {
                    low: 24 + n as u8,
                    high: 24 + n as u8,
                },
                next: I::StartJoin::And,
            }],
            ..Default::default()
        });
        inst.articulations.push(I::Articulation {
            source: format!("native:fixture:{n}"),
            name: art.into(),
            switch_keys: vec![24 + n as u8],
            default: n == 0,
            ..Default::default()
        });
        let mut zone = I::Zone::new(I::AssetRef(0));
        zone.group = Some(I::GroupRef(n));
        zone.keys = I::KeyRange { low: 48, high: 72 };
        inst.zones.push(zone);
    }
    let source = Arc::new(inst);
    let p = Arc::new(crate::plugin::SamplerParams::new());
    let ids = crate::sound::articulation::identities(&source.articulations);
    let mut part = crate::plugin::Part {
        path: "/synthetic/Linked groups.nki".into(),
        ..Default::default()
    };
    part.articulation_overlay
        .set(&ids[1], crate::sound::articulation::Input::Keys(vec![49]));
    part.articulation_overlay
        .move_to(&source.articulations, &ids[1], 0);
    p.selection.write().unwrap().parts.push(part);
    {
        let mut v = p.shared.view.lock().unwrap();
        v.parts[0].instrument = Some(source.clone());
        v.parts[0].active = source.name.clone();
    }
    (p, source)
}

#[test]
fn mapping_group_search_keeps_selection_and_offers_empty_recovery() {
    let (p, source) = mapping_linked_fixture();
    let before = (*source).clone();
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Sound");
        ui.press("sound-tab-0-Mapping");
        assert!(
            ui.ui.scene().unwrap().surface("map-search-0").is_some(),
            "Mapping needs its own group/articulation search"
        );
        let scene = ui.ui.scene().unwrap();
        let field = scene.surface("map-search-0").unwrap().frame;
        let rail = scene.surface("map-groups-0").unwrap().frame;
        assert!(
            field.x >= rail.x + theme::CONTROL,
            "search glyph has its own space before the editable text canvas"
        );
        let at = Point::new(rail.x + theme::SPACE, field.y + field.size.height / 2.);
        for buttons in [Buttons::PRIMARY, Buttons::default()] {
            ui.tick(Input {
                pointer: PointerInput {
                    pos: Some(at),
                    buttons,
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        ui.idle(2);
        assert!(
            ui.ui.focused("map-search-0"),
            "the leading icon area still focuses the shared search field"
        );
        ui.press("map-group-0-1");
        ui.ui.focus("map-search-0");
        ui.tick(Input {
            text: " TrEmOlO ".into(),
            ..Default::default()
        });
        ui.idle(3);
        assert!(ui.ui.scene().unwrap().surface("map-group-0-0").is_none());
        assert!(
            ui.ui.scene().unwrap().surface("map-group-0-1").is_some(),
            "articulation names find their source groups, ignoring case and surrounding space"
        );
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some()
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = Path::new(&dir).join(format!("mapping-search-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        ui.press("map-search-0-clear");
        ui.ui.focus("map-search-0");
        ui.tick(Input {
            text: "missing group".into(),
            ..Default::default()
        });
        ui.idle(3);
        assert!(
            ui.ui.scene().unwrap().surface("map-no-groups-0").is_some(),
            "empty search has visible recovery guidance"
        );
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some(),
            "search never changes the selected source or group filter"
        );
        ui.press("map-search-0-clear");
        assert!(ui.ui.scene().unwrap().surface("map-group-0-0").is_some());
        assert!(ui.ui.scene().unwrap().surface("map-group-0-1").is_some());
        assert!(
            ui.ui
                .scene()
                .unwrap()
                .surface("map-inspector-0-1")
                .is_some()
        );
        assert!(
            p.shared.keyboard.pop().is_none() && p.shared.articulation_edits.pop().is_none(),
            "search dispatches no musical input"
        );
    }
    assert_eq!(
        *source, before,
        "search leaves source predicates, zone ranges and articulation identities untouched"
    );
}

#[test]
fn mapping_art_link_selection_tracks_keyswitch_identity() {
    let (p, source) = mapping_linked_fixture();
    let before = (*source).clone();
    let ids = crate::sound::articulation::identities(&source.articulations);
    for (w, h) in [(1180, 780), (900, 640)] {
        let mut ui = Harness::new(&p, w as f64, h as f64);
        ui.press("view-0-Mapping");
        ui.press("map-art-0-1-1");
        assert_eq!(
            p.shared.articulation_edits.pop(),
            Some((0, 1)),
            "link selects source identity despite overlay reorder/remap"
        );
        // The metadata-only fixture models the engine acknowledging its queued selection.
        let runtime = p.shared.part(0).unwrap();
        runtime
            .articulation
            .store(1, std::sync::atomic::Ordering::Relaxed);
        ui.idle(3);
        let link = ui.ui.scene().unwrap().surface("map-art-0-1-1").unwrap();
        assert!(
            matches!(
                link.semantics.as_ref().unwrap().role,
                A11y::Toggle { on: true }
            ),
            "Mapping announces and paints the same selected articulation as the keyswitch panel"
        );
        assert!(
            link.tip.as_deref().unwrap().contains("C#2"),
            "link shows the effective remapped trigger"
        );
        runtime
            .articulation
            .store(u32::MAX, std::sync::atomic::Ordering::Relaxed);
        ui.press("view-0-Articulations");
        let row = format!("{}-name", super::inside::row_id(0, &ids[1]));
        assert!(
            matches!(
                ui.ui
                    .scene()
                    .unwrap()
                    .surface(&row)
                    .unwrap()
                    .semantics
                    .as_ref()
                    .unwrap()
                    .role,
                A11y::Toggle { on: true }
            ),
            "shared selection survives chrome navigation"
        );
        if let Some(dir) = std::env::var_os("KONTAKTO_MAPPING_SHOTS") {
            let path = Path::new(&dir).join(format!("keyswitch-linked-{w}x{h}.png"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            moose::core::screenshot::save_png(&path, &pixels(&ui.ui, w, h), w as u32, h as u32);
        }
        runtime
            .articulation
            .store(0, std::sync::atomic::Ordering::Relaxed);
        ui.idle(3);
        let first = format!("{}-name", super::inside::row_id(0, &ids[0]));
        assert!(
            matches!(
                ui.ui
                    .scene()
                    .unwrap()
                    .surface(&first)
                    .unwrap()
                    .semantics
                    .as_ref()
                    .unwrap()
                    .role,
                A11y::Toggle { on: true }
            ),
            "engine feedback remains authoritative over the fallback UI selection"
        );
        assert!(
            p.shared.keyboard.pop().is_none(),
            "direct articulation selection does not synthesize a note"
        );
    }
    assert_eq!(*source, before);
}

#[test]
fn v1_effects_list_includes_native_zone_chains_once_per_group() {
    use sampler_ir as I;
    let mut i = I::Instrument::default();
    i.groups = vec![
        I::Group {
            chain: Some(I::ChainRef(1)),
            ..Default::default()
        },
        I::Group::default(),
    ];
    for (scope, processor) in [
        (
            I::Scope::Voice,
            I::Processor::LadderLP4(I::LadderLP4 {
                address: None,
                gain: 0.,
                cutoff: 0.5,
                resonance: 0.2,
                record_version: 0x92,
            }),
        ),
        (
            I::Scope::Group(I::GroupRef(0)),
            I::Processor::Gain(I::Gain::Decibels(-6.)),
        ),
        (I::Scope::Master, I::Processor::Rectify(I::Rectifier::Full)),
        (
            I::Scope::Bus(I::BusRef(0)),
            I::Processor::LoFi {
                bits: 8.,
                frequency: 12000.,
                noise: 0.,
                color: 0.5,
            },
        ),
        (I::Scope::Voice, I::Processor::Rectify(I::Rectifier::Half)),
        (
            I::Scope::Voice,
            I::Processor::Daft(I::Daft {
                gain: 0.,
                cutoff: 0.5,
                resonance: 0.2,
                highpass: true,
            }),
        ),
    ] {
        i.chains.push(I::Chain {
            scope,
            pre_amplitude: vec![processor],
            post_amplitude: vec![],
        });
    }
    i.buses.push(I::Bus {
        name: "Room".into(),
        chain: Some(I::ChainRef(3)),
        sends: vec![],
        output: I::Output::Master,
        gain: I::Gain::UNITY,
    });
    for (group, chain) in [(0, 0), (0, 0), (0, 5), (1, 4)] {
        let mut zone = I::Zone::new(I::AssetRef(0));
        zone.group = Some(I::GroupRef(group));
        zone.chain = Some(I::ChainRef(chain));
        i.zones.push(zone);
    }
    let labels = |i: &I::Instrument| {
        let ui = settle(800., 600., |_| super::chain::effects(i, 0));
        ui.scene()
            .unwrap()
            .surfaces()
            .filter_map(|s| s.text_value.as_deref().map(str::to_owned))
            .collect::<Vec<_>>()
    };
    let text = labels(&i);
    let count = |prefix: &str| text.iter().filter(|s| s.starts_with(prefix)).count();
    assert_eq!(
        count("Gain · -6.0 dB"),
        1,
        "the summed group chain is retained"
    );
    assert_eq!(
        count("Rectifier · full wave"),
        1,
        "instrument inserts remain visible"
    );
    assert_eq!(count("LoFi"), 1, "bus effects remain visible");
    assert_eq!(
        count("Ladder low-pass"),
        1,
        "native group inserts live on shared zone voice chains"
    );
    assert_eq!(
        count("Daft high-pass"),
        1,
        "every distinct zone chain of this group is inventoried"
    );
    assert_eq!(
        count("GROUP INSERTS"),
        1,
        "one group heading covers the complete insert inventory"
    );
    assert_eq!(
        count("Rectifier · half wave"),
        0,
        "another group's voice effects stay out of this list"
    );
    i.groups[0].chain = Some(I::ChainRef(0));
    let text = labels(&i);
    assert_eq!(
        text.iter()
            .filter(|s| s.starts_with("Ladder low-pass"))
            .count(),
        1,
        "a chain referenced by both zones and group is shown once"
    );
}

#[test]
fn menus_show_a_fallback_without_editing_the_script_value() {
    let control = ir::ControlId(7);
    let mut menu = ir::Widget::new(
        "$menu",
        ir::PageRef(0),
        ir::Rect::new(0, 0, 120, 20),
        ir::Kind::Menu {
            items: vec![
                ir::MenuItem {
                    text: "First".into(),
                    value: 10,
                    visible: true,
                },
                ir::MenuItem {
                    text: "Second".into(),
                    value: 30,
                    visible: true,
                },
            ],
        },
    );
    menu.binding = ir::Binding::Control(control);
    let face = ir::Interface {
        pages: vec![ir::Page {
            size: ir::Size {
                width: 160,
                height: 60,
            },
            ..Default::default()
        }],
        widgets: vec![menu],
        ..Default::default()
    };
    let assets = ir_view::Assets::default();
    for value in [0., 10., 30., -1.] {
        let mut values: ir_view::Values = [(control, value)].into();
        settle(160., 60., |ui| {
            ir_view::view(
                ui,
                &face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Vector,
                1.,
                &mut values,
            )
        });
        assert_eq!(
            values[&control], value,
            "drawing a menu must not submit an edit"
        );
    }
}

#[test]
fn an_unsized_axis_does_not_discard_the_authored_other_axis() {
    let mut control = ir::Widget::new(
        "$slider",
        ir::PageRef(0),
        ir::Rect::new(0, 0, 220, 0),
        ir::Kind::Slider {
            range: Default::default(),
            orientation: ir::Orientation::Horizontal,
        },
    );
    control.auto_size = true;
    control.default_axes = [false, true];
    let face = ir::Interface {
        pages: vec![ir::Page::default()],
        widgets: vec![control],
        ..Default::default()
    };
    let resolved = ir_view::resolved(&face);
    assert_eq!(
        (
            resolved.widgets[0].rect.width,
            resolved.widgets[0].rect.height
        ),
        (220, 18)
    );
}

#[test]
fn rack_interfaces_have_independent_input_identities() {
    let mut widget = ir::Widget::new(
        "$switch",
        ir::PageRef(0),
        ir::Rect::new(0, 0, 85, 18),
        ir::Kind::Switch,
    );
    let id = ir::ControlId(7);
    widget.binding = ir::Binding::Control(id);
    let face = ir::Interface {
        pages: vec![ir::Page {
            size: ir::Size {
                width: 100,
                height: 30,
            },
            ..Default::default()
        }],
        widgets: vec![widget],
        ..Default::default()
    };
    let assets = ir_view::Assets::default();
    let mut a: ir_view::Values = [(id, 0.)].into();
    let mut b: ir_view::Values = [(id, 1.)].into();
    let mut input_a = ir_view::InputState::default();
    let mut input_b = ir_view::InputState::default();
    let ui = settle(100., 80., |ui| {
        col![
            ir_view::view_state(
                ui,
                "part-0",
                &face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Vector,
                1.,
                &mut a,
                &mut input_a
            ),
            ir_view::view_state(
                ui,
                "part-1",
                &face,
                ir::PageRef(0),
                &assets,
                ir::Presentation::Vector,
                1.,
                &mut b,
                &mut input_b
            )
        ]
    });
    assert!(ui.scene().unwrap().surface("part-0-ir-0").is_some());
    assert!(ui.scene().unwrap().surface("part-1-ir-0").is_some());
    assert_eq!((a[&id], b[&id]), (0., 1.));
}

#[test]
fn script_value_changes_wake_an_idle_editor() {
    use std::sync::atomic::Ordering;
    let params = crate::plugin::SamplerParams::new();
    params.shared.ensure_parts(1);
    let mut watch = super::Watch::default();
    let meters = super::Meters::default();
    let computer = super::computer::Computer::default();
    assert!(watch.changed(&params, &meters, &computer));
    assert!(!watch.changed(&params, &meters, &computer));
    params
        .shared
        .part(0)
        .unwrap()
        .scalar_revision
        .fetch_add(1, Ordering::Release);
    // Script readouts join the existing bounded editor poll.
    watch.cpu_at = None;
    assert!(watch.changed(&params, &meters, &computer));
    assert!(!watch.changed(&params, &meters, &computer));
}

#[test]
fn w10_authored_uvi_fixture_paints_its_declared_ui() {
    let xml = include_str!("../../tests/fixtures/uvi-clear-features.uvip");
    let host = sampler_uvi::script::ScriptHost::new(xml, (), Default::default()).unwrap();
    let face = host.interface();
    let mut values = ir_view::Values::default();
    let assets = ir_view::Assets::default();
    let ui = settle(320., 140., |ui| {
        ir_view::view(
            ui,
            &face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Vector,
            1.,
            &mut values,
        )
    });
    for id in ["ir-2"] {
        let frame = ui.scene().unwrap().surface(id).unwrap().frame;
        assert!(
            frame.size.width > 0. && frame.size.height > 0. && frame.y + frame.size.height <= 140.
        );
    }
    #[cfg(feature = "shots")]
    if let Some(out) = std::env::var_os("KONTRA_UVI_FIXTURE_SHOTS").map(std::path::PathBuf::from) {
        std::fs::create_dir_all(&out).unwrap();
        moose::core::screenshot::save_png(
            &out.join("uvi-authored-original.png"),
            &pixels(&ui, 320, 140),
            320,
            140,
        );
    }
}

/// Opt-in, one-preset shard: records authored metadata and production readback,
/// without exporting samples or script source.
#[test]
fn keyswitch_audit_installed_preset() {
    use crate::sound::{
        Core, CoreLoader, LoadRequest,
        event::Event,
        v2::{V2Core, V2Loader},
    };
    use std::sync::atomic::Ordering;
    let Ok(path) = std::env::var("KONTRA_KEYSWITCH_AUDIT_PATH") else {
        return;
    };
    let output = std::env::var("KONTRA_KEYSWITCH_AUDIT_OUT").expect("audit output directory");
    let mut loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: path.clone().into(),
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    let inst = loaded.instrument.clone().expect("instrument metadata");
    let keys = loaded.scripts.keys();
    let authored_keys: Vec<_> = keys
        .iter()
        .enumerate()
        .filter(|(_, k)| k.name.is_some() || k.control)
        .map(
            |(n, k)| serde_json::json!({"key":n,"name":k.name,"control":k.control,"color":k.color}),
        )
        .collect();
    let authored_choices: Vec<_> = loaded.interfaces.iter().flat_map(|ui| &ui.widgets).filter(|w| matches!(w.kind, ir::Kind::Button {..} | ir::Kind::Switch | ir::Kind::Label | ir::Kind::Menu {..}) && !w.text.is_empty()).map(|w| serde_json::json!({"name":w.name,"text":w.text,"binding":format!("{:?}",w.binding),"hidden":w.hidden,"rect":[w.rect.x,w.rect.y,w.rect.width,w.rect.height]})).collect();
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.clone(),
            ..Default::default()
        });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.parts[0].active = inst.name.clone();
        view.parts[0].instrument = Some(inst.clone());
        view.parts[0].keys = keys;
    }
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part.take());
    let ids = crate::sound::articulation::identities(&inst.articulations);
    let out = Path::new(&output);
    std::fs::create_dir_all(out).unwrap();
    let mut report = serde_json::json!({"path":path,"name":inst.name,"owner":format!("{:?}",inst.switching.owner),"articulations":inst.articulations.iter().map(|a|serde_json::json!({"name":a.name,"source":a.source,"keys":a.switch_keys,"switches":[]})).collect::<Vec<_>>(),"authored_keys":authored_keys,"authored_choices":authored_choices,"feedback_complete":ids.is_empty()});
    std::fs::write(
        out.join("audit.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if ids.is_empty() {
        return;
    }
    let mut ui = Harness::new(&p, 1180., 780.);
    if !ids.is_empty() {
        ui.press("view-0-Articulations");
    }
    let mut rows = Vec::new();
    for (n, a) in inst.articulations.iter().enumerate() {
        let mut switches = Vec::new();
        for &key in &a.switch_keys {
            core.event(0, Event::midi1(0x90, key, 100));
            core.render(16);
            core.event(0, Event::midi1(0x80, key, 0));
            core.render(16);
            core.take_effects(0, &mut |instance, effect| {
                loaded.scripts.apply(instance, effect);
                true
            });
            let active = core.articulation(0);
            if let Some(active) = active {
                p.shared
                    .part(0)
                    .unwrap()
                    .articulation
                    .store(active as u32, Ordering::Relaxed);
            }
            ui.idle(3);
            let highlighted = ui
                .ui
                .scene()
                .unwrap()
                .surface(&format!("{}-name", super::inside::row_id(0, &ids[n])))
                .and_then(|s| s.semantics.as_ref())
                .map(|s| matches!(s.role, A11y::Toggle { on: true }));
            switches.push(serde_json::json!({"key":key,"active":active,"highlighted":highlighted,"control_value":a.control.and_then(|c| core.control_value(0,ir::ControlId(c)))}));
        }
        rows.push(serde_json::json!({"name":a.name,"source":a.source,"keys":a.switch_keys,"default":a.default,"control":a.control.map(|c|format!("{c:032x}")),"switches":switches}));
    }
    report["articulations"] = serde_json::json!(rows);
    report["feedback_complete"] = true.into();
    std::fs::write(
        out.join("audit.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    if !ids.is_empty() {
        moose::core::screenshot::save_png(
            &out.join("panel-1180.png"),
            &pixels(&ui.ui, 1180, 780),
            1180,
            780,
        );
        ui.resize(Size::new(900., 640.));
        ui.idle(3);
        moose::core::screenshot::save_png(
            &out.join("panel-900.png"),
            &pixels(&ui.ui, 900, 640),
            900,
            640,
        );
    }
}

#[test]
fn keyswitch_real_areia_rows_match_authored_names_and_live_selection() {
    use crate::sound::{
        Core, CoreLoader, LoadRequest,
        event::Event,
        v2::{V2Core, V2Loader},
    };
    use std::sync::atomic::Ordering;
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/Areia 1.2.0 [Audio Imperia]/Instruments/01 Core Technique Patches/07 Areia - Full Ens - Core Techniques.nki",
    );
    if !path.is_file() {
        eprintln!("SKIP: Areia audit preset missing");
        return;
    }
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: path.into(),
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    let inst = loaded.instrument.unwrap();
    let keys = loaded.scripts.keys();
    assert_eq!(inst.articulations.len(), 16);
    for a in &inst.articulations {
        let [key] = a.switch_keys.as_slice() else {
            panic!("expected authored single-key technique")
        };
        assert_eq!(
            Some(a.name.as_str()),
            keys[*key as usize].name.as_deref(),
            "row at key {key} must use the instrument's own technique name"
        );
    }
    let p = Arc::new(crate::plugin::SamplerParams::new());
    p.selection
        .write()
        .unwrap()
        .parts
        .push(crate::plugin::Part {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        });
    {
        let mut v = p.shared.view.lock().unwrap();
        v.parts[0].instrument = Some(inst.clone());
        v.parts[0].active = inst.name.clone();
        v.parts[0].keys = keys;
    }
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    let ids = crate::sound::articulation::identities(&inst.articulations);
    let mut h = Harness::new(&p, 1180., 780.);
    h.press("view-0-Articulations");
    for (n, a) in inst.articulations.iter().enumerate() {
        let key = a.switch_keys[0];
        core.event(0, Event::midi1(0x90, key, 100));
        core.render(16);
        core.event(0, Event::midi1(0x80, key, 0));
        core.render(16);
        assert_eq!(
            core.articulation(0),
            Some(n),
            "authored key {key} switches the actual runtime"
        );
        p.shared
            .part(0)
            .unwrap()
            .articulation
            .store(n as u32, Ordering::Relaxed);
        h.idle(3);
        assert!(matches!(
            h.ui.scene()
                .unwrap()
                .surface(&format!("{}-name", super::inside::row_id(0, &ids[n])))
                .unwrap()
                .semantics
                .as_ref()
                .unwrap()
                .role,
            A11y::Toggle { on: true }
        ));
        if n > 0 {
            assert!(
                matches!(
                    h.ui.scene()
                        .unwrap()
                        .surface(&format!("{}-name", super::inside::row_id(0, &ids[n - 1])))
                        .unwrap()
                        .semantics
                        .as_ref()
                        .unwrap()
                        .role,
                    A11y::Toggle { on: false }
                ),
                "the previous row loses its highlight when the next key switches"
            );
        }
    }
    if let Some(output) = std::env::var_os("KONTRA_KEYSWITCH_SHOTS") {
        let key = inst.articulations[0].switch_keys[0];
        core.event(0, Event::midi1(0x90, key, 100));
        core.render(16);
        core.event(0, Event::midi1(0x80, key, 0));
        core.render(16);
        p.shared
            .part(0)
            .unwrap()
            .articulation
            .store(core.articulation(0).unwrap() as u32, Ordering::Relaxed);
        h.idle(3);
        let out = Path::new(&output);
        std::fs::create_dir_all(out).unwrap();
        for (w, height) in [(1180, 780), (900, 640)] {
            h.resize(Size::new(w as f64, height as f64));
            h.idle(3);
            moose::core::screenshot::save_png(
                &out.join(format!("areia-selected-{w}.png")),
                &pixels(&h.ui, w, height),
                w as u32,
                height as u32,
            );
        }
    }
}

#[test]
fn keyswitch_real_analog_strings_does_not_invent_preset_browser_articulations() {
    use crate::sound::{CoreLoader, LoadRequest, v2::V2Loader};
    let path = Path::new(
        "/mnt/MAIN_STORAGE/Libraries/Kontakt/ANALOG STRINGS/Instruments/ANALOG STRINGS.nki",
    );
    if !path.is_file() {
        eprintln!("SKIP: Analog Strings audit preset missing");
        return;
    }
    let loaded = V2Loader
        .prepare(
            &LoadRequest {
                path: path.into(),
                sample_rate: 48000.,
                ..Default::default()
            },
            &mut |_| {},
            &|| false,
        )
        .unwrap();
    assert!(
        loaded.instrument.unwrap().articulations.is_empty(),
        "preset browser entries and zero-sized auxiliary controls must not become articulation rows"
    );
}

#[test]
fn w10_scripted_uvi_strips_positions_and_callback_paint_authored_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let colours = [[255,0,0,255],[0,255,0,255],[0,0,255,255],[255,255,0,255]];
    for (name,width,height,horizontal) in [("h.png",32,8,true),("v.png",8,32,false)] {
        let data: Vec<u8> = (0..height).flat_map(|y| (0..width).flat_map(move |x| colours[if horizontal {x/8} else {y/8}])).collect();
        moose::core::screenshot::save_png(&dir.path().join(name),&data,width as u32,height as u32);
    }
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      require('uvi.ChordRec')
      setSize(320,200); setHeight(160)
      local p=Panel{'Root',bounds={10,20,300,120}}
      local h=Knob{'Horizontal',0,0,1,parent=p,x=1,y=2,width=3,height=4,size={10,11},position={3,4},pos={5,6},bounds={20,0,32,32},showLabel=false,showValue=false}
      h:setStripImage('h.png',4,true)
      local v=Slider{'Vertical',0,0,1,false,true,parent=p,x=1,y=2,width=3,height=4,size={10,11},position={3,4},pos={5,6},bounds={80,0,32,32},showLabel=false,showValue=false}
      v:setStripImage('v.png',4,false)
      local label=p:Label{'State',bounds={130,0,150,32},text='Ready'}
      local button=p:Button{'Fire',bounds={20,80,90,25}}
      local unitBox=p:NumBox{'Percent',0.25,0,1,bounds={130,80,110,25},unit=Unit.PercentNormalized,showLabel=false}
      button.changed=function()
        local root,kind=ChordRec.chordKind({60,64,67})
        label.text=kind; h:setValue(1,false); v:setValue(1,false); unitBox:setValue(0.5,false); setHeight(160)
      end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let patch = dir.path().join("preset.uvip");
    std::fs::write(&patch,xml).unwrap();
    let mut host = sampler_uvi::script::ScriptHost::new(xml,(),Default::default()).unwrap();
    assert!(host.findings().is_empty(),"{:?}",host.findings());
    let mut assets = ir_view::Assets::default();
    let mut values = ir_view::Values::default();
    for (after,name,expected) in [(false,"uvi-scripted-before.png",colours[0]),(true,"uvi-scripted-after.png",colours[3])] {
        if after { host.set_control(sampler_uvi::script::control_id(5,0),1.).unwrap(); }
        let face=host.interface();
        assert_eq!(face.page_rect(ir::WidgetRef(1)),ir::Rect::new(30,20,32,32));
        assert_eq!(face.page_rect(ir::WidgetRef(2)),ir::Rect::new(90,20,32,32));
        assert_eq!((face.pages[0].size.width,face.pages[0].size.height),(320,160));
        assert_eq!(face.widgets[5].initial_value,if after {0.5} else {0.25});
        assert_eq!(face.widgets[5].value_text.as_deref(),Some(if after {"50 %"} else {"25 %"}));
        for (control,value) in host.control_values() { values.insert(control,value); }
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
        loop {
            assets.prepare(&patch,&face,ir::PageRef(0),ir::Presentation::Bitmap,1.,&values);
            if assets.pending()==0 { break; }
            assert!(std::time::Instant::now()<deadline,"authored asset preparation must finish");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let ui=settle(320.,160.,|ui| ir_view::view(ui,&face,ir::PageRef(0),&assets,ir::Presentation::Bitmap,1.,&mut values));
        let data=pixels(&ui,320,160);
        for x in [46,106] {
            let at=(36*320+x)*4;
            assert_eq!(&data[at..at+4],&expected,"{name}: authored strip frame at x={x}");
        }
        if after { assert_eq!(face.widgets[3].text,"M"); }
        #[cfg(feature="shots")]
        if let Some(out)=std::env::var_os("KONTRA_UVI_FIXTURE_SHOTS").map(std::path::PathBuf::from) {
            std::fs::create_dir_all(&out).unwrap();
            moose::core::screenshot::save_png(&out.join(name),&data,320,160);
        }
    }
}
