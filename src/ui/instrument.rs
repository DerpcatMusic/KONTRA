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
        | "UVI playback failed. Open Logs for the cause."
        | "The current audio configuration is unsupported by UVI playback.")
}

/// UI presentation only: the machine report and its complete first cause stay intact.
fn native_worker_is_cause(report: &serde_json::Value) -> bool {
    matches!(report["terminal_failure"]["endpoint"]["error"].as_str(),
        None | Some("Bridge(Worker(Failed))" | "Bridge(Worker(Stopped))"))
}

fn native_failure_cause(report: &serde_json::Value) -> Option<&str> {
    if !native_worker_is_cause(report) {
        // Independent callback errors remain primary even if the worker fails later.
        return report["terminal_failure"]["endpoint"]["error"].as_str();
    }
    // The loader has already selected the original canonical cause.
    report["failure"].as_str()
        .or_else(|| report["failure"]["reason"].as_str())
        .or_else(|| report["terminal_failure"]["reason"].as_str())
        .or_else(|| report["terminal_failure"]["worker"]["failure"].as_str())
        .or_else(|| report["terminal_failure"]["endpoint"]["error"].as_str())
}

fn native_failure_notice(report: Option<&serde_json::Value>, status: &str) -> String {
    let cause = report.and_then(native_failure_cause).unwrap_or(status);
    // Keep the exact first-line cause bounded here; Info/Logs retain the full text.
    let cause = if cause == "Bridge(RequestCapacity)" { "Pending request queue filled (RequestCapacity)" } else { cause };
    let first = cause.lines().next().unwrap_or(cause).trim();
    let mut short = first.chars().take(112).collect::<String>();
    if first.chars().count() > 112 { short.push('…'); }
    format!("{short} · See Info and Logs.")
}

/// Format captured endpoint and worker fields once, without reprinting the
/// loader's combined reason/summary paragraph or inferring a performance cause.
fn native_failure_lines(report: &serde_json::Value) -> Vec<(String, bool)> {
    let mut lines = Vec::new();
    let terminal = &report["terminal_failure"];
    let worker = &terminal["worker"];
    let endpoint = &terminal["endpoint"];
    if let Some(cause) = native_failure_cause(report) {
        lines.push((format!("Cause: {cause}"), true));
    }
    if let Some(failure) = worker["failure"].as_str()
        && Some(failure) != native_failure_cause(report)
    {
        lines.push((format!("Additional worker failure: {failure}"), true));
    }
    if native_worker_is_cause(report) && worker["failure"].as_str().is_some()
        && let Some(symptom) = endpoint["error"].as_str()
    {
        lines.push((format!("Endpoint symptom: {symptom}"), false));
    }
    if let Some(code) = endpoint["error_code"].as_u64() {
        lines.push((format!("Endpoint error code: {code}"), false));
    }
    if let Some(stage) = endpoint["stage"].as_str() {
        let frame = endpoint["frame"].as_u64().map_or_else(|| "unavailable".into(), |n| n.to_string());
        lines.push((format!("Endpoint location: {stage} · reported frame {frame}"), false));
    } else if let Some(code) = terminal["code"].as_str() {
        let location = match code {
            "uvi_worker_configuration_failed" => "Configuring the worker",
            "uvi_worker_start_failed" => "Starting the worker",
            "uvi_delay_admission_failed" => "Preparing host audio buffers",
            "uvi_endpoint_failed" => "Connecting the audio endpoint",
            "uvi_worker_failed" => "UVI playback worker",
            "uvi_audio_endpoint_failed" => "Audio endpoint",
            _ => code,
        };
        lines.push((format!("Failure location: {location}"), false));
    }
    let frontier = &endpoint["bridge_frontier"];
    if let (Some(pending), Some(capacity), Some(prefetched)) = (
        frontier["pending_requests"].as_u64(), frontier["pending_capacity"].as_u64(),
        frontier["prefetched_audio"].as_u64()) {
        lines.push((format!("At bridge abort: {pending}/{capacity} pending requests · {prefetched} prefetched audio packets"), false));
    }
    if let (Some(frame), Some(partial), Some(submitted), Some(received)) = (
        frontier["bridge_frame"].as_u64(), frontier["partial_packet_frame"].as_u64(),
        frontier["submitted_frame"].as_u64(), frontier["received_frame"].as_u64()) {
        lines.push((format!("Bridge frame {frame} · current input packet starts at {partial} · next frame to submit {submitted} · next frame to receive {received}"), false));
    }
    if let Some(source) = endpoint["source_file"].as_str() {
        let line = endpoint["line"].as_u64().map_or_else(String::new, |n| format!(":{n}"));
        lines.push((format!("Endpoint source: {source}{line} · inspect context in Logs"), false));
    }
    let lua = &worker["lua_failure"];
    if let Some(chunk) = lua["chunk"].as_str() {
        let line = lua["line"].as_u64().map_or_else(String::new, |n| format!(":{n}"));
        lines.push((format!("Lua source: {chunk}{line} · inspect context in Logs"), false));
    }
    if let Some(status) = worker["status"].as_str() {
        let errors = worker["stats"]["errors"].as_u64().map_or_else(|| "unavailable".into(), |n| n.to_string());
        lines.push((format!("Worker observation: {status} · {errors} recorded worker errors"), false));
    }
    if let Some(packets) = worker["configured_pending_packets"].as_u64() {
        lines.push((format!("Configured pending packets: {packets}"), false));
    }
    for text in [worker["timing"]["summary"].as_str(), worker["timing"]["cpu_summary"].as_str(),
                 worker["timing"]["phase_summary"].as_str(), worker["timing"]["activity_summary"].as_str(),
                 worker["timing"]["ui_summary"].as_str(), worker["timing"]["counter_summary"].as_str(),
                 worker["timing"]["scope"].as_str(), worker["observation"].as_str(),
                 worker["callback_counter_scope"].as_str()].into_iter().flatten() {
        lines.push((text.to_owned(), false));
    }
    lines
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
            out.push(banner(Role::Danger, native_failure_notice(v.load_report.as_deref(), &v.status))
                .w(Len::Pct(100.)).min_w(0));
        }
        return (!out.is_empty()).then(|| col(out).gap(TIGHT).pad((INSET, SPACE)).w(Len::Pct(100.)).min_w(0).shrink(0));
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
        perf_view::shows(cx, slot).hash(&mut h);
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
        #[cfg(feature = "uvi")]
        if let Some((published, current)) = native_panel(cx, slot) {
            if let Some((snapshot, captured, applied_sequence)) = published.snapshots.iter().rev().find(|s| s.root.performance_view)
                .and_then(|snapshot| Some((snapshot, published.snapshot_stamp(snapshot.processor)?, published.snapshot_sequence(snapshot.processor)?))) {
                let mode = perf_view::shows(cx, slot);
                let shared = &cx.p.shared;
                let state = cx.state.uvi.entry(slot).or_default();
                state.set_interactive(!native_problem(&v.status));
                return super::uvi_instrument::view(ui, state, slot,
                    current, captured, (shared.admitted_uvi_edit_sequence(), applied_sequence),
                    snapshot, &published.pictures, &published.fonts, mode,
                    |stamp, edit| shared.edit_uvi(slot, stamp, edit));
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
    let view = cx.view.parts.get(slot)?;
    let published = view.uvi_ui.clone()?;
    if !view.uvi_matches(cx.p, slot, &cx.selection, published.stamp)
        || published.stamp.epoch != cx.p.shared.uvi_activation_epoch() { return None; }
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
        #[cfg(feature = "uvi")]
        return super::uvi_mapping::view(ui, cx);
        #[cfg(not(feature = "uvi"))]
        return native_unavailable("Key and velocity mapping", "UVI support is disabled in this version.");
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
    let grid = mapping_grid(zones, "Selected group key and velocity mapping");
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

/// Shared read-only key × velocity canvas. Each backend owns its range adapter.
pub(super) fn mapping_grid(zones: Vec<(u8, u8, u8, u8, bool)>, name: &'static str) -> El {
    mapping_grid_selected(zones, name, None, None)
}

/// Shared selection/probe paint only. Backend adapters retain zone ownership.
pub(super) fn mapping_grid_selected(zones: Vec<(u8, u8, u8, u8, bool)>, name: &'static str,
    selected: Option<usize>, probe: Option<(u8, u8)>) -> El {
    canvas(move |s| {
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
        for (index, &(lo, hi, lv, hv, available)) in zones.iter().enumerate() {
            let x = f64::from(lo) / 128. * s.width;
            let y = f64::from(127 - hv) / 128. * s.height;
            let w = f64::from(hi.saturating_sub(lo) + 1) / 128. * s.width;
            let h = (f64::from(hv.saturating_sub(lv) + 1) / 128. * s.height).max(1.);
            let fill = if available {
                Role::Ink.alpha(0.35)
            } else {
                Role::Danger.alpha(0.25)
            };
            let area = rect(x, y, (w - 1.).max(1.), (h - 1.).max(1.));
            draw.push(Draw::fill(area.clone(), fill));
            if selected == Some(index) {
                draw.push(Draw::stroke(area, Role::Primary, 2.));
            }
        }
        if let Some((note, velocity)) = probe {
            let x = (f64::from(note) + 0.5) / 128. * s.width;
            let y = (127.5 - f64::from(velocity)) / 128. * s.height;
            draw.push(Draw::fill(rect(x, 0., 1., s.height), Role::Primary.alpha(0.5)));
            draw.push(Draw::fill(rect(0., y, s.width, 1.), Role::Primary.alpha(0.5)));
        }
        draw
    })
    .flex(1)
    .min_h(0)
    .fill(Role::Field)
    .radius(0)
    .clip()
    .named(name)

}


/// A clicked point on a key × velocity canvas. No MIDI or editing authority.
pub(super) fn mapping_point(ui: &Ui, id: &str) -> Option<(u8, u8)> {
    let response = ui.get(id);
    if !response.pressed || response.button != Some(Button::Primary) { return None; }
    let at = ui.local(id)?;
    let size = ui.scene()?.surface(id)?.frame.size;
    mapping_coordinates(at, size)
}

fn mapping_coordinates(at: Point, size: Size) -> Option<(u8, u8)> {
    if size.width <= 0. || size.height <= 0. || !(0. ..size.width).contains(&at.x)
        || !(0. ..size.height).contains(&at.y) { return None; }
    Some(((at.x / size.width * 128.).floor().clamp(0., 127.) as u8,
        127 - (at.y / size.height * 128.).floor().clamp(0., 127.) as u8))
}

#[cfg(test)]
#[test]
fn mapping_coordinates_preserve_all_inclusive_midi_edges() {
    let size = Size::new(128., 128.);
    assert_eq!(mapping_coordinates(Point::new(0., 0.), size), Some((0, 127)));
    assert_eq!(mapping_coordinates(Point::new(127.99, 127.99), size), Some((127, 0)));
    assert_eq!(mapping_coordinates(Point::new(60.5, 27.5), size), Some((60, 100)));
    assert_eq!(mapping_coordinates(Point::new(128., 0.), size), None);
    assert_eq!(mapping_coordinates(Point::new(0., 128.), size), None);
    assert_eq!(mapping_coordinates(Point::new(-1., 0.), size), None);
    assert_eq!(mapping_coordinates(Point::new(0., 0.), Size::new(0., 128.)), None);
}

#[cfg(feature = "uvi")]
fn native_stage_name(phase: &str) -> &str {
    match phase {
        "starting" => "Starting worker",
        "bank_open" => "Opening bank",
        "program_decode" => "Parsing program",
        "graph_diagnosis_and_preflight" => "Inspecting program graph",
        "preflight" | "player_preflight" => "Checking playback support",
        "resources" => "Decoding initial sample resources",
        "modules" => "Reading script modules",
        "lua_init" => "Initializing scripts and controls",
        "renderer_init" | "restore_renderer_init" => "Preparing sound renderer",
        "restore_validation_and_audio" => "Validating restored audio",
        "restore_apply" => "Restoring settings",
        "player_finalize" => "Finishing player initialization",
        "serve" => "Rendering playback",
        "packet_render" => "Rendering packet",
        _ => phase,
    }
}

/// Formats only the worker's bounded scalar snapshot; never asks Lua or copies
/// the graph. Initial resource totals deliberately exclude authored later loads.
#[cfg(feature = "uvi")]
fn native_activity_lines(activity: &crate::uvi::worker::WorkerLoadActivity, include_failure: bool,
    include_pcm: bool, include_render_timing: bool) -> Vec<(String, bool)> {
    use crate::uvi::worker::Status;
    let mut lines = Vec::new();
    let completed = activity.stages.iter().filter(|stage| stage.outcome == "finished").count();
    let state = match activity.status {
        Status::Starting => "Loading · partial, playback not ready",
        Status::Ready => "Worker ready at last loader observation",
        Status::Failed => "Worker failed",
        Status::Stopped => "Worker stopped",
    };
    lines.push((format!("{state} · {:.1} s initialization · {completed} completed stages", activity.elapsed.as_secs_f64()),
        activity.status == Status::Failed));
    let phase_label = if activity.status == Status::Starting { "Current loading phase" } else { "Observed phase" };
    lines.push((format!("{phase_label}: {}", native_stage_name(activity.phase)), false));
    if let Some(nodes) = activity.nodes {
        lines.push((format!("Parsed: {nodes} graph nodes · {} sample zones · {} script processors",
            activity.sample_zones.map_or_else(|| "unknown".to_owned(), |n| n.to_string()),
            activity.script_processors.map_or_else(|| "unknown".to_owned(), |n| n.to_string())), false));
    } else {
        lines.push(("Program graph inventory: pending or unavailable".into(), false));
    }
    if let Some(rejected) = activity.static_rejected_nodes {
        lines.push((format!("Static playback check: {rejected} known rejected graph nodes"), rejected != 0));
    } else {
        lines.push(("Static playback check: pending or unavailable".into(), false));
    }
    if activity.status == Status::Starting {
        lines.push(("Counts show partial loading progress. Controls become available when audio setup is complete.".into(), false));
    }
    let resources = &activity.resources;
    if let Some(total) = resources.total {
        let cache = match resources.cache_hit { Some(true) => " · cache hit", Some(false) => " · cache miss", None => "" };
        lines.push((format!("Initial resources: {} / {total} paths loaded · {} unique decodes · {:.2} MiB decoded PCM{cache}",
            resources.loaded, resources.unique_decodes, resources.bytes as f64 / (1024. * 1024.)), false));
        if let Some(path) = &resources.current {
            let label = if activity.phase == "resources" { "Current resource" } else { "Last initial resource" };
            lines.push((format!("{label}: {path}"), false));
        }
        lines.push(("Resource totals cover the initial program references; additional resources loaded by scripts are not counted here.".into(), false));
    } else {
        lines.push(("Initial resource inventory: pending or unavailable".into(), false));
    }
    for stage in &activity.stages {
        let outcome = match stage.outcome {
            "finished" => "Completed",
            "in_progress" => "Current",
            "failed" => "Failed",
            "cancelled" => "Cancelled",
            other => other,
        };
        lines.push((format!("{outcome} · {} · {:.0} ms", native_stage_name(stage.phase),
            stage.elapsed.as_secs_f64() * 1000.), stage.outcome == "failed"));
    }
    if include_failure && let Some(reason) = &activity.failure {
        lines.push((format!("Worker error during {} at frame {}: {reason}",
            native_stage_name(activity.phase), activity.frame), true));
    }
    let stats = activity.stats;
    if include_pcm && let Some(bytes) = activity.owned_pcm_bytes {
        lines.push((format!("Observed worker-owned PCM: {:.2} MiB · {} aliases · resource revision {} (includes script-loaded resources; excludes Lua/DSP/UI)",
            bytes as f64 / 1_048_576., stats.resource_alias_count, stats.resource_revision), false));
    }
    if include_render_timing {
        let wall = if stats.render_attempts > 0 {
            format!("wall {:.3} ms mean / {:.3} ms max",
                stats.render_ns as f64 / stats.render_attempts as f64 / 1e6, stats.max_render_ns as f64 / 1e6)
        } else { "render wall timing unavailable".to_owned() };
        let cpu = if stats.render_cpu_samples > 0 {
            format!("CPU {:.3} ms mean / {:.3} ms max ({} measured attempts)",
                stats.render_cpu_ns as f64 / stats.render_cpu_samples as f64 / 1e6,
                stats.max_render_cpu_ns as f64 / 1e6, stats.render_cpu_samples)
        } else { "render CPU timing unavailable".to_owned() };
        lines.push((format!("Observed render attempts: {} · {wall} · {cpu}", stats.render_attempts), false));
    }
    lines.push((format!("Playback: {} rendered packets · {} active voice instances · {} worker errors",
        stats.rendered_blocks, stats.active_voices, stats.errors), stats.errors != 0));
    lines.push((format!("Empty output polls: {} · queue backpressure: {} · stale packets: {} · omitted script prints: {}",
        stats.empty_output_polls, stats.backpressure, stats.stale_packets, stats.dropped_logs),
        stats.dropped_logs != 0));
    lines.push(("Loader observations use a 250 ms snapshot cache; these are not endpoint readiness or exact fault-capture measurements. Render timings exclude queue waiting and UI snapshots. Voice instances include held or releasing silent voices; normal empty output polls are counted separately from audio shortages.".into(), false));
    lines
}

#[cfg(feature = "uvi")]
fn native_activity_summary_visibility(report: Option<&serde_json::Value>) -> (bool, bool) {
    let timing = report.map(|report| &report["terminal_failure"]["worker"]["timing"]);
    let has_pcm = timing.and_then(|timing| timing["activity_summary"].as_str())
        .is_some_and(|summary| !summary.is_empty());
    // A zero-attempt capture still owns timing rows but has no PCM summary.
    // Initialization PCM must remain visible independently of render timing.
    (!has_pcm, !timing.is_some_and(|timing| !timing.is_null()))
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
            caption("Playback and controls follow this UVI program. Mapping inspects its initial sample zones without editing; Sound editing is unavailable. Use the instrument’s Rack controls to change its sound.")
                .fill(secondary()).lines(4)];
        let reported_failure = v.load_report.as_deref().and_then(native_failure_cause);
        let reported_worker_failure = v.load_report.as_ref()
            .and_then(|report| report["terminal_failure"]["worker"]["failure"].as_str());
        if let Some(report) = &v.load_report {
            let details = native_failure_lines(report);
            if !details.is_empty() { rows.push(section("Failure details")); }
            for (index, (text, warning)) in details.into_iter().enumerate() {
                rows.push(body(text).text_size(TEXT).fill(if warning { Role::Warning.into() } else { secondary() })
                    .lines(8).shrink(0).id(format!("uvi-failure-detail-{index}")));
            }
        }
        #[cfg(feature = "uvi")]
        if let Some(activity) = &v.uvi_activity
            && v.uvi_matches(cx.p, cx.state.selected, &cx.selection, activity.stamp) {
            rows.push(section("Loading details"));
            let (include_pcm, include_render_timing) = native_activity_summary_visibility(v.load_report.as_deref());
            for (index, (line, warning)) in native_activity_lines(activity, activity.failure.as_deref() != reported_failure
                && activity.failure.as_deref() != reported_worker_failure, include_pcm, include_render_timing).into_iter().enumerate() {
                rows.push(body(line.clone()).text_size(TEXT).fill(if warning { Role::Warning.into() } else { secondary() })
                    .lines(6).shrink(0).named(line).id(format!("uvi-load-detail-{index}")));
            }
        }
        rows.push(logs_el);
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

#[cfg(test)]
mod native_status_tests {
    use super::*;
    #[cfg(feature = "uvi")]
    #[test]
    fn bounded_info_metrics_preserve_unknown_counts_and_valid_cpu_sample_scope() {
        use crate::uvi::worker::{Stamp, Stats, Status, WorkerLoadActivity};
        let mut activity = WorkerLoadActivity { stamp: Stamp { epoch: 7, generation: 11, frame: 256 },
            mapping: None, status: Status::Ready, phase: "serve", frame: 256,
            elapsed: std::time::Duration::ZERO, stages: vec![], nodes: Some(3),
            static_rejected_nodes: Some(0), sample_zones: None, script_processors: None,
            resources: Default::default(), owned_pcm_bytes: Some(8 * 1_048_576), failure: None,
            stats: Stats { resource_alias_count: 2, resource_revision: 3,
                render_attempts: 4, render_ns: 12_000_000, max_render_ns: 4_000_000,
                render_cpu_samples: 2, render_cpu_ns: 2_000_000, max_render_cpu_ns: 1_500_000,
                ..Default::default() } };
        let lines = native_activity_lines(&activity, true, true, true);
        assert!(lines.iter().any(|(line, _)| line == "Parsed: 3 graph nodes · unknown sample zones · unknown script processors"));
        assert!(lines.iter().any(|(line, _)| line.contains("8.00 MiB · 2 aliases · resource revision 3")));
        assert!(lines.iter().any(|(line, _)| line.contains("wall 3.000 ms mean / 4.000 ms max · CPU 1.000 ms mean / 1.500 ms max (2 measured attempts)")));
        let failure_details = native_activity_lines(&activity, false, false, false);
        assert!(!failure_details.iter().any(|(line, _)| line.starts_with("Observed render attempts:") || line.starts_with("Observed worker-owned PCM:")),
            "canonical terminal details already own the captured timing/census");
        // Genuine pre-first-packet census: no wall/CPU/render attempts occurred.
        activity.stats = Stats { resource_alias_count: 2, resource_resident_pcm_bytes: 8 * 1_048_576,
            resource_revision: 3, ..Default::default() };
        let missing = native_activity_lines(&activity, true, true, true);
        assert!(missing.iter().any(|(line, _)| line.contains("render wall timing unavailable · render CPU timing unavailable")));
        assert!(!missing.iter().any(|(line, _)| line.contains("CPU 0.000")));
        let report = serde_json::json!({"terminal_failure":{"worker":{"timing":{
            "summary":"Recorded render wall time: unavailable", "activity_summary":null}}}});
        let (pcm, timing) = native_activity_summary_visibility(Some(&report));
        assert_eq!((pcm, timing), (true, false));
        let zero_attempt_failure = native_activity_lines(&activity, false, pcm, timing);
        assert!(zero_attempt_failure.iter().any(|(line, _)| line.contains("8.00 MiB · 2 aliases · resource revision 3")));
        assert!(!zero_attempt_failure.iter().any(|(line, _)| line.starts_with("Observed render attempts:")));
        let mut canonical = report;
        canonical["terminal_failure"]["worker"]["timing"]["activity_summary"] = "Captured render observation: 8.00 MiB PCM".into();
        assert_eq!(native_activity_summary_visibility(Some(&canonical)), (false, false));
    }

    #[test]
    fn native_loader_failure_is_terminal_in_the_performance_view() {
        let status="UVI playback failed. Open Logs for the cause.";
        assert!(native_problem(status));
        assert!(!native_problem("Preparing UVI playback…"));
        if cfg!(feature="uvi") {
            assert_eq!(native_wait(false,status),"Instrument could not be loaded. See the error above or Logs for details.");
            assert!(!native_wait(false,status).contains("Waiting"));
        } else {
            assert_eq!(native_wait(false,status),"UVI support is disabled in this version.");
        }
    }
    #[test]
    fn capacity_banner_uses_exact_endpoint_cause_and_info_shows_timing_once() {
        let report = serde_json::json!({"failure":"Bridge(RequestCapacity). combined long timing text",
            "terminal_failure":{"endpoint":{"error":"Bridge(RequestCapacity)","error_code":36,"stage":"process","frame":8192,"source_file":"src/plugin/uvi.rs","line":42},
                "worker":{"status":"ready","failure":null,"configured_pending_packets":27,"stats":{"errors":0},
                    "timing":{"summary":"Recorded render wall time: mean 6.40 ms · budget 5.33 ms", "counter_summary":"72 over-budget attempts", "scope":"CPU time is not measured."}}}});
        let before = report.clone();
        assert_eq!(native_failure_notice(Some(&report), "failed"), "Pending request queue filled (RequestCapacity) · See Info and Logs.");
        let lines = native_failure_lines(&report);
        assert_eq!(lines.iter().filter(|(s,_)| s.contains("mean 6.40 ms")).count(),1);
        assert!(!lines.iter().any(|(s,_)| s.contains("combined long timing text")));
        assert!(lines.iter().any(|(s,_)| s=="Endpoint error code: 36"));
        assert!(lines.iter().any(|(s,_)| s.contains("process · reported frame 8192")));
        assert!(lines.iter().any(|(s,_)| s.contains("src/plugin/uvi.rs:42")));
        assert!(lines.iter().any(|(s,_)| s=="CPU time is not measured."));
        assert_eq!(report,before);
    }

    #[test]
    fn bridge_abort_frontier_stays_separate_from_later_worker_observation() {
        let report = serde_json::json!({"failure":"Bridge(RequestCapacity)",
            "terminal_failure":{"endpoint":{"error":"Bridge(RequestCapacity)", "stage":"process", "frame":78464,
                "bridge_frontier":{"bridge_frame":78592,"partial_packet_frame":78336,
                    "submitted_frame":71424,"received_frame":69120,"pending_requests":27,
                    "pending_capacity":27,"prefetched_audio":0}},
                "worker":{"status":"ready","stats":{"errors":0,"rendered_blocks":278}}}});
        let lines = native_failure_lines(&report);
        assert!(lines.iter().any(|(s,_)|s=="At bridge abort: 27/27 pending requests · 0 prefetched audio packets"));
        assert!(lines.iter().any(|(s,_)|s.contains("Bridge frame 78592 · current input packet starts at 78336")));
        assert!(lines.iter().any(|(s,_)|s.contains("process · reported frame 78464")));
        assert!(lines.iter().any(|(s,_)|s=="Worker observation: ready · 0 recorded worker errors"));
        assert!(!native_failure_lines(&serde_json::json!({})).iter().any(|(s,_)|s.starts_with("At bridge abort:")));
    }

    #[test]
    fn canonical_worker_cause_outranks_worker_failed_symptom_and_preserves_lua_location() {
        let report = serde_json::json!({"failure":"Authored callback failed",
            "terminal_failure":{"worker":{"failure":"Authored callback failed","lua_failure":{"chunk":"authored fixture","line":7}},
                "endpoint":{"error":"Bridge(Worker(Failed))","stage":"feed"}}});
        assert_eq!(native_failure_cause(&report),Some("Authored callback failed"));
        let lines = native_failure_lines(&report);
        assert_eq!(lines.iter().filter(|(s,_)|s.contains("Authored callback failed")).count(),1);
        assert!(lines.iter().any(|(s,_)|s=="Endpoint symptom: Bridge(Worker(Failed))"));
        assert!(lines.iter().any(|(s,_)|s.contains("authored fixture:7")));
        assert!(lines.iter().any(|(s,_)|s.contains("frame unavailable")));
        assert!(!lines.iter().any(|(s,_)|s.contains("0 recorded worker errors")));
    }

    #[test]
    fn non_endpoint_failure_keeps_complete_info_cause_and_bounds_only_banner() {
        let reason = "authored parse failure ".repeat(12);
        let report = serde_json::json!({"failure":{"reason":reason}});
        let notice = native_failure_notice(Some(&report),"failed");
        assert!(notice.chars().count()<=138);
        assert!(notice.contains('…'));
        assert_eq!(native_failure_lines(&report), vec![(format!("Cause: {reason}"),true)]);
        assert_eq!(native_failure_notice(None,"UVI load failed"),"UVI load failed · See Info and Logs.");
    }

    #[test]
    fn independent_endpoint_error_preserves_primary_cause_and_later_worker_evidence() {
        for error in ["InvalidInput", "Bridge(RequestCapacity)"] {
            let report = serde_json::json!({"failure":format!("{error} at process (frame 1024)"),
                "terminal_failure":{"endpoint":{"error":error,"stage":"process","frame":1024},
                    "worker":{"failure":"Later worker failure","status":"failed"}}});
            let before = report.clone();
            assert_eq!(native_failure_cause(&report),Some(error));
            assert!(!native_failure_notice(Some(&report),"failed").contains("Later worker failure"));
            let lines = native_failure_lines(&report);
            assert!(lines.iter().any(|(s,_)|s==&format!("Cause: {error}")));
            assert_eq!(lines.iter().filter(|(s,_)|s.contains("Later worker failure")).count(),1);
            assert!(lines.iter().any(|(s,_)|s=="Additional worker failure: Later worker failure"));
            assert!(!lines.iter().any(|(s,_)|s.starts_with("Endpoint symptom:")));
            assert_eq!(report,before);
        }
    }

    #[test]
    fn endpoint_absent_keeps_canonical_report_cause() {
        let report = serde_json::json!({"failure":"Original static failure",
            "terminal_failure":{"worker":{"failure":"Additional captured failure"}}});
        assert_eq!(native_failure_cause(&report),Some("Original static failure"));
        assert!(native_failure_lines(&report).iter().any(|(s,_)|s.contains("Additional captured failure")));
    }

}
