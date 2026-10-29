//! Read-only discovery of installed Kontakt executables.
use std::path::{Path, PathBuf};

const WINDOWS: &[&str] = &[
    "Program Files/Native Instruments/Kontakt 8/Kontakt 8.exe",
    "Program Files/Native Instruments/Kontakt 7/Kontakt 7.exe",
    "Program Files/Common Files/VST3/Kontakt 8.vst3/Contents/x86_64-win/Kontakt 8.vst3",
];

pub(super) fn candidates() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("KONTAKT_STOCK_EXECUTABLE") {
        return vec![PathBuf::from(path)];
    }
    let mut roots = Vec::new();
    if cfg!(target_os = "windows") {
        if let Some(mut drive) = std::env::var_os("SystemDrive") {
            drive.push("\\");
            roots.push(PathBuf::from(drive));
        }
    } else if cfg!(target_os = "macos") {
        return vec![PathBuf::from(
            "/Applications/Native Instruments/Kontakt 8/Kontakt 8.app/Contents/MacOS/Kontakt 8",
        )];
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
                if let Ok(prefixes) = std::fs::read_dir(directory) {
                    roots.extend(
                        prefixes
                            .flatten()
                            .map(|prefix| prefix.path().join("drive_c")),
                    );
                }
            }
            if let Ok(entries) = std::fs::read_dir(home) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    if !name.to_string_lossy().starts_with(".wine") {
                        continue;
                    }
                    let prefix = entry.path();
                    roots.push(prefix.join("drive_c"));
                    if let Ok(slots) = std::fs::read_dir(prefix) {
                        roots.extend(slots.flatten().map(|slot| slot.path().join("drive_c")));
                    }
                }
            }
        }
    }
    roots
        .into_iter()
        .flat_map(|root| paths(&root).collect::<Vec<_>>())
        .filter(|path| path.is_file())
        .collect()
}

fn paths(root: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let bundles = root.join("Program Files/Common Files/VST3");
    let nested = std::fs::read_dir(bundles)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("Kontakt 8/x64/Kontakt 8.exe"))
        .collect::<Vec<_>>();
    WINDOWS
        .iter()
        .map(|relative| root.join(relative))
        .chain(nested)
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
