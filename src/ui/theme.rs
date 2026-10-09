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
/// Width of the native fader thumb, in logical points.
pub const FADER_THUMB: f64 = SPACE * 0.75;
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

/// The accent: focus and the current view. Light and achromatic, so the
/// only colors on screen are the parts' own, the library's and real errors.
pub fn accent() -> Color {
    Color::oklch(0.86, 0., 0.)
}

/// Part `slot`'s color, down its header's edge and over the keys it plays:
/// hues a golden angle apart, so no two parts look alike and neighbors
/// least of all, at one quiet lightness and chroma. The first is blue.
pub fn part_color(slot: usize) -> Color {
    Color::oklch(0.7, 0.1, golden_hue(250., slot))
}

/// Orange, amber and mustard: hues no color is drawn in.
pub const ORANGE: std::ops::Range<f32> = 40.0..105.0;

/// Whether `hue` (OKLCH degrees) falls in [`ORANGE`].
pub fn orange(hue: f32) -> bool {
    ORANGE.contains(&hue.rem_euclid(360.))
}

/// Step `n` of a walk a golden angle at a time from `from`, over the hue
/// circle with [`ORANGE`] cut out, so steps stay far apart and none is orange.
pub fn golden_hue(from: f32, n: usize) -> f32 {
    let arc = 360. - (ORANGE.end - ORANGE.start);
    let start = (from - ORANGE.end).rem_euclid(360.) * 360. / arc;
    let t = (f64::from(start) + n as f64 * 137.507_764) % 360.;
    (ORANGE.end + t as f32 * arc / 360.) % 360.
}

/// The hairline every boundary is drawn with.
pub fn hairline() -> Fill {
    Role::Ink.alpha(0.08)
}

/// Secondary text and glyphs: one solid grey, never ink faded by alpha, so
/// what is behind never shows through a word.
pub fn secondary() -> Fill {
    Fill::from(Color::oklch(0.72, 0., 0.))
}

/// A value's fill on a track or a knob: neutral, brighter than the track.
pub fn value_ink(lift: f32) -> Color {
    Color::oklch(0.78 + 0.1 * lift, 0., 0.)
}

/// The bundled face (OFL), variable in weight and width.
pub const NOTO_SANS: &[u8] = include_bytes!("../../assets/NotoSans.ttf");

pub fn ui() -> Ui {
    Ui::new(Theme {
        palette: Palette {
            neutral: Pigment::new(260.0, 0.008),
            // Yellow, not amber: no orange anywhere.
            warning: Pigment::new(105.0, 0.15),
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
        Font::new(NOTO_SANS)
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
        .fill(secondary())
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

/// A lift `steps` above the surface, opaque. Text grounds on its parent's
/// fill ignoring alpha, so a translucent ink wash under text would read as
/// a light ground and turn the text dark: anything text sits on is opaque.
pub fn lift(steps: i32) -> Fill {
    Role::Level(steps).into()
}

/// The states every flat control shares: the pointing hand, a faint lift on
/// hover, a deeper one while pressed, a 1 px accent ring for keyboard focus.
pub fn interactive(el: El, selected: bool) -> El {
    el.cursor(Cursor::Hand)
        .on(State::Hover, move |s| if selected { s } else { s.fill(lift(1)) })
        .on(State::Press, |s| s.fill(lift(2)))
        .on(State::FocusVisible, |s| s.stroke(Role::Primary.alpha(0.9)).stroke_width(1))
        .animate_with(quick())
}

/// A text button. `selected` raises it on a neutral surface and inks its text.
pub fn action(ui: &mut Ui, id: impl Into<Id>, label: &str, selected: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        body(label.to_owned())
            .text_size(TEXT)
            .fill(if selected { Fill::from(Role::Ink) } else { secondary() })
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
            .fill(if on { Fill::from(Role::Ink) } else { secondary() })
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
    Picture,
    Search,
    Filter,
    #[allow(dead_code, reason = "the mixer's audition")]
    Play,
    Menu,
    /// A favorite not yet set, and set.
    Star,
    StarFilled,
    /// Three keys of a piano: the computer keyboard playing notes.
    Keys,
    /// A clock face: what was opened lately.
    Recent,
    /// A five-pin DIN socket with an arrow in, or out: MIDI routing.
    MidiIn,
    #[allow(dead_code, reason = "the mixer's routing")]
    MidiOut,
    /// A waveform with an arrow in, or out: audio routing.
    #[allow(dead_code, reason = "the mixer's routing")]
    AudioIn,
    AudioOut,
    /// A tuning fork: tune.
    Fork,
    /// A tick: a set check box.
    Check,
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
            Icon::Picture => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5), (2.5, 3.5)]),
                line(&[(3., 11.5), (6.5, 7.5), (9., 10.), (11., 8.), (13., 11.5)]),
                dot(10.5, 5.5),
            ],
            Icon::Search => vec![
                Draw::stroke(
                    arc(ox + 7. * u, oy + 7. * u, 4. * u, 0., 2. * PI),
                    ink.clone(),
                    weight,
                ),
                line(&[(10., 10.), (13.5, 13.5)]),
            ],
            Icon::Filter => vec![line(&[(2.5, 3.5), (13.5, 3.5), (9.5, 8.), (9.5, 12.), (6.5, 13.), (6.5, 8.), (2.5, 3.5)])],
            Icon::Play => vec![Draw::fill(
                DrawPath::polyline([p(5., 3.5), p(12.5, 8.), p(5., 12.5)], true),
                ink.clone(),
            )],
            Icon::Menu => vec![
                line(&[(3., 4.5), (13., 4.5)]),
                line(&[(3., 8.), (13., 8.)]),
                line(&[(3., 11.5), (13., 11.5)]),
            ],
            Icon::Keys => {
                let black = |x: f64| Draw::fill(rect(ox + x * u, oy + 3.5 * u, 2.4 * u, 5. * u), ink.clone());
                vec![
                    line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5), (2.5, 3.5)]),
                    line(&[(6.2, 8.5), (6.2, 12.5)]),
                    line(&[(9.8, 8.5), (9.8, 12.5)]),
                    black(5.),
                    black(8.6),
                ]
            }
            Icon::MidiIn | Icon::MidiOut | Icon::AudioIn | Icon::AudioOut => {
                let pin = |x: f64, y: f64| {
                    Draw::fill(rect(ox + (x - 0.8) * u, oy + (y - 0.8) * u, 1.6 * u, 1.6 * u), ink.clone())
                };
                let mut draw = if matches!(icon, Icon::MidiIn | Icon::MidiOut) {
                    vec![
                        Draw::stroke(arc(ox + 6. * u, oy + 8. * u, 4.8 * u, 0., 2. * PI), ink.clone(), weight),
                        pin(3.7, 8.3),
                        pin(6., 5.8),
                        pin(8.3, 8.3),
                        line(&[(6., 12.8), (6., 11.)]),
                    ]
                } else {
                    vec![line(&[(1., 8.), (2.6, 4.), (4.2, 12.), (5.8, 3.), (7.4, 13.), (9., 6.), (10.2, 8.)])]
                };
                // In points at the socket; out points away from it.
                draw.push(if matches!(icon, Icon::MidiIn | Icon::AudioIn) {
                    line(&[(13.4, 6.), (11.6, 8.), (13.4, 10.), (11.6, 8.), (15.5, 8.)])
                } else {
                    line(&[(11.8, 8.), (15.4, 8.), (13.6, 6.), (15.4, 8.), (13.6, 10.)])
                });
                draw
            }
            Icon::Fork => vec![
                line(&[(5., 1.5), (5., 7.8), (5.7, 9.7), (8., 10.8), (10.3, 9.7), (11., 7.8), (11., 1.5)]),
                line(&[(8., 10.8), (8., 14.8)]),
            ],
            Icon::Check => vec![line(&[(3.5, 8.5), (6.5, 11.5), (12.5, 4.5)])],
            Icon::Recent => vec![
                Draw::stroke(arc(ox + 8. * u, oy + 8. * u, 5.5 * u, 0., 2. * PI), ink.clone(), weight),
                line(&[(8., 5.), (8., 8.), (10.5, 9.5)]),
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
    let ink = if on || hover > 0.5 { Fill::from(Role::Ink) } else { secondary() };
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

/// An [`icon_button`] that does nothing now: its glyph in a solid dark grey.
pub fn dead_icon(icon: Icon, name: &str) -> El {
    stack![glyph(icon, TEXT + TIGHT, Fill::from(Color::oklch(0.42, 0., 0.))).centered()]
        .square(CONTROL)
        .disabled()
        .named(name.to_owned())
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
            .fill(if current { Fill::from(Role::Ink) } else { secondary() })
            .lines(1),
        spacer(),
        block(Len::Pct(100.), 1).fill(underline)
    ]
    .gap(0)
    .align(Align::Center)
    .pad((SPACE, 0))
    .h(CONTROL + SPACE)
    .cursor(Cursor::Hand)
    .focusable()
    .a11y(A11y::Button)
    .named(label.to_owned())
    .on(State::FocusVisible, |s| s.fill(lift(1)))
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
        glyph(Icon::Down, TEXT, secondary())
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
    pub const LEVEL: Self = Self {
        origin: -60.,
        detent: Some(0.),
        reset: 0.,
        widest: "-60.0 dB",
        vertical: false,
        length: None,
    };

    /// A track `length` long.
    pub const fn length(self, length: f64) -> Self {
        Self {
            length: Some(length),
            ..self
        }
    }

}

thread_local! {
    /// Where the held scrollbar's thumb was grabbed, from its top: one
    /// pointer holds one bar at a time.
    static BAR_GRAB: std::cell::Cell<f64> = const { std::cell::Cell::new(0.) };
}

/// A thumb's top and length on a track `track` long, for a pane `view_h`
/// tall over `content_h` scrolled to `y`.
fn thumb(track: f64, y: f64, view_h: f64, content_h: f64) -> (f64, f64) {
    let len = (track * view_h / content_h).max(CONTROL).min(track);
    (((track - len) * (y / (content_h - view_h)).clamp(0., 1.)).round(), len)
}

/// A self-scrolled pane's bar under the hand: the thumb follows the
/// pointer from where it was grabbed, and a press on the track beside it
/// jumps the thumb's middle there, with no drag threshold to cross.
pub fn bar_drag(ui: &mut Ui, id: &str, y: &mut f64, view_h: f64, content_h: f64) {
    let r = ui.get(id);
    let (Some(at), true) = (ui.local(id), r.held && content_h > view_h && view_h > 0.) else { return };
    let track = ui.scene().and_then(|s| s.surface(id)).map_or(view_h, |s| s.frame.size.height);
    let (top, len) = thumb(track, *y, view_h, content_h);
    if r.pressed {
        BAR_GRAB.with(|g| g.set(if (top..top + len).contains(&at.y) { at.y - top } else { len / 2. }));
    }
    let span = (track - len).max(1.);
    *y = ((at.y - BAR_GRAB.with(std::cell::Cell::get)) / span * (content_h - view_h)).clamp(0., content_h - view_h);
}

/// Where a self-scrolled pane draws, gliding to `y`: on whole points, so
/// text and lines stay sharp. `snap` puts it there at once: under a held
/// bar, or across a jump too far to glide through; the glide unread for a
/// frame starts over from there.
pub fn glide(ui: &mut Ui, id: &str, y: f64, snap: bool) -> f64 {
    if snap { y.round() } else { ui.tween_with(format!("{id}-glide"), y, quick()).round() }
}

/// A jump from `from` to `to` too far to glide through: another list, Home, End.
pub fn leaps(from: f64, to: f64) -> bool {
    (to - from).abs() > 1500.
}

/// A self-scrolled pane's bar: a thin thumb that warms under the pointer.
pub fn scrollbar(ui: &mut Ui, id: &str, name: &str, y: f64, view_h: f64, content_h: f64) -> El {
    let r = ui.get(id);
    let lift = ui.state(id).hover.max(if r.held { 1. } else { 0. }) as f32;
    canvas(move |s| {
        let (at, len) = thumb(s.height, y, view_h, content_h);
        let w = 3. + 2. * f64::from(lift);
        vec![Draw::fill(rect(s.width - w - 1., at + 2., w, len - 4.), Role::Ink.alpha(0.18 + 0.3 * lift))]
    })
    .w(8)
    .h(Len::Pct(100.))
    .shrink(0)
    .a11y(A11y::Slider { value: y, min: 0., max: content_h - view_h })
    .named(name.to_owned())
    .id(id.to_owned())
}

/// The grab margin around a resizable edge, each side.
pub const EDGE_GRAB: f64 = 6.;

/// A resizable edge's mark: a 1 px accent line, 2 px held, at `at` along
/// the cross axis of a strip `s`, shown as the edge warms (`lift`, 0..1).
pub fn edge_mark(s: Size, at: f64, vertical_line: bool, lift: f32) -> Vec<Draw> {
    if lift <= 0.01 {
        return Vec::new();
    }
    let t = if lift > 0.9 { 2. } else { 1. };
    let ink = Fill::from(accent().with_alpha(0.35 + 0.55 * lift));
    vec![if vertical_line {
        Draw::fill(rect((at - t / 2.).round(), 0., t, s.height), ink)
    } else {
        Draw::fill(rect(0., (at - t / 2.).round(), s.width, t), ink)
    }]
}

/// How warm a resizable edge is: hovered, or 1 while held.
pub fn edge_lift(ui: &mut Ui, id: &str) -> f32 {
    let held = ui.get(id).held;
    ui.state(id).hover.max(if held { 1. } else { 0. }) as f32
}

thread_local! {
    /// A control turned by the wheel since [`wheel_taken`] last looked.
    static WHEELED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a control took the wheel since the last call: a list that
/// scrolls itself asks, so the wheel over a knob turns the knob alone.
pub fn wheel_taken() -> bool {
    WHEELED.with(|w| w.replace(false))
}

thread_local! {
    /// One pointer capture per window thread. The UI address distinguishes
    /// editors whose controls have the same IDs; it is never dereferenced.
    static GRIPPED: std::cell::RefCell<(usize, String, f64)> = const { std::cell::RefCell::new((0, String::new(), 0.)) };
}

/// `id` is being dragged: return whether this is its first drag frame.
fn grip(ui: &Ui, id: &str, value: f64) -> bool {
    let owner = std::ptr::from_ref(ui) as usize;
    GRIPPED.with(|g| {
        let mut g = g.borrow_mut();
        if g.0 != owner || g.1 != id {
            g.0 = owner;
            g.1.clear();
            g.1.push_str(id);
            g.2 = value;
            true
        } else {
            false
        }
    })
}

/// Whether the pointer hides now, as Kontakt's does: a knob, slider or
/// number is being dragged. The window brings it back where it hid.
pub fn pointer_hidden(ui: &Ui) -> bool {
    GRIPPED.with(|g| {
        let g = g.borrow();
        g.0 == std::ptr::from_ref(ui) as usize && !g.1.is_empty() && ui.get(g.1.as_str()).held
    })
}

/// MUI starts a drag after its click threshold, but its first delta excludes
/// earlier movement. Recover that movement once so returning to the press
/// position also returns to the starting value.
fn drag(ui: &Ui, id: &str, value: &mut f64, range: &RangeInclusive<f64>, travel: f64, vertical: bool) {
    let r = ui.get(id);
    if r.pressed {
        GRIPPED.with(|g| {
            let mut g = g.borrow_mut();
            if g.0 == std::ptr::from_ref(ui) as usize {
                g.1.clear();
            }
        });
    }
    let first = r.dragged && grip(ui, id, *value);
    if r.dragged {
        // Keep the grab in authored units. Integer readback must not erase
        // fractional motion, and a renderer without readback uses the same path.
        GRIPPED.with(|g| {
            let mut g = g.borrow_mut();
            let mut raw = g.2;
            if first && travel.is_finite() && travel > 0. {
                let missed = r.drag_total - r.drag_delta;
                let d = if vertical { -missed.y } else { missed.x };
                let fine = if r.mods.shift { FINE_DRAG } else { 1. };
                if d.is_finite() {
                    raw = (raw + d * fine / travel * (range.end() - range.start()))
                        .clamp(range.start().min(*range.end()), range.start().max(*range.end()));
                }
            }
            ui.drag(id, &mut raw, range.clone(), travel, vertical);
            g.2 = raw;
            *value = raw;
        });
    } else {
        ui.drag(id, value, range.clone(), travel, vertical);
    }
}

/// Pointer, wheel and keys on a continuous control `id`, as Kontakt's: drag
/// along `vertical` (up or right increases) across `travel` px, Shift for
/// fine; wheel and arrows step; double-click or Ctrl/Cmd-click resets.
pub fn drive(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    range: &RangeInclusive<f64>,
    travel: f64,
    vertical: bool,
    reset: f64,
) -> bool {
    drive_widget(ui,id,value,range,travel,vertical,reset,None,true)
}

#[allow(clippy::too_many_arguments)]
pub fn drive_widget(ui:&mut Ui,id:&str,value:&mut f64,range:&RangeInclusive<f64>,travel:f64,vertical:bool,reset:f64,step:Option<f64>,double_reset:bool)->bool {
    let (lo, hi) = (*range.start(), *range.end());
    let bound = |v:f64| v.clamp(lo.min(hi),lo.max(hi));
    let step = step.filter(|s|s.is_finite() && *s>0.);
    let r = ui.get(id);
    drag(ui, id, value, range, travel, vertical);
    if let Some(wheel) = ui.wheel(id) {
        WHEELED.with(|w| w.set(true));
        let notch = (hi-lo).abs()/if r.mods.shift {500.} else {50.};
        let step = step.map_or(notch,|quantum| (notch/quantum).round().max(1.)*quantum);
        let dir = if wheel.y.abs() >= wheel.x.abs() {
            -wheel.y
        } else {
            wheel.x
        };
        *value = bound(*value + dir.signum() * step);
    }
    if let Some(step) = step {
        for key in ui.keys(id) {
            *value = bound(match key.key {
                Key::Up|Key::Right => *value+step,
                Key::Down|Key::Left => *value-step,
                Key::PageUp => *value+step*10., Key::PageDown => *value-step*10.,
                Key::Home => lo, Key::End => hi, _=>continue,
            });
        }
    } else { stepped(ui, id, value, range); }
    if double_reset && r.double_clicked || r.pressed && (r.mods.ctrl || r.mods.cmd) {
        *value = bound(reset);
        GRIPPED.with(|g|g.borrow_mut().1.clear());
    }
    r.held
}

/// The shared KONTRA control face, fitted by its caller.
pub fn fader_face(at: f64, from: f64, detent: Option<f64>, vertical: bool, lift: f32, focused: bool) -> El {
    canvas(move |s| {
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
        let thumb_w = FADER_THUMB;
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
    let held = drive(ui, id, value, &range, track - FADER_THUMB, vertical, kind.reset);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let unit = |v: f64| ((v - lo) / (hi - lo)).clamp(0., 1.);
    let (at, from) = (unit(*value), unit(kind.origin));
    let detent = kind.detent.map(unit);
    let focused = ui.focus_visible(id);
    let track_el = fader_face(at, from, detent, vertical, lift, focused)
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

// Strip controls, after Koda's part strip: small, dense, each one readable
// at a glance. The mixer uses them too; keep their signatures.

/// A strip control's height: a hair under a [`CONTROL`].
pub const STRIP: f64 = CONTROL - TIGHT;
/// The pan wedge's width.
const WEDGE: f64 = TEXT * 3.5;

/// Signal and state colors: a lit activity dot and a meter's safe range,
/// its hot range, and clipping.
pub fn signal() -> Color {
    Color::oklch(0.8, 0.19, 145.)
}
fn hot() -> Color {
    Color::oklch(0.86, 0.16, 100.)
}
fn clip() -> Color {
    Color::oklch(0.64, 0.21, 27.)
}

/// A compact check box: a tick on the accent when set, an outline when
/// clear. Returns whether it was clicked.
pub fn check(ui: &mut Ui, id: impl Into<Id>, name: &str, on: bool) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let size = TEXT + 2.;
    let el = stack![glyph(Icon::Check, size, if on { Fill::from(Color::oklch(0.18, 0., 0.)) } else { Role::Ink.alpha(0.) })]
        .square(size)
        .fill(if on { Fill::from(accent()) } else { Role::Field.into() })
        .when(!on, |e| e.stroke(Role::Ink.alpha(0.3)).stroke_width(1))
        .focusable()
        .a11y(A11y::Toggle { on })
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id)
        .shrink(0);
    (hit, interactive(el, on))
}

/// Kontakt's pan, tight: "C", "L23", "R40".
pub fn pan_short(pan: f64) -> String {
    pan_text(pan).replace(' ', "")
}

/// Common wiring of a strip control: focus, cursor, a slider's name and value.
fn slider_el(el: El, id: &str, name: &str, value: f64, range: &RangeInclusive<f64>, tip: String) -> El {
    el.cursor(Cursor::ResizeV)
        .focusable()
        .a11y(A11y::Slider {
            value,
            min: *range.start(),
            max: *range.end(),
        })
        .named(name.to_owned())
        .tip(tip)
        .id(id.to_owned())
        .shrink(0)
}

/// Koda's pan: the value ("C", "L23") over a thin wedge that widens from
/// the middle to both ends, filled from the middle to a white tick at the
/// value. Drag up or down (Shift is fine), wheel, arrows; double-click centers.
pub fn pan_wedge(ui: &mut Ui, id: &str, pan: &mut f64) -> El {
    let range = -1.0..=1.0;
    let held = drive(ui, id, pan, &range, TRAVEL, true, 0.);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let focused = ui.focus_visible(id);
    let at = pan.clamp(-1., 1.);
    let wedge = canvas(move |s| {
        let (w, h) = (s.width, s.height);
        let c = w / 2.;
        let mid = h / 2.;
        let half = |x: f64| 0.5 + (mid - 0.5) * ((x - c).abs() / c).min(1.);
        let shape = |a: f64, b: f64| {
            DrawPath::polyline(
                [(a, mid - half(a)), (b, mid - half(b)), (b, mid + half(b)), (a, mid + half(a))]
                    .map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let track = Role::Ink.alpha(0.16 + 0.08 * lift);
        let x = c + at * c;
        let mut draw = vec![Draw::fill(shape(0., c), track.clone()), Draw::fill(shape(c, w), track)];
        if (x - c).abs() > 0.5 {
            draw.push(Draw::fill(shape(c.min(x), c.max(x)), value_ink(0.)));
        }
        let tick = (x - 0.75).clamp(0., w - 1.5);
        draw.push(Draw::fill(rect(tick, 0., 1.5, h), Color::oklch(0.98, 0., 0.)));
        if focused {
            draw.push(Draw::stroke(rect(0.5, 0.5, w - 1., h - 1.), Role::Primary.alpha(0.9), 1.));
        }
        draw
    })
    .w(WEDGE)
    .h(TIGHT * 2.)
    .shrink(0);
    let el = col![
        caption(pan_short(at)).text_size(SMALL - 1.).fill(secondary()).reserve("R100"),
        wedge
    ]
    .gap(2)
    .align(Align::Center)
    .justify(Justify::Center)
    .h(STRIP);
    slider_el(el, id, "Pan", at, &range, "Pan: drag, Shift for fine, double-click to center".into())
}

/// A small gain knob: a 270° track, an accent arc from 0 dB to the value,
/// and the level in dB beside it ("-6.0", "-inf"). -60 to +6 dB; drag up
/// or down, wheel, arrows; double-click returns to 0 dB.
pub fn gain_knob(ui: &mut Ui, id: &str, db: &mut f64) -> El {
    let range = -60.0..=6.0;
    let held = drive(ui, id, db, &range, TRAVEL, true, 0.);
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let focused = ui.focus_visible(id);
    let unit = |v: f64| ((v + 60.) / 66.).clamp(0., 1.);
    let (at, from) = (unit(*db), unit(0.));
    let knob = canvas(move |s| {
        let (cx, cy) = (s.width / 2., s.height / 2.);
        let weight = 2.;
        let r = s.width.min(s.height) / 2. - weight / 2. - 0.5;
        let (start, sweep) = (0.75 * PI, 1.5 * PI);
        let mut draw = vec![
            Draw::fill(circle(cx, cy, r - weight - 0.5), Role::Raised.alpha(1.)),
            Draw::stroke(arc(cx, cy, r, start, sweep), Role::Ink.alpha(0.24 + 0.08 * lift), weight),
        ];
        let (a, b) = if at < from { (at, from) } else { (from, at) };
        if b - a > 0.004 {
            draw.push(Draw::stroke(arc(cx, cy, r, start + sweep * a, sweep * (b - a)), accent(), weight));
        }
        let angle = start + sweep * at;
        let (inner, outer) = (r * 0.15, r - weight - 0.5);
        draw.push(Draw::stroke(
            DrawPath::polyline(
                [
                    Point::new(cx + angle.cos() * inner, cy + angle.sin() * inner),
                    Point::new(cx + angle.cos() * outer, cy + angle.sin() * outer),
                ],
                false,
            ),
            value_ink(lift),
            1.5,
        ));
        if focused {
            draw.push(Draw::stroke(circle(cx, cy, r + 1.5), Role::Primary.alpha(0.9), 1.));
        }
        draw
    })
    .square(STRIP)
    .shrink(0);
    let text = if *db <= -59.95 { "-inf".to_owned() } else { format!("{:.1}", *db + 0.) };
    let el = row![knob, caption(text).text_size(SMALL).reserve("-60.0")]
        .gap(TIGHT)
        .align(Align::Center);
    slider_el(el, id, "Volume", *db, &range, "Volume: drag up or down, Shift for fine, double-click for 0 dB".into())
}

/// A tuning fork and the tune in semitones ("0.00", "-1.25"): drag up or
/// down (Shift is fine), wheel, arrows; double-click returns to 0. Cents
/// are the finest step.
pub fn tune_field(ui: &mut Ui, id: &str, semitones: &mut f64, range: RangeInclusive<f64>) -> El {
    let held = drive(ui, id, semitones, &range, TRAVEL * 2., true, 0.);
    *semitones = (*semitones * 100.).round() / 100.;
    let lift = ui.state(id).hover.max(if held { 1. } else { 0. }) as f32;
    let text = if semitones.abs() < 0.005 { "0.00".to_owned() } else { format!("{:+.2}", *semitones) };
    let el = row![
        glyph(Icon::Fork, TEXT, if lift > 0.5 { Fill::from(Role::Ink) } else { secondary() }),
        caption(text).text_size(SMALL).reserve("+36.00")
    ]
    .gap(2)
    .align(Align::Center)
    .h(STRIP)
    .pad((2, 0))
    .when(ui.focus_visible(id), |e| e.stroke(Role::Primary.alpha(0.9)).stroke_width(1));
    slider_el(el, id, "Tune", *semitones, &range, "Tune in semitones: drag, Shift for fine, double-click for 0".into())
}

/// Where a meter's level sits on its scale: -60 dB at the foot, +6 at the top.
fn meter_unit(level: f32) -> f64 {
    if level <= 0. {
        return 0.;
    }
    ((20. * f64::from(level).log10() + 60.) / 66.).clamp(0., 1.)
}

/// A thin vertical stereo meter. `level` is read each time the meter is
/// laid out, so feeding it an atomic moves the meter on any frame without
/// rebuilding what is around it. Linear peaks, drawn on a dB scale: signal
/// green, hot above -6 dB, red past 0.
pub fn meter_v(level: impl Fn() -> [f32; 2] + 'static) -> El {
    canvas(move |s| {
        let bar = ((s.width - 1.) / 2.).floor().max(1.);
        let mut draw = vec![
            Draw::fill(rect(0., 0., bar, s.height), Role::Ink.alpha(0.16)),
            Draw::fill(rect(bar + 1., 0., bar, s.height), Role::Ink.alpha(0.16)),
        ];
        let y = |u: f64| s.height * (1. - u);
        let (hot_at, clip_at) = (54. / 66., 60. / 66.);
        for (n, level) in level().into_iter().enumerate() {
            let top = meter_unit(level);
            let x = n as f64 * (bar + 1.);
            for (from, to, color) in [(0., hot_at, signal()), (hot_at, clip_at, hot()), (clip_at, 1., clip())] {
                if top > from {
                    let to = top.min(to);
                    draw.push(Draw::fill(rect(x, y(to), bar, y(from) - y(to)), color));
                }
            }
        }
        draw
    })
    .w(5)
    .h(Len::Pct(100.))
    .shrink(0)
    .named("Level")
}

/// A small dot, lit green while `lit` says the part is sounding; read at
/// layout like [`meter_v`].
pub fn activity_dot(lit: impl Fn() -> bool + 'static) -> El {
    canvas(move |s| {
        let fill = if lit() { Fill::from(signal()) } else { Role::Ink.alpha(0.16) };
        vec![Draw::fill(circle(s.width / 2., s.height / 2., s.width.min(s.height) / 2.), fill)]
    })
    .square(TIGHT * 1.75)
    .shrink(0)
    .named("Activity")
}

/// Solo and mute as a tight pair, `solo-{key}` and `mute-{key}`: set solo
/// fills amber, set mute blue, both with dark ink.
pub fn solo_mute(ui: &mut Ui, key: &str, solo: &mut bool, mute: &mut bool) -> El {
    let one = |ui: &mut Ui, id: String, letter: &str, name: &str, on: &mut bool, lit: Color| {
        if ui.get(id.as_str()).activated() {
            *on = !*on;
        }
        let el = row![
            caption(letter.to_owned())
                .text_size(SMALL)
                .text_weight(Weight::SEMIBOLD)
                .fill(if *on { Fill::from(Color::oklch(0.18, 0., 0.)) } else { secondary() })
        ]
        .align(Align::Center)
        .justify(Justify::Center)
        .square(STRIP)
        .fill(if *on { Fill::from(lit) } else { Role::Field.into() })
        .focusable()
        .a11y(A11y::Toggle { on: *on })
        .named(name.to_owned())
        .tip(name.to_owned())
        .id(id)
        .shrink(0);
        interactive(el, *on)
    };
    let s = one(ui, format!("solo-{key}"), "S", "Solo", solo, accent());
    let m = one(ui, format!("mute-{key}"), "M", "Mute", mute, Color::oklch(0.72, 0.12, 240.));
    row![s, m].gap(1).align(Align::Center).shrink(0)
}

/// A compact routing menu: an icon (MIDI or audio, in or out), the current
/// choice kept `widest` wide, and a caret. Returns whether it was clicked.
pub fn route(ui: &mut Ui, id: impl Into<Id>, icon: Icon, text: &str, widest: &str, name: &str) -> (bool, El) {
    let id: Id = id.into();
    let hit = ui.get(id.clone()).activated();
    let el = row![
        glyph(icon, TEXT + 2., secondary()),
        caption(text.to_owned()).text_size(SMALL).lines(1).reserve(widest.to_owned()),
        glyph(Icon::Down, TIGHT * 2.5, secondary())
    ]
    .gap(3)
    .align(Align::Center)
    .pad((TIGHT, 0))
    .h(STRIP)
    .focusable()
    .a11y(A11y::Button)
    .named(format!("{name}: {text}"))
    .tip(name.to_owned())
    .id(id)
    .shrink(0);
    (hit, interactive(el, false))
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

/// A notice: ink text on an opaque raised ground, and a 2 px edge in
/// `role` (Warning or Danger) that carries the severity. The color never
/// reaches the text, so it reads on any theme.
pub fn banner(role: Role, text: impl Into<String>) -> El {
    row![
        block(2, Len::Pct(100.)).fill(role),
        body(text.into())
            .text_size(TEXT)
            .fill(Role::Ink)
            .lines(3)
            .flex(1)
            .min_w(0)
    ]
    .gap(SPACE)
    .align(Align::Stretch)
    .pad(edges(SPACE, INSET, SPACE, 0.))
    .fill(Role::Raised)
    .shrink(0)
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i16 / 12 - 2)
}

/// `name` without its library's name in front: "Vista - Harp" in
/// "Performance Samples Vista" is "Harp". Only a name set off by a dash,
/// colon or underscore goes; "Una Corda Pure" stays whole.
pub fn without_library<'a>(name: &'a str, library: &str) -> &'a str {
    let library = library_label(library);
    let library = library.split(" [").next().unwrap_or_default().trim();
    let rest = (name.get(..library.len()))
        .filter(|head| !library.is_empty() && head.eq_ignore_ascii_case(library))
        .map(|_| &name[library.len()..]);
    rest.and_then(|rest| {
        let shorn = rest.trim_start_matches([' ', '-', '–', '—', ':', '_']);
        let set_off = rest.len() - shorn.len() > rest.len() - rest.trim_start().len();
        (set_off && !shorn.is_empty()).then_some(shorn)
    })
    .unwrap_or(name)
}

/// A library folder name without the vendor noise.
pub fn library_label(name: &str) -> String {
    crate::library::label(name)
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

/// The shared KONTRA control face, fitted by its caller.
pub fn dial_face(at: f64, from: f64, lift: f32, focused: bool) -> El {
    canvas(move |s| {
        let (cx, cy) = (s.width / 2., s.height / 2.);
        let weight = TIGHT * 0.75;
        let r = s.width.min(s.height) / 2. - weight;
        let (start, sweep) = (0.75 * PI, 1.5 * PI);
        let mut draw = vec![
            Draw::fill(circle(cx, cy, r - weight * 1.5), Role::Raised.alpha(1.)),
            Draw::stroke(arc(cx, cy, r, start, sweep), Role::Ink.alpha(0.14 + 0.06 * lift), weight),
        ];
        let (a, b) = if at < from { (at, from) } else { (from, at) };
        if b - a > 0.002 {
            draw.push(Draw::stroke(arc(cx, cy, r, start + sweep * a, sweep * (b - a)), value_ink(0.), weight));
        }
        let angle = start + sweep * at;
        let (inner, outer) = (r * 0.2, r - weight * 2.);
        draw.push(Draw::stroke(
            DrawPath::polyline(
                [Point::new(cx + angle.cos() * inner, cy + angle.sin() * inner), Point::new(cx + angle.cos() * outer, cy + angle.sin() * outer)],
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_leave_their_library_out() {
        assert_eq!(without_library("Vista - Harp", "Performance Samples Vista"), "Harp");
        assert_eq!(without_library("Solo_Violin", "Solo"), "Violin");
        assert_eq!(without_library("Una Corda Pure", "Una Corda Library"), "Una Corda Pure");
        assert_eq!(without_library("Vistas", "Vista"), "Vistas");
        assert_eq!(without_library("Vista - ", "Vista"), "Vista - ");
        assert_eq!(without_library("Harp", ""), "Harp");
    }

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
        assert_eq!(pan_short(-0.23), "L23");
        assert_eq!(pan_short(0.), "C");
        assert_eq!(meter_unit(0.), 0.);
        assert_eq!(meter_unit(1.), 60. / 66.);
        assert_eq!(meter_unit(1e-4), 0., "-80 dB sits at the foot");
    }
}

/// Kontakt's authored KEY_COLOR indices. Source colours are preserved, including warm hues.
pub fn ksp_key_color(index: u8) -> Option<Color> {
    const COLORS: [[u8; 3]; 21] = [
        [220, 55, 64], [240, 116, 37], [255, 168, 85], [245, 195, 65], [242, 224, 72],
        [177, 213, 66], [71, 178, 85], [89, 208, 160], [74, 198, 225], [43, 164, 180],
        [76, 121, 219], [134, 97, 165], [138, 99, 220], [174, 79, 204], [209, 75, 171], [231, 80, 125],
        [180, 180, 180], [90, 90, 90], [180, 180, 180], [235, 235, 235], [35, 35, 35],
    ];
    if matches!(index, 16 | 18) { return None; }
    let [r, g, b] = *COLORS.get(index as usize)?;
    Some(Color::srgb(f32::from(r) / 255., f32::from(g) / 255., f32::from(b) / 255.))
}

// Port from v1 0cb7a8a0:src/ui/theme.rs.
pub fn megabytes(bytes: usize) -> String {
    format!("{:.0} MB", bytes as f64 / 1_048_576.)
}
