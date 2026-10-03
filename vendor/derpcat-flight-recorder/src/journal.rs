use crate::Record;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const MAGIC: &[u8; 8] = b"DFRSLT01";
const SCHEMA: u16 = 1;
const SLOT_BYTES: usize = 1_024;
const HEADER_BYTES: usize = 52;
const PAYLOAD_BYTES: usize = SLOT_BYTES - HEADER_BYTES;

pub(crate) struct Journal {
    file: File,
    slots: u64,
}

impl Journal {
    pub(crate) fn create(path: &Path, requested_bytes: usize) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Zero selects an append-only session journal; positive budgets retain ring behavior.
        let slots = if requested_bytes == 0 {
            0
        } else {
            (requested_bytes / SLOT_BYTES).max(8)
        };
        let bytes = u64::try_from(slots.saturating_mul(SLOT_BYTES))
            .map_err(|_| std::io::Error::other("journal size does not fit u64"))?;
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.set_len(bytes)?;
        Ok(Self {
            file,
            slots: u64::try_from(slots)
                .map_err(|_| std::io::Error::other("journal slot count does not fit u64"))?,
        })
    }

    pub(crate) fn write(&mut self, record: &Record) -> std::io::Result<()> {
        let payload = serde_json::to_vec(record).map_err(std::io::Error::other)?;
        if payload.len() > PAYLOAD_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "diagnostic record exceeds one journal slot",
            ));
        }

        let mut slot = [0_u8; SLOT_BYTES];
        slot[..8].copy_from_slice(MAGIC);
        slot[8..10].copy_from_slice(&SCHEMA.to_le_bytes());
        slot[10..12].copy_from_slice(
            &u16::try_from(payload.len())
                .map_err(|_| std::io::Error::other("journal payload length does not fit u16"))?
                .to_le_bytes(),
        );
        slot[12..20].copy_from_slice(&record.sequence.to_le_bytes());
        slot[20..52].copy_from_slice(blake3::hash(&payload).as_bytes());
        slot[HEADER_BYTES..HEADER_BYTES + payload.len()].copy_from_slice(&payload);

        let slot_index = if self.slots == 0 {
            record.sequence
        } else {
            record.sequence % self.slots
        };
        let offset = slot_index.saturating_mul(SLOT_BYTES as u64);
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(&slot)
    }

    pub(crate) fn sync(&mut self) -> std::io::Result<()> {
        self.file.sync_data()
    }
}

pub struct JournalCapture {
    pub records: Vec<Record>,
    pub bytes: u64,
    pub blake3: String,
    pub valid_slots: u64,
    pub omitted_slots: u64,
}

/// Bounded startup/recent capture with a digest of the complete original file.
/// Hashing and slot validation stream through one fixed buffer; cancellation
/// leaves the caller's original file untouched.
pub fn capture(
    path: &Path,
    head: usize,
    tail: usize,
    stopping: &AtomicBool,
) -> std::io::Result<JournalCapture> {
    if stopping.load(Ordering::Acquire) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "journal capture stopped",
        ));
    }
    let mut file = File::open(path)?;
    let bytes = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    let mut first = BTreeMap::new();
    let mut last = BTreeMap::new();
    let mut valid_slots = 0_u64;
    let mut slot = [0_u8; SLOT_BYTES];
    for _ in 0..bytes / SLOT_BYTES as u64 {
        if stopping.load(Ordering::Acquire) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "journal capture stopped",
            ));
        }
        file.read_exact(&mut slot)?;
        hasher.update(&slot);
        let Some(record) = decode_slot(&slot) else {
            continue;
        };
        valid_slots += 1;
        if head != 0 {
            first
                .entry(record.sequence)
                .or_insert_with(|| record.clone());
            if first.len() > head {
                first.pop_last();
            }
        }
        if tail != 0 {
            last.entry(record.sequence).or_insert(record);
            if last.len() > tail {
                last.pop_first();
            }
        }
    }
    if stopping.load(Ordering::Acquire) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "journal capture stopped",
        ));
    }
    let remainder = (bytes % SLOT_BYTES as u64) as usize;
    file.read_exact(&mut slot[..remainder])?;
    hasher.update(&slot[..remainder]);
    first.extend(last);
    let omitted_slots = valid_slots.saturating_sub(first.len() as u64);
    Ok(JournalCapture {
        records: first.into_values().collect(),
        bytes,
        blake3: hasher.finalize().to_hex().to_string(),
        valid_slots,
        omitted_slots,
    })
}

fn decode_slot(slot: &[u8; SLOT_BYTES]) -> Option<Record> {
    if &slot[..8] != MAGIC || u16::from_le_bytes([slot[8], slot[9]]) != SCHEMA {
        return None;
    }
    let payload_len = usize::from(u16::from_le_bytes([slot[10], slot[11]]));
    if payload_len > PAYLOAD_BYTES {
        return None;
    }
    let payload = &slot[HEADER_BYTES..HEADER_BYTES + payload_len];
    if blake3::hash(payload).as_bytes() != &slot[20..52] {
        return None;
    }
    let record: Record = serde_json::from_slice(payload).ok()?;
    (record.sequence == u64::from_le_bytes(slot[12..20].try_into().ok()?)).then_some(record)
}

pub fn read(path: &Path, limit: usize) -> std::io::Result<Vec<Record>> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let slots = file_len / SLOT_BYTES as u64;
    if limit == 0 {
        return Ok(Vec::new());
    }
    // Retain only the requested newest records while scanning; sorting every
    // record and truncating afterwards consumed the whole append-only journal.
    let mut records = BTreeMap::new();
    let mut slot = [0_u8; SLOT_BYTES];

    for index in 0..slots {
        file.seek(SeekFrom::Start(index.saturating_mul(SLOT_BYTES as u64)))?;
        if file.read_exact(&mut slot).is_err() {
            continue;
        }
        let Some(record) = decode_slot(&slot) else {
            continue;
        };
        records.entry(record.sequence).or_insert(record);
        if records.len() > limit {
            records.pop_first();
        }
    }

    Ok(records.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_capture_preserves_original_digest_and_startup_recent_windows() {
        let directory = std::env::temp_dir().join(format!("dfr-capture-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("events.dfr");
        let mut journal = Journal::create(&path, 0).unwrap();
        for sequence in 1..=32 {
            journal
                .write(&Record {
                    sequence,
                    action: format!("event-{sequence}"),
                    ..Default::default()
                })
                .unwrap();
        }
        journal
            .file
            .seek(SeekFrom::Start(16 * SLOT_BYTES as u64 + 20))
            .unwrap();
        journal.file.write_all(&[0; 32]).unwrap();
        journal.file.seek(SeekFrom::End(0)).unwrap();
        journal.file.write_all(b"partial trailing slot").unwrap();
        journal.sync().unwrap();
        let original = std::fs::read(&path).unwrap();
        let captured = capture(&path, 2, 3, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            captured
                .records
                .iter()
                .map(|r| r.sequence)
                .collect::<Vec<_>>(),
            [1, 2, 30, 31, 32]
        );
        assert_eq!(captured.valid_slots, 31);
        assert_eq!(captured.omitted_slots, 26);
        assert_eq!(captured.bytes, original.len() as u64);
        assert_eq!(
            captured.blake3,
            blake3::hash(&original).to_hex().to_string()
        );
        assert_eq!(
            capture(&path, 2, 3, &AtomicBool::new(true))
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                journal.file.metadata().unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn bounded_reads_keep_newest_valid_records_in_append_and_ring_journals() {
        for budget in [0, SLOT_BYTES * 8] {
            let directory =
                std::env::temp_dir().join(format!("dfr-bounded-{}-{budget}", std::process::id()));
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join("events.dfr");
            let mut journal = Journal::create(&path, budget).unwrap();
            for sequence in 1..=32 {
                journal
                    .write(&Record {
                        sequence,
                        action: format!("event-{sequence}"),
                        ..Default::default()
                    })
                    .unwrap();
            }
            journal.sync().unwrap();
            let newest = read(&path, 3).unwrap();
            assert_eq!(
                newest.iter().map(|r| r.sequence).collect::<Vec<_>>(),
                [30, 31, 32]
            );
            assert_eq!(newest[2].action, "event-32");
            assert!(read(&path, 0).unwrap().is_empty());
            // Corrupt the newest slot's checksum. The bounded view must step
            // back to valid records rather than count that slot as evidence.
            let index = if budget == 0 { 32 } else { 32 % 8 };
            journal
                .file
                .seek(SeekFrom::Start(index * SLOT_BYTES as u64 + 20))
                .unwrap();
            journal.file.write_all(&[0_u8; 32]).unwrap();
            journal.sync().unwrap();
            assert_eq!(
                read(&path, 3)
                    .unwrap()
                    .iter()
                    .map(|r| r.sequence)
                    .collect::<Vec<_>>(),
                [29, 30, 31]
            );
            drop(journal);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }
}
