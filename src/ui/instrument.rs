//! A part's instrument: its performance controls, key/velocity mapping,
//! details, and plain-spoken notices.

use super::{Cx, panel, perf_view, theme::*};
use crate::library::ViewMode;
use crate::import::Instrument;
use moose::mui::mui::prelude::*;
use std::path::Path;
use std::sync::Arc;

/// The instrument loaded in `slot`, once it matches the part.
pub(super) fn instrument_of<'a>(cx: &'a Cx, slot: usize) -> Option<&'a Arc<Instrument>> {
    let part = cx.selection.parts.get(slot).filter(|p| p.uvi.is_none() && !p.path.is_empty())?;
    let v = &cx.view.parts[slot];
    v.instrument
        .as_ref()
        .filter(|i| i.path == Path::new(&part.path) && v.program == part.program)
}

/// The native program's catalog name, falling back to its actual member name.
/// It never treats a bank member as a filesystem import path.
pub(super) fn native_name(cx: &Cx, slot: usize) -> Option<String> {
    let source = cx.selection.parts.get(slot)?.uvi.as_ref()?;
    Some(cx.view.shelf.uvi.get(&source.bank)
        .and_then(|bank| bank.presets.iter().find(|preset| &preset.source == source && !preset.name.is_empty()))
        .map_or_else(|| super::header::stem(&source.member.replace('\\', "/")), |preset| preset.name.clone()))
}

fn native_problem(status: &str) -> bool {
    matches!(status, "The UVI instrument could not be loaded."
        | "The current audio configuration is unsupported by UVI playback.")
}

/// Includes the native loader's terminal states, rather than only Kontakt imports.
pub(super) fn failed(cx: &Cx, slot: usize) -> bool {
    cx.view.parts.get(slot).is_some_and(|v| {
        if cx.selection.parts.get(slot).is_some_and(|p| p.uvi.is_some()) {
            !v.loading && native_problem(&v.status)
        } else { v.status.starts_with("Load failed") }
    })
}

fn native_wait(loading: bool, status: &str) -> String {
    if !cfg!(feature = "uvi") {
        "UVI support is disabled in this version.".into()
    } else if native_problem(status) {
        "Instrument could not be loaded. See the error above or Logs for details.".into()
    } else if loading {
        if status.contains("UVI") { status.into() } else { "Loading UVI instrument…".into() }
    } else if status == "UVI instrument" {
        "This instrument is ready. Performance controls have not been published.".into()
    } else {
        "Waiting for the UVI instrument to start…".into()
    }
}

/// A native source remains playable through its authored Rack controls while
/// the app's Kontakt-only editing surfaces are unavailable.
pub(super) fn native_unavailable(what: &str, help: &str) -> El {
    col![body(format!("{what} is unavailable for UVI instruments.")),
        caption(help.to_owned()).fill(secondary()).lines(3)]
        .gap(SPACE).align(Align::Start).pad(INSET).flex(1).min_h(0)
}

/// The instrument loaded for the selected part.
pub(super) fn current<'a>(cx: &'a Cx) -> Option<&'a Arc<Instrument>> {
    instrument_of(cx, cx.state.selected)
}

/// The empty rack's view.
pub fn welcome(cx: &Cx) -> El {
    let mut lines = vec![
        title("Pick an instrument").text_weight(Weight::SEMIBOLD),
        body("Choose a library on the left and click an instrument, or drag it onto the rack. Multis load the whole rack.")
            .fill(secondary())
            .lines(3)
            .max_size(Size::new(TEXT * 35., CONTROL * 3.)),
    ];
    if !cx.view.multi_status.is_empty() {
        lines.push(
            caption(cx.view.multi_status.clone())
                .fill(secondary())
                .lines(2),
        );
    }
    col![spacer(), col(lines).gap(SPACE).align(Align::Start), spacer()]
        .align(Align::Center)
        .pad(INSET * 3.)
        .flex(1)
        .min_h(0)
}

/// Why `slot` is silent or incomplete, in words a player can act on.
pub fn notices(cx: &Cx, slot: usize) -> Option<El> {
    let v = &cx.view.parts[slot];
    let mut out = Vec::new();
    if cx.selection.parts.get(slot).is_some_and(|p| p.uvi.is_some()) {
        if !v.loading && native_problem(&v.status) {
            out.push(banner(Role::Danger, v.status.clone()));
        }
        return (!out.is_empty()).then(|| col(out).gap(TIGHT).pad((INSET, SPACE)).shrink(0));
    }
    if v.status == "Loading snapshot…" {
        out.push(banner(Role::Ink, v.status.clone()));
    } else if let Some(reason) = v.status.strip_prefix("Snapshot was not loaded: ") {
        out.push(banner(Role::Danger, format!("Snapshot was not loaded: {reason}")));
    }
    if let Some(reason) = v.status.strip_prefix("Load failed: ") {
        let still = if v.active.is_empty() {
            String::new()
        } else {
            format!(" Still playing: {}.", v.active)
        };
        out.push(banner(
            Role::Danger,
            format!("This instrument could not be loaded: {reason}.{still}"),
        ));
    }
    if let Some(i) = instrument_of(cx, slot) {
        if let Some(w) = i.warnings.iter().find(|w| w.contains("read back as zeros")) {
            out.push(banner(Role::Warning, sentence(w)));
        } else if v.status.contains("zones skipped") || !i.missing_samples.is_empty() {
            let silent = i.zones.iter().filter(|z| !z.available).count();
            out.push(banner(
                Role::Warning,
                format!(
                    "Some samples are missing{}, so parts of this instrument stay silent. Repair the library in Native Access or check its folder.",
                    if silent > 0 { format!(" ({silent} zones)") } else { String::new() }
                ),
            ));
        }
    }
    (!out.is_empty()).then(|| col(out).gap(TIGHT).pad((INSET, SPACE)).shrink(0))
}

/// Capitalize and end with a period.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    out.push_str(chars.as_str());
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

/// `slot`'s performance controls, rebuilt natively, or a word on why there are none.
/// Everything [`stage`] reads, hashed: while it holds, the stage is kept as
/// drawn and not built again.
pub fn stage_deps(ui: &Ui, cx: &Cx, slot: usize) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let v = &cx.view.parts[slot];
    let original = instrument_of(cx, slot).is_some() && perf_view::shows(cx, slot) != ViewMode::Kontra;
    let at = |a: Option<*const ()>| a.map_or(0, |p| p as usize);
    let mut h = DefaultHasher::new();
    slot.hash(&mut h);
    if let Some(source) = cx.selection.parts.get(slot).and_then(|p| p.uvi.as_ref()) {
        source.hash(&mut h);
        #[cfg(feature = "uvi")]
        {
            at(v.uvi_ui.as_ref().map(|s| Arc::as_ptr(s).cast())).hash(&mut h);
            cx.p.shared.part(slot).map(|p| p.uvi_generation.load(std::sync::atomic::Ordering::Acquire)).hash(&mut h);
            cx.p.shared.uvi_activation_epoch().hash(&mut h);
            ui.scene().and_then(|s| s.surface(&format!("part-{slot}")))
                .map(|s| s.frame.size.width.to_bits()).hash(&mut h);
        }
    }
    (v.loading, &v.status).hash(&mut h);
    v.live_revisions.hash(&mut h);
    for edit in v.edited_values() { edit.hash(&mut h); }
    at(v.interface.as_ref().map(|i| Arc::as_ptr(i).cast())).hash(&mut h);
    at(instrument_of(cx, slot).map(|i| Arc::as_ptr(i).cast())).hash(&mut h);
    (Arc::as_ptr(&v.pictures) as usize, &v.interface_status).hash(&mut h);
    if let (Some(cache), Some(interface)) = (cx.state.panels.get(&slot), &v.interface) {
        panel::values(cache, interface).hash(&mut h);
    }
    cx.state.held.filter(|(p, ..)| *p == slot).map(|(_, c, v)| (c, v.to_bits())).hash(&mut h);
    panel::deps(cx, slot).hash(&mut h);
    original.hash(&mut h);
    if original {
        (perf_view::deps(ui, cx, slot), super::fitted::generation(slot)).hash(&mut h);
    }
    h.finish()
}

pub fn stage(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    if cx.selection.parts.get(slot).is_some_and(|p| p.uvi.is_some()) {
        let v = &cx.view.parts[slot];
        let status = native_wait(v.loading, &v.status);
        if native_problem(&v.status) {
            return caption(status.clone()).named(status).fill(secondary()).lines(3).pad(INSET)
                .id(format!("stage-{slot}"));
        }
        #[cfg(feature = "uvi")]
        if let Some((published, current)) = native_panel(cx, slot) {
            if let Some(snapshot) = published.snapshots.iter().rev().find(|s| s.root.performance_view) {
                let shared = &cx.p.shared;
                return super::uvi_instrument::view(ui, cx.state.uvi.entry(slot).or_default(), slot,
                    current, published.stamp, snapshot, &published.pictures,
                    |stamp, input| shared.edit_uvi(slot, stamp, input.edit));
            }
            return caption("This instrument has no performance controls.").fill(secondary()).pad(INSET);
        }
        return caption(status.clone()).named(status).fill(secondary()).lines(3).pad(INSET)
            .id(format!("stage-{slot}"));
    }
    let page = stage_page(ui, cx, slot);
    let pages = cx.view.parts[slot].script_pages.clone();
    if pages.views.len() < 2 { return page; }
    let selected = cx.view.parts[slot].script_slot;
    let mut tabs = Vec::with_capacity(pages.views.len());
    for p in &pages.views {
        let (clicked, tab) = tab(ui, format!("script-page-{slot}-{}", p.slot), &p.title, p.slot == selected);
        if clicked && cx.p.shared.select_script_page(slot, cx.view.parts[slot].script_epoch, p.slot) {
            cx.state.held = None;
            cx.state.typing = None;
            cx.state.reveal = Some(slot);
        }
        tabs.push(tab);
    }
    col![page, row(tabs).gap(0).w(Len::Pct(100.)).min_w(0).id(format!("script-pages-{slot}"))]
        .gap(0).align(Align::Stretch).w(Len::Pct(100.))
}

#[cfg(feature = "uvi")]
pub(super) fn native_panel(cx: &Cx, slot: usize) -> Option<(Arc<crate::plugin::uvi_ui::Published>, crate::uvi::worker::Stamp)> {
    let source = cx.selection.parts.get(slot)?.uvi.as_ref()?;
    let view = cx.view.parts.get(slot)?;
    if native_problem(&view.status) { return None; }
    let published = view.uvi_ui.clone()?;
    if !view.uvi_matches(source, published.stamp) { return None; }
    let generation = cx.p.shared.part(slot)?.uvi_generation.load(std::sync::atomic::Ordering::Acquire);
    Some((published.clone(), crate::uvi::worker::Stamp { epoch: cx.p.shared.uvi_activation_epoch(), generation,
        frame: published.stamp.frame }))
}

fn stage_page(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let loaded = instrument_of(cx, slot).is_some();
    if loaded && perf_view::shows(cx, slot) != ViewMode::Kontra {
        // The articulation setup follows the controls whichever view shows them.
        if let Some(interface) = cx.view.parts[slot].interface.clone() {
            let pictures = cx.view.parts[slot].pictures.clone();
            let sections = panel::cached(cx.state.panels.entry(slot).or_default(), &interface, &pictures);
            panel::sync(cx, slot, &sections);
        }
        return perf_view::view(ui, cx, slot).id(format!("stage-{slot}"));
    }
    let v = &cx.view.parts[slot];
    let scripted = instrument_of(cx, slot).is_some_and(|i| !i.scripts.is_empty());
    let sections = match &v.interface {
        Some(interface) if loaded => panel::cached(cx.state.panels.entry(slot).or_default(), interface, &v.pictures),
        _ => Arc::default(),
    };
    if !sections.is_empty() {
        return panel::view(ui, cx, slot, &sections).id(format!("stage-{slot}"));
    }
    // A script that failed to load says why, not only that nothing shows.
    let failed = v
        .interface_status
        .lines()
        .find(|l| {
            l.starts_with("Slot ")
                && l.split_once(": ").is_some_and(|(slot, error)| {
                    !slot.contains(" line") && !error.starts_with("callback disabled")
                })
        });
    let text = if !loaded {
        if v.loading {
            "Loading instrument…"
        } else if v.status.starts_with("Load failed: ") {
            "Instrument could not be loaded. See the error above or Logs for details."
        } else {
            "No instrument is loaded."
        }.to_owned()
    } else if let (true, Some(error)) = (scripted, failed) {
        format!("This instrument's script shows no controls. {}", sentence(error))
    } else if scripted {
        "This instrument's script shows no controls.".to_owned()
    } else {
        "This instrument has no performance controls. Play it from the keyboard.".to_owned()
    };
    row![caption(text.clone()).fill(secondary()).lines(3).min_w(0)]
        .align(Align::Center)
        .named(text)
        .pad(INSET)
        .w(Len::Pct(100.))
        .id(format!("stage-{slot}"))
}

/// Groups on the left; the selected group's zones on a key × velocity grid.
pub fn mapping(ui: &mut Ui, cx: &mut Cx) -> El {
    if cx.part().is_some_and(|part| part.uvi.is_some()) {
        return native_unavailable("Key and velocity mapping",
            "Use this instrument’s Rack controls for its supported settings.");
    }
    let instrument = current(cx).cloned();
    let slot = cx.state.selected;
    let group = cx.part().map_or(0, |p| p.group);
    let mut groups = Vec::new();
    if let Some(i) = &instrument {
        for (n, g) in i.groups.iter().enumerate() {
            let label = if g.name.is_empty() {
                format!("Group {}", n + 1)
            } else {
                g.name.clone()
            };
            let (hit, el) = action(ui, format!("group-{n}"), &label, group == n as u32);
            if hit {
                cx.selection.parts[slot].group = n as u32;
            }
            groups.push(el.lines(1).min_w(0).w(Len::Pct(100.)).shrink(0));
        }
    }
    let zones: Vec<_> = instrument
        .iter()
        .flat_map(|i| i.zones.iter())
        .filter(|z| z.group == group as usize)
        .map(|z| {
            (
                z.low_key,
                z.high_key,
                z.low_velocity,
                z.high_velocity,
                z.available,
            )
        })
        .collect();
    let grid = canvas(move |s| {
        let mut draw = Vec::new();
        for n in (0..128).step_by(12) {
            draw.push(Draw::fill(
                rect(f64::from(n) / 128. * s.width, 0., 1., s.height),
                Role::Ink.alpha(0.08),
            ));
        }
        for v in [32., 64., 96.] {
            draw.push(Draw::fill(
                rect(0., s.height * (1. - v / 128.), s.width, 1.),
                Role::Ink.alpha(0.06),
            ));
        }
        for &(lo, hi, lv, hv, available) in &zones {
            let x = f64::from(lo) / 128. * s.width;
            let y = f64::from(127 - hv) / 128. * s.height;
            let w = f64::from(hi.saturating_sub(lo) + 1) / 128. * s.width;
            let h = (f64::from(hv.saturating_sub(lv) + 1) / 128. * s.height).max(1.);
            let fill = if available {
                Role::Ink.alpha(0.35)
            } else {
                Role::Danger.alpha(0.25)
            };
            draw.push(Draw::fill(
                rect(x, y, (w - 1.).max(1.), (h - 1.).max(1.)),
                fill,
            ));
        }
        draw
    })
    .flex(1)
    .min_h(0)
    .fill(Role::Field)
    .radius(0)
    .clip()
    .named("Selected group key and velocity mapping");
    row![
        col(groups)
            .gap(1)
            .w(SIDEBAR_MIN)
            .shrink(0)
            .min_h(0)
            .scroll()
            .id("groups-scroll"),
        col![
            grid,
            row![
                caption(note_name(0)),
                spacer(),
                caption("Key × velocity"),
                spacer(),
                caption(note_name(127))
            ]
            .shrink(0)
        ]
        .gap(SPACE)
        .flex(1)
        .min_w(0)
        .min_h(0)
    ]
    .gap(INSET)
    .pad(INSET)
    .flex(1)
    .min_h(0)
}

/// What was loaded and what could not be.
pub fn info(ui: &mut Ui, cx: &mut Cx) -> El {
    let (logs, logs_el) = action(ui, "info-open-logs", "Open Logs for this load", false);
    if logs {
        let load = if cx.part().is_some_and(|part| part.uvi.is_some()) { String::new() } else {
            cx.part_view().load_report.as_ref().map(|report| {
            report["load_id"].as_str().map(str::to_owned)
                .or_else(|| report["load_id"].as_u64().map(|id| id.to_string())).unwrap_or_default()
            }).unwrap_or_default()
        };
        cx.state.logs.for_load(&load);
        cx.state.tab = super::Tab::Logs;
    }
    let v = cx.part_view();
    if let Some(source) = cx.part().and_then(|part| part.uvi.as_ref()) {
        let status = if cfg!(feature = "uvi") && (native_problem(&v.status)
            || v.status == "UVI instrument" || v.loading && v.status.contains("UVI")) {
            v.status.clone()
        } else { native_wait(v.loading, &v.status) };
        let mut rows = vec![section("UVI instrument"),
            body(native_name(cx, cx.state.selected).unwrap_or_default()).lines(2),
            caption(format!("Bank: {}", source.bank.display())).fill(secondary()).lines(4),
            caption(format!("Program: {}", source.member)).fill(secondary()).lines(4),
            body(status).lines(3).id("native-instrument-status"),
            caption("Playback and controls follow this UVI program. Key mapping and the Sound editor are unavailable; use the instrument’s Rack controls.")
                .fill(secondary()).lines(4), logs_el];
        if let Some(path) = crate::diagnostics::log_path() {
            rows.push(caption(format!("Log file: {}", path.display())).fill(Role::Dim).lines(4));
        }
        let mut text = col(rows).align(Align::Start).gap(SPACE);
        if let Some(surface) = ui.scene().and_then(|scene| scene.surface("details-scroll")) {
            text = text.w((surface.frame.size.width - 2. * INSET).max(0.));
        }
        return col![text].pad(INSET).flex(1).min_h(0).scroll().id("details-scroll");
    }
    let mut rows = Vec::new();
    if v.loading {
        rows.push(body(v.status.clone()).lines(2));
        if let Some(path) = crate::diagnostics::log_path() { rows.push(caption(format!("Log file: {}", path.display())).fill(Role::Dim).lines(4)); }
    }
    if let Some(report) = &v.load_report {
        rows.push(section("Load diagnostics"));
        rows.push(body(format!("{} · {:.0} ms", report["status"].as_str().unwrap_or("unknown"), report["elapsed_ms"].as_f64().unwrap_or(0.))).lines(2).id("load-diagnostic-status"));
        if let Some(reason) = report["failure"].as_str()
            .or_else(|| report["failure"]["reason"].as_str())
            .or_else(|| report["last_error"]["message"].as_str())
        {
            rows.push(body(reason).fill(Role::Warning).lines(4).id("load-diagnostic-failure"));
        }
        rows.push(caption(report["path"].as_str().unwrap_or_default()).fill(Role::Dim).lines(4));
        if let Some(path) = report["log_path"].as_str() { rows.push(caption(format!("Log file: {path}")).fill(Role::Dim).lines(4)); }
        if let Some(error) = report["logging_error"].as_str() { rows.push(body(format!("Could not write the log: {error}")).lines(4)); }
        if let Some(stages) = report["stages_ms"].as_object() {
            for (name, ms) in stages { rows.push(caption(format!("{name}: {:.0} ms", ms.as_f64().unwrap_or(0.))).fill(Role::Dim)); }
        }
        for key in ["artwork", "preload", "ram_fill", "script_restore"] {
            if let Some(status) = report[key]["status"].as_str() {
                rows.push(caption(format!("{key}: {status} · {:.0} ms", report[key]["elapsed_ms"].as_f64().unwrap_or(0.))).fill(Role::Dim));
            }
        }
        let reports = [report.as_ref(), &report["script_restore"], &report["artwork"], &report["preload"], &report["ram_fill"]];
        let retained: usize = reports.iter().map(|report| report["issues"].as_array().map_or(0, Vec::len)).sum();
        let omitted: u64 = reports.iter().map(|report| report["issues_omitted"].as_u64().unwrap_or(0)).sum();
        rows.push(caption(format!("{retained} retained issue examples · {omitted} additional occurrences omitted from these reports. Open Logs for available messages, stages and reasons.")).fill(secondary()).lines(3).id("load-diagnostic-counts"));
        rows.push(logs_el);
    }
    if let Some(i) = current(cx) {
        rows.push(section("Instrument"));
        rows.push(
            body(format!(
                "{} groups · {} zones · {} missing sample references",
                i.groups.len(),
                i.zones.len(),
                i.missing_samples.len()
            ))
            .lines(3),
        );
        rows.push(
            caption(i.path.display().to_string())
                .fill(secondary())
                .lines(4),
        );
        if !v.status.is_empty() {
            rows.push(caption(v.status.clone()).fill(secondary()).lines(3));
        }
        if v.load_report.is_none() && !i.warnings.is_empty() {
            rows.push(section("Import notes").pad(edges(SPACE, 0., 0., 0.)));
            // A line apiece: text broken by a newline measures short.
            for w in i.warnings.iter().flat_map(|w| w.lines()).filter(|l| !l.is_empty()) {
                rows.push(
                    body(w)
                        .text_size(TEXT)
                        .fill(secondary())
                        .lines(6)
                        .shrink(0),
                );
            }
        }
    }
    rows.push(section("Scripts").pad(edges(SPACE, 0., 0., 0.)));
    rows.push(
        body("Scripts drive the library controls and playback. Unsupported features and script errors are recorded in the diagnostics.")
            .fill(secondary())
            .text_size(TEXT)
            .lines(4),
    );
    for line in v.interface_status.lines().chain(v.runtime_status.lines()).filter(|l| !l.is_empty()) {
        rows.push(caption(line.to_owned()).fill(secondary()).lines(3).shrink(0));
    }
    if !v.wallpaper_status.is_empty() {
        rows.push(caption(v.wallpaper_status.clone()).fill(secondary()).lines(3));
    }
    // Text in a scroll is measured without a width, so wraps short and
    // overlaps what follows: the column takes the width it was last laid
    // out at.
    let mut text = col(rows).align(Align::Start).gap(SPACE);
    if let Some(s) = ui.scene().and_then(|s| s.surface("details-scroll")) {
        text = text.w((s.frame.size.width - 2. * INSET).max(0.));
    }
    col![text]
        .pad(INSET)
        .flex(1)
        .min_h(0)
        .scroll()
        .id("details-scroll")
}
