//! Inspect saved-state shapes without exporting library source or saved text.
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{Bank, Program, Snapshot, snapshot_metadata_names},
};
use sampler_kontakt::{Chunks, Limits, SavedEntry, SavedValue, Script};
use sampler_ksp::model::WidgetKind as Decl;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
#[derive(Default)]
struct Census {
    counts: BTreeMap<String, usize>,
    shapes: BTreeMap<String, usize>,
    file_keys: BTreeSet<String>,
    contexts: BTreeMap<(String, String), Vec<BTreeMap<String, Decl>>>,
    slots: Vec<BTreeMap<String, Decl>>,
    library: String,
    path: std::path::PathBuf,
    family: String,
    probe: bool,
}
impl Census {
    fn count(&mut self, key: impl Into<String>) {
        let key = key.into();
        self.file_keys.insert(key.clone());
        *self.counts.entry(key).or_default() += 1;
    }
    fn entry(&mut self, e: &[u8], declaration: Option<Decl>) {
        let Ok(s) = std::str::from_utf8(e) else {
            self.count("entry non-UTF8");
            return;
        };
        let (name, value) = s.split_once(' ').unwrap_or((s, ""));
        let tag = name.chars().next().unwrap_or('\0');
        self.count(format!("tag {tag}"));
        self.count(format!("wire {} tag {tag}", self.family));
        match SavedEntry::parse(
            e,
            declaration,
            Limits {
                bytes: 128 << 20,
                records: 1 << 20,
            },
        ) {
            Ok(parsed) => {
                self.count(format!("decoded {} tag {tag}", self.family));
                self.count(format!("saved declaration {declaration:?}"));
                if let SavedValue::Texts(texts) = parsed.value {
                    if texts.iter().any(|t| std::str::from_utf8(t).is_err()) {
                        self.count("text array non-UTF8 cells");
                    }
                    if !value.ends_with('\n') {
                        self.count("string array missing terminal LF");
                    }
                }
                if self.probe
                    && (name.to_lowercase().contains("vel") || name.to_lowercase().contains("sord"))
                {
                    match parsed.value {
                        SavedValue::Int(v) | SavedValue::MenuIndex(v) => println!(
                            "PROBE\t{}\t{name}\t{declaration:?}\t{v}",
                            self.path.display()
                        ),
                        _ => (),
                    }
                }
            }
            Err(error) => {
                self.count(format!("undecoded {} tag {tag}", self.family));
                println!(
                    "VALUE_FAIL\t{}\ttag={tag} declaration={declaration:?} error={error}",
                    self.path.display()
                );
            }
        }
        let tokens: Vec<_> = value.split_whitespace().collect();
        let shape = if tokens.iter().all(|v| v.parse::<i32>().is_ok()) {
            "integers"
        } else if tokens.iter().all(|v| v.parse::<f64>().is_ok()) {
            "reals"
        } else {
            "text"
        };
        *self
            .shapes
            .entry(format!(
                "tag {tag} {shape} tokens={} tabs={} LF={} CR={} NUL={} quote={}",
                tokens.len(),
                value.matches('\t').count(),
                value.matches('\n').count(),
                value.matches('\r').count(),
                value.matches('\0').count(),
                value.matches('"').count()
            ))
            .or_default() += 1;
    }
    fn script(&mut self, c: &Chunk) -> Result<(), String> {
        let mut wire = c.id.to_le_bytes().to_vec();
        wire.extend((c.data.len() as u32).to_le_bytes());
        wire.extend(&c.data);
        let limits = Limits {
            bytes: 128 << 20,
            records: 1 << 20,
        };
        let chunks = Chunks::parse(&wire, limits).map_err(|e| e.to_string())?;
        let script =
            Script::parse(chunks.iter().next().unwrap(), limits).map_err(|e| e.to_string())?;
        self.count(format!("script version {:x}", script.object.version));
        self.count(format!(
            "script extension bytes {}",
            script.extension.data().len()
        ));
        self.count(format!("script structured {}", script.object.is_structured));
        self.count(format!(
            "script private bytes {}",
            script.object.private.data().len()
        ));
        self.count(match script.persistent {
            None => "script table absent",
            Some(t) if t.is_empty() => "script table empty",
            _ => "script table populated",
        });
        let mut declarations = BTreeMap::new();
        let mut instrument_only = BTreeSet::new();
        let mut dimensions = BTreeMap::new();
        if let Some(text) = script.text {
            let text = String::from_utf8_lossy(text.data()).to_lowercase();
            for line in text.lines() {
                if line.trim_start().starts_with("declare ") {
                    for word in line.split_whitespace() {
                        if word.starts_with(['%', '?', '!']) {
                            if let Some((name, size)) = word.split_once('[') {
                                if let Some(n) =
                                    size.split(']').next().and_then(|s| s.parse::<usize>().ok())
                                {
                                    dimensions.insert(name.to_owned(), n);
                                }
                            }
                        }
                    }
                }
                if let Some(call) = line.trim().strip_prefix("make_instr_persistent") {
                    if let Some(name) = call
                        .trim()
                        .strip_prefix('(')
                        .and_then(|s| s.split(')').next())
                    {
                        instrument_only.insert(name.trim().to_owned());
                    }
                }
                if line.trim_start().starts_with("declare ui_") {
                    let mut words = line.split_whitespace();
                    words.next();
                    let kind = words.next().unwrap_or("");
                    self.count(format!("declaration {kind}"));
                    let name = words
                        .next()
                        .unwrap_or("")
                        .split(['(', '['])
                        .next()
                        .unwrap_or("");
                    let Some(declaration) = Decl::from_keyword(kind) else {
                        continue;
                    };
                    declarations.insert(name.to_owned(), declaration);
                }
            }
            if self.probe {
                for (name, declaration) in &declarations {
                    if *declaration == Decl::Menu && name.contains("vel") {
                        let values: Vec<_> = text
                            .lines()
                            .filter(|line| {
                                line.trim_start().starts_with("add_menu_item")
                                    && line.contains(name)
                            })
                            .filter_map(|line| {
                                line.rsplit(',')
                                    .next()?
                                    .split(')')
                                    .next()?
                                    .trim()
                                    .parse::<i32>()
                                    .ok()
                            })
                            .collect();
                        println!("MENU_ITEMS\t{}\t{name}\t{values:?}", self.path.display());
                    }
                }
            }
            for command in [
                "make_instr_persistent",
                "load_array",
                "load_array_str",
                "save_array",
                "save_array_str",
            ] {
                if text.contains(command) {
                    self.count(format!("script command {command}"));
                }
            }
        }
        self.family = format!("0x06/0x{:x}", script.object.version);
        if let Some(table) = script.persistent {
            for entry in table.iter() {
                let name = entry.data().split(|b| *b == b' ').next().unwrap_or(&[]);
                let name = String::from_utf8_lossy(name).to_lowercase();
                let declaration = declarations.get(&name).copied();
                if instrument_only.contains(&name) {
                    self.count("instrument-only saved entry");
                }
                if let Some(&dimension) = dimensions.get(&name) {
                    let typed = SavedEntry::from_bytes(entry, declaration, limits)
                        .map_err(|e| e.to_string())?;
                    match typed.value {
                        SavedValue::Ints { values, .. } => {
                            self.count(format!(
                                "integer array encoded/declared {}",
                                if values.len() < dimension {
                                    "shorter"
                                } else if values.len() == dimension {
                                    "equal"
                                } else {
                                    "longer"
                                }
                            ));
                        }
                        SavedValue::Reals { values, .. } => {
                            self.count(format!(
                                "real array encoded/declared {}",
                                if values.len() < dimension {
                                    "shorter"
                                } else if values.len() == dimension {
                                    "equal"
                                } else {
                                    "longer"
                                }
                            ));
                        }
                        SavedValue::Texts(values) => {
                            self.count(format!(
                                "text array encoded/declared {}",
                                if values.len() < dimension {
                                    "shorter"
                                } else if values.len() == dimension {
                                    "equal"
                                } else {
                                    "longer"
                                }
                            ));
                        }
                        _ => (),
                    }
                }
                self.entry(entry.data(), declaration);
            }
        }
        self.slots.push(declarations);
        Ok(())
    }
    fn chunk(&mut self, c: &Chunk) -> Result<(), String> {
        match c.id {
            6 => self.script(c)?,
            0x28 | 0x29 => {
                let object = StructuredObject::try_from(c).map_err(|e| e.to_string())?;
                for child in &object.children {
                    self.chunk(child)?;
                }
            }
            3 => {
                let bank = Bank::try_from(c).map_err(|e| e.to_string())?;
                for child in bank.0.children.iter().filter(|c| c.id == 6 || c.id == 0x29) {
                    self.chunk(child)?;
                }
                for (_, slot) in bank.slot_list().map_err(|e| e.to_string())?.slots {
                    for program in slot.program_list().map_err(|e| e.to_string())?.programs {
                        self.program(&program)?;
                    }
                }
            }
            0x4f => {
                let snapshot = Snapshot::try_from(c).map_err(|e| e.to_string())?;
                self.count(format!("snapshot version {}", snapshot.version));
                self.family = format!("0x4f/{}", snapshot.version);
                for (i, slot) in snapshot.persistent.iter().enumerate() {
                    for entry in slot {
                        let name = entry.split(' ').next().unwrap_or("").to_lowercase();
                        let declaration = self.slots.get(i).and_then(|s| s.get(&name)).copied();
                        self.entry(entry.as_bytes(), declaration);
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn program(&mut self, p: &Program) -> Result<(), String> {
        for child in &p.0.children {
            self.chunk(child)?;
        }
        Ok(())
    }
}
fn walk(path: &Path, files: &mut Vec<std::path::PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| {
            ["nki", "nkm", "nkb", "nksn"]
                .iter()
                .any(|x| e.eq_ignore_ascii_case(x))
        }) {
            files.push(path.to_owned());
        }
    } else if let Ok(dir) = std::fs::read_dir(path) {
        for e in dir.flatten() {
            walk(&e.path(), files);
        }
    }
}
fn main() {
    let root = std::env::args().nth(1).expect("ROOT");
    let mut files = Vec::new();
    walk(Path::new(&root), &mut files);
    files.sort();
    let mut census = Census {
        probe: std::env::args().any(|a| a == "--probe"),
        ..Default::default()
    };
    let mut file_counts = BTreeMap::<String, usize>::new();
    let mut libraries = BTreeMap::<(String, String), usize>::new();
    for (index, path) in files.iter().enumerate() {
        census.path = path.clone();
        census.slots.clear();
        census.file_keys.clear();
        census.library = path
            .strip_prefix(&root)
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        let ext = path.extension().unwrap().to_string_lossy();
        census.count(format!("file {ext}"));
        let result = sampler_kontakt::read_chunks(path)
            .map_err(|e| e.to_string())
            .and_then(|chunks| {
                if ext == "nksn" {
                    if let Some(c) = chunks.find_first(0x51) {
                        let (a, b) = snapshot_metadata_names(c).map_err(|e| e.to_string())?;
                        census.slots = [a, b]
                            .into_iter()
                            .find_map(|name| {
                                census
                                    .contexts
                                    .get(&(census.library.clone(), name))
                                    .cloned()
                            })
                            .unwrap_or_default();
                        if census.slots.is_empty() {
                            census.count("snapshot declaration context missing");
                        }
                    }
                }
                for c in &chunks.0 {
                    census.chunk(c)?;
                }
                if ext == "nki" {
                    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
                    census
                        .contexts
                        .insert((census.library.clone(), stem), census.slots.clone());
                    if let Some(program) = chunks.program() {
                        if let Ok(params) = program.and_then(|p| p.params()) {
                            census.contexts.insert(
                                (census.library.clone(), params.name),
                                census.slots.clone(),
                            );
                        }
                    }
                }
                Ok(())
            });
        if let Err(e) = result {
            println!("FAIL\t{}\t{}", path.display(), e);
            census.count(format!("failed {ext}"));
        }
        for key in &census.file_keys {
            *file_counts.entry(key.clone()).or_default() += 1;
            *libraries
                .entry((census.library.clone(), key.clone()))
                .or_default() += 1;
        }
        if index % 100 == 0 {
            eprintln!("{} / {} files", index, files.len());
        }
    }
    for (k, v) in file_counts {
        println!("FILES\t{k}\t{v}");
    }
    for ((lib, k), v) in libraries {
        if k.starts_with("wire ") || k.starts_with("file ") || k.starts_with("failed") {
            println!("LIBRARY\t{lib}\t{k}\t{v}");
        }
    }
    for (k, v) in census.counts {
        println!("COUNT\t{k}\t{v}");
    }
    for (k, v) in census.shapes {
        println!("SHAPE\t{k}\t{v}");
    }
}
