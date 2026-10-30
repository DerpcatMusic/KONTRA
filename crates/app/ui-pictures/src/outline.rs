//! Optional installed outline face, identified by content rather than bundled.
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const FONTS: [(u64, usize, &str); 2] = [
    (
        0x6d84000,
        194_293,
        "a62fbeada38ee2011fbff83531bb868a0b7555ad1e897f2244eaa57e19d7c8a3",
    ),
    (
        0x7164750,
        4_000_340,
        "12e16fc01063aeb2963c53b63d11f8d102f2ea743852554172ae98dec06f6ad5",
    ),
];

fn valid(bytes: &[u8], index: usize) -> bool {
    FONTS.get(index).is_some_and(|(_, length, hash)| {
        bytes.len() == *length && format!("{:x}", Sha256::digest(bytes)) == *hash
    })
}

pub(super) fn extract(file: &mut std::fs::File, index: usize) -> Option<Vec<u8>> {
    // The offset identifies the 8.13.1 resource; the hash rejects other data.
    let (offset, length, _) = *FONTS.get(index)?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes).ok()?;
    valid(&bytes, index).then_some(bytes)
}

pub(super) fn read(path: &Path, index: usize) -> Option<Vec<u8>> {
    let (_, length, _) = FONTS.get(index)?;
    if path.metadata().ok()?.len() != *length as u64 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    valid(&bytes, index).then_some(bytes)
}
