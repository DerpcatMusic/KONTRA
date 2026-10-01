pub mod ksp;
pub mod import;
// Encrypted library content: the `library-access` feature (off by default).
#[cfg_attr(feature = "library-access", path = "access.rs")]
#[cfg_attr(not(feature = "library-access"), path = "no_access.rs")]
mod access;
mod cache;
pub mod modulation;
pub mod audio;
pub mod engine;
pub mod articulate;
pub mod timing;
pub mod fx;
pub mod creator;
#[cfg(feature="plugin")]
mod artwork;
#[cfg(feature="plugin")]
mod library;
#[cfg(feature="plugin")]
mod plugin;
#[cfg(feature="plugin")]
mod routing;
#[cfg(feature="plugin")]
mod ui;
#[cfg(feature="plugin")]
pub use plugin::{Plugin, bench_host};
/// `kontakto audit-ui`: see `ui::audit`.
#[cfg(feature="plugin")]
pub use ui::audit::run as audit_ui;
/// The library folders the player scans, from its settings.
#[cfg(feature="plugin")]
pub fn library_roots() -> Vec<std::path::PathBuf> {
    library::Settings::path().and_then(|p| library::Settings::load(&p)).map(|s| s.roots.into_iter().map(|r| r.path.into()).collect()).unwrap_or_default()
}
