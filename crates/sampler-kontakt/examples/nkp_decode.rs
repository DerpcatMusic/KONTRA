//! Decode every object of Kontakt preset/instrument files with Kontakt's own reader programs
//! (ni_file native_schema). Never writes preset payloads beyond numeric field values.
//!
//!   nkp_decode survey PATH...   per class/version: files, objects, decoded exactly, rejected+reason
//!   nkp_decode dump FILE        one row per decoded field: path class ver idx off kind depth slot value
//!
//! `cargo run -p sampler-kontakt --example nkp_decode -- survey <Presets dir>`
use ni_file::kontakt::{Chunk, StructuredObject, native_schema as ns};
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn files(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            files(&e.path(), out);
        }
    } else if p
        .extension()
        .is_some_and(|e| ["nkp", "nki"].iter().any(|x| e.eq_ignore_ascii_case(x)))
    {
        out.push(p.to_path_buf());
    }
}

/// Visit every structured object: (id path, chunk id, object).
fn walk(chunk: &Chunk, path: &str, f: &mut impl FnMut(&str, u16, &StructuredObject)) {
    // Effect racks are unstructured arrays of BParFX records: walk each occupied slot.
    if chunk.id == 0x3a {
        if let Ok(a) = ni_file::kontakt::objects::BParamArrayBParFX8::read(Cursor::new(&chunk.data), 8) {
            for (n, c) in a.items.iter().enumerate() {
                if let Some(c) = c {
                    walk(c, &format!("{path}/3a[{n}]"), f);
                }
            }
            return;
        }
    }
    let Ok(so) = StructuredObject::read(Cursor::new(&chunk.data)) else {
        return;
    };
    let here = format!("{path}/{:x}", chunk.id);
    if std::env::var_os("TREE").is_some() {
        if std::env::var_os("HEX").is_some() { eprintln!("{:02x?}", chunk.data); }
        eprintln!("{here} v0x{:x} pub {} priv {} children {}", so.version, so.public_data.len(), so.private_data.len(), so.children.len());
    }
    f(&here, chunk.id, &so);
    for c in &so.children {
        walk(c, &here, f);
    }
}

/// NKP preset wrapper (0x30): `00 tag` then one BParFX (single-effect presets) or a
/// BParameterArray<BParFX,8> (effect-chain presets). Other preset kinds are not effects.
fn preset_objects(chunk: &Chunk) -> Vec<Chunk> {
    let Ok(so) = StructuredObject::read(Cursor::new(&chunk.data)) else { return vec![] };
    let Some(body) = so.public_data.get(2..) else { return vec![] };
    match body.first() {
        Some(1) => vec![Chunk { id: 0x25, data: body.to_vec() }],
        Some(0) => ni_file::kontakt::objects::BParamArrayBParFX8::read(Cursor::new(body), 8)
            .map(|a| a.items.into_iter().flatten().collect())
            .unwrap_or_default(),
        _ => vec![],
    }
}

fn decode(id: u16, so: &StructuredObject) -> Result<ns::Decoded, String> {
    if !ns::accepts(id, so.version) {
        return Err(if ns::class_of(id, so.version).is_some() { "version-not-accepted" } else { "no-reader" }.into());
    }
    ns::decode(id, so.version, if id == 0x25 { &so.private_data } else { &so.public_data }).map_err(|e| format!("{e:?}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("");
    let mut paths = Vec::new();
    for a in &args[1..] {
        files(Path::new(a), &mut paths);
    }
    paths.sort();
    // (id, version) -> (class, files, ok, rejected reason -> count)
    let mut stats: BTreeMap<(u16, u16), (String, usize, usize, BTreeMap<String, usize>)> = BTreeMap::new();
    let mut unreadable = 0;
    for p in &paths {
        let chunks = match sampler_kontakt::read_chunks(p) {
            Ok(c) => c,
            Err(e) => {
                if std::env::var_os("TREE").is_some() { eprintln!("UNREADABLE {}: {e}", p.display()); }
                unreadable += 1;
            continue;
            }
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut roots: Vec<Chunk> = Vec::new();
        for c in &chunks.0 {
            if c.id == 0x30 { roots.extend(preset_objects(c)); } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nki")) { roots.push(Chunk { id: c.id, data: c.data.clone() }); }
        }
        for c in &roots {
            walk(c, "", &mut |path, id, so| {
                let r = decode(id, so);
                if mode == "dump" {
                    if let Ok(d) = &r {
                        for (i, f) in d.fields.iter().enumerate() {
                            let v = f.as_f32().map(|x| format!("{x}")).or(f.value.map(|v| v.to_string())).unwrap_or_default();
                            println!("{path}\t{}\t0x{:x}\t{i}\t{}\t{}\t{}\t{}\t{v}", d.class, so.version, f.offset, f.kind, f.depth, f.slot.map(|s| s.to_string()).unwrap_or_default());
                        }
                    }
                    return;
                }
                let e = stats.entry((id, so.version)).or_insert_with(|| (ns::class_of(id, so.version).unwrap_or("?").into(), 0, 0, BTreeMap::new()));
                if seen.insert((id, so.version)) {
                    e.1 += 1;
                }
                match r {
                    Ok(_) => e.2 += 1,
                    Err(m) => *e.3.entry(m).or_default() += 1,
                }
            });
        }
    }
    if mode != "dump" {
        println!("# files {} unreadable {unreadable}", paths.len());
        println!("id\tclass\tversion\tfiles\tobjects\tdecoded\trejected\treasons");
        for ((id, v), (class, files, ok, bad)) in &stats {
            let n: usize = bad.values().sum();
            let reasons: Vec<_> = bad.iter().map(|(k, c)| format!("{k}x{c}")).collect();
            println!("0x{id:x}\t{class}\t0x{v:x}\t{files}\t{}\t{ok}\t{n}\t{}", ok + n, reasons.join("; "));
        }
    }
}
