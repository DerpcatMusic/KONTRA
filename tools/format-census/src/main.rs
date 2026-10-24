//! Format coverage census over an installed library tree. Parses every file with
//! the reader KONTRA ships for its family (no audio decode, no key use: the
//! `default-features = false` build refuses protected content) and prints:
//!   stdout  one TSV line per file: family, outcome, detail
//!   stderr  aggregate counts per (family, outcome, detail)
//!
//! Usage: format-census <library-root> [--only <ext>]
//! Outputs carry extensions and error classes only, never installed names.
use ni_file::NIFile;
use sampler_kontakt::{LoadError, Options, StreamPolicy, load_read_streamed};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

/// Read the first bytes of a file (magic + a little header).
fn head(path: &Path) -> std::io::Result<[u8; 16]> {
    let mut buf = [0u8; 16];
    let mut file = std::fs::File::open(path)?;
    let n = file.read(&mut buf)?;
    buf[n..].fill(0);
    Ok(buf)
}

/// Human name for the first four magic bytes of a family we do not key on extension.
fn magic_name(h: &[u8; 16]) -> String {
    match h {
        b if b.starts_with(b"aswv") => "aswv".into(),
        b if b.starts_with(b"UFS2") => "UFS2".into(),
        b if b.starts_with(b"IKMPAK") => "IKMPAK".into(),
        b if b.starts_with(b"/\\ NI FC") => "FileContainer".into(),
        b if b[12..16] == *b"hsin" => "NIS".into(),
        b => format!("{:02x}{:02x}{:02x}{:02x}", b[0], b[1], b[2], b[3]),
    }
}

/// Coarse, stable class for an error without leaking paths.
fn class(e: &LoadError) -> String {
    let stage = e
        .stage()
        .map(|s| format!("{s:?}").to_lowercase())
        .unwrap_or_default();
    format!("{stage}/{}", cause_class(e.cause()))
}

fn cause_class(e: &LoadError) -> String {
    match e {
        LoadError::Io { .. } => "io".into(),
        LoadError::Decode { what, .. } => format!("decode:{what}"),
        LoadError::Access { .. } => "access-required".into(),
        LoadError::Invalid { .. } => "invalid".into(),
        LoadError::Lower(_) => "lower".into(),
        LoadError::Canceled => "canceled".into(),
        other => format!("{other:?}")
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or("other")
            .to_lowercase(),
    }
}

/// Full typed read plus streamed instrument build; `partial` when the reader
/// dropped features it cannot represent (reported with their feature names).
fn kontakt(path: &Path, program: Option<usize>) -> (String, String) {
    let read = match program {
        Some(i) => sampler_kontakt::read_program(path, i),
        None => sampler_kontakt::read(path),
    };
    let kontakt = match read {
        Ok(k) => k,
        Err(e) => return ("failed".into(), class(&e)),
    };
    let options = Options {
        library: Some(path.into()),
        ..Options::default()
    };
    match load_read_streamed(kontakt, &options, &StreamPolicy::default(), |_| {}) {
        Err(e) => ("failed".into(), format!("load:{}", class(&e))),
        Ok(s) => {
            let mut features = BTreeMap::<String, usize>::new();
            for u in &s.loaded.instrument.unsupported {
                // Notes are not losses: they never make an instrument partial.
                if u.reason == sampler_ir::Reason::Info {
                    continue;
                }
                // Local diagnostics: CENSUS_VALUES adds the value text (not committed).
                let key = if std::env::var_os("CENSUS_VALUES").is_some() {
                    format!("{}={}", u.feature, u.value.chars().take(160).collect::<String>())
                } else {
                    u.feature.to_string()
                };
                *features.entry(key).or_default() += 1;
            }
            if features.is_empty() {
                ("full".into(), String::new())
            } else {
                let list: Vec<String> = features.iter().map(|(f, n)| format!("{f}×{n}")).collect();
                ("partial".into(), list.join(","))
            }
        }
    }
}

fn judge(path: &Path, ext: &str, h: &[u8; 16]) -> (String, String) {
    match ext {
        "nki" => kontakt(path, None),
        "nkm" => match sampler_kontakt::read_multi(path) {
            Err(e) => ("failed".into(), format!("multi:{}", class(&e))),
            Ok(m) => {
                let mut worst = ("full".to_string(), String::new());
                for i in 0..m.programs.len() {
                    let (o, d) = kontakt(path, Some(i));
                    if o != "full" {
                        worst = (o, d);
                    }
                }
                if m.programs.is_empty() {
                    ("container-only".into(), "no-programs".into())
                } else {
                    worst
                }
            }
        },
        "nka" => ("text".into(), "preset-list".into()),
        // NKC is an NI stream cache (NICache), not a preset chunk container: judge the
        // magic only. Its payload is read through the NCW stream path, not here.
        "nkc" => {
            let le = [0x7a, 0x10, 0xe1, 0x3f];
            let be_magic = [0x3f, 0xe1, 0x10, 0x7a];
            if h.starts_with(&le) || h.starts_with(&be_magic) {
                ("container-only".into(), "stream-cache".into())
            } else {
                ("failed".into(), "not-nicache".into())
            }
        }
        "nksn" => match sampler_kontakt::read_chunks(path) {
            Ok(_) => ("container-only".into(), "chunks".into()),
            Err(e) => ("failed".into(), format!("chunks:{}", class(&e))),
        },
        // Monoliths are GB-sized: open the member index only (NIFile::read
        // validates the whole NIS body and stalls the census on I/O).
        "nkx" | "nkr" => match sampler_kontakt::ResourceContainer::open(path) {
            Ok(_) => ("container-only".into(), "archive".into()),
            Err(e) => ("failed".into(), format!("resource:{}", class(&e))),
        },
        "nicnt" => match std::fs::File::open(path)
            .map_err(|e| e.to_string())
            .and_then(|mut f| NIFile::read(&mut f).map_err(|e| format!("{e:?}")))
        {
            Ok(_) => ("container-only".into(), "detected".into()),
            Err(_) => match sampler_kontakt::ResourceContainer::open(path) {
                Ok(_) => ("container-only".into(), "archive".into()),
                Err(e) => ("failed".into(), format!("resource:{}", class(&e))),
            },
        },
        // UFS2 banks: translate the program without decoding samples (UVI's own
        // bank reader; the XML-only `read` path does not accept binary banks).
        "ufs" => match sampler_uvi::translate_path(path) {
            Ok(_) => ("full".into(), "translate".into()),
            Err(e) => ("failed".into(), scrub(&format!("{e:?}"))),
        },
        "ncw" => match std::fs::File::open(path).map(std::io::BufReader::new) {
            Err(_) => ("failed".into(), "io".into()),
            Ok(file) => match ncw::NcwReader::read(file) {
                Ok(_) => ("container-only".into(), "header".into()),
                Err(e) => (
                    "failed".into(),
                    format!("ncw:{e}").chars().take(40).collect(),
                ),
            },
        },
        _ => {
            let _ = h;
            ("no-reader".into(), String::new())
        }
    }
}

/// Error text with installed paths removed: keep the reason, drop the file names.
fn scrub(text: &str) -> String {
    let reason = text.split("reason: \"").nth(1).unwrap_or(text);
    let words: Vec<&str> = reason
        .split_whitespace()
        .filter(|w| {
            !w.contains('/') && !w.contains(".ufs") && !w.contains(".uvip") && !w.contains(".nki")
        })
        .collect();
    words.join(" ").chars().take(90).collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// `--ser`: Kontakt's serializable classes as ni-file's native schema reads them
/// (one row per class and stored-version range), summarised by reader status.
fn ser_table() {
    let mut by_status = BTreeMap::<&str, usize>::new();
    println!("class\tsertype\tcurrent\tstatus\tversions");
    for r in ni_file::kontakt::native_schema::readers() {
        *by_status.entry(r.status).or_default() += 1;
        let versions: Vec<String> = r
            .versions
            .iter()
            .map(|(lo, hi)| format!("{lo:#x}-{hi:#x}"))
            .collect();
        println!(
            "{}\t{:#04x}\t{:#x}\t{}\t{}",
            r.class,
            r.sertype,
            r.current,
            r.status,
            versions.join(",")
        );
    }
    for (status, n) in &by_status {
        eprintln!("{n}\t{status}");
    }
}

/// Zone- and slot-level playback-mode tally for every `.nki` under a tree. One line per
/// mode key: zones, and distinct libraries (top-level folders, counted, never named) using it.
/// Loop slots are keyed by their own flags (alternating, crossfade, counted, until-release,
/// tuned). Output carries mode names only, never installed names.
fn modes(root: &Path) {
    use sampler_ir::{Looping, Span, Trigger};
    let mut files = Vec::new();
    walk(root, &mut files);
    files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")));
    files.sort();
    let mut zones = BTreeMap::<String, usize>::new();
    let mut libs = BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut failed = 0usize;
    let mut add = |key: String, lib: &str| {
        *zones.entry(key.clone()).or_default() += 1;
        libs.entry(key).or_default().insert(lib.to_string());
    };
    for path in &files {
        let lib = path
            .strip_prefix(root)
            .ok()
            .and_then(|r| r.components().next())
            .map_or(String::new(), |c| c.as_os_str().to_string_lossy().into_owned());
        let Ok(k) = sampler_kontakt::read(path) else {
            failed += 1;
            continue;
        };
        let options = Options {
            library: Some(path.into()),
            ..Options::default()
        };
        let Ok(s) = load_read_streamed(k, &options, &StreamPolicy::default(), |_| {}) else {
            failed += 1;
            continue;
        };
        let inst = &s.loaded.instrument;
        for zone in &inst.zones {
            let group = zone.group.map(|g| &inst.groups[g.0]);
            let looping = match zone.playback.looping {
                Looping::None => "none".to_string(),
                Looping::OneShot => "one-shot".into(),
                Looping::Continuous(l) => format!("continuous{}{}", xf(l.crossfade), if l.alternating { "-alt" } else { "" }),
                Looping::UntilRelease(l) => format!("until-release{}", xf(l.crossfade)),
                Looping::Slots(slots) => {
                    for slot in slots.iter().flatten() {
                        let mut flags = Vec::new();
                        if slot.range.alternating {
                            flags.push("alt");
                        }
                        if slot.range.crossfade != Span::Frames(0) {
                            flags.push("xfade");
                        }
                        if slot.count > 0 {
                            flags.push("counted");
                        }
                        if slot.until_release {
                            flags.push("until-release");
                        }
                        if slot.tuning != 1.0 {
                            flags.push("tuned");
                        }
                        let key = format!("slot[{}]", if flags.is_empty() { "plain".into() } else { flags.join("+") });
                        add(key, &lib);
                    }
                    let n = slots.iter().flatten().count();
                    format!("slots{n}")
                }
            };
            let trigger = match zone.trigger {
                Trigger::Transition { .. } => "transition".to_string(),
                t => format!("{t:?}").to_lowercase(),
            };
            let starts = group.map_or(String::new(), |g| {
                g.start
                    .iter()
                    .map(|st| match st.test {
                        sampler_ir::StartTest::Key { .. } => "key",
                        sampler_ir::StartTest::Controller { .. } => "cc",
                        sampler_ir::StartTest::RoundRobin(_) => "rr",
                        sampler_ir::StartTest::Random => "random",
                    })
                    .collect::<Vec<_>>()
                    .join("+")
            });
            let stretch = group.and_then(|g| g.time_stretch).map_or("-".to_string(), |t| format!("{:?}", t.kind));
            let machine = group.and_then(|g| g.machine).map_or("-".to_string(), |m| format!("{m:?}").split([' ', '{', '(']).next().unwrap_or("").to_string());
            add(format!("loop={looping} trigger={trigger} start={starts} stretch={stretch} machine={machine}").replace("  ", " "), &lib);
        }
    }
    for (key, n) in &zones {
        println!("{n}\t{}\t{key}", libs[key].len());
    }
    eprintln!("files={} failed={failed}", files.len());

    fn xf(span: Span) -> &'static str {
        match span {
            Span::Frames(0) => "",
            _ => "-xfade",
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--ser") {
        return ser_table();
    }
    if args.first().is_some_and(|a| a == "--modes") {
        return modes(Path::new(args.get(1).expect("--modes <dir>")));
    }
    if args.first().is_some_and(|a| a == "--ncw") {
        // Full NCW decode (not just header) of every .ncw under a directory: one line per file.
        let mut files = Vec::new();
        walk(Path::new(args.get(1).expect("--ncw <dir>")), &mut files);
        files.retain(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ncw")));
        files.sort();
        for path in &files {
            let outcome = std::fs::File::open(path)
                .map(std::io::BufReader::new)
                .map_err(|e| e.to_string())
                .and_then(|f| ncw::NcwReader::read(f).map_err(|e| e.to_string()))
                .and_then(|mut r| {
                    r.decode_samples()
                        .map(|s| s.len())
                        .map_err(|e| e.to_string())
                });
            match outcome {
                Ok(n) => println!("ok\t{n}\t{}", path.display()),
                Err(e) => println!("ERR\t{e}\t{}", path.display()),
            }
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--nkx") {
        // Monolith resolution: every NCW member of every .nkx/.nkr below a directory,
        // read through the archive index and fully decoded. One line per member.
        let mut files = Vec::new();
        let root = Path::new(args.get(1).expect("--nkx <dir|archive>"));
        if root.is_file() {
            files.push(root.to_path_buf());
        } else {
            walk(root, &mut files);
        }
        files.retain(|p| p.extension().is_some_and(|e| e == "nkx" || e == "nkr"));
        files.sort();
        for path in &files {
            let mut container = match sampler_kontakt::ResourceContainer::open(path) {
                Ok(c) => c,
                Err(e) => {
                    println!("archive\tERR\t{e:?}\t{}", path.display());
                    continue;
                }
            };
            let names: Vec<String> = container.names().iter().map(|n| n.to_string()).collect();
            for name in names {
                if !name.to_lowercase().ends_with(".ncw") {
                    continue;
                }
                let outcome = match container.read(&name) {
                    Ok(Some(bytes)) => ncw::NcwReader::read(std::io::Cursor::new(bytes))
                        .map_err(|e| e.to_string())
                        .and_then(|mut r| r.decode_samples().map(|s| s.len()).map_err(|e| e.to_string()))
                        .map(|n| format!("ok\t{n}"))
                        .unwrap_or_else(|e| format!("ERR\t{e}")),
                    Ok(None) => "ERR\tabsent".into(),
                    Err(e) => format!("ERR\t{e:?}"),
                };
                if std::env::var_os("CENSUS_PATHS").is_some() {
                    println!("{outcome}\t{name}\t{}", path.display());
                } else {
                    println!("{outcome}");
                }
            }
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--script") {
        // `--script <nki>`: print the KSP source of each script slot to stdout (local reading only).
        let k = match args.get(2) {
            Some(i) => sampler_kontakt::read_program(Path::new(&args[1]), i.parse().expect("index")),
            None => sampler_kontakt::read(Path::new(&args[1])),
        }
        .expect("read");
        for b in &k.instrument.behaviors {
            println!("--- {} ({:?})", b.name, b.language);
            println!("{}", b.source);
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--state") {
        // `--state <nkm|nki> [program]`: persisted script variables per script slot (names and kinds only).
        let k = match args.get(2) {
            Some(i) => sampler_kontakt::read_program(Path::new(&args[1]), i.parse().expect("index")),
            None => sampler_kontakt::read(Path::new(&args[1])),
        }
        .expect("read");
        for b in &k.instrument.behaviors {
            let names: Vec<String> = b
                .state
                .iter()
                .map(|(n, v)| format!("{n}:{}", match v {
                    sampler_ir::Saved::Int(_) => "int",
                    sampler_ir::Saved::Real(_) => "real",
                    sampler_ir::Saved::Text(_) => "text",
                    sampler_ir::Saved::Ints(_) => "ints",
                    sampler_ir::Saved::Reals(_) => "reals",
                    sampler_ir::Saved::Texts(_) => "texts",
                }))
                .collect();
            println!("{} : {}", b.name, names.join(" "));
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--resource") {
        // `--resource <archive> [member...]`: list members, or read named ones (size only).
        let mut container =
            sampler_kontakt::ResourceContainer::open(Path::new(&args[1])).expect("open archive");
        for name in &args[2..] {
            match container.read(name) {
                Ok(Some(bytes)) if std::env::var_os("CENSUS_HEX").is_some() => {
                    println!("{name}\t{} bytes\t{:02x?}", bytes.len(), &bytes[..bytes.len().min(256)]);
                }
                Ok(Some(bytes)) => println!("{name}\t{} bytes", bytes.len()),
                Ok(None) => println!("{name}\tabsent"),
                Err(e) => println!("{name}\t{e:?}"),
            }
        }
        return;
    }
    if args.first().is_some_and(|a| a == "--explain") {
        // Full error text for one file (local paths: diagnostics only, never committed).
        let path = Path::new(args.get(1).expect("--explain <file>"));
        if path.extension().is_some_and(|e| e == "ufs") {
            match sampler_uvi::translate_path(path) {
                Ok(_) => eprintln!("translate: ok"),
                Err(e) => {
                    let mut chain = format!("{e:?}");
                    let mut src = std::error::Error::source(e.as_ref());
                    while let Some(inner) = src {
                        chain.push_str(&format!(" <- {inner}"));
                        src = inner.source();
                    }
                    eprintln!("translate: {chain}");
                }
            }
            return;
        }
        match sampler_kontakt::read(path) {
            Err(e) => eprintln!("read: {e:?}"),
            Ok(_) => eprintln!("read: ok"),
        }
        if let Ok(k) = sampler_kontakt::read(path) {
            let options = Options {
                library: Some(path.into()),
                ..Options::default()
            };
            if let Err(e) = load_read_streamed(k, &options, &StreamPolicy::default(), |_| {}) {
                eprintln!("load: {e:?}");
            }
        }
        return;
    }
    let root = args
        .first()
        .expect("usage: format-census <root> [--only <ext>]");
    let only = args
        .iter()
        .position(|a| a == "--only")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mut files = Vec::new();
    walk(Path::new(root), &mut files);
    files.sort();
    let mut counts = BTreeMap::<(String, String, String), usize>::new();
    for path in &files {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if only.as_deref().is_some_and(|o| o != ext) {
            continue;
        }
        let Ok(h) = head(path) else { continue };
        let family = format!("{ext}:{}", magic_name(&h));
        let (outcome, detail) = match ext.as_str() {
            "wav" | "ogg" | "png" | "txt" | "pdf" | "rtf" | "html" | "webloc" | "exe" | "cf"
            | "dat" | "ascf" | "pak" | "s20" | "obw" | "parts" | "" => {
                if matches!(
                    ext.as_str(),
                    "wav" | "ogg" | "png" | "txt" | "pdf" | "rtf" | "html" | "webloc" | "exe"
                ) {
                    ("media-or-doc".into(), String::new())
                } else {
                    ("no-reader".into(), String::new())
                }
            }
            _ => judge(path, &ext, &h),
        };
        if std::env::var_os("CENSUS_PATHS").is_some() {
            // Local diagnostics only: the installed path, never committed.
            println!("{family}\t{outcome}\t{detail}\t{}", path.display());
        } else {
            println!("{family}\t{outcome}\t{detail}");
        }
        *counts.entry((family, outcome, detail)).or_default() += 1;
    }
    for ((family, outcome, detail), n) in &counts {
        eprintln!("{n}\t{family}\t{outcome}\t{detail}");
    }
}
