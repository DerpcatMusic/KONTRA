//! Wiring between front ends, formats and the engine (07-APP.md). Each piece
//! here only adapts one crate's output to another's input.

#[cfg(feature = "align")]
pub mod align;
pub mod browser;
pub mod instrument;
pub use kontra_face::perf_view;
pub mod persist;
pub mod presets;
pub mod rack;
pub mod relink;
pub mod script;
pub mod sound;
pub mod state;
pub use kontra_face as ui;
/// The picture a UVI bank shows in the browser.
pub use kontra_format_uvi::cover as bank_cover;
pub mod view;
