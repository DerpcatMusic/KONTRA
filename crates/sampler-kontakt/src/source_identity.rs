//! Admission identity shared by disk decoders. No sample payload, held file
//! descriptor, filesystem lookup or allocation is retained by the stamp.
use std::{
    fs::{File, Metadata},
    io,
};

/// Identity of bytes admitted as an immutable source revision. Unix includes
/// inode/device and high-resolution change time, so restoring mtime cannot hide
/// an in-place rewrite. Other platforms currently retain length/modify/create
/// times; equivalent by-handle change-time detection there remains unverified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIdentity {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(not(unix))]
    created: std::time::SystemTime,
}
impl FileIdentity {
    pub fn capture(meta: &Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            dev: meta.dev(),
            #[cfg(unix)]
            inode: meta.ino(),
            #[cfg(unix)]
            changed: (meta.ctime(), meta.ctime_nsec()),
            #[cfg(not(unix))]
            created: meta.created()?,
        })
    }
    pub fn len(&self) -> u64 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Validate the actual opened handle, never just a path that could have
    /// changed between lookup and open. Repeat after physical reads as well.
    pub fn check(&self, file: &File) -> io::Result<()> {
        if *self != Self::capture(&file.metadata()?)? {
            return Err(io::Error::new(io::ErrorKind::InvalidData, SourceChanged));
        }
        Ok(())
    }
    pub fn check_range(&self, offset: u64, size: u64) -> io::Result<()> {
        if offset.checked_add(size).is_none_or(|end| end > self.len) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "sample range exceeds admitted source",
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct SourceChanged;
impl std::fmt::Display for SourceChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("sample source changed since admission")
    }
}
impl std::error::Error for SourceChanged {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admitted_handle_and_reopened_path_cannot_rebind_identity() {
        let dir =
            std::env::temp_dir().join(format!("kontakt-source-identity-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.bin");
        std::fs::write(&path, [1u8; 128]).unwrap();
        let held = File::open(&path).unwrap();
        let identity = FileIdentity::capture(&held.metadata().unwrap()).unwrap();
        assert!(identity.check(&held).is_ok());
        assert!(identity.check_range(64, 64).is_ok());
        assert!(identity.check_range(64, 65).is_err());
        assert!(identity.check_range(u64::MAX, 2).is_err());
        let other = dir.join("replacement.bin");
        std::fs::write(&other, [2u8; 128]).unwrap();
        let stamp = held.metadata().unwrap().modified().unwrap();
        File::options()
            .write(true)
            .open(&other)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp))
            .unwrap();
        std::fs::rename(other, &path).unwrap();
        assert!(identity.check(&File::open(&path).unwrap()).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn same_size_rewrite_of_retained_handle_with_restored_mtime_is_detected() {
        let path =
            std::env::temp_dir().join(format!("kontakt-source-rewrite-{}.bin", std::process::id()));
        std::fs::write(&path, [1u8; 128]).unwrap();
        let held = File::open(&path).unwrap();
        let identity = FileIdentity::capture(&held.metadata().unwrap()).unwrap();
        let stamp = held.metadata().unwrap().modified().unwrap();
        std::fs::write(&path, [2u8; 128]).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stamp))
            .unwrap();
        let error = identity.check(&held).unwrap_err();
        assert!(
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<SourceChanged>()
                .is_some()
        );
        std::fs::remove_file(path).unwrap();
    }
}
