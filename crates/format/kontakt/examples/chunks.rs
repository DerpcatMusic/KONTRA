//! Decode coverage of Kontakt files: walks every chunk of every file given
//! and counts, per chunk id and version, how ni-file handles it: a typed
//! decoder that succeeds, one that fails, a named id with no decoder, or an
//! unknown id. `cargo run --release -p kontra-format-kontakt --example chunks -- <files>`
//! (or paths on stdin). `KONTRA_CHUNK_DUMP=<dir>` keeps the undecoded chunks.

use std::collections::BTreeMap;

use ni_file::kontakt::{Chunk, KontaktObject, StructuredObject, objects::read_param_slots};

#[derive(Default)]
struct Tally {
    typed: usize,
    failed: usize,
    named: usize,
    unknown: usize,
    example: String,
}

fn main() {
    let mut by_id: BTreeMap<(u16, u16), Tally> = BTreeMap::new();
    let (mut files, mut unreadable) = (0, BTreeMap::<String, usize>::new());
    let mut paths: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    if paths.is_empty() {
        // One path per line on stdin, for more files than a command line holds.
        paths = std::io::stdin()
            .lines()
            .map_while(Result::ok)
            .map(Into::into)
            .collect();
    }
    for path in paths {
        files += 1;
        match kontra_format_kontakt::chunks(&path) {
            Ok(chunks) => {
                for c in &chunks.0 {
                    walk(c, &mut by_id, &path.display().to_string(), 0);
                }
            }
            Err(e) => {
                *unreadable
                    .entry(format!("{}: {}", e.what, e.reason))
                    .or_default() += 1
            }
        }
    }
    println!(
        "files {files}, unreadable {}",
        unreadable.values().sum::<usize>()
    );
    for (why, n) in &unreadable {
        println!("  unreadable {n}× {why}");
    }
    println!("id\tversion\ttyped\tfailed\tnamed-only\tunknown\texample");
    for ((id, version), t) in &by_id {
        println!(
            "{id:#06x}\t{version:#x}\t{}\t{}\t{}\t{}\t{}",
            t.typed, t.failed, t.named, t.unknown, t.example
        );
    }
}

fn walk(chunk: &Chunk, by_id: &mut BTreeMap<(u16, u16), Tally>, file: &str, depth: usize) {
    let object = StructuredObject::read(std::io::Cursor::new(chunk.data.as_slice())).ok();
    let version = object.as_ref().map_or(0, |o| o.version);
    let t = by_id.entry((chunk.id, version)).or_default();
    let mut nested = Vec::new();
    match chunk.into_object() {
        Ok(KontaktObject::Unsupported(_)) => {
            t.unknown += 1;
            dump(chunk, t.unknown + t.named);
        }
        Ok(o) if is_named_only(&o) => {
            t.named += 1;
            dump(chunk, t.unknown + t.named);
        }
        Ok(o) => {
            t.typed += 1;
            nested = inner_chunks(o);
        }
        Err(e) => {
            t.failed += 1;
            if t.example.is_empty() {
                t.example = format!("{e} ({file})");
            }
        }
    }
    if depth < 16 {
        nested.extend(object.into_iter().flat_map(|o| o.children));
        for c in &nested {
            walk(c, by_id, file, depth + 1);
        }
    }
}

/// Chunks a decoder holds outside the structured children: parameter-array
/// slots and a snapshot's group records.
fn inner_chunks(o: KontaktObject) -> Vec<Chunk> {
    let slots = |o: &StructuredObject, count: usize| {
        read_param_slots(o, count)
            .map_or_else(|_| Vec::new(), |s| s.into_iter().map(|(_, c)| c).collect())
    };
    match o {
        KontaktObject::BParameterArraySerBParFX8(a) => a.items.into_iter().flatten().collect(),
        KontaktObject::BParameterArraySerBParInternalMod16(a) => slots(&a.0, 16),
        KontaktObject::BParameterArraySerBParExternalMod32(a) => {
            slots(&a.0, a.slot_count().unwrap_or(32))
        }
        KontaktObject::BSnapshot(s) => {
            let groups = s.group_snapshots().unwrap_or_default();
            let mut out = s.effect_children;
            for (_, g) in groups {
                for array in [g.fx, g.internal, g.external] {
                    out.extend(array.items.into_iter().flatten());
                }
            }
            out
        }
        KontaktObject::BOutputConfiguration(o) => {
            o.partitions.into_iter().flat_map(|p| p.fx).collect()
        }
        _ => Vec::new(),
    }
}

/// With `KONTRA_CHUNK_DUMP=<dir>`, writes undecoded chunks there as
/// `<id>-<n>.bin` for study.
fn dump(chunk: &Chunk, n: usize) {
    if let Some(dir) = std::env::var_os("KONTRA_CHUNK_DUMP") {
        let path = std::path::Path::new(&dir).join(format!("{:04x}-{n}.bin", chunk.id));
        if let Err(e) = std::fs::write(&path, &chunk.data) {
            eprintln!("{}: {e}", path.display());
        }
    }
}

/// Variants that only name the id: the bytes are not decoded.
fn is_named_only(o: &KontaktObject) -> bool {
    let name = format!("{o:?}");
    !name.contains('(')
}
