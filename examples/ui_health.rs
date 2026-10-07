//! Read-only Kontakt UI census. Rendered screenshots and records stay in the caller's cache.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = PathBuf::from(
        args.first()
            .expect("ui_health OUT.jsonl [--only substring] [--shots DIR]"),
    );
    let only = args
        .windows(2)
        .find(|a| a[0] == "--only")
        .map(|a| a[1].as_str())
        .unwrap_or("");
    let option = |name: &str| args.windows(2).find(|a| a[0] == name).map(|a| a[1].clone());
    let cache_dir = PathBuf::from(option("--cache").expect("--cache DIR is required"));
    let budget: u64 = option("--budget-seconds").unwrap_or("240".into()).parse()?;
    let started_shard = std::time::Instant::now();
    let binary = blake3::hash(&fs::read(std::env::current_exe()?)?)
        .to_hex()
        .to_string();
    let cache_dir = cache_dir.join(binary);
    fs::create_dir_all(&cache_dir)?;
    let shots = args
        .windows(2)
        .find(|a| a[0] == "--shots")
        .map(|a| PathBuf::from(&a[1]));
    let max_items: usize = option("--max-items").unwrap_or("25".into()).parse()?;
    let list = fs::read_to_string(
        Path::new(&std::env::var("HOME")?).join(".cache/kontakto-corpus/items.tsv"),
    )?;
    if let Some(name) = option("--resource-check") {
        let path = list
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .find(|(k, id)| *k == "kontakt" && id.contains(only))
            .map(|(_, id)| Path::new(id))
            .ok_or_else(|| anyhow::anyhow!("no matching NKI"))?;
        let mut resources = sampler_kontakt::Resources::of(path);
        let locations = resources.locations();
        let checks: Vec<_> = [
            format!("Resources/pictures/{name}.png"),
            format!("Resources/pictures/{name}.txt"),
            format!("resources/pictures/{}.PNG", name.to_uppercase()),
        ]
        .into_iter()
        .map(|candidate| {
            let size = resources.read(&candidate).map(|b| b.len());
            json!({"path":candidate,"found":size.is_some(),"bytes":size})
        })
        .collect();
        let root = path
            .ancestors()
            .find(|a| {
                a.parent()
                    .is_some_and(|p| p.file_name().is_some_and(|n| n == "Kontakt"))
            })
            .unwrap();
        let mut containers = Vec::new();
        let mut loose = Vec::new();
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
        {
            let p = entry.path();
            if p.file_name()
                .unwrap()
                .to_string_lossy()
                .to_lowercase()
                .contains(&name.to_lowercase())
            {
                loose.push(p.to_owned());
            }
            if p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("nkr") || e.eq_ignore_ascii_case("nicnt"))
            {
                match sampler_kontakt::ResourceContainer::open(p) {
                    Ok(mut container) => {
                        let matches: Vec<_> = container
                            .names()
                            .into_iter()
                            .filter(|n| {
                                n.to_lowercase().contains("artic")
                                    || n.to_lowercase().contains(&name.to_lowercase())
                            })
                            .map(str::to_owned)
                            .collect();
                        let reads: Vec<_> = checks.iter().map(|c| { let candidate = c["path"].as_str().unwrap(); let found = container.read(candidate); json!({"path":candidate,"found":matches!(found, Ok(Some(_))),"error":found.err().map(|e| e.to_string())}) }).collect();
                        containers.push(json!({"path":p,"members":container.names().len(),"matches":matches,"reads":reads}));
                    }
                    Err(e) => containers.push(json!({"path":p,"error":e.to_string()})),
                }
            }
        }
        println!(
            "{}",
            json!({"instrument":path,"locations":locations,"checks":checks,"containers":containers,"loose_matches":loose})
        );
        return Ok(());
    }
    fs::create_dir_all(out.parent().unwrap())?;
    let mut output = BufWriter::new(fs::File::create(&out)?);
    let mut pending = false;
    let mut completed = 0;
    for (n, line) in list
        .lines()
        .filter(|l| l.starts_with("kontakt\t") || l.starts_with("kontakt-multi\t"))
        .enumerate()
    {
        let (kind, id) = line.split_once('\t').unwrap();
        if !id.contains(only) {
            continue;
        }
        let path = Path::new(id);
        let mut item_key = blake3::Hasher::new();
        item_key.update(id.as_bytes());
        fingerprint(&mut item_key, path);
        for resource in sampler_kontakt::Resources::of(path).locations() {
            fingerprint(&mut item_key, &resource);
        }
        item_key.update(format!("{shots:?}").as_bytes());
        let item_cache = cache_dir.join(format!("item-{}.json", item_key.finalize().to_hex()));
        if let Ok(bytes) = fs::read(&item_cache) {
            let record: Value = serde_json::from_slice(&bytes)?;
            serde_json::to_writer(&mut output, &record)?;
            writeln!(output)?;
            continue;
        }
        if completed >= max_items || (completed > 0 && started_shard.elapsed().as_secs() >= budget)
        {
            pending = true;
            continue;
        }
        let multi = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nkm"));
        let count = if multi {
            sampler_kontakt::read_multi(path).map(|m| m.programs.len())
        } else {
            Ok(1)
        };
        let mut programs = Vec::new();
        let started = std::time::Instant::now();
        match count {
            Err(e) => programs.push(json!({"loaded":false,"usable":false,"error":e.to_string()})),
            Ok(count) => {
                for p in 0..count {
                    let read = if multi {
                        sampler_kontakt::read_program(path, p)
                    } else {
                        sampler_kontakt::read(path)
                    };
                    match read {
                    Err(e) => programs.push(json!({"program":p,"loaded":false,"usable":false,"error":e.to_string()})),
                    Ok(mut k) => {
                        // Only frontend inputs: source, saved state, slot, group names and resource root.
                        // Never cache or write decrypted script/resource bytes.
                        let mut key = blake3::Hasher::new();
                        let root = path.ancestors().find(|a| a.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "Kontakt"))).unwrap_or(path.parent().unwrap());
                        key.update(root.as_os_str().as_encoded_bytes());
                        key.update(format!("{shots:?}").as_bytes());
                        for resource in sampler_kontakt::Resources::of(path).locations() { fingerprint(&mut key, &resource); }
                        for b in &k.instrument.behaviors { key.update(format!("{:?}", b).as_bytes()); }
                        for g in &k.instrument.groups { key.update(g.name.as_bytes()); key.update(b"\0"); }
                        let key = key.finalize().to_hex().to_string();
                        let input_cache = cache_dir.join(format!("input-{key}.json"));
                        let cached = fs::read(&input_cache).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
                        let reused = cached.is_some();
                        let ui = if let Some(ui) = cached { ui } else {
                            let options = sampler_kontakt::Options { library:Some(path.to_owned()), ..Default::default() };
                            let dir = shots.as_ref().map(|d| d.join(&key));
                            let ui = kontakto::ui_health(&mut k.instrument, &options, dir.as_deref());
                            fs::write(&input_cache, serde_json::to_vec(&ui).expect("serialize UI report")).expect("cache UI report");
                            ui
                        };
                        programs.push(json!({"program":p,"name":k.instrument.name,"ui":ui,"reused":reused}));
                    }
                }
                }
            }
        }
        let record = json!({"id":id,"kind":kind,"ui":{"programs":programs},"ms":started.elapsed().as_millis()});
        let temp = item_cache.with_extension("tmp");
        fs::write(&temp, serde_json::to_vec(&record)?)?;
        fs::rename(&temp, &item_cache)?;
        completed += 1;
        serde_json::to_writer(&mut output, &record)?;
        writeln!(output)?;
        output.flush()?;
        if n % 25 == 0 {
            eprintln!(
                "{n}: {completed} new items in shard, {}",
                path.file_name().unwrap().to_string_lossy()
            );
        }
    }
    output.flush()?;
    if pending {
        std::process::exit(75);
    }
    eprintln!(
        "shard: {completed} items in {:.1}s",
        started_shard.elapsed().as_secs_f64()
    );
    Ok(())
}

fn fingerprint(key: &mut blake3::Hasher, path: &Path) {
    key.update(path.as_os_str().as_encoded_bytes());
    if let Ok(m) = fs::metadata(path) {
        key.update(&m.len().to_le_bytes());
        if let Ok(t) = m.modified().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            key.update(&t.as_nanos().to_le_bytes());
        }
    }
    key.update(b"\0");
}
