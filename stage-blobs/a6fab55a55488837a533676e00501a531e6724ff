//! What a sample's file name (and the folders above it) says about it: the
//! note, the velocity layer, the round robin and the microphone, and what is
//! left over, which names the instrument.

/// What one path says. Notes are MIDI numbers in KONTRA's (and Kontakt's)
/// naming, where C3 is 60; `octave_shift` says how a note name was read.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Parsed {
    /// MIDI root from a note name, read with C3 = 60.
    pub note_name: Option<i32>,
    /// MIDI root from a bare or prefixed number (`_060_`, `midi60`).
    pub note_number: Option<i32>,
    /// Velocity layer: an ordinal (`v1`, `pp`) or an upper velocity (`vel127`).
    pub velocity: Option<Velocity>,
    pub round_robin: Option<u32>,
    pub mic: Option<String>,
    /// The words left: the instrument's name.
    pub rest: Vec<String>,
    /// Numbers that could have been the note or a layer, for the report.
    pub ambiguous: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Velocity {
    /// Layer order only: `v1` < `v2`, `pp` < `ff`.
    Ordinal(u32),
    /// The layer's top velocity, 1..=127.
    Upper(u8),
}

impl Parsed {
    /// The root, a note name before a number.
    pub fn note(&self) -> Option<i32> {
        self.note_name.or(self.note_number)
    }
}

const DYNAMICS: [&str; 8] = ["ppp", "pp", "p", "mp", "mf", "f", "ff", "fff"];
const MICS: [&str; 22] = [
    "close", "cl", "near", "spot", "room", "amb", "ambient", "far", "mid", "overhead", "overheads", "oh", "tree",
    "decca", "outrigger", "outriggers", "surround", "di", "amp", "mix", "stage", "hall",
];

/// Parse `components`: folders from the sample root down, then the file
/// stem. A folder that is only a mic, layer or round robin counts as one.
pub fn parse(components: &[&str]) -> Parsed {
    let mut out = Parsed::default();
    let mut numbers = Vec::new();
    for (n, part) in components.iter().enumerate() {
        let last = n + 1 == components.len();
        for word in words(part) {
            if !classify(&word, &mut out) {
                // A bare number in the file name may be the note; elsewhere it is a name.
                match word.parse::<i32>() {
                    Ok(v) if last && (0..=127).contains(&v) && word.len() >= 2 => numbers.push(word),
                    _ => out.rest.push(word),
                }
            }
        }
    }
    // The first bare number is the note, unless the name gave one.
    let mut numbers = numbers.into_iter();
    if out.note().is_none() {
        out.note_number = numbers.next().and_then(|w| w.parse().ok());
    }
    out.ambiguous.extend(numbers);
    // Folder and file often repeat the name: "Piano/Piano_C3".
    out.rest.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    out
}

/// Words of a path component: split on separators and between a word and a
/// tag glued to it ("PianoC3" stays one word; "Piano_C3" is two).
fn words(part: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = part.chars().peekable();
    while let Some(c) = chars.next() {
        let word = out.last_mut().unwrap();
        // The minus of a negative octave (F#-1) stays with its note.
        let negative_octave = c == '-'
            && chars.peek().is_some_and(char::is_ascii_digit)
            && note_name(&format!("{word}0")).is_some();
        if !negative_octave && (c.is_whitespace() || matches!(c, '_' | '-' | '.' | '(' | ')' | '[' | ']' | ',' | '+')) {
            out.push(String::new());
        } else {
            word.push(c);
        }
    }
    out.retain(|w| !w.is_empty());
    out
}

/// Take `word` into `out` if it is a tag; false if it is part of the name.
fn classify(word: &str, out: &mut Parsed) -> bool {
    let lower = word.to_lowercase();
    if let Some(note) = note_name(word) {
        out.note_name = out.note_name.or(Some(note));
        return true;
    }
    for prefix in ["midi", "note", "nn", "m", "n"] {
        if let Some(v) = number_after(&lower, prefix).filter(|v| (0..=127).contains(v)) {
            out.note_number = out.note_number.or(Some(v));
            return true;
        }
    }
    for prefix in ["velocity", "vel", "vl", "v"] {
        if let Some(v) = number_after(&lower, prefix) {
            // A small number is a layer; a large one a velocity.
            let layer = if v > 16 { Velocity::Upper(v.clamp(1, 127) as u8) } else { Velocity::Ordinal(v as u32) };
            out.velocity = out.velocity.or(Some(layer));
            return true;
        }
    }
    if let Some(at) = DYNAMICS.iter().position(|d| *d == lower) {
        out.velocity = out.velocity.or(Some(Velocity::Ordinal(at as u32)));
        return true;
    }
    for prefix in ["roundrobin", "rr", "seq"] {
        if let Some(v) = number_after(&lower, prefix) {
            out.round_robin = out.round_robin.or(Some(v as u32));
            return true;
        }
    }
    // A lone letter a-h is a round robin take: "_a", "_b".
    if lower.len() == 1 && ('a'..='h').contains(&lower.chars().next().unwrap()) {
        out.round_robin = out.round_robin.or(Some(lower.as_bytes()[0] as u32 - b'a' as u32 + 1));
        return true;
    }
    if MICS.contains(&lower.as_str()) {
        out.mic = out.mic.take().or(Some(word.to_owned()));
        return true;
    }
    false
}

/// `prefix` followed only by digits: their value.
fn number_after(word: &str, prefix: &str) -> Option<i32> {
    let digits = word.strip_prefix(prefix)?;
    (!digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok())?
}

/// A note name with its octave: C3, c#3, Db3, F#-1, Bb2, Cs3. C3 is 60.
pub fn note_name(word: &str) -> Option<i32> {
    let mut chars = word.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let class = match letter {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = chars.collect();
    let (shift, octave) = if let Some(r) = rest.strip_prefix(['#', '♯', 's']) {
        (1, r)
    } else if let Some(r) = rest.strip_prefix(['b', '♭']).filter(|r| !r.is_empty()) {
        (-1, r)
    } else {
        (0, rest.as_str())
    };
    let ok = !octave.is_empty()
        && octave.len() <= 2
        && octave.trim_start_matches('-').bytes().all(|b| b.is_ascii_digit())
        && octave.trim_start_matches('-').len() == 1;
    let octave: i32 = octave.parse().ok().filter(|_| ok)?;
    let midi = (octave + 2) * 12 + class + shift;
    (0..=127).contains(&midi).then_some(midi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_parse() {
        use Velocity::*;
        // (path components, note, velocity, round robin, mic, rest)
        let cases: &[(&[&str], Option<i32>, Option<Velocity>, Option<u32>, Option<&str>, &str)] = &[
            (&["Piano_C3"], Some(60), None, None, None, "Piano"),
            (&["Piano_C#3_v2"], Some(61), Some(Ordinal(2)), None, None, "Piano"),
            (&["Piano Db3 pp"], Some(61), Some(Ordinal(1)), None, None, "Piano"),
            (&["Cello_060_rr2"], Some(60), None, Some(2), None, "Cello"),
            (&["Cello-MIDI60-vel127"], Some(60), Some(Upper(127)), None, None, "Cello"),
            (&["Harp", "Close", "Harp_A2_ff_b"], Some(57), Some(Ordinal(6)), Some(2), Some("Close"), "Harp"),
            (&["Bass_F#-1"], Some(18), None, None, None, "Bass"),
            (&["Bells_Bb4_mf_RR3"], Some(82), Some(Ordinal(4)), Some(3), None, "Bells"),
            (&["Kick"], None, None, None, None, "Kick"),
            (&["Pad_Cs2_vl1_room"], Some(49), Some(Ordinal(1)), None, Some("room"), "Pad"),
            (&["Glass_72_100"], Some(72), None, None, None, "Glass"),
            (&["Strings", "Legato_E3"], Some(64), None, None, None, "Strings Legato"),
        ];
        for (components, note, velocity, rr, mic, rest) in cases {
            let p = parse(components);
            assert_eq!(p.note(), *note, "{components:?}");
            assert_eq!(p.velocity, *velocity, "{components:?}");
            assert_eq!(p.round_robin, *rr, "{components:?}");
            assert_eq!(p.mic.as_deref(), *mic, "{components:?}");
            assert_eq!(p.rest.join(" "), *rest, "{components:?}");
        }
        assert_eq!(parse(&["Glass_72_100"]).ambiguous, ["100"], "a second number is reported");
        assert_eq!(note_name("B3"), Some(71));
        assert_eq!(note_name("b"), None, "a bare letter is not a note");
        assert_eq!(note_name("Bass"), None);
    }
}
