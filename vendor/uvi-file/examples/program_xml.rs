//! Writes every program of a bank as XML text to a directory.
//! `cargo run --release --example program_xml -- bank.ufs out/`
use uvi_file::access::{Namespaces, recover_content_key};
use uvi_file::{
    crypto,
    ufs::{Protection, Ufs},
};

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let [bank, out] = &a[..] else {
        anyhow::bail!("bank out-dir")
    };
    let ns = Namespaces::native();
    let ufs = Ufs::open(bank.as_ref())?;
    let dir = ufs.decode_directory(&ns.metadata)?;
    let key = if dir.files.iter().any(|m| m.mode == Protection::Content) {
        Some(recover_content_key(bank.as_ref(), &ufs, &dir)?)
    } else {
        None
    };
    for (n, m) in dir
        .files
        .iter()
        .filter(|m| m.name.to_ascii_lowercase().ends_with(".uvip"))
        .enumerate()
    {
        let text =
            crypto::decode_program_bytes(&ufs.read_member(m, dir.metadata_key, key)?, &ns.program)?;
        // The whole path, numbered: leaf names repeat across folders, and
        // flattened paths can collide.
        let name = m
            .path
            .as_deref()
            .unwrap_or(&m.name)
            .replace(['/', '\\'], "_");
        let name = format!("{n:04}_{name}");
        std::fs::write(std::path::Path::new(out).join(name), text)?;
    }
    Ok(())
}
