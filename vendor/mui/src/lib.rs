//! MUI: a styled tree in, pixels and gestures out.
//!
//! ```
//! use mui::prelude::*;
//! let mut ui = Ui::default();
//! let mut cutoff = 0.5;
//! // One frame: build the tree, hand it in with the input, draw what comes back.
//! let root = col![
//!     body("Filter"),
//!     slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0),
//! ]
//! .gap(S)
//! .pad(M)
//! .fill(Role::Surface);
//! let frame = ui.frame(root, Some(Size::new(240.0, 96.0)), Input::default(), 1.0 / 60.0).unwrap();
//! assert!(frame.scene.paint.len() > 3);
//! // frame.cursor is what to set; frame.tip is the tooltip that came due;
//! // frame.animating says whether to schedule another frame; frame.edits is
//! // every gesture that began or ended, and frame.clipboard is what a copy
//! // wants put on the system clipboard.
//! ```
#![forbid(unsafe_code)]

// The crates underneath, each by name and each its own curated surface: a
// renderer, a layout engine or a pointer model is reached as
// `mui::vello::..`, `mui::layout::..`, never flattened into `mui::`.
pub use mui_geometry as geometry;
pub use mui_input as input;
pub use mui_layout as layout;
pub use mui_material as material;
pub use mui_motion as motion;
pub use mui_scene as scene;
pub use mui_vello as vello;

mod actions;
pub mod host;
pub mod profiling;
mod ui;
pub mod widgets;
pub use actions::SemanticAction;
pub use mui_vello::diagnostics;
pub use widgets::presets;

pub use ui::{Clipboard, Edit, Frame, Interaction, MotionPolicy, TextInputState, TextRuns, Ui};

/// What a MUI app writes against, named one by one: the widgets, the DSL,
/// the input a host hands in. No globs, so nothing arrives here because a
/// crate underneath grew it. `Response` is the widgets'; a pointer's
/// per-target report is `mui::input::Response`.
pub mod prelude {
    pub use crate::widgets::presets::{card, chip, glass, meter, panel, tile};
    pub use crate::widgets::{
        BinAxis, BinEdit, Bins, ColorFormat, ColorOpts, Control, CurveEdit, ListEvent, ListItem,
        ListOptions, ListRow, ListState, Newline, OklchPicker, PickerShape, Response, ScrollTo,
        TextEdit, TextOpts, Variant, bins, bins_hover, button, color_picker, curve, drag_value,
        knob, oklch_picker, slider, stepped, text_edit, text_input, toggle, uniform_list,
        variable_list,
    };
    pub use crate::{Edit, Frame, Interaction, MotionPolicy, SemanticAction, Ui};
    pub use mui_input::{
        Axis, Button, Buttons, FINE_DRAG, Ime, Input, Key, KeyPress, Mods, PointerInput, Vec2,
    };
    pub use mui_material::{Capture, Material};
    pub use mui_scene::prelude::{
        A11y, Align, Appear, Area, Axes, BorderAlign, BorderRamp, CanvasCache, Color, Corner,
        CornerStyle, Cursor, Draw, Ease, El, Elevation, Fill, Fit, Font, Gradient, GridTrack, Id,
        Image, IntoEl, Justify, Keys, L, Len, M, Match, Mix, Paints, Path, Pin, Point, Radius,
        Resolver, Role, S, SceneSpec, Shadow, ShapeLayout, Size, Spacing, State, Style, Styled,
        Theme, Weight, Weld, WeldBackend, WeldChannel, WeldQuality, Xl, Xs, block, body, canvas,
        canvas_keyed, caption, clamp, col, cq, fits, grid, icon, pct, resolve, row, spacer, stack,
        step, sym, text, title, weld,
    };
    pub use mui_scene::{Corners, Mode, Palette, Pigment, SpacingToken, Spring};
}

/// The before/after example in `docs/DSL-V2.md`, compiled.
#[cfg(doctest)]
#[doc = include_str!("../docs/DSL-V2.md")]
struct DslDoctests;

/// Every runnable `rust` block in the README, compiled and run by
/// `cargo test --doc`; `rust,ignore` blocks remain illustrative by design.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
