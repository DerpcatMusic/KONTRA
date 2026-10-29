//! Lists a UVI bank's programs and the Lua members each one references (dev helper).
use std::path::PathBuf;

use sampler_uvi::Bank;

fn main() {
    for path in std::env::args_os().skip(1).map(PathBuf::from) {
        let bank = Bank::open(&path).unwrap_or_else(|e| panic!("open {}: {e:?}", path.display()));
        let scripts = bank.scripts();
        println!(
            "bank {} lua members {}",
            path.display(),
            scripts.sources().len()
        );
        for (name, source) in scripts.sources().iter() {
            println!("  lua {name} {} bytes", source.len());
        }
        if std::env::var_os("NAMES").is_some() {
            for program in bank.programs() {
                println!("program {program}");
            }
            continue;
        }
        for program in bank.programs().iter().take(5) {
            let (xml, _) = bank.program(program).expect("program");
            let sources = scripts.sources();
            let refs: Vec<&str> = sources
                .iter()
                .map(|(n, _)| n.as_str())
                .filter(|n| {
                    let stem = n.rsplit('/').next().unwrap_or(n);
                    let stem = stem.strip_suffix(".lua").unwrap_or(stem);
                    !stem.is_empty() && xml.to_lowercase().contains(&stem.to_lowercase())
                })
                .collect();
            println!("program {program} xml {} bytes, refs {refs:?}", xml.len());
        }
    }
}
