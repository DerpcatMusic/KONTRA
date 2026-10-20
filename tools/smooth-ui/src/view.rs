//! SMOOTH's editor: the tree, and nothing else.
//!
//! This file states no length. It composes parts from `skin`, and those parts
//! read their metrics from `tokens`; the only numbers here are flex weights,
//! which are proportions rather than sizes. The layout is intrinsic -- a row
//! of column / rail / column, columns of groups, rows of controls -- so the
//! pieces line up because of how they are nested, not because of where they
//! were put.

use crate::graph;
use crate::skin::*;
use crate::theme;
use crate::tokens::{icon, metric, radius, space, ty};
use mui::prelude::*;
use std::f64::consts::FRAC_PI_2;

/// The engine's three qualities, as the dropdown names them.
pub const QUALITY_NAMES: [&str; 3] = ["normal", "high", "ultra"];

/// How much more room the name at the top gets than the gaps below it: a
/// proportion of the column's slack, not a distance.
const NAME_SLACK: f64 = 1.4;

/// What the editor is showing.
pub struct Model {
    /// The bands the graph's handles stand for.
    pub bands: [graph::Band; graph::BANDS],
    /// What the engine is doing, read by the plot.
    pub telemetry: std::sync::Arc<graph::Telemetry>,
    pub soft: bool,
    pub depth: f64,
    pub detail: f64,
    pub attack: f64,
    pub release: f64,
    pub mix: f64,
    pub out: f64,
    pub bypass: bool,
    pub delta: bool,
    pub low_latency: bool,
    /// The engine's quality: normal, high, ultra.
    pub quality: usize,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            bands: graph::AT_REST_BANDS,
            telemetry: std::sync::Arc::new(graph::Telemetry::default()),
            soft: true,
            depth: 6.0,
            detail: 6.3,
            attack: 5.0,
            release: 3.0,
            mix: 100.0,
            out: 0.0,
            bypass: false,
            delta: false,
            low_latency: false,
            quality: 0,
        }
    }
}

/// The whole editor.
///
/// The window is four layers, and the order they stack in is the order the
/// reference draws them: the plot and the footer at the bottom, the rail over
/// them, the toolbar over the rail, and the control column over everything.
/// That is why the toolbar's first button tucks under the rail and the
/// footer's first switch is half hidden by it -- the same layering, not a
/// placement.
pub fn editor(ui: &mut Ui, m: &mut Model, dial: &mut Dial) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    stack![
        // The plot, under the toolbar's band.
        col![
            block(W, BAR).shrink(0),
            row![
                block(LEFT, space::NONE).shrink(0),
                graph::response(ui, &mut m.bands, &m.telemetry)
            ]
            .w(W),
        ]
        .anchor(Align::Start, Align::Start),
        // The footer, at the bottom of the window, under the rail: its first
        // switch runs under the rail's edge, as the reference's does.
        row![block(FOOTER_X, space::NONE).shrink(0), footer(ui, m)]
            .w(W)
            .justify(Justify::Start)
            .anchor(Align::Start, Align::End),
        // The rail, over the footer's edge and the plot.
        row![block(PANEL, space::NONE).shrink(0), rail()]
            .w(W)
            .h(H)
            .anchor(Align::Start, Align::Start),
        // The toolbar, over the rail.
        row![block(TOOLBAR_X, space::NONE).shrink(0), bar()]
            .w(W)
            .justify(Justify::Start)
            .anchor(Align::Start, Align::Start),
        // The control column, over everything.
        panel(ui, m, dial).anchor(Align::Start, Align::Start),
    ]
    .size(W, H)
    .radius(radius::WINDOW)
    .fill(c(t.ground))
}

// ------------------------------------------------------------------ panel --
/// The control column: the name, the mode, and the four bands.
///
/// The spacers are what make it read: the slack between the groups is shared
/// evenly, so the column breathes at any height instead of sitting at a
/// hand-computed offset.
fn panel(ui: &mut Ui, m: &mut Model, dial: &mut Dial) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    // The switch carries an index; the model carries the mode. The part runs
    // first so the choice it made is written back before the tree is built.
    let mut mode = usize::from(!m.soft);
    let mode_el = segmented(ui, "mode", &["soft", "hard"], &mut mode);
    m.soft = mode == 0;
    col![
        brand(),
        // The first gap is the widest: the name sits on its own above the
        // controls.
        spacer().grow(NAME_SLACK),
        mode_el,
        spacer(),
        control(
            ui,
            dial,
            "depth",
            "depth",
            &mut m.depth,
            0.0..=10.0,
            metric::DIAL
        ),
        spacer(),
        control(
            ui,
            dial,
            "detail",
            "detail",
            &mut m.detail,
            0.0..=10.0,
            metric::DIAL_SMALL
        ),
        spacer(),
        row![
            control(
                ui,
                dial,
                "attack",
                "attack",
                &mut m.attack,
                0.0..=10.0,
                metric::DIAL_SMALL
            ),
            control(
                ui,
                dial,
                "release",
                "release",
                &mut m.release,
                0.0..=10.0,
                metric::DIAL_SMALL
            ),
        ]
        .gap(space::M)
        .justify(Justify::SpaceBetween),
    ]
    .size(PANEL, H)
    // More clear below the last control than above the name: the panel's
    // content ends higher than the window does.
    .pad(mui::scene::Pad::Insets(mui::layout::Insets {
        left: space::NONE,
        right: space::NONE,
        top: space::L,
        bottom: space::XL + space::XS,
    }))
    .align(Align::Center)
    .shrink(0)
    .fill(Gradient::vertical(c(t.panel_top), c(t.panel_bottom)))
}

/// The lockup at the top: the name over who makes it and which version.
fn brand() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    col![
        wordmark(WORDMARK, ty::WORDMARK, c(t.ink_logo)),
        row![
            glyph(Icon::Globe, icon::LARGE, c(t.ink_quiet)),
            label(MAKER, ty::CAPTION, c(t.ink)),
            label(VERSION, ty::CAPTION, c(t.ink_quiet)),
        ]
        .gap(space::XS)
        .align(Align::Center),
    ]
    .gap(space::NONE)
    .align(Align::Start)
    .shrink(0)
}

// ------------------------------------------------------------------- rail --
/// The side rail: a handle at the top, then one marker per view.
///
/// The three groups are one pitch apart, and the pitch is a token rather than
/// a position: change `metric::RAIL_PITCH` and the rail re-spaces itself.
fn rail() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mark = metric::RAIL_MARK;
    let label_h = metric::RAIL_LABEL_H;
    let word_w = metric::RAIL_WORD_W;

    // A word turned on its side is as wide as it is long and as tall as a
    // line: the rail is one line wide, so the word keeps its own width and
    // the rail lays it over. A label squeezed to the rail's width would come
    // out as an ellipsis, and worse at any scale but one.
    let group = |s: &str| {
        col![
            block(mark, mark)
                .radius(mark / 2.0)
                .fill(c(t.thumb))
                .shrink(0),
            stack![label(s, ty::FINE, c(t.ink)).reserve(s)]
                .size(word_w, label_h)
                .center()
                .shrink(0)
                .rotation(-FRAC_PI_2),
        ]
        .gap(space::XS)
        .align(Align::Center)
        .shrink(0)
    };

    // What one pitch leaves for the gap between two groups.
    let run = metric::RAIL_PITCH - mark - space::XS - label_h;

    col![
        block(metric::RAIL_GRIP_W, metric::RAIL_GRIP_H)
            .radius(metric::RAIL_GRIP_W / 2.0)
            .fill(c(t.thumb))
            .shrink(0),
        block(space::NONE, space::XL + space::XS).shrink(0),
        group("stereo"),
        block(space::NONE, run).shrink(0),
        group("tilt"),
        block(space::NONE, run).shrink(0),
        group("scale"),
        spacer(),
    ]
    .pad((space::S, space::NONE))
    .w(RAIL)
    .align(Align::Center)
    .fill(Gradient::linear(
        90.0,
        [(0.0, c(t.rail_left)), (1.0, c(t.rail_right))],
    ))
}

// ---------------------------------------------------------------- toolbar --
/// The preset bar and the view controls, over the plot.
///
/// Four groups on one row, spaced by the row's own gap: no spacer blocks, no
/// optical fudge. Where a group starts follows from what is in it.
fn bar() -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    row![
        // The view controls, a tight group.
        row![text_button("sc"), icon_button(Icon::Headphones)].gap(space::XS),
        preset_bar("Smooth vocal", "vocals"),
        row![
            row![icon_button(Icon::Undo), icon_button(Icon::Redo)].gap(space::XS),
            row![pair_pill("a", "b", 0), compare_pill("a", "b")].gap(space::XS),
        ]
        .gap(space::L),
        row![
            mark_button("?"),
            icon_button(Icon::Gear),
            icon_button(Icon::Expand),
        ]
        .gap(space::XS),
    ]
    .gap(space::L)
    .pad((space::NONE, space::TIGHT))
    .h(BAR)
    .align(Align::Center)
    .justify(Justify::Start)
    .shrink(0)
    .grow(1.0)
    .fill(c(t.bar))
}

// ----------------------------------------------------------------- footer --
/// The mix, the output trim, and what the engine is doing.
///
/// Three groups spread across the footer by the row itself: the slack is the
/// layout's, not a spacer's.
fn footer(ui: &mut Ui, m: &mut Model) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    row![
        // The switches.
        row![
            toggle_pill(ui, "bypass", "bypass", &mut m.bypass),
            toggle_pill(ui, "delta", "delta", &mut m.delta),
        ]
        .gap(space::S),
        // The mix and the trim: a name and the plate it names, twice.
        row![
            row![
                label("mix", ty::CAPTION, c(t.ink)),
                plate(&format!("{:.0} %", m.mix)),
            ]
            .gap(space::XS),
            row![
                label("out", ty::CAPTION, c(t.ink)),
                plate(&format!("{:.1} dB", m.out)),
            ]
            .gap(space::XS),
        ]
        .gap(space::M),
        // What the engine is doing: the reading, the list it is read
        // against, and the switch that trades latency for it.
        row![
            row![
                glyph(Icon::Quality, icon::BASE, c(t.ink)),
                label("quality", ty::CAPTION, c(t.ink)),
            ]
            .gap(space::XS),
            dropdown(ui, "quality", &QUALITY_NAMES, &mut m.quality),
            row![
                glyph(Icon::Lock, icon::BASE, c(t.ink)),
                toggle_pill(ui, "lowlatency", "low latency", &mut m.low_latency),
            ]
            .gap(space::XS),
        ]
        .gap(space::L),
    ]
    .justify(Justify::SpaceBetween)
    // The footer runs to the window's edge; its last control stops short of
    // it by one wide margin, and its first starts one step in.
    .pad(mui::scene::Pad::Insets(mui::layout::Insets {
        left: space::NONE,
        right: space::L,
        top: space::S,
        bottom: space::S,
    }))
    .h(FOOTER)
    .align(Align::Center)
    .shrink(0)
    .grow(1.0)
    .fill(c(t.ground))
}
