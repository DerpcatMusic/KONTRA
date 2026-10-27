//! Decode coverage of Kontakt files: walks every chunk of every file given
//! and counts, per chunk id and version, how ni-file handles it: a typed
//! decoder that succeeds, one that fails, a named id with no decoder, or an
//! unknown id. `cargo run --release -p kontra-format-kontakt --example chunks -- <files>`
//! (or paths on stdin)

use std::collections::BTreeMap;

use ni_file::kontakt::{Chunk, KontaktObject, StructuredObject};

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
    match chunk.into_object() {
        Ok(KontaktObject::Unsupported(_)) => t.unknown += 1,
        Ok(o) if is_named_only(&o) => t.named += 1,
        Ok(_) => t.typed += 1,
        Err(e) => {
            t.failed += 1;
            if t.example.is_empty() {
                t.example = format!("{e} ({file})");
            }
        }
    }
    if depth < 16
        && let Some(o) = object
    {
        for c in &o.children {
            walk(c, by_id, file, depth + 1);
        }
    }
}

/// Variants that only name the id: the bytes are not decoded.
fn is_named_only(o: &KontaktObject) -> bool {
    let name = format!("{o:?}");
    !name.contains('(')
}
