//! The top bar: browser toggle, wordmark, what is loading, engine readouts,
//! master volume with its meter and the global actions; plus the library
//! folder setting and the load indicator under it.

use super::{Cx, View, menu, theme::*};
use crate::plugin::{P, SamplerParams};
use moose::mui::{Bridge, mui::prelude::*};
use std::sync::atomic::Ordering;
use std::time::Instant;

#[derive(Default)]
struct NativeReadouts {
    parts: usize,
    pcm_bytes: usize,
    pending_pcm: usize,
    voices: u64,
    pending_voices: usize,
    cpu_ns: u64,
    cpu_samples: u64,
    cpu_max_ns: u64,
}

fn native_readouts(cx: &Cx) -> NativeReadouts {
    let mut metrics = NativeReadouts::default();
    #[cfg(feature = "uvi")]
    for (slot, part) in cx.selection.parts.iter().enumerate() {
        if part.uvi.is_none() { continue; }
        metrics.parts += 1;
        let observation = cx.view.parts.get(slot)
            .and_then(|v| v.uvi_metrics(cx.p, slot, &cx.selection));
        let Some((activity, adopted)) = observation else {
            metrics.pending_pcm += 1;
            metrics.pending_voices += 1;
            continue;
        };
        if let Some(bytes) = activity.owned_pcm_bytes {
            metrics.pcm_bytes = metrics.pcm_bytes.saturating_add(bytes);
        } else { metrics.pending_pcm += 1; }
        if adopted {
            metrics.voices = metrics.voices.saturating_add(activity.stats.active_voices);
            if activity.stats.render_cpu_samples > 0 {
                metrics.cpu_ns = metrics.cpu_ns.saturating_add(activity.stats.render_cpu_ns);
                metrics.cpu_samples = metrics.cpu_samples.saturating_add(activity.stats.render_cpu_samples);
                metrics.cpu_max_ns = metrics.cpu_max_ns.max(activity.stats.max_render_cpu_ns);
            }
        } else { metrics.pending_voices += 1; }
    }
    metrics
}

pub fn top_bar(ui: &mut Ui, cx: &mut Cx, bridge: &mut Bridge<SamplerParams>) -> El {
    let p = cx.p;
    let cpu = f32::from_bits(cx.state.meters.cpu.load(Ordering::Relaxed));
    let disk = f32::from_bits(cx.state.meters.disk.load(Ordering::Relaxed));
    let voices = p.shared.voices.load(Ordering::Relaxed);
    let audible = p.shared.audible.load(Ordering::Relaxed);
    #[cfg(feature = "uvi")]
    let native_voices = p.shared.uvi_voices.load(Ordering::Relaxed);
    #[cfg(not(feature = "uvi"))]
    let native_voices = 0;
    let native = native_readouts(cx);
    // Kontakt's audible census excludes UVI. Replace the callback's native
    // census with the scoped worker observation instead of adding it twice.
    let voice_count = audible.saturating_add(native.voices);
    let voice_text = partial_readout(voice_count.to_string(), voice_count == 0, native.pending_voices);
    let voice_tip = format!("Kontakt: {audible} audible, {} running. UVI in this rack: {} worker voice instances; includes silent and releasing layers, sampled on loader polls with a 250 ms snapshot cache. {} UVI part observations pending or unavailable.",
        voices.saturating_sub(native_voices), native.voices, native.pending_voices);
    // Kontakt samples are process-wide; UVI workers own their PCM per rack.
    let kontakt_memory = crate::engine::resident_bytes();
    let memory = kontakt_memory.saturating_add(native.pcm_bytes);
    let memory_text = partial_readout(megabytes(memory), memory == 0, native.pending_pcm);
    let freed: u64 = cx.view.parts.iter().map(|v| v.freed).sum();
    let memory_tip = format!("Sample PCM: {} Kontakt process-wide (shared samples counted once) + {} UVI owned by current workers in this rack (shared aliases counted once per worker). {} UVI part observations pending or unavailable. Excludes Lua, DSP and UI allocations; {} Kontakt sample memory freed.",
        megabytes(kontakt_memory), megabytes(native.pcm_bytes), native.pending_pcm, megabytes(freed as usize));
    let mut cpu_tip = "Host audio callback wall-time load, including waiting; not worker CPU usage.".to_owned();
    if native.cpu_samples > 0 {
        cpu_tip.push_str(&format!(" UVI worker render CPU: {:.3} ms mean per measured attempt, {:.3} ms max across current rack workers ({} samples since activation); excludes waiting and UI snapshots.",
            native.cpu_ns as f64 / native.cpu_samples as f64 / 1e6,
            native.cpu_max_ns as f64 / 1e6, native.cpu_samples));
    } else if native.parts > 0 { cpu_tip.push_str(" UVI worker render CPU measurement unavailable."); }
    let disk_tip = if native.parts > 0 {
        "Application read rate: Kontakt sample streaming plus UVI bank/cache reads, including OS-cached reads; not physical disk utilization. UVI samples are preloaded into owned PCM before playback; this is not a UVI DFD streaming meter."
    } else { "Kontakt sample streaming application read rate, including OS-cached reads; not physical disk utilization." };

    let loading: Vec<_> = cx
        .view
        .parts
        .iter()
        .zip(&cx.selection.parts)
        .filter(|(v, part)| v.loading && !part.is_empty())
        .map(|(_, part)| part.uvi.as_ref().map_or_else(|| stem(&part.path), |source| stem(&source.member)))
        .collect();
    let activity = match loading.as_slice() {
        [] if cx.view.multi_status.starts_with("Loading") => cx.view.multi_status.clone(),
        // Streamed audio played late or script calls lost; zero when all is well.
        [] => match p.shared.dropouts.load(Ordering::Relaxed) {
            0 => String::new(),
            n => format!("{n} audio dropouts"),
        },
        [one] => format!("Loading {one}…"),
        many => format!("Loading {} instruments…", many.len()),
    };

    let roomy = ui.scene().and_then(|s| s.surface("activity")).is_none_or(|s| s.frame.size.width >= TEXT * 8.);
    let (browser, browser_el) = icon_button(
        ui,
        "toggle-browser",
        Icon::Sidebar,
        if cx.state.browser { "Hide the browser" } else { "Show the browser" },
        cx.state.browser,
    );
    if browser {
        cx.state.browser = !cx.state.browser;
    }

    let master = bridge.bind(ui, P::Volume, |ui, id, v| {
        let mut db = *v * 66. - 60.;
        let (_, el) = fader(
            ui,
            id.as_str(),
            "Master volume",
            &mut db,
            -60.0..=6.0,
            Fader {
                reset: -12.,
                ..Fader::LEVEL
            }
            .length(TEXT * 8.),
            db_text,
        );
        *v = (db + 60.) / 66.;
        el
    });
    let level = bridge.meter(P::Level);

    let (thru, thru_el) = action(ui, "midi-thru", "MIDI Thru", cx.selection.midi_thru);
    if thru {
        cx.selection.midi_thru = !cx.selection.midi_thru;
    }
    let (qwerty, qwerty_el) = icon_button(
        ui,
        "qwerty",
        Icon::Keys,
        "Play from the computer keyboard: A to ' play, Z X step the octave, C V the velocity",
        cx.selection.qwerty,
    );
    if qwerty {
        cx.selection.qwerty = !cx.selection.qwerty;
        if !cx.selection.qwerty {
            cx.state.computer.release(p);
        }
    }
    cx.state.computer.on.store(cx.selection.qwerty, Ordering::Relaxed);
    let (panic, panic_el) = action(ui, "panic", "Panic", false);
    if panic {
        p.shared.panic.store(true, Ordering::Release);
    }
    let open = cx
        .state
        .menu
        .as_ref()
        .is_some_and(|m| m.target == menu::Target::App);
    let (more, more_el) = icon_button(ui, "app-menu", Icon::Menu, "Menu", open);
    if more {
        if open {
            cx.state.menu = None;
        } else {
            menu::open_under(ui, cx, menu::Target::App, "app-menu");
            // Right-aligned under the button.
            if let Some(m) = cx.state.menu.as_mut() {
                m.at.x -= menu::WIDTH - CONTROL;
            }
        }
    }

    strip(vec![
        browser_el,
        body("KONTRA")
            .text_size(TEXT + 1.)
            .text_weight(Weight::BOLD)
            .fill(Role::Ink)
            .shrink(0),
        // Squeezed to a letter or two, it says nothing: the loading bar and
        // the parts' own headers carry it, and the words stay in a tip.
        caption(if roomy { activity.clone() } else { String::new() })
            .fill(secondary())
            .lines(1)
            .flex(1)
            .min_w(0)
            .when(!activity.is_empty(), |e| e.tip(activity))
            .id("activity"),
        stat("CPU", format!("{:.0}%", cpu * 100.), "100%").tip(cpu_tip),
        // Kontakt audible voices plus current UVI worker voice instances.
        stat("Voices", voice_text.clone(), "000").tip(voice_tip)
            .named(format!("Voices {voice_text}")).id("readout-voices"),
        stat("RAM", memory_text.clone(), "00000 MB").tip(memory_tip)
            .named(format!("Sample PCM {memory_text}")).id("readout-ram"),
        stat("Disk", format!("{disk:.1} MiB/s"), "000.0 MiB/s").tip(disk_tip),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![section("Master"), master, meter_bar(level)]).gap(SPACE),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![qwerty_el, thru_el, panic_el, more_el]),
    ])
    .gap(SPACE)
    .pad((SPACE, SPACE))
    .fill(Role::Surface)
}

fn partial_readout(known: String, zero: bool, pending: usize) -> String {
    match (pending, zero) {
        (0, _) => known,
        (_, true) => "—".to_owned(),
        (_, false) => format!("{known}+"),
    }
}

#[cfg(test)]
mod metric_tests {
    #[test]
    fn unavailable_native_observation_is_not_zero_ram_or_zero_voices() {
        assert_eq!(super::partial_readout("0 MB".into(), true, 1), "—");
        assert_eq!(super::partial_readout("304 MB".into(), false, 1), "304 MB+");
        assert_eq!(super::partial_readout("0".into(), true, 0), "0");
        assert_eq!(super::partial_readout("4".into(), false, 1), "4+");
    }
}

/// The master output level: a thin bar in dB, neutral until it nears clipping.
fn meter_bar(level: f32) -> El {
    let db = 20. * f64::from(level.max(1e-6)).log10();
    let unit = ((db + 60.) / 66.).clamp(0., 1.);
    let hot = db > -1.;
    canvas(move |s| {
        let t = TIGHT;
        let mid = (s.height / 2. - t / 2.).round();
        let mut draw = vec![Draw::fill(rect(0., mid, s.width, t), Role::Ink.alpha(0.1))];
        if unit > 0. {
            let fill = if hot { Role::Danger.alpha(1.) } else { Role::Ink.alpha(0.7) };
            draw.push(Draw::fill(rect(0., mid, (unit * s.width).round(), t), fill));
        }
        // Unity.
        let x = (60. / 66. * s.width).round();
        draw.push(Draw::fill(rect(x, mid - t / 2., 1., 2. * t), Role::Ink.alpha(0.3)));
        draw
    })
    .w(CONTROL * 2.)
    .h(CONTROL)
    .shrink(0)
    .named("Output level")
}

/// The library folders, managed under the top bar: each with what was
/// found in it and a remove button; adding one, picked or typed; scanning
/// them again.
pub fn settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let libraries = &cx.p.shared.libraries;
    let scanned = cx.view.scanned == libraries.wanted();
    let mut rows = Vec::new();
    for (n, root) in cx.settings.roots.iter().enumerate() {
        let (remove, remove_el) = icon_button(ui, format!("root-remove-{n}"), Icon::Close, "Remove this folder", false);
        if remove {
            libraries.remove_root(n);
        }
        let found = match cx.view.shelf.per_root.get(n) {
            _ if !scanned => "Scanning".to_owned(),
            Some(1) => "1 library".to_owned(),
            Some(k) => format!("{k} libraries"),
            None => "Scanning".to_owned(),
        };
        let kind = if root.single { "Library" } else { "Folder of libraries" };
        rows.push(
            row![
                body(root.path.clone()).text_size(TEXT).lines(1).flex(1).min_w(0),
                caption(format!("{kind} · {found}")).fill(secondary()).lines(1).shrink(0),
                remove_el
            ]
            .gap(SPACE)
            .align(Align::Center)
            .pad(edges(0., SPACE, 0., INSET))
            .shrink(0),
        );
    }
    if cx.settings.roots.is_empty() {
        rows.push(
            col![caption("No library folders yet. Add the folder that holds your Kontakt libraries, or one library's own folder.")
                .fill(secondary())
                .lines(2)]
            .align(Align::Start)
            .pad(edges(0., INSET, 0., INSET))
            .shrink(0),
        );
    }
    // Typed, for a desktop with no file dialog.
    let field = text_input(ui, "root", &mut cx.state.root);
    let (add, add_el) = action(ui, "root-add", "Add", false);
    let typed = cx.state.root.trim().to_owned();
    if add && !typed.is_empty() {
        libraries.add_root(std::path::Path::new(&typed), false);
        cx.state.root.clear();
    }
    let (many, many_el) = action(ui, "root-pick-many", "Add folder of libraries…", false);
    let (one, one_el) = action(ui, "root-pick-one", "Add library folder…", false);
    if many || one {
        add_folder(cx, one);
    }
    let (import, import_el) = action(ui, "root-import", "Import from Kontakt", false);
    if import {
        cx.p.shared.libraries.import_kontakt();
    }
    let (scan, scan_el) = action(ui, "scan", "Rescan", false);
    if scan {
        cx.p.shared.libraries.rescan();
    }
    let (close, close_el) = icon_button(ui, "settings-close", Icon::Close, "Close", false);
    if close {
        cx.state.settings = false;
    }
    let mut body = vec![
        row![section("Library folders").flex(1), many_el, one_el, import_el, scan_el, close_el]
            .gap(SPACE)
            .align(Align::Center)
            .pad(edges(SPACE, SPACE, 0., INSET))
            .shrink(0),
    ];
    body.extend(rows);
    body.push(
        row![
            field.el.flex(1).min_w(0).h(CONTROL).named("A folder to add, typed"),
            add_el
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad(edges(0., SPACE, 0., INSET))
        .shrink(0),
    );
    body.push(interface_settings(ui, cx));
    body.push(view_settings(ui, cx));
    body.push(new_part_settings(ui, cx));
    col![col(body).gap(TIGHT).align(Align::Stretch), rule()]
        .gap(0)
        .shrink(0)
        .fill(Role::Surface)
}

/// Which performance view parts show unless they choose, and the scale of
/// a library's own.
fn interface_settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let scale = cx.settings.editor_scale();
    let mut choices = Vec::new();
    for (to, label) in [(1.0, "100%"), (1.25, "125%"), (1.5, "150%"), (2.0, "200%")] {
        let (hit, el) = action(ui, format!("ui-scale-{label}"), label, scale == to);
        if hit { cx.p.shared.libraries.edit(|s| s.ui_scale = to); }
        choices.push(el);
    }
    let (reset, reset_el) = action(ui, "ui-scale-reset", "Reset", false);
    if reset { cx.p.shared.libraries.edit(|s| s.ui_scale = 1.0); }
    row![caption("Interface scale").fill(secondary()).lines(1).shrink(0),
        segmented(choices), reset_el,
        caption("Window size is remembered").fill(secondary()).lines(1).flex(1).min_w(0)]
        .gap(SPACE).align(Align::Center).pad(edges(TIGHT, SPACE, SPACE, INSET)).shrink(0)
}

fn view_settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let libraries = &cx.p.shared.libraries;
    let (mode, scale) = (cx.settings.view_mode, cx.settings.view_scale);
    let mut views = Vec::new();
    for to in crate::library::ViewMode::ALL {
        let (hit, el) = action(ui, format!("view-default-{to:?}"), to.label(), mode == to);
        if hit {
            libraries.edit(|s| s.view_mode = to);
        }
        views.push(el);
    }
    let mut scales = Vec::new();
    for (to, label) in [(0., "Fit"), (1., "1×"), (1.5, "1.5×"), (2., "2×")] {
        let (hit, el) = action(ui, format!("view-scale-{label}"), label, scale == to);
        if hit {
            libraries.edit(|s| s.view_scale = to);
        }
        scales.push(el);
    }
    row![
        caption("Performance view").fill(secondary()).lines(1).shrink(0),
        segmented(views),
        caption("Scale").fill(secondary()).lines(1).shrink(0),
        segmented(scales),
    ]
    .gap(SPACE)
    .align(Align::Center)
    .pad(edges(TIGHT, SPACE, SPACE, INSET))
    .shrink(0)
}

/// The MIDI input and output bus new parts take.
fn new_part_settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let libraries = &cx.p.shared.libraries;
    let (input, output) = (cx.settings.new_input, cx.settings.new_output);
    let fixed = input.filter(|&(_, c)| c >= 0);
    let mut inputs = Vec::new();
    for (id, label, to, on) in [
        ("next", "Next free channel", None, input.is_none()),
        ("omni", "Omni", Some((0, -1)), input.is_some_and(|(_, c)| c < 0)),
        ("fixed", "Channel", Some(fixed.unwrap_or((0, 0))), fixed.is_some()),
    ] {
        let (hit, el) = action(ui, format!("new-input-{id}"), label, on);
        if hit {
            libraries.edit(|s| s.new_input = to);
        }
        inputs.push(el);
    }
    // A1…D16, stepped.
    let step = |ui: &mut Ui, id: &str, now: usize, count: usize| {
        let (down, down_el) = icon_button(ui, format!("{id}-down"), Icon::Left, "Previous", false);
        let (up, up_el) = icon_button(ui, format!("{id}-up"), Icon::Right, "Next", false);
        let to = if down { Some((now + count - 1) % count) } else if up { Some((now + 1) % count) } else { None };
        (to, down_el, up_el)
    };
    let mut items = vec![caption("New parts: MIDI").fill(secondary()).lines(1).shrink(0), segmented(inputs)];
    if let Some((port, channel)) = fixed {
        let now = usize::from(port) * 16 + channel as usize;
        let (to, down, up) = step(ui, "new-channel", now, 64);
        if let Some(n) = to {
            libraries.edit(|s| s.new_input = Some(((n / 16) as u8, (n % 16) as i16)));
        }
        let name = format!("{}{}", char::from(b'A' + port), channel + 1);
        items.push(cluster(vec![down, caption(name).reserve("D16").justify(Justify::Center), up]));
    }
    let mut outputs = Vec::new();
    for (id, label, to, on) in [("auto", "Automatic", None, output.is_none()), ("fixed", "Bus", Some(output.unwrap_or(0)), output.is_some())] {
        let (hit, el) = action(ui, format!("new-output-{id}"), label, on);
        if hit {
            libraries.edit(|s| s.new_output = to);
        }
        outputs.push(el);
    }
    items.extend([caption("Output").fill(secondary()).lines(1).shrink(0), segmented(outputs)]);
    if let Some(bus) = output {
        let (to, down, up) = step(ui, "new-bus", usize::from(bus), crate::engine::BUSES);
        if let Some(n) = to {
            libraries.edit(|s| s.new_output = Some(n as u8));
        }
        items.push(cluster(vec![down, caption(format!("st.{}", bus + 1)).reserve("st.16").justify(Justify::Center), up]));
    }
    row(items).gap(SPACE).align(Align::Center).pad(edges(TIGHT, SPACE, SPACE, INSET)).shrink(0)
}

/// Ask for a library folder (`single`) or a folder of libraries to add;
/// with no file dialog to ask, the library folders strip takes it typed.
pub fn add_folder(cx: &mut Cx, single: bool) {
    let from = (cx.settings.roots.first())
        .map(|r| std::path::PathBuf::from(&r.path))
        .or_else(dirs::home_dir)
        .unwrap_or_default();
    if !cx.state.picker.ask(super::picker::Ask::Folder { from, single }) {
        cx.state.settings = true;
    }
}

/// Where a multi named `name` is saved: `root`'s `Multis`, so the browser
/// lists it with the rest.
pub fn multi_path(root: &str, name: &str) -> std::path::PathBuf {
    std::path::Path::new(root)
        .join("Multis")
        .join(format!("{name}.{}", crate::import::SAVED_MULTI))
}

/// The folder whose `Multis` saved multis go in: the first folder of
/// libraries, else the app's data folder (which is scanned too).
pub fn root(cx: &Cx) -> String {
    match cx.settings.roots.iter().find(|r| !r.single) {
        Some(root) => root.path.clone(),
        None => crate::library::data_dir().unwrap_or_default().to_string_lossy().into_owned(),
    }
}

/// Save the rack as a multi at `path`, named for its file, and scan again
/// so the browser lists it.
pub fn save_multi_as(cx: &mut Cx, path: &std::path::Path) -> anyhow::Result<()> {
    let name = stem(&path.to_string_lossy());
    #[cfg(feature = "uvi")]
    let captured = if cx.selection.parts.iter().any(|part| part.uvi.is_some()) {
        // Save the accepted snapshot without changing this frame's native bytes.
        // Its normal merge can then retain a newer controller publication.
        let mut selection = cx.selection.clone();
        cx.p.capture_uvi_state(&mut selection)?;
        Some(selection)
    } else {
        None
    };
    let selection = &cx.selection;
    #[cfg(feature = "uvi")]
    let selection = captured.as_ref().unwrap_or(selection);
    crate::plugin::SavedMulti::of(&name, selection).save(path)?;
    cx.selection.multi = path.to_string_lossy().into_owned();
    cx.p.shared.libraries.rescan();
    Ok(())
}

/// The strip that names and saves the rack as a multi.
pub fn save_multi(ui: &mut Ui, cx: &mut Cx) -> El {
    let Some(name) = cx.state.saving.as_mut() else {
        return block(0, 0);
    };
    if ui.scene().and_then(|s| s.surface("multi-name")).is_none() {
        ui.focus("multi-name");
    }
    let field = text_edit(ui, "multi-name", name, TextOpts::default());
    let cancel = ui.keys("multi-name").iter().any(|k| k.key == Key::Escape);
    let (save, save_el) = action(ui, "multi-save", "Save", false);
    let (close, close_el) = icon_button(ui, "multi-close", Icon::Close, "Close", false);
    if cancel || close {
        cx.state.saving = None;
    } else if save || field.changed.submitted {
        // A name is one file name: no folders, no leading dots.
        let name: String = name
            .trim()
            .trim_start_matches('.')
            .chars()
            .filter(|c| !matches!(c, '/' | '\\' | ':'))
            .collect();
        let result = if name.is_empty() {
            Err(anyhow::anyhow!("Name the multi first"))
        } else {
            save_multi_as(cx, &multi_path(&root(cx), &name))
        };
        match result {
            Ok(()) => cx.state.saving = None,
            Err(e) => cx.state.save_error = format!("{e:#}"),
        }
    }
    let note = if cx.state.save_error.is_empty() {
        "Into the library folder's Multis".to_owned()
    } else {
        cx.state.save_error.clone()
    };
    let error = Some(caption(note).fill(secondary()).lines(1).shrink(0));
    let mut line = vec![
        section("Save multi"),
        field.el.flex(1).min_w(0).h(CONTROL).named("Multi name"),
    ];
    line.extend(error);
    line.extend([save_el, close_el]);
    col![
        row(line).gap(SPACE).align(Align::Center).pad((INSET, SPACE)),
        rule()
    ]
    .gap(0)
    .shrink(0)
    .fill(Role::Surface)
}

/// A line under the top bar. It fills as loading parts read their
/// samples; before any sample is read, a segment sweeps across it.
pub fn loading_bar(view: &View, p: &SamplerParams, started: Instant) -> El {
    let busy = view.parts.iter().any(|v| v.loading) || view.multi_status.starts_with("Loading");
    if !busy {
        return rule();
    }
    let fraction = load_fraction(view, p);
    let phase = (started.elapsed().as_secs_f64() / 1.4).fract();
    canvas(move |s| {
        let (x, w) = if fraction > 0. {
            (0., fraction.min(1.) * s.width)
        } else {
            let w = s.width * 0.3;
            (-w + phase * (s.width + w), w)
        };
        vec![
            Draw::fill(rect(0., 0., s.width, s.height), Role::Ink.alpha(0.1)),
            Draw::fill(rect(x, 0., w, s.height), Role::Ink.alpha(0.7)),
        ]
    })
    .w(Len::Pct(100.))
    .h(1)
    .shrink(0)
    .named("Loading")
}

/// How far the loading parts are, 0..1, averaged.
pub fn load_fraction(view: &View, p: &SamplerParams) -> f64 {
    let fractions: Vec<f64> = (view.parts.iter().enumerate())
        .filter(|(_, v)| v.loading)
        .map(|(n, _)| {
            let done = p.shared.part(n).map_or(0, |part| part.load_progress.load(Ordering::Relaxed));
            f64::from(done) / f64::from(crate::engine::LOAD_DONE)
        })
        .collect();
    fractions.iter().sum::<f64>() / fractions.len().max(1) as f64
}

pub fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
