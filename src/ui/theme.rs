//! The look and the parts every view is built from.
//!
//! One text size sets the scale: spacing and control sizes derive from it,
//! and every strip, panel and field sizes itself to its content. Graphite
//! neutrals, square edges, hairline separators, no shadows. The accent marks
//! focus and the current view with a 1 px line; it never fills a surface or
//! colors text.

use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use moose::mui::mui::{layout::Insets, scene::TypeScale};
use std::f64::consts::PI;
use std::ops::RangeInclusive;

/// Body text size. Everything below derives from it.
pub const TEXT: f64 = 12.;
/// Captions, readouts and section titles.
pub const SMALL: f64 = TEXT - 1.;
/// Between the parts of one control: a label and its value, a track and its readout.
pub const TIGHT: f64 = TEXT / 3.;
/// Between sibling controls.
pub const SPACE: f64 = TEXT * 2. / 3.;
/// Around a panel's content, and between panels' sections.
pub const INSET: f64 = TEXT;
/// A control's height: one line of text with room to aim at.
pub const CONTROL: f64 = TEXT * 2.;
/// A knob's diameter.
pub const KNOB: f64 = CONTROL * 1.5;
/// Pointer travel for a knob or a vertical fader's full span.
pub const TRAVEL: f64 = KNOB * 5.;

/// The browser's width when first shown, and how far it resizes, in lines
/// of text it shows.
pub const SIDEBAR: f64 = TEXT * 23.;
pub const SIDEBAR_MIN: f64 = TEXT * 18.;
pub const SIDEBAR_MAX: f64 = TEXT * 36.;

/// Padding per side, in CSS order.
pub fn edges(top: f64, right: f64, bottom: f64, left: f64) -> Insets {
    Insets {
        left,
        right,
        top,
        bottom,
    }
}

/// The accent's hue and chroma (OKLCH): focus, the current view, played keys.
const ACCENT_HUE: f32 = 64.0;
const ACCENT_CHROMA: f32 = 0.15;

pub fn accent() -> Color {
    Color::oklch(0.76, ACCENT_CHROMA, ACCENT_HUE)
}

/// The hairline every boundary is drawn with.
pub fn hairline() -> Fill {
    Role::Ink.alpha(0.08)
}

/// A value's fill on a track or a knob: neutral, brighter than the track.
fn value_ink(lift: f32) -> Color {
    Color::oklch(0.78 + 0.1 * lift, 0., 0.)
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
        // Hardware-panel edges: square everywhere.
        corners: Corners {
            field: 0.,
            box_: 0.,
            selector: 0.,
            concave: 0.,
        },
        text: TEXT + 1.,
        control: 3.,
        type_scale: TypeScale {
            title: TEXT * 5. / 3.,
            body: TEXT + 1.,
            caption: SMALL,
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

// Layout parts.

/// A strip of controls: a title bar, a toolbar, a header row. Its height is
/// its tallest control plus a tight inset.
pub fn strip(items: Vec<El>) -> El {
    row(items)
        .gap(SPACE)
        .align(Align::Center)
        .pad((SPACE, TIGHT))
        .shrink(0)
}

/// Controls that belong together, closer than their neighbours.
pub fn cluster(items: Vec<El>) -> El {
    row(items).gap(TIGHT).align(Align::Center).shrink(0)
}

/// A horizontal hairline.
pub fn rule() -> El {
    block(Len::Pct(100.), 1).fill(hairline()).shrink(0)
}

/// A vertical hairline.
pub fn vrule() -> El {
    block(1, Len::Pct(100.)).fill(hairline()).shrink(0)
}

/// A small uppercase title.
pub fn section(label: &str) -> El {
    caption(label.to_uppercase())
        .text_size(SMALL - 1.)
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
        .gap(TIGHT)
        .align(Align::Center)
        .pad(edges(TIGHT, TIGHT, TIGHT, INSET))
        .shrink(0)
}

/// A dim label beside an ink value that keeps its width as digits change.
pub fn stat(label: &str, value: String, widest: &str) -> El {
    row![
        section(label),
        caption(value).text_size(TEXT).reserve(widest.to_owned())
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .shrink(0)
}

// Controls.

/// The states every flat control shares: a faint lift on hover, a deeper one
/// while pressed, a 1 px accent ring for keyboard focus.
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

/// A text button. `selected` raises it on a neutral surface and inks its text.
pub fn action(ui: &mut Ui, id: impl Into<Id>, label: &str, selected: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        body(label.to_owned())
            .text_size(TEXT)
            .fill(if selected { Role::Ink } else { Role::Dim })
            .lines(1)
            .min_w(0)
    ]
    .align(Align::Center)
    .justify(Justify::Center)
    .pad((SPACE, 0))
    .h(CONTROL)
    .when(selected, |e| e.fill(Role::Raised))
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .id(id);
    (hit, interactive(el, selected))
}

/// A switch with a word on it. Set, it sits raised with ink text over a 1 px
/// accent line; clear, it is a dim word on a faint field.
pub fn latch(ui: &mut Ui, id: impl Into<Id>, label: &str, name: &str, on: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = col![
        spacer(),
        body(label.to_owned())
            .text_size(SMALL)
            .text_weight(Weight::SEMIBOLD)
            .fill(if on { Role::Ink } else { Role::Dim })
            .lines(1)
            .min_w(0),
        spacer(),
        block(Len::Pct(100.), 1).fill(if on {
            Fill::from(accent())
        } else {
            Role::Ink.alpha(0.)
        })
    ]
    .gap(0)
    .align(Align::Center)
    .pad((SPACE, 0))
    .min_w(CONTROL)
    .h(CONTROL)
    .fill(if on { Role::Raised } else { Role::Field })
    .focusable()
    .a11y(A11y::Toggle { on })
    .named(name.to_owned())
    .tip(name.to_owned())
    .id(id);
    (hit, interactive(el, on))
}

/// Switches that read as one control: a segmented row with hairline seams.
pub fn segmented(items: Vec<El>) -> El {
    row(items).gap(1).align(Align::Stretch).fill(hairline()).shrink(0)
}

/// Line icons drawn on a 16-unit square, crisp at any size, no icon font.
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
    /// A favorite not yet set, and set.
    Star,
    StarFilled,
}

/// `icon` in `ink`, `size` points square.
pub fn glyph(icon: Icon, size: f64, ink: Fill) -> El {
    canvas(move |s| {
        let u = s.width.min(s.height) / 16.;
        let (ox, oy) = ((s.width - 16. * u) / 2., (s.height - 16. * u) / 2.);
        let p = |x: f64, y: f64| Point::new(ox + x * u, oy + y * u);
        let weight = 1.5 * u.max(0.75);
        let line = |pts: &[(f64, f64)]| {
            Draw::stroke(
                DrawPath::polyline(pts.iter().map(|&(x, y)| p(x, y)), false),
                ink.clone(),
                weight,
            )
        };
        let dot = |x: f64, y: f64| {
            Draw::fill(
                rect(ox + (x - 1.) * u, oy + (y - 1.) * u, 2. * u, 2. * u),
                ink.clone(),
            )
        };
        match icon {
            Icon::Left => vec![line(&[(10., 4.), (6., 8.), (10., 12.)])],
            Icon::Right => vec![line(&[(6., 4.), (10., 8.), (6., 12.)])],
            Icon::Up => vec![line(&[(4., 10.), (8., 6.), (12., 10.)])],
            Icon::Down => vec![line(&[(4., 6.), (8., 10.), (12., 6.)])],
            Icon::More => vec![dot(3.5, 8.), dot(8., 8.), dot(12.5, 8.)],
            Icon::Plus => vec![
                line(&[(8., 3.5), (8., 12.5)]),
                line(&[(3.5, 8.), (12.5, 8.)]),
            ],
            Icon::Close => vec![
                line(&[(4.5, 4.5), (11.5, 11.5)]),
                line(&[(11.5, 4.5), (4.5, 11.5)]),
            ],
            Icon::Sidebar => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5), (2.5, 3.5)]),
                line(&[(6.5, 3.5), (6.5, 12.5)]),
            ],
            Icon::Search => vec![
                Draw::stroke(
                    arc(ox + 7. * u, oy + 7. * u, 4. * u, 0., 2. * PI),
                    ink.clone(),
                    weight,
                ),
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
            Icon::Star | Icon::StarFilled => {
                // Five points round (8, 8.6), alternating outer and inner radii.
                let points = (0..10).map(|n| {
                    let r = if n % 2 == 0 { 6. } else { 2.6 };
                    let a = -PI / 2. + f64::from(n) * PI / 5.;
                    p(8. + r * a.cos(), 8.6 + r * a.sin())
                });
                let star = DrawPath::polyline(points, true);
                vec![if icon == Icon::StarFilled {
                    Draw::fill(star, ink.clone())
                } else {
                    Draw::stroke(star, ink.clone(), weight * 0.8)
                }]
            }
        }
    })
    .square(size)
    .shrink(0)
}

/// A square icon button. `on` holds it raised, like a set switch.
pub fn icon_button(ui: &mut Ui, id: impl Into<Id>, icon: Icon, name: &str, on: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let hover = ui.state(id.clone()).hover as f32;
    let ink = if on {
        Role::Ink.alpha(1.)
    } else {
        Role::Ink.alpha(0.6 + 0.4 * hover)
    };
    let el = stack![glyph(icon, TEXT + TIGHT, ink).centered()]
        .square(CONTROL)
        .when(on, |e| e.fill(Role::Raised))
        .focusable()
        .a11y(A11y::Button)
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id);
    (hit, interactive(el, on))
}

/// A view switch: ink when current with a 1 px accent underline, dim otherwise.
pub fn tab(ui: &mut Ui, id: impl Into<Id>, label: &str, current: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let hover = ui.state(id.clone()).hover as f32;
    let underline = if current {
        Fill::from(accent())
    } else {
        Role::Ink.alpha(0.2 * hover)
    };
    let el = col![
        spacer(),
        body(label.to_owned())
            .text_size(TEXT)
            .fill(if current { Role::Ink } else { Role::Dim })
            .lines(1),
        spacer(),
        block(Len::Pct(100.), 1).fill(underline)
    ]
    .gap(0)
    .align(Align::Center)
    .pad((SPACE, 0))
    .h(CONTROL + SPACE)
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .on(State::FocusVisible, |s| s.fill(Role::Ink.alpha(0.06)))
    .id(id)
    .shrink(0);
    (hit, el)
}

/// A field that opens a list: the current choice and a caret.
pub fn dropdown(ui: &mut Ui, id: impl Into<Id>, text: &str, name: &str) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        body(text.to_owned())
            .text_size(SMALL)
            .lines(1)
            .flex(1)
            .min_w(0),
        glyph(Icon::Down, TEXT, Role::Dim.alpha(1.))
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .pad((SPACE, 0))
    .h(CONTROL)
    .min_w(CONTROL * 3.)
    .fill(Role::Field)
    .focusable()
    .a11y(A11y::Button)
    .named(format!("{name}: {text}"))
    .tip(name.to_owned())
    .id(id);
    (hit, interactive(el, false))
}

/// A labelled number to drag, type or step. Boxes share one width, so
/// they line up down a column.
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
        caption(label).fill(Role::Dim).lines(1).flex(1).min_w(0),
        c.el.value_text(display)
            .el()
            .h(CONTROL)
            .reserve(reserve)
            .min_w(CONTROL * 2.)
            .shrink(0)
    ]
    .gap(SPACE)
    .align(Align::Center)
    .shrink(0)
}

/// How a slider maps, marks and reads its value.
#[derive(Clone, Copy, PartialEq)]
pub struct Fader {
    /// Where the fill starts: the low end for a level, the middle for pan.
    pub origin: f64,
    /// A marked value on the track: center for pan, unity for a level.
    pub detent: Option<f64>,
    /// What a double-click restores.
    pub reset: f64,
    /// The widest readout, so the track keeps its length as the value changes.
    pub widest: &'static str,
    /// Up and down rather than sideways.
    pub vertical: bool,
    /// A preferred track length, shrinking when crowded; `None` takes the
    /// room it is given.
    pub length: Option<f64>,
}

impl Fader {
    pub const PAN: Self = Self {
        origin: 0.,
        detent: Some(0.),
        reset: 0.,
        widest: "R 100",
        vertical: false,
        length: None,
    };
    pub const LEVEL: Self = Self {
        origin: -60.,
        detent: Some(0.),
        reset: 0.,
        widest: "-60.0 dB",
        vertical: false,
        length: None,
    };

    /// A plain slider over `range`, filled from its low end.
    pub const fn over(range: &RangeInclusive<f64>, reset: f64, widest: &'static str) -> Self {
        Self {
            origin: *range.start(),
            detent: None,
            reset,
            widest,
            vertical: false,
            length: None,
        }
    }

    /// The same, bipolar: filled from and marked at the range's middle.
    pub fn bipolar(range: &RangeInclusive<f64>, reset: f64, widest: &'static str) -> Self {
        let mid = (range.start() + range.end()) / 2.;
        Self {
            origin: mid,
            detent: Some(mid),
            ..Self::over(range, reset, widest)
        }
    }

    /// A track `length` long.
    pub const fn length(self, length: f64) -> Self {
        Self {
            length: Some(length),
            ..self
        }
    }

    /// Up and down.
    pub const fn vertical(self) -> Self {
        Self {
            vertical: true,
            ..self
        }
    }
}

/// Pointer, wheel and keys on a continuous control `id`: drag across
/// `travel` px (Shift is fine), wheel and arrows step, double-click resets.
fn drive(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    range: &RangeInclusive<f64>,
    travel: f64,
    vertical: bool,
    reset: f64,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let r = ui.get(id);
    ui.drag(id, value, range.clone(), travel, vertical);
    if let Some(wheel) = ui.wheel(id) {
        let step = (hi - lo) / if r.mods.shift { 500. } else { 50. };
        let dir = if wheel.y.abs() >= wheel.x.abs() {
            -wheel.y
        } else {
            wheel.x
        };
        *value = (*value + dir.signum() * step).clamp(lo, hi);
    }
    stepped(ui, id, value, range);
    if r.double_clicked {
        *value = reset;
    }
    r.held
}

/// A Koda-style slider: a thin track with its detent marked, a neutral fill
/// from the origin to the value, a small square thumb, and a readout.
pub fn fader(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    kind: Fader,
    readout: impl Fn(f64) -> String,
) -> (bool, El) {
    let (lo, hi) = (*range.start(), *range.end());
    let before = *value;
    let (vertical, length) = (kind.vertical, kind.length);
    let track = ui
        .scene()
        .and_then(|s| s.surface(id))
        .map_or(TRAVEL, |s| {
            if vertical {
                s.frame.size.height
            } else {
                s.frame.size.width
            }
        })
        .max(CONTROL);
    let held = drive(ui, id, value, &range, track - SPACE, vertical, kind.reset);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let unit = |v: f64| ((v - lo) / (hi - lo)).clamp(0., 1.);
    let (at, from) = (unit(*value), unit(kind.origin));
    let detent = kind.detent.map(unit);
    let focused = ui.focus_visible(id);
    let track_el = canvas(move |s| {
        // Drawn along x; a vertical fader swaps the axes and runs bottom-up.
        let (len, across) = if vertical {
            (s.height, s.width)
        } else {
            (s.width, s.height)
        };
        let place = |along: f64, off: f64, l: f64, t: f64| {
            if vertical {
                rect(off, len - along - l, t, l)
            } else {
                rect(along, off, l, t)
            }
        };
        let thumb_w = SPACE * 0.75;
        let inner = len - thumb_w;
        let x = |u: f64| thumb_w / 2. + u * inner;
        let mid = (across / 2.).round();
        let mut draw = vec![Draw::fill(
            place(thumb_w / 2., mid - 1., inner, 2.),
            Role::Ink.alpha(0.14 + 0.06 * lift),
        )];
        let (a, b) = if x(at) < x(from) {
            (x(at), x(from))
        } else {
            (x(from), x(at))
        };
        if b - a > 0.5 {
            draw.push(Draw::fill(place(a, mid - 1., b - a, 2.), value_ink(0.)));
        }
        if let Some(d) = detent {
            draw.push(Draw::fill(
                place(x(d).round() - 0.5, mid - TIGHT - 1., 1., 2. * TIGHT + 2.),
                Role::Ink.alpha(0.4),
            ));
        }
        draw.push(Draw::fill(
            place(
                (x(at) - thumb_w / 2.).round(),
                mid - TIGHT - 1.,
                thumb_w,
                2. * TIGHT + 2.,
            ),
            value_ink(lift),
        ));
        if focused {
            draw.push(Draw::stroke(
                rect(0.5, 0.5, s.width - 1., s.height - 1.),
                Role::Primary.alpha(0.9),
                1.,
            ));
        }
        draw
    })
    .cursor(if vertical {
        Cursor::ResizeV
    } else {
        Cursor::ResizeH
    })
    .focusable()
    .a11y(A11y::Slider {
        value: *value,
        min: lo,
        max: hi,
    })
    .named(name.to_owned())
    .tip(format!("{name}: drag, Shift for fine, double-click to reset"))
    .id(id.to_owned());
    let text = caption(readout(*value))
        .text_size(SMALL)
        .reserve(kind.widest);
    let el = if vertical {
        let track_el = track_el.w(CONTROL).h(length.unwrap_or(TRAVEL / 2.)).shrink(0);
        col![text, track_el].gap(TIGHT).align(Align::Center).shrink(0)
    } else {
        let track_el = match length {
            // A preferred length that gives way in a crowded row.
            Some(w) => track_el.w(w).min_w(CONTROL * 2.),
            None => track_el.flex(1).min_w(CONTROL * 2.),
        }
        .h(CONTROL - TIGHT);
        row![track_el, text.justify(Justify::End)]
            .gap(SPACE)
            .align(Align::Center)
            .when(length.is_none(), |e| e.flex(1))
            .min_w(0)
    };
    (value.to_bits() != before.to_bits(), el)
}

/// A flat knob: a 270° track, a neutral arc from the origin to the value and
/// a pointer. Drags vertically; wheel, arrows, double-click as a fader.
pub fn dial(
    ui: &mut Ui,
    id: &str,
    name: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
    kind: Fader,
) -> (bool, El) {
    let (lo, hi) = (*range.start(), *range.end());
    let before = *value;
    let held = drive(ui, id, value, &range, TRAVEL, true, kind.reset);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let unit = |v: f64| ((v - lo) / (hi - lo)).clamp(0., 1.);
    let (at, from) = (unit(*value), unit(kind.origin));
    let focused = ui.focus_visible(id);
    let el = canvas(move |s| {
        let (cx, cy) = (s.width / 2., s.height / 2.);
        let weight = TIGHT * 0.75;
        let r = s.width.min(s.height) / 2. - weight;
        let (start, sweep) = (0.75 * PI, 1.5 * PI);
        let mut draw = vec![
            Draw::fill(circle(cx, cy, r - weight * 1.5), Role::Raised.alpha(1.)),
            Draw::stroke(
                arc(cx, cy, r, start, sweep),
                Role::Ink.alpha(0.14 + 0.06 * lift),
                weight,
            ),
        ];
        let (a, b) = if at < from { (at, from) } else { (from, at) };
        if b - a > 0.002 {
            draw.push(Draw::stroke(
                arc(cx, cy, r, start + sweep * a, sweep * (b - a)),
                value_ink(0.),
                weight,
            ));
        }
        let angle = start + sweep * at;
        let (inner, outer) = (r * 0.2, r - weight * 2.);
        draw.push(Draw::stroke(
            DrawPath::polyline(
                [
                    Point::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
                    Point::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
                ],
                false,
            ),
            value_ink(lift),
            weight,
        ));
        if focused {
            draw.push(Draw::stroke(circle(cx, cy, r + weight), Role::Primary.alpha(0.9), 1.));
        }
        draw
    })
    .square(KNOB)
    .shrink(0)
    .cursor(Cursor::ResizeV)
    .focusable()
    .a11y(A11y::Slider {
        value: *value,
        min: lo,
        max: hi,
    })
    .named(name.to_owned())
    .tip(format!("{name}: drag up or down, Shift for fine, double-click to reset"))
    .id(id.to_owned());
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

/// Semitones, signed, with cents when there are any: "0 st", "+3 st", "-1.25 st".
pub fn tune_text(semitones: f64) -> String {
    let cents = (semitones * 100.).round();
    if cents == 0. {
        "0 st".into()
    } else if cents % 100. == 0. {
        format!("{:+.0} st", cents / 100.)
    } else {
        format!("{:+.2} st", cents / 100.)
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
            .text_size(TEXT)
            .fill(Role::Ink)
            .lines(2)
            .flex(1)
            .min_w(0)
    ]
    .gap(SPACE)
    .align(Align::Stretch)
    .pad((INSET, SPACE))
    .fill(Role::Ink.alpha(0.04))
    .shrink(0)
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
        assert_eq!(tune_text(0.001), "0 st");
        assert_eq!(tune_text(-12.), "-12 st");
        assert_eq!(tune_text(1.25), "+1.25 st");
    }
}
