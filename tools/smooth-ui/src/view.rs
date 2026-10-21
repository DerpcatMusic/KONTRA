//! SMOOTH's editor: the tree, and nothing else.
//!
//! This file states no length. It composes parts from `skin`, and those parts
//! read their metrics from `tokens`; the only numbers here are flex weights,
//! which are proportions rather than sizes. The layout is intrinsic -- a row
//! of column / rail / column, columns of groups, rows of controls -- so the
//! pieces line up because of how they are nested, not because of where they
//! were put.

use crate::graph;
use crate::presets::{self, Presets};
use crate::skin::*;
use crate::slots::{self, Bind, Slider, Slot};
use crate::theme;
use crate::tokens::{icon, metric, radius, space, ty};
use mui::prelude::*;
use std::f64::consts::FRAC_PI_2;

/// The engine's three qualities, as the dropdown names them.
pub const QUALITY_NAMES: [&str; 3] = ["normal", "high", "ultra"];

/// How much more room the name at the top gets than the gaps below it: a
/// proportion of the column's slack, not a distance.
const NAME_SLACK: f64 = 1.4;

/// Whether the primary pointer button is down: an edit that spans frames is
/// one edit until it lets go.
pub fn pointer_down(ui: &Ui) -> bool {
    ui.pointer().buttons.contains(mui::input::Button::Primary)
}

/// What a press on the toolbar asks of the host side: the view cannot undo
/// anything itself, so it says what was asked and the plug-in does it.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Undo,
    Redo,
    /// Swap to the other parameter set.
    SwitchAb,
    /// Replace the other parameter set with a copy of this one.
    CopyAb,
    /// Something the preset menu asked for.
    Preset(presets::Ask),
}

/// What the editor is showing.
pub struct Model {
    /// Which parameter set is live: 0 is a, 1 is b.
    pub ab: usize,
    /// The preset bar and its menu.
    pub presets: Presets,
    /// What the toolbar asked for since the plug-in last looked.
    pub actions: Vec<Action>,
    /// Whether there is anything to undo or to redo.
    pub can_undo: bool,
    pub can_redo: bool,
    /// The bands the graph's handles stand for.
    pub bands: [graph::Band; graph::BANDS],
    /// What the plot draws: the engine's frame, eased, and the depth curves.
    pub plot: graph::PlotView,
    pub soft: bool,
    /// Every continuous control: its normalised value and what it reads as.
    pub slots: [Slider; Slot::COUNT],
    /// The band the band panel is showing.
    pub selected: usize,
    /// Whether the rail's side panel is open.
    pub side: bool,
    /// The stereo mode: left/right (false) or mid/side (true).
    pub mid_side: bool,
    pub bypass: bool,
    pub delta: bool,
    pub low_latency: bool,
    /// The engine's quality: normal, high, ultra.
    pub quality: usize,
    /// Which of the graph's display ranges the chip is on.
    pub range: usize,
    /// The padlocks: a locked setting keeps its value when a preset loads.
    pub lock_quality: bool,
    pub lock_latency: bool,
    /// Which toolbar menu is open, and the interface's preferences.
    pub menu: crate::menus::Open,
    pub prefs: crate::menus::Prefs,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            ab: 0,
            presets: Presets::default(),
            actions: Vec::new(),
            can_undo: false,
            can_redo: false,
            bands: graph::AT_REST_BANDS,
            plot: graph::PlotView::default(),
            soft: true,
            slots: slots::at_rest(),
            selected: 1,
            side: false,
            mid_side: false,
            bypass: false,
            delta: false,
            low_latency: false,
            quality: 0,
            range: graph::DEFAULT_RANGE,
            lock_quality: false,
            lock_latency: false,
            menu: crate::menus::Open::default(),
            prefs: crate::menus::Prefs::default(),
        }
    }
}

/// The whole editor.
///
/// The window is four layers, and the order they stack in is the order the
/// reference draws them: the plot, the rail over it, the footer over the
/// rail's foot, the toolbar over the rail, and the control column over
/// everything. That is why the toolbar's first button tucks under the rail
/// while the footer's first switch sits clear of it -- the same layering, not
/// a placement.
pub fn editor(ui: &mut Ui, m: &mut Model, bind: &mut Bind) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    // The bar's presses first: the menu below is built from what they did,
    // so opening it shows on this frame, not the next.
    let asks = presets::bar_asks(ui, &mut m.presets);
    m.actions.extend(asks.into_iter().map(Action::Preset));
    crate::menus::steer(ui, &mut m.menu);
    crate::tips::enable(m.prefs.tooltips);
    if ui.get("lock-quality").clicked {
        m.lock_quality = !m.lock_quality;
    }
    if ui.get("lock-latency").clicked {
        m.lock_latency = !m.lock_latency;
    }
    // The chip steps through the display ranges.
    if ui.get("range").clicked {
        m.range = (m.range + 1) % graph::RANGES.len();
    }
    stack![
        // The plot, under the toolbar's band.
        col![
            block(W, BAR).shrink(0),
            row![
                block(LEFT, space::NONE).shrink(0),
                graph::response(ui, &mut m.bands, &mut m.selected, &m.plot, m.range, m.prefs.frequency_near_cursor)
            ]
            .w(W),
        ]
        .anchor(Align::Start, Align::Start),
        // The rail, over the plot.
        row![block(PANEL, space::NONE).shrink(0), rail(ui, m)]
            .w(W)
            .h(H)
            .anchor(Align::Start, Align::Start),
        // The preset menu, under the toolbar.
        {
            let (menu, asks) = presets::menu(ui, &mut m.presets);
            m.actions.extend(asks.into_iter().map(Action::Preset));
            col![
                block(W, BAR).shrink(0),
                row![block(TOOLBAR_X, space::NONE).shrink(0), menu],
            ]
            .anchor(Align::Start, Align::Start)
        },
        // The settings or help menu, under the toolbar's right end.
        col![
            block(W, BAR).shrink(0),
            row![spacer(), crate::menus::menu(ui, m.menu, &mut m.prefs)].w(W),
        ]
        .anchor(Align::Start, Align::Start),
        // The selected band's controls, along the plot's foot.
        col![
            block(W, BAR).shrink(0),
            row![block(LEFT, space::NONE).shrink(0), band_bar(ui, m, bind)],
        ]
        .size(W, H - FOOTER)
        .justify(Justify::End)
        .anchor(Align::Start, Align::Start),
        // The side panel the rail opens, over the plot.
        col![
            block(W, BAR).shrink(0),
            row![block(LEFT, space::NONE).shrink(0), side_panel(ui, m, bind)],
        ]
        .anchor(Align::Start, Align::Start),
        // The footer, at the bottom of the window and over the rail's foot:
        // its first switch sits clear of the rail, as the reference's does.
        row![block(FOOTER_X, space::NONE).shrink(0), footer(ui, m, bind)]
            .w(W)
            .justify(Justify::Start)
            .anchor(Align::Start, Align::End),
        // The toolbar, over the rail.
        row![block(TOOLBAR_X, space::NONE).shrink(0), bar(ui, m)]
            .w(W)
            .justify(Justify::Start)
            .anchor(Align::Start, Align::Start),
        // The control column, over everything.
        panel(ui, m, bind).anchor(Align::Start, Align::Start),
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
fn panel(ui: &mut Ui, m: &mut Model, bind: &mut Bind) -> El {
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
        knob_control(ui, bind, m, Slot::Depth, "depth", metric::DIAL),
        spacer(),
        knob_control(ui, bind, m, Slot::Detail, "detail", metric::DIAL_SMALL),
        spacer(),
        row![
            knob_control(ui, bind, m, Slot::Attack, "attack", metric::DIAL_SMALL),
            knob_control(ui, bind, m, Slot::Release, "release", metric::DIAL_SMALL),
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

/// One of the column's knobs, bound to its slot.
fn knob_control(
    ui: &mut Ui,
    bind: &mut Bind,
    m: &mut Model,
    slot: Slot,
    name: &str,
    d: f64,
) -> El {
    control(ui, bind, slot, &mut m.slots[slot.index()], name, d)
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
fn rail(ui: &mut Ui, m: &mut Model) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let mark = metric::RAIL_MARK;
    let label_h = metric::RAIL_LABEL_H;
    let word_w = metric::RAIL_WORD_W;

    // A word turned on its side is as wide as it is long and as tall as a
    // line: the rail is one line wide, so the word keeps its own width and
    // the rail lays it over. A label squeezed to the rail's width would come
    // out as an ellipsis, and worse at any scale but one.
    // A press on a view's marker opens the panel; a double click on the grip,
    // or on the panel's own background, closes it.
    if ["rail-stereo", "rail-tilt", "rail-scale"]
        .iter()
        .any(|id| ui.get(*id).clicked)
    {
        m.side = true;
    }
    if ui.get("rail-grip").double_clicked {
        m.side = false;
    }
    // A view's marker is dark when anything in it differs from its default.
    let differs = |slots: &[Slot]| {
        slots
            .iter()
            .any(|slot| (m.slots[slot.index()].norm - slot.rest().0).abs() > f64::EPSILON)
    };
    let changed = [
        differs(&[Slot::Link, Slot::Focus]),
        differs(&[
            Slot::DetailLow,
            Slot::DetailHigh,
            Slot::AttackLow,
            Slot::AttackHigh,
            Slot::ReleaseLow,
            Slot::ReleaseHigh,
        ]),
        differs(&[Slot::MaxCut]),
    ];
    let group = |id: &str, s: &str, changed: bool| {
        col![
            block(mark, mark)
                .radius(mark / 2.0)
                .fill(if changed { c(t.selected) } else { c(t.thumb) })
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
        .id(id)
        .cursor(Cursor::Grab)
    };

    // What one pitch leaves for the gap between two groups.
    let run = metric::RAIL_PITCH - mark - space::XS - label_h;

    col![
        block(metric::RAIL_GRIP_W, metric::RAIL_GRIP_H)
            .radius(metric::RAIL_GRIP_W / 2.0)
            .fill(c(t.thumb))
            .shrink(0)
            .id("rail-grip"),
        block(space::NONE, space::XL + space::XS).shrink(0),
        group("rail-stereo", "stereo", changed[0]),
        block(space::NONE, run).shrink(0),
        group("rail-tilt", "tilt", changed[1]),
        block(space::NONE, run).shrink(0),
        group("rail-scale", "scale", changed[2]),
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
fn bar(ui: &mut Ui, m: &mut Model) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    for (id, action) in [
        ("undo", Action::Undo),
        ("redo", Action::Redo),
        ("ab-switch", Action::SwitchAb),
        ("ab-copy", Action::CopyAb),
    ] {
        if ui.get(id).clicked {
            m.actions.push(action);
        }
    }
    row![
        // The view controls, a tight group.
        row![text_button("sc"), icon_button(Icon::Headphones)].gap(space::XS),
        preset_bar(
            m.presets.title(),
            m.presets.current_row().map_or("", |r| r.category.as_str()),
            m.presets.current_row().is_some_and(|r| r.favourite),
        ),
        row![
            row![icon_button(Icon::Undo).tipped("undo"), icon_button(Icon::Redo).tipped("redo")].gap(space::XS),
            row![pair_pill("a", "b", m.ab).tipped("ab-switch"), compare_pill("a", "b").tipped("ab-copy")].gap(space::XS),
        ]
        .gap(space::L),
        row![
            mark_button("?").tipped("help"),
            icon_button(Icon::Gear).tipped("gear"),
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
/// A padlock: closed when the setting it guards holds across presets.
fn padlock(on: bool, id: &str) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    glyph(if on { Icon::Locked } else { Icon::Lock }, icon::BASE, c(t.ink)).tipped(id)
}

/// The mix, the output trim, and what the engine is doing.
///
/// Three groups spread across the footer by the row itself: the slack is the
/// layout's, not a spacer's.
fn footer(ui: &mut Ui, m: &mut Model, bind: &mut Bind) -> El {
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
                slot_slider(ui, bind, Slot::Mix, &mut m.slots[Slot::Mix.index()], metric::FIELD_W),
            ]
            .gap(space::XS),
            row![
                label("out", ty::CAPTION, c(t.ink)),
                slot_slider(ui, bind, Slot::Out, &mut m.slots[Slot::Out.index()], metric::FIELD_W),
            ]
            .gap(space::XS),
        ]
        .gap(space::XL),
        // What the engine is doing: the reading, the list it is read
        // against, and the switch that trades latency for it.
        row![
            row![
                padlock(m.lock_quality, "lock-quality"),
                label("quality", ty::CAPTION, c(t.ink)),
            ]
            .gap(space::XS),
            // The reference's rule: quality has no say while low latency holds.
            if m.low_latency {
                dropdown_unavailable(
                    "quality",
                    QUALITY_NAMES[m.quality.min(QUALITY_NAMES.len() - 1)],
                    crate::tips::QUALITY_DISABLED,
                )
            } else {
                dropdown(ui, "quality", &QUALITY_NAMES, &mut m.quality)
            },
            row![
                padlock(m.lock_latency, "lock-latency"),
                toggle_pill(ui, "lowlatency", "low latency", &mut m.low_latency),
            ]
            .gap(space::XS),
        ]
        .gap(space::XS),
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

// ------------------------------------------------------------- side panel --
/// What the rail opens: the stereo section, the tilt section, and the scale
/// section, one panel over the plot's left edge.
///
/// The reference's tooltips name each control: the stereo mode and the two
/// stereo amounts, the three tilts (each scales its setting below 500 Hz and
/// above 2 kHz), and the deepest cut the engine may take.
fn side_panel(ui: &mut Ui, m: &mut Model, bind: &mut Bind) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    if !m.side {
        return block(space::NONE, space::NONE).shrink(0);
    }
    if ui.get("side-panel").double_clicked {
        m.side = false;
    }
    let mut mode = usize::from(m.mid_side);
    let mode_el = segmented(ui, "stereo-mode", &["l/r", "m/s"], &mut mode);
    m.mid_side = mode == 1;
    let mut row_of = |ui: &mut Ui, m: &mut Model, slot: Slot, name: &str| -> El {
        row![
            centred(label(name, ty::CAPTION, c(t.ink)), metric::SIDE_LABEL_W, metric::FIELD_H),
            slot_slider(ui, bind, slot, &mut m.slots[slot.index()], metric::SLIDER_W),
        ]
        .gap(space::XS)
        .align(Align::Center)
        .shrink(0)
    };
    let title = |s: &str| label(s, ty::BODY, c(t.ink_quiet));
    col![
        title("stereo"),
        mode_el,
        row_of(ui, m, Slot::Link, "link"),
        row_of(ui, m, Slot::Focus, "focus"),
        title("tilt"),
        row_of(ui, m, Slot::DetailLow, "detail low"),
        row_of(ui, m, Slot::DetailHigh, "detail high"),
        row_of(ui, m, Slot::AttackLow, "attack low"),
        row_of(ui, m, Slot::AttackHigh, "attack high"),
        row_of(ui, m, Slot::ReleaseLow, "release low"),
        row_of(ui, m, Slot::ReleaseHigh, "release high"),
        title("scale"),
        row_of(ui, m, Slot::MaxCut, "max cut"),
    ]
    .gap(space::S)
    .pad(space::L)
    .w(metric::SIDE_W)
    .radius(radius::BOX)
    .fill(ca(t.ground, 0.94))
    .shrink(0)
    .tipped("side-panel")
}

// --------------------------------------------------------------- band bar --
/// The selected band's controls: its shape, where it sits, how deep and how
/// wide it is, its slope and balance, and the switches that bypass or remove
/// it. The tooltips name each: the shape, the centre frequency, the depth
/// curve's boost or cut, the bandwidth, the slope, the stereo balance.
fn band_bar(ui: &mut Ui, m: &mut Model, bind: &mut Bind) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let index = m.selected.min(graph::BANDS - 1);
    if !m.bands[index].active {
        return block(space::NONE, space::NONE).shrink(0);
    }
    // The shape is one of eight: the dropdown cycles an index, the slot holds
    // it as a step of seven.
    let shape_el = {
        let state = &mut m.slots[Slot::BandShape.index()];
        bind(ui, Slot::BandShape, &mut state.norm, &mut |ui, id, norm| {
            let mut choice = (*norm * 7.0).round() as usize;
            let el = dropdown(ui, id, &graph::SHAPES, &mut choice);
            *norm = choice as f64 / 7.0;
            el
        })
    };
    let mut bypass = m.bands[index].bypass;
    let bypass_el = toggle_pill(ui, "band-bypass", "bypass", &mut bypass);
    m.bands[index].bypass = bypass;
    if ui.get("band-delete").clicked {
        m.bands[index].active = false;
    }
    // A cut or a reject has no gain to set: the depth slot is dimmed.
    let no_depth = matches!((m.slots[Slot::BandShape.index()].norm * 7.0).round() as usize, 0 | 4 | 7);
    let mut pick = |ui: &mut Ui, slot: Slot| {
        slot_slider(ui, bind, slot, &mut m.slots[slot.index()], metric::SLIDER_W)
    };
    row![
        shape_el,
        pick(ui, Slot::BandFrequency),
        if no_depth {
            unavailable_slider("band-depth", "n/a", metric::SLIDER_W, crate::tips::NO_DEPTH)
        } else {
            pick(ui, Slot::BandDepth)
        },
        pick(ui, Slot::BandWidth),
        pick(ui, Slot::BandSlope),
        pick(ui, Slot::BandBalance),
        bypass_el,
        text_button("x").tipped("band-delete"),
    ]
    .gap(space::XS)
    .pad(space::XS)
    .align(Align::Center)
    .radius(radius::BOX)
    .fill(ca(t.ground, 0.94))
    .shrink(0)
}
