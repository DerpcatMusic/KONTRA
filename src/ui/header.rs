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
    let cpu = f32::from_bits(cx.state.meters.cpu.load(Ordering::Relaxed));
    let disk = f32::from_bits(cx.state.meters.disk.load(Ordering::Relaxed));
    let voices = p.shared.voices.load(Ordering::Relaxed);
    let audible = p.shared.audible.load(Ordering::Relaxed);
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
        // Streamed audio played late or script calls lost; zero when all is well.
        [] => match p.shared.dropouts.load(Ordering::Relaxed) {
            0 => String::new(),
            n => format!("{n} audio dropouts"),
        },
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
        body("KONTAKTO")
            .text_size(TEXT + 1.)
            .text_weight(Weight::BOLD)
            .fill(Role::Ink)
            .shrink(0),
        caption(activity).fill(Role::Dim).lines(1).flex(1).min_w(0),
        stat("CPU", format!("{:.0}%", cpu * 100.), "100%"),
        // Heard voices; scripts start and mute crossfade layers and mic
        // positions too, which cost next to nothing.
        stat("Voices", audible.to_string(), "000").tip(format!(
            "{voices} running, {} muted by the script",
            voices.saturating_sub(audible)
        )),
        stat("RAM", megabytes(memory), "00000 MB"),
        stat("Disk", format!("{disk:.1} MB/s"), "000.0 MB/s"),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![section("Master"), master, meter_bar(level)]).gap(SPACE),
        vrule().h(CONTROL - TIGHT),
        cluster(vec![qwerty_el, thru_el, panic_el, more_el]),
    ])
    .gap(SPACE)
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
        .pad((INSET, SPACE)),
        rule()
    ]
    .gap(0)
    .shrink(0)
    .fill(Role::Surface)
}

/// Where a multi named `name` is saved: the library folder's `Multis`, so
/// the browser lists it with the rest.
pub fn multi_path(root: &str, name: &str) -> std::path::PathBuf {
    std::path::Path::new(root)
        .join("Multis")
        .join(format!("{name}.{}", crate::import::SAVED_MULTI))
}

/// The library folder, or the default when none is set.
pub fn root(cx: &Cx) -> String {
    match cx.selection.root.as_str() {
        "" => crate::import::LIBRARY_ROOT.to_owned(),
        root => root.to_owned(),
    }
}

/// Save the rack as a multi at `path`, named for its file, and scan again
/// so the browser lists it.
pub fn save_multi_as(cx: &mut Cx, path: &std::path::Path) -> anyhow::Result<()> {
    let name = stem(&path.to_string_lossy());
    crate::plugin::SavedMulti::of(&name, &cx.selection).save(path)?;
    cx.selection.multi = path.to_string_lossy().into_owned();
    super::lock(&cx.p.shared.view).root.clear();
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
    let error = Some(caption(note).fill(Role::Dim).lines(1).shrink(0));
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
