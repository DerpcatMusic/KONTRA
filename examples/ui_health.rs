//! Read-only Kontakt UI census. Rendered screenshots and records stay in the caller's cache.
use std::{collections::HashMap, fs, io::{BufWriter, Write}, path::{Path, PathBuf}};
use serde_json::{Value, json};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = PathBuf::from(args.first().expect("ui_health OUT.jsonl [--only substring] [--shots DIR]"));
    let only = args.windows(2).find(|a| a[0] == "--only").map(|a| a[1].as_str()).unwrap_or("");
    let shots = args.windows(2).find(|a| a[0] == "--shots").map(|a| PathBuf::from(&a[1]));
    let list = fs::read_to_string(Path::new(&std::env::var("HOME")?).join(".cache/kontakto-corpus/items.tsv"))?;
    fs::create_dir_all(out.parent().unwrap())?;
    let mut output = BufWriter::new(fs::File::create(&out)?);
    let mut cache = HashMap::<String, Value>::new();
    for (n, line) in list.lines().filter(|l| l.starts_with("kontakt\t") || l.starts_with("kontakt-multi\t")).enumerate() {
        let (kind, id) = line.split_once('\t').unwrap();
        if !id.contains(only) { continue; }
        let path = Path::new(id);
        let multi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkm"));
        let count = if multi { sampler_kontakt::read_multi(path).map(|m| m.programs.len()) } else { Ok(1) };
        let mut programs = Vec::new();
        let started = std::time::Instant::now();
        match count {
            Err(e) => programs.push(json!({"loaded":false,"usable":false,"error":e.to_string()})),
            Ok(count) => for p in 0..count {
                let read = if multi { sampler_kontakt::read_program(path,p) } else { sampler_kontakt::read(path) };
                match read {
                    Err(e) => programs.push(json!({"program":p,"loaded":false,"usable":false,"error":e.to_string()})),
                    Ok(mut k) => {
                        // Only frontend inputs: source, saved state, slot, group names and resource root.
                        // Never cache or write decrypted script/resource bytes.
                        let mut key = blake3::Hasher::new();
                        let root = path.ancestors().find(|a| a.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "Kontakt"))).unwrap_or(path.parent().unwrap());
                        key.update(root.as_os_str().as_encoded_bytes());
                        for resource in sampler_kontakt::Resources::of(path).locations() { key.update(resource.as_os_str().as_encoded_bytes()); key.update(b"\0"); }
                        for b in &k.instrument.behaviors { key.update(format!("{:?}", b).as_bytes()); }
                        for g in &k.instrument.groups { key.update(g.name.as_bytes()); key.update(b"\0"); }
                        let key = key.finalize().to_hex().to_string();
                        let reused = cache.contains_key(&key);
                        let ui = cache.entry(key.clone()).or_insert_with(|| {
                            let options = sampler_kontakt::Options { library:Some(path.to_owned()), ..Default::default() };
                            let dir = shots.as_ref().map(|d| d.join(&key));
                            kontakto::ui_health(&mut k.instrument, &options, dir.as_deref())
                        }).clone();
                        programs.push(json!({"program":p,"name":k.instrument.name,"ui":ui,"reused":reused}));
                    }
                }
            },
        }
        let record = json!({"id":id,"kind":kind,"ui":{"programs":programs},"ms":started.elapsed().as_millis()});
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
        output.flush()?;
        if n % 25 == 0 { eprintln!("{n}: {} UI inputs, {}", cache.len(), path.file_name().unwrap().to_string_lossy()); }
    }
    Ok(())
}
