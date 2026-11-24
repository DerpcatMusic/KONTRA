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
    preset_owner_kind(root, preset, snapshot, true)
}

fn preset_owner_kind(
    root: &std::path::Path,
    preset: &std::path::Path,
    snapshot: bool,
    strip_extension: bool,
) -> Option<String> {
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
    } else if strip_extension {
        preset.file_stem()?
    } else {
        preset.file_name()?
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
    preset_family_for(root, presets, &[], current)
}

/// Like [`preset_family`], but also treats sorted indices in `recalls`
/// (excluding `"User/"` user-saved presets) as snapshots even when file
/// extensions were stripped for display (e.g. `Snapshots/Conflux/Keys/Pad`).
#[must_use]
pub fn preset_family_for<P: AsRef<std::path::Path>>(
    root: &std::path::Path,
    presets: &[P],
    recalls: &[usize],
    current: Option<usize>,
) -> Vec<usize> {
    preset_family_by(root, presets, recalls, current, false)
}

/// Groups the editor's display paths, whose file extensions were already removed.
#[must_use]
pub fn displayed_preset_family<P: AsRef<std::path::Path>>(
    root: &std::path::Path,
    presets: &[P],
    recalls: &[usize],
    current: Option<usize>,
) -> Vec<usize> {
    preset_family_by(root, presets, recalls, current, true)
}

fn preset_family_by<P: AsRef<std::path::Path>>(
    root: &std::path::Path,
    presets: &[P],
    recalls: &[usize],
    current: Option<usize>,
    displayed: bool,
) -> Vec<usize> {
    let key = |idx: usize, p: &std::path::Path| {
        let is_user = p
            .components()
            .next()
            .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case("user"));
        let snapshot = (!displayed
            && p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nksn")))
            || (!is_user && recalls.binary_search(&idx).is_ok());
        preset_owner_kind(root, p, snapshot, !displayed)
    };
    let all = || (0..presets.len()).collect();
    let Some(mine) = current.and_then(|c| key(c, presets.get(c)?.as_ref())) else {
        return all();
    };
    let family: Vec<usize> = (presets.iter().enumerate())
        .filter(|(idx, p)| key(*idx, p.as_ref()).as_deref() == Some(mine.as_str()))
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

    #[test]
    fn dotted_instrument_names_group_from_files_and_display_paths() {
        let root = std::path::Path::new("");
        let files = [
            "Instruments/Piano v1.2.NKI",
            "Instruments/Other.nki",
            "Snapshots/Piano v1.2/Soft.nksn",
            "Snapshots/Piano v1.2/Loud.nksn",
        ];
        let displayed = [
            "Instruments/Piano v1.2",
            "Instruments/Other",
            "Snapshots/Piano v1.2/Soft",
            "Snapshots/Piano v1.2/Loud",
        ];
        assert_eq!(preset_family(root, &files, Some(2)), [0, 2, 3]);
        assert_eq!(
            super::displayed_preset_family(root, &displayed, &[2, 3], Some(2)),
            [0, 2, 3]
        );
    }

    #[test]
    fn extension_stripped_snapshots_group_with_their_instrument() {
        let presets = [
            "Instruments/Conflux",
            "Instruments/Other",
            "Snapshots/Conflux/Keys/Pad",
            "Snapshots/Conflux/Bass/Sub",
            "User Content/Conflux/Custom",
            "Snapshots/Other/Warm",
        ];
        let recalls = [2, 3, 4, 5];
        let root = std::path::Path::new("");
        assert_eq!(
            super::preset_family_for(root, &presets, &recalls, Some(0)),
            [0, 2, 3, 4]
        );
        assert_eq!(
            super::preset_family_for(root, &presets, &recalls, Some(2)),
            [0, 2, 3, 4]
        );
        assert_eq!(
            super::preset_family_for(root, &presets, &recalls, Some(1)),
            [1, 5]
        );
    }
}
