//! Read-only discovery of installed Kontakt executables.
use std::path::{Path, PathBuf};

const WINDOWS: &[&str] = &[
    "Program Files/Native Instruments/Kontakt 8/Kontakt 8.exe",
    "Program Files/Native Instruments/Kontakt 7/Kontakt 7.exe",
    "Program Files/Common Files/VST3/Kontakt 8.vst3/Contents/x86_64-win/Kontakt 8.vst3",
];

/// Existing installed executables, in discovery order.
#[must_use]
pub fn candidates() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("KONTAKT_STOCK_EXECUTABLE") {
        return vec![PathBuf::from(path)];
    }
    let mut roots = Vec::new();
    let mut direct = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(programs) = std::env::var_os("ProgramFiles") {
            let programs = PathBuf::from(programs).join("Native Instruments");
            direct.push(programs.join("Kontakt 8/Kontakt 8.exe"));
            direct.push(programs.join("Kontakt 7/Kontakt 7.exe"));
        }
        if let Some(common) = std::env::var_os("CommonProgramFiles") {
            direct.push(
                PathBuf::from(common)
                    .join("VST3/Kontakt 8.vst3/Contents/x86_64-win/Kontakt 8.vst3"),
            );
        }
        if let Some(mut drive) = std::env::var_os("SystemDrive") {
            drive.push("\\");
            roots.push(PathBuf::from(drive));
        }
    } else if cfg!(target_os = "macos") {
        return ["8", "7"].into_iter().map(|version| PathBuf::from(format!(
            "/Applications/Native Instruments/Kontakt {version}/Kontakt {version}.app/Contents/MacOS/Kontakt {version}"
        ))).filter(|path| path.is_file()).collect();
    } else {
        if let Some(prefix) = std::env::var_os("WINEPREFIX") {
            roots.push(PathBuf::from(prefix).join("drive_c"));
        }
        if let Some(home) = dirs::home_dir() {
            for directory in [
                home.join(".local/share/wineprefixes"),
                home.join(".local/share/bottles/bottles"),
                home.join(".var/app/com.usebottles.bottles/data/bottles/bottles"),
            ] {
                roots.extend(
                    sorted_entries(&directory)
                        .into_iter()
                        .map(|prefix| prefix.join("drive_c")),
                );
            }
            for prefix in sorted_entries(&home) {
                let is_wine = prefix
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".wine"));
                if !is_wine {
                    continue;
                }
                roots.push(prefix.join("drive_c"));
                roots.extend(
                    sorted_entries(&prefix)
                        .into_iter()
                        .map(|slot| slot.join("drive_c")),
                );
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    direct
        .into_iter()
        .chain(
            roots
                .into_iter()
                .flat_map(|root| paths(&root).collect::<Vec<_>>()),
        )
        .filter(|path| path.is_file())
        .filter(|path| seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())))
        .collect()
}

fn paths(root: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let nested = sorted_entries(&root.join("Program Files/Common Files/VST3"))
        .into_iter()
        .map(|bundle| bundle.join("Kontakt 8/x64/Kontakt 8.exe"));
    WINDOWS
        .iter()
        .map(|relative| root.join(relative))
        .chain(nested)
}

/// A folder's entries in name order, so discovery picks the same install on every run.
fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|read| read.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default();
    found.sort();
    found
}

#[cfg(test)]
mod tests {
    #[test]
    fn wine_prefix_contains_standard_install_paths() {
        let paths: Vec<_> = super::paths(std::path::Path::new("/prefix/drive_c")).collect();
        assert!(paths.contains(&std::path::PathBuf::from(
            "/prefix/drive_c/Program Files/Native Instruments/Kontakt 8/Kontakt 8.exe"
        )));
    }
}
