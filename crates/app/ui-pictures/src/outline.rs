//! Optional installed outline face, identified by content rather than bundled.
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const LENGTH: usize = 194_293;
const HASH: &str = "a62fbeada38ee2011fbff83531bb868a0b7555ad1e897f2244eaa57e19d7c8a3";

fn valid(bytes: &[u8]) -> bool {
    bytes.len() == LENGTH && format!("{:x}", Sha256::digest(bytes)) == HASH
}

pub(super) fn extract(file: &mut std::fs::File) -> Option<Vec<u8>> {
    // The offset identifies the 8.13.1 resource; the hash rejects other data.
    file.seek(SeekFrom::Start(0x6d84000)).ok()?;
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
