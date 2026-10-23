//! Naming grammar of Modo Drum pack members. See `docs/re/formats/ikmpak.md`.
//!
//! `…/<folders>/[<NOTE>_]v<VEL>_rr<RR>[ _]<name>-<MIC>.wav`

/// Which recorded layer a member is. The pack names two per hit: `DRY` and `WET`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mic {
    Dry,
    Wet,
}

/// A pack member parsed from its path. Borrows from the path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Member<'a> {
    /// Folders above the file, outermost first. The last one is the articulation.
    pub folders: &'a str,
    /// Key name as written in the file name (`E2`, `C#1`); absent for many cymbals.
    pub note: Option<&'a str>,
    pub velocity: u8,
    pub round_robin: u16,
    /// Hit name after the round-robin index, without the mic suffix.
    pub name: &'a str,
    pub mic: Mic,
}

impl Member<'_> {
    /// The innermost folder, e.g. `Hit`, `Choke` or `Hit Hat Foot Open`.
    pub fn articulation(&self) -> Option<&str> {
        self.folders.rsplit('/').next()
    }
}

/// Parses a pack path. `None` when the path does not follow the grammar.
pub fn parse_member(path: &str) -> Option<Member<'_>> {
    let (folders, file) = path.rsplit_once('/')?;
    let stem = file.strip_suffix(".wav")?;
    let (head, mic) = stem.rsplit_once('-')?;
    let mic = match mic {
        "DRY" => Mic::Dry,
        "WET" => Mic::Wet,
        _ => return None,
    };
    let mut rest = head.trim_start_matches([' ', '_']);

    let mut note = None;
    if let Some((tok, after)) = rest.split_once('_')
        && note_number(tok).is_some()
    {
        note = Some(tok);
        rest = after;
    }

    let rest = rest.strip_prefix(['v', 'V'])?;
    let vel_end = rest.find(|c: char| !c.is_ascii_digit())?;
    let velocity: u8 = rest[..vel_end].parse().ok()?;
    let rest = rest[vel_end..].strip_prefix('_')?;
    let rest = rest
        .get(..2)
        .filter(|t| t.eq_ignore_ascii_case("rr"))
        .map(|_| &rest[2..])?;
    let rr_end = rest.find(|c: char| !c.is_ascii_digit())?;
    let round_robin: u16 = rest[..rr_end].parse().ok()?;
    let name = rest[rr_end..].trim_start_matches([' ', '_']);

    Some(Member {
        folders,
        note,
        velocity,
        round_robin,
        name,
        mic,
    })
}

/// MIDI number of a key name such as `C3`, `C#2`, `Bb-1`, with C3 = 60.
///
/// The packs carry no key map, so the octave numbering is an assumption
/// (the Kontakt/IK convention). Check it against the product before relying on it.
pub fn note_number(name: &str) -> Option<u8> {
    let mut chars = name.chars();
    let base: i32 = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (acc, rest) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, rest),
    };
    let octave: i32 = rest.parse().ok()?;
    let n = (octave + 2) * 12 + base + acc;
    u8::try_from(n).ok().filter(|n| *n <= 127)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grammar_variants() {
        let m = parse_member("Cymbals/Crash/Hit/E2_v127_rr1 China Cymbal Choke-DRY.wav").unwrap();
        assert_eq!(
            (m.note, m.velocity, m.round_robin, m.name, m.mic),
            (Some("E2"), 127, 1, "China Cymbal Choke", Mic::Dry)
        );
        assert_eq!(m.articulation(), Some("Hit"));

        // Leading space, sharp note, double-underscore separator, name keeps its own '-'.
        let m = parse_member("Kits/Metal/Toms/Rack 3/ B1_v127_rr10 Rack 3 Rimshot-Off-WET.wav")
            .unwrap();
        assert_eq!(
            (m.note, m.round_robin, m.name, m.mic),
            (Some("B1"), 10, "Rack 3 Rimshot-Off", Mic::Wet)
        );

        // No key in the name, underscore before the velocity.
        let m = parse_member(
            "Cymbals/Hi-Hats/HH/HH Foot Open/_V127_rr1 Pearl-Hit Hat Foot Open-DRY.wav",
        )
        .unwrap();
        assert_eq!(
            (m.note, m.velocity, m.name),
            (None, 127, "Pearl-Hit Hat Foot Open")
        );

        assert_eq!(parse_member("Cymbals/x/y/E2_v127_rr1 Name-OTHER.wav"), None);
        assert_eq!(parse_member("Cymbals/x/y/E2_v127_rr1 Name-DRY.aif"), None);
    }

    #[test]
    fn note_numbers_with_c3_60() {
        assert_eq!(note_number("C3"), Some(60));
        assert_eq!(note_number("E2"), Some(52));
        assert_eq!(note_number("C#-2"), Some(1));
        assert_eq!(note_number("Bb1"), Some(46));
        assert_eq!(note_number("H1"), None);
    }
}
