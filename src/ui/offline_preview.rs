//! Initial-state witnesses without a plugin instance, native window or audio host.
//! Layout, sparse asset requests and painting are the editor's production path.
#[cfg(all(test, feature = "shots"))]
mod uvi_inventory_tests;
use super::{ir_view, native_ui, theme};
use moose::mui::mui::{prelude::*, vello};
use sampler_ui_ir as ir;
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const PAGE_LIMIT: usize = 16;
const PREPARE_LIMIT: Duration = Duration::from_secs(30);
const MAX_SIDE: f64 = 2048.;

struct Painted {
    png_hex: String,
    timings: Value,
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 15) as usize] as char);
    }
    output
}

fn paint(
    face: &ir::Interface,
    page: ir::PageRef,
    assets: &ir_view::Assets,
    mode: ir::Presentation,
    scale: f64,
    values: &mut ir_view::Values,
    width: u16,
    height: u16,
) -> Result<Painted, &'static str> {
    let mut ui = theme::ui();
    let (mut build_ms, mut layout_ms)=(0.,0.);
    for _ in 0..2 {
        let started=Instant::now();
        let view = ir_view::view(&mut ui, face, page, assets, mode, scale, values);
        build_ms+=started.elapsed().as_secs_f64()*1000.;
        let started=Instant::now();
        ui.frame(
            view,
            Some(Size::new(width.into(), height.into())),
            Input::default(),
            1. / 60.,
        )
        .map_err(|_| "UI frame failed; authored payload omitted")?;
        layout_ms+=started.elapsed().as_secs_f64()*1000.;
    }
    let mut painted=encode_scene(&ui, width, height)?;
    painted.timings["view_build_ms"]=json!(build_ms);
    painted.timings["frame_layout_ms"]=json!(layout_ms);
    painted.timings["frames"]=json!(2);
    Ok(painted)
}

fn encode_scene(ui: &Ui, width: u16, height: u16) -> Result<Painted, &'static str> {
    let started=Instant::now();
    let mut context = vello::vello_cpu::RenderContext::new(width, height);
    let mut resources = vello::vello_cpu::Resources::default();
    let setup_ms=started.elapsed().as_secs_f64()*1000.;
    let started=Instant::now();
    vello::paint(
        &mut vello::Cpu {
            ctx: &mut context,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        },
        ui.scene().ok_or("No UI scene")?,
        vello::kurbo::Affine::IDENTITY,
    )
    .map_err(|_| "CPU painting failed; authored payload omitted")?;
    let commands_ms=started.elapsed().as_secs_f64()*1000.;
    let started=Instant::now();
    context.flush();
    let mut pixels = vello::vello_cpu::Pixmap::new(width, height);
    context.render(&mut pixels, &mut resources);
    let raster_ms=started.elapsed().as_secs_f64()*1000.;
    let started=Instant::now();
    let rgba: Vec<u8> = pixels
        .take_unpremultiplied()
        .iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    let extract_ms=started.elapsed().as_secs_f64()*1000.;
    let started=Instant::now();
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, u32::from(width), u32::from(height));
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| "PNG header failed")?;
        writer
            .write_image_data(&rgba)
            .map_err(|_| "PNG encoding failed")?;
        writer.finish().map_err(|_| "PNG finalization failed")?;
    }
    let png_ms=started.elapsed().as_secs_f64()*1000.;
    let started=Instant::now();
    let png_hex=hex(&png_bytes);
    Ok(Painted{png_hex,timings:json!({"renderer_setup_ms":setup_ms,"scene_commands_ms":commands_ms,"cpu_raster_ms":raster_ms,"rgba_extract_ms":extract_ms,"png_encode_ms":png_ms,"hex_encode_ms":started.elapsed().as_secs_f64()*1000.})})
}

/// Returns ephemeral PNG pixels and safe structured observations. The caller owns
/// their lifetime; this function never writes source, artwork or reports to disk.
pub fn render(
    face: &ir::Interface,
    path: &Path,
    initial_values: &[(ir::ControlId, f64)],
    program: u32,
    faces: &[ir::Interface],
) -> Value {
    let started = Instant::now();
    if let Err(error) = face.validate() {
        return json!({"status":"failed","validation_error":error.to_string(),"pages":[],"shots":[]});
    }
    if let Some(native) = &face.native_ui {
        return render_native(face, path, initial_values, program, faces, &native.entry);
    }
    let resolve_started = Instant::now();
    let face = ir_view::resolved(face);
    let resolve_ms = resolve_started.elapsed().as_secs_f64() * 1000.;
    let observations_started=Instant::now();
    let widgets: Vec<_> = face.widgets.iter().enumerate().map(|(index, widget)| {
        let rect = face.page_rect(ir::WidgetRef(index));
        let page = &face.pages[widget.page.0];
        json!({"index":index,"page":widget.page.0,"visible":face.visible(ir::WidgetRef(index)),
            "rect":{"x":rect.x,"y":rect.y,"width":rect.width,"height":rect.height},
            "outside_page":rect.x<0. || rect.y<0. || rect.x+rect.width>page.size.width || rect.y+rect.height>page.size.height,
            "zero_size":(rect.width==0. || rect.height==0.) && !matches!(widget.kind,ir::Kind::Panel)})
    }).collect();
    let geometry_ms=observations_started.elapsed().as_secs_f64()*1000.;
    let mut assets = ir_view::Assets::default();
    let mut shots = Vec::new();
    let mut passive_changes = Vec::new();
    for mode in [ir::Presentation::Bitmap, ir::Presentation::Vector] {
        for page in 0..face.pages.len().min(PAGE_LIMIT) {
            let size = face.pages[page].size;
            let scale = (MAX_SIDE / size.width.max(1.))
                .min(MAX_SIDE / size.height.max(1.))
                .min(1.);
            let width = (size.width * scale).ceil().clamp(1., MAX_SIDE) as u16;
            let height = (size.height * scale).ceil().clamp(1., MAX_SIDE) as u16;
            let mut values: ir_view::Values = initial_values.iter().copied().collect();
            let prepare_started = Instant::now();
            loop {
                assets.prepare_for_program(
                    path,
                    program,
                    &face,
                    ir::PageRef(page),
                    mode,
                    scale,
                    &values,
                );
                if assets.pending() == 0 || prepare_started.elapsed() >= PREPARE_LIMIT {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            let preparation_ms = prepare_started.elapsed().as_secs_f64() * 1000.;
            let pending = assets.pending();
            let paint_started = Instant::now();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                paint(
                    &face,
                    ir::PageRef(page),
                    &assets,
                    mode,
                    scale,
                    &mut values,
                    width,
                    height,
                )
            }));
            let (png_hex, paint_timings, error) = match result {
                Ok(Ok(png)) => (Some(png.png_hex), png.timings, None),
                Ok(Err(error)) => (None, Value::Null, Some(error)),
                Err(_) => (None, Value::Null, Some("CPU renderer panic; authored payload omitted")),
            };
            let paint_and_encode_ms = paint_started.elapsed().as_secs_f64() * 1000.;
            for (id, value) in &values {
                if initial_values
                    .iter()
                    .find(|(old, _)| old == id)
                    .is_some_and(|(_, old)| old != value)
                {
                    passive_changes.push(json!({"binding_id":id.0.to_string(),"page":page,"presentation":format!("{mode:?}")}));
                }
            }
            shots.push(json!({"page":page,"presentation":format!("{mode:?}"),"width":width,"height":height,"scale":scale,
                "pending_assets":pending,"asset_failures":assets.failures(),"prepared_bytes":assets.bytes(),
                "preparation_ms":preparation_ms,"paint_and_encode_ms":paint_and_encode_ms,
                "paint_timings":paint_timings,
                "status":if error.is_some(){"failed"}else if pending!=0 || !assets.failures().is_empty(){"partial"}else{"ok"},
                "error":error,"png_hex":png_hex}));
        }
    }
    let scan = assets.scan();
    let font_fallbacks:Vec<_>=face.styles.iter().enumerate().filter_map(|(index,style)|{
        if let ir::Font::Named(name)=&style.font {Some(json!({"style":index,"font":name,"status":"fallback","reason":"Authored named font family is unresolved by the production renderer; Kontra default face is used"}))}else{None}
    }).collect();
    let status = if shots.is_empty() {
        "empty"
    } else if shots.iter().all(|s| s["status"] == "failed") {
        "failed"
    } else if shots.iter().any(|s| s["status"] != "ok")
        || !passive_changes.is_empty()
        || face.pages.len() > PAGE_LIMIT
        || !font_fallbacks.is_empty()
    {
        "partial"
    } else {
        "ok"
    };
    json!({"status":status,"renderer":"Kontra production UI layout, asset worker and CPU painter",
        "scope":"Initial-state CPU witness; asynchronous updates, gestures, waveform/meter services and native Kontakt/Falcon fidelity are unverified",
        "geometry_scope":"Production resolved page coordinates in source pixels; images may be downscaled to 2048 pixels per side",
        "font_fallbacks":font_fallbacks,
        "pages":face.pages.iter().enumerate().map(|(index,p)|json!({"index":index,"name":p.name,"width":p.size.width,"height":p.size.height})).collect::<Vec<_>>(),
        "widgets":widgets,"shots":shots,"omitted_pages":face.pages.len().saturating_sub(PAGE_LIMIT),"passive_value_changes":passive_changes,
        "assets":assets.probes(),"asset_probe_dropped":assets.probe_dropped(),
        "asset_scope":"Bounded request history across the rendered pages and presentations, not an existence verdict for unrequested assets",
        "resource_program_ownership":"Explicit selected program through the production asset worker",
        "timings":{"resolve_ms":resolve_ms,"geometry_observations_ms":geometry_ms,"total_ms":started.elapsed().as_secs_f64()*1000.,"resource_discovery_ms":scan.discovery_us as f64/1000.,"resource_read_ms":scan.read_us as f64/1000.,"resource_decode_ms":scan.decode_us as f64/1000.},
        "asset_summary":{"lookups":scan.lookups,"found":scan.lookup_ok,"missing":scan.lookup_missing,"ambiguous":scan.lookup_ambiguous,
            "invalid":scan.lookup_invalid,"corrupt":scan.lookup_corrupt,"read_failed":scan.lookup_read,"unavailable":scan.lookup_unavailable,"limit":scan.lookup_limit,
            "decodes":scan.decodes,"decode_ok":scan.decode_ok,"fonts":scan.fonts,"font_ok":scan.font_ok,"encoded_bytes":scan.encoded_bytes,
            "preparation_oversized":scan.preparation_oversized,"preparation_key_budget":scan.preparation_key_budget}})
}

fn render_native(
    face: &ir::Interface,
    path: &Path,
    initial_values: &[(ir::ControlId, f64)],
    program: u32,
    faces: &[ir::Interface],
    entry: &str,
) -> Value {
    let started = Instant::now();
    let controls = faces
        .iter()
        .flat_map(|face| {
            face.widgets
                .iter()
                .enumerate()
                .map(move |(index, widget)| (face.source, index, widget.clone()))
        })
        .collect();
    let mut native = native_ui::State::new_for_program(path, program, entry, controls);
    let values: ir_view::Values = initial_values.iter().copied().collect();
    let input = ir_view::InputState::default();
    let mut ui = theme::ui();
    let mut frames = 0usize;
    let mut settled = 0usize;
    let mut frame_error = None;
    let mut frame_ms = 0.;
    let (mut width, mut height, mut scale) = (970u16, 600u16, 1.);
    while started.elapsed() < PREPARE_LIMIT {
        let size = native.authored();
        scale = (MAX_SIDE / size.width.max(1.))
            .min(MAX_SIDE / size.height.max(1.))
            .min(1.);
        width = (size.width * scale).ceil().clamp(1., MAX_SIDE) as u16;
        height = (size.height * scale).ceil().clamp(1., MAX_SIDE) as u16;
        let frame_started = Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let root = native.view(&mut ui, 0, scale, face, &values, &input);
            ui.frame(
                root,
                Some(Size::new(width.into(), height.into())),
                Input::default(),
                1. / 60.,
            )
            .map(|_| ())
        }));
        frame_ms += frame_started.elapsed().as_secs_f64() * 1000.;
        frames += 1;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(_)) => {
                frame_error = Some("NativeUI frame failed; authored payload omitted");
                break;
            }
            Err(_) => {
                frame_error = Some("NativeUI renderer panic; authored payload omitted");
                break;
            }
        }
        if native.diagnostic().is_some() {
            break;
        }
        if native.pending() == 0 && native.graph_depth().is_some() {
            settled += 1;
        } else {
            settled = 0;
        }
        if settled >= 3 {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let preparation_ms = started.elapsed().as_secs_f64() * 1000.;
    let pending = native.pending();
    let failures = native.failures();
    let diagnostic = native.diagnostic().map(|message| {
        message
            .split(';')
            .next()
            .unwrap_or("NativeUI operation failed")
            .to_owned()
    });
    let ready = settled >= 3 && frame_error.is_none() && diagnostic.is_none();
    // Never call a loading/error caption an authored native UI witness.
    let paint_started = Instant::now();
    let png = ready.then(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            encode_scene(&ui, width, height)
        }))
    });
    let (png_hex, paint_timings, error) = match png {
        Some(Ok(Ok(png))) => (Some(png.png_hex), png.timings, None),
        Some(Ok(Err(error))) => (None, Value::Null, Some(error)),
        Some(Err(_)) => (
            None,
            Value::Null,
            Some("NativeUI CPU painting panic; authored payload omitted"),
        ),
        None => (
            None,
            Value::Null,
            frame_error.or(Some(
                "NativeUI did not reach an authored frame within the bounded preparation window",
            )),
        ),
    };
    let paint_and_encode_ms = paint_started.elapsed().as_secs_f64() * 1000.;
    let size = native.authored();
    let nodes:Vec<_>=ui.scene().into_iter().flat_map(|scene|scene.surfaces())
        .take(4096).map(|surface|json!({"path":surface.key.as_str().splitn(3,'-').nth(2).unwrap_or(surface.key.as_str()),
            "rect":{"x":surface.frame.x/scale,"y":surface.frame.y/scale,"width":surface.frame.size.width/scale,"height":surface.frame.size.height/scale}})).collect();
    let scan = native.scan();
    let status = if error.is_some() || diagnostic.is_some() {
        "failed"
    } else if pending != 0 || !failures.is_empty() {
        "partial"
    } else {
        "ok"
    };
    let shot = json!({"page":0,"presentation":"Bitmap","width":width,"height":height,"scale":scale,"status":status,"png_hex":png_hex,
        "pending_assets":pending,"asset_failures":failures,"prepared_bytes":native.bytes(),"preparation_ms":preparation_ms,
        "paint_and_encode_ms":paint_and_encode_ms,"paint_timings":paint_timings,"error":error});
    let mut vector = shot.clone();
    vector["presentation"] = json!("Vector");
    vector["presentation_scope"] = json!(
        "Production NativeUI preserves the evaluated native composition in both presentations"
    );
    json!({"status":status,"renderer":"Kontra production NativeUI package, Session, view and CPU painter",
        "scope":"Evaluated authored NativeUI composition with initialized control seeds; no KSP initializer rerun, plugin instance, audio Runtime, live callbacks or native Kontakt pixel proof",
        "geometry_scope":"Named CPU scene surfaces of the evaluated NativeUI composition; unkeyed primitives are not enumerated. These coordinates are separate from classic IR control bindings",
        "pages":[{"index":0,"name":"Authored NativeUI","width":size.width,"height":size.height}],"widgets":[],"native_nodes":nodes,
        "shots":[shot,vector],"assets":native.probes(),"asset_probe_dropped":native.probe_dropped(),"asset_scope":"Bounded actual NativeUI font and image request history; aggregate counts include dropped trace records",
        "native_ui":{"entry":entry,"diagnostic":diagnostic,"graph_depth":native.graph_depth(),"graph_work":native.graph_work(),"fonts_loaded":native.font_success(),"frames_before_witness":frames},
        "timings":{"total_ms":started.elapsed().as_secs_f64()*1000.,"native_frames_ms":frame_ms,"resource_discovery_ms":scan.discovery_us as f64/1000.,"resource_read_ms":scan.read_us as f64/1000.,"resource_decode_ms":scan.decode_us as f64/1000.},
        "asset_summary":{"lookups":scan.lookups,"found":scan.lookup_ok,"missing":scan.lookup_missing,"ambiguous":scan.lookup_ambiguous,"invalid":scan.lookup_invalid,
            "corrupt":scan.lookup_corrupt,"read_failed":scan.lookup_read,"unavailable":scan.lookup_unavailable,"decodes":scan.decodes,"decode_ok":scan.decode_ok,"encoded_bytes":scan.encoded_bytes}})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_witness_preserves_resolved_layout_hidden_parent_and_values() {
        let mut face = ir::Interface {
            pages: vec![ir::Page {
                size: ir::Size {
                    width: 160.,
                    height: 90.,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut panel = ir::Widget::new(
            "Hidden panel",
            ir::PageRef(0),
            ir::Rect::new(10, 10, 100, 50),
            ir::Kind::Panel,
        );
        panel.hidden = true;
        let mut child = ir::Widget::new(
            "Authored label",
            ir::PageRef(0),
            ir::Rect::new(5, 6, 60, 20),
            ir::Kind::Label,
        );
        child.parent = Some(ir::WidgetRef(0));
        child.text = "Authored".into();
        face.widgets = vec![panel, child];
        let report = render(
            &face,
            Path::new("/nonexistent/authored.uvip"),
            &[],
            0,
            std::slice::from_ref(&face),
        );
        assert_eq!(report["widgets"][1]["visible"], false);
        assert_eq!(report["widgets"][1]["rect"]["x"], 15.);
        assert_eq!(report["widgets"][1]["rect"]["y"], 16.);
        assert_eq!(report["shots"].as_array().unwrap().len(), 2);
        assert!(
            report["shots"][0]["png_hex"]
                .as_str()
                .unwrap()
                .starts_with("89504e470d0a1a0a")
        );
        assert!(
            report["passive_value_changes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn native_witness_uses_evaluated_composition_and_keeps_presentations_equal() {
        let root = std::env::var_os("KONTRA_TEST_TMPDIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let dir = tempfile::tempdir_in(root).unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(dir.path().join("Resources/native_ui/main.nui"),
            "local ui=require('native_ui')\nreturn function() return @ui.Rectangle {color=ui.Color(20,40,60)}.frame(width=96,height=48).on_tap_gesture({complete=function() end}) end").unwrap();
        let face = ir::Interface {
            native_ui: Some(ir::NativeUi {
                entry: "main".into(),
            }),
            ..Default::default()
        };
        let report = render(
            &face,
            &dir.path().join("Authored.nki"),
            &[],
            0,
            std::slice::from_ref(&face),
        );
        assert_eq!(report["status"], "ok", "{:?}", report["native_ui"]);
        assert_eq!(report["pages"][0]["width"], 96.);
        assert_eq!(report["pages"][0]["height"], 48.);
        assert_eq!(report["shots"][0]["png_hex"], report["shots"][1]["png_hex"]);
        assert!(!report["native_nodes"].as_array().unwrap().is_empty());
    }
}
