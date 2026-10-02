//! Stand-in for the `library-access` feature: this build reads no library
//! access data, so encrypted presets and archive members are refused.

use anyhow::{bail, Result};
use ni_file::nis::LibraryKey;
use std::{path::Path, sync::Arc};

/// Called only for encrypted content.
pub(crate) fn library_key(_: &Path) -> Result<Option<Arc<dyn LibraryKey>>> {
    bail!("Encrypted library content is not supported in this build.")
}

pub(crate) fn require_library_key(_: &Path) -> Result<Arc<dyn LibraryKey>> {
    bail!("Encrypted library content is not supported in this build.")
}
