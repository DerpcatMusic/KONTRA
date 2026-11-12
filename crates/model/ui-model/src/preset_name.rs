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
