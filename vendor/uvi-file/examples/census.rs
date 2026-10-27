//! Decodes every bank given: directory, content key, every program's XML and
//! one sample per extension. Prints one line per bank with stage-tagged
//! failures. `cargo run --release --example census -- A.ufs B.ufs ...`

use std::collections::BTreeMap;
use std::path::Path;

use uvi_file::access::{Namespaces, recover_content_key};
use uvi_file::ufs::{Protection, Ufs};
use uvi_file::{crypto, parse_program_xml};

fn main() {
    for arg in std::env::args_os().skip(1) {
        let path = Path::new(&arg);
        let name = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into());
        match bank(path) {
            Ok(line) => println!("ok\t{name}\t{line}"),
            Err(e) => println!("FAIL\t{name}\t{e:#}"),
        }
    }
}

fn bank(path: &Path) -> anyhow::Result<String> {
    let t = std::time::Instant::now();
    let ns = Namespaces::native();
    let ufs = Ufs::open(path).map_err(|e| e.context("open"))?;
    let dir = ufs.decode_directory(&ns.metadata).map_err(|e| e.context("directory"))?;
    let mut by_ext: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut modes: BTreeMap<String, usize> = BTreeMap::new();
    for m in &dir.files {
        let ext = m.name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
        let e = by_ext.entry(ext).or_default();
        e.0 += 1;
        e.1 += m.size;
        *modes.entry(format!("{:?}", m.mode)).or_default() += 1;
    }
    let needs_key = dir.files.iter().any(|m| m.mode == Protection::Content);
    let key = if needs_key {
        Some(recover_content_key(path, &ufs, &dir).map_err(|e| e.context("content key"))?)
    } else {
        None
    };
    let (mut programs, mut bad_programs, mut first_bad) = (0, 0, None);
    for m in dir.files.iter().filter(|m| m.name.to_ascii_lowercase().ends_with(".uvip")) {
        programs += 1;
        let r = ufs
            .read_member(m, dir.metadata_key, key)
            .and_then(|b| crypto::decode_program_bytes(&b, &ns.program))
            .and_then(|text| parse_program_xml(&text).map(|_| ()));
        if let Err(e) = r {
            bad_programs += 1;
            first_bad.get_or_insert(format!("{}: {e:#}", m.name));
        }
    }
    let mut samples = Vec::new();
    for ext in ["wav", "aif", "aiff", "flac", "ogg", "ncw", "uvs"] {
        if let Some(m) = dir.files.iter().find(|m| m.name.to_ascii_lowercase().ends_with(&format!(".{ext}"))) {
            let (offset, size, k) = ufs.locate(m, dir.metadata_key, key)?;
            let mut head = vec![0; size.min(4096) as usize];
            let mut f = ufs.open_snapshot()?;
            use std::io::{Read, Seek, SeekFrom};
            f.seek(SeekFrom::Start(offset))?;
            f.read_exact(&mut head)?;
            if let Some(k) = k {
                crypto::transform_blocks(&mut head, k, offset);
            }
            samples.push(format!("{ext}:{}", String::from_utf8_lossy(head.get(..4).unwrap_or_default()).escape_debug()));
        }
    }
    Ok(format!(
        "v{} {:.1}s members {} modes {modes:?} ext {by_ext:?} programs {programs} bad {bad_programs}{} samples {samples:?} warnings {}",
        ufs.header.version,
        t.elapsed().as_secs_f64(),
        dir.files.len(),
        first_bad.map(|b| format!(" ({b})")).unwrap_or_default(),
        dir.warnings.len(),
    ))
}
