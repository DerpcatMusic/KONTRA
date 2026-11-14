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

/// The presets Kontakt's instrument header lists while `current` plays:
/// the instrument and its snapshots, by index into `presets`. A snapshot
/// belongs to the instrument its folder names (`Snapshots/<instrument>/`,
/// Kontakt's user `<instrument>/`), any case. An instrument without
/// snapshots, or nothing playing, lists them all.
#[must_use]
pub fn preset_family<P: AsRef<std::path::Path>>(
    presets: &[P],
    current: Option<usize>,
) -> Vec<usize> {
    fn key(p: &std::path::Path) -> Option<String> {
        let snapshot = p
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nksn"));
        let named = if snapshot {
            p.parent()?.file_name()?
        } else {
            p.file_stem()?
        };
        Some(named.to_str()?.to_lowercase())
    }
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
        ];
        assert_eq!(preset_family(&presets, Some(0)), [0, 2, 4]);
        // From a snapshot too.
        assert_eq!(preset_family(&presets, Some(3)), [1, 3]);
        // No snapshots, or nothing playing: the whole library.
        assert_eq!(preset_family(&presets, Some(5)).len(), 6);
        assert_eq!(preset_family(&presets, None).len(), 6);
    }
}
