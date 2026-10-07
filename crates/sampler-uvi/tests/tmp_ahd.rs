//! Temporary: print the attributes of every AHD node kind seen in the installed corpus.
#[test]
#[ignore]
fn ahd_attributes() {
    let root = std::env::var("KONTRA_UVI_LIBRARIES").unwrap();
    let mut seen = std::collections::BTreeMap::<String, usize>::new();
    let mut samples = Vec::new();
    for lib in std::fs::read_dir(&root).unwrap().flatten() {
        for f in std::fs::read_dir(lib.path()).unwrap().flatten() {
            if f.path().extension().is_some_and(|e| e == "ufs") {
                let Ok(bank) = sampler_uvi::Bank::open(&f.path()) else { continue };
                for program in bank.programs() {
                    let Ok((text, _)) = bank.program(&program) else { continue };
                    let doc = roxmltree::Document::parse(&text).unwrap();
                    for node in doc.descendants().filter(|n| n.has_tag_name("AHD")) {
                        let mut attrs: Vec<String> = node.attributes().map(|a| a.name().to_string()).collect();
                        attrs.sort();
                        *seen.entry(attrs.join(",")).or_default() += 1;
                        if samples.len() < 6 {
                            let conns: Vec<String> = doc.descendants().filter(|c| c.has_tag_name("SignalConnection") && c.attribute("Source").is_some_and(|s| s.ends_with(node.attribute("Name").unwrap_or("?")))).map(|c| format!("{}->{}", c.attribute("Source").unwrap_or(""), c.attribute("Destination").unwrap_or(""))).collect();
                            samples.push(format!("{} :: {:?} conns {:?}", program, node.attributes().map(|a| format!("{}={}", a.name(), a.value())).collect::<Vec<_>>(), conns));
                        }
                    }
                }
            }
        }
    }
    println!("KINDS {seen:#?}");
    for s in samples { println!("SAMPLE {s}"); }
}
