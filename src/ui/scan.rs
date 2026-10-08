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

#[path = "scan_coverage.rs"]
mod coverage;

fn add(counts: &mut BTreeMap<String, usize>, key: impl Into<String>) {
    *counts.entry(key.into()).or_default() += 1;
}

// Validate the legacy reader's public bytes with the existing strict script parser.
pub(crate) fn strict_table(data: &[u8], version: u16) -> (&'static str,BTreeMap<String,usize>) {
    let mut body=vec![0];body.extend(version.to_le_bytes());body.extend(data);
    let mut wire=6u16.to_le_bytes().to_vec();
    let Ok(len)=u32::try_from(body.len()) else{return ("unknown",BTreeMap::new())};
    wire.extend(len.to_le_bytes());wire.extend(body);
    let limits=sampler_kontakt::Limits{bytes:wire.len(),records:data.len()/4+1};
    let parsed=sampler_kontakt::Chunks::parse(&wire,limits).and_then(|c|sampler_kontakt::Script::parse(c.iter().next().unwrap(),limits));
    match parsed {
        Ok(script)=>match script.persistent {
            None=>("absent",BTreeMap::new()),
            Some(entries)=>{let mut tags=BTreeMap::new();for e in entries.iter(){add(&mut tags,metrics::metadata::sigil(e.data()));}("decoded",tags)}
        },
        Err(_)=>("malformed",BTreeMap::new()),
    }
}

// Preparation publishes the current page; scanner success spans every painted page.
fn observed_font_styles(styles:&[ir::TextStyle],ready:&std::collections::BTreeSet<usize>)->usize {
    styles.iter().filter(|s| match s.font {
        ir::Font::Stock(_) => true,
        ir::Font::Default | ir::Font::Named(_) => false,
        ir::Font::Bitmap(a) | ir::Font::File(a) => ready.contains(&a.0),
    }).count()
}

fn render(
    face: &ir::Interface,
    path: &Path,
    interfaces: &[ir::Interface],
    values: &mut ir_view::Values,
    typed_targets: &std::collections::BTreeSet<(u8,String)>,
    load_started: Instant,
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
        let token=u.feature.strip_suffix("[]").unwrap_or(&u.feature);
        if include_str!("../../tools/kontra-scan/ui-symbols.txt").lines().any(|name|name==token) {
            add(&mut properties,u.feature.clone());
        } else { add(&mut properties,"unsupported UI feature (private identifier omitted)"); }
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
    let typed_refs=face.widgets.iter().enumerate().filter(|(n,w)|face.visible(ir::WidgetRef(*n)) && matches!(w.binding,ir::Binding::Variable{..})).count();
    let typed_bound=face.widgets.iter().enumerate().filter(|(n,w)|face.visible(ir::WidgetRef(*n)) && matches!(&w.binding,ir::Binding::Variable{script,name} if typed_targets.contains(&(*script,name.clone())))).count();
    let mut renders = Vec::new();
    let mut ready_fonts = std::collections::BTreeSet::new();
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
                #[cfg(target_os = "linux")]
                let watermark = std::env::var_os("KONTRA_SCAN_STACK").map(|_| super::native_ui::stack_watermark());
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
                let first_frame_ms=load_started.elapsed().as_secs_f64()*1000.;
                let rgba: Vec<_> = pix
                    .take_unpremultiplied()
                    .iter()
                    .flat_map(|p| [p.r, p.g, p.b, p.a])
                    .collect();
                let mut report = metrics::pixels(&rgba);
                report["ui_first_frame_ms"]=json!(first_frame_ms);
                let color = face.pages[p].background.color.map(|c| [c.r, c.g, c.b, c.a]);
                report["background"] = metrics::background(&rgba, color);
                if std::env::var_os("KONTRA_SCAN_SHOTS").is_some() {
                    let shot = out.join(format!("{prefix}-page-{p}-original.png"));
                    moose::core::screenshot::save_png(&shot, &rgba, w as u32, h as u32);
                    report["shot"] = json!(shot);
                }
                report["size"] = json!([w, h]);
                #[cfg(target_os = "linux")]
                if let Some(watermark) = watermark {
                    let peak = super::native_ui::stack_peak(watermark);
                    report["stack_peak_bytes"] = json!(peak);
                    report["stack_watermark_unmarked_top_bytes"] = json!(watermark.2 - watermark.1);
                    report["stack_watermark_saturated"] = json!(peak >= watermark.2 - watermark.0 - 4096);
                }
                Ok(report)
            }));
        renders.push(match result {
            Ok(Ok(mut r)) => {
                r["ok"] = json!(true);
                r["ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
                r
            }
            Ok(Err(e)) => {
                json!({"ok":false,"budget_hit":metrics::budget(&e),"reason":native.as_ref().and_then(|n|n.diagnostic()).unwrap_or_else(||metrics::message(&e))})
            }
            Err(_) => json!({"ok":false,"budget_hit":false,"reason":"Original renderer panic"}),
        });
        missing.extend(native.as_ref().map_or_else(|| assets.failures(),|n|n.failures()));
        for (i,asset) in face.assets.iter().enumerate() {
            let ready=match asset.kind {
                ir::AssetKind::TrueTypeFont => assets.font(&ir::AssetRef(i)).is_some(),
                ir::AssetKind::BitmapFont => assets.get(ir::AssetRef(i)).is_some(),
                _ => false,
            };
            if ready { ready_fonts.insert(i); }
        }
    }
    let passive = values
        .iter()
        .filter(|(c, v)| initial.get(c).is_some_and(|old| old != *v))
        .count();
    let legacy_fonts_declared = face
        .styles
        .iter()
        .filter(|s| !matches!(s.font, ir::Font::Default))
        .count();
    let scan = native.as_ref().map_or_else(|| assets.scan(), |n| n.scan());
    missing.sort();
    missing.dedup();
    let legacy_font_success = observed_font_styles(&face.styles,&ready_fonts);
    let native_fonts=native.as_ref().and_then(|n|n.font_success());
    let fonts_declared=if native.is_some(){native_fonts}else{Some(legacy_fonts_declared)};
    let font_success=if native.is_some(){native_fonts}else{Some(legacy_font_success)};
    let resources_known=native.is_none()||native_fonts.is_some();
    // The worker hashes asset identity, including its kind. Separate font requests.
    let font_hashes: std::collections::BTreeSet<_> = face.assets.iter()
        .filter(|a| matches!(a.kind,ir::AssetKind::TrueTypeFont|ir::AssetKind::BitmapFont))
        .map(|a| blake3::hash(format!("{}:{:?}",a.path,a.kind).as_bytes()).to_hex().to_string())
        .collect();
    let missing_font_hashes:Vec<_>=missing.iter().filter(|h|font_hashes.contains(*h)).cloned().collect();
    missing.retain(|h| !font_hashes.contains(h));
    let missing_fonts=resources_known.then(|| scan.fonts.saturating_sub(scan.font_ok).max(missing_font_hashes.len()));
    let mut failures=BTreeMap::new();
    if resources_known {
        failures.insert("lookup-not-found",scan.lookup_missing);
        failures.insert("lookup-invalid",scan.lookup_invalid);
        failures.insert("lookup-ambiguous",scan.lookup_ambiguous);
        failures.insert("lookup-corrupt",scan.lookup_corrupt);
        failures.insert("lookup-limit",scan.lookup_limit);
        failures.insert("lookup-read",scan.lookup_read);
        failures.insert("lookup-unavailable",scan.lookup_unavailable);
        failures.insert("decode-failed",scan.decodes-scan.decode_ok);
    }
    if let Some(missing_fonts)=missing_fonts {
        failures.insert("font-service-unavailable",missing_fonts);
    }
    json!({"bound_typed":if matches!(face.source,ir::Source::FalconLua){None}else{Some(typed_bound)},"typed_binding_refs":typed_refs,"typed_binding_basis":"installed script model target; live typed edit/readback unmeasured","phantom_free_controls":null,"controls_declared":declared,"controls_bound_declared":declared_bound,
        "asset_lookup_requested":resources_known.then_some(scan.lookups),"asset_lookup_ok":resources_known.then_some(scan.lookup_ok),
        "asset_decode_requested":resources_known.then_some(scan.decodes),"asset_decode_ok":resources_known.then_some(scan.decode_ok),
        "font_declared":fonts_declared,"font_success":font_success,"font_unresolved_styles":fonts_declared.zip(font_success).map(|(declared,success)|declared.saturating_sub(success)),

        "custom_font_uses":if native.is_some(){None}else{Some(face.styles.iter().filter(|s|matches!(s.font,ir::Font::Named(_)|ir::Font::Bitmap(_)|ir::Font::File(_))).count())},
        "image_strips":face.assets.iter().filter(|a|matches!(&a.kind,ir::AssetKind::Image(m)if m.frames>1)).count(),
        "image_frames":face.assets.iter().filter_map(|a|if let ir::AssetKind::Image(m)=&a.kind{Some(m.frames.max(1))}else{None}).sum::<u32>(),
        "image_margins":face.assets.iter().filter(|a|matches!(&a.kind,ir::AssetKind::Image(m)if m.margins!=ir::Margins::default())).count(),
        "asset_failure_reasons":failures,"source_presentation":if native.is_some(){"native-package"}else{"legacy-authored"},"native_frontend_consumed":native.as_ref().map(|_|!renders.is_empty()),"native_paint_ok":native.as_ref().map(|_|renders.iter().any(|r|r["ok"]==true)),
        "native_diagnostic":native.as_ref().and_then(|n|n.diagnostic()),
        "native_graph_depth":native.as_ref().and_then(|n|n.graph_depth()),
        "widgets":face.widgets.len(),"visible":visible,"interactive":interactive,"bound":bound,
        "kinds":kinds,"placeholder_widgets":placeholders,"unsupported_params":properties,"geometry":geometry,
        "missing_images":missing.len(),"missing_image_hashes":missing,"missing_fonts":missing_fonts,"missing_font_hashes":missing_font_hashes,"assets":face.assets.len(),
        "decoded_image_bytes":native.as_ref().map_or_else(||assets.bytes(),|n|n.bytes()),"passive_value_changes":passive,"renders":renders})
}

pub fn one(id: &str, out: &Path) -> Value {
    let mut first_audio_ms=None;
    let (path, program_name) = id
        .split_once("::")
        .map_or((id, None), |(p, n)| (p, Some(n)));
    let path = PathBuf::from(path);
    let is_uvi = program_name.is_some()
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("uvip"));
    let path = program_name.map_or(path.clone(), |n| path.join(n));
    let mut result = json!({"loads":"no","ui":"error","controls_bound":"0/0","plays_note":"no","stage":"parse","programs":[],"cache_state":"cold","cache_state_basis":"frozen v2 baseline has no product metadata cache; OS page cache uncontrolled"});
    if !is_uvi {
        result["metadata"] = match sampler_kontakt::read_chunks(&path) {
            Ok(chunks) => metrics::metadata::inspect_with(&chunks.0,strict_table),
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
        mut ui_missing_font,
        mut any_ui,
        mut any_blank,
        mut budget_hit,
    ) = (true, 0, 0, false, false, false, false, false, false, false);
    let mut load_ms = 0.;
    let load_started=Instant::now();
    result["onset_basis"]=json!("monotonic from first production program import; shared collector paints Original and auditions concurrently; first output excludes lexical metadata prepass");
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
        let dsp_slots = loaded.instrument.as_ref().map(|i|coverage::slots(i)).unwrap_or(json!({"complete":false}));
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
        let typed_targets=loaded.scripts.views.iter().flat_map(|view|view.model().interface.widgets.iter().filter(|w|matches!(w.value,sampler_ksp::model::WidgetValue::Text(_)|sampler_ksp::model::WidgetValue::Ints(_)|sampler_ksp::model::WidgetValue::Reals(_))).map(|w|(view.slot(),w.name.clone()))).collect();
        let faces = loaded.interfaces.clone();
        any_ui |= faces.iter().any(|face| !face.widgets.is_empty() || face.native_ui.is_some());
        let paint_path = path.clone();
        let paint_out = out.to_path_buf();
        let paint = std::thread::Builder::new().stack_size(32 << 20).spawn(move || {

        let mut views = Vec::new();
        for (slot, face) in faces.iter().enumerate() {
            if face.widgets.is_empty() && face.native_ui.is_none() {
                continue;
            }
            let view = render(
                face,
                &paint_path,
                &faces,
                &mut values,
                &typed_targets,
                load_started,
                &paint_out,
                &format!("program-{program}-slot-{slot}"),
            );
            views.push(view);
        }
        views
        }).expect("paint worker start");
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
                best
            })
        })();
        let mut excluded = native_invalid.clone();
        if let Some(i)=&loaded.instrument { excluded.extend(i.articulations.iter().flat_map(|a|a.switch_keys.iter().copied())); }
        let pick = metrics::note(program as u32).or(candidate).or_else(||metrics::fallback_note(&excluded));
        let pick_source = match pick {
            Some((key, 64)) if candidate==pick && native_valid.contains(&key) && !native_invalid.contains(&key) => {
                "native_declared"
            }
            Some((_, 64)) if candidate==pick => "zone_coverage",
            _ => "fallback",
        };
        let declared_switch=loaded.instrument.as_ref().and_then(|i|i.articulations.iter().flat_map(|a|a.switch_keys.iter().copied()).min())
            .or_else(||loaded.scripts.views.iter().flat_map(|v|v.model().interface.keys.iter().enumerate()).find(|(_,k)|k.kind==Some(1)&&k.color!=Some(17)).map(|(key,_)|key as u8));
        let keyswitch=metrics::planned_keyswitch(program as u32).unwrap_or(declared_switch);
        let sample_zone_count=loaded.instrument.as_ref().map(|i|i.zones.len());
        let mut runtime_faults = Vec::new();
        let mut heard = false;
        // Diagnostic repeats stay opt-in so gate load/onset timings keep their protocol.
        let family_repeats = std::env::var("KONTRA_SCAN_FAMILY_REPEATS").ok().and_then(|v|v.parse::<usize>().ok()).unwrap_or(0).min(128);
        let mut family_takes = Vec::new();
        result["stage"] = json!(format!("play program {program}"));
        metrics::checkpoint(out, &result);
        if let Some((key, velocity)) = pick {
            core.event(0, Event::midi1(0xb0, 1, 100));
            core.event(0, Event::midi1(0xb0, 11, 127));
            if let Some(switch)=keyswitch {
                core.event(0, Event::midi1(0x90,switch,64));
                let audio=core.render(128);
                if first_audio_ms.is_none() && metrics::nonzero(audio.buses.iter().flat_map(|bus|bus.iter().flat_map(|channel|channel.iter().take(128).copied()))) {first_audio_ms=Some(load_started.elapsed().as_secs_f64()*1000.);}
                core.event(0, Event::midi1(0x80,switch,0));
            }
            core.event(0, Event::midi1(0x90, key, velocity));
            for _ in 0..180 {
                std::thread::sleep(Duration::from_millis(3));
                let audio = core.render(128);
                if first_audio_ms.is_none() && metrics::nonzero(audio.buses.iter().flat_map(|bus|bus.iter().flat_map(|channel|channel.iter().take(128).copied()))) {first_audio_ms=Some(load_started.elapsed().as_secs_f64()*1000.);}
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
        if let Some((key, velocity)) = pick {
            if family_repeats > 0 {
                core.event(0, Event::midi1(0x80, key, 0));
                core.scan_record_selections(0, true);
                for repeat in 0..family_repeats {
                    core.event(0, Event::midi1(0x90, key, velocity));
                    for _ in 0..24 {
                        std::thread::sleep(Duration::from_millis(3));
                        core.render(128);
                        runtime_faults.extend(core.scan_runtime_faults(0));
                    }
                    let attack = coverage::selections(core.scan_selections(0));
                    core.event(0, Event::midi1(0x80, key, 0));
                    for _ in 0..24 {
                        std::thread::sleep(Duration::from_millis(3));
                        core.render(128);
                        runtime_faults.extend(core.scan_runtime_faults(0));
                    }
                    family_takes.push(json!({"repeat":repeat,"attack":attack,"release":coverage::selections(core.scan_selections(0))}));
                }
                core.scan_record_selections(0, false);
            }
        }
        result["stage"]=json!(format!("Original paint join program {program}"));
        metrics::checkpoint(out,&result);
        let views=paint.join().unwrap_or_else(|_|vec![json!({"renders":[{"ok":false,"budget_hit":false,"reason":"paint worker panicked"}]})]);
        for view in &views {
            total_bound += view["bound"].as_u64().unwrap_or(0);
            total_interactive += view["interactive"].as_u64().unwrap_or(0);
            ui_missing |= view["missing_images"].as_u64().unwrap_or(0) > 0;
            ui_missing_font |= view["missing_fonts"].as_u64().unwrap_or(0) > 0;
            for r in view["renders"].as_array().unwrap() {
                ui_error |= r["ok"] != true;
                budget_hit |= r["budget_hit"] == true;
                any_blank |= r["uniform"] == true && view["visible"].as_u64().unwrap_or(0) > 0;
            }
        }
        any_heard |= heard;
        let mut view_requests=BTreeMap::new();
        for view in &loaded.scripts.views { for request in &view.model().requests {
            if matches!(request.command,"load_native_ui"|"load_komplete_ui"|"load_performance_view") {add(&mut view_requests,request.command);}
        }}
        let native_requested=view_requests.contains_key("load_native_ui")||view_requests.contains_key("load_komplete_ui");
        let native_consumed=native_requested.then(||views.iter().any(|v|v["native_frontend_consumed"]==true));
        let lua = core.scan_lua(0);
        let lua_report = lua.as_ref().map(|l| json!({"init_faults":l.init_count,"runtime_faults":l.runtime_count,
            "init_first":l.init_first.as_deref().map(metrics::message),"runtime_first":l.runtime_first.as_deref().map(metrics::message),"budget_hits":l.budget_hits}));
        result["programs"].as_array_mut().unwrap().push(json!({"family_takes":family_takes,"dsp_slots":dsp_slots,"authored_view_requests":view_requests,"native_frontend_consumed":native_consumed,"ksp":ksp,"ksp_runtime_faults":runtime_faults.iter().map(|(program,outcome)|json!({"program":program,"callback":sampler_ksp::callback_of(&loaded.scripts.views,*program),"category":match outcome{sampler_core::Outcome::FuelExhausted=>"fuel-budget",_=>"runtime-fault"},"core_error":match outcome{sampler_core::Outcome::Fault(e)=>Some(format!("{e:?}")),_=>None}})).collect::<Vec<_>>(),"lua":lua_report,"admitted_saved_entries_by_sigil":admitted,
            "load_path":if is_uvi {if lua.is_some(){"scripted-worker"}else{"offline-loader"}}else{"kontakt-v2-loader"},
            "sample_zone_count":sample_zone_count,"decoded_zone_count":loaded.report.decoded.zones,"sample_count":loaded.report.decoded.samples,"sample_resident_bytes":sample_resident_bytes,"underruns":core.problems(0).underruns,
            "keyswitch":keyswitch,"fallback_note":pick_source=="fallback","zero_zone_reason":if sample_zone_count==Some(0) {Some("unknown")} else {None},"pick_source":pick_source,"native_valid_keys":native_valid,"native_key_conflicts":native.as_ref().map(|n|n.native_key_conflicts),"native_preferred_note":candidate.filter(|(k,_)|native_valid.contains(k)),
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
    } else if ui_missing_font {
        "missing_font"
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
    result["first_audio_ms"]=json!(first_audio_ms);
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

#[cfg(test)]
mod font_observation_tests {
    use super::*;
    #[test]
    fn font_success_spans_pages_and_keeps_unrequested_fonts_unsuccessful() {
        let styles:Vec<_>=(0..3).map(|i|ir::TextStyle {font:ir::Font::File(ir::AssetRef(i)),size:None,color:ir::Rgba::rgb(0xffffff),align:ir::Align::Center}).collect();
        let mut ready=std::collections::BTreeSet::new();
        ready.insert(0); // First page.
        ready.insert(1); // Second page, whose preparation no longer holds font 0.
        assert_eq!(observed_font_styles(&styles,&ready),2);
    }
}
