//! Local format check: prints names, counts and errors, never keys or preset content.
//! Build with rustc using the project's existing ni_file/aes/anyhow/serde_json rlibs.
use std::{fs, io::{self, Cursor, Write}, path::Path, time::Instant};
use ni_file::{NIFile, kontakt::KontaktChunks};
#[path = "../src/access.rs"]
mod access;
struct Compare<'a> { original: &'a [u8], position: usize }
impl Write for Compare<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.position.checked_add(bytes.len()).ok_or_else(|| io::Error::other("write length overflow"))?;
        if self.original.get(self.position..end) != Some(bytes) { return Err(io::Error::other("roundtrip bytes differ")); }
        self.position = end;Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}
fn check(path: &Path) -> anyhow::Result<serde_json::Value> {
    let bytes = fs::read(path)?;
    let start = Instant::now();
    let file = NIFile::read(Cursor::new(&bytes))?;
    let mut compare = Compare { original: &bytes, position: 0 };
    file.write(&mut compare)?;
    anyhow::ensure!(compare.position == bytes.len(), "roundtrip omitted file bytes");
    let key = access::library_key(path)?;
    let preset = file.inner_preset_with_key(key.as_deref())?;
    let encrypted = file.inner_preset().is_err();
    let chunks = KontaktChunks::read(Cursor::new(&preset))?;
    let mut compare = Compare { original: &preset, position: 0 };
    chunks.write(&mut compare)?;
    anyhow::ensure!(compare.position == preset.len(), "roundtrip omitted preset bytes");
    Ok(serde_json::json!({"path":path,"file_bytes":bytes.len(),"preset_bytes":preset.len(),"chunks":chunks.0.len(),"requires_caller_key":encrypted,"file_roundtrip_exact":true,"raw_preset_roundtrip_exact":true,"read_roundtrip_ms":start.elapsed().as_secs_f64()*1000.0,"decoded_sample_payloads_verified":false}))
}
fn main() {
    for arg in std::env::args_os().skip(1) {
        let path = Path::new(&arg);
        let result = std::panic::catch_unwind(|| check(path));
        let row = match result {
            Ok(Ok(row)) => row,
            Ok(Err(error)) => serde_json::json!({"path":path,"error":format!("{error:#}")}),
            Err(_) => serde_json::json!({"path":path,"error":"Parser panic"}),
        };
        println!("{}", row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_rejects_mismatch_extra_bytes_and_overflow() {
        let mut writer = Compare { original: b"authored", position: 0 };
        writer.write_all(b"author").unwrap();
        assert!(writer.write_all(b"xx").is_err());
        assert_eq!(writer.position, 6);
        writer.write_all(b"ed").unwrap();
        assert_eq!(writer.position, writer.original.len());
        assert!(writer.write_all(b"extra").is_err());
        writer.position = usize::MAX;
        assert!(writer.write_all(b"x").is_err());
    }
}
