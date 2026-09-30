//! The look: one accent, graphite neutrals, an 8 px grid, one bundled face,
//! and the few controls every view shares: flat buttons, icon buttons, tabs,
//! section bars and the Koda-style faders.

use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use moose::mui::mui::{layout::Insets, scene::TypeScale};
use std::f64::consts::PI;
use std::ops::RangeInclusive;

/// The spacing grid. Everything pads and gaps by these.
pub const GAP: f64 = 8.0;
pub const HALF: f64 = 4.0;
pub const WIDE: f64 = 16.0;

/// Padding per side, in CSS order.
pub fn edges(top: f64, right: f64, bottom: f64, left: f64) -> Insets {
    Insets {
        left,
        right,
        top,
        bottom,
    }
}

/// Fixed strip heights.
pub const TOP_BAR: f64 = 40.0;
/// A section's title bar, and a list row.
pub const BAR: f64 = 28.0;
/// A control's height: buttons, fields, faders.
pub const CONTROL: f64 = 24.0;
/// The browser's width when first shown, and how far it resizes.
pub const SIDEBAR: f64 = 272.0;
pub const SIDEBAR_MIN: f64 = 216.0;
pub const SIDEBAR_MAX: f64 = 440.0;

/// The accent's hue and chroma (OKLCH). Selection, focus, values, played keys.
const ACCENT_HUE: f32 = 64.0;
const ACCENT_CHROMA: f32 = 0.15;

pub fn accent() -> Color {
    Color::oklch(0.76, ACCENT_CHROMA, ACCENT_HUE)
}

/// The hairline every section boundary is drawn with.
pub fn hairline() -> Fill {
    Role::Ink.alpha(0.08)
}

pub fn ui() -> Ui {
    Ui::new(Theme {
        palette: Palette {
            neutral: Pigment::new(260.0, 0.008),
            primary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            secondary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            tertiary: Pigment::new(ACCENT_HUE, ACCENT_CHROMA),
            step: 0.035,
            ..Palette::NEUTRAL
        },
        // Kontakt 8's hardware-panel edges: square, 2 px where a hit area needs one.
        corners: Corners {
            field: 2.,
            box_: 0.,
            selector: 2.,
            ..Corners::DEFAULT
        },
        text: 13.,
        control: 3.,
        type_scale: TypeScale {
            title: 20.,
            body: 13.,
            caption: 11.,
        },
        ..Theme::DEFAULT
    })
    .font(
        Font::new(include_bytes!("../../assets/NotoSans.ttf").as_slice())
            .expect("bundled Noto Sans"),
    )
}

/// State changes land in about a tenth of a second: felt, not watched.
pub fn quick() -> Spring {
    Spring::new(0.12, 1.)
}

/// The interactive states every flat control shares: a faint lift on hover,
/// a deeper one while pressed, an accent ring only for keyboard focus.
pub fn interactive(el: El, selected: bool) -> El {
    el.on(State::Hover, move |s| {
        if selected {
            s
        } else {
            s.fill(Role::Ink.alpha(0.06))
        }
    })
    .on(State::Press, |s| s.fill(Role::Ink.alpha(0.11)))
    .on(State::FocusVisible, |s| {
        s.stroke(Role::Primary.alpha(0.9)).stroke_width(1)
    })
    .animate_with(quick())
}

/// A quiet text button; `selected` raises it on a neutral fill. Text stays ink:
/// the accent marks state, never words.
pub fn action(ui: &mut Ui, id: impl Into<Id>, label: &str, selected: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        body(label.to_owned())
            .text_size(12)
            .fill(if selected { Role::Ink } else { Role::Dim })
            .lines(1)
            .min_w(0)
    ]
    .align(Align::Center)
    .justify(Justify::Center)
    .pad((GAP + 2., 0))
    .h(CONTROL)
    .radius(2)
    .when(selected, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .id(id);
    (hit, interactive(el, selected))
}

/// Line icons drawn on a 16-unit grid, so they stay crisp at any size and
/// need no icon font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Left,
    Right,
    Up,
    Down,
    More,
    Plus,
    Close,
    Sidebar,
    Search,
    Play,
    Menu,
}

/// `icon` in `ink`, `size` points square.
pub fn glyph(icon: Icon, size: f64, ink: Fill) -> El {
    canvas(move |s| {
        let u = s.width.min(s.height) / 16.;
        let (ox, oy) = ((s.width - 16. * u) / 2., (s.height - 16. * u) / 2.);
        let p = |x: f64, y: f64| Point::new(ox + x * u, oy + y * u);
        let line = |pts: &[(f64, f64)]| {
            Draw::stroke(
                DrawPath::polyline(pts.iter().map(|&(x, y)| p(x, y)), false),
                ink.clone(),
                1.5 * u.max(0.75),
            )
        };
        let dot = |x: f64, y: f64| Draw::fill(rect(ox + (x - 1.) * u, oy + (y - 1.) * u, 2. * u, 2. * u), ink.clone());
        match icon {
            Icon::Left => vec![line(&[(10., 4.), (6., 8.), (10., 12.)])],
            Icon::Right => vec![line(&[(6., 4.), (10., 8.), (6., 12.)])],
            Icon::Up => vec![line(&[(4., 10.), (8., 6.), (12., 10.)])],
            Icon::Down => vec![line(&[(4., 6.), (8., 10.), (12., 6.)])],
            Icon::More => vec![dot(3.5, 8.), dot(8., 8.), dot(12.5, 8.)],
            Icon::Plus => vec![line(&[(8., 3.5), (8., 12.5)]), line(&[(3.5, 8.), (12.5, 8.)])],
            Icon::Close => vec![line(&[(4.5, 4.5), (11.5, 11.5)]), line(&[(11.5, 4.5), (4.5, 11.5)])],
            Icon::Sidebar => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5), (2.5, 3.5)]),
                line(&[(6.5, 3.5), (6.5, 12.5)]),
            ],
            Icon::Search => vec![
                Draw::stroke(arc(ox + 7. * u, oy + 7. * u, 4. * u, 0., 2. * PI), ink.clone(), 1.5 * u.max(0.75)),
                line(&[(10., 10.), (13.5, 13.5)]),
            ],
            Icon::Play => vec![Draw::fill(
                DrawPath::polyline([p(5., 3.5), p(12.5, 8.), p(5., 12.5)], true),
                ink.clone(),
            )],
            Icon::Menu => vec![
                line(&[(3., 4.5), (13., 4.5)]),
                line(&[(3., 8.), (13., 8.)]),
                line(&[(3., 11.5), (13., 11.5)]),
            ],
        }
    })
    .square(size)
    .shrink(0)
}

/// A square icon button. `on` holds it raised, like a toggle that is set.
pub fn icon_button(ui: &mut Ui, id: impl Into<Id>, icon: Icon, name: &str, on: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let hover = ui.state(id.clone()).hover as f32;
    let ink = if on {
        Role::Ink.alpha(1.)
    } else {
        Role::Ink.alpha(0.6 + 0.4 * hover)
    };
    let el = stack![glyph(icon, 16., ink).centered()]
        .square(CONTROL)
        .radius(2)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id);
    (hit, interactive(el, on))
}

/// A small square switch with a letter: Mute and Solo. Set, it takes the
/// accent fill; the letter then reads dark on it.
pub fn letter_toggle(ui: &mut Ui, id: impl Into<Id>, letter: &str, name: &str, on: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        caption(letter.to_owned())
            .text_size(11)
            .text_weight(Weight::SEMIBOLD)
            .fill(if on {
                Color::oklch(0.18, 0., 0.).into()
            } else {
                Role::Dim.alpha(1.)
            })
    ]
    .align(Align::Center)
    .justify(Justify::Center)
    .square(20)
    .radius(2)
    .fill(if on { accent().into() } else { Role::Ink.alpha(0.05) })
    .focusable()
    .a11y(A11y::Toggle { on })
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    (
        hit,
        el.on(State::Hover, move |s| if on { s } else { s.fill(Role::Ink.alpha(0.1)) })
            .on(State::FocusVisible, |s| s.stroke(Role::Primary.alpha(0.9)).stroke_width(1))
            .animate_with(quick()),
    )
}

/// A view switch: ink when current, dim otherwise, with an accent underline.
pub fn tab(ui: &mut Ui, id: impl Into<Id>, label: &str, current: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let hover = ui.state(id.clone()).hover as f32;
    let underline = if current {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.18 * hover)
    };
    let el = col![
        spacer(),
        body(label.to_owned())
            .text_size(12)
            .fill(if current { Role::Ink } else { Role::Dim })
            .lines(1),
        spacer(),
        block(Len::Pct(100.), 2).fill(underline)
    ]
    .gap(0)
    .align(Align::Center)
    .pad((0, GAP + 2.))
    .h(Len::Pct(100.))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .on(State::FocusVisible, |s| s.fill(Role::Ink.alpha(0.06)))
    .id(id)
    .shrink(0);
    (hit, el)
}

/// A horizontal hairline.
pub fn rule() -> El {
    block(Len::Pct(100.), 1).fill(hairline()).shrink(0)
}

/// A vertical hairline.
pub fn vrule() -> El {
    block(1, Len::Pct(100.)).fill(hairline()).shrink(0)
}

/// A small uppercase label.
pub fn section(label: &str) -> El {
    caption(label.to_uppercase())
        .text_size(10)
        .fill(Role::Dim)
        .text_weight(Weight::SEMIBOLD)
        .lines(1)
        .min_w(0)
}

/// A section's title bar: small caps on the left, `actions` on the right.
pub fn section_bar(label: &str, actions: Vec<El>) -> El {
    let mut items = vec![section(label), spacer()];
    items.extend(actions);
    row(items)
        .gap(HALF)
        .align(Align::Center)
        .pad(edges(0., HALF, 0., GAP + HALF))
        .h(BAR)
        .shrink(0)
}

/// A labelled number to drag, type or step.
pub fn number(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    display: String,
) -> El {
    let reserve = display.clone();
    let c = drag_value(ui, id, label, value, range).size(S);
    row![
        caption(label).fill(Role::Dim).lines(1),
        c.el.value_text(display)
            .el()
            .radius(2)
            .min_w(40)
            .h(CONTROL - 2.)
            .reserve(reserve)
    ]
    .gap(HALF + 2.)
    .align(Align::Center)
    .shrink(0)
}

/// A bare number field for a table cell, `width` wide.
pub fn field(
    ui: &mut Ui,
    id: impl Into<Id>,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    display: String,
    width: f64,
) -> El {
    drag_value(ui, id, name, value, range)
        .size(S)
        .value_text(display)
        .el
        .el()
        .radius(2)
        .w(width)
        .h(CONTROL - 2.)
        .shrink(0)
}

/// A dim label beside an ink value that keeps its width as digits change.
pub fn stat(label: &str, value: String, widest: &str) -> El {
    row![
        section(label),
        caption(value).text_size(12).reserve(widest.to_owned())
    ]
    .gap(HALF + 2.)
    .align(Align::Center)
    .shrink(0)
}

/// How a fader's fill runs.
#[derive(Clone, Copy, PartialEq)]
pub struct Fader {
    /// Where the fill starts: the range's low end for a level, its middle for pan.
    pub origin: f64,
    /// A marked value on the track: center for pan, unity for a level.
    pub detent: Option<f64>,
    /// What a double-click restores.
    pub reset: f64,
    /// The widest readout, so the track keeps its length as the value changes.
    pub widest: &'static str,
}

impl Fader {
    pub const PAN: Self = Self {
        origin: 0.,
        detent: Some(0.),
        reset: 0.,
        widest: "R 100",
    };
    pub const LEVEL: Self = Self {
        origin: -60.,
        detent: Some(0.),
        reset: 0.,
        widest: "-60.0 dB",
    };
}

/// A Koda-style fader: a thin track with its detent marked, a fill from the
/// origin to the value, a small square thumb, and a readout beside it. Drag
/// sideways (Shift is fine), wheel or arrow keys step, double-click resets.
/// `width` fixes the track; `None` lets it take the room it is given.
pub fn fader(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    kind: Fader,
    readout: fn(f64) -> String,
    width: Option<f64>,
) -> (bool, El) {
    let (lo, hi) = (*range.start(), *range.end());
    let before = *value;
    let track = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or(96., |s| s.frame.size.width)
        .max(24.);
    let r = ui.get(id);
    ui.drag(id, value, range.clone(), track - 6., false);
    if let Some(wheel) = ui.wheel(id) {
        let step = (hi - lo) / if r.mods.shift { 500. } else { 50. };
        let dir = if wheel.y.abs() >= wheel.x.abs() { -wheel.y } else { wheel.x };
        *value = (*value + dir.signum() * step).clamp(lo, hi);
    }
    stepped(ui, id, value, &range);
    if r.double_clicked {
        *value = kind.reset;
    }
    let state = ui.state(id);
    let held = r.held;
    let unit = |v: f64| ((v - lo) / (hi - lo)).clamp(0., 1.);
    let (at, from) = (unit(*value), unit(kind.origin));
    let detent = kind.detent.map(unit);
    let lift = state.hover.max(if held { 1. } else { 0. }) as f32;
    let focused = ui.focus_visible(id);
    let track_el = canvas(move |s| {
        let (w, h) = (s.width, s.height);
        let inner = w - 6.;
        let x = |u: f64| 3. + u * inner;
        let mid = (h / 2.).round();
        let mut draw = vec![Draw::fill(rect(3., mid - 1., inner, 2.), Role::Ink.alpha(0.14 + 0.06 * lift))];
        let (a, b) = if x(at) < x(from) { (x(at), x(from)) } else { (x(from), x(at)) };
        if b - a > 0.5 {
            draw.push(Draw::fill(rect(a, mid - 1., b - a, 2.), accent()));
        }
        if let Some(d) = detent {
            draw.push(Draw::fill(rect(x(d).round() - 0.5, mid - 5., 1., 10.), Role::Ink.alpha(0.4)));
        }
        let thumb = Color::oklch(0.8 + 0.15 * lift, 0., 0.);
        draw.push(Draw::fill(rect((x(at) - 3.).round(), mid - 5., 6., 10.), thumb));
        if focused {
            draw.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), Role::Primary.alpha(0.9), 1.));
        }
        draw
    })
    .h(CONTROL - 4.)
    .cursor(Cursor::ResizeH)
    .focusable()
    .a11y(A11y::Slider {
        value: *value,
        min: lo,
        max: hi,
    })
    .named(name.to_owned())
    .tip(format!("{name}: drag, Shift for fine, double-click to reset"))
    .id(id.to_owned());
    let track_el = match width {
        Some(w) => track_el.w(w).shrink(0),
        None => track_el.flex(1).min_w(40),
    };
    let el = row![
        track_el,
        caption(readout(*value))
            .text_size(11)
            .reserve(kind.widest)
            .justify(Justify::End)
    ]
    .gap(GAP)
    .align(Align::Center)
    .when(width.is_some(), |e| e.shrink(0))
    .when(width.is_none(), |e| e.flex(1).min_w(0));
    (value.to_bits() != before.to_bits(), el)
}

/// Kontakt's pan readout: "C", "L 23", "R 40".
pub fn pan_text(pan: f64) -> String {
    let n = (pan.abs() * 100.).round();
    if n < 1. {
        "C".into()
    } else {
        format!("{} {n:.0}", if pan < 0. { "L" } else { "R" })
    }
}

pub fn db_text(db: f64) -> String {
    if db <= -59.95 {
        "-inf dB".into()
    } else {
        format!("{db:.1} dB")
    }
}

/// A flat notice with a colored edge. `role` is Warning or Danger.
pub fn banner(role: Role, text: impl Into<String>) -> El {
    row![
        block(2, Len::Pct(100.)).fill(role.alpha(1.)),
        body(text.into())
            .text_size(12)
            .fill(Color::oklch(0.86, 0., 0.))
            .lines(2)
            .flex(1)
            .min_w(0)
    ]
    .gap(GAP)
    .align(Align::Stretch)
    .pad((GAP + HALF, GAP))
    .fill(Role::Ink.alpha(0.04))
    .shrink(0)
}

/// `text` cut to `max` characters with an ellipsis, so a label never wraps or spills.
pub fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut + "…"
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i16 / 12 - 2)
}

/// A library folder name without the vendor noise.
pub fn library_label(name: &str) -> String {
    name.replace("Performance Samples ", "")
        .replace(" Library", "")
}

pub fn megabytes(bytes: usize) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.)
}

pub fn rect(x: f64, y: f64, w: f64, h: f64) -> DrawPath {
    DrawPath::polyline(
        [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(x, y)| Point::new(x, y)),
        true,
    )
}

pub fn circle(cx: f64, cy: f64, r: f64) -> DrawPath {
    arc(cx, cy, r, 0., 2. * PI)
}

pub fn arc(cx: f64, cy: f64, r: f64, from: f64, sweep: f64) -> DrawPath {
    let steps = ((sweep.abs() / (2. * PI)) * 48.).ceil().max(2.) as usize;
    let closed = sweep >= 2. * PI - 1e-9;
    DrawPath::polyline(
        (0..=steps).map(|n| {
            let a = from + sweep * n as f64 / steps as f64;
            Point::new(cx + a.cos() * r, cy + a.sin() * r)
        }),
        closed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_follow_kontakt() {
        // Kontakt calls MIDI 60 "C3".
        assert_eq!(note_name(60), "C3");
        assert_eq!(note_name(0), "C-2");
        assert_eq!(note_name(127), "G8");
    }

    #[test]
    fn readouts() {
        assert_eq!(pan_text(0.), "C");
        assert_eq!(pan_text(0.004), "C");
        assert_eq!(pan_text(-0.23), "L 23");
        assert_eq!(pan_text(0.4), "R 40");
        assert_eq!(db_text(-60.), "-inf dB");
        assert_eq!(db_text(-3.04), "-3.0 dB");
    }
}
