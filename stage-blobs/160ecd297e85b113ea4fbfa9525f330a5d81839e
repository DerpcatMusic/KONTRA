//! Survey aid, ignored: what a program's scripts do per key. Prints names and
//! counts to the terminal only. `KONTRA_PROBE='<bank path>::<program>'`,
//! `KONTRA_PROBE_SCRIPT=1` prints the program's script text.
#![cfg(feature = "library-access")]
use sampler_uvi::script::{Command, Config, ScriptHost};

#[test]
#[ignore]
fn probe_program() {
    let spec = std::env::var("KONTRA_PROBE").unwrap();
    let (path, program) = spec.split_once("::").unwrap();
    let bank = sampler_uvi::Bank::open(std::path::Path::new(path)).unwrap();
    let (text, _) = bank.program(program).unwrap();
    let doc = roxmltree::Document::parse(&text).unwrap();
    let mut tags = std::collections::BTreeMap::<String, usize>::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        *tags.entry(n.tag_name().name().to_owned()).or_default() += 1;
    }
    println!("TAGS {tags:?}");
    if std::env::var_os("KONTRA_PROBE_SCRIPT").is_some() {
        for n in doc.descendants().filter(|n| n.has_tag_name("ScriptProcessor")) {
            let attrs: Vec<_> = n.attributes().map(|a| format!("{}={}", a.name(), a.value().chars().take(40).collect::<String>())).collect();
            println!("SP {attrs:?}");
            for c in n.children().filter(|c| c.is_element()) {
                if c.tag_name().name() == "script" {
                    println!("---- script ----\n{}", c.text().unwrap_or(""));
                } else {
                    println!("child {} {:?}", c.tag_name().name(), c.attributes().map(|a| format!("{}={}", a.name(), a.value())).collect::<Vec<_>>());
                }
            }
        }
    }
    let mut host = ScriptHost::new(&text, bank.scripts(), Config::default()).unwrap();
    println!("INIT findings {:?}", host.findings());
    let mut playing = Vec::new();
    for key in 0..128u8 {
        for vel in [100u8] {
            host.note_on(1000 + u64::from(key), key, vel, 0);
            host.advance(10.0 * f64::from(key) + 1.0);
            let cmds = host.take_commands();
            let plays: Vec<_> = cmds.iter().filter_map(|c| if let Command::Play(p) = c { Some((p.key, p.osc, p.layers.0, p.duration_ms)) } else { None }).collect();
            if !plays.is_empty() {
                playing.push((key, plays));
            }
            host.note_off(1000 + u64::from(key), key, 0, 0);
            host.advance(10.0 * f64::from(key) + 5.0);
            host.take_commands();
        }
    }
    println!("KEYS playing {}: {:?}", playing.len(), playing.iter().map(|(k, p)| (*k, p.len())).collect::<Vec<_>>());
    if let Some((k, p)) = playing.first() {
        println!("first {k}: {p:?}");
    }
    println!("FINDINGS {:?}", host.findings());
}
