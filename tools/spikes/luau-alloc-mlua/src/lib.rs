//! Spike P3.1b: keep mlua's API on an arena. See docs/architecture-v3/spikes/luau-alloc-mlua.md.
//!
//! The routing allocator is installed here, so every binary and test in this crate uses it.

pub mod routed;

#[global_allocator]
pub static GLOBAL: routed::Routed = routed::Routed;
