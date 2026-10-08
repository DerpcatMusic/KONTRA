pub mod sound;
pub mod build_info;
pub mod diagnostics;
pub mod support;
#[cfg(feature = "shots")]
#[path = "../tools/kontra-scan/metrics.rs"]
pub(crate) mod scan_metrics;
#[cfg(feature = "shots")]
pub use ui::scan::one as scan_one;
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
pub use plugin::Plugin;
/// The library folders the player scans, from its settings.
#[cfg(feature="plugin")]
pub fn library_roots() -> Vec<std::path::PathBuf> {
    library::Settings::path().and_then(|p| library::Settings::load(&p)).map(|s| s.roots.into_iter().map(|r| r.path.into()).collect()).unwrap_or_default()
}

/// Metadata-only UVI UI render check; assets and pixels remain in memory.
#[cfg(feature = "shots")]
pub use ui::uvi_ui_health;
