//! The mixer: a channel strip per part, as v2 drew it: its colour, name,
//! pan, fader and meter, solo and mute, output.

use mui::prelude::*;

use super::rack::{output, pan, solo_mute};
use super::{Action, App, db};
use crate::kit::{self, section, strip};
use crate::theme::{SPACE, TEXT, TIGHT, lift, part_color, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let widths = ["Narrow", "Wide"];
    let width = kit::dropdown(
        ui,
        &mut app.view.menu,
        "mixer-strips",
        (
            None,
            if app.view.mixer_wide {
                "Wide"
            } else {
                "Narrow"
            },
            "Strip width",
        ),
        &widths,
        Some(usize::from(app.view.mixer_wide)),
    );
    if let Some(w) = width.changed {
        app.view.mixer_wide = w == 1;
    }
    let each = app.parts.iter().zip(1..).all(|(p, k)| p.output == k);
    let one = app.parts.iter().all(|p| p.output == 1);
    let layouts = ["One per instrument", "All to st.1"];
    let layout = kit::dropdown(
        ui,
        &mut app.view.menu,
        "mixer-outputs",
        (
            None,
            if each {
                "One per instrument"
            } else if one {
                "All to st.1"
            } else {
                "Custom"
            },
            "Route every part at once",
        ),
        &layouts,
        None,
    );
    if let Some(l) = layout.changed {
        for (n, p) in app.parts.iter_mut().enumerate() {
            p.output = if l == 0 { (n + 1).min(16) as u8 } else { 1 };
            out.push(Action::Output(n, p.output));
        }
    }
    let bar = strip(vec![
        section("Strips"),
        width.el,
        spacer(),
        section("Outputs"),
        layout.el,
    ]);
    let w = if app.view.mixer_wide {
        TEXT * 10.
    } else {
        TEXT * 7.
    };
    let strips: Vec<El> = (0..app.parts.len())
        .map(|n| channel(ui, app, n, w, out))
        .collect();
    let body = if strips.is_empty() {
        caption("Load an instrument and its strip shows here.")
            .fill(secondary())
            .pad(SPACE * 2.)
    } else {
        row(strips)
            .gap(TIGHT)
            .pad(SPACE)
            .align(Align::Start)
            .scroll()
    };
    col![bar, body.min_h(0).grow(1)]
}

fn channel(ui: &mut Ui, app: &mut App, n: usize, w: f64, out: &mut Vec<Action>) -> El {
    let id = |what: &str| format!("mix-{n}-{what}");
    let output = output(ui, app, n, id("output"), out);
    let Some(p) = app.parts.get_mut(n) else {
        return spacer();
    };
    let mut value = p.pan;
    let pan = pan(ui, id("pan"), &mut value);
    if pan.changed {
        p.pan = value;
        out.push(Action::Pan(n, value));
    }
    let mut value = p.volume;
    let height = TEXT * 22.;
    let fader = kit::slider(
        ui,
        id("volume"),
        "Volume",
        &mut value,
        -60.0..=12.0,
        0.,
        (TEXT, height),
    );
    if fader.changed {
        p.volume = value;
        out.push(Action::Volume(n, value));
    }
    let t = p.level.first().map_or(0., |[l, r]| l.max(*r)).clamp(0., 1.);
    let meter = col![
        block(3, 0).grow(1. - t),
        block(3, 0).grow(t).fill(Fill::from(part_color(n)))
    ]
    .h(height)
    .fill(lift(3))
    .shrink(0);
    let kind = if p.library.is_empty() {
        "Bus"
    } else {
        "Instrument"
    };
    col![
        block(Len::Pct(100.), 2).fill(Fill::from(part_color(n))),
        caption(p.name.clone())
            .text_weight(Weight::SEMIBOLD)
            .lines(2)
            .w(Len::Pct(100.))
            .h(TEXT * 2.6)
            .tip(p.name.clone()),
        caption(kind).fill(secondary()),
        pan.el,
        row![fader.el, meter].gap(TIGHT),
        caption(db(p.volume)).reserve("-inf dB"),
        solo_mute(ui, p, n, &id("sm"), out),
        output,
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad(TIGHT)
    .w(w)
    .fill(lift(1))
    .shrink(0)
}
