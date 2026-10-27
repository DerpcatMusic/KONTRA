//! Writes every member whose name ends with the given suffix to a directory.
//! `cargo run --release --example dump_member -- bank.ufs .asd out/`
use std::io::{Read, Seek, SeekFrom};
use uvi_file::access::{Namespaces, recover_content_key};
use uvi_file::{
    crypto,
    ufs::{Protection, Ufs},
};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [bank, suffix, out] = &a[..] else {
        anyhow::bail!("bank suffix out-dir")
    };
    let ns = Namespaces::native();
    let ufs = Ufs::open(bank.as_ref())?;
    let dir = ufs.decode_directory(&ns.metadata)?;
    let key = if dir.files.iter().any(|m| m.mode == Protection::Content) {
        Some(recover_content_key(bank.as_ref(), &ufs, &dir)?)
    } else {
        None
    };
    for m in dir
        .files
        .iter()
        .filter(|m| m.name.to_ascii_lowercase().ends_with(suffix.as_str()))
    {
        let (offset, size, k) = ufs.locate(m, dir.metadata_key, key)?;
        let mut buf = vec![0; size as usize];
        let mut f = ufs.open_snapshot()?;
        f.seek(SeekFrom::Start(offset))?;
        f.read_exact(&mut buf)?;
        if let Some(k) = k {
            crypto::transform_blocks(&mut buf, k, offset);
        }
        let name = m.name.replace(['/', '\\'], "_");
        std::fs::write(std::path::Path::new(out).join(&name), &buf)?;
        println!("{} {}", m.name, buf.len());
    }
    Ok(())
}
