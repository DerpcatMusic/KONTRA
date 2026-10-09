//! Named resource bytes from the user's NICNT/NKR containers, in memory only.

use crate::LoadError;
use std::path::Path;

/// An indexed resource container. No Debug implementation: access stays private.
pub struct ResourceContainer {
    #[cfg(feature = "library-access")]
    file: std::fs::File,
    #[cfg(feature = "library-access")]
    path: std::path::PathBuf,
    #[cfg(feature = "library-access")]
    index: Index,
    #[cfg(feature = "library-access")]
    key: Option<std::sync::Arc<dyn ni_file::nis::LibraryKey>>,
}

#[cfg(feature = "library-access")]
enum Index {
    Archive(ni_file::nkr::Archive),
    Files(ni_file::file_container::NIFileContainer),
}

#[cfg(feature = "library-access")]
impl ResourceContainer {
    /// Index a NICNT resource section or an NKX/NKR directory. Reads no pictures.
    pub fn open(path: &Path) -> Result<Self, LoadError> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).map_err(|e| LoadError::io(path, e))?;
        let index = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
        {
            // NICNT product metadata precedes an embedded FileContainer. Bound
            // the search; never expose or persist the product's access fields.
            let mut head = Vec::new();
            file.by_ref()
                .take(4 << 20)
                .read_to_end(&mut head)
                .map_err(|e| LoadError::io(path, e))?;
            let marker = b"/\\ NI FC MTD  /\\";
            let start = head
                .windows(marker.len())
                .enumerate()
                .skip(1)
                .find_map(|(at, bytes)| (bytes == marker).then_some(at))
                .ok_or_else(|| LoadError::Invalid {
                    path: path.into(),
                    reason: "NICNT has no indexed resource section in its first 4 MiB".into(),
                })?;
            file.seek(SeekFrom::Start(start as u64))
                .map_err(|e| LoadError::io(path, e))?;
            Index::Files(
                ni_file::file_container::NIFileContainer::read(&mut file)
                    .map_err(|e| LoadError::decode(path, "NICNT resources", e))?,
            )
        } else {
            Index::Archive(
                ni_file::nkr::Archive::read_index(&mut file)
                    .map_err(|e| LoadError::decode(path, "resource directory", e))?,
            )
        };
        Ok(Self {
            file,
            path: path.into(),
            index,
            key: None,
        })
    }

    /// All authored resource names, including picture layout `.txt` companions.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<_> = match &self.index {
            Index::Archive(archive) => archive.members().map(|e| e.name.as_str()).collect(),
            Index::Files(files) => files.items.iter().map(|e| e.filename.as_str()).collect(),
        };
        names.sort_unstable();
        names
    }

    /// Read one named picture or companion, at most 32 MiB, without extraction.
    /// `None` means the name is absent; malformed/protected resources are errors.
    /// Names are case insensitive; NICNT's `|` separators also accept `/`.
    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, LoadError> {
        use std::io::{Read, Seek, SeekFrom};
        let (offset, size) = match &self.index {
            Index::Archive(archive) => {
                let Some(entry) = archive
                    .member(&mut self.file, name)
                    .map_err(|e| LoadError::decode(&self.path, "resource header", e))?
                else {
                    return Ok(None);
                };
                if !entry.valid {
                    return Err(LoadError::Invalid {
                        path: self.path.clone(),
                        reason: "Invalid resource member".into(),
                    });
                }
                if entry.size > 32 << 20 {
                    return Err(LoadError::Invalid {
                        path: self.path.clone(),
                        reason: "Resource exceeds 32 MiB".into(),
                    });
                }
                if entry.encoded && entry.key_index != 0xff && self.key.is_none() {
                    self.key = Some(crate::library_key(&self.path).map_err(|reason| {
                        LoadError::Access {
                            path: self.path.clone(),
                            reason,
                        }
                    })?);
                }
                return archive
                    .read_entry_with_key(&mut self.file, name, self.key.as_deref())
                    .map(Some)
                    .map_err(|e| LoadError::decode(&self.path, "resource bytes", e));
            }
            Index::Files(files) => {
                let normalized = name.replace(['|', '\\'], "/");
                let mut found = files.items.iter().filter(|item| {
                    item.filename
                        .replace(['|', '\\'], "/")
                        .eq_ignore_ascii_case(&normalized)
                });
                let Some(item) = found.next() else {
                    return Ok(None);
                };
                if found.next().is_some() {
                    return Err(LoadError::Invalid {
                        path: self.path.clone(),
                        reason: "Ambiguous resource name".into(),
                    });
                }
                (
                    files.file_section_offset + item.file_start_offset,
                    item.file_size,
                )
            }
        };
        if size > 32 << 20 {
            return Err(LoadError::Invalid {
                path: self.path.clone(),
                reason: "Resource exceeds 32 MiB".into(),
            });
        }
        self.file
            .seek(SeekFrom::Start(offset))
            .map_err(|e| LoadError::io(&self.path, e))?;
        let mut bytes = vec![0; size as usize];
        self.file
            .read_exact(&mut bytes)
            .map_err(|e| LoadError::io(&self.path, e))?;
        Ok(Some(bytes))
    }
}

#[cfg(not(feature = "library-access"))]
impl ResourceContainer {
    pub fn open(path: &Path) -> Result<Self, LoadError> {
        Err(LoadError::Access {
            path: path.into(),
            reason: "Resource containers need the library-access feature".into(),
        })
    }
    pub fn names(&self) -> Vec<&str> {
        Vec::new()
    }
    pub fn read(&mut self, _: &str) -> Result<Option<Vec<u8>>, LoadError> {
        Ok(None)
    }
}
