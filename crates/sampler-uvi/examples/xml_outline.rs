//! Element outline of one bank program: tag paths with attribute names,
//! stdout only (never writes library payloads). `--gaps` prints the
//! translator's unsupported findings grouped by feature and value instead;
//! `--host` prints the unsupported features after script initialization;
//! `--grep PAT` prints the program scripts' lines containing PAT.
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let gaps = args.iter().any(|a| a == "--gaps");
    let grep = args.iter().position(|a| a == "--grep").map(|i| {
        let pat = args.remove(i + 1);
        args.remove(i);
        pat
    });
    let host = args.iter().any(|a| a == "--host");
    args.retain(|a| a != "--gaps" && a != "--host");
    let item = args.first().ok_or("expected bank::program")?;
    let (path, member) = item.split_once("::").ok_or("expected bank::program")?;
    let bank = sampler_uvi::Bank::open(Path::new(path))?;
    let (text, _) = bank.program(member)?;
    if let Some(pat) = grep {
        // Script lines matching `pat`, printed only (never written to disk).
        for member in bank.members().into_iter().filter(|m| m.to_ascii_lowercase().ends_with(".lua")) {
            let Ok(bytes) = bank.file(&member) else { continue };
            let source = String::from_utf8_lossy(&bytes);
            for (i, line) in source.lines().enumerate().filter(|(_, l)| l.contains(pat.as_str())) {
                println!("{member}:{}: {}", i + 1, line.trim());
            }
        }
        return Ok(());
    }
    if host {
        // After script initialization and the translation it patches: each
        // unsupported feature once, with one example value and its count.
        let (ir, _) = sampler_uvi::inspect_decoded_scripted_program(text, bank.scripts(), sampler_uvi::ScriptedOrigin::Bank(member))
            .map_err(|e| format!("{e:?}"))?;
        let mut by: BTreeMap<String, (usize, String)> = BTreeMap::new();
        for u in &ir.unsupported {
            by.entry(u.feature.clone()).or_insert((0, u.value.clone())).0 += 1;
        }
        for (feature, (n, value)) in by {
            println!("{n:6} {feature} = {}", value.chars().take(120).collect::<String>());
        }
        return Ok(());
    }
    if gaps {
        let uvi = sampler_uvi::translate(&text, Path::new("."))?;
        let mut by: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
        for u in &uvi.instrument.unsupported {
            let e = by.entry((u.feature.clone(), u.value.clone())).or_insert((0, u.location.clone()));
            e.0 += 1;
        }
        for ((feature, value), (n, at)) in by {
            println!("{n:6} {feature} = {value}  @ {at}");
        }
        return Ok(());
    }
    let doc = roxmltree::Document::parse(&text)?;
    for n in doc.descendants().filter(|n| n.is_element()) {
        let depth = n.ancestors().count() - 1;
        let attrs: Vec<String> = n.attributes().map(|a| format!("{}={}", a.name(), a.value().chars().take(24).collect::<String>())).collect();
        let name = if n.has_tag_name("script") { "script".into() } else { attrs.join(" ") };
        println!("{}{} [{}] {}", "  ".repeat(depth), n.tag_name().name(), n.id().get_usize(), name);
    }
    Ok(())
}
