//! Workspace facts from `cargo metadata` (no compilation, so no build slot needed).

use cargo_metadata::{Metadata, MetadataCommand, Package};
use std::path::{Path, PathBuf};

/// Metadata for the workspace in the current directory.
pub fn load() -> Result<Metadata, String> {
    MetadataCommand::new()
        .exec()
        .map_err(|error| format!("cargo metadata failed: {error}"))
}

/// The directory holding the workspace `Cargo.toml`.
pub fn root(metadata: &Metadata) -> PathBuf {
    metadata.workspace_root.as_std_path().to_path_buf()
}

/// The directory holding a package's `Cargo.toml`.
pub fn package_dir(package: &Package) -> PathBuf {
    let manifest = package.manifest_path.as_std_path();
    manifest
        .parent()
        .map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}
