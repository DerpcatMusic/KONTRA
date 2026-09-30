//! The top bar: browser toggle, wordmark, what is loading, engine readouts,
//! master volume with its meter and the global actions; plus the library
//! folder setting and the load indicator under it.

use super::{Cx, View, menu, theme::*};
use crate::plugin::{P, SamplerParams};
use moose::mui::{Bridge, mui::prelude::*};
use std::sync::atomic::Ordering;
use std::time::Instant;

pub fn top_bar(ui: &mut Ui, cx: &mut Cx, bridge: &mut Bridge<SamplerParams>) -> El {
    let p = cx.p;
    let cpu = f32::from_bits(cx.state.cpu.load(Ordering::Relaxed));
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
        body("KONTAKTO")
            .text_size(TEXT + 1.)
            .text_weight(Weight::BOLD)
            .fill(Role::Ink)
            .shrink(0),
        caption(activity).fill(Role::Dim).lines(1).flex(1).min_w(0),
        stat("CPU", format!("{:.0}%", cpu * 100.), "100%"),
        stat("Voices", voices.to_string(), "000"),
        stat("RAM", megabytes(memory), "00000 MB"),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![section("Master"), master, meter_bar(level)]).gap(SPACE),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![thru_el, panic_el, more_el]),
    ])
    .gap(INSET)
    .pad((SPACE, SPACE))
    .fill(Role::Surface)
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

/// The library folder, shown under the top bar while it is being set.
pub fn settings(ui: &mut Ui, cx: &mut Cx) -> El {
    let field = text_input(ui, "root", &mut cx.state.root);
    let (scan, scan_el) = action(ui, "scan", "Rescan", false);
    if scan {
        cx.selection.root = cx.state.root.clone();
        // Forget the scanned root so the loader scans again even when it is unchanged.
        super::lock(&cx.p.shared.view).root.clear();
    }
    let (close, close_el) = icon_button(ui, "settings-close", Icon::Close, "Close", false);
    if close {
        cx.state.settings = false;
    }
    col![
        row![
            section("Library folder"),
            field
                .el
                .flex(1)
                .min_w(0)
                .h(CONTROL)
                .named("Library folder"),
            scan_el,
            close_el
        ]
        .gap(SPACE)
        .align(Align::Center)
        .pad((SPACE, INSET)),
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
            let done = p.shared.load_progress[n].load(Ordering::Relaxed);
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
