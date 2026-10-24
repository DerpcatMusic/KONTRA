//! Exact heap totals from Luau's own heap dump (`lua_memorydump`).
//!
//! The dump is one JSON object per line, `"0xADDR":{"type":…,"cat":…,"size":…,…}`.
//! `size` is the collector's own byte count for that object, including a table's
//! node and array parts, a proto's bytecode and constants, and a thread's stack.
//! Parsing only the header of each line keeps this O(heap) with no JSON tree.

use std::{
    ffi::{CString, c_void},
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

use mlua::{Lua, ffi};

/// One object type's live total after the dump.
#[derive(Debug, Clone, Copy)]
pub struct TypeTotal {
    pub kind: &'static str,
    pub objects: u64,
    pub bytes: u64,
}

/// The whole heap, split by collector object type.
#[derive(Debug, Default)]
pub struct Totals {
    pub objects: u64,
    pub bytes: u64,
    pub by_type: Vec<TypeTotal>,
    /// The largest single objects, biggest first.
    pub largest: Vec<Largest>,
}

/// How many objects [`Totals::largest`] keeps.
const LARGEST_KEPT: usize = 10;

/// One object, by collector size.
#[derive(Debug, Clone, Copy)]
pub struct Largest {
    pub kind: &'static str,
    pub bytes: u64,
}

impl Totals {
    /// Bytes of one type, zero when the type is absent.
    #[must_use]
    pub fn bytes(&self, kind: &str) -> u64 {
        self.by_type
            .iter()
            .find(|t| t.kind == kind)
            .map_or(0, |t| t.bytes)
    }

    /// Objects of one type, zero when the type is absent.
    #[must_use]
    pub fn objects(&self, kind: &str) -> u64 {
        self.by_type
            .iter()
            .find(|t| t.kind == kind)
            .map_or(0, |t| t.objects)
    }
}

/// Writes a heap dump for `lua` to `path` and returns its per-type totals.
///
/// # Errors
/// Fails when the path is not a valid C string, the file cannot be opened, or
/// a dump line's header cannot be read.
pub fn dump(lua: &Lua, path: &Path) -> Result<Totals, String> {
    let path = path
        .to_str()
        .ok_or_else(|| format!("dump path is not UTF-8: {}", path.display()))?;
    let path = CString::new(path).map_err(|e| format!("dump path: {e}"))?;
    // SAFETY: `path` is a NUL-terminated C string naming a writable location;
    // `fopen` returns null on failure, which is checked before use.
    let file = unsafe { libc::fopen(path.as_ptr(), c"w".as_ptr()) };
    if file.is_null() {
        return Err(format!("cannot create heap dump {}", path.to_string_lossy()));
    }
    // SAFETY: `file` is a live stream opened above and `lua.state()` is the live
    // state of `lua`. The dump only walks the collector and writes to `file`.
    unsafe { ffi::lua_memorydump(lua.state(), file.cast::<c_void>(), None) };
    // SAFETY: `file` was opened above, has not been closed, and is not used after.
    unsafe { libc::fclose(file) };

    let result = parse(&path.to_string_lossy());
    if std::env::var_os("LUAU_GC_KEEP_DUMP").is_none() {
        let _ = std::fs::remove_file(path.to_string_lossy().as_ref());
    } else {
        eprintln!("kept heap dump at {}", path.to_string_lossy());
    }
    result
}

/// Reads a dump file into per-type totals.
fn parse(path: &str) -> Result<Totals, String> {
    let file = File::open(path).map_err(|e| format!("open dump {path}: {e}"))?;
    let mut totals = Totals::default();
    let mut line = Vec::new();
    let mut reader = BufReader::with_capacity(1 << 20, file);
    loop {
        line.clear();
        let n = reader
            .read_until(b'\n', &mut line)
            .map_err(|e| format!("read dump: {e}"))?;
        if n == 0 {
            break;
        }
        let Some((kind, bytes)) = header(&line) else {
            continue;
        };
        let entry = match totals.by_type.iter_mut().find(|t| t.kind == kind) {
            Some(entry) => entry,
            None => {
                totals.by_type.push(TypeTotal {
                    kind,
                    objects: 0,
                    bytes: 0,
                });
                totals.by_type.last_mut().expect("just pushed")
            }
        };
        entry.objects += 1;
        entry.bytes += bytes;
        totals.objects += 1;
        totals.bytes += bytes;
        if totals.largest.len() < LARGEST_KEPT || bytes > totals.largest.last().map_or(0, |l| l.bytes) {
            let at = totals.largest.partition_point(|l| l.bytes >= bytes);
            totals.largest.insert(at, Largest { kind, bytes });
            totals.largest.truncate(LARGEST_KEPT);
        }
    }
    totals.by_type.sort_unstable_by(|a, b| b.bytes.cmp(&a.bytes));
    Ok(totals)
}

/// The `type`/`size` header of one dump line, if it is an object line.
fn header(line: &[u8]) -> Option<(&'static str, u64)> {
    if !line.starts_with(b"\"0x") {
        return None;
    }
    let text = std::str::from_utf8(line).ok()?;
    let at = text.find("\"type\":\"")?;
    let rest = &text[at + 8..];
    let end = rest.find('"')?;
    let kind = match &rest[..end] {
        "string" => "string",
        "table" => "table",
        "function" => "function",
        "userdata" => "userdata",
        "thread" => "thread",
        "buffer" => "buffer",
        "proto" => "proto",
        "upvalue" => "upvalue",
        "class" => "class",
        "object" => "object",
        _ => return None,
    };
    let at = text.find("\"size\":")? + 7;
    let rest = &text[at..];
    let end = rest.find(|c: char| !c.is_ascii_digit())?;
    Some((kind, rest[..end].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_type_and_size_from_an_object_line() {
        let line = br#""0x1a2b":{"type":"table","cat":0,"size":1234,"node":[],"array":[]},
"#;
        assert_eq!(header(line), Some(("table", 1234)));
    }

    #[test]
    fn ignores_the_roots_and_stats_lines() {
        assert_eq!(header(br#""mainthread":"0x1","#), None);
        assert_eq!(header(br#""0": {"type":"userdata"}"#), None);
    }

    #[test]
    fn does_not_read_a_size_from_the_string_payload() {
        let line = br#""0x1":{"type":"string","cat":0,"size":20,"data":"\"size\":999"},
"#;
        assert_eq!(header(line), Some(("string", 20)));
    }

    #[test]
    fn keeps_the_largest_objects_sorted_and_bounded() {
        let path = std::env::temp_dir().join("kontra-luau-gc-largest-test.json");
        let mut text = String::new();
        for size in [10, 900, 400, 100, 800, 200, 700, 300, 600, 500, 1000, 50] {
            text.push_str(&format!(
                "\"0x{size}\":{{\"type\":\"table\",\"cat\":0,\"size\":{size}}},\n"
            ));
        }
        std::fs::write(&path, text).expect("write fixture");
        let totals = parse(path.to_str().expect("utf-8 path")).expect("parse fixture");
        let _ = std::fs::remove_file(&path);

        let sizes: Vec<u64> = totals.largest.iter().map(|l| l.bytes).collect();
        assert_eq!(sizes, vec![1000, 900, 800, 700, 600, 500, 400, 300, 200, 100]);
        assert_eq!(totals.objects, 12);
    }
}
