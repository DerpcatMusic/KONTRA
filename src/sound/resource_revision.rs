//! Worker-captured identity of a Kontakt UI resource namespace.
//!
//! Preset parameters do not invalidate decoded artwork. Changes to the base
//! instrument or any resource do. Unknown identities deliberately disable reuse.
use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileRevision {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    unix: (u64, u64, i64, i64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResourceRevision {
    owner: PathBuf,
    root: PathBuf,
    program: u32,
    files: Vec<FileRevision>,
}

impl ResourceRevision {
    /// Loader only: no discovery, stat or filesystem work runs in the editor.
    pub(crate) fn capture(
        owner: &Path,
        program: u32,
        resources: &sampler_kontakt::Resources,
    ) -> Option<Self> {
        if !resources.namespace_complete() {
            return None;
        }
        let owner = owner.canonicalize().ok()?;
        let mut paths = resources.locations();
        paths.push(owner.clone());
        paths.sort();
        paths.dedup();
        let files = paths
            .into_iter()
            .map(|path| {
                let metadata = path.metadata().ok()?;
                if !metadata.is_file() {
                    return None;
                }
                #[cfg(unix)]
                let unix = {
                    use std::os::unix::fs::MetadataExt;
                    (
                        metadata.dev(),
                        metadata.ino(),
                        metadata.ctime(),
                        metadata.ctime_nsec(),
                    )
                };
                Some(FileRevision {
                    bytes: metadata.len(),
                    modified: metadata.modified().ok()?,
                    path,
                    #[cfg(unix)]
                    unix,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            owner,
            root: resources.root().into(),
            program,
            files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_namespace_reuses_but_resource_edit_or_missing_file_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let owner = dir.path().join("instrument.nki");
        let resources_dir = dir.path().join("Resources");
        std::fs::create_dir(&resources_dir).unwrap();
        let picture = resources_dir.join("wallpaper.png");
        std::fs::write(&owner, b"synthetic owner").unwrap();
        std::fs::write(&picture, b"first").unwrap();
        let resources = sampler_kontakt::Resources::of(&owner);
        let before = ResourceRevision::capture(&owner, 0, &resources).unwrap();
        assert_eq!(
            Some(&before),
            ResourceRevision::capture(&owner, 0, &resources).as_ref()
        );
        assert_ne!(
            Some(&before),
            ResourceRevision::capture(&owner, 1, &resources).as_ref()
        );
        // Replacement has the same size and can preserve timestamps; Unix inode
        // and change time still invalidate it.
        let replacement = resources_dir.join("replacement");
        std::fs::write(&replacement, b"other").unwrap();
        std::fs::rename(&replacement, &picture).unwrap();
        assert_ne!(
            Some(before),
            ResourceRevision::capture(&owner, 0, &resources)
        );
        std::fs::remove_file(&picture).unwrap();
        assert_eq!(None, ResourceRevision::capture(&owner, 0, &resources));
    }
}
