//! Single-program topology and dropped-module probe; vendor text stays in memory.
#![cfg(feature = "library-access")]
#[test]
#[ignore = "one installed-program metadata probe; approved reader and kontakto-heavy required"]
fn structure_probe() {
    use std::{
        collections::{BTreeMap, BTreeSet},
        io::Write,
    };
    let item = std::env::var("UVI_AUDIT_ITEM").unwrap();
    let (path, program) = item.split_once("::").unwrap();
    let banks = [(path.to_owned(), vec![program.to_owned()])];
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::var("UVI_AUDIT_STRUCTURE_OUT").unwrap())
        .unwrap();
    for (path, programs) in banks {
        let bank = sampler_uvi::Bank::open(std::path::Path::new(&path)).unwrap();
        for program in programs {
            let (xml, _) = bank.program(&program).unwrap();
            let doc = sampler_uvi::parse_program_xml(&xml).unwrap();
            let ir = sampler_uvi::translate_program(&bank, &program).unwrap();
            let mut active = BTreeMap::<String, usize>::new();
            let mut attrs = BTreeSet::new();
            let mut stacked = 0usize;
            let mut keygroups = 0usize;
            for n in doc.descendants().filter(|n| n.is_element()) {
                let tag = n.tag_name().name();
                if matches!(tag, "Program" | "Layer" | "Keygroup" | "SamplePlayer") {
                    for a in n.attributes() {
                        attrs.insert(format!("{tag}.{}", a.name()));
                    }
                }
                if tag == "Keygroup" {
                    keygroups += 1;
                    stacked += usize::from(
                        n.children()
                            .filter(|c| c.has_tag_name("Oscillators"))
                            .flat_map(|c| c.children())
                            .filter(|c| c.has_tag_name("SamplePlayer"))
                            .count()
                            > 1,
                    );
                }
                if n.parent().is_some_and(|p| p.has_tag_name("Inserts"))
                    && n.attribute("Bypass") != Some("1")
                {
                    let scope = n.parent().unwrap().parent().unwrap().tag_name().name();
                    *active.entry(format!("{scope}/{tag}")).or_default() += 1;
                }
                if tag == "SignalConnection"
                    && n.attribute("Bypass") != Some("1")
                    && n.attribute("Ratio") != Some("0")
                {
                    let scope = n
                        .parent()
                        .and_then(|p| p.parent())
                        .map(|p| p.tag_name().name())
                        .unwrap_or("");
                    *active
                        .entry(format!("{scope}/SignalConnection"))
                        .or_default() += 1;
                }
            }
            let dropped: BTreeSet<_> = ir
                .unsupported
                .iter()
                .filter(|u| u.feature == "module")
                .map(|u| u.value.clone())
                .collect();
            let r = serde_json::json!({"id":format!("{path}::{program}"),"active":active,"dropped_modules":dropped,"authored_attributes":attrs,"keygroups":keygroups,"stacked_keygroups":stacked});
            writeln!(out, "{r}").unwrap();
            out.flush().unwrap();
        }
    }
}
