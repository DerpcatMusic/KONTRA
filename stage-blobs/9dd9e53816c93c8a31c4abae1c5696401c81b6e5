//! Microphone positions inferred from a translated instrument. Sources have no
//! mic object: libraries spell them as buses their groups play through, or as
//! a position word in the group's name ("Brass_Legato_Close").

use sampler_ir as ir;

/// Words that name a position, matched whole against a group name's tokens.
const POSITIONS: &[&str] = &[
    "close", "room", "tree", "surround", "ambient", "ambience", "amb", "spot", "mix", "decca", "hall", "midhall",
    "mid", "wide", "stage", "outrigger", "outriggers", "mic", "far", "rear", "overhead", "diffuse", "balcony", "direct",
    "flank", "flanks", "front", "back", "main", "cls", "dcc", "fm", "fmp", "fr", "otrggr", "spt",
];

/// A position word's full name, for the abbreviations libraries use.
fn expand(word: &str) -> &str {
    match word {
        "cls" => "close",
        "dcc" => "decca",
        "fm" => "full mix",
        "spt" => "spot",
        "otrggr" => "outriggers",
        other => other,
    }
}

/// Words of a name: split at punctuation and where a lowercase letter meets a capital.
fn words(name: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<char> = None;
    for (i, c) in name.char_indices() {
        let boundary = !c.is_alphanumeric() || prev.is_some_and(|p| (p.is_lowercase() || p.is_ascii_digit()) && c.is_uppercase());
        if boundary {
            if start < i {
                out.push(&name[start..i]);
            }
            start = if c.is_alphanumeric() { i } else { i + c.len_utf8() };
        }
        prev = Some(c);
    }
    if start < name.len() {
        out.push(&name[start..]);
    }
    out
}

/// Buses that carry effects, not a position.
fn effect_bus(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "insert" || name == "main" || name == "master" || name.starts_with("send") || name.starts_with("aux")
}

/// The position a group name spells: its last positional word, digits kept
/// ("Spt2") so numbered spots stay apart.
fn position(name: &str) -> Option<String> {
    words(name).into_iter().rev().find_map(|w| {
        let word = w.trim_end_matches(|c: char| c.is_ascii_digit()).to_ascii_lowercase();
        POSITIONS.contains(&word.as_str()).then(|| format!("{}{}", expand(&word), &w[word.len()..]))
    })
}

/// Mic names and their groups, from (in order) the buses groups play through,
/// then position words in group names. Empty when fewer than two positions
/// emerge or fewer than half the groups carry one.
// ponytail: script mic mixers (UI widgets driving group volume) are not read.
pub fn infer(instrument: &ir::Instrument) -> Vec<(String, Vec<usize>)> {
    let mut by_bus: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, group) in instrument.groups.iter().enumerate() {
        let ir::Output::Bus(bus) = group.output else { continue };
        let name = &instrument.buses[bus.0].name;
        if effect_bus(name) {
            continue;
        }
        match by_bus.iter_mut().find(|(n, _)| n == name) {
            Some((_, groups)) => groups.push(i),
            None => by_bus.push((name.clone(), vec![i])),
        }
    }
    if by_bus.len() >= 2 {
        return by_bus;
    }
    let mut by_name: Vec<(String, Vec<usize>)> = Vec::new();
    for (i, group) in instrument.groups.iter().enumerate() {
        let Some(mic) = position(&group.name) else { continue };
        match by_name.iter_mut().find(|(n, _)| *n == mic) {
            Some((_, groups)) => groups.push(i),
            None => by_name.push((mic, vec![i])),
        }
    }
    let matched: usize = by_name.iter().map(|(_, g)| g.len()).sum();
    if by_name.len() < 2 || matched * 2 < instrument.groups.len() {
        return Vec::new();
    }
    for (name, _) in &mut by_name {
        if let Some(first) = name.get_mut(..1) {
            first.make_ascii_uppercase();
        }
    }
    by_name
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(names: &[&str]) -> ir::Instrument {
        let mut i = ir::Instrument::default();
        for n in names {
            i.groups.push(ir::Group { name: (*n).into(), ..Default::default() });
        }
        i
    }

    #[test]
    fn mics_come_from_group_name_positions() {
        let i = named(&["BRASS_Breath_Close", "BRASS_Breath_Decca", "BRASS_Breath_Hall", "BRASS_Sus_Close", "BRASS_Sus_Decca", "BRASS_Sus_Hall"]);
        let mics = infer(&i);
        assert_eq!(mics.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Close", "Decca", "Hall"]);
        assert_eq!(mics[0].1, [0, 3]);
    }

    #[test]
    fn abbreviations_in_camel_case_names_are_positions() {
        let i = named(&["WmnLgtAhClsDyn1RR1", "WmnLgtAhDccDyn1RR1", "WmnLgtAhFMpDyn1RR1", "1VlnLgtSpt1Dyn1RR1", "1VlnLgtSpt2Dyn1RR1"]);
        let names: Vec<_> = infer(&i).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["Close", "Decca", "Fmp", "Spot1", "Spot2"]);
    }

    #[test]
    fn one_position_or_unlabelled_groups_have_no_mics() {
        assert!(infer(&named(&["a_Close", "b_Close"])).is_empty());
        assert!(!infer(&named(&["Legato", "Staccato", "a_Close", "b_Room"])).is_empty());
        assert!(infer(&named(&["Legato", "Staccato", "Marcato", "a_Close", "b_Room"])).is_empty());
    }

    #[test]
    fn groups_on_separate_buses_are_mics() {
        let mut i = named(&["a", "b", "c"]);
        for (n, name) in ["Close", "Room"].into_iter().enumerate() {
            i.buses.push(ir::Bus { name: name.into(), chain: None, sends: Vec::new(), output: ir::Output::Master, gain: ir::Gain::UNITY });
            i.groups[n].output = ir::Output::Bus(ir::BusRef(n));
        }
        let mics = infer(&i);
        assert_eq!(mics.len(), 2);
        assert_eq!(mics[1], ("Room".to_string(), vec![1]));
    }

    /// How many real instruments get mic nodes. `KONTRA_KONTAKT_LIBRARIES=… cargo test -- --ignored --nocapture census`.
    #[test]
    #[ignore]
    fn census() {
        let root = std::env::var("KONTRA_KONTAKT_LIBRARIES").unwrap();
        let mut stack = vec![std::path::PathBuf::from(root)];
        let (mut total, mut with) = (0, 0);
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !p.to_string_lossy().contains("recovery") {
                        stack.push(p)
                    }
                } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) {
                    let Ok(k) = sampler_kontakt::read(&p) else { continue };
                    total += 1;
                    let mics = infer(&k.instrument);
                    if !mics.is_empty() {
                        with += 1;
                        println!("MICS\t{}\t{:?}", p.display(), mics.iter().map(|(n, g)| (n.as_str(), g.len())).collect::<Vec<_>>());
                    }
                }
            }
        }
        println!("MICS total={total} with_mics={with}");
    }
}
