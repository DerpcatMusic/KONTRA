//! Optional installed outline faces, identified by content rather than bundled.
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// A face stored as plain bytes at a fixed place in the 8.13.1 executable.
pub(super) struct Face {
    at: u64,
    len: usize,
    hash: &'static str,
}

/// The library's label outline face (`@stock/raldo`).
pub(super) const RALDO: Face = Face {
    at: 0x716_4750,
    len: 4_000_340,
    hash: "12e16fc01063aeb2963c53b63d11f8d102f2ea743852554172ae98dec06f6ad5",
};

/// The face stock fonts 1 and 13 are drawn in (the reference 8.13, the
/// text and fields probes: its hinted glyphs at 11 and 10 px per em).
pub(super) const STOCK: Face = Face {
    at: 0x6d8_4000,
    len: 194_293,
    hash: "a62fbeada38ee2011fbff83531bb868a0b7555ad1e897f2244eaa57e19d7c8a3",
};

impl Face {
    fn valid(&self, bytes: &[u8]) -> bool {
        bytes.len() == self.len && format!("{:x}", Sha256::digest(bytes)) == self.hash
    }

    pub(super) fn extract(&self, file: &mut std::fs::File) -> Option<Vec<u8>> {
        // The offset identifies the 8.13.1 resource; the hash rejects other data.
        file.seek(SeekFrom::Start(self.at)).ok()?;
        let mut bytes = vec![0; self.len];
        file.read_exact(&mut bytes).ok()?;
        self.valid(&bytes).then_some(bytes)
    }

    pub(super) fn read(&self, path: &Path) -> Option<Vec<u8>> {
        if path.metadata().ok()?.len() != self.len as u64 {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        self.valid(&bytes).then_some(bytes)
    }
}
