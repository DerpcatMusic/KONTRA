//! `kontakto audit-ui`: every instrument's performance view laid out
//! headlessly as the player lays it out, its problems ranked by how many
//! instruments they touch. Prints names and counts, never source or art.

use super::perf_view::{self, FONT, Kind, Shown, caption_of, prop, value};
use super::*;
use crate::artwork;
use crate::ksp::Interface;
use std::collections::{BTreeMap, BTreeSet};

/// One instrument's findings: problem key -> occurrences.
#[derive(Default)]
struct Found {
    problems: BTreeMap<String, usize>,
    kinds: BTreeMap<String, usize>,
    size: (u32, u32),
}

impl Found {
    fn add(&mut self, key: impl Into<String>) {
        *self.problems.entry(key.into()).or_default() += 1;
    }
}

/// Line and slot numbers vary per script; the problem is the same.
fn general(s: &str) -> String {
    let mut out = String::new();
    let mut digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !digits {
                out.push('N');
            }
            digits = true;
        } else {
            digits = false;
            out.push(c);
        }
    }
    out
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Knob => "knob",
        Kind::Slider => "slider",
        Kind::Switch => "switch",
        Kind::Button => "button",
        Kind::Menu => "menu",
        Kind::Value => "value edit",
        Kind::Label => "label",
        Kind::Table => "table",
        Kind::TextEdit => "text edit",
        Kind::FileSelector => "file selector",
        Kind::Area => "mouse area",
        Kind::Meter => "level meter",
        Kind::Waveform => "waveform",
        Kind::Other => "other",
    }
}

/// What the laid-out view `shown` of `u` gets wrong, as far as can be told
/// without Kontakt to compare with.
fn inspect(i: &import::Instrument, u: &Interface, shown: &[Shown], pictures: &HashMap<String, Arc<artwork::Picture>>, found: &mut Found) {
    let (w, h) = (f64::from(u.width), f64::from(u.height));
    for s in shown {
        let c = &u.controls[s.control];
        let kind = kind_name(s.kind);
        if let Some(reason) = crate::diagnostics::widget_limit(&c.kind) { found.add(format!("{}: {reason}", c.kind)); }
        *found.kinds.entry(kind.into()).or_default() += 1;
        let named = prop(c, "$CONTROL_PAR_PICTURE");
        if !named.is_empty() && !pictures.contains_key(named) {
            found.add("missing picture");
        } else if s.kind == Kind::Waveform && perf_view::wave_now(i, c).is_none() {
            found.add("waveform: zone's wave not drawn");
        } else if s.kind == Kind::Other && crate::diagnostics::widget_limit(&c.kind).is_none() {
            // A control with no picture named is Kontakt's stock one, drawn
            // natively; only a kind with no drawing of its own falls back.
            found.add(format!("vector fallback: {}", c.kind));
        }
        // The view clips as Kontakt's does; a control mostly outside is lost.
        let inside = (s.x + s.w).min(w) - s.x.max(0.);
        let inside = inside.max(0.) * ((s.y + s.h).min(h) - s.y.max(0.)).max(0.);
        if inside < 0.5 * s.w * s.h {
            found.add(format!("out of bounds: {kind}"));
        }
        let (said, _, top) = caption_of(c, s.kind, value(c));
        // Text set below the control's foot ($CONTROL_PAR_TEXTPOS_Y 500:
        // Areia's output buttons) is clipped away, as Kontakt hides it.
        let said = if top.is_some_and(|y| y >= s.h) { String::new() } else { said };
        // A label breaks its lines and wraps where it is tall enough: it
        // overflows when a line is wider than it, or the lines taller.
        let lines = if s.kind == Kind::Label {
            perf_view::break_lines(&said, s.w - 4., FONT * 0.75, s.h >= 2. * perf_view::LINE)
        } else {
            vec![said]
        };
        let wide = lines.iter().any(|l| !l.trim().is_empty() && super::cover::advance(l, FONT * 0.75) > s.w - 4.);
        if wide || lines.len() > 1 && lines.len() as f64 * perf_view::LINE > s.h + 1. {
            found.add(format!("text overflow: {kind}"));
        }
    }
    // Two controls taking the same clicks: more than half of the larger.
    // One inside another (a row with its own button) is by design, and a
    // display takes none: a slider over a waveform is its marker.
    let takes = |s: &&Shown| !matches!(s.kind, Kind::Label | Kind::Area | Kind::Waveform | Kind::Meter);
    let live: Vec<&Shown> = shown.iter().filter(takes).collect();
    for (n, a) in live.iter().enumerate() {
        for b in &live[n + 1..] {
            let iw = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
            let ih = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
            if iw > 0. && ih > 0. && iw * ih > 0.5 * (a.w * a.h).max(b.w * b.h) {
                let mut pair = [kind_name(a.kind), kind_name(b.kind)];
                pair.sort();
                found.add(format!("overlapping controls: {}/{}", pair[0], pair[1]));
            }
        }
    }
}

/// What the vectorized view of `u` draws wrong: words wider than their
/// room, outside the view, over each other or under a control drawn over
/// them.
fn vectorized(u: &Interface, shown: &[Shown], pictures: &HashMap<String, Arc<artwork::Picture>>, found: &mut Found) {
    use super::vector::plan;
    let drawn: Vec<_> = shown.iter().map(|s| (s.clone(), perf_view::frame_of(s, &u.controls[s.control]))).collect();
    let plans = plan(u, pictures, &drawn, &mut super::vector::Assets::default(), |_| None);
    let (vw, vh) = (f64::from(u.width), f64::from(u.height));
    // Each word's ink, absolute: (owner, x, y, w, h).
    let mut inks = Vec::new();
    for (n, (s, p)) in shown.iter().zip(&plans).enumerate() {
        let kind = kind_name(s.kind);
        for w in &p.words {
            let (x, wide) = w.ink();
            if super::cover::advance(&w.text, w.size) > w.w + 0.5 || w.size < FONT * super::vector::SMALLEST - 1e-9 {
                found.add(format!("vectorized text overflow: {kind}"));
            }
            let (x, y) = (s.x + x, s.y + w.y + (w.h - w.size * 1.2).max(0.) / 2.);
            let tall = w.size * 1.2;
            if x < 0. || y < 0. || x + wide > vw + 0.5 || y + tall > vh + 0.5 {
                found.add(format!("vectorized text outside the view: {kind}"));
            }
            inks.push((n, x, y, wide, tall));
        }
    }
    let meets = |(ax, ay, aw, ah): (f64, f64, f64, f64), (bx, by, bw, bh): (f64, f64, f64, f64)| {
        (ax + aw).min(bx + bw) - ax.max(bx) > 0.5 && (ay + ah).min(by + bh) - ay.max(by) > 0.5
    };
    for (k, a) in inks.iter().enumerate() {
        if inks[k + 1..].iter().any(|b| b.0 != a.0 && meets((a.1, a.2, a.3, a.4), (b.1, b.2, b.3, b.4))) {
            found.add("vectorized text over other text");
        }
        // A control drawn later with a face of its own covers it.
        let covered = (shown.iter().zip(&plans).skip(a.0 + 1))
            .filter_map(|(o, p)| super::vector::hides(o, &u.controls[o.control], p.face))
            .any(|o| meets((a.1, a.2, a.3, a.4), (o.x, o.y, o.w, o.h)));
        if covered {
            found.add("vectorized text under a control");
        }
    }
}

/// `kontakto audit-ui [roots or presets] [--shots DIR] [--json out.json]`.
pub fn run(args: &[String]) -> anyhow::Result<()> {
    let opt = |flag: &str| args.iter().position(|a| a == flag);
    let (shots_at, json_at, control_at) = (opt("--shots"), opt("--json"), opt("--native-control"));
    let selection = control_at.map(|at| -> anyhow::Result<(String, f64)> {
        let name = args.get(at + 1).ok_or_else(|| anyhow::anyhow!("--native-control requires an identifier and value"))?.clone();
        let value: f64 = args.get(at + 2).ok_or_else(|| anyhow::anyhow!("--native-control requires a value"))?.parse()?;
        anyhow::ensure!(value.is_finite(), "Control value must be finite");
        Ok((name, value))
    }).transpose()?;
    let shots = shots_at.and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let json = json_at.and_then(|i| args.get(i + 1));
    anyhow::ensure!(shots.is_none() || cfg!(feature = "shots"), "Screenshots require cargo build --release --features shots");
    let mut roots: Vec<PathBuf> = (args.iter().enumerate())
        .filter(|&(i, a)| !a.starts_with("--") && [shots_at, json_at].iter().all(|o| o.is_none_or(|o| i != o + 1)) && control_at.is_none_or(|o| i != o + 1 && i != o + 2))
        .map(|(_, a)| a.into())
        .collect();
    if roots.is_empty() {
        roots.push(import::LIBRARY_ROOT.into());
        roots.extend(crate::library_roots());
        roots.sort();
        roots.dedup();
    }
    if let Some(dir) = &shots {
        std::fs::create_dir_all(dir)?;
    }
    let mut ranked: BTreeMap<String, (BTreeSet<String>, usize)> = BTreeMap::new();
    let (mut instruments, mut viewed, mut clean, mut failed) = (0usize, 0usize, 0usize, 0usize);
    let mut rows = Vec::new();
    for root in &roots {
        let presets = match import::presets(root) {
            Ok(presets) => presets,
            Err(e) => {
                let mut trace = crate::diagnostics::LoadTrace::new(root, 0, None);
                trace.stage("catalog"); trace.fail(format!("{e:#}"));
                rows.push(serde_json::json!({"instrument":root,"performance_view":false,"diagnostics":*trace.finish("failed")}));
                failed += 1;
                continue;
            }
        };
        for path in presets {
            let programs = if import::is_multi(&path) {
                import::read_multi(&path).map(|m| m.parts.into_iter().map(|p| p.program).collect::<Vec<_>>())
            } else {
                Ok(vec![0])
            };
            let programs = match programs {
                Ok(programs) => programs,
                Err(e) => {
                    let mut trace = crate::diagnostics::LoadTrace::new(&path, 0, None);
                    trace.stage("import_multi"); trace.fail(format!("{e:#}"));
                    let report = trace.finish("failed");
                    failed += 1;
                    rows.push(serde_json::json!({"instrument":path,"performance_view":false,"diagnostics":*report}));
                    continue;
                }
            };
            for program in programs {
                let name = format!("{}#{program}", path.display());
                let mut trace = crate::diagnostics::LoadTrace::new(&path, program, None);
                trace.stage("import");
                let i = match import::read_program(&path, program) {
                    Ok(i) => Arc::new(i),
                    Err(e) => {
                        eprintln!("{name}: {e:#}");
                        trace.fail(format!("{e:#}"));
                        let report = trace.finish("failed");
                        failed += 1;
                        rows.push(serde_json::json!({"instrument":name,"performance_view":false,"diagnostics":*report}));
                        continue;
                    }
                };
                instruments += 1;
                let mut found = Found::default();
                trace.detail("groups", i.groups.len());
                trace.detail("zones_total", i.zones.len());
                trace.detail("missing_samples", i.missing_samples.len());
                for w in &i.warnings { trace.issue("import", crate::diagnostics::code(w), w); }
                for sample in &i.missing_samples { trace.issue("samples", "missing", sample); }
                let shown = audit_one_selected(&i, &mut found, &mut trace, selection.as_ref());
                let has_view = shown.is_some();
                trace.detail("performance_view", has_view);
                let mut rendered = true;
                if let (Some(dir), Some(part)) = (&shots, shown) {
                    trace.stage("render");
                    let stem = format!("{}-{program}", i.name.replace(['/', '\\'], "_"));
                    for (code, mode) in [(1, "original"), (2, "kontra"), (3, "vectorized")] {
                        if let Err(e) = shot(&part, code, &dir.join(format!("{stem}-{mode}.png"))) {
                            eprintln!("{name}: shot: {e:#}");
                            rendered = false;
                            trace.issue("render", "render_failed", format!("{mode}: {e:#}"));
                        }
                    }
                }
                if has_view { viewed += 1; clean += usize::from(found.problems.is_empty() && rendered); }
                for (k, n) in &found.problems {
                    trace.issue("ui", crate::diagnostics::code(k), format!("{k} ({n} occurrences)"));
                    let e = ranked.entry(k.clone()).or_default();
                    e.0.insert(name.clone());
                    e.1 += n;
                }
                let report = trace.finish(if rendered { "loaded" } else { "failed" });
                rows.push(serde_json::json!({
                    "instrument": name,
                    "performance_view": has_view,
                    "diagnostics": *report,
                    "size": [found.size.0, found.size.1],
                    "controls": found.kinds,
                    "problems": found.problems,
                }));
            }
        }
    }
    if let Some(out) = json {
        std::fs::write(out, serde_json::to_string_pretty(&rows)?)?;
    }
    println!("UI audit: {instruments} instruments, {viewed} with a performance view, {clean} of those with no UI problem found, {failed} failed imports");
    let mut ranked: Vec<_> = ranked.into_iter().collect();
    ranked.sort_by(|a, b| b.1.0.len().cmp(&a.1.0.len()).then(a.0.cmp(&b.0)));
    println!("{:>11} {:>11}  problem", "instruments", "occurrences");
    for (k, (who, n)) in ranked {
        println!("{:>11} {n:>11}  {k}", who.len());
    }
    Ok(())
}

/// Run `i`'s scripts as the player does, lay out its view and note what
/// is wrong; the part to draw, if it has a view.
#[cfg(test)]
fn audit_one(i: &Arc<import::Instrument>, found: &mut Found, trace: &mut crate::diagnostics::LoadTrace) -> Option<PartView> {
    audit_one_selected(i, found, trace, None)
}
fn audit_one_selected(i: &Arc<import::Instrument>, found: &mut Found, trace: &mut crate::diagnostics::LoadTrace, selection: Option<&(String, f64)>) -> Option<PartView> {
    trace.stage("scripts");
    let (rt, errors) = crate::engine::load_scripts(i, i.script_state.clone(), 48_000.);
    for e in &errors {
        trace.script_issue("initialization_failed", e, &i.scripts);
        found.add(format!("script: {}", general(e.split_once(": ").map_or(e, |x| x.1))));
    }
    let mut rt = rt?;
    // A second of audio: listeners and waits run as they would once playing,
    // against the instrument's groups, modulators and effects.
    trace.stage("script_callbacks");
    let mut engine = crate::engine::ScriptSetup::new(i, 48_000.0);
    (0..100).for_each(|_| rt.process(&mut engine, 480));
    if let Some((name, value)) = selection {
        let control = rt.exposed_controls().into_iter().find(|c| c.identifier == *name)?;
        if !rt.native_control(&mut engine, control.slot, control.control, 0, *value) { return None; }
    }
    #[cfg(test)]
    tests::select_benchmark_page(&mut rt, i, false);
    let script = crate::plugin::script_interface(Some(&rt));
    trace.script_runtime(&rt, &i.scripts);
    for d in script.status.lines().filter(|d| !d.trim().is_empty()) {
        found.add(format!("script: {}", general(d)));
    }
    let Some(u) = script.interface.clone().filter(|u| u.performance && !u.controls.is_empty()) else {
        found.problems.retain(|k, _| k.starts_with("script"));
        return None;
    };
    if !u.native_ui.is_empty() {
        let package=match crate::native_ui::Package::load(&i.path) {
            Ok(package)=>Arc::new(package),
            Err(e)=>{trace.issue("native_ui","resource_load",format!("{e:#}"));found.add("NativeUI resources failed to load");return None;}
        };
        let controls:Arc<[crate::ksp::ExposedControl]>=Arc::from(rt.exposed_controls());
        match crate::native_ui::Session::new(package.clone(),&u.native_ui,controls.clone()).and_then(|session|session.render()) {
            Ok(_)=>{},
            Err(e)=>{trace.issue("native_ui","initialization_failed",format!("{e:#}"));found.add("NativeUI initialization failed");}
        }
        found.size=(u.width.max(0) as u32,u.height.max(0) as u32);
        return Some(PartView {native_ui:Some(package),exposed_controls:controls,script_epoch:1,interface:Some(u),keys:script.keys,instrument:Some(i.clone()),active:i.name.clone(),..Default::default()});
    }
    for warning in perf_view::font_fallbacks(&u) { trace.issue("ui", "font_fallback", &warning); }
    found.size = (u.width.max(0) as u32, u.height.max(0) as u32);
    let names: Vec<_> = artwork::picture_names(&u).collect();
    trace.stage("artwork");
    trace.detail("controls", u.controls.len());
    let (pictures, errors) = artwork::pictures_report(&i.path, names.iter().map(|name| name.as_ref()));
    for e in errors { trace.issue("artwork", crate::diagnostics::code(&e), e); }
    trace.detail("pictures_loaded", pictures.len());
    let pictures = Arc::new(pictures);
    let wallpaper = match artwork::performance(i, Some(&u)) {
        Ok(w) => w,
        Err(e) => {
            trace.issue("artwork", crate::diagnostics::code(&e), e);
            found.add("missing wallpaper");
            None
        }
    };
    if let Some(w) = &wallpaper
        && (i64::from(w.atlas.map_or(w.frames[0].width, |a| a[0])) - i64::from(u.width)).abs() > 2
    {
        found.add("wallpaper narrower or wider than the view");
    }
    let shown = perf_view::layout(&u, &pictures);
    inspect(i, &u, &shown, &pictures, found);
    vectorized(&u, &shown, &pictures, found);
    Some(PartView {
        pictures,
        wallpaper,
        interface: Some(u),
        keys: script.keys,
        instrument: Some(i.clone()),
        active: i.name.clone(),
        ..Default::default()
    })
}

/// The whole window with `part` racked alone, its view in mode `code`
/// ([`Part::view`]), drawn by the CPU renderer to a PNG at `to`.
#[cfg(not(any(feature = "shots", test)))]
fn shot(_: &PartView, _: u8, _: &Path) -> anyhow::Result<()> {
    anyhow::bail!("built without the CPU renderer: cargo build --release --features shots")
}

#[cfg(any(feature = "shots", test))]
fn shot(part: &PartView, code: u8, to: &Path) -> anyhow::Result<()> {
    shot_scaled(part, code, to, 1.)
}

#[cfg(any(feature = "shots", test))]
fn shot_scaled(part: &PartView, code: u8, to: &Path, device_scale: f64) -> anyhow::Result<()> {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let (width, height) = ((1180. * device_scale).round() as u16, (900. * device_scale).round() as u16);
    let p = Arc::new(SamplerParams::new());
    let i = part.instrument.as_ref().expect("a viewed part has its instrument");
    p.selection.write().unwrap().parts.push(Part {
        path: i.path.to_string_lossy().into(),
        group: i.first_playable_group().unwrap_or(0) as u32,
        view: code,
        ..Default::default()
    });
    {
        let mut view = p.shared.view.lock().unwrap();
        view.files = Arc::new(vec![i.path.clone()]);
        view.parts[0] = part.clone();
    }
    let mut ui = theme::ui();
    ui.set_scale(Some(device_scale));
    let mut build = build(&p, Arc::default(), Arc::default(), Arc::default(), Arc::default());
    let mut bridge = Bridge::new(p.clone());
    #[cfg(test)]
    let staged_native = std::env::var_os("KONTRA_UI_BENCH_STAGED_NATIVE_ASSETS").is_some();
    #[cfg(not(test))]
    let staged_native = false;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    let mut frames = 0;
    let mut settled = 0;
    loop {
        let root = build(&mut ui, &mut bridge);
        ui.frame(root, Some(Size::new(1180., 900.)), Input::default(), 1. / 60.)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        frames += 1;
        if !staged_native {
            if frames == 8 { break; }
            continue;
        }
        let ready = {
            let view = p.shared.view.lock().unwrap();
            let active = &view.parts[0];
            !active.visual_worker && !active.visual_demand.native_images.is_empty()
                && active.native_ui.as_ref().is_some_and(|package| {
                    active.visual_demand.native_images.iter().all(|name| package.images.contains_key(name))
                })
        };
        settled = if ready { settled + 1 } else { 0 };
        if settled == 8 {
            let view = p.shared.view.lock().unwrap();
            let active = &view.parts[0];
            let package = active.native_ui.as_ref().unwrap();
            println!("UI_STAGED_NATIVE {}", serde_json::json!({
                "frames":frames,"settled_frames":settled,"revision":active.visual_revision,
                "requested_images":active.visual_demand.native_images.len(),
                "decoded_images":package.images.len(),"image_metadata":package.image_metadata.len(),
                "worker_complete":!active.visual_worker,"resource_status":active.wallpaper_status,
                "decoded_bytes":package.images.values().map(|image| image.rgba.len()).sum::<usize>(),
                "scope":"actual full-app demand request and production visual worker publication"
            }));
            break;
        }
        if std::time::Instant::now() >= deadline {
            let view = p.shared.view.lock().unwrap();
            let active = &view.parts[0];
            anyhow::bail!("NativeUI staged demand did not settle: worker={} requested={} decoded={} status={}",
                active.visual_worker, active.visual_demand.native_images.len(),
                active.native_ui.as_ref().map_or(0, |package| package.images.len()), active.wallpaper_status);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let scene = ui.scene().ok_or_else(|| anyhow::anyhow!("nothing drawn"))?;
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    vello::paint(
        &mut vello::Cpu { ctx: &mut ctx, resources: &mut resources, cache: &mut vello::Cache::default() },
        scene,
        vello::kurbo::Affine::scale(device_scale),
    )
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut resources);
    let rgba: Vec<u8> = pix.take_unpremultiplied().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    moose::core::screenshot::save_png(to, &rgba, u32::from(width), u32::from(height));
    #[cfg(test)]
    if std::env::var_os("KONTRA_UI_BENCH_GPU_CAPTURE").is_some() {
        let original = scene.clone();
        let normal_images = original.paint.iter().filter(|paint|
            matches!(paint.paint, moose::mui::mui::scene::Paint::Image { .. })).count();
        let vectors = original.paint.iter().filter(|paint|
            matches!(paint.paint, moose::mui::mui::scene::Paint::Vector { .. })).count();
        assert!(normal_images>0,"actual Conflux scene must include normal image artwork");
        println!("UI_GPU_SCENE {}",serde_json::json!({"normal_images":normal_images,"vectors":vectors,
            "paint_entries":original.paint.len(),"device_scale":device_scale,
            "scope":"exact resolved full-app Conflux scene, no renderer fixture"}));
        let size = [u32::from(width),u32::from(height)];
        let mut gpu = tests::GpuCapture::new(size,device_scale)?;
        gpu.original(&original,&rgba,to)?;
        let note = original.paint.iter().filter_map(|paint|
            paint.key.as_str().strip_prefix("key-").and_then(|name|name.parse::<usize>().ok()))
            .find(|&note|note<128).ok_or_else(||anyhow::anyhow!("real keyboard is not shown"))?;
        let previous = p.shared.played[note].swap(127,std::sync::atomic::Ordering::Relaxed);
        assert_eq!(previous,0,"capture-only fixture has no pre-existing host note feedback");
        for _ in 0..2 {
            let root = build(&mut ui,&mut bridge);
            ui.frame(root,Some(Size::new(1180.,900.)),Input::default(),1./60.)
                .map_err(|e|anyhow::anyhow!("{e:?}"))?;
        }
        let changed = ui.scene().ok_or_else(||anyhow::anyhow!("changed scene missing"))?.clone();
        p.shared.played[note].store(previous,std::sync::atomic::Ordering::Relaxed);
        assert_ne!(original.paint,changed.paint,"real keyboard feedback must alter actual UI paint");
        let mut changed_ctx = RenderContext::new(width,height);
        let mut changed_resources = Resources::default();
        vello::paint(&mut vello::Cpu { ctx:&mut changed_ctx,resources:&mut changed_resources,
            cache:&mut vello::Cache::default() },&changed,vello::kurbo::Affine::scale(device_scale))
            .map_err(|e|anyhow::anyhow!("{e:?}"))?;
        changed_ctx.flush();
        let mut changed_pix = Pixmap::new(width,height);
        changed_ctx.render(&mut changed_pix,&mut changed_resources);
        let changed_cpu:Vec<u8> = changed_pix.take_unpremultiplied().iter().flat_map(|p|[p.r,p.g,p.b,p.a]).collect();
        moose::core::screenshot::save_png(&to.with_extension("cpu-changed.png"),&changed_cpu,size[0],size[1]);
        println!("UI_GPU_FEEDBACK note={note} value=127");
        gpu.changed(&changed,&changed_cpu,to)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal control separates Vello compute output from MUI's retained present.
    #[test]
    #[ignore = "parent-run GPU correctness control; no display or owned library required"]
    fn conflux_gpu_solid_control_separates_compute_and_retained_present() {
        use moose::mui::mui::{scene::prelude::*,vello::{self,vello::wgpu}};
        let out = PathBuf::from(std::env::var_os("KONTRA_UI_GPU_CONTROL_OUTPUT")
            .expect("KONTRA_UI_GPU_CONTROL_OUTPUT"));
        std::fs::create_dir_all(&out).unwrap();
        let size = [192,144];
        let mut capture = GpuCapture::new(size,1.5).unwrap();
        capture.calibrate(&out.join("solid.png")).unwrap();
        let direct_target = capture.device.create_texture(&wgpu::TextureDescriptor {
            label:Some("direct Vello compute-only solid target"),
            size:wgpu::Extent3d { width:size[0],height:size[1],depth_or_array_layers:1 },
            mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,
            format:wgpu::TextureFormat::Rgba8Unorm,
            usage:wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats:&[],
        });
        let direct_view = direct_target.create_view(&Default::default());
        let mut direct_scene = vello::vello::Scene::new();
        direct_scene.fill(vello::vello::peniko::Fill::NonZero,vello::kurbo::Affine::IDENTITY,
            vello::vello::peniko::Color::WHITE,None,
            &vello::kurbo::Rect::new(0.,0.,f64::from(size[0]),f64::from(size[1])));
        let encoding = direct_scene.encoding();
        println!("UI_GPU_DIRECT_ENCODING {}",serde_json::json!({"path_tags":encoding.path_tags.len(),
            "path_data":encoding.path_data.len(),"draw_tags":encoding.draw_tags.len(),
            "draw_data":encoding.draw_data.len(),"transforms":encoding.transforms.len()}));
        let mut direct = gpu_checked(&capture.device,"direct_solid_setup",||Ok(
            vello::vello::Renderer::new(&capture.device,vello::vello::RendererOptions {
                use_cpu:false,antialiasing_support:vello::vello::AaSupport::area_only(),..Default::default()
            })?)).unwrap();
        gpu_checked(&capture.device,"direct_solid_draw",||Ok(direct.render_to_texture(
            &capture.device,&capture.queue,&direct_scene,&direct_view,&vello::vello::RenderParams {
                base_color:vello::vello::peniko::Color::TRANSPARENT,width:size[0],height:size[1],
                antialiasing_method:vello::vello::AaConfig::Area,
            })?)).unwrap();
        let compute = gpu_checked(&capture.device,"direct_solid_readback",||capture.readback_texture(&direct_target)).unwrap();
        moose::core::screenshot::save_png(&out.join("direct-vello-solid.png"),&compute,size[0],size[1]);
        let scene = moose::mui::mui::scene::resolve(&SceneSpec::new(
            block(128.,96.).radius(0.).fill(Color::oklcha(1.,0.,0.,1.)))
            .offered(Size::new(128.,96.)).scale(1.5)).unwrap();
        let stats = gpu_checked(&capture.device,"retained_solid_draw",||Ok(
            capture.renderer.render(&scene,capture.transform,&capture.view)?)).unwrap();
        let presented = gpu_checked(&capture.device,"retained_solid_readback",||capture.readback()).unwrap();
        moose::core::screenshot::save_png(&out.join("mui-retained-solid.png"),&presented,size[0],size[1]);
        let receipt = serde_json::json!({"direct_nontransparent_pixels":compute.chunks_exact(4).filter(|p|p[3]!=0).count(),
            "direct_white_pixels":compute.chunks_exact(4).filter(|p|*p==[255,255,255,255]).count(),
            "retained_nontransparent_pixels":presented.chunks_exact(4).filter(|p|p[3]!=0).count(),
            "comparison":gpu_pixel_diff(&compute,&presented,size),"retained_renders":stats.renders,
            "retained_paint_entries":scene.paint.len(),"size":size,"device_scale":1.5});
        println!("UI_GPU_SOLID_CONTROL {receipt}");
        std::fs::write(out.join("solid-control.json"),serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        assert!(compute.chunks_exact(4).all(|p|p==[255,255,255,255]),
            "direct Vello compute control failed: {receipt}");
        assert_eq!(compute,presented,"MUI retained encoding/presentation differs from direct solid output: {receipt}");
    }

    /// Opt-in correctness probe: synchronous readback never enters production UI.
    pub(super) struct GpuCapture {
        device: moose::mui::mui::vello::vello::wgpu::Device,
        queue: moose::mui::mui::vello::vello::wgpu::Queue,
        renderer: moose::mui::mui::vello::effects::GpuRenderer,
        target: moose::mui::mui::vello::vello::wgpu::Texture,
        view: moose::mui::mui::vello::vello::wgpu::TextureView,
        size: [u32; 2],
        transform: moose::mui::mui::vello::kurbo::Affine,
    }
    fn gpu_wait<F: std::future::Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            match future.as_mut().poll(&mut context) {
                std::task::Poll::Ready(result) => return result,
                std::task::Poll::Pending => std::thread::sleep(Duration::from_millis(1)),
            }
        }
    }
    fn gpu_checked<T>(device: &moose::mui::mui::vello::vello::wgpu::Device,
        stage: &str, operation: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
        use moose::mui::mui::vello::vello::wgpu;
        let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
        let result = operation();
        let poll = device.poll(wgpu::PollType::wait_indefinitely());
        let errors: Vec<String> = [internal,memory,validation].into_iter()
            .filter_map(|scope|gpu_wait(scope.pop())).map(|error|error.to_string()).collect();
        println!("UI_GPU_CHECKED {}",serde_json::json!({"stage":stage,"errors":errors}));
        if !errors.is_empty() { anyhow::bail!("GPU {stage}: {}",errors.join("; ")); }
        poll?;
        result
    }
    fn gpu_pixel_diff(a: &[u8], b: &[u8], size: [u32; 2]) -> serde_json::Value {
        assert_eq!(a.len(), b.len());
        let mut different = 0usize;
        let mut over_eight = 0usize;
        let mut max = 0u8;
        let mut sum = 0u64;
        let mut bounds = [size[0], size[1], 0, 0];
        for (n, (a, b)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
            let delta = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
            max = max.max(delta);
            sum += a.iter().zip(b).map(|(a, b)| u64::from(a.abs_diff(*b))).sum::<u64>();
            if delta > 0 {
                different += 1;
                let (x, y) = (n as u32 % size[0], n as u32 / size[0]);
                bounds = [bounds[0].min(x), bounds[1].min(y), bounds[2].max(x + 1), bounds[3].max(y + 1)];
            }
            over_eight += usize::from(delta > 8);
        }
        serde_json::json!({"different_pixels":different,"pixels_over_8":over_eight,
            "max_channel_delta":max,"absolute_channel_delta_sum":sum,
            "difference_bounds":(different != 0).then_some(bounds),"total_pixels":u64::from(size[0])*u64::from(size[1])})
    }
    impl GpuCapture {
        pub(super) fn new(size: [u32; 2], scale: f64) -> anyhow::Result<Self> {
            use moose::mui::mui::vello::{self, vello::wgpu};
            let instance = wgpu::Instance::default();
            let adapter = gpu_wait(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default()
            }))?;
            // Software adapters are valid correctness evidence; identify them explicitly.
            println!("UI_GPU_ADAPTER {:?}", adapter.get_info());
            let (device, queue) = gpu_wait(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
            println!("UI_GPU_LIMITS {:?}",device.limits());
            device.on_uncaptured_error(Arc::new(|error|eprintln!("UI_GPU_UNCAPTURED {error}")));
            device.set_device_lost_callback(|reason,message|eprintln!("UI_GPU_LOST {reason:?} {message}"));
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("actual Conflux correctness readback"),
                size: wgpu::Extent3d { width:size[0], height:size[1], depth_or_array_layers:1 },
                mip_level_count:1, sample_count:1, dimension:wgpu::TextureDimension::D2,
                format:wgpu::TextureFormat::Rgba8Unorm,
                usage:wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats:&[],
            });
            let view = target.create_view(&Default::default());
            let renderer = gpu_checked(&device,"renderer_setup",||Ok(gpu_wait(
                vello::effects::GpuRenderer::new(&device,&queue,wgpu::TextureFormat::Rgba8Unorm,
                    size,vello::effects::Budget::default()))?))?;
            Ok(Self { device, queue, renderer, target, view, size, transform:vello::kurbo::Affine::scale(scale) })
        }
        fn readback(&self) -> anyhow::Result<Vec<u8>> {
            self.readback_texture(&self.target)
        }
        fn readback_texture(&self, texture: &moose::mui::mui::vello::vello::wgpu::Texture) -> anyhow::Result<Vec<u8>> {
            use moose::mui::mui::vello::vello::wgpu;
            let stride = (self.size[0] * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label:Some("actual Conflux readback rows"),size:u64::from(stride)*u64::from(self.size[1]),
                usage:wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,mapped_at_creation:false,
            });
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.copy_texture_to_buffer(texture.as_image_copy(), wgpu::TexelCopyBufferInfo {
                buffer:&buffer,layout:wgpu::TexelCopyBufferLayout {
                    offset:0,bytes_per_row:Some(stride),rows_per_image:Some(self.size[1]),
                },
            },wgpu::Extent3d { width:self.size[0],height:self.size[1],depth_or_array_layers:1 });
            self.queue.submit([encoder.finish()]);
            let (send, recv) = std::sync::mpsc::sync_channel(1);
            buffer.slice(..).map_async(wgpu::MapMode::Read,move |result| { let _ = send.send(result); });
            self.device.poll(wgpu::PollType::wait_indefinitely())?;
            recv.recv_timeout(Duration::from_secs(10))??;
            let mapped = buffer.slice(..).get_mapped_range()?;
            let mut pixels = Vec::with_capacity(self.size[0] as usize*self.size[1] as usize*4);
            for row in mapped.chunks_exact(stride as usize) {
                pixels.extend_from_slice(&row[..self.size[0] as usize*4]);
            }
            drop(mapped);
            buffer.unmap();
            Ok(pixels)
        }
        fn calibrate(&self, to: &Path) -> anyhow::Result<()> {
            use moose::mui::mui::vello::vello::wgpu;
            gpu_checked(&self.device,"clear_and_readback_calibration",|| {
                let mut encoder = self.device.create_command_encoder(&Default::default());
                {
                    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label:Some("readback calibration red clear"),
                        color_attachments:&[Some(wgpu::RenderPassColorAttachment {
                            view:&self.view,depth_slice:None,resolve_target:None,
                            ops:wgpu::Operations { load:wgpu::LoadOp::Clear(wgpu::Color {
                                r:1.,g:0.,b:0.,a:1. }),store:wgpu::StoreOp::Store },
                        })],..Default::default()
                    });
                }
                self.queue.submit([encoder.finish()]);
                let red = self.readback()?;
                moose::core::screenshot::save_png(&to.with_extension("gpu-readback-calibration.png"),
                    &red,self.size[0],self.size[1]);
                let correct = red.chunks_exact(4).all(|pixel|pixel==[255,0,0,255]);
                println!("UI_GPU_CALIBRATION red_clear_exact={correct}");
                anyhow::ensure!(correct,"GPU readback failed exact red-clear calibration");
                Ok(())
            })
        }
        pub(super) fn original(&mut self, scene: &moose::mui::mui::scene::ResolvedScene,
            cpu: &[u8], to: &Path) -> anyhow::Result<()> {
            self.calibrate(to)?;
            let full = gpu_checked(&self.device,"original_full_draw",||Ok(
                self.renderer.render(scene,self.transform,&self.view)?))?;
            let initial = gpu_checked(&self.device,"original_readback",||self.readback())?;
            let retained = gpu_checked(&self.device,"original_unchanged_draw",||Ok(
                self.renderer.render(scene,self.transform,&self.view)?))?;
            let unchanged = gpu_checked(&self.device,"original_unchanged_readback",||self.readback())?;
            assert_eq!(full.rendered_pixels,u64::from(self.size[0])*u64::from(self.size[1]));
            println!("UI_GPU_PIXELS {}",serde_json::json!({"stage":"original_full",
                "nontransparent_pixels":initial.chunks_exact(4).filter(|pixel|pixel[3]!=0).count()}));
            moose::core::screenshot::save_png(&to.with_extension("gpu-full.png"),&initial,self.size[0],self.size[1]);
            println!("UI_GPU_COMPARE {}",serde_json::json!({"stage":"original_cpu_vs_fresh_gpu",
                "comparison":gpu_pixel_diff(cpu,&initial,self.size),"renders":full.renders,
                "rendered_pixels":full.rendered_pixels,"paint_entries":scene.paint.len()}));
            assert_eq!(retained.renders,0,"unchanged exact scene must be retained");
            assert_eq!(initial,unchanged,"unchanged retained GPU frame changed pixels");
            Ok(())
        }
        pub(super) fn changed(&mut self, scene: &moose::mui::mui::scene::ResolvedScene,
            cpu: &[u8], to: &Path) -> anyhow::Result<()> {
            use moose::mui::mui::vello::{self,vello::wgpu};
            let stats = gpu_checked(&self.device,"changed_retained_draw",||Ok(
                self.renderer.render(scene,self.transform,&self.view)?))?;
            let retained = gpu_checked(&self.device,"changed_retained_readback",||self.readback())?;
            let mut fresh = gpu_checked(&self.device,"changed_fresh_setup",||Ok(gpu_wait(
                vello::effects::GpuRenderer::new(&self.device,&self.queue,wgpu::TextureFormat::Rgba8Unorm,
                    self.size,vello::effects::Budget::default()))?))?;
            let full = gpu_checked(&self.device,"changed_fresh_draw",||Ok(
                fresh.render(scene,self.transform,&self.view)?))?;
            let reference = gpu_checked(&self.device,"changed_fresh_readback",||self.readback())?;
            moose::core::screenshot::save_png(&to.with_extension("gpu-changed-retained.png"),&retained,self.size[0],self.size[1]);
            moose::core::screenshot::save_png(&to.with_extension("gpu-changed-fresh.png"),&reference,self.size[0],self.size[1]);
            let comparison = gpu_pixel_diff(&retained,&reference,self.size);
            let receipt = serde_json::json!({"stage":"real_keyboard_feedback_retained_vs_same_scene_fresh_gpu",
                "comparison":comparison,"cpu_vs_fresh_gpu":gpu_pixel_diff(cpu,&reference,self.size),
                "renders":stats.renders,"rendered_pixels":stats.rendered_pixels,
                "fresh_rendered_pixels":full.rendered_pixels,"paint_entries":scene.paint.len(),
                "partial_exercised":stats.rendered_pixels>0 && stats.rendered_pixels<full.rendered_pixels,
                "nontransparent_pixels":reference.chunks_exact(4).filter(|pixel|pixel[3]!=0).count()});
            println!("UI_GPU_COMPARE {receipt}");
            std::fs::write(to.with_extension("gpu-comparison.json"),serde_json::to_vec_pretty(&receipt)?)?;
            assert_eq!(stats.renders,1,"real feedback change must repaint");
            anyhow::ensure!(reference.chunks_exact(4).any(|pixel|pixel[3]!=0),
                "fresh GPU scene is entirely transparent after calibrated readback; inspect early scope and setup receipts");
            // Record full redraws as diagnostic evidence; they cannot certify partial damage.
            // Fractional damage translation can change one-pixel edge rounding.
            // Record every delta; reject a corruption footprint above 0.1% of frame.
            let large = comparison["pixels_over_8"].as_u64().unwrap();
            assert!(large*1000<=u64::from(self.size[0])*u64::from(self.size[1]),
                "retained GPU frame diverges materially from exact fresh scene: {receipt}");
            Ok(())
        }
    }


    #[test]
    fn numbers_do_not_split_a_problem() {
        assert_eq!(super::general("Script 2: KSP line 41: no x1"), "Script N: KSP line N: no xN");
    }

    /// After the native worker stops: diagnostic microcosts, not frame samples.
    fn cost_partition(p: &Arc<SamplerParams>) -> serde_json::Value {
        use std::hint::black_box;
        fn measure(mut f: impl FnMut()) -> serde_json::Value {
            for _ in 0..8 { f(); }
            let mut times: Vec<_> = (0..16).map(|_| {
                let start = Instant::now();
                f();
                start.elapsed().as_secs_f64() * 1000.
            }).collect();
            times.sort_by(f64::total_cmp);
            serde_json::json!({"n":times.len(),"mean_ms":times.iter().sum::<f64>()/times.len() as f64,"p99_ms":times[times.len()-1]})
        }
        let selection = p.selection.read().unwrap().clone();
        let view = shown(&p.shared.view);
        let part = &view.parts[0];
        let interface = part.interface.as_ref().expect("published interface");
        let instrument = part.instrument.as_ref().expect("published instrument");
        let drawn: Vec<_> = perf_view::layout(interface, &part.pictures).into_iter().map(|s| {
            let image = perf_view::frame_of(&s, &interface.controls[s.control]);
            (s, image)
        }).collect();
        let wallpaper = part.wallpaper.as_ref().and_then(|p| p.wallpaper(interface.wallpaper_state, interface.skin_offset));
        let mut assets = super::super::vector::Assets::default();
        let mut stages = BTreeMap::new();
        stages.insert("selection_double_clone_and_drop", measure(|| {
            let copy = black_box(selection.clone());
            black_box(copy.clone());
        }));
        stages.insert("shown_view_clone_and_drop", measure(|| { black_box(shown(&p.shared.view)); }));
        stages.insert("watch_fingerprint", measure(|| {
            let mut h = DefaultHasher::new();
            fingerprint(&view, &mut h);
            black_box(h.finish());
        }));
        stages.insert("interface_clone_and_drop", measure(|| { black_box(interface.as_ref().clone()); }));
        stages.insert("scalar_table_property_hash", measure(|| {
            let mut h = DefaultHasher::new();
            perf_view::hash_properties(interface, &mut h);
            black_box(h.finish());
        }));
        stages.insert("visible_layout_and_drop", measure(|| { black_box(perf_view::layout(interface, &part.pictures)); }));
        stages.insert("warm_vector_plan_and_drop", measure(|| {
            black_box(super::super::vector::plan(interface, &part.pictures, &drawn, &mut assets, |n| part.control_value(n)));
        }));
        stages.insert("original_luma_prefixes", measure(|| {
            let mut sum = 0.;
            for (n, (s, _)) in drawn.iter().enumerate() {
                for dx in [-0.25, 0., 0.25] {
                    sum += perf_view::luma_under(&drawn[..=n], wallpaper.as_ref().map(|(i, origin)| (i.as_ref(), *origin)),
                        s.x + s.w / 2. + dx * s.w, s.y + s.h / 2.).unwrap_or(0.);
                }
            }
            black_box(sum);
        }));
        stages.insert("conditional_missing_zone_count", measure(|| {
            black_box(instrument.zones.iter().filter(|z| !z.available).count());
        }));
        serde_json::json!({"scope":"16 bounded microcost samples after native worker completion; clone timings include drop, not concurrent contention or whole-frame latency",
            "selection_script_state_bytes":selection.parts.iter().map(|p|p.script_state.len()).sum::<usize>(),
            "view_script_state_bytes":p.shared.view.lock().unwrap().parts.iter().map(|p|p.script_state.len()).sum::<usize>(),
            "controls":interface.controls.len(),"visible_controls":drawn.len(),
            "properties":interface.controls.iter().map(|c|c.properties.len()).sum::<usize>(),
            "interface_array_cells":interface.controls.iter().flat_map(|c|c.properties.values()).map(|v| match v {
                crate::ksp::Value::IntArray(a) => a.len(), crate::ksp::Value::RealArray(a) => a.len(),
                crate::ksp::Value::Array(a) => a.len(), _ => 0,
            }).sum::<usize>(),
            "groups":instrument.groups.len(),"zones":instrument.zones.len(),
            "missing_notice_active":!instrument.warnings.iter().any(|w|w.contains("read back as zeros"))
                && (part.status.contains("zones skipped") || !instrument.missing_samples.is_empty()),"stages":stages})
    }

    /// Fixture page changes use the authored callback, including its listeners.
    pub(super) fn select_benchmark_page(runtime: &mut crate::ksp::Runtime, instrument: &import::Instrument, cold: bool) {
        let Ok(variable) = std::env::var("KONTRA_UI_BENCH_PAGE") else { return };
        let mut engine = crate::engine::ScriptSetup::new(instrument, 48000.);
        if cold { (0..100).for_each(|_| runtime.process(&mut engine, 480)); }
        // Some authored pages have a top-level and an inner tab. Execute each
        // callback in order; changing visibility directly would bypass scripts.
        for variable in variable.split(',').map(str::trim).filter(|v| !v.is_empty()) {
            let live = runtime.live();
            let control = live.interface.as_ref().expect("page interface").controls.iter()
                .position(|c| c.variable == variable).expect("authored page control exists");
            runtime.ui_control(&mut engine, live.slot, control, 1);
            (0..100).for_each(|_| runtime.process(&mut engine, 480));
        }
    }

    /// Opt-in local measurement; prints timings/counts, never library payloads.
    #[test]
    #[ignore = "set KONTRA_UI_BENCH_PATCH to a locally owned instrument"]
    fn real_instrument_frame_benchmark() {
        use moose::mui::mui::vello::{self, vello_cpu::{Pixmap, RenderContext, Resources}};
        let patch = std::env::var_os("KONTRA_UI_BENCH_PATCH").expect("KONTRA_UI_BENCH_PATCH");
        let option = |key: &str, default: u32| std::env::var(key).ok().map(|v| v.parse().expect(key)).unwrap_or(default);
        let program = option("KONTRA_UI_BENCH_PROGRAM", 0);
        let mode = option("KONTRA_UI_BENCH_MODE", 1);
        assert!((1..=3).contains(&mode), "mode must be Original=1, KONTRA=2, Vectorized=3");
        let appearance = option("KONTRA_UI_BENCH_APPEARANCE", 0);
        assert!(appearance <= 2, "appearance must be Plain=0, Color=1, Artwork=2");
        let view_scale = std::env::var("KONTRA_UI_BENCH_VIEW_SCALE").ok().map(|v| v.parse::<f32>().expect("view scale")).unwrap_or(0.);
        assert!([0.,0.5,1.,2.].contains(&view_scale), "view scale must be fit=0, .5, 1 or 2");
        let device_scale = std::env::var("KONTRA_UI_BENCH_DEVICE_SCALE").ok().map(|v| v.parse::<f64>().expect("device scale")).unwrap_or(1.);
        assert!([1.,1.5,2.].contains(&device_scale), "device scale must be 1, 1.5 or 2");
        let started = Instant::now();
        let snapshot = std::env::var_os("KONTRA_UI_BENCH_SNAPSHOT");
        let instrument = Arc::new(if let Some(snapshot) = &snapshot {
            assert_eq!(program, 0, "snapshot restoration requires a single NKI program");
            import::read_snapshot(Path::new(&patch), Path::new(snapshot)).unwrap()
        } else {
            import::read_program(Path::new(&patch), program).unwrap()
        });
        let import_ms = started.elapsed().as_secs_f64() * 1000.;
        let started = Instant::now();
        let mut trace = crate::diagnostics::LoadTrace::new(Path::new(&patch), program, Some(0));
        let Some(mut part) = audit_one(&instrument, &mut Found::default(), &mut trace) else {
            println!("UI_BENCH status=skipped reason=no_performance_view program={program}");
            return;
        };
        part.program = program;
        if std::env::var_os("KONTRA_UI_BENCH_STAGED_NATIVE_ASSETS").is_some() {
            assert!(part.interface.as_ref().is_some_and(|interface| !interface.native_ui.is_empty()),
                "staged capture requires the actual authored NativeUI route");
            let sources = crate::native_ui::Package::load_sources(&instrument.path).unwrap();
            assert!(sources.images.is_empty() && sources.fonts.is_empty());
            assert!(!sources.image_metadata.is_empty());
            part.native_ui = Some(Arc::new(sources));
            println!("UI_STAGED_NATIVE_START {}", serde_json::json!({
                "decoded_images":0,"source_modules":part.native_ui.as_ref().unwrap().members.len(),
                "image_metadata":part.native_ui.as_ref().unwrap().image_metadata.len(),
                "native_entry":part.interface.as_ref().unwrap().native_ui,
                "scope":"production sources-only package before first app frame"
            }));
        }
        println!("UI_BENCH program={program} mode={mode} device_scale={device_scale} snapshot_restored={}", snapshot.is_some());
        println!("UI_BENCH import_ms={import_ms:.3} setup_ms={:.3} controls={} pictures={}",
            started.elapsed().as_secs_f64() * 1000., part.interface.as_ref().unwrap().controls.len(), part.pictures.len());
        if std::env::var_os("KONTRA_UI_BENCH_FONTS").is_some() {
            for (n, name) in part.interface.as_ref().unwrap().fonts.iter().enumerate() {
                let font = part.pictures.get(&artwork::font_key(name)).expect("requested bitmap font was loaded without fallback");
                assert_eq!(font.frames.len(), 256);
                println!("UI_BENCH_FONT id={} glyphs={} height={} A_advance={} i_advance={}", n+26, font.frames.len(), font.frames[0].height, font.frames[65].width, font.frames[105].width);
            }
        }
        if std::env::var_os("KONTRA_UI_BENCH_CAPTURE_ONLY").is_some() {
            let to = std::env::var_os("KONTRA_UI_BENCH_SHOT").expect("capture output PNG");
            shot_scaled(&part, mode as u8, Path::new(&to), device_scale).unwrap();
            println!("UI_CAPTURE {}", serde_json::json!({"build":crate::build_info::LABEL,"mode":mode,
                "device_scale":device_scale,"snapshot_restored":snapshot.is_some(),
                "page_callbacks":std::env::var("KONTRA_UI_BENCH_PAGE").ok(),
                "controls":part.interface.as_ref().unwrap().controls.len(),"pictures":part.pictures.len(),
                "wallpaper_loaded":part.wallpaper.is_some()}));
            return;
        }
        let shown = perf_view::layout(part.interface.as_ref().unwrap(), &part.pictures);
        let pictured = shown.iter()
            .filter(|c| matches!(c.kind, Kind::Knob | Kind::Slider) && c.picture.as_ref().is_some_and(|p| p.frames.len() > 1))
            .max_by(|a, b| (a.w * a.h).total_cmp(&(b.w * b.h)));
        let Some(changed) = pictured.or_else(|| shown.iter().find(|c| matches!(c.kind, Kind::Knob | Kind::Slider) && c.picture.is_none())) else {
            println!("UI_BENCH status=skipped reason=no_animated_knob_or_slider program={program}");
            return;
        };
        let mut changed_control = changed.control;
        println!("UI_BENCH control={changed_control} kind={:?} size={}x{} sprite_frames={}",
            changed.kind, changed.w, changed.h, changed.picture.as_ref().map_or(0, |p| p.frames.len()));
        let params = Arc::new(SamplerParams::new());
        params.selection.write().unwrap().appearance = appearance as u8;
        params.shared.libraries.edit(|s| s.view_scale = view_scale);
        if let Some(root) = std::env::var_os("KONTRA_UI_BENCH_LIBRARY_ROOT") {
            let root = PathBuf::from(root);
            assert!(instrument.path.starts_with(&root), "selected program belongs to its library");
            let library = crate::library::Library {
                name: std::env::var("KONTRA_UI_BENCH_LIBRARY_NAME").expect("library name"),
                hue: artwork::own_hue(&root), dir: root, ..Default::default()
            };
            let mut view = params.shared.view.lock().unwrap();
            view.artwork = artwork::scan(std::slice::from_ref(&library));
            view.shelf = Arc::new(crate::library::Shelf::new(vec![library]));
        }
        println!("UI_BENCH view_scale={view_scale} appearance={appearance} library_artwork={}", params.shared.view.lock().unwrap().artwork.len());
        params.selection.write().unwrap().parts.push(Part {
            path: instrument.path.to_string_lossy().into(),
            program,
            view: mode as u8,
            ..Default::default()
        });
        params.shared.view.lock().unwrap().parts[0] = part.clone();
        let mut ui = theme::ui();
        ui.set_scale(Some(device_scale));
        let art = Arc::<art::Art>::default();
        let mut draw = build(&params, Arc::default(), Arc::default(), Arc::default(), art.clone());
        let mut bridge = Bridge::new(params.clone());
        let (width, height) = ((1180. * device_scale).round() as u16, (900. * device_scale).round() as u16);
        let transform = vello::kurbo::Affine::scale(device_scale);
        let mut ctx = RenderContext::new(width, height);
        let mut resources = Resources::default();
        let mut cache = vello::Cache::default();
        let mut pixmap = Pixmap::new(width, height);
        let mut gpu = std::env::var_os("KONTRA_UI_BENCH_GPU").map(|_| {
            use vello::vello::wgpu;
            fn wait<F: std::future::Future>(future: F) -> F::Output {
                let mut future = std::pin::pin!(future);
                let mut context = std::task::Context::from_waker(std::task::Waker::noop());
                loop {
                    match future.as_mut().poll(&mut context) {
                        std::task::Poll::Ready(result) => return result,
                        std::task::Poll::Pending => std::thread::sleep(Duration::from_millis(1)),
                    }
                }
            }
            let instance = wgpu::Instance::default();
            let adapter = wait(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })).expect("GPU adapter");
            let info = adapter.get_info();
            println!("UI_BENCH adapter={info:?}");
            assert_ne!(info.device_type, wgpu::DeviceType::Cpu, "software rasterizer cannot measure physical GPU performance");
            let (device, queue) = wait(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
            let target = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("UI benchmark target"),
                size: wgpu::Extent3d { width: u32::from(width), height: u32::from(height), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let renderer = wait(vello::effects::GpuRenderer::new(&device, &queue, wgpu::TextureFormat::Rgba8Unorm,
                [u32::from(width), u32::from(height)], vello::effects::Budget::default())).unwrap();
            (device, renderer, target.create_view(&Default::default()))
        });
        let mut gpu_pixels = 0;
        let tree = draw(&mut ui, &mut bridge);
        ui.frame(tree, Some(Size::new(1180.,900.)), Input::default(),1./60.).unwrap();
        let art_started = Instant::now();
        while art.busy() && art_started.elapsed() < Duration::from_secs(10) { std::thread::sleep(Duration::from_millis(2)); }
        assert!(!art.busy(), "library appearance preparation completes outside measurement");
        let prefix = if mode == 2 { "ksp" } else { "kpv" };
        let visible = shown.iter().filter(|c| matches!(c.kind, Kind::Knob | Kind::Slider))
            .filter(|c| mode == 2 || c.picture.as_ref().is_none_or(|p| p.frames.len() > 1))
            .filter(|c| ui.scene().unwrap().surface(&format!("{prefix}-0-{}", c.control)).is_some_and(|s| {
                let r = s.frame;
                r.x >= 0. && r.y >= 0. && r.x + r.size.width <= 1180. && r.y + r.size.height <= 900.
            }))
            .max_by(|a,b| (a.w*a.h).total_cmp(&(b.w*b.h)));
        let Some(visible) = visible else {
            println!("UI_BENCH status=skipped reason=no_visible_controllable_surface program={program}");
            return;
        };
        changed_control = visible.control;
        println!("UI_BENCH visible_control={changed_control}");
        let frame_count = std::env::var("KONTRA_UI_BENCH_FRAMES").ok().and_then(|n| n.parse::<usize>().ok()).unwrap_or(24).clamp(4, 120);
        for changing in [false, true] {
            let mut times = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
            let mut edit_ms = Vec::new();
            let mut snapshot_ms = Vec::new();
            let mut changed_renders = 0;
            for frame in 0..frame_count + 8 {
                fitted::ready();
                if changing {
                    let drawing_interface = params.shared.view.lock().unwrap().parts[0].interface.clone().unwrap();
                    let control = &drawing_interface.controls[changed_control];
                    let bound = |name, default| match control.properties.get(name) {
                        Some(crate::ksp::Value::Int(n)) => *n,
                        _ => default,
                    };
                    let (lo, hi) = (bound("$CONTROL_PAR_MIN_VALUE", 0), bound("$CONTROL_PAR_MAX_VALUE", 127));
                    let start = Instant::now();
                    params.shared.edit_control(0, changed_control, if frame % 2 == 0 {lo} else {hi});
                    if frame >= 8 { edit_ms.push(start.elapsed().as_secs_f64() * 1000.); }
                }
                let start = Instant::now();
                let snapshot = params.shared.view.lock().unwrap();
                if frame >= 8 { snapshot_ms.push(start.elapsed().as_secs_f64() * 1000.); }
                drop(snapshot);
                let start = Instant::now();
                let tree = draw(&mut ui, &mut bridge);
                let built = Instant::now();
                ui.frame(tree, Some(Size::new(1180., 900.)), Input::default(), 1. / 60.).unwrap();
                let laid_out = Instant::now();
                let scene = ui.scene().unwrap();
                let scene_copy = scene.clone();
                let cloned = Instant::now();
                let painted = if let Some((device, renderer, target)) = &mut gpu {
                    let stats = renderer.render(&scene_copy, transform, target).unwrap();
                    gpu_pixels += stats.rendered_pixels;
                    if changing && frame >= 8 && stats.renders > 0 { changed_renders += 1; }
                    let painted = Instant::now();
                    device.poll(vello::vello::wgpu::PollType::wait_indefinitely()).unwrap();
                    painted
                } else {
                    ctx.reset();
                    vello::paint(&mut vello::Cpu { ctx: &mut ctx, resources: &mut resources, cache: &mut cache }, &scene_copy, transform).unwrap();
                    ctx.flush();
                    let painted = Instant::now();
                    ctx.render(&mut pixmap, &mut resources);
                    painted
                };
                let rasterized = Instant::now();
                if frame >= 8 {
                    for (samples, (a, b)) in times.iter_mut().zip([(start, built), (built, laid_out), (laid_out, cloned), (cloned, painted), (painted, rasterized)]) {
                        samples.push((b - a).as_secs_f64() * 1000.);
                    }
                }
            }
            if changing && gpu.is_some() {
                assert_eq!(changed_renders, frame_count, "each changed control value must produce a GPU render");
            }
            let renderer_stages = if gpu.is_some() { ["gpu_prepare_upload_submit", "gpu_wait"] } else { ["cpu_paint", "cpu_raster"] };
            for (stage, mut samples) in ["build", "layout", "scene_clone", renderer_stages[0], renderer_stages[1]].into_iter().zip(times)
                .chain([("edit_control", edit_ms), ("snapshot_lock", snapshot_ms)]) {
                if samples.is_empty() { continue; }
                samples.sort_by(f64::total_cmp);
                println!("UI_BENCH changing={changing} stage={stage} n={} mean_ms={:.3} median_ms={:.3} p99_ms={:.3}",
                    samples.len(), samples.iter().sum::<f64>() / samples.len() as f64, samples[samples.len()/2], samples[((samples.len()-1) as f64 * 0.99).ceil() as usize]);
            }
        }
        if std::env::var_os("KONTRA_UI_BENCH_CALLBACKS").is_some() {
            let (runtime, errors) = crate::engine::load_scripts(&instrument, instrument.script_state.clone(), 48000.);
            assert!(errors.is_empty(), "authored callback initialization failed");
            let mut runtime = runtime.expect("performance instrument runtime");
            select_benchmark_page(&mut runtime, &instrument, true);
            let interface = runtime.live().interface.expect("performance interface");
            let control = std::env::var("KONTRA_UI_BENCH_CONTROL_VARIABLE").ok().map_or(changed_control, |name| {
                interface.controls.iter().position(|c| c.variable == name).expect("requested callback control exists")
            });
            assert!(perf_view::layout(&interface, &part.pictures).iter().any(|shown| shown.control == control),
                "callback benchmark control must be visible after authored page selection");
            let bound = |name, default| match interface.controls[control].properties.get(name) {
                Some(crate::ksp::Value::Int(n)) => *n, _ => default,
            };
            let (low, high) = (bound("$CONTROL_PAR_MIN_VALUE", 0), bound("$CONTROL_PAR_MAX_VALUE", 127));
            println!("UI_BENCH callback_control={control} callback_variable={} callback_min={low} callback_max={high}", interface.controls[control].variable);
            let mut engine = crate::engine::Engine::default();
            let bank_started = Instant::now();
            let full_bank = std::env::var_os("KONTRA_UI_BENCH_FULL_BANK").is_some();
            let bank = if full_bank {
                crate::engine::Bank::load_counting(&instrument, 512 << 20, crate::engine::Streaming::Auto,
                    &runtime.init_controllers, &std::sync::atomic::AtomicU32::new(0)).unwrap()
            } else { crate::engine::Bank::load_bare(&instrument).unwrap() };
            println!("UI_BENCH_BANK full={full_bank} load_ms={:.3} resident_bytes={} zones={} skipped={}",
                bank_started.elapsed().as_secs_f64()*1000., bank.bytes, bank.zones().len(), bank.skipped_zones);
            engine.set_bank(Some(Box::new(bank)));
            engine.set_fx(crate::engine::effects(&instrument, Some(&runtime), 48000.));
            engine.set_script(Some(runtime));
            let mut observer_draw = None;
            let mut rendered = Vec::new();
            let mut rendered_changes = 0;
            let mut publication_ms = Vec::new();
            let mut frame_stages = [Vec::new(), Vec::new(), Vec::new()];
            let mut cpu_stages = [Vec::new(), Vec::new(), Vec::new()];
            // Weak identities reserve addresses without keeping source pixels live.
            let mut seen_image_buffers = std::collections::HashMap::new();
            let mut frame_work = Vec::new();
            let initial_fitted = fitted::audit_counts(0);
            let mut previous_fitted = initial_fitted;
            let partition = std::env::var_os("KONTRA_UI_BENCH_PARTITION").is_some();
            let cold_cpu_cache = std::env::var_os("KONTRA_UI_BENCH_COLD_CPU_CACHE").is_some();
            assert!(!cold_cpu_cache || (gpu.is_none() && std::env::var_os("KONTRA_UI_BENCH_BUILD_ONLY").is_none()),
                "cold CPUCache comparison requires the software reference painter");
            let mut last_published = None;
            let mut published_arc_owners = Vec::new();
            let mut publication_rows = Vec::new();
            let mut previous_revision = None;
            let mut observer = |published: &Arc<SamplerParams>| -> anyhow::Result<()> {
                if partition { last_published = Some(published.clone()); }
                if observer_draw.is_none() {
                    published.selection.write().unwrap().parts[0].view = mode as u8;
                    published.selection.write().unwrap().appearance = appearance as u8;
                    published.shared.libraries.edit(|s| s.view_scale = view_scale);
                    let mut view = published.shared.view.lock().unwrap();
                    let prepared = params.shared.view.lock().unwrap();
                    view.artwork = prepared.artwork.clone();
                    view.shelf = prepared.shelf.clone();
                    view.parts[0].pictures = part.pictures.clone();
                    view.parts[0].wallpaper = part.wallpaper.clone();
                    drop(view);
                    observer_draw = Some((Bridge::new(published.clone()), build(published, Arc::default(), Arc::default(), Arc::default(), art.clone())));
                    // The direct-stage benchmark otherwise warmed this cache.
                    // Opt in to one real cold observer frame, then retain it.
                    if cold_cpu_cache { cache = vello::Cache::default(); }
                }
                fitted::ready();
                // Native MuiEditor publishes before its changed fingerprint;
                // this headless pump has no Watch, so perform the same stage.
                if partition {
                    let view = published.shared.view.lock().unwrap();
                    if let Some(source) = &view.parts[0].interface {
                        published_arc_owners.push((Arc::strong_count(source), Arc::weak_count(source)));
                    }
                }
                let start = Instant::now();
                published.shared.publish_live(true);
                publication_ms.push(start.elapsed().as_secs_f64()*1000.);
                if partition {
                    let view = published.shared.view.lock().unwrap();
                    let part = &view.parts[0];
                    let revision = part.live_revisions.map(|(interface, _)| interface);
                    if revision != previous_revision {
                        if let Some((copied, total, reused)) = part.publication_rows {
                            publication_rows.push(serde_json::json!({"copied_rows":copied,"total_rows":total,"reused_controls_allocation":reused}));
                        }
                        previous_revision = revision;
                    }
                }
                let (bridge, draw) = observer_draw.as_mut().unwrap();
                let builds = perf_view::control_builds();
                let stage = Instant::now();
                let tree = draw(&mut ui, bridge);
                frame_stages[0].push(stage.elapsed().as_secs_f64()*1000.);
                let stage = Instant::now();
                ui.frame(tree, Some(Size::new(1180.,900.)), Input::default(),1./60.).unwrap();
                frame_stages[1].push(stage.elapsed().as_secs_f64()*1000.);
                let counts = fitted::audit_counts(0);
                let mut work = serde_json::json!({
                    "control_builds":perf_view::control_builds()-builds,
                    "fitted_queued":counts.0-previous_fitted.0,
                    "fitted_completed":counts.1-previous_fitted.1,
                    "fitted_generation":fitted::generation(0),
                });
                if partition {
                    let scene = ui.scene().unwrap();
                    let mut frame_images = std::collections::HashSet::new();
                    let mut image_paints = 0;
                    let mut new_buffers = 0;
                    let mut new_bytes = 0;
                    for paint in &scene.paint {
                        if let moose::mui::mui::scene::Paint::Image { image, .. } = &paint.paint {
                            image_paints += 1;
                            let key = Arc::as_ptr(&image.rgba) as *const u8 as usize;
                            frame_images.insert(key);
                            if let std::collections::hash_map::Entry::Vacant(entry) = seen_image_buffers.entry(key) {
                                entry.insert(Arc::downgrade(&image.rgba));
                                new_buffers += 1;
                                new_bytes += image.rgba.len();
                            }
                        }
                    }
                    work["scene_images"] = serde_json::json!({
                        "paints":image_paints,"unique_frame_buffers":frame_images.len(),
                        "new_buffers":new_buffers,"new_rgba_bytes":new_bytes,
                        "unique_buffers_seen":seen_image_buffers.len(),
                        "scope":"direct bitmap paint identities; not renderer cache entries or texture uploads",
                    });
                }
                frame_work.push(work);
                previous_fitted = counts;
                let stage = Instant::now();
                let scene = ui.scene().unwrap();
                if std::env::var_os("KONTRA_UI_BENCH_BUILD_ONLY").is_some() {
                    // The concurrent host probe isolates callback delivery and
                    // UI build/layout from software reference rasterization.
                } else if let Some((device, renderer, target)) = &mut gpu {
                    let stats = renderer.render(scene, transform, target).unwrap();
                    rendered_changes += usize::from(stats.renders > 0);
                    device.poll(vello::vello::wgpu::PollType::wait_indefinitely()).unwrap();
                } else {
                    let at = Instant::now();
                    ctx.reset();
                    vello::paint(&mut vello::Cpu {ctx: &mut ctx, resources: &mut resources, cache: &mut cache}, scene, transform).unwrap();
                    cpu_stages[0].push(at.elapsed().as_secs_f64()*1000.);
                    let at = Instant::now();
                    ctx.flush();
                    cpu_stages[1].push(at.elapsed().as_secs_f64()*1000.);
                    let at = Instant::now();
                    ctx.render(&mut pixmap, &mut resources);
                    cpu_stages[2].push(at.elapsed().as_secs_f64()*1000.);
                    frame_work.last_mut().unwrap()["cpu_stage_ms"] = serde_json::json!({
                        "encode_and_reset":cpu_stages[0].last(),"flush":cpu_stages[1].last(),"raster":cpu_stages[2].last(),
                    });
                }
                frame_stages[2].push(stage.elapsed().as_secs_f64()*1000.);
                rendered.push(start.elapsed().as_secs_f64()*1000.);
                Ok(())
            };
            let edits = option("KONTRA_UI_BENCH_EDITS", 24).clamp(4,120) as usize;
            let frames = option("KONTRA_UI_BENCH_HOST_FRAMES",128) as usize;
            let (report, _) = crate::plugin::bench_ui_worker(engine, instrument.clone(), program, control, low, high, frames, edits, Some(&mut observer)).unwrap();
            assert!(!rendered.is_empty(), "native publication must reach the observer");
            assert_eq!(report["editor_frames"].as_u64().unwrap(), rendered.len() as u64, "every scheduled editor frame reaches the native UI renderer");
            assert!(report["latest_edits_published"].as_u64().unwrap_or(0) > 0, "actual callback state must settle at least one queued edit: {report}");
            rendered.sort_by(f64::total_cmp);
            publication_ms.sort_by(f64::total_cmp);
            let summarize = |mut samples: Vec<f64>| {
                if samples.is_empty() { return serde_json::json!({"n":0}); }
                samples.sort_by(f64::total_cmp);
                serde_json::json!({"n":samples.len(),"mean":samples.iter().sum::<f64>()/samples.len() as f64,
                    "p99":samples[((samples.len()-1) as f64*0.99).ceil() as usize]})
            };
            let stages: serde_json::Map<String, serde_json::Value> = ["build", "layout", "render"]
                .into_iter().zip(frame_stages).map(|(name,samples)| (name.into(),summarize(samples))).collect();
            let cpu_cache_groups: serde_json::Map<String, serde_json::Value> = ["encode_and_reset", "flush", "raster"]
                .into_iter().zip(&cpu_stages).map(|(name,samples)| (name.into(),serde_json::json!({
                    "first_observer":summarize(samples.first().copied().into_iter().collect()),
                    "later_observers":summarize(samples.iter().skip(1).copied().collect()),
                }))).collect();
            let cpu_stage_ms: serde_json::Map<String, serde_json::Value> = ["encode_and_reset", "flush", "raster"]
                .into_iter().zip(cpu_stages).map(|(name,samples)| (name.into(),summarize(samples))).collect();
            println!("UI_BENCH_CALLBACK {}", serde_json::json!({"worker":report,
                "stage_ms":stages,"cpu_stage_ms":cpu_stage_ms,
                "cpu_cache":{"reset_before_first_observer":cold_cpu_cache,
                    "scope":"MUI CPUCache only; existing CPUResources/context remain; new scene identities are not cache misses",
                    "groups":cpu_cache_groups},"frame_work":frame_work,
                "fitted_jobs":{"queued":previous_fitted.0-initial_fitted.0,"completed":previous_fitted.1-initial_fitted.1},"observer_frames":rendered.len(),"changed_gpu_frames":rendered_changes,
                "live_publication_ms":{"mean":publication_ms.iter().sum::<f64>()/publication_ms.len() as f64,"p99":publication_ms[((publication_ms.len()-1) as f64*0.99).ceil() as usize]},
                "callback_published_render_ms":{"mean":rendered.iter().sum::<f64>()/rendered.len() as f64,"p99":rendered[((rendered.len()-1) as f64*0.99).ceil() as usize]}}));
            if let Some(published) = last_published {
                let mut costs = cost_partition(&published);
                costs["published_arc_owners"] = serde_json::json!({
                    "scope":"before native publication; no extra interface Arc is retained by this counter",
                    "frames":published_arc_owners.len(),
                    "unique_strong_frames":published_arc_owners.iter().filter(|(strong,_)| *strong == 1).count(),
                    "max_strong":published_arc_owners.iter().map(|(strong,_)| *strong).max(),
                    "max_weak":published_arc_owners.iter().map(|(_,weak)| *weak).max(),
                });
                costs["publication_rows"] = serde_json::json!({
                    "scope":"source revision changes only; stamp candidates plus independently compared menu rows; initial or retained-reader snapshots copy all rows",
                    "updates":publication_rows,
                });
                println!("UI_BENCH_COST {}", costs);
            }
        }
        if let Some(to) = std::env::var_os("KONTRA_UI_BENCH_SHOT") {
            ctx.reset();
            vello::paint(&mut vello::Cpu { ctx: &mut ctx, resources: &mut resources, cache: &mut cache }, ui.scene().unwrap(), transform).unwrap();
            ctx.flush();
            ctx.render(&mut pixmap, &mut resources);
            let rgba: Vec<u8> = pixmap.clone().take_unpremultiplied().iter().flat_map(|p| [p.r,p.g,p.b,p.a]).collect();
            moose::core::screenshot::save_png(Path::new(&to), &rgba, u32::from(width), u32::from(height));
        }
        if gpu.is_some() {
            assert!(gpu_pixels > 0, "real GPU UI renders pixels");
        } else {
            assert!(pixmap.take_unpremultiplied().iter().any(|p| p.a > 0), "real UI renders pixels");
        }
    }
}
