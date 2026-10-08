//! Read-only shared scanner adapter. Only the Original authored presentation is rendered.
use super::{ir_view, pictures, theme};
use crate::{
    scan_metrics as metrics,
    sound::{
        Core, CoreLoader, LoadRequest,
        event::Event,
        v2::{V2Core, V2Loader},
    },
};
use moose::mui::mui::{prelude::*, vello};
use sampler_ui_ir as ir;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn add(counts: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *counts.entry(key.into()).or_default() += 1;
}

fn render(
    face: &ir::Interface,
    source: &mut pictures::Source,
    values: &mut ir_view::Values,
    out: &Path,
    prefix: &str,
) -> Value {
    let face = ir_view::resolved(face);
    let mut missing = Vec::new();
    let mut assets = ir_view::Assets::default();
    assets.sync(&face, ir::Presentation::Bitmap, |a| {
        let image = source.load(a);
        if image.is_none() {
            missing.push(blake3::hash(a.path.as_bytes()).to_hex().to_string());
        }
        image
    });
    let (mut geometry, mut kinds, mut placeholders, mut properties) = (
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    for u in &face.unsupported {
        add(&mut properties, u.feature.clone());
    }
    let (mut visible, mut interactive, mut bound) = (0, 0, 0);
    for (n, w) in face.widgets.iter().enumerate() {
        let kind = match w.kind {
            ir::Kind::Panel => "ui_panel",
            ir::Kind::Knob { .. } => "ui_knob",
            ir::Kind::Slider { .. } => "ui_slider",
            ir::Kind::Button { .. } => "ui_button",
            ir::Kind::Switch => "ui_switch",
            ir::Kind::Menu { .. } => "ui_menu",
            ir::Kind::Label => "ui_label",
            ir::Kind::ValueEdit { .. } => "ui_value_edit",
            ir::Kind::Table { .. } => "ui_table",
            ir::Kind::Xy { .. } => "ui_xy",
            ir::Kind::Waveform => "ui_waveform",
            ir::Kind::Wavetable { .. } => "ui_wavetable",
            ir::Kind::LevelMeter { .. } => "ui_level_meter",
            ir::Kind::FileSelector { .. } => "ui_file_selector",
            ir::Kind::TextEdit => "ui_text_edit",
            ir::Kind::Image => "image",
            ir::Kind::MouseArea => "ui_mouse_area",
        };
        add(&mut kinds, kind);
        if !face.visible(ir::WidgetRef(n)) {
            continue;
        }
        visible += 1;
        if !matches!(
            w.kind,
            ir::Kind::Panel
                | ir::Kind::Label
                | ir::Kind::Image
                | ir::Kind::Waveform
                | ir::Kind::Wavetable { .. }
                | ir::Kind::LevelMeter { .. }
        ) {
            interactive += 1;
            bound += usize::from(
                matches!(w.binding, ir::Binding::Control(c) if values.contains_key(&c)),
            );
        }
        if matches!(
            w.kind,
            ir::Kind::Table { .. }
                | ir::Kind::Xy { .. }
                | ir::Kind::Waveform
                | ir::Kind::Wavetable { .. }
                | ir::Kind::LevelMeter { .. }
                | ir::Kind::FileSelector { .. }
                | ir::Kind::TextEdit
                | ir::Kind::MouseArea
        ) {
            add(&mut placeholders, kind);
        }
        if w.drag.is_some() {
            add(
                &mut properties,
                "renderer ignores authored drag sensitivity",
            );
        }
        if !w.enabled {
            add(&mut properties, "renderer ignores disabled state");
        }
        if w.text.contains('\n') {
            add(&mut properties, "renderer collapses multiline labels");
        }
        let r = face.page_rect(ir::WidgetRef(n));
        let page = &face.pages[w.page.0];
        if r.width == 0 || r.height == 0 {
            add(&mut geometry, "zero sized visible widget");
        }
        if r.x < 0
            || r.y < 0
            || i64::from(r.x) + i64::from(r.width) > i64::from(page.size.width)
            || i64::from(r.y) + i64::from(r.height) > i64::from(page.size.height)
        {
            add(&mut geometry, "outside authored page candidate");
        }
    }
    let mut renders = Vec::new();
    let initial = values.clone();
    for p in 0..face.pages.len() {
        let start = Instant::now();
        // A 1200x900 viewport, fitted in both axes. Geometry flags use the uncropped source bounds.
        let scale = (1200. / f64::from(face.pages[p].size.width.max(1)))
            .min(900. / f64::from(ir_view::height(&face, ir::PageRef(p)).max(1)))
            .min(1.);
        let w = (f64::from(face.pages[p].size.width.max(1)) * scale)
            .ceil()
            .clamp(1., 1200.) as u16;
        let h = (f64::from(ir_view::height(&face, ir::PageRef(p)).max(1)) * scale)
            .ceil()
            .clamp(1., 900.) as u16;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value, String> {
                let mut ui = theme::ui();
                for _ in 0..2 {
                    let el = ir_view::view(
                        &mut ui,
                        "scan-",
                        &face,
                        ir::PageRef(p),
                        &assets,
                        ir::Presentation::Bitmap,
                        scale,
                        values,
                    );
                    ui.frame(
                        el,
                        Some(Size::new(w as f64, h as f64)),
                        Input::default(),
                        1. / 60.,
                    )
                    .map_err(|_| "layout failed")?;
                }
                let mut ctx = vello::vello_cpu::RenderContext::new(w, h);
                let mut resources = vello::vello_cpu::Resources::default();
                vello::paint(
                    &mut vello::Cpu {
                        ctx: &mut ctx,
                        resources: &mut resources,
                        cache: &mut vello::Cache::default(),
                    },
                    ui.scene().ok_or("no scene")?,
                    vello::kurbo::Affine::IDENTITY,
                )
                .map_err(|_| "paint failed")?;
                ctx.flush();
                let mut pix = vello::vello_cpu::Pixmap::new(w, h);
                ctx.render(&mut pix, &mut resources);
                let rgba: Vec<_> = pix
                    .take_unpremultiplied()
                    .iter()
                    .flat_map(|p| [p.r, p.g, p.b, p.a])
                    .collect();
                let mut report = metrics::pixels(&rgba);
                if std::env::var_os("KONTRA_SCAN_SHOTS").is_some() {
                    let shot = out.join(format!("{prefix}-page-{p}-original.png"));
                    moose::core::screenshot::save_png(&shot, &rgba, w as u32, h as u32);
                    report["shot"] = json!(shot);
                }
                report["size"] = json!([w, h]);
                Ok(report)
            }));
        renders.push(match result {
            Ok(Ok(mut r)) => {
                r["ok"] = json!(true);
                r["ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
                r
            }
            _ => json!({"ok":false,"reason":"Original renderer failure"}),
        });
    }
    let passive = values
        .iter()
        .filter(|(c, v)| initial.get(c).is_some_and(|old| old != *v))
        .count();
    json!({"widgets":face.widgets.len(),"visible":visible,"interactive":interactive,"bound":bound,
        "kinds":kinds,"placeholder_widgets":placeholders,"unsupported_params":properties,"geometry":geometry,
        "missing_images":missing.len(),"missing_image_hashes":missing,"assets":face.assets.len(),
        "decoded_image_bytes":assets.bytes(),"passive_value_changes":passive,"renders":renders})
}

pub fn one(id: &str, out: &Path) -> Value {
    let (path, program_name) = id
        .split_once("::")
        .map_or((id, None), |(p, n)| (p, Some(n)));
    let path = PathBuf::from(path);
    let is_uvi = program_name.is_some()
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("uvip"));
    let path = program_name.map_or(path.clone(), |n| path.join(n));
    let mut result = json!({"loads":"no","ui":"error","controls_bound":"0/0","plays_note":"no","stage":"parse","programs":[]});
    metrics::checkpoint(out, &result);
    let count = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nkm"))
    {
        match sampler_kontakt::read_multi(&path) {
            Ok(m) => m.programs.len(),
            Err(e) => {
                result["failure"] = metrics::error("multi parse", e);
                result["reason"] = json!("multi parse failed");
                return result;
            }
        }
    } else {
        1
    };
    let (
        mut all_load,
        mut total_bound,
        mut total_interactive,
        mut any_heard,
        mut ui_error,
        mut ui_missing,
        mut any_ui,
        mut any_blank,
    ) = (true, 0, 0, false, false, false, false, false);
    let mut load_ms = 0.;
    for program in 0..count {
        let start = Instant::now();
        result["stage"] = json!(format!("load program {program}"));
        metrics::checkpoint(out, &result);
        let request = LoadRequest {
            path: path.clone(),
            program: program as u32,
            sample_rate: 48000.,
            dynamics_start: Some(100),
            threads: None,
            ..Default::default()
        };
        let mut loaded = match V2Loader.prepare(&request, &mut |_| {}, &|| false) {
            Ok(l) => l,
            Err(e) => {
                all_load = false;
                ui_error = true;
                result["programs"]
                    .as_array_mut()
                    .unwrap()
                    .push(metrics::error("production load", e));
                continue;
            }
        };
        load_ms += start.elapsed().as_secs_f64() * 1000.;
        let mut symbols = BTreeMap::<String, usize>::new();
        let mut script_errors = BTreeMap::new();
        if let Some(i) = &loaded.instrument {
            for b in &i.behaviors {
                for (k, v) in metrics::symbols(&b.source) {
                    *symbols.entry(k).or_default() += v;
                }
            }
            for u in &i.unsupported {
                if u.feature.starts_with("script") || u.feature.starts_with("performance view") {
                    add(&mut script_errors, u.feature.clone());
                }
            }
        }
        let mut core = V2Core::with_parts(1, 48000.);
        core.install(0, loaded.part);
        let mut values = ir_view::Values::new();
        for face in &loaded.interfaces {
            for w in &face.widgets {
                if let ir::Binding::Control(c) = w.binding
                    && let Some(v) = core.control_value(0, c)
                {
                    values.insert(c, v);
                }
            }
        }
        result["stage"] = json!(format!("Original UI program {program}"));
        metrics::checkpoint(out, &result);
        let mut source = pictures::Source::of(&path);
        let mut views = Vec::new();
        for (slot, face) in loaded.interfaces.iter().enumerate() {
            if face.widgets.is_empty() {
                continue;
            }
            any_ui = true;
            let view = render(
                face,
                &mut source,
                &mut values,
                out,
                &format!("program-{program}-slot-{slot}"),
            );
            total_bound += view["bound"].as_u64().unwrap_or(0);
            total_interactive += view["interactive"].as_u64().unwrap_or(0);
            ui_missing |= view["missing_images"].as_u64().unwrap_or(0) > 0;
            for r in view["renders"].as_array().unwrap() {
                ui_error |= r["ok"] != true;
                any_blank |= r["uniform"] == true && view["visible"].as_u64().unwrap_or(0) > 0;
            }
            views.push(view);
        }
        // A scalar UI diagnostic does not mean the audio loader failed.
        let failed_script = script_errors.get("script").copied().unwrap_or(0) > 0
            || script_errors.get("script interface").copied().unwrap_or(0) > 0;
        ui_error |= failed_script;
        let pick = loaded.instrument.as_ref().and_then(|i| {
            let switch: std::collections::BTreeSet<_> = i
                .articulations
                .iter()
                .flat_map(|a| a.switch_keys.iter().copied())
                .collect();
            let best = (0..=127u8)
                .filter(|k| !switch.contains(k))
                .map(|k| {
                    let n = i
                        .zones
                        .iter()
                        .filter(|z| {
                            (z.keys.low..=z.keys.high).contains(&k)
                                && (z.velocities.low..=z.velocities.high).contains(&64)
                        })
                        .count();
                    (n, std::cmp::Reverse(k.abs_diff(60)), k)
                })
                .max()
                .filter(|(n, ..)| *n > 0)
                .map(|(.., k)| (k, 64));
            best.or_else(|| {
                i.zones
                    .iter()
                    .find(|z| !switch.contains(&z.keys.low))
                    .map(|z| {
                        (
                            z.keys.low,
                            ((u16::from(z.velocities.low) + u16::from(z.velocities.high)) / 2)
                                .max(1) as u8,
                        )
                    })
            })
        });
        let mut heard = false;
        result["stage"] = json!(format!("play program {program}"));
        metrics::checkpoint(out, &result);
        if let Some((key, velocity)) = pick {
            core.event(0, Event::midi1(0xb0, 1, 100));
            core.event(0, Event::midi1(0xb0, 11, 127));
            core.event(0, Event::midi1(0x90, key, velocity));
            for _ in 0..180 {
                std::thread::sleep(Duration::from_millis(3));
                let audio = core.render(128);
                if audio.buses.iter().any(|bus| {
                    bus.iter()
                        .flatten()
                        .any(|x| x.is_finite() && x.abs() > 1e-5)
                }) {
                    heard = true;
                    break;
                }
            }
        }
        any_heard |= heard;
        result["programs"].as_array_mut().unwrap().push(json!({"program":program,"loaded":true,"source":if is_uvi {"uvi"}else{"kontakt"},"script_errors":script_errors,"symbols":symbols,"views":views,"plays_note":if heard {"yes"}else{"silent"},"pick":pick,"load_ms":start.elapsed().as_secs_f64()*1000.}));
        // Keep the streaming owner alive throughout the note probe.
        loaded.stream.take();
    }
    result["loads"] = json!(if all_load && count > 0 { "yes" } else { "no" });
    result["ui"] = json!(if ui_error {
        "error"
    } else if any_blank {
        "blank"
    } else if ui_missing {
        "missing-images"
    } else if !any_ui {
        "no-ui"
    } else {
        "original-ok"
    });
    result["controls_bound"] = json!(format!("{total_bound}/{total_interactive}"));
    result["plays_note"] = json!(if any_heard {
        "yes"
    } else if all_load {
        "silent"
    } else {
        "no"
    });
    result["load_ms"] = json!(load_ms);
    result["reason"] = json!(format!(
        "{count} programs; Original only; bound {total_bound}/{total_interactive}; audio {}",
        if any_heard {
            "audible"
        } else {
            "not audible in 0.5s probe"
        }
    ));
    result["stage"] = json!("complete");
    result
}
