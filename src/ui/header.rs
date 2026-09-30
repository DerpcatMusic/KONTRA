//! The top bar: wordmark, what is loading, engine meters, master volume and
//! global actions; plus the library folder setting and the load indicator.

use super::{Cx, View, theme::*};
use crate::plugin::{P, SamplerParams};
use moose::mui::{Bridge, mui::prelude::*};
use moose::prelude::*;
use std::sync::atomic::Ordering;
use std::time::Instant;

pub fn top_bar(ui: &mut Ui, cx: &mut Cx, bridge: &mut Bridge<SamplerParams>) -> El {
    let p = cx.p;
    // The audio thread keeps the peak since we last looked; ease it down so it reads.
    let peak = f32::from_bits(p.shared.cpu.swap(0, Ordering::Relaxed) as u32);
    cx.state.cpu = peak.max(cx.state.cpu * 0.92);
    let voices = p.shared.voices.load(Ordering::Relaxed);
    let memory: usize = cx.view.parts.iter().map(|v| v.bytes).sum();

    let loading: Vec<_> = cx
        .view
        .parts
        .iter()
        .zip(&cx.selection.parts)
        .filter(|(v, part)| v.loading && !part.path.is_empty())
        .map(|(_, part)| stem(&part.path))
        .collect();
    let activity = match loading.as_slice() {
        [] if cx.view.multi_status.starts_with("Loading") => cx.view.multi_status.clone(),
        [] => String::new(),
        [one] => format!("Loading {one}…"),
        many => format!("Loading {} instruments…", many.len()),
    };

    let master_text = format!("{:.1} dB", p.volume.value());
    let master = bridge.bind(ui, P::Volume, |ui, id, v| {
        let mut db = *v * 66. - 60.;
        let control = drag_value(ui, id, "Master", &mut db, -60.0..=6.0)
            .size(S)
            .value_text(master_text);
        *v = (db + 60.) / 66.;
        control
    });
    let level = meter(ui, "output", bridge.meter(P::Level)).w(72).h(4);

    let (thru, thru_el) = action(ui, "midi-thru", "MIDI thru", cx.selection.midi_thru);
    if thru {
        cx.selection.midi_thru = !cx.selection.midi_thru;
    }
    let (panic, panic_el) = action(ui, "panic", "Panic", false);
    if panic {
        p.shared.panic.store(true, Ordering::Release);
    }
    let (settings, settings_el) = action(ui, "settings", "Folders", cx.state.settings);
    if settings {
        cx.state.settings = !cx.state.settings;
    }

    row![
        body("KONTAKTO").text_size(14).text_weight(Weight::BOLD),
        caption(fit(&activity, 60))
            .fill(Role::Dim)
            .lines(1)
            .flex(1)
            .min_w(0),
        stat("Voices", voices.to_string(), "0000"),
        stat("CPU", format!("{:.0}%", cx.state.cpu * 100.), "100%"),
        stat("RAM", megabytes(memory), "0000 MB"),
        vrule().h(20),
        row![
            caption("Master").fill(Role::Dim),
            master.radius(2).min_w(64)
        ]
        .gap(HALF)
        .align(Align::Center),
        level,
        vrule().h(20),
        row![thru_el, panic_el, settings_el].gap(HALF),
    ]
    .gap(WIDE)
    .align(Align::Center)
    .pad((WIDE, 0))
    .h(TOP_BAR)
    .shrink(0)
    .fill(Role::Surface)
}

/// The library folder, shown under the top bar while "Folders" is on.
pub fn settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let field = text_input(ui, "root", &mut cx.state.root);
    let (scan, scan_el) = action(ui, "scan", "Rescan", false);
    if scan {
        cx.selection.root = cx.state.root.clone();
        // Forget the scanned root so the loader scans again even when it is unchanged.
        cx.p.shared.view.lock().unwrap().root.clear();
    }
    row![
        caption("Library folder").fill(Role::Dim),
        field.el.flex(1).min_w(0).radius(2).named("Library folder"),
        scan_el
    ]
    .gap(GAP)
    .align(Align::Center)
    .pad((WIDE, GAP))
    .shrink(0)
    .fill(Role::Surface)
}

/// A 2 px line under the top bar. It fills as loading parts read their
/// samples; before any sample is read, a segment sweeps across it.
pub fn loading_bar(view: &View, p: &SamplerParams, started: Instant) -> El {
    let busy = view.parts.iter().any(|v| v.loading) || view.multi_status.starts_with("Loading");
    if !busy {
        return block(Len::Pct(100.), 2)
            .fill(Role::Ink.alpha(0.05))
            .shrink(0);
    }
    let fractions: Vec<f64> = (view.parts.iter().enumerate())
        .filter(|(_, v)| v.loading)
        .map(|(n, _)| {
            let done = p.shared.load_progress[n].load(Ordering::Relaxed);
            f64::from(done) / f64::from(crate::engine::LOAD_DONE)
        })
        .collect();
    let fraction = fractions.iter().sum::<f64>() / fractions.len().max(1) as f64;
    let phase = (started.elapsed().as_secs_f64() / 1.4).fract();
    canvas(move |s| {
        let (x, w) = if fraction > 0. {
            (0., fraction.min(1.) * s.width)
        } else {
            let w = s.width * 0.3;
            (-w + phase * (s.width + w), w)
        };
        let r = |x: f64, w: f64| {
            Path::polyline(
                [(x, 0.), (x + w, 0.), (x + w, s.height), (x, s.height)]
                    .map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        vec![
            Draw::fill(r(0., s.width), Role::Primary.alpha(0.15)),
            Draw::fill(r(x, w), Role::Primary.alpha(1.)),
        ]
    })
    .w(Len::Pct(100.))
    .h(2)
    .shrink(0)
    .named("Loading")
}

pub fn stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
