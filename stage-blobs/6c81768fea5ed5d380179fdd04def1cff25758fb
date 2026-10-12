//! Inert catalog for project identity when UVI support is disabled.

use super::{UviBank, UviSource};
use std::{path::{Path, PathBuf}, sync::Arc};

#[derive(Default)]
pub(super) struct UviCatalog {
    pub(super) reader_path: Option<PathBuf>,
}

impl UviCatalog {
    pub(super) fn inventory(&mut self, path: &Path) -> Result<(String, Arc<UviBank>), &'static str> {
        let _ = path;
        Err("UVI inventory requires a build with UVI support.")
    }

    pub(super) fn inspect(&mut self, source: &UviSource) -> Result<(), &'static str> {
        let _ = source;
        Err("UVI playback requires a build with UVI support.")
    }
}

/// A disabled backend has no reader authority or cache reader binding.
pub(super) fn effective_uvi_reader(settings: &super::Settings) -> Option<PathBuf> {
    let _ = settings; None
}
