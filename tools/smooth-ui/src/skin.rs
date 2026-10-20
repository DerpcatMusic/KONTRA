//! SMOOTH's look: the palette, and the parts every view is built from.
//!
//! This file is the only one that turns a token into a control. A view names
//! a part and passes it a value; the part decides how tall a pill is, how
//! wide a plate is, how far a ring sits from its name. Nothing here takes a
//! size at the call site, so a control looks the same wherever it lands and
//! resizes with the tokens rather than with the caller's guess.
//!
//! `SKIN` is what MUI resolves its own defaults against -- radii, text roles,
//! the unit a control scales by. The sRGB tokens under it are SMOOTH's own
//! surfaces, which no palette derivation reaches from a seed, so they are
//! stated once, here, and nowhere else.

use crate::theme;
use crate::tokens::{icon, metric, radius, space, stroke, ty};
use mui::prelude::*;
use mui::scene::{SpacingScale, TypeScale};

// ------------------------------------------------------------------ layout --
/// The editor's logical size. Everything below is measured against it.
pub const W: f64 = 800.;
pub const H: f64 = 473.;

/// The control column on the left, the side rail beside it, and the rest.
pub const PANEL: f64 = 154.;
pub const RAIL: f64 = 27.;
/// Where the plot starts: past the control column and the rail.
pub const LEFT: f64 = PANEL + RAIL;
pub const RIGHT: f64 = W - PANEL - RAIL;
/// Where the toolbar and the footer begin. They start a little left of the
/// plot, so their first control tucks under the rail -- as the reference
/// does.
pub const TOOLBAR_X: f64 = 172.;
pub const FOOTER_X: f64 = 163.;

/// The right column: a toolbar over the graph over a footer.
pub const BAR: f64 = 26.;
pub const FOOTER: f64 = 30.;
pub const GRAPH: f64 = H - BAR - FOOTER;

/// What the editor calls itself.
pub const WORDMARK: &str = "smooth";
pub const MAKER: &str = "derpcat";
pub const VERSION: &str = "1.0.0";

// ------------------------------------------------------------------ theme --
/// The theme MUI resolves against: SMOOTH's radii, its text roles, and the
/// spacing scale the DSL's `Xs`..`Xl` tokens read.
///
/// The numbers come from the token scales, so the theme and the parts cannot
/// disagree about what "one step" means.
pub const SKIN: Theme = Theme {
    corners: Corners {
        // Small things read as pills; fields and plates barely round.
        selector: radius::SELECTOR,
        field: radius::FIELD,
        box_: radius::BOX,
        concave: radius::CONCAVE,
    },
    type_scale: TypeScale {
        title: ty::TITLE,
        body: ty::BODY,
        caption: ty::CAPTION,
    },
    text: ty::BODY,
    control: space::TIGHT,
    spacing: SpacingScale {
        unit: space::TIGHT,
        xs: space::XS,
        s: space::S,
        m: space::M,
        l: space::L,
        xl: space::XL,
    },
    ..Theme::DEFAULT
};

// ------------------------------------------------------------------- ink ---
/// A colour token, from the sRGB literal the design was drawn in.
pub fn c(hex: u32) -> Color {
    Color::srgb(
        ((hex >> 16) & 255) as f32 / 255.0,
        ((hex >> 8) & 255) as f32 / 255.0,
        (hex & 255) as f32 / 255.0,
    )
}
/// The same, faded.
pub fn ca(hex: u32, alpha: f32) -> Color {
    c(hex).with_alpha(alpha)
}

// ------------------------------------------------------------------ type ---
/// A label in SMOOTH's face: a name, set medium.
pub fn label(s: &str, size: f64, ink: Color) -> El {
    text(s)
        .text_size(size)
        .fill(ink)
        .text_axis("wght", ty::WEIGHT)
        .lines(1)
}

/// A value, set lighter than the label that names it.
pub fn readout(s: &str, size: f64, ink: Color) -> El {
    text(s)
        .text_size(size)
        .fill(ink)
        .text_axis("wght", ty::WEIGHT_VALUE)
        .lines(1)
}

/// The product's name, set heavier and rounder.
pub fn wordmark(s: &str, size: f64, ink: Color) -> El {
    text(s)
        .text_size(size)
        .fill(ink)
        .text_axis("wght", ty::WEIGHT_WORDMARK)
        .text_axis("ROND", ty::ROND)
        .lines(1)
}

/// A caption: the quiet ink, one size down.
pub fn caption(s: &str, ink: Color) -> El {
    label(s, ty::CAPTION, ink)
}

/// A label centred in a box of a stated size, so a row of them lines up.
pub fn centred(s: El, w: f64, h: f64) -> El {
    stack![s].size(w, h).center()
}

// ----------------------------------------------------------------- plates --
/// A value's plate: the field a readout sits in, at the given width.
fn field_at(w: f64) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    block(w, metric::FIELD_H)
        .radius(SKIN.corners.field)
        .fill(c(t.field))
        .shrink(0)
}

/// A value's plate at the footer's field width: the field a row of readouts
/// shares so they line up whatever they happen to say.
pub fn field() -> El {
    field_at(metric::FIELD_W)
}

fn plate_at(s: &str, w: f64) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    field_at(w).push(centred(readout(s, ty::BODY, c(t.ink)), w, metric::FIELD_H))
}

/// A footer plate: as wide as a signed reading (`0.0 dB`) so two of them line
/// up in a row whatever they happen to say.
pub fn plate(s: &str) -> El {
    plate_at(s, metric::FIELD_W)
}

/// A dial's reading: the compact plate under a knob, as narrow as the value
/// it states, because a dial is already its own column.
pub fn reading(s: &str) -> El {
    plate_at(s, metric::READING_W)
}

/// A pill: a row of things sharing one rounded plate at one height.
///
/// Intrinsic: the width is whatever the row needs, the radius is always half
/// the height, so a pill is a pill at any content.
fn pill(items: Vec<El>, fill: Color) -> El {
    row(items)
        .gap(space::XS)
        .pad((space::S, space::NONE))
        .h(metric::PILL)
        .radius(metric::PILL / 2.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .shrink(0)
        .fill(fill)
}

/// A word in a hairline box: a chip that does not carry a fill.
pub fn outline_chip(s: &str) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    centred(
        label(s, ty::FINE, c(t.ink)),
        metric::BADGE_W,
        metric::BADGE_H,
    )
    .radius(SKIN.corners.field)
    .stroke(ca(t.ink, t.wash / 2.0))
    .no_fill()
    .shrink(0)
}

/// A footer switch: a hollow ring and a name, on the footer's own plate.
///
/// The footer sits on the darker ground, so a wash over it would read as a
/// smudge rather than a plate; the reference gives these switches the same
/// opaque plate the value fields use.
pub fn toggle_pill(ui: &mut Ui, id: &str, name: &str, on: &mut bool) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    if ui.get(id).activated() {
        *on = !*on;
    }
    pill(
        vec![
            ring_dot(metric::RING, if *on { c(t.mark) } else { c(t.ink) }),
            label(name, ty::CAPTION, c(t.ink)),
        ],
        c(t.field),
    )
    .id(id)
    .a11y(A11y::Toggle { on: *on })
}

/// A round plate under a glyph: a wash light enough to work on the bar, the
/// footer, or the plot.
pub fn disc(d: f64) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    block(d, d).radius(d / 2.0).fill(ca(t.plate, t.wash))
}

/// A hollow circle, as a switch reads before it is on.
pub fn ring_dot(d: f64, ink: Color) -> El {
    canvas(move |s| {
        let r = s.width.min(s.height) / 2.0 - stroke::RULE / 2.0;
        vec![Draw::stroke(
            disc_path(s.width / 2.0, s.height / 2.0, r),
            ink,
            stroke::RULE,
        )]
    })
    .size(d, d)
    .shrink(0)
}

/// A round button with a glyph on it: the toolbar's shape.
pub fn icon_button(icon: Icon) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let d = metric::BUTTON;
    disc(d).push(centred(glyph(icon, icon::LARGE, c(t.ink)), d, d))
}

/// A round button that carries a word or a mark instead of a glyph.
pub fn text_button(s: &str) -> El {
    text_button_at(s, metric::BUTTON, ty::LABEL)
}

/// The reference sets the one-letter button a size up and a pixel smaller
/// than its neighbours.
pub fn mark_button(s: &str) -> El {
    text_button_at(s, metric::BUTTON_SMALL, ty::TITLE)
}

/// A round button with a word on it, at a stated rung.
fn text_button_at(s: &str, d: f64, size: f64) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    disc(d).push(centred(label(s, size, c(t.ink)), d, d))
}

/// Two letters, one pill: the chosen one carries a dark disc.
///
/// A/B is a choice between exactly two things, so the part takes the two
/// names and the choice, and never a width.
pub fn pair_pill(left: &str, right: &str, chosen: usize) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let dot = metric::DOT;
    let side = |s: &str, on: bool| -> El {
        if on {
            block(dot, dot)
                .radius(dot / 2.0)
                .fill(c(t.selected))
                .push(centred(label(s, ty::CAPTION, c(t.on_selected)), dot, dot))
                .shrink(0)
        } else {
            centred(label(s, ty::CAPTION, c(t.ink)), dot, dot).shrink(0)
        }
    };
    row![side(left, chosen == 0), side(right, chosen == 1)]
        .gap(space::XS)
        .pad(space::TIGHT)
        .w(metric::PAIR_W)
        .h(metric::PILL)
        .radius(metric::PILL / 2.0)
        .align(Align::Center)
        .justify(Justify::Center)
        .shrink(0)
        .fill(c(t.chip))
}

/// A pill that runs from one side to the other: the compare control.
pub fn compare_pill(from: &str, to: &str) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    pill(
        vec![
            label(from, ty::CAPTION, c(t.ink)),
            glyph(Icon::Arrow, icon::BASE, c(t.ink)),
            label(to, ty::CAPTION, c(t.ink)),
        ],
        c(t.chip),
    )
    .w(metric::COMPARE_W)
}

/// A field whose value is picked from a list.
///
/// Intrinsic behaviour, not just looks: the part owns the cycling, so a view
/// asks for a list of names and a place to keep the choice.
pub fn dropdown(ui: &mut Ui, id: &str, options: &[&str], choice: &mut usize) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    if ui.get(id).clicked {
        *choice = (*choice + 1) % options.len().max(1);
    }
    let name = options.get(*choice).copied().unwrap_or("");
    block(metric::DROPDOWN_W, metric::FIELD_H)
        .radius(SKIN.corners.field)
        .fill(c(t.field))
        .push(
            stack![
                label(name, ty::BODY, c(t.ink))
                    .anchor(Align::Start, Align::Center)
                    .offset(space::L, space::NONE),
                glyph(Icon::Triangle, icon::CARET, c(t.ink))
                    .anchor(Align::End, Align::Center)
                    .offset(-space::M, space::NONE),
            ]
            .full(),
        )
        .id(id)
        .a11y(A11y::Toggle { on: *choice > 0 })
}

/// The current preset, its pager, and what it is for.
///
/// One part, because it is one thing: the reference draws the title, the
/// pager and the tags on a single plate at one height.
pub fn preset_bar(title: &str, tag: &str) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    row![
        row![
            glyph(Icon::Left, icon::CARET, c(t.ink)),
            glyph(Icon::Right, icon::CARET, c(t.ink)),
        ]
        .gap(space::L),
        label(title, ty::LABEL, c(t.ink)).grow(1.0),
        row![
            glyph(Icon::Heart, icon::SMALL, c(t.heart)),
            glyph(Icon::Person, icon::BASE, c(t.ink)),
            outline_chip(tag),
        ]
        .gap(space::S),
    ]
    .gap(space::M)
    .pad((space::S, space::NONE))
    .size(metric::PRESET_W, metric::PRESET_H)
    .align(Align::Center)
    .radius(metric::PRESET_H / 2.0)
    .shrink(0)
    .fill(c(t.chip))
}

// -------------------------------------------------------------- geometry ---
/// A point `deg` clockwise from twelve o'clock, at `r` from `(cx, cy)`.
pub fn pt(cx: f64, cy: f64, r: f64, deg: f64) -> Point {
    let a = deg.to_radians();
    Point::new(cx + r * a.sin(), cy - r * a.cos())
}

/// A closed ring, drawn as a polyline at one segment every three degrees.
fn ring(cx: f64, cy: f64, ri: f64, ro: f64) -> Path {
    const N: usize = 96;
    let at = |i: usize| 360.0 * i as f64 / N as f64;
    let mut pts: Vec<Point> = (0..=N).map(|i| pt(cx, cy, ro, at(i))).collect();
    pts.extend((0..=N).rev().map(|i| pt(cx, cy, ri, at(i))));
    Path::polyline(pts, true)
}

/// A ring's sector, from `from` to `to`, both clockwise from twelve o'clock.
fn sector(cx: f64, cy: f64, ri: f64, ro: f64, from: f64, to: f64) -> Path {
    let steps = (((to - from).abs() / 3.0).ceil() as usize).max(2);
    let at = |i: usize| from + (to - from) * i as f64 / steps as f64;
    let mut pts: Vec<Point> = (0..=steps).map(|i| pt(cx, cy, ro, at(i))).collect();
    pts.extend((0..=steps).rev().map(|i| pt(cx, cy, ri, at(i))));
    Path::polyline(pts, true)
}

/// A disc, as four cubics.
pub fn disc_path(cx: f64, cy: f64, r: f64) -> Path {
    let k = 0.552_284_749_8 * r;
    Path::default()
        .move_to(Point::new(cx, cy - r))
        .cubic_to(
            Point::new(cx + k, cy - r),
            Point::new(cx + r, cy - k),
            Point::new(cx + r, cy),
        )
        .cubic_to(
            Point::new(cx + r, cy + k),
            Point::new(cx + k, cy + r),
            Point::new(cx, cy + r),
        )
        .cubic_to(
            Point::new(cx - k, cy + r),
            Point::new(cx - r, cy + k),
            Point::new(cx - r, cy),
        )
        .cubic_to(
            Point::new(cx - r, cy - k),
            Point::new(cx - k, cy - r),
            Point::new(cx, cy - r),
        )
        .close()
}

/// An open segment.
pub fn line(a: Point, b: Point) -> Path {
    Path::polyline([a, b], false)
}

// ------------------------------------------------------------------ knob ---
/// The ring's thickness, and the seam that keeps the body off it.
const RING: f64 = 6.5;
const SEAM: f64 = 1.5;

/// Where the value arc starts, clockwise from twelve o'clock, and how far a
/// full turn of the range reaches.
const ARC_START: f64 = 228.0;
const ARC_SWEEP: f64 = 259.0;

/// A knob: a ring with an arc for `value`, and a flat body inside it.
///
/// Everything derives from `d`, the outer diameter -- the ring, the body, the
/// arc, and the padding that lets the shadow breathe. One call, one size, and
/// it drags: up and down, with fine steps under `Shift`.
pub fn knob(
    ui: &Ui,
    id: impl Into<Id> + Clone,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    d: f64,
) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    let id = id.into();
    ui.drag(id.clone(), value, range.clone(), metric::DIAL * 2.5, true);
    let state = ui.state(id.clone());
    let lift = (state.hover.max(state.press)) as f32;
    let v = ((*value - range.start()) / (range.end() - range.start())).clamp(0.0, 1.0);
    let body = d - 2.0 * RING - SEAM;
    let mid = (d - RING) / 2.0;
    let (ri, ro) = (mid - RING / 2.0, mid + RING / 2.0);
    let end = ARC_START + ARC_SWEEP * v;
    let focus = ui.focus_visible(id.clone());
    let halo = d + metric::TUCK - space::HAIR;

    canvas(move |s| {
        let (cx, cy) = (s.width / 2.0, s.height / 2.0);
        // The ring is lit from the upper right, so its ramp runs from the top
        // right corner of the box down to the bottom left one.
        let ramp = (ro / (4.0 * s.width / 2.0)) as f32;
        let mut draws = vec![
            Draw::fill(
                disc_path(cx + space::HAIR, cy + space::TIGHT + space::HAIR, d / 2.0),
                ca(t.shade, t.shade_alpha),
            ),
            Draw::fill(
                ring(cx, cy, ri, ro),
                Gradient::linear(
                    225.0,
                    [(0.5 - ramp, c(t.ring_dark)), (0.5 + ramp, c(t.ring_light))],
                ),
            ),
            Draw::fill(
                sector(cx, cy, ri, ro, ARC_START, end),
                ca(t.mark, 1.0 - 0.10 * lift),
            ),
            Draw::fill(disc_path(cx, cy, body / 2.0), c(t.body)),
        ];
        if lift > 0.0 {
            // Under the pointer the body lifts a shade; nothing moves.
            draws.push(Draw::fill(
                disc_path(cx, cy, body / 2.0),
                ca(t.mark, 0.05 * lift),
            ));
        }
        if focus {
            draws.push(Draw::stroke(
                disc_path(cx, cy, mid + space::HAIR + space::HAIR / 2.0),
                c(t.ink),
                stroke::HAIR,
            ));
        }
        draws
    })
    .size(halo, halo)
    .shrink(0)
    .id(id.clone())
    .focusable()
    .cursor(Cursor::Grab)
    .tip(format!("{id}: drag up or down, Shift for fine"))
    .a11y(A11y::Slider {
        value: *value,
        min: *range.start(),
        max: *range.end(),
    })
}

/// One knob and its readout, stacked: the editor's whole control unit.
///
/// The name goes above the dial, the value in a plate below it, and the three
/// travel together wherever the row or column puts them.
pub fn control(
    ui: &mut Ui,
    dial: &mut Dial,
    id: &str,
    name: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    d: f64,
) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    // The dial runs first: it may write a value the host just moved, and the
    // readout under it has to agree with the arc above it.
    let dial_el = dial(ui, id, d, value, range);
    let readout = format!("{:.1}", *value);
    let halo = d + metric::TUCK - space::HAIR;
    col![
        caption(name, c(t.ink)),
        // The readout tucks under the dial: one box, anchored top and bottom,
        // so the two never drift apart and no gap has to be hand-tuned.
        stack![
            dial_el.anchor(Align::Center, Align::Start),
            reading(&readout).anchor(Align::Center, Align::End),
        ]
        .size(halo, halo - metric::TUCK + metric::FIELD_H)
        .shrink(0),
    ]
    .gap(space::TIGHT)
    .align(Align::Center)
    .shrink(0)
}

/// How a dial reaches its value.
///
/// Outside a host the value is a field on the model and [`knob`] edits it
/// directly. Inside a plug-in the same dial is bound to a host parameter, so
/// the same call site records automation instead of moving a local number.
/// The view does not know which: it names the control and the range, and the
/// dial decides where the value lives.
pub type Dial<'a> =
    dyn FnMut(&mut Ui, &str, f64, &mut f64, std::ops::RangeInclusive<f64>) -> El + 'a;

/// The dial a preview uses: the knob, on the model.
pub fn model_dial() -> impl FnMut(&mut Ui, &str, f64, &mut f64, std::ops::RangeInclusive<f64>) -> El
{
    |ui, id, d, value, range| knob(ui, id, value, range, d)
}

/// A segmented switch: one plate, one choice, and the chosen segment carries
/// the dark pill.
///
/// It is a real control: a click or a key turns it over, and the state it
/// carries is the caller's, so the same switch works in a preview and in a
/// host. It takes any number of names, not just two.
pub fn segmented(ui: &mut Ui, id: &str, options: &[&str], chosen: &mut usize) -> El {
    // The active palette: the parts read tokens, never literals.
    let t = theme::t();
    if ui.get(id).activated() {
        *chosen = (*chosen + 1) % options.len().max(1);
    }
    let h = metric::SWITCH_H;
    let inner = h - space::TIGHT - space::TIGHT;
    let n = options.len().max(1) as f64;
    let half = (metric::SWITCH_W - space::TIGHT * (n + 1.0)) / n;
    let segments: Vec<El> = options
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let on = index == *chosen;
            stack![centred(
                label(name, ty::BODY, if on { c(t.on_selected) } else { c(t.ink) }),
                half,
                inner,
            )]
            .size(half, inner)
            .radius(inner / 2.0)
            .fill(if on { c(t.selected) } else { c(t.toggle) })
        })
        .collect();
    row(segments)
        .gap(space::TIGHT)
        .pad(space::TIGHT)
        .size(metric::SWITCH_W, h)
        .radius(h / 2.0)
        .fill(c(t.toggle))
        .shrink(0)
        .id(id)
        .a11y(A11y::Toggle { on: *chosen > 0 })
}

// ---------------------------------------------------------------- glyphs ---
/// The marks SMOOTH draws, one per Phosphor outline in `icons`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Left,
    Right,
    Down,
    Triangle,
    Arrow,
    Undo,
    Redo,
    Gear,
    Expand,
    Headphones,
    Heart,
    Person,
    Lock,
    Globe,
    Quality,
    Search,
    Speaker,
}

impl Icon {
    /// The outline, in Phosphor's own 256-unit space.
    fn data(self) -> &'static str {
        match self {
            Icon::Left => crate::icons::CARET_LEFT,
            Icon::Right => crate::icons::CARET_RIGHT,
            Icon::Down => crate::icons::CARET_DOWN,
            Icon::Triangle => crate::icons::TRIANGLE,
            Icon::Arrow => crate::icons::ARROW_RIGHT,
            Icon::Undo => crate::icons::UNDO,
            Icon::Redo => crate::icons::REDO,
            Icon::Gear => crate::icons::GEAR,
            Icon::Expand => crate::icons::EXPAND,
            Icon::Headphones => crate::icons::HEADPHONES,
            Icon::Heart => crate::icons::HEART,
            Icon::Person => crate::icons::USER,
            Icon::Lock => crate::icons::LOCK,
            Icon::Globe => crate::icons::GLOBE,
            Icon::Quality => crate::icons::WAVEFORM,
            Icon::Search => crate::icons::SEARCH,
            Icon::Speaker => crate::icons::SPEAKER,
        }
    }
}

/// A glyph at `size`, inked with `ink`.
///
/// The outline is flattened in its own 256-unit space and scaled into the
/// box here, so one call gives a crisp mark at any size without a font, a
/// codepoint table, or a baseline to correct for.
pub fn glyph(icon: Icon, size: f64, ink: Color) -> El {
    canvas(move |s| {
        let k = s.width.min(s.height) / 256.0;
        let src = Path::from_svg_data(icon.data()).unwrap_or_default();
        let mut out = Path::default();
        for contour in src.flatten(0.5, 100_000).unwrap_or_default() {
            let mut it = contour.into_iter();
            let Some(first) = it.next() else { continue };
            out = out.move_to(Point::new(first.x * k, first.y * k));
            for p in it {
                out = out.line_to(Point::new(p.x * k, p.y * k));
            }
            out = out.close();
        }
        vec![Draw::fill(out, ink)]
    })
    .size(size, size)
    .shrink(0)
}
