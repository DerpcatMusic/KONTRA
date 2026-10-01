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
        Kind::Area => "mouse area",
        Kind::Meter => "level meter",
        Kind::Waveform => "waveform",
        Kind::Other => "other",
    }
}

/// What the laid-out view `shown` of `u` gets wrong, as far as can be told
/// without Kontakt to compare with.
fn inspect(u: &Interface, shown: &[Shown], pictures: &HashMap<String, Arc<artwork::Picture>>, found: &mut Found) {
    let (w, h) = (f64::from(u.width), f64::from(u.height));
    for s in shown {
        let c = &u.controls[s.control];
        let kind = kind_name(s.kind);
        if let Some(reason) = crate::diagnostics::widget_limit(&c.kind) { found.add(format!("{}: {reason}", c.kind)); }
        *found.kinds.entry(kind.into()).or_default() += 1;
        let named = prop(c, "$CONTROL_PAR_PICTURE");
        if !named.is_empty() && !pictures.contains_key(named) {
            found.add("missing picture");
        } else if s.kind == Kind::Waveform {
            found.add("waveform: zone wave is not drawn");
        } else if s.kind == Kind::Other && crate::diagnostics::widget_limit(&c.kind).is_none() {
            found.add(format!("unsupported UI widget: {}", c.kind));
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
    // One inside another (a row with its own button) is by design.
    let takes = |s: &&Shown| !matches!(s.kind, Kind::Label | Kind::Area);
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

/// `kontakto audit-ui [roots] [--shots DIR] [--json out.json]`.
pub fn run(args: &[String]) -> anyhow::Result<()> {
    let opt = |flag: &str| args.iter().position(|a| a == flag);
    let (shots_at, json_at) = (opt("--shots"), opt("--json"));
    let shots = shots_at.and_then(|i| args.get(i + 1)).map(PathBuf::from);
    let json = json_at.and_then(|i| args.get(i + 1));
    anyhow::ensure!(shots.is_none() || cfg!(feature = "shots"), "Screenshots require cargo build --release --features shots");
    let mut roots: Vec<PathBuf> = (args.iter().enumerate())
        .filter(|&(i, a)| !a.starts_with("--") && [shots_at, json_at].iter().all(|o| o.is_none_or(|o| i != o + 1)))
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
                let shown = audit_one(&i, &mut found, &mut trace);
                let has_view = shown.is_some();
                trace.detail("performance_view", has_view);
                let mut rendered = true;
                if let (Some(dir), Some(part)) = (&shots, shown) {
                    trace.stage("render");
                    let stem = format!("{}-{program}", i.name.replace(['/', '\\'], "_"));
                    for (code, mode) in [(1, "original"), (3, "vectorized")] {
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
fn audit_one(i: &Arc<import::Instrument>, found: &mut Found, trace: &mut crate::diagnostics::LoadTrace) -> Option<PartView> {
    trace.stage("scripts");
    let (rt, errors) = crate::engine::load_scripts(i, i.script_state.clone(), 48_000.);
    for e in &errors {
        trace.issue("scripts", "initialization_failed", e);
        found.add(format!("script: {}", general(e.split_once(": ").map_or(e, |x| x.1))));
    }
    let mut rt = rt?;
    // A second of audio: listeners and waits run as they would once playing,
    // against the instrument's groups, modulators and effects.
    trace.stage("script_callbacks");
    let mut engine = crate::engine::ScriptSetup::new(i, 48_000.0);
    (0..100).for_each(|_| rt.process(&mut engine, 480));
    let script = crate::plugin::script_interface(Some(&rt));
    for d in script.status.lines().filter(|d| !d.trim().is_empty()) {
        trace.issue("scripts", crate::diagnostics::code(d), d);
        found.add(format!("script: {}", general(d)));
    }
    let Some(u) = script.interface.clone().filter(|u| u.performance && !u.controls.is_empty()) else {
        found.problems.retain(|k, _| k.starts_with("script"));
        return None;
    };
    found.size = (u.width.max(0) as u32, u.height.max(0) as u32);
    let names = u.controls.iter().map(|c| prop(c, "$CONTROL_PAR_PICTURE")).filter(|n| !n.is_empty());
    trace.stage("artwork");
    trace.detail("controls", u.controls.len());
    let (pictures, errors) = artwork::pictures_report(&i.path, names);
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
        && (i64::from(w.frames[0].width) - i64::from(u.width)).abs() > 2
    {
        found.add("wallpaper narrower or wider than the view");
    }
    let shown = perf_view::layout(&u, &pictures);
    inspect(&u, &shown, &pictures, found);
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
#[cfg(not(feature = "shots"))]
fn shot(_: &PartView, _: u8, _: &Path) -> anyhow::Result<()> {
    anyhow::bail!("built without the CPU renderer: cargo build --release --features shots")
}

#[cfg(feature = "shots")]
fn shot(part: &PartView, code: u8, to: &Path) -> anyhow::Result<()> {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let (width, height) = (1180u16, 900u16);
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
    let mut build = build(&p, Arc::default(), Arc::default(), Arc::default(), Arc::default());
    let mut bridge = Bridge::new(p.clone());
    for _ in 0..8 {
        let root = build(&mut ui, &mut bridge);
        ui.frame(root, Some(Size::new(f64::from(width), f64::from(height))), Input::default(), 1. / 60.)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    }
    let scene = ui.scene().ok_or_else(|| anyhow::anyhow!("nothing drawn"))?;
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    vello::paint(
        &mut vello::Cpu { ctx: &mut ctx, resources: &mut resources, cache: &mut vello::Cache::default() },
        scene,
        vello::kurbo::Affine::IDENTITY,
    )
    .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut resources);
    let rgba: Vec<u8> = pix.take_unpremultiplied().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
    moose::core::screenshot::save_png(to, &rgba, u32::from(width), u32::from(height));
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn numbers_do_not_split_a_problem() {
        assert_eq!(super::general("Script 2: KSP line 41: no x1"), "Script N: KSP line N: no xN");
    }
}
