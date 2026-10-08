//! Opt-in single-instrument rendering witness. Library plaintext stays in memory.
use super::{ir_view, pictures, tests::pixels, theme};
use moose::mui::mui::prelude::*;
use sampler_ksp::model::{Value, WidgetValue};
use sampler_ui_ir as ir;
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::Instant,
};

fn tokens(source: &str) -> BTreeSet<String> {
    let (mut comment, mut quoted, mut word) = (false, false, String::new());
    let mut out = BTreeSet::new();
    for c in source.chars().chain([' ']) {
        if comment {
            if c == '}' {
                comment = false;
            }
            continue;
        }
        if quoted {
            if c == '"' {
                quoted = false;
            }
            continue;
        }
        if c == '{' || c == '"' {
            comment = c == '{';
            quoted = c == '"';
        }
        if !comment && !quoted && (c.is_ascii_alphanumeric() || c == '_' || c == '$') {
            word.push(c);
        } else if !word.is_empty() {
            out.insert(std::mem::take(&mut word));
        }
    }
    out
}

fn count(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *map.entry(key.into()).or_default() += 1;
}

fn environment(
    b: &sampler_ir::Behavior,
    index: usize,
    groups: Vec<String>,
) -> sampler_ksp::Environment {
    use sampler_ir::Saved;
    sampler_ksp::Environment {
        groups,
        slot: b.slot.unwrap_or(index as u8),
        persisted: b
            .state
            .iter()
            .filter_map(|(n, v)| {
                Some((
                    n.clone(),
                    match v {
                        Saved::Int(v) => Value::Int(*v as i32),
                        Saved::Real(v) => Value::Real(*v),
                        Saved::Text(v) => Value::Text(v.clone()),
                        _ => return None,
                    },
                ))
            })
            .collect(),
        persisted_arrays: b
            .state
            .iter()
            .filter_map(|(n, v)| {
                Some((
                    n.clone(),
                    match v {
                        Saved::Ints(v) => v.iter().map(|v| Value::Int(*v as i32)).collect(),
                        Saved::Reals(v) => v.iter().map(|v| Value::Real(*v)).collect(),
                        _ => return None,
                    },
                ))
            })
            .collect(),
        ..Default::default()
    }
}

fn measure(path: &Path, out: &Path, shots: bool) -> Json {
    let start = Instant::now();
    let instruments = if path.extension().is_some_and(|e| e == "nkm") {
        sampler_kontakt::read_multi(path).and_then(|m| {
            m.programs
                .iter()
                .enumerate()
                .map(|(index, _)| sampler_kontakt::read_program(path, index).map(|k| k.instrument))
                .collect::<Result<Vec<_>, _>>()
        })
    } else {
        sampler_kontakt::read(path).map(|k| vec![k.instrument])
    };
    let Ok(instruments) = instruments else {
        return json!({"path":path,"read_failed":true});
    };
    let read_ms = start.elapsed().as_secs_f64() * 1000.;
    let resources = std::cell::RefCell::new(sampler_kontakt::Resources::of(path));
    let mut uses = BTreeSet::new();
    let mut scripts = Vec::new();
    for (part, instrument) in instruments.iter().enumerate() {
        for (index, b) in instrument.behaviors.iter().enumerate() {
            let source_tokens = tokens(&b.source);
            uses.extend(
                source_tokens
                    .iter()
                    .filter(|t| {
                        [
                            "ui_button",
                            "ui_file_selector",
                            "ui_knob",
                            "ui_label",
                            "ui_level_meter",
                            "ui_menu",
                            "ui_mouse_area",
                            "ui_panel",
                            "ui_slider",
                            "ui_switch",
                            "ui_table",
                            "ui_text_edit",
                            "ui_value_edit",
                            "ui_waveform",
                            "ui_wavetable",
                            "ui_xy",
                            "ui_control",
                            "ui_controls",
                            "ui_update",
                        ]
                        .contains(&t.as_str())
                            || t.starts_with("$CONTROL_PAR_")
                            || [
                                "add_menu_item",
                                "add_text_line",
                                "attach_level_meter",
                                "attach_zone",
                                "expose_controls",
                                "fs_get_filename",
                                "fs_navigate",
                                "get_control_par",
                                "get_control_par_arr",
                                "get_control_par_real",
                                "get_control_par_real_arr",
                                "get_control_par_str",
                                "get_control_par_str_arr",
                                "get_font_id",
                                "get_menu_item_str",
                                "get_menu_item_value",
                                "get_menu_item_visibility",
                                "get_num_menu_items",
                                "get_ui_id",
                                "get_ui_wf_property",
                                "hide_part",
                                "load_native_ui",
                                "load_komplete_ui",
                                "load_performance_view",
                                "make_perfview",
                                "move_control",
                                "move_control_px",
                                "set_control_help",
                                "set_control_par",
                                "set_control_par_arr",
                                "set_control_par_real",
                                "set_control_par_real_arr",
                                "set_control_par_str",
                                "set_control_par_str_arr",
                                "set_knob_defval",
                                "set_knob_label",
                                "set_knob_unit",
                                "set_menu_item_str",
                                "set_menu_item_value",
                                "set_menu_item_visibility",
                                "set_table_steps_shown",
                                "set_script_title",
                                "set_skin_offset",
                                "set_text",
                                "set_ui_color",
                                "set_ui_height",
                                "set_ui_height_px",
                                "set_ui_width_px",
                                "set_ui_wf_property",
                                "set_key_color",
                                "set_key_name",
                                "set_key_type",
                                "set_key_pressed",
                                "set_key_pressed_support",
                                "set_keyrange",
                                "set_keyrange_name",
                            ]
                            .contains(&t.as_str())
                    })
                    .cloned(),
            );
            let mut env = environment(
                b,
                index,
                instrument.groups.iter().map(|g| g.name.clone()).collect(),
            );
            let mut view_failed = false;
            if let Some(name) = sampler_ksp::nckp::view_name(&b.source) {
                match resources
                    .borrow_mut()
                    .read(&format!("Resources/performance_view/{name}.nckp"))
                    .and_then(|bytes| sampler_ksp::nckp::parse(&bytes).ok())
                {
                    Some((view, _)) => env.performance_view = view,
                    None => view_failed = true,
                }
            }
            let compile_at = Instant::now();
            let script = sampler_ksp::compile_with(
                &b.source,
                48000,
                sampler_ksp::Limits::LIBRARY,
                &[],
                &env,
            );
            let Ok(script) = script else {
                scripts.push(json!({"part":part,"slot":env.slot,"compile_failed":true,"view_failed":view_failed}));
                continue;
            };
            let compile_ms = compile_at.elapsed().as_secs_f64() * 1000.;
            let mut properties = BTreeMap::new();
            for w in &script.model().interface.widgets {
                for p in w.properties.keys().chain(w.indexed_properties.keys()) {
                    count(&mut properties, p);
                }
            }
            let native_requests = script
                .model()
                .requests
                .iter()
                .filter(|r| r.command == "load_native_ui")
                .count();
            let Ok(face) = script.ui(&|p| resources.borrow_mut().picture(p)) else {
                scripts.push(json!({"part":part,"slot":env.slot,"ir_failed":true}));
                continue;
            };
            let face = ir_view::resolved(&face);
            let mut kinds = BTreeMap::new();
            for w in &face.widgets {
                count(
                    &mut kinds,
                    format!("{:?}", w.kind).split([' ', '{']).next().unwrap(),
                );
            }
            let mut asset_metrics = Vec::new();
            let mut source = pictures::Source::of(path);
            let mut assets = ir_view::Assets::default();
            let asset_at = Instant::now();
            assets.sync(&face, ir::Presentation::Bitmap, |a| {
                let bytes = resources.borrow_mut().read(&a.path);
                let picture = source.load(a);
                let meta = match &a.kind { ir::AssetKind::Image(m) => Some(m), _ => None };
                asset_metrics.push(json!({"path_hash":blake3::hash(a.path.as_bytes()).to_hex().to_string(),
                    "present":bytes.is_some(),"bytes":bytes.as_ref().map(Vec::len),"decoded":picture.is_some(),
                    "frames":meta.map(|m| m.frames),"axis":meta.map(|m|format!("{:?}",m.axis)),
                    "size":meta.and_then(|m|m.size.map(|s|[s.width,s.height])),
                    "margins":meta.map(|m|[m.margins.top,m.margins.bottom,m.margins.left,m.margins.right]),
                    "stretch":meta.map(|m|m.stretch)}));
                picture
            });
            let asset_ms = asset_at.elapsed().as_secs_f64() * 1000.;
            let visible: Vec<_> = face
                .draw_order(ir::PageRef(0))
                .into_iter()
                .filter(|&n| face.visible(n))
                .collect();
            let wall = face.pages[0].background.image;
            let mut values: ir_view::Values = script
                .model()
                .interface
                .widgets
                .iter()
                .filter_map(|w| match (w.control, &w.value) {
                    (Some(c), WidgetValue::Int(v)) => Some((ir::ControlId(c.0), f64::from(*v))),
                    _ => None,
                })
                .collect();
            let size = face.pages[0].size;
            let (w, h) = (
                size.width.clamp(1, 1200) as u16,
                ir_view::height(&face, ir::PageRef(0)).clamp(1, 900) as u16,
            );
            let mut ui = theme::ui();
            let draw_at = Instant::now();
            for _ in 0..3 {
                let root = ir_view::view(
                    &mut ui,
                    &face,
                    ir::PageRef(0),
                    &assets,
                    ir::Presentation::Bitmap,
                    1.,
                    &mut values,
                );
                ui.frame(
                    root,
                    Some(Size::new(f64::from(w), f64::from(h))),
                    Input::default(),
                    1. / 60.,
                )
                .unwrap();
            }
            let rgba = pixels(&ui, w, h);
            let white = rgba
                .chunks_exact(4)
                .filter(|c| c[0] >= 245 && c[1] >= 245 && c[2] >= 245)
                .count();
            let bg = face.pages[0].background.color;
            let ground_pixels = rgba
                .chunks_exact(4)
                .filter(|c| {
                    bg.is_some_and(|b| {
                        c[0].abs_diff(b.r) <= 2
                            && c[1].abs_diff(b.g) <= 2
                            && c[2].abs_diff(b.b) <= 2
                    })
                })
                .count();
            let draw_ms = draw_at.elapsed().as_secs_f64() * 1000.;
            if shots && !visible.is_empty() {
                moose::core::screenshot::save_png(
                    &out.join(format!("part-{part}-slot-{}-bitmap.png", env.slot)),
                    &rgba,
                    w.into(),
                    h.into(),
                );
            }
            let numeric_geometry: Vec<_> = if shots {
                visible.iter().map(|&n| {
                let wd=&face.widgets[n.0]; let r=face.page_rect(n);
                json!({"index":n.0,"rect":[r.x,r.y,r.width as i32,r.height as i32],"z":wd.z,"parent":wd.parent.map(|p|p.0),
                    "kind":format!("{:?}",wd.kind).split([' ','{']).next().unwrap(),"hide_background":wd.hide.background,
                    "binding_control":matches!(wd.binding,ir::Binding::Control(_)),"drag":wd.drag.map(|d|[if d.axis==ir::Orientation::Vertical {0}else{1},d.sensitivity])})
            }).collect()
            } else {
                Vec::new()
            };
            scripts.push(json!({"part":part,"slot":env.slot,"widgets":face.widgets.len(),"visible":visible.len(),
                "kinds":kinds,"properties":properties,"fonts":script.model().interface.fonts.len(),"native_requests":native_requests,
                "assets":asset_metrics,"classic_wallpaper_requested":wall.is_some_and(|a|face.assets[a.0].path.eq_ignore_ascii_case("Resources/pictures/wallpaper.png")),"wallpaper_index":wall.map(|a|a.0),"wallpaper_loaded":wall.is_some_and(|a|assets.get(a).is_some()),
                "background_color":face.pages[0].background.color.map(|c|[c.r,c.g,c.b,c.a]),"skin_offset":face.pages[0].background.offset_y,
                "page_size":[size.width,size.height],"render_size":[w,h],"geometry":numeric_geometry,"picture_bytes":assets.bytes(),
                "white_fraction":white as f64/(usize::from(w)*usize::from(h)) as f64,
                "ground_fraction":ground_pixels as f64/(usize::from(w)*usize::from(h)) as f64,
                "pixel_hash":blake3::hash(&rgba).to_hex().to_string(),
                "unsupported_features":face.unsupported.iter().map(|u|u.feature.clone()).collect::<BTreeSet<_>>(),
                "compile_ms":compile_ms,"asset_ms":asset_ms,"draw_ms":draw_ms,"view_failed":view_failed}));
        }
    }
    let mut containers = Vec::new();
    if shots {
        let root = path
            .ancestors()
            .find(|p| p.join("Samples").is_dir())
            .unwrap_or(path.parent().unwrap());
        let folders = std::iter::once(root.to_owned()).chain(
            std::fs::read_dir(root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir()),
        );
        for folder in folders {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let p = entry.path();
                if !p.extension().is_some_and(|e| {
                    e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt")
                }) {
                    continue;
                }
                if let Ok(mut c) = sampler_kontakt::ResourceContainer::open(&p) {
                    let names: Vec<_> = c.names().into_iter().map(String::from).collect();
                    let mut kinds = BTreeMap::new();
                    for n in &names {
                        let k = if n.contains("native_ui") {
                            "native_ui"
                        } else if n.contains("pictures") {
                            "pictures"
                        } else if n.contains("performance_view") {
                            "performance_view"
                        } else {
                            "other"
                        };
                        count(&mut kinds, k);
                    }
                    let nui: Vec<_> = names.iter().filter(|n| n.ends_with(".nui")).collect();
                    let mut readable_nui = 0;
                    let mut nui_bytes = 0;
                    for n in &nui {
                        if let Ok(Some(b)) = c.read(n) {
                            readable_nui += 1;
                            nui_bytes += b.len();
                        }
                    }
                    containers.push(json!({"extension":p.extension().unwrap().to_string_lossy(),"members":names.len(),
                        "namespaces":kinds,"nui":nui.len(),"readable_nui":readable_nui,"nui_bytes":nui_bytes,
                        "classic_wallpaper_present":names.iter().any(|n|n.eq_ignore_ascii_case("Resources/pictures/wallpaper.png"))}));
                }
            }
        }
    }
    json!({"path":path,"read_ms":read_ms,"total_ms":start.elapsed().as_secs_f64()*1000.,"programs":instruments.len(),"uses":uses,"scripts":scripts,"containers":containers})
}

#[test]
fn token_filter_ignores_comments_and_strings() {
    assert_eq!(
        tokens("{ui_slider} declare ui_knob $x\nset_text($x,\"ui_table\")"),
        ["$x", "declare", "set_text", "ui_knob"]
            .map(String::from)
            .into()
    );
}

fn painted(
    face: &ir::Interface,
    load: impl FnMut(&ir::Asset) -> Option<std::sync::Arc<ir_view::Picture>>,
) -> Vec<u8> {
    let mut assets = ir_view::Assets::default();
    assets.sync(face, ir::Presentation::Bitmap, load);
    let mut ui = theme::ui();
    let mut values = Default::default();
    for _ in 0..3 {
        let el = ir_view::view(
            &mut ui,
            face,
            ir::PageRef(0),
            &assets,
            ir::Presentation::Bitmap,
            1.,
            &mut values,
        );
        ui.frame(el, Some(Size::new(100., 100.)), Input::default(), 1. / 60.)
            .unwrap();
    }
    pixels(&ui, 100, 100)
}

/// Baseline characterization, not a compatibility pass. Reverse these assertions
/// when implementing the fixes; each is an observable source-to-pixel omission.
#[test]
fn baseline_rendering_omissions_are_observable() {
    let s=sampler_ksp::compile("on init\nmake_perfview\ndeclare ui_label $label(1,1)\nset_text($label,\"Alpha\")\nset_control_par(get_ui_id($label),$CONTROL_PAR_FONT_TYPE,get_font_id(\"testfont\"))\nend on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let face = s.ui(&|_| None).unwrap();
    assert_eq!(s.model().interface.fonts.len(), 1);
    assert!(face.assets.is_empty(), "custom font has no resource asset");
    assert!(
        matches!(face.styles[0].font, ir::Font::Stock(0)),
        "custom font id collides with factory font zero"
    );
    let mut f = ir_view::resolved(&face);
    f.pages[0].size = ir::Size {
        width: 100,
        height: 100,
    };
    f.widgets[0].rect = ir::Rect::new(0, 0, 100, 30);
    let a = painted(&f, |_| None);
    f.styles[0].align = ir::Align::Right;
    f.widgets[0].text_y = Some(20);
    assert_eq!(
        a,
        painted(&f, |_| None),
        "alignment and text_y do not alter pixels"
    );
    f.widgets[0].kind = ir::Kind::Table {
        columns: 2,
        range: ir::Range {
            min: 0.,
            max: 100.,
            default: 0.,
            step: Some(1.),
        },
        bipolar: false,
        cells: vec![0, 0],
        steps_shown: None,
    };
    let a = painted(&f, |_| None);
    if let ir::Kind::Table { cells, .. } = &mut f.widgets[0].kind {
        *cells = vec![100, 100];
    }
    assert_eq!(a, painted(&f, |_| None), "table values do not alter pixels");
    f.widgets.clear();
    f.assets.push(ir::Asset {
        path: "synthetic.png".into(),
        kind: ir::AssetKind::Image(Default::default()),
    });
    f.pages[0].background.image = Some(ir::AssetRef(0));
    let solid = |c: [u8; 4]| {
        std::sync::Arc::new(moose::mui::mui::scene::Image::rgba(100, 100, c.repeat(10000)).unwrap())
    };
    let load = |_: &ir::Asset| {
        Some(std::sync::Arc::new(ir_view::Picture {
            frames: vec![solid([255, 0, 0, 255]), solid([0, 0, 255, 255])],
        }))
    };
    let rgba = painted(&f, load);
    assert_eq!(
        &rgba[0..4],
        &[255, 0, 0, 255],
        "wallpaper always draws its first frame from the top"
    );
}

#[test]
#[ignore = "set KONTRA_RENDER_PATH and KONTRA_RENDER_OUT for a single witness"]
fn installed_render_witness() {
    let path = std::path::PathBuf::from(std::env::var("KONTRA_RENDER_PATH").unwrap());
    let dir = std::path::PathBuf::from(std::env::var("KONTRA_RENDER_OUT").unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    let shots = std::env::var_os("KONTRA_RENDER_SHOTS").is_some();
    // Suppress third-party parser panic payloads: they can include library text.
    let old = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| measure(&path, &dir, shots))
        .unwrap_or_else(|_| json!({"path": path, "panic": true}));
    std::panic::set_hook(old);
    std::fs::write(
        dir.join("witness.json"),
        serde_json::to_vec(&result).unwrap(),
    )
    .unwrap();
    assert!(
        result.get("panic").is_none(),
        "witness parser/render panicked"
    );
    assert!(result.get("read_failed").is_none(), "witness read failed");
}
