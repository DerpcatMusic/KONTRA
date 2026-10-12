//! Controls as compositions. Nothing here is placed absolutely: a thumb sits
//! where two flex weights put it, a knob's pointer is an anchored offset.
use std::ops::RangeInclusive;
use std::sync::Arc;

use mui_input::{FINE_DRAG, Key};
use mui_material::prelude::*;
use mui_scene::{Palette, Px, SpacingToken, Spring, Stroke};

use crate::Ui;
use crate::widgets::{Response, TextOpts, text_edit};

/// How solid a control looks. daisyUI's four button styles, resolved from
/// the role and the palette rather than from a table of colours.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let quiet = button(&mut ui, "bypass", "Bypass").el;
/// // Ink only: the resting box paints nothing.
/// assert_eq!(quiet.variant(Variant::Ghost).el().payload().style.fill, None);
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Variant {
    /// The role, filled. The one live action on a panel.
    #[default]
    Solid,
    /// The role at a low alpha: the same colour, mixed into whatever it sits
    /// on, with the role as its ink.
    Soft,
    /// The role as a stroke and as ink; nothing filled.
    Outline,
    /// Ink only, until the pointer arrives.
    Ghost,
}

/// A control's look, resolved: the pixel height it was sized to, the
/// palette the variant does its arithmetic with, and what the readout was
/// told to say.
struct Look {
    px: f64,
    variant: Variant,
    role: Role,
    palette: Palette,
    text: Option<String>,
}

/// A keyboard activation is the same primary action as a click. `Ui::keys`
/// is already gated to the focused surface, so widgets do not need to invent
/// a second focus test or consume another key stream.
fn activated(ui: &Ui, id: &str) -> bool {
    ui.get(id).activated()
}
impl Look {
    /// What the control's own box is painted with, where the role is the
    /// fill: a button, a toggle that is on.
    fn style(&self) -> Style {
        self.face(self.role)
    }
    /// The same, for a control whose resting box is *not* the role -- a
    /// knob's face, a slider's track: `Solid` leaves it alone.
    fn face(&self, resting: Role) -> Style {
        match self.variant {
            Variant::Solid => Style::default().fill(resting),
            // A faded role *is* the mix with the ground: it composites over
            // whatever is under it, so it tracks the panel it lands on.
            Variant::Soft => Style::default().fill(self.role.alpha(0.18)),
            Variant::Outline => Style {
                stroke: Some(Stroke {
                    fill: Some(self.role.into()),
                    width: None,
                }),
                ..Style::default()
            },
            Variant::Ghost => Style::default(),
        }
    }
    /// The ink that reads on that box: `Palette::on` computes the contrast
    /// for a filled one, and the role speaks for itself on the rest.
    fn ink(&self) -> Fill {
        match self.variant {
            Variant::Solid => Role::Ink.into(),
            _ => self.role.into(),
        }
    }
    /// The vertical padding that centres one line of 14-unit text in the
    /// control's height.
    fn pad_y(&self) -> f64 {
        ((self.px - 14.0) / 2.0).max(2.0)
    }
    /// The state closure that lifts whatever the control is filled with, so
    /// a hover is the palette's own step and not a second colour.
    fn hover(&self) -> impl Fn(Style) -> Style + use<> {
        let (p, role, variant) = (self.palette, self.role, self.variant);
        move |s: Style| {
            let ground = p.surface();
            match variant {
                // Nothing to lift: the hover is the role, arriving.
                Variant::Outline | Variant::Ghost => s.fill(role.alpha(0.12)),
                _ => {
                    let lifted = s
                        .fill
                        .as_ref()
                        .unwrap_or(&Fill::None)
                        .map(&p, ground, |c| p.hover(c));
                    s.fill(lifted)
                }
            }
        }
    }
}

/// A control the widget has taken its value from, with what is left to say
/// about how it looks: the variant, the size, and the pixels as an escape
/// hatch. Finish it with [`Control::el`], or drop it straight into a
/// `row![..]`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut cutoff = 0.5;
/// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0).el;
/// let strip = row![dial.size(L), body("post")];
/// ```
pub struct Control {
    look: Look,
    size: SpacingToken,
    px: Option<f64>,
    kind: Kind,
}

/// Which control, with what it read from the runtime: the tree waits for
/// the look, which the caller can still change. One label allocation,
/// shared by the text and the accessible name.
enum Kind {
    Slider(Id, Arc<str>, Dial),
    Knob(Id, Arc<str>, Dial),
    Button(Id, Arc<str>),
    Toggle(Id, Arc<str>, bool),
    Drag(Id, Arc<str>, Dial),
    /// A [`drag_value`] being typed into: its field, at the width it had.
    Field(Box<El>, f64),
}

/// A ranged control's reading: the value and its range, where the drawn
/// thumb or pointer sits (`t`, 0..1, tweened) and how hovered it is.
#[derive(Clone, Copy)]
struct Dial {
    value: f64,
    min: f64,
    max: f64,
    t: f64,
    hover: f64,
}
impl Dial {
    /// Drag `value` over `travel` (vertically for a dial), step it by the
    /// focused keys, and read it back with its thumb tweened toward it.
    fn read(
        ui: &mut Ui,
        id: &Id,
        value: &mut f64,
        range: &RangeInclusive<f64>,
        travel: f64,
        vertical: bool,
    ) -> Self {
        ui.drag(id, value, range.clone(), travel, vertical);
        stepped(ui, id, value, range);
        // Fast enough that a drag still feels direct, slow enough that a
        // preset change is a glide. The value and readout stay exact.
        let t = ui.tween_with(id, unit(*value, range), Spring::new(0.12, 1.0));
        Self::of(*value, range, t, ui.state(id).hover)
    }
    fn of(value: f64, range: &RangeInclusive<f64>, t: f64, hover: f64) -> Self {
        let (min, max) = (*range.start(), *range.end());
        Dial {
            value,
            min,
            max,
            t,
            hover,
        }
    }
    fn a11y(&self) -> A11y {
        A11y::Slider {
            value: self.value,
            min: self.min,
            max: self.max,
        }
    }
    fn readout(&self, look: &Look) -> El {
        readout(look.text.clone(), self.value, self.min, self.max)
    }
}

impl Control {
    fn new(ui: &Ui, kind: Kind) -> Self {
        Self {
            look: Look {
                px: ui.theme().control,
                variant: Variant::Solid,
                role: Role::Primary,
                palette: ui.theme().palette,
                text: None,
            },
            size: SpacingToken::M,
            px: None,
            kind,
        }
    }
    /// How solid this control looks.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut on = false;
    /// let sw = toggle(&mut ui, "bypass", "Bypass", &mut on).el;
    /// let sw = sw.variant(Variant::Outline);
    /// assert!(sw.el().payload().style.stroke.is_some(), "outlined");
    /// ```
    pub fn variant(mut self, v: Variant) -> Self {
        self.look.variant = v;
        self
    }
    /// The palette role this control speaks with: `Primary` unless it is a
    /// destructive action.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let clear = button(&mut ui, "clear", "Clear").el;
    /// let el = clear.role(Role::Danger).variant(Variant::Outline).el();
    /// let ring = el.payload().style.stroke.clone().and_then(|s| s.fill);
    /// assert_eq!(ring, Some(Fill::Role(Role::Danger)));
    /// ```
    pub fn role(mut self, r: Role) -> Self {
        self.look.role = r;
        self
    }
    /// One of the five sizes, the same five everywhere: `Xs` through `Xl`
    /// multiply the theme's [`control`](mui_scene::Theme::control) unit.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut on = false;
    /// let small = toggle(&mut ui, "bypass", "Bypass", &mut on).size(Xs);
    /// ```
    pub fn size(mut self, s: SpacingToken) -> Self {
        self.size = s;
        self
    }
    /// What the readout says, instead of the default two decimals: the
    /// value in the units the parameter actually has. A knob has no header,
    /// so it says this under the dial in place of its label.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut cutoff = 440.0;
    /// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 20.0..=20_000.0).el;
    /// let dial = dial.value_text(format!("{cutoff:.0} Hz"));
    /// ```
    pub fn value_text(mut self, s: impl Into<String>) -> Self {
        self.look.text = Some(s.into());
        self
    }
    /// The control's height in pixels, for the knob that has to match a
    /// hardware panel. Overrides [`Control::size`].
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let mut v = 0.5;
    /// let dial = knob(&mut ui, "cut", "Cutoff", &mut v, 0.0..=1.0).px(37);
    /// ```
    pub fn px(mut self, px: impl Px) -> Self {
        self.px = Some(px.px());
        self
    }
    /// The tree, finished.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let el: El = button(&mut ui, "go", "Go").el.el();
    /// ```
    pub fn el(mut self) -> El {
        // Ten units at M, two either side: daisyUI's -xs .. -xl scale, so one
        // `Theme.control` rescales every control in the tree.
        let steps = match self.size {
            SpacingToken::Xs => 6.0,
            SpacingToken::S => 8.0,
            SpacingToken::M => 10.0,
            SpacingToken::L => 12.0,
            SpacingToken::Xl => 14.0,
        };
        self.look.px = self.px.unwrap_or(self.look.px * steps);
        let look = &self.look;
        match self.kind {
            Kind::Slider(id, label, d) => slider_el(look, id, label, d),
            Kind::Knob(id, label, d) => knob_el(look, id, label, d),
            Kind::Button(id, label) => button_el(look, id, label),
            Kind::Toggle(id, label, on) => toggle_el(look, id, label, on),
            Kind::Drag(id, label, d) => drag_el(look, id, label, d),
            Kind::Field(el, width) => el.w(width),
        }
    }
}
impl From<Control> for El {
    fn from(c: Control) -> Self {
        c.el()
    }
}
impl IntoEl for Control {
    fn into_el(self) -> El {
        self.el()
    }
}

/// One arrow-key step of a slider or knob across `range`: a hundredth of it,
/// signed with the range, so an inverted control still steps toward its end.
/// A platform's `Increment` and `Decrement` take the same step.
pub(crate) fn step(range: &RangeInclusive<f64>) -> f64 {
    (range.end() - range.start()) / 100.0
}

/// Step `value` by the focused control's keys: arrows by a hundredth of
/// `range` (a tenth of that with Shift), Page Up and Down by ten steps, Home
/// and End to the ends. The runtime brackets the frame as one edit. Returns
/// whether the value moved.
///
/// The keyboard half of [`slider`] and [`knob`], for a control of your own:
///
/// ```
/// use mui::prelude::*;
/// let ui = Ui::default();
/// let mut trim = 0.0;
/// assert!(!stepped(&ui, "trim", &mut trim, &(-12.0..=12.0)), "nothing is focused");
/// ```
pub fn stepped(ui: &Ui, id: &str, value: &mut f64, range: &RangeInclusive<f64>) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    if !(lo.is_finite() && hi.is_finite()) {
        return false;
    }
    let before = *value;
    for k in ui.keys(id) {
        let one = step(range) * if k.mods.shift { FINE_DRAG } else { 1.0 };
        *value = match k.key {
            Key::Right | Key::Up => *value + one,
            Key::Left | Key::Down => *value - one,
            Key::PageUp => *value + 10.0 * step(range),
            Key::PageDown => *value - 10.0 * step(range),
            Key::Home => lo,
            Key::End => hi,
            _ => continue,
        }
        .clamp(lo.min(hi), lo.max(hi));
    }
    moved(before, *value)
}

/// Whether a drag or a key moved the value. Bitwise, so a NaN the caller
/// handed in is not a change on every frame.
fn moved(before: f64, after: f64) -> bool {
    before.to_bits() != after.to_bits()
}

fn unit(value: f64, range: &RangeInclusive<f64>) -> f64 {
    // A fixed parameter reports min == max; without this the divide is NaN,
    // `clamp` passes NaN through, and the flex weight fails validation.
    // An inverted range still divides correctly, so only zero bails.
    let span = range.end() - range.start();
    if span == 0.0 {
        return 0.0;
    }
    ((value - range.start()) / span).clamp(0.0, 1.0)
}

/// What the header says: the text the caller gave, or the value at two
/// decimals. Either way it is measured for the widest string it can say, so
/// digits coming and going never shuffle the label beside it.
///
/// ponytail: a caller's own text is its own reserve -- one string is all the
/// widget is told -- so pad it to its widest form if the units change width.
fn readout(given: Option<String>, value: f64, min: f64, max: f64) -> El {
    if let Some(t) = given {
        text(t.clone()).reserve(t)
    } else {
        let (lo, hi) = (format!("{min:.2}"), format!("{max:.2}"));
        text(format!("{value:.2}")).reserve(if hi.len() > lo.len() { hi } else { lo })
    }
}

/// The thumb's diameter and the lane it slides in, as shares of the
/// control's height.
const THUMB: f64 = 0.35;
const LANE: f64 = 0.45;

/// Label, readout, and a track whose fill and thumb are flex shares. Returns
/// the control and whether the value changed.
///
/// The whole lane is the target: a press on the track jumps the value there
/// and the drag carries on from it, a full-width drag sweeps the full range,
/// and a focused slider steps with the arrow keys. As on a [`knob`], the
/// drawn thumb follows a tween, so a value set from outside glides. Call `Ui::edit` with the
/// same id to bracket the gesture for a host's automation: `Begin` on the
/// press, `End` on the release.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut gain = 0.5;
/// let fader = slider(&mut ui, "gain", "Gain", &mut gain, 0.0..=1.0);
/// assert!(!fader.changed && gain == 0.5, "no gesture, no change");
/// let fader = fader.size(S);
/// ```
pub fn slider(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let before = *value;
    let h = ui.state(&id).hover;
    // Last frame's lane, less the thumb riding it, is the travel a
    // full-range drag covers; before the first frame nothing can drag.
    let (grip, travel) = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .map_or((0.0, 0.0), |s| {
            let grip = s.frame.size.height * THUMB / LANE + 2.0 * h;
            (grip, s.frame.size.width - grip)
        });
    if ui.get(&id).pressed
        && travel > 0.0
        && let Some(p) = ui.local(&id)
    {
        // Off the thumb, a press puts the thumb's centre under it.
        let at = unit(*value, &range) * travel + grip / 2.0;
        if (p.x - at).abs() > grip / 2.0 {
            let t = ((p.x - grip / 2.0) / travel).clamp(0.0, 1.0);
            *value = range.start() + t * (range.end() - range.start());
        }
    }
    let dial = Dial::read(ui, &id, value, &range, travel, false);
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Slider(id, label.into(), dial)),
    }
}

fn slider_el(look: &Look, id: Id, label: Arc<str>, d: Dial) -> El {
    // The rail is a fraction of the control's height, so one size token
    // moves the track, the thumb and the row together.
    let (track, thumb, lane) = (look.px * 0.15, look.px * THUMB, look.px * LANE);
    let (t, h) = (d.t, d.hover);
    let grip = block(thumb + 2.0 * h, thumb + 2.0 * h)
        .pill()
        .fill(look.role);
    col([
        row([
            text(label.clone()),
            spacer(),
            d.readout(look).fill(Role::Dim),
        ])
        .gap(S),
        stack([
            row([
                block(0.0, track).grow(t).pill().fill(look.role),
                block(0.0, track).grow(1.0 - t),
            ])
            .anchor(Align::Stretch, Align::Center)
            .pill()
            .preset(look.face(Role::Field)),
            row([spacer().grow(t), grip, spacer().grow(1.0 - t)])
                .anchor(Align::Stretch, Align::Center),
        ])
        .h(lane)
        .a11y(d.a11y())
        .named(label)
        .focusable()
        .id(id),
    ])
    .gap(Xs)
}

/// A dial: vertical drag, pointer on a 270° sweep; returns the control and
/// whether the value changed. The pointer follows a
/// tween, so a value set from outside -- a preset, a host automation curve --
/// glides instead of jumping, while a drag still tracks the pointer.
///
/// Bracket it with `Ui::edit`, as for [`slider`]. The
/// diameter comes from the theme and the size token; `.px(72.0)` is the
/// hatch for a dial that has to match a hardware panel.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut cutoff = 0.5;
/// let dial = knob(&mut ui, "cut", "Cutoff", &mut cutoff, 0.0..=1.0);
/// assert!(!dial.changed && cutoff == 0.5, "no gesture, no change");
/// let dial = dial.size(Xl);
/// ```
pub fn knob(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let before = *value;
    // Unlike a slider's, a dial's travel is a hand distance, not a geometry:
    // the whole face is the target and 120 px of vertical drag sweeps the
    // range whatever the diameter, so a small knob is not a twitchy one.
    let dial = Dial::read(ui, &id, value, &range, 120.0, true);
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Knob(id, label.into(), dial)),
    }
}

fn knob_el(look: &Look, id: Id, label: Arc<str>, d: Dial) -> El {
    // A dial reads bigger than a button of the same size token: the
    // label sits under it rather than inside it.
    let size = look.px * 1.8;
    let a = (135.0 + 270.0 * d.t).to_radians();
    let r = size / 2.0 - size / 12.0;
    let caption: Arc<str> = look
        .text
        .as_deref()
        .map_or_else(|| label.clone(), Arc::from);
    col([
        stack([
            block(size, size)
                .pill()
                .preset(look.face(Role::Raised))
                .shell(size / 24.0 + 1.0 * d.hover, Role::Field)
                .a11y(d.a11y())
                .named(label)
                .focusable()
                .id(id),
            // The pointer is the reading, so it keeps the role at full
            // strength whatever the variant does to the face.
            block(size / 12.0, size / 12.0)
                .pill()
                .fill(look.role)
                .centered_at(r * a.cos(), r * a.sin()),
        ]),
        text(caption).fill(Role::Dim),
    ])
    .gap(Xs)
    .align(Align::Center)
}

/// A labelled action. Returns the control and whether it was clicked last
/// frame.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let save = button(&mut ui, "save", "Save");
/// assert!(!save.changed, "nothing pressed it last frame");
/// let save = save.variant(Variant::Soft).size(S);
/// ```
pub fn button(ui: &mut Ui, id: impl Into<Id>, label: &str) -> Response<bool, Control> {
    let id: Id = id.into();
    Response {
        changed: activated(ui, &id),
        el: Control::new(ui, Kind::Button(id, label.into())),
    }
}

fn button_el(look: &Look, id: Id, label: Arc<str>) -> El {
    row([text(label.clone()).fill(look.ink())])
        .pad((look.px * 0.4, look.pad_y()))
        .pill()
        .preset(look.style())
        .on(State::Hover, look.hover())
        .animate()
        .a11y(A11y::Button)
        .named(label)
        .focusable()
        .id(id)
}

/// A switch: the knob's side is a flex share it slides between, the click
/// flips it. Returns the control and whether it flipped. The label is the
/// switch's accessible name; the caption beside it is the caller's to draw.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut bypass = false;
/// let sw = toggle(&mut ui, "bypass", "Bypass", &mut bypass);
/// assert!(!sw.changed && !bypass, "nothing clicked it, so it did not flip");
/// let sw = sw.size(Xs);
/// ```
pub fn toggle(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    on: &mut bool,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let flipped = activated(ui, &id);
    if flipped {
        *on = !*on;
    }
    Response {
        el: Control::new(ui, Kind::Toggle(id, label.into(), *on)),
        changed: flipped,
    }
}

fn toggle_el(look: &Look, id: Id, label: Arc<str>, on: bool) -> El {
    let t = f64::from(on);
    // A track is as wide as the control's height and a bit over half as
    // tall: 40 x 22 at the default theme's `M`.
    let (w, h) = (look.px, look.px * 0.55);
    row([
        spacer().grow(t),
        // The knob's side is a flex share; its frame glides between them.
        block(h * 0.73, h * 0.73)
            .pill()
            .fill(Role::Ink)
            .animate_layout(),
        spacer().grow(1.0 - t),
    ])
    .size(w, h)
    .pad(h * 0.14)
    .pill()
    .preset(if on {
        look.style()
    } else {
        look.face(Role::Field)
    })
    .animate()
    .a11y(A11y::Toggle { on })
    .when(!label.is_empty(), |e| e.named(label))
    .focusable()
    .id(id)
}

/// How far a [`drag_value`] is dragged to sweep its whole range.
const DRAG_TRAVEL: f64 = 200.0;

/// A number to drag: a horizontal drag of 200 px sweeps `range` (Shift is
/// fine), the arrow keys step it, and a double click -- or Enter while it
/// is focused -- turns it into a field to type the number into. Enter or a
/// click away takes what was typed, Escape leaves the value alone. Returns
/// the control and whether the value changed. `.value_text(..)` says it
/// in units, as for a [`slider`]. The label is its accessible name.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut bpm = 120.0;
/// let tempo = drag_value(&mut ui, "bpm", "Tempo", &mut bpm, 20.0..=300.0);
/// assert!(!tempo.changed, "no gesture, no change");
/// ```
pub fn drag_value(
    ui: &mut Ui,
    id: impl Into<Id>,
    label: &str,
    value: &mut f64,
    range: RangeInclusive<f64>,
) -> Response<bool, Control> {
    let id: Id = id.into();
    let before = *value;
    let (lo, hi) = (
        range.start().min(*range.end()),
        range.start().max(*range.end()),
    );
    let field = id.field("edit");
    // What was typed, and the width the number had when typing started.
    let mut typing = ui.stash::<(String, f64)>(&field).cloned();
    let r = ui.get(&id);
    // Focus moves to the field only once it is built: this frame's keys --
    // the Enter that opened it -- are not the field's to type.
    let opening = typing.is_none() && (r.double_clicked || r.key_activated);
    if opening {
        let width = ui
            .scene()
            .and_then(|s| s.surface(&id))
            .map_or(ui.theme().control * 16.0, |s| s.frame.size.width);
        typing = Some((format!("{value}"), width));
    }
    if let Some((mut s, width)) = typing {
        let take = |s: &str, value: &mut f64| {
            if let Ok(v) = s.trim().parse::<f64>()
                && v.is_finite()
            {
                *value = v.clamp(lo, hi);
            }
        };
        if opening || ui.focused(&field) {
            let opts = TextOpts {
                blur_on_submit: true,
                ..TextOpts::default()
            };
            let Response { el, changed: e } = text_edit(ui, field.as_str(), &mut s, opts);
            if opening {
                ui.set_sel(&field, 0, s.chars().count());
                ui.focus(field.clone());
            }
            if !e.submitted {
                ui.set_stash(&field, Some((s, width)));
                return Response {
                    el: Control::new(ui, Kind::Field(Box::new(el), width)),
                    changed: false,
                };
            }
            take(&s, value);
        } else if !ui.shortcuts().iter().any(|k| k.key == Key::Escape) {
            // The focus went elsewhere: a click away keeps what was typed.
            take(&s, value);
        }
        ui.set_stash::<(String, f64)>(&field, None);
    } else {
        ui.drag(&id, value, range.clone(), DRAG_TRAVEL, false);
        stepped(ui, &id, value, &range);
    }
    // Nothing is drawn sliding, so there is no thumb to tween.
    let dial = Dial::of(*value, &range, 0.0, 0.0);
    Response {
        changed: moved(before, *value),
        el: Control::new(ui, Kind::Drag(id, label.into(), dial)),
    }
}

fn drag_el(look: &Look, id: Id, label: Arc<str>, d: Dial) -> El {
    row([d.readout(look)])
        .pad((look.px * 0.3, look.pad_y()))
        .radius(4.0)
        .preset(look.face(Role::Field))
        .cursor(Cursor::ResizeH)
        .a11y(d.a11y())
        .when(!label.is_empty(), |e| e.named(label))
        .focusable()
        .id(id)
}
