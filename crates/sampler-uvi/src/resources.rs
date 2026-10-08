//! UI resources remain library-owned and are decoded in memory off audio.
use std::path::{Path, PathBuf};
const LIMIT: u64 = 32 << 20;
pub struct Resources {
    #[cfg(feature = "library-access")]
    bank: Option<(crate::Bank, String)>,
    root: PathBuf,
}
impl Resources {
    pub fn of(program: &Path) -> Self {
        #[cfg(feature = "library-access")]
        let bank = program
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ufs")))
            .and_then(|p| {
                Some((
                    crate::Bank::open(p).ok()?,
                    program
                        .strip_prefix(p)
                        .ok()?
                        .to_string_lossy()
                        .replace('\\', "/"),
                ))
            });
        Self {
            #[cfg(feature = "library-access")]
            bank,
            root: program.parent().unwrap_or(Path::new(".")).to_owned(),
        }
    }
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        #[cfg(feature = "library-access")]
        if let Some((bank, program)) = &self.bank {
            return bank.ui_resource(program, path).ok();
        }
        let path = path.replace('\\', "/");
        let relative = Path::new(&path);
        if relative.is_absolute()
            || path.contains(['\0', ':'])
            || relative
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return None;
        }
        let root = self.root.canonicalize().ok()?;
        let file = root.join(relative).canonicalize().ok()?;
        if !file.starts_with(root) || std::fs::metadata(&file).ok()?.len() > LIMIT {
            return None;
        }
        std::fs::read(file).ok()
    }
}
