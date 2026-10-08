//! Read-only shared scanner adapter. Only the Original authored presentation is rendered.
use super::{ir_view, theme};
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
    path: &Path,
    interfaces: &[ir::Interface],
    values: &mut ir_view::Values,
    out: &Path,
    prefix: &str,
) -> Value {
    let face = ir_view::resolved(face);
    let mut missing = Vec::new();
    let mut assets = ir_view::Assets::default();
    let mut native = face.native_ui.as_ref().map(|n| {
        super::native_ui::State::new(
            path,
            &n.entry,
            interfaces
                .iter()
                .flat_map(|f| {
                    f.widgets
                        .iter()
                        .enumerate()
                        .map(move |(n, w)| (f.source, n, w.clone()))
                })
                .collect(),
        )
    });
    let input = ir_view::InputState::default();
    let (mut geometry, mut kinds, mut placeholders, mut properties) = (
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
    );
    for u in &face.unsupported {
        add(&mut properties, u.feature.clone());
    }
    let (mut visible, mut interactive, mut bound, mut declared, mut declared_bound) =
        (0, 0, 0, 0, 0);
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
        if !matches!(
            w.kind,
            ir::Kind::Panel
                | ir::Kind::Label
                | ir::Kind::Image
                | ir::Kind::Waveform
                | ir::Kind::Wavetable { .. }
                | ir::Kind::LevelMeter { .. }
        ) {
            declared += 1;
            declared_bound += usize::from(
                matches!(w.binding, ir::Binding::Control(c) if values.contains_key(&c)),
            );
        }
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
        let authored = native.as_ref().map(|n| n.authored());
        let w = (authored.map_or(f64::from(face.pages[p].size.width.max(1)), |s| s.width) * scale)
            .ceil()
            .clamp(1., 1200.) as u16;
        let h = (authored.map_or(
            f64::from(ir_view::height(&face, ir::PageRef(p)).max(1)),
            |s| s.height,
        ) * scale)
            .ceil()
            .clamp(1., 900.) as u16;
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value, String> {
                let mut ui = theme::ui();
                let deadline = Instant::now() + Duration::from_secs(15);
                let mut settled = 0;
                while settled < 2 {
                    if Instant::now() > deadline {
                        return Err("authored image preparation time budget exceeded".into());
                    }
                    let el = if let Some(native) = &mut native {
                        native.view(&mut ui, 0, scale, &face, values, &input)
                    } else {
                        assets.prepare(
                            path,
                            &face,
                            ir::PageRef(p),
                            ir::Presentation::Bitmap,
                            scale,
                            values,
                        );
                        ir_view::view(
                            &mut ui,
                            &face,
                            ir::PageRef(p),
                            &assets,
                            ir::Presentation::Bitmap,
                            scale,
                            values,
                        )
                    };
                    ui.frame(
                        el,
                        Some(Size::new(w as f64, h as f64)),
                        Input::default(),
                        1. / 60.,
                    )
                    .map_err(|e| e.to_string())?;
                    if let Some(error) = native.as_ref().and_then(|n| n.diagnostic()) {
                        return Err(error);
                    }
                    let pending = native
                        .as_ref()
                        .map_or_else(|| assets.pending(), |n| n.pending());
                    if pending == 0 {
                        settled += 1;
                    } else {
                        settled = 0;
                        std::thread::sleep(Duration::from_millis(2));
                    }
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
                .map_err(|e| e.to_string())?;
                ctx.flush();
                let mut pix = vello::vello_cpu::Pixmap::new(w, h);
                ctx.render(&mut pix, &mut resources);
                let rgba: Vec<_> = pix
                    .take_unpremultiplied()
                    .iter()
                    .flat_map(|p| [p.r, p.g, p.b, p.a])
                    .collect();
                let mut report = metrics::pixels(&rgba);
                let color = face.pages[p].background.color.map(|c| [c.r, c.g, c.b, c.a]);
                report["background"] = metrics::background(&rgba, color);
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
            Ok(Err(e)) => {
                json!({"ok":false,"budget_hit":metrics::budget(&e),"reason":metrics::message(&e)})
            }
            Err(_) => json!({"ok":false,"budget_hit":false,"reason":"Original renderer panic"}),
        });
    }
    let passive = values
        .iter()
        .filter(|(c, v)| initial.get(c).is_some_and(|old| old != *v))
        .count();
    let fonts_declared = face
        .styles
        .iter()
        .filter(|s| !matches!(s.font, ir::Font::Default))
        .count();
    let scan = native.as_ref().map_or_else(|| assets.scan(), |n| n.scan());
    missing.extend(
        native
            .as_ref()
            .map_or_else(|| assets.failures(), |n| n.failures()),
    );
    missing.sort();
    missing.dedup();
    let font_success = face
        .styles
        .iter()
        .filter(|s| match s.font {
            ir::Font::Stock(_) => true,
            ir::Font::Default | ir::Font::Named(_) => false,
            ir::Font::Bitmap(a) => assets.get(a).is_some(),
            ir::Font::File(a) => assets.font(&a).is_some(),
        })
        .count();
    json!({"controls_declared":declared,"controls_bound_declared":declared_bound,
        "asset_lookup_requested":scan.lookups,"asset_lookup_ok":scan.lookup_ok,
        "asset_decode_requested":scan.decodes,"asset_decode_ok":scan.decode_ok,
        "font_declared":fonts_declared,"font_success":font_success,
        "custom_font_uses":face.styles.iter().filter(|s|matches!(s.font,ir::Font::Named(_)|ir::Font::Bitmap(_))).count(),
        "image_strips":face.assets.iter().filter(|a|matches!(&a.kind,ir::AssetKind::Image(m)if m.frames>1)).count(),
        "image_frames":face.assets.iter().filter_map(|a|if let ir::AssetKind::Image(m)=&a.kind{Some(m.frames.max(1))}else{None}).sum::<u32>(),
        "image_margins":face.assets.iter().filter(|a|matches!(&a.kind,ir::AssetKind::Image(m)if m.margins!=ir::Margins::default())).count(),
        "asset_failure_reasons":{"lookup-not-found":(scan.lookups)-(scan.lookup_ok),
            "decode-failed":(scan.decodes)-(scan.decode_ok),"font-service-unavailable":fonts_declared.saturating_sub(font_success)},
        "widgets":face.widgets.len(),"visible":visible,"interactive":interactive,"bound":bound,
        "kinds":kinds,"placeholder_widgets":placeholders,"unsupported_params":properties,"geometry":geometry,
        "missing_images":missing.len(),"missing_image_hashes":missing,"assets":face.assets.len(),
        "decoded_image_bytes":native.as_ref().map_or_else(||assets.bytes(),|n|n.bytes()),"passive_value_changes":passive,"renders":renders})
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
    if !is_uvi {
        result["metadata"] = match sampler_kontakt::read_chunks(&path) {
            Ok(chunks) => metrics::metadata::inspect(&chunks.0),
            Err(e) => metrics::error("script metadata parse", e),
        };
    }
    sampler_ksp::scan::begin();
    sampler_uvi::script::scan_load_error();
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
        mut budget_hit,
    ) = (true, 0, 0, false, false, false, false, false, false);
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
                let ksp = ksp_observations();
                result["programs"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()["ksp"] = ksp;
                if let Some(e) = sampler_uvi::script::scan_load_error() {
                    let p = result["programs"]
                        .as_array_mut()
                        .unwrap()
                        .last_mut()
                        .unwrap();
                    p["lua"] = json!({"init_faults":1,"runtime_faults":0,"init_first":metrics::message(&e),"budget_hits":usize::from(e.contains("time budget exceeded"))});
                    p["load_path"] = json!("scripted-worker");
                }
                continue;
            }
        };
        let ksp = ksp_observations();
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
        let mut admitted = std::collections::BTreeMap::<String, usize>::new();
        if let Some(i) = &loaded.instrument {
            for b in &i.behaviors {
                for (name, _) in &b.state {
                    *admitted
                        .entry(metrics::metadata::sigil(name.as_bytes()).into())
                        .or_default() += 1;
                }
            }
        }
        let sample_resident_bytes = loaded.stream.as_ref().map(|s| s.resident_bytes());
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
        let mut views = Vec::new();
        for (slot, face) in loaded.interfaces.iter().enumerate() {
            if face.widgets.is_empty() {
                continue;
            }
            any_ui = true;
            let view = render(
                face,
                &path,
                &loaded.interfaces,
                &mut values,
                out,
                &format!("program-{program}-slot-{slot}"),
            );
            total_bound += view["bound"].as_u64().unwrap_or(0);
            total_interactive += view["interactive"].as_u64().unwrap_or(0);
            ui_missing |= view["missing_images"].as_u64().unwrap_or(0) > 0;
            for r in view["renders"].as_array().unwrap() {
                ui_error |= r["ok"] != true;
                budget_hit |= r["budget_hit"] == true;
                any_blank |= r["uniform"] == true && view["visible"].as_u64().unwrap_or(0) > 0;
            }
            views.push(view);
        }
        // A scalar UI diagnostic does not mean the audio loader failed.
        let failed_script = script_errors.get("script").copied().unwrap_or(0) > 0
            || script_errors.get("script interface").copied().unwrap_or(0) > 0;
        ui_error |= failed_script;
        let native = core.scan_lua(0);
        let mut native_valid: std::collections::BTreeSet<u8> = native
            .as_ref()
            .map(|n| n.native_valid_keys.iter().copied().collect())
            .unwrap_or_default();
        let mut native_invalid: std::collections::BTreeSet<u8> = native
            .as_ref()
            .map(|n| n.native_invalid_keys.iter().copied().collect())
            .unwrap_or_default();
        for view in &loaded.scripts.views {
            for (key, k) in view.model().interface.keys.iter().enumerate() {
                if matches!(k.kind, Some(1 | 2)) || k.color == Some(17) {
                    native_invalid.insert(key as u8);
                } else if matches!(k.color, Some(18 | 19)) {
                    native_valid.insert(key as u8);
                }
            }
        }
        let candidate = (|| {
            loaded.instrument.as_ref().and_then(|i| {
                let switch: std::collections::BTreeSet<_> = i
                    .articulations
                    .iter()
                    .flat_map(|a| a.switch_keys.iter().copied())
                    .collect();
                let mut valid: std::collections::BTreeSet<_> = native
                    .as_ref()
                    .map(|n| n.native_valid_keys.iter().copied().collect())
                    .unwrap_or_default();
                let mut invalid: std::collections::BTreeSet<_> = native
                    .as_ref()
                    .map(|n| n.native_invalid_keys.iter().copied().collect())
                    .unwrap_or_default();
                for view in &loaded.scripts.views {
                    for (key, k) in view.model().interface.keys.iter().enumerate() {
                        if matches!(k.kind, Some(1 | 2)) || k.color == Some(17) {
                            invalid.insert(key as u8);
                        } else if matches!(k.color, Some(18 | 19)) {
                            valid.insert(key as u8);
                        }
                    }
                }
                let best = (0..=127u8)
                    .filter(|k| !switch.contains(k) && !invalid.contains(k))
                    .map(|k| {
                        let n = i
                            .zones
                            .iter()
                            .filter(|z| {
                                (z.keys.low..=z.keys.high).contains(&k)
                                    && (z.velocities.low..=z.velocities.high).contains(&64)
                            })
                            .count();
                        (
                            n > 0,
                            valid.contains(&k),
                            std::cmp::Reverse(k.abs_diff(60)),
                            n,
                            k,
                        )
                    })
                    .max()
                    .filter(|(mapped, ..)| *mapped)
                    .map(|(.., k)| (k, 64));
                best.or_else(|| {
                    i.zones
                        .iter()
                        .find(|z| !switch.contains(&z.keys.low) && !invalid.contains(&z.keys.low))
                        .map(|z| {
                            (
                                z.keys.low,
                                ((u16::from(z.velocities.low) + u16::from(z.velocities.high)) / 2)
                                    .max(1) as u8,
                            )
                        })
                })
            })
        })();
        let pick = metrics::note(program as u32).or(candidate);
        let pick_source = match pick {
            Some((key, 64)) if native_valid.contains(&key) && !native_invalid.contains(&key) => {
                "native_declared"
            }
            Some((_, 64)) => "zone_coverage",
            _ => "fallback",
        };
        let mut runtime_faults = Vec::new();
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
                }
                runtime_faults.extend(core.scan_runtime_faults(0));
            }
        }
        any_heard |= heard;
        let lua = core.scan_lua(0);
        let lua_report = lua.as_ref().map(|l| json!({"init_faults":l.init_count,"runtime_faults":l.runtime_count,
            "init_first":l.init_first.as_deref().map(metrics::message),"runtime_first":l.runtime_first.as_deref().map(metrics::message),"budget_hits":l.budget_hits}));
        result["programs"].as_array_mut().unwrap().push(json!({"ksp":ksp,"ksp_runtime_faults":runtime_faults.iter().map(|(program,outcome)|json!({"program":program,"callback":sampler_ksp::callback_of(&loaded.scripts.views,*program),"category":match outcome{sampler_core::Outcome::FuelExhausted=>"fuel-budget",_=>"runtime-fault"},"core_error":match outcome{sampler_core::Outcome::Fault(e)=>Some(format!("{e:?}")),_=>None}})).collect::<Vec<_>>(),"lua":lua_report,"admitted_saved_entries_by_sigil":admitted,
            "load_path":if is_uvi {if lua.is_some(){"scripted-worker"}else{"offline-loader"}}else{"kontakt-v2-loader"},
            "sample_resident_bytes":sample_resident_bytes,"underruns":core.problems(0).underruns,
            "pick_source":pick_source,"native_valid_keys":native_valid,"native_key_conflicts":native.as_ref().map(|n|n.native_key_conflicts),"native_preferred_note":candidate.filter(|(k,_)|native_valid.contains(k)),
            "program":program,"loaded":true,"source":if is_uvi {"uvi"}else{"kontakt"},"script_errors":script_errors,"symbols":symbols,"views":views,"plays_note":if heard {"yes"}else{"silent"},"pick":pick,"load_ms":start.elapsed().as_secs_f64()*1000.}));
        // Keep the streaming owner alive throughout the note probe.
        loaded.stream.take();
    }
    result["loads"] = json!(if all_load && count > 0 { "yes" } else { "no" });
    result["ui"] = json!(if budget_hit {
        "budget-hit"
    } else if ui_error {
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

fn ksp_observations() -> Value {
    let attempts = sampler_ksp::scan::take();
    let obs: Vec<_> = attempts
        .iter()
        .filter(|o| o.attempt == "runtime-preparation")
        .collect();
    let compile = if obs.is_empty() {
        "no-scripts"
    } else if obs.iter().all(|s| s.compile_ok) {
        "yes"
    } else {
        "no"
    };
    let init = if obs.is_empty() {
        "no-scripts"
    } else if obs.iter().any(|s| s.init_ok == Some(false)) {
        "no"
    } else if obs.iter().all(|s| s.init_ok == Some(true)) {
        "yes"
    } else {
        "unknown"
    };

    let diagnostic = |e: &sampler_ksp::scan::Diagnostic| json!({"phase":e.phase,"kind":e.kind,"category":e.category,"builtin":e.builtin,"offset":e.offset,"line":e.line,"column":e.column,"inactive_region":"unknown"});
    let phase = |p: &sampler_ksp::scan::Phase| json!({"present":p.present,"completion":p.completion,"status":match p.completion {"not_present"=>"absent","not_reached"=>"not_started","failed" if p.fault.as_ref().is_some_and(|f|f.category=="fuel-budget")=>"budget_stopped","failed"=>"faulted",_=>"completed"},"fault":p.fault.as_ref().map(&diagnostic)});
    let first = obs
        .iter()
        .find_map(|s| {
            s.error
                .as_ref()
                .or(s.init.fault.as_ref())
                .or(s.persistence_changed.fault.as_ref())
        })
        .map(|e| {
            format!(
                "{} {} {}:{} builtin={}",
                e.phase,
                e.category,
                e.line,
                e.column,
                e.builtin.unwrap_or("none")
            )
        });
    let slots:Vec<_>=obs.iter().map(|o|json!({"owner":"program","slot":o.slot,"wire_slot":o.slot,"runtime_slot":o.slot,"compile_ok":o.compile_ok,"compile_admitted":o.compile_ok,"compile_clean":o.compile_ok,"attempt":o.attempt,
        "compile_fault":o.error.as_ref().filter(|e|!matches!(e.phase,"init"|"persistence_changed")).map(&diagnostic),
        "init":phase(&o.init),"persistence_changed":phase(&o.persistence_changed)})).collect();
    json!({"compile_ok":compile,"init_ok":init,"first_error":first,"scripts":obs.len(),"slots":slots,"attempts":attempts.iter().map(|o|json!({"attempt":o.attempt,"wire_slot":o.slot,"compile_admitted":o.compile_ok,"init":phase(&o.init),"persistence_changed":phase(&o.persistence_changed),"error":o.error.as_ref().map(&diagnostic)})).collect::<Vec<_>>()})
}
