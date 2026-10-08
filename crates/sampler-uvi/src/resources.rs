//! UI resources remain library-owned and are decoded in memory off audio.
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const LIMIT: u64 = 32 << 20;

/// Message-free failures shared with the UI worker. Absence is `Ok(None)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceError {
    InvalidPath,
    Ambiguous,
    Corrupt,
    Limit,
    Read,
    Unavailable,
}
impl std::fmt::Display for ResourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPath => "invalid UVI resource path",
            Self::Ambiguous => "ambiguous UVI resource",
            Self::Corrupt => "corrupt UVI resource",
            Self::Limit => "UVI resource exceeds 32 MiB",
            Self::Read => "UVI resource read failed",
            Self::Unavailable => "UVI resource authority unavailable",
        })
    }
}
impl std::error::Error for ResourceError {}

// Ported from v1 4bffbb18:src/uvi/host.rs::resource_path; typed errors replace
// the Lua error at this resource-provider boundary.
pub(crate) fn validate_path(path: &str) -> Result<(), ResourceError> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(ResourceError::InvalidPath);
    }
    Ok(())
}

pub struct Resources {
    #[cfg(feature = "library-access")]
    bank: Option<Result<(crate::Bank, String), ResourceError>>,
    root: PathBuf,
}
impl Resources {
    pub fn of(program: &Path) -> Self {
        #[cfg(feature = "library-access")]
        let bank = program
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
            .map(|p| {
                let bank = crate::Bank::open(p).map_err(|_| ResourceError::Unavailable)?;
                let member = program
                    .strip_prefix(p)
                    .map_err(|_| ResourceError::InvalidPath)?
                    .to_string_lossy()
                    .replace('\\', "/");
                Ok((bank, member))
            });
        Self {
            #[cfg(feature = "library-access")]
            bank,
            root: program.parent().unwrap_or(Path::new(".")).to_owned(),
        }
    }
    pub fn read_result(&self, path: &str) -> Result<Option<Vec<u8>>, ResourceError> {
        validate_path(path)?;
        #[cfg(feature = "library-access")]
        if let Some(bank) = &self.bank {
            let (bank, program) = bank.as_ref().map_err(|e| *e)?;
            return bank.ui_resource_result(program, path);
        }
        #[cfg(not(feature = "library-access"))]
        if self
            .root
            .ancestors()
            .any(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
        {
            return Err(ResourceError::Unavailable);
        }
        let path = path.replace('\\', "/");
        let relative = Path::new(&path);
        if relative.is_absolute()
            || path.contains(':')
            || path.starts_with('$')
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(ResourceError::InvalidPath);
        }
        let canonical = |path: &Path| match path.canonicalize() {
            Ok(p) => Ok(Some(p)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(ResourceError::Read),
        };
        let Some(root) = canonical(&self.root)? else {
            return Ok(None);
        };
        let Some(file) = canonical(&root.join(relative))? else {
            return Ok(None);
        };
        if !file.starts_with(&root) {
            return Err(ResourceError::InvalidPath);
        };
        let file = std::fs::File::open(file).map_err(|_| ResourceError::Read)?;
        if file.metadata().map_err(|_| ResourceError::Read)?.len() > LIMIT {
            return Err(ResourceError::Limit);
        };
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ResourceError::Read)?;
        if bytes.len() as u64 > LIMIT {
            return Err(ResourceError::Limit);
        };
        Ok(Some(bytes))
    }
    /// Compatibility accessor; the shared renderer uses read_result diagnostics.
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.read_result(path).ok().flatten()
    }
}
