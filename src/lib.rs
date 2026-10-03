pub mod ksp;
pub mod build_info;
pub mod import;
// Encrypted library content: the `library-access` feature (enabled by default).
#[cfg_attr(feature = "library-access", path = "access.rs")]
#[cfg_attr(not(feature = "library-access"), path = "no_access.rs")]
mod access;
mod cache;
mod resources;
pub mod diagnostics;
pub mod modulation;
pub mod audio;
pub mod engine;
pub mod articulate;
pub mod timing;
#[cfg(feature = "uvi")]
pub mod uvi;
pub mod fx;
pub mod creator;
#[cfg(feature="plugin")]
mod artwork;
#[cfg(feature="plugin")]
mod library;
#[cfg(feature="plugin")]
mod plugin;
#[cfg(feature="plugin")]
pub mod project_migration;
#[cfg(feature="plugin")]
pub(crate) use plugin::{Part as MigrationPart, SavedMulti as MigrationSavedMulti, Selection as MigrationSelection};
#[cfg(feature="plugin")]
mod routing;
#[cfg(feature="plugin")]
mod ui;
#[cfg(feature="plugin")]
pub use plugin::{Plugin, bench_host, bench_ui_control};
/// `kontakto audit-ui`: see `ui::audit`.
#[cfg(feature="plugin")]
pub use ui::audit::run as audit_ui;
/// The library folders the player scans, from its settings.
#[cfg(feature="plugin")]
pub fn library_roots() -> Vec<std::path::PathBuf> {
    library::Settings::path().and_then(|p| library::Settings::load(&p)).map(|s| s.roots.into_iter().map(|r| r.path.into()).collect()).unwrap_or_default()
}
