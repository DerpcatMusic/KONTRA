//! Wiring between front ends, formats and the engine (07-APP.md). Each piece
//! here only adapts one crate's output to another's input.

#[cfg(feature = "align")]
pub mod align;
pub mod instrument;
pub mod perf_view;
pub mod rack;
pub mod script;
pub mod ui;
