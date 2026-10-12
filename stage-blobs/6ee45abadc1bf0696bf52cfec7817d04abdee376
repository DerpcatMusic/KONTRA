//! Runtime preset discovery and management - re-exported from
//! [`moose_utils::presets`], where the implementation lives so
//! `cargo moose preset` shares it without inheriting `moose-core`'s
//! runtime dependency chain (the same split as [`crate::state`]).
//!
//! Format wrappers use the discovery half (`enumerate_scope`,
//! `read_preset_ref`, `load_preset_file`, the scope roots) to
//! surface presets to hosts; in-editor preset menus use
//! [`PresetStore`] for the management operations.

pub use moose_utils::presets::*;
