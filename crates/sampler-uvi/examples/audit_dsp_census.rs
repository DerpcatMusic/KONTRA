//! Metadata only, shardable corpus probe. Never emits XML, scripts or samples.
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

fn add(rows: &mut BTreeMap<(String, String), usize>, category: &str, name: &str) {
    *rows.entry((category.into(), name.into())).or_default() += 1;
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 5, "items.tsv CACHE_DIR START END");
    let start: usize = args[3].parse().unwrap();
    let end: usize = args[4].parse().unwrap();
    let cache = Path::new(&args[2]);
    std::fs::create_dir_all(cache).unwrap();
    let began = Instant::now();
    let mut banks = BTreeMap::new();
    for (index, line) in std::fs::read_to_string(&args[1])
        .unwrap()
        .lines()
        .enumerate()
    {
        if index < start || index >= end || !line.starts_with("uvi-program\t") {
            continue;
        }
        let output = cache.join(format!("{index}.tsv"));
        if std::fs::read_to_string(&output).is_ok_and(|v| v.starts_with("census2\n")) {
            continue;
        }
        if began.elapsed() >= Duration::from_secs(240) {
            break;
        }
        let mut rows = BTreeMap::new();
        let (_, source) = line.split_once('\t').unwrap();
        let (bank_path, program) = source.split_once("::").unwrap();
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if !banks.contains_key(bank_path) {
                banks.insert(
                    bank_path.to_string(),
                    sampler_uvi::Bank::open(Path::new(bank_path))?,
                );
            }
            let bank = &banks[bank_path];
            let (xml, _) = bank.program(program)?;
            let doc = sampler_uvi::parse_program_xml(&xml)?;
            for node in doc.descendants().filter(|n| n.is_element()) {
                let Some(parent) = node.parent() else {
                    continue;
                };
                let enabled = node
                    .attribute("Bypass")
                    .is_none_or(|v| v == "0" || v == "false");
                if node.has_tag_name("SignalConnection")
                    && enabled
                    && node
                        .attribute("Ratio")
                        .is_none_or(|v| v.parse::<f64>() != Ok(0.0))
                {
                    if let Some(source) = node.attribute("Source").filter(|s| s.starts_with('@')) {
                        add(&mut rows, "external_enabled", source);
                    }
                    if let Some(destination) = node.attribute("Destination") {
                        let scope = parent.parent().map_or("unknown", |p| p.tag_name().name());
                        add(
                            &mut rows,
                            "route_enabled",
                            &format!("{scope}:{destination}"),
                        );
                    }
                }
                let category = match parent.tag_name().name() {
                    "Inserts" => "effect",
                    "ControlSignalSources" => "modulator",
                    _ => continue,
                };
                let tag = node.tag_name().name();
                add(&mut rows, &format!("{category}_saved"), tag);
                if enabled {
                    add(&mut rows, &format!("{category}_enabled"), tag);
                    if category == "effect" {
                        let scope = parent.parent().map_or("unknown", |p| p.tag_name().name());
                        add(&mut rows, "effect_scope_enabled", &format!("{scope}:{tag}"));
                    }
                }
            }
            add(&mut rows, "files", "parsed");
            Ok(())
        })();
        if let Err(error) = &result {
            add(&mut rows, "files", "failed");
            if let Some(safe) = error.downcast_ref::<sampler_uvi::AccessError>() {
                eprintln!("item {index}: {safe}"); // AccessError is explicitly sanitized.
            }
        }
        let mut text = String::from("census2\n");
        for ((category, name), count) in rows {
            assert!(!name.contains(['\t', '\n']));
            text.push_str(&format!("{category}\t{name}\t{count}\n"));
        }
        std::fs::write(&output, text).unwrap();
        eprintln!(
            "item {index}: {}",
            if result.is_ok() { "ok" } else { "failed" }
        );
    }
}
