use crate::Record;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

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
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(path)?;
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
        if file.read_exact(&mut slot).is_err() || &slot[..8] != MAGIC {
            continue;
        }
        if u16::from_le_bytes([slot[8], slot[9]]) != SCHEMA {
            continue;
        }
        let payload_len = usize::from(u16::from_le_bytes([slot[10], slot[11]]));
        if payload_len > PAYLOAD_BYTES {
            continue;
        }
        let payload = &slot[HEADER_BYTES..HEADER_BYTES + payload_len];
        if blake3::hash(payload).as_bytes() != &slot[20..52] {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<Record>(payload) else {
            continue;
        };
        if record.sequence
            != u64::from_le_bytes(
                slot[12..20]
                    .try_into()
                    .map_err(|_| std::io::Error::other("invalid journal sequence"))?,
            )
        {
            continue;
        }
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
