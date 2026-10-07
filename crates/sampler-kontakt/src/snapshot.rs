//! Kontakt snapshots (`.nksn`): the saved state of an existing instrument.
//! The script persistent values are applied; the compact per-group state
//! and bus effect state are read but not applied yet.
use crate::{Kontakt, LoadError};
use ni_file::kontakt::objects::{Snapshot, snapshot_metadata_names};
use std::path::Path;

const SNAPSHOT: u16 = 0x4f;
const METADATA: u16 = 0x51;

/// What a snapshot file holds that this crate applies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotState {
    /// The instrument the snapshot was saved from (its metadata name).
    pub instrument: String,
    /// Saved persistent entries (`"<name> <value>"`) of the five script slots.
    pub persistent: Vec<Vec<String>>,
    /// The snapshot also holds native group and effect state, not applied.
    pub native_state: bool,
}

pub fn read_snapshot(path: &Path) -> Result<SnapshotState, LoadError> {
    let chunks = crate::read_chunks(path).map_err(|e| e.at(crate::Stage::Container))?;
    let decode = |what, error| LoadError::decode(path, what, error).at(crate::Stage::Parse);
    let snapshot = Snapshot::try_from(chunks.find_first(SNAPSHOT).ok_or_else(|| LoadError::Invalid {
        path: path.into(),
        reason: "not a snapshot".into(),
    })?)
    .map_err(|e| decode("snapshot", e))?;
    let instrument = match chunks.find_first(METADATA) {
        Some(chunk) => snapshot_metadata_names(chunk).map_err(|e| decode("snapshot metadata", e))?.0,
        None => String::new(),
    };
    Ok(SnapshotState { instrument, persistent: snapshot.persistent, native_state: snapshot.groups.is_some() })
}

/// Replace the instrument's saved script values with the snapshot's, slot by slot.
pub fn apply_snapshot(kontakt: &mut Kontakt, snapshot: &SnapshotState) {
    for behavior in &mut kontakt.instrument.behaviors {
        let Some(entries) = behavior.slot.and_then(|s| snapshot.persistent.get(usize::from(s))) else { continue };
        for (name, value) in crate::library::saved(entries) {
            match behavior.state.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = value,
                None => behavior.state.push((name, value)),
            }
        }
    }
}
