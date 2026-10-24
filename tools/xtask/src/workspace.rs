//! Workspace facts from `cargo metadata`. The call goes through build-slot like every cargo call.

use cargo_metadata::{Metadata, MetadataCommand, Package};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Metadata for the workspace in the current directory.
pub fn load() -> Result<Metadata, String> {
    let output = Command::new("build-slot")
        .args(["cargo", "metadata", "--format-version", "1"])
        .stderr(Stdio::inherit())
        .output()
        .map_err(|error| format!("cannot run build-slot: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed ({})", output.status));
    }
    let json = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo metadata output is not UTF-8: {error}"))?;
    MetadataCommand::parse(json).map_err(|error| format!("cannot parse cargo metadata: {error}"))
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
