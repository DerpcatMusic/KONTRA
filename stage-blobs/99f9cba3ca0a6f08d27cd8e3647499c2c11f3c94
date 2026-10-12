//! The controls MUI ships with, and the presets they sit on.
//!
//! Every widget here is an ordinary styled tree, and every one has the same
//! shape: `&mut Ui`, the id, the label (controls only), the value it edits
//! by `&mut`, then its range or options. Every one returns a [`Response`]:
//! the element and what happened last frame -- `changed: bool` for
//! [`button`] (clicked), [`toggle`], [`slider`], [`knob`], [`drag_value`],
//! [`text_input`], [`color_picker`] and [`oklch_picker`], [`TextEdit`] for [`text_edit`], and
//! `Option<edit>` for [`curve`] and [`bins`], whose edit says which part
//! moved. A control's `el` is a [`Control`], finished with its look
//! (`.variant`, `.size`, `.role`) or dropped straight into a `row![..]`.
//! [`presets`] holds the shapes a panel is made of.
//!
//! A widget reads last frame's gestures and state from the [`Ui`](crate::Ui)
//! it is handed; nothing here owns a frame, a spring table or an event loop,
//! and nothing here rasterizes.

use mui_scene::{
    Align, Appear, El, Fill, IntoEl, Len, Pad, Paints, Px, Role, SpacingToken, Styled,
};
use std::sync::Arc;

/// What a widget hands back: its tree, and what happened to it last frame.
///
/// `C` is `bool` for a value widget (changed, or clicked for a button) and
/// the edit for [`curve`] and [`bins`]. A control's `E` is a [`Control`],
/// which still takes its look; everything else's is an [`El`]. Either drops
/// into a `row![..]` whole, or read `.el` and `.changed`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut on = false;
/// let sw = toggle(&mut ui, "bypass", "Bypass", &mut on);
/// assert!(!sw.changed);
/// let strip = row![sw.size(Xs), body("Bypass")];
/// ```
#[must_use]
pub struct Response<C = bool, E = El> {
    pub el: E,
    pub changed: C,
}
impl<C, E: IntoEl> IntoEl for Response<C, E> {
    fn into_el(self) -> El {
        self.el.into_el()
    }
}
impl<C, E: IntoEl> From<Response<C, E>> for El {
    fn from(r: Response<C, E>) -> Self {
        r.el.into_el()
    }
}

/// `$name(args)` on a `Response`, applied to its `el` as `$method(args)`.
macro_rules! forward {
    ($out:ty, $conv:path: $($name:ident($($a:ident: $t:ty),*);)*) => {$(
        #[doc = concat!("`.", stringify!($name), "(..)` on the element; `changed` rides along.")]
        pub fn $name(self, $($a: $t),*) -> Response<C, $out> {
            Response { el: $conv(self.el).$name($($a),*), changed: self.changed }
        }
    )*};
}

/// A control's look, straight on the response:
/// `knob(ui, id, "Gain", v, 0.0..=1.0).size(L).tip("Gain")`.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let mut v = 0.5;
/// let gain = knob(&mut ui, "gain", "Gain", &mut v, 0.0..=1.0).size(L).tip("Gain");
/// if gain.changed { /* write v back */ }
/// let strip = row![gain, body("dB")];
/// ```
impl<C> Response<C, Control> {
    forward! { Control, std::convert::identity:
        size(s: SpacingToken);
        variant(v: Variant);
        role(r: Role);
        value_text(s: impl Into<String>);
        px(px: impl Px);
    }
}
impl<C> Response<C, El> {
    forward! { El, std::convert::identity:
        size(w: impl Into<Len>, h: impl Into<Len>);
    }
}
/// The common element verbs on any response; a control is finished first,
/// so its look goes before these.
impl<C, E: IntoEl> Response<C, E> {
    forward! { El, IntoEl::into_el:
        tip(s: impl Into<Arc<str>>);
        named(s: impl Into<Arc<str>>);
        disabled();
        when(cond: bool, f: impl FnOnce(El) -> El);
        fill(f: impl Into<Fill>);
        opacity(o: f32);
        animate();
        appear(a: Appear);
        w(l: impl Into<Len>);
        h(l: impl Into<Len>);
        square(l: impl Into<Len>);
        min_w(x: impl Px);
        min_h(y: impl Px);
        grow(weight: impl Px);
        shrink(weight: impl Px);
        basis(b: impl Px);
        flex(weight: impl Px);
        align_self(a: Align);
        pad(p: impl Into<Pad>);
        at(dx: impl Px, dy: impl Px);
        offset(dx: impl Px, dy: impl Px);
        centered();
        centered_at(dx: impl Px, dy: impl Px);
        span(cols: usize);
        order(o: i32);
    }
}

mod bins;
mod collections;
mod color_picker;
mod controls;
mod curve;
mod grapheme;
mod pick;
pub mod presets;
mod text;
pub mod visualization;

pub use bins::{BinAxis, BinEdit, Bins, bins, bins_hover};
pub use color_picker::{ColorFormat, OklchPicker, PickerShape, oklch_picker};
pub(crate) use controls::step;
pub use controls::{Control, Variant, button, drag_value, knob, slider, stepped, toggle};
pub use curve::{CurveEdit, curve};
pub use pick::{ColorOpts, color_picker};
pub use presets::{card, chip, glass, meter, panel, tile};
pub use text::{Newline, TextEdit, TextOpts, text_edit, text_input};

pub use collections::{
    ListEvent, ListItem, ListOptions, ListRow, ListState, ScrollTo, uniform_list, variable_list,
};
