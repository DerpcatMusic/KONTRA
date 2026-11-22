//! Shared sampler math and MIDI note parsing used across format translators.

/// Parses a MIDI note number (`0..=127`) from either an integer string or a
/// scientific pitch name (`C4` = 60, `C-1` = 0, `G9` = 127, case-insensitive).
///
/// Accepts `#` for sharps and `b`/`B` for flats, matching SFZ and DecentSampler.
#[must_use]
pub fn parse_midi_note(s: &str) -> Option<u8> {
    let s = s.trim();
    if let Ok(key) = s.parse::<u8>() {
        return (key <= 127).then_some(key);
    }
    let mut chars = s.chars();
    let letter: i32 = match chars.next()?.to_ascii_lowercase() {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, digits) = if let Some(digits) = rest.strip_prefix('#') {
        (1, digits)
    } else if let Some(digits) = rest.strip_prefix('b').or_else(|| rest.strip_prefix('B')) {
        (-1, digits)
    } else {
        (0, rest)
    };
    if digits.is_empty() || digits.starts_with('+') {
        return None;
    }
    let octave: i32 = digits.parse().ok()?;
    let key = octave
        .checked_add(1)?
        .checked_mul(12)?
        .checked_add(letter + shift)?;
    u8::try_from(key).ok().filter(|&k| k <= 127)
}

/// Converts decibels to linear amplitude gain (`10^(db / 20)`).
#[must_use]
pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Converts linear amplitude gain to decibels (`20 * log10(gain)`).
///
/// Non-positive gains return `f32::NEG_INFINITY`.
#[must_use]
pub fn gain_to_db(gain: f32) -> f32 {
    if gain <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * gain.log10()
    }
}

/// Converts cents to semitones (`cents / 100.0`).
#[must_use]
pub fn cents_to_semitones(cents: f32) -> f32 {
    cents / 100.0
}

/// Clamps a signed integer into the valid MIDI range `0..=127`.
#[must_use]
pub fn clamp_midi(v: i32) -> u8 {
    v.clamp(0, 127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numeric_and_scientific_midi_notes() {
        assert_eq!(parse_midi_note("0"), Some(0));
        assert_eq!(parse_midi_note("60"), Some(60));
        assert_eq!(parse_midi_note("127"), Some(127));
        assert_eq!(parse_midi_note("128"), None);
        assert_eq!(parse_midi_note("-1"), None);

        assert_eq!(parse_midi_note("C-1"), Some(0));
        assert_eq!(parse_midi_note("c-1"), Some(0));
        assert_eq!(parse_midi_note("Db-1"), Some(1));
        assert_eq!(parse_midi_note("DB-1"), Some(1));
        assert_eq!(parse_midi_note("C#3"), Some(49));
        assert_eq!(parse_midi_note("c#3"), Some(49));
        assert_eq!(parse_midi_note("C4"), Some(60));
        assert_eq!(parse_midi_note("A4"), Some(69));
        assert_eq!(parse_midi_note("G9"), Some(127));

        // Out-of-range scientific notes
        assert_eq!(parse_midi_note("G#9"), None);
        assert_eq!(parse_midi_note("Cb-1"), None);
        assert_eq!(parse_midi_note("C10"), None);
        assert_eq!(parse_midi_note("C-2"), None);
        assert_eq!(parse_midi_note(""), None);
        assert_eq!(parse_midi_note("H4"), None);
        assert_eq!(parse_midi_note("C+4"), None);
    }

    #[test]
    fn db_gain_round_trip_and_edge_cases() {
        assert!((db_to_gain(0.0) - 1.0).abs() < 1e-6);
        assert!((db_to_gain(-6.020_6) - 0.5).abs() < 1e-4);
        assert!((gain_to_db(1.0) - 0.0).abs() < 1e-6);
        assert!((gain_to_db(0.5) + 6.020_6).abs() < 1e-4);
        assert!(gain_to_db(0.0).is_infinite() && gain_to_db(0.0).is_sign_negative());
        assert!(gain_to_db(-1.0).is_infinite());
        assert_eq!(db_to_gain(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn cents_and_midi_clamping() {
        assert!((cents_to_semitones(50.0) - 0.5).abs() < 1e-6);
        assert!((cents_to_semitones(-1200.0) + 12.0).abs() < 1e-6);
        assert_eq!(clamp_midi(-50), 0);
        assert_eq!(clamp_midi(64), 64);
        assert_eq!(clamp_midi(200), 127);
    }
}
