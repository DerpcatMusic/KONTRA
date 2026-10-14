//! Print a line range of one instrument script (RAM only, to the terminal).
//! Usage: show_script PATH PROGRAM|- BEHAVIOR FROM|state TO [GREP]
//! `PROGRAM` is the multi program index, `-` for an .nki. With GREP, prints
//! every line containing it (with its number) instead of a range.
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = Path::new(&a[1]);
    let kontakt = if a[2] == "-" {
        sampler_kontakt::read(path)
    } else {
        sampler_kontakt::read_program(path, a[2].parse().unwrap())
    }
    .expect("read");
    let names: Vec<_> = kontakt.instrument.behaviors.iter().map(|b| b.name.as_str()).collect();
    let Some(behavior) = kontakt.instrument.behaviors.iter().find(|b| b.name == a[3]) else {
        println!("behaviors: {names:?}");
        return;
    };
    // `state 0 GREP`: the persisted values whose variable name contains GREP.
    if a[4] == "state" {
        for (name, saved) in behavior.state.iter().filter(|(n, _)| a.get(6).is_none_or(|g| n.contains(g.as_str()))) {
            println!("{name} = {saved:?}");
        }
        return;
    }
    let (from, to): (usize, usize) = (a[4].parse().unwrap(), a[5].parse().unwrap());
    for (i, line) in behavior.source.lines().enumerate() {
        let n = i + 1;
        match a.get(6) {
            Some(g) if line.contains(g.as_str()) => println!("{n}: {line}"),
            None if (from..=to).contains(&n) => println!("{n}: {line}"),
            _ => {}
        }
    }
}
