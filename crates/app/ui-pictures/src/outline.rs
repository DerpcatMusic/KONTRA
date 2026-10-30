//! Optional installed outline face, identified by content rather than bundled.
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const LENGTH: usize = 4_000_340;
const HASH: &str = "12e16fc01063aeb2963c53b63d11f8d102f2ea743852554172ae98dec06f6ad5";

fn valid(bytes: &[u8]) -> bool {
    bytes.len() == LENGTH && format!("{:x}", Sha256::digest(bytes)) == HASH
}

pub(super) fn extract(file: &mut std::fs::File) -> Option<Vec<u8>> {
    // The offset identifies the 8.13.1 resource; the hash rejects other data.
    file.seek(SeekFrom::Start(0x7164750)).ok()?;
    let mut bytes = vec![0; LENGTH];
    file.read_exact(&mut bytes).ok()?;
    valid(&bytes).then_some(bytes)
}

pub(super) fn read(path: &Path) -> Option<Vec<u8>> {
    if path.metadata().ok()?.len() != LENGTH as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    valid(&bytes).then_some(bytes)
}
