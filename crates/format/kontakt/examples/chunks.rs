//! ni-file parity census: of every Kontakt file under the roots given, how
//! much of what Kontakt reads KONTRA reads. Prints the markdown baseline kept
//! in docs/re/NI_FILE_PARITY.md.
//!
//! `cargo run --release -p kontra-format-kontakt --example chunks -- <dir or file>...`
//! `KONTRA_CHUNK_DUMP=<dir>` keeps the chunks not read in full, `KONTRA_CENSUS_EXAMPLES=1`
//! names one file per object class on stderr.
//!
//! Every chunk of every chunk stream is walked (structured children and the
//! slots a decoder holds). Each one is judged against Kontakt 8.13.1's own
//! reader for its SerType and stored version (`native_schema`):
//! - **full**: Kontakt's reader accounts for every byte of each non-empty
//!   block of an object whose framing covers the chunk exactly;
//! - **partial**: a reader recognises it but bytes stay unaccounted for
//!   (trailing, truncated, rejected, unmodelled), or only a hand reader with
//!   no byte accounting reads it;
//! - **unrecognised**: no reader for it at all.
//!
//! Presets open through the plugin's own load path (`decode`), as installed
//! libraries do in KONTRA; what it cannot open is protected. No chunk of an
//! encrypted preset is dumped: only counts and class names are kept.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use kontra_format::ProtectionStatus;
use ni_file::kontakt::native_schema::{self, Undecoded};
use ni_file::kontakt::{
    Chunk, KontaktChunks, KontaktObject, StructuredObject, objects::read_param_slots,
};
use ni_file::nkr::Archive;

/// Preset files: a Kontakt chunk stream inside NKS, NIS or a monolith.
const PRESETS: [&str; 4] = ["nki", "nkm", "nkb", "nksn"];
/// Resource archives, the sample cache and library metadata: framing only.
const OTHERS: [&str; 4] = ["nkr", "nkx", "nkc", "nicnt"];

#[derive(Default)]
struct Files {
    total: usize,
    read: usize,
    encrypted: usize,
    protected: usize,
    unreadable: usize,
}

#[derive(Default)]
struct Tally {
    n: usize,
    full: usize,
    partial: BTreeMap<&'static str, usize>,
    unrecognised: usize,
    bytes: usize,
    unaccounted: usize,
    hand: [usize; 4],
}

type Key = (u16, Option<u16>, &'static str);

#[derive(Default)]
struct Census {
    files: BTreeMap<String, Files>,
    why: BTreeMap<String, usize>,
    objects: BTreeMap<Key, Tally>,
    /// Opaque nested bytes: (parent SerType, nested SerType, cause) -> (parents, bytes).
    nested: BTreeMap<(u16, Option<u16>, &'static str), (usize, usize)>,
    /// Whether chunks of the current file may be dumped: never an encrypted one's.
    keep: bool,
}

fn main() {
    let roots: Vec<PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let mut paths = Vec::new();
    for root in &roots {
        collect(root, &mut paths);
    }
    paths.sort();
    let mut c = Census::default();
    for path in &paths {
        c.file(path);
    }
    c.print();
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    match std::fs::read_dir(path) {
        Ok(dir) => dir.flatten().for_each(|e| collect(&e.path(), out)),
        Err(_) => out.push(path.to_path_buf()),
    }
}

fn ext(path: &Path) -> Option<String> {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

impl Census {
    fn file(&mut self, path: &Path) {
        let Some(ext) = ext(path) else { return };
        if OTHERS.contains(&ext.as_str()) {
            return self.container(path, ext);
        }
        if !PRESETS.contains(&ext.as_str()) {
            return;
        }
        // The plugin's own load path: a library's preset opens here exactly
        // when it opens in KONTRA. Only counts and class names leave it.
        let opened = match kontra_format_kontakt::decode(path) {
            Ok((bytes, encrypted)) => KontaktChunks::read(Cursor::new(bytes))
                .map(|c| (c, encrypted == Some(true)))
                .map_err(|e| Some(format!("Kontakt chunks: {e}"))),
            Err(e) if e.protection != ProtectionStatus::Open => Err(None),
            Err(e) => Err(Some(format!("{}: {}", e.what, e.reason))),
        };
        let f = self.files.entry(ext.clone()).or_default();
        f.total += 1;
        match opened {
            Ok((chunks, encrypted)) => {
                f.read += 1;
                f.encrypted += usize::from(encrypted);
                self.keep = !encrypted;
                let file = path.display().to_string();
                for chunk in &chunks.0 {
                    self.walk(chunk, &file, 0);
                }
            }
            Err(None) => f.protected += 1,
            Err(Some(why)) => {
                f.unreadable += 1;
                *self.why.entry(format!(".{ext}: {why}")).or_default() += 1;
            }
        }
    }

    /// A file that is not a chunk stream: only its framing is read for now.
    fn container(&mut self, path: &Path, ext: String) {
        let read = match ext.as_str() {
            "nkr" | "nkx" => std::fs::File::open(path)
                .map_err(|e| e.to_string())
                .and_then(|f| Archive::read(f).map_err(|e| e.to_string()))
                .and_then(|a| a.issues.first().map_or(Ok(()), |i| Err(i.clone()))),
            _ => Err("no reader".into()),
        };
        let f = self.files.entry(ext.clone()).or_default();
        f.total += 1;
        match read {
            Ok(()) => f.read += 1,
            Err(why) => {
                f.unreadable += 1;
                *self.why.entry(format!(".{ext}: {why}")).or_default() += 1;
            }
        }
    }

    fn walk(&mut self, chunk: &Chunk, file: &str, depth: usize) {
        // A serialized object starts with its framing flag: 1 structured
        // (private, public, children), 0 a version and the public bytes only.
        // Unframed records (lists led by a u32 count) can start with either
        // byte, so framing counts only when it covers the chunk exactly, and a
        // 0 only when Kontakt has a reader for that version.
        let structured = chunk.data.first() == Some(&1);
        let object = matches!(chunk.data.first(), Some(0 | 1))
            .then(|| StructuredObject::read(Cursor::new(chunk.data.as_slice())).ok())
            .flatten()
            .filter(|o| {
                if structured {
                    framed(o, true) == chunk.data.len()
                } else {
                    native_schema::accepts(chunk.id, o.version)
                }
            });
        let hand = chunk.into_object();
        let version = object.as_ref().map(|o| o.version);
        let class = version
            .and_then(|v| native_schema::class_of(chunk.id, v))
            .unwrap_or_else(|| hand_name(&hand));
        let t = self.objects.entry((chunk.id, version, class)).or_default();
        if t.n == 0 && std::env::var_os("KONTRA_CENSUS_EXAMPLES").is_some() {
            eprintln!("{:#06x} {class}: {file}", chunk.id);
        }
        t.n += 1;
        let hand_typed = match &hand {
            Ok(KontaktObject::Unsupported(_)) => {
                t.hand[3] += 1;
                false
            }
            Ok(o) if is_named_only(o) => {
                t.hand[2] += 1;
                false
            }
            Ok(_) => {
                t.hand[0] += 1;
                true
            }
            Err(_) => {
                t.hand[1] += 1;
                false
            }
        };
        let verdict = match &object {
            Some(o) => {
                t.bytes += o.public_data.len() + o.private_data.len();
                // Kontakt reads the public and private blocks with separate
                // readers (docs/re/NI_FILE_BINARY_RECORDS.md §2); a class keeps
                // its fields in one, and the catalog reader is that one's.
                let mut blocks: Vec<&[u8]> = [&o.public_data, &o.private_data]
                    .into_iter()
                    .filter(|b| !b.is_empty())
                    .map(Vec::as_slice)
                    .collect();
                if blocks.is_empty() {
                    blocks.push(&[]);
                }
                let mut why = None;
                let mut miss = 0;
                for block in blocks {
                    let (w, n) = match native_schema::decode(chunk.id, o.version, block) {
                        Ok(d) if d.opaque.is_empty() => continue,
                        Ok(d) => {
                            for o in &d.opaque {
                                let e =
                                    self.nested.entry((chunk.id, o.sertype, o.why)).or_default();
                                e.0 += 1;
                                e.1 += o.bytes;
                            }
                            ("nested object not decoded", d.opaque_bytes())
                        }
                        Err(Undecoded::Trailing(n)) => ("trailing bytes", n),
                        Err(Undecoded::NoReader) if hand_typed => ("hand reader only", block.len()),
                        Err(Undecoded::NoReader) => ("", block.len()),
                        Err(e) => (reason(&e), block.len()),
                    };
                    why.get_or_insert(w);
                    miss += n;
                }
                why.map_or(Ok(()), |w| Err((w, miss)))
            }
            None => {
                t.bytes += chunk.data.len();
                Err((
                    if hand_typed {
                        "raw, hand reader only"
                    } else {
                        ""
                    },
                    chunk.data.len(),
                ))
            }
        };
        match verdict {
            Ok(()) => t.full += 1,
            Err(("", n)) => {
                t.unrecognised += 1;
                t.unaccounted += n;
            }
            Err((why, n)) => {
                *t.partial.entry(why).or_default() += 1;
                t.unaccounted += n;
            }
        }
        if verdict.is_err() && self.keep {
            dump(chunk, t.n);
        }
        let mut nested = hand.map(inner_chunks).unwrap_or_default();
        if depth < 16 {
            nested.extend(object.into_iter().flat_map(|o| o.children));
            for c in &nested {
                self.walk(c, file, depth + 1);
            }
        }
    }

    fn print(&self) {
        println!("## Files\n");
        println!("| Type | Files | Read | of them encrypted | Protected | Unreadable |");
        println!("| --- | ---: | ---: | ---: | ---: | ---: |");
        for (ext, f) in &self.files {
            println!(
                "| .{ext} | {} | {} | {} | {} | {} |",
                f.total, f.read, f.encrypted, f.protected, f.unreadable
            );
        }
        if !self.why.is_empty() {
            println!("\nUnreadable:\n");
            for (why, n) in &self.why {
                println!("- {n} × {why}");
            }
        }
        let sum = |f: fn(&Tally) -> usize| self.objects.values().map(f).sum::<usize>();
        let partial = |t: &Tally| t.partial.values().sum::<usize>();
        println!("\n## Objects\n");
        println!(
            "{} objects: {} full, {} partial, {} unrecognised. Bytes {}, unaccounted for {}.\n",
            sum(|t| t.n),
            sum(|t| t.full),
            sum(partial),
            sum(|t| t.unrecognised),
            sum(|t| t.bytes),
            sum(|t| t.unaccounted),
        );
        let mut reasons = BTreeMap::<&str, usize>::new();
        for t in self.objects.values() {
            for (why, n) in &t.partial {
                *reasons.entry(why).or_default() += n;
            }
        }
        println!("| Partial because | Objects |\n| --- | ---: |");
        for (why, n) in &reasons {
            println!("| {why} | {n} |");
        }
        println!(
            "\nNested objects not decoded, by parent, nested SerType and cause (bytes overlap \
             when nested objects nest further).\n"
        );
        println!(
            "| Parent | Nested | Cause | Parents | Bytes |\n| --- | --- | --- | ---: | ---: |"
        );
        let mut nested: Vec<_> = self.nested.iter().collect();
        nested.sort_by_key(|(k, (_, b))| (std::cmp::Reverse(*b), **k));
        for ((parent, sertype, why), (n, bytes)) in nested {
            let sertype = sertype.map_or("unnamed".into(), |t| format!("{t:#06x}"));
            println!("| {parent:#06x} | {sertype} | {why} | {n} | {bytes} |");
        }
        println!(
            "\nBy SerType, stored version and class, most unaccounted bytes first. Hand: KONTRA's \
             typed decoder succeeded / failed / names the id only / does not know it.\n"
        );
        println!(
            "| SerType | Version | Class | Objects | Full | Partial | Unrecognised | Bytes | Unaccounted | Hand |"
        );
        println!("| --- | --- | --- | ---: | ---: | --- | ---: | ---: | ---: | --- |");
        let mut rows: Vec<_> = self.objects.iter().collect();
        rows.sort_by_key(|(k, t)| {
            (
                std::cmp::Reverse(t.unaccounted),
                std::cmp::Reverse(t.n),
                **k,
            )
        });
        for ((id, version, class), t) in rows {
            let partial = t
                .partial
                .iter()
                .map(|(why, n)| format!("{n} {why}"))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "| {id:#06x} | {} | {class} | {} | {} | {partial} | {} | {} | {} | {} |",
                version.map_or("raw".into(), |v| format!("{v:#x}")),
                t.n,
                t.full,
                t.unrecognised,
                t.bytes,
                t.unaccounted,
                t.hand.map(|n| n.to_string()).join("/"),
            );
        }
    }
}

/// The bytes a structured object's framing covers.
fn framed(o: &StructuredObject, structured: bool) -> usize {
    if !structured {
        return 1 + 2 + o.public_data.len();
    }
    let children: usize = o.children.iter().map(|c| 6 + c.data.len()).sum();
    1 + 2 + 4 + o.private_data.len() + 4 + o.public_data.len() + 4 + children
}

fn reason(e: &Undecoded) -> &'static str {
    match e {
        Undecoded::Truncated => "truncated for Kontakt's reader",
        Undecoded::Rejected => "Kontakt's reader rejects it",
        Undecoded::Unresolved(why) => leak(format!("reader not modelled: {why}")),
        Undecoded::Budget => "host-state search over budget",
        Undecoded::Trailing(_) => "trailing bytes",
        Undecoded::NoReader => "no reader",
    }
}

/// The hand reader's name for a chunk with no native class.
fn hand_name(hand: &Result<KontaktObject, ni_file::Error>) -> &'static str {
    let Ok(o) = hand else { return "?" };
    let name = format!("{o:?}");
    let name = name.split(['(', ' ', '{']).next().unwrap_or("?");
    if name == "Unsupported" {
        return "?";
    }
    leak(name.to_owned())
}

/// A class or reason name, kept for the whole run (few distinct ones).
fn leak(s: String) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static SEEN: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut seen = SEEN
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let seen = seen.get_or_insert_with(HashSet::new);
    if let Some(&s) = seen.get(s.as_str()) {
        return s;
    }
    let s: &'static str = Box::leak(s.into_boxed_str());
    seen.insert(s);
    s
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

/// With `KONTRA_CHUNK_DUMP=<dir>`, writes chunks not read in full there as
/// `<id>-<n>.bin` for study.
fn dump(chunk: &Chunk, n: usize) {
    if let Some(dir) = std::env::var_os("KONTRA_CHUNK_DUMP") {
        let path = Path::new(&dir).join(format!("{:04x}-{n}.bin", chunk.id));
        if let Err(e) = std::fs::write(&path, &chunk.data) {
            eprintln!("{}: {e}", path.display());
        }
    }
}

/// Variants that only name the id: the bytes are not decoded.
fn is_named_only(o: &KontaktObject) -> bool {
    !format!("{o:?}").contains('(')
}
