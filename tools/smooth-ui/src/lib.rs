//! SMOOTH's interface, as a library.
//!
//! The editor is one intrinsic tree. `skin` holds the look -- metrics,
//! palette, and the parts every view is built from; `view` composes those
//! parts into the column, rail and right column; `graph` is the response
//! plot. Nothing is placed at a coordinate: canvases are the only place with
//! geometry, and even they are given a box by the layout.
//!
//! Two consumers share it: `smooth-preview` renders it to a PNG for pixel
//! comparison against the reference, and the SMOOTH plug-in hosts the same
//! tree in a DAW window.

pub mod graph;
pub mod input;
pub mod presets;
pub mod icons;
pub mod menus;
pub mod skin;
pub mod slots;
pub mod theme;
pub mod tips;
pub mod tokens;
pub mod view;

pub use skin::{H, W};
pub use view::Model;
