pub mod build_info;
pub mod creator;
pub mod diagnostics;
pub mod parity;
#[cfg(feature = "shots")]
#[path = "../tools/kontra-scan/metrics.rs"]
pub(crate) mod scan_metrics;
pub mod sound;
pub mod support;
pub mod timing;
#[cfg(feature = "shots")]
pub use ui::scan::one as scan_one;
#[cfg(feature = "plugin")]
mod artwork;
#[cfg(feature = "plugin")]
mod library;
#[cfg(feature = "plugin")]
mod plugin;
#[cfg(feature = "plugin")]
mod routing;
#[cfg(feature = "plugin")]
mod ui;
#[cfg(feature = "plugin")]
pub use plugin::Plugin;
#[cfg(feature = "plugin")]
pub fn export_multi_state(
    multi: &std::path::Path,
    destination: &std::path::Path,
) -> anyhow::Result<()> {
    plugin::export_multi_state(multi, destination)
}
/// The library folders the player scans, from its settings.
#[cfg(feature = "plugin")]
pub fn library_roots() -> Vec<std::path::PathBuf> {
    library::Settings::path()
        .and_then(|p| library::Settings::load(&p))
        .map(|s| s.roots.into_iter().map(|r| r.path.into()).collect())
        .unwrap_or_default()
}

#[cfg(feature = "shots")]
pub use ui::health::survey as ui_health;
/// In-memory authored UI witness through the editor's layout, asset worker and CPU painter.
#[cfg(feature = "shots")]
pub use ui::offline_preview::render as offline_ui_preview;
/// Explicit bounded production plan-activation witness, without a plugin or audio device.
#[cfg(feature = "shots")]
pub use ui::offline_lifecycle::{run as offline_ui_lifecycle, Witness as OfflineUiLifecycle};
/// Metadata-only UVI UI render check; assets and pixels remain in memory.
#[cfg(feature = "shots")]
pub use ui::uvi_ui_health;
