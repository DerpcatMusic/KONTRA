//! A player's preset name as the file it is saved as, so the editor and the
//! host agree on which names collide.

/// The longest name kept, in characters.
const NAME: usize = 64;

/// What the player typed, as a file name: no separators, control or
/// reserved characters, no leading dot, at most 64 characters.
#[must_use]
pub fn preset_file_name(name: &str) -> Option<String> {
    let kept: String = (name.chars())
        .filter(|c| !c.is_control() && !r#"/\:*?"<>|"#.contains(*c))
        .take(NAME)
        .collect();
    let kept = kept.trim().trim_start_matches('.').trim();
    (!kept.is_empty()).then(|| kept.to_owned())
}

/// The instrument `preset` belongs to, lower case: an instrument its own
/// name, a snapshot the instrument its folder names. Below `root` (the
/// library's folder; paths may also be relative to it) that is the folder
/// after a leading `Snapshots`, however deep its category folders go
/// (Conflux's `Snapshots/Conflux/Keys/`), else its own folder (Kontakt's
/// user `<instrument>/`), any case. A `Snapshots` folder above the library
/// root names nothing.
#[must_use]
pub fn preset_owner(root: &std::path::Path, preset: &std::path::Path) -> Option<String> {
    let snapshot = preset
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
    let named = if snapshot {
        let inside = preset.strip_prefix(root).unwrap_or(preset);
        let mut folders = inside.parent()?.iter();
        let library = folders
            .next()
            .is_some_and(|f| f.eq_ignore_ascii_case("snapshots"));
        match folders.next() {
            Some(instrument) if library => instrument,
            _ => preset.parent()?.file_name()?,
        }
    } else {
        preset.file_stem()?
    };
    Some(named.to_str()?.to_lowercase())
}

/// The presets Kontakt's instrument header lists while `current` plays:
/// the instrument and its snapshots ([`preset_owner`]), by index into
/// `presets`. An instrument without snapshots, or nothing playing, lists
/// them all.
#[must_use]
pub fn preset_family<P: AsRef<std::path::Path>>(
    root: &std::path::Path,
    presets: &[P],
    current: Option<usize>,
) -> Vec<usize> {
    let key = |p: &std::path::Path| preset_owner(root, p);
    let all = || (0..presets.len()).collect();
    let Some(mine) = current
        .and_then(|c| presets.get(c))
        .and_then(|p| key(p.as_ref()))
    else {
        return all();
    };
    let family: Vec<usize> = (presets.iter().enumerate())
        .filter(|(_, p)| key(p.as_ref()).as_deref() == Some(mine.as_str()))
        .map(|(n, _)| n)
        .collect();
    if family.len() > 1 { family } else { all() }
}

#[cfg(test)]
mod family_tests {
    use super::preset_family;

    #[test]
    fn an_instrument_lists_itself_and_its_snapshots() {
        let presets = [
            "Instruments/Felt.nki",
            "Instruments/Cotton.nki",
            "Snapshots/Felt/Harmonic Crackles.nksn",
            "Snapshots/Cotton/Intimate.nksn",
            "/kontakt/felt/Mine.nksn",
            "Instruments/Lone.nki",
            "Snapshots/Felt/Keys/Deep.nksn",
        ];
        let root = std::path::Path::new("");
        assert_eq!(preset_family(root, &presets, Some(0)), [0, 2, 4, 6]);
        // From a snapshot too.
        assert_eq!(preset_family(root, &presets, Some(3)), [1, 3]);
        // No snapshots, or nothing playing: the whole library.
        assert_eq!(preset_family(root, &presets, Some(5)).len(), 7);
        assert_eq!(preset_family(root, &presets, None).len(), 7);
    }

    #[test]
    fn a_snapshots_folder_above_the_library_names_nothing() {
        // The library lives under a `Snapshots` folder of the player's:
        // only its own `Snapshots/Felt/` names the instrument.
        let root = std::path::Path::new("/Snapshots/Lib");
        let presets = [
            "/Snapshots/Lib/Instruments/Felt.nki",
            "/Snapshots/Lib/Instruments/Lib.nki",
            "/Snapshots/Lib/Snapshots/Felt/Soft.nksn",
            "/Snapshots/Lib/Extra/Felt/Loud.nksn",
        ];
        assert_eq!(preset_family(root, &presets, Some(0)), [0, 2, 3]);
        assert_eq!(preset_family(root, &presets, Some(1)).len(), 4);
    }
}
