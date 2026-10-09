//! Kontakt snapshots (`.nksn`): the saved state of an existing instrument.
//! Applied: script persistent values, instrument racks and buses, and each
//! group's level, pan, tune, flags, insert rack and modulation arrays.
use crate::{Kontakt, LoadError};
use ni_file::kontakt::objects::{Snapshot, snapshot_metadata_names};
use std::path::Path;

const SNAPSHOT: u16 = 0x4f;
const METADATA: u16 = 0x51;

/// A group's saved state: the compact record's level, pan and tune (octaves,
/// 2^x as a ratio), key tracking and reverse flags, and its insert rack.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupState {
    pub volume: f32,
    pub pan: f32,
    /// Tune in octaves; 0 leaves the pitch alone.
    pub octaves: f32,
    pub key_tracking: bool,
    pub reverse: bool,
    /// The group insert rack's array version and its slots as (chunk id, bytes).
    pub fx: (u16, Vec<Option<RackSlot>>),
    /// Internal and external modulation array chunks, preserving versions and slots.
    pub modulation: Vec<RackSlot>,
}

/// One insert slot: chunk id and bytes.
pub type RackSlot = (u16, Vec<u8>);

/// Replace only the two modulation arrays, retaining the original group topology.
pub(crate) fn overlay_modulation(
    group: &ni_file::kontakt::objects::Group,
    saved: &[RackSlot],
) -> ni_file::kontakt::objects::Group {
    use ni_file::kontakt::{Chunk, StructuredObject, objects::Group};
    let mut children: Vec<_> = group
        .0
        .children
        .iter()
        .map(|c| Chunk {
            id: c.id,
            data: c.data.clone(),
        })
        .collect();
    for &(id, ref data) in saved.iter().filter(|(id, _)| matches!(id, 0x3b | 0x3c)) {
        let chunk = Chunk {
            id,
            data: data.clone(),
        };
        match children.iter_mut().find(|c| c.id == id) {
            Some(original) => *original = chunk,
            None => children.push(chunk),
        }
    }
    Group(StructuredObject {
        version: group.0.version,
        public_data: group.0.public_data.clone(),
        private_data: group.0.private_data.clone(),
        children,
    })
}

/// What a snapshot file holds that this crate applies.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotState {
    /// The instrument the snapshot was saved from (its metadata name).
    pub instrument: String,
    /// Saved persistent entries (`"<name> <value>"`) of the five script slots.
    pub persistent: Vec<Vec<String>>,
    /// The instrument's insert/send/(main) racks (chunk 0x3a) and 16 buses
    /// (0x45), in file order, as (chunk id, bytes).
    pub effects: Vec<(u16, Vec<u8>)>,
    /// Per group, in group order; empty for script-only snapshots.
    pub groups: Vec<GroupState>,
}

pub fn read_snapshot(path: &Path) -> Result<SnapshotState, LoadError> {
    let chunks = crate::read_chunks(path).map_err(|e| e.at(crate::Stage::Container))?;
    let decode = |what, error| LoadError::decode(path, what, error).at(crate::Stage::Parse);
    let snapshot =
        Snapshot::try_from(
            chunks
                .find_first(SNAPSHOT)
                .ok_or_else(|| LoadError::Invalid {
                    path: path.into(),
                    reason: "not a snapshot".into(),
                })?,
        )
        .map_err(|e| decode("snapshot", e))?;
    let instrument = match chunks.find_first(METADATA) {
        Some(chunk) => {
            snapshot_metadata_names(chunk)
                .map_err(|e| decode("snapshot metadata", e))?
                .0
        }
        None => String::new(),
    };
    let float = |bytes: &[u8]| f32::from_le_bytes(bytes.try_into().expect("four bytes"));
    let groups = snapshot
        .group_snapshots()
        .map_err(|e| decode("snapshot groups", e))?
        .into_iter()
        .map(|(_, g)| {
            let modulation = g
                .modulation_chunks()?
                .into_iter()
                .map(|c| (c.id, c.data))
                .collect();
            Ok(GroupState {
                volume: float(&g.public_data[0..4]),
                pan: float(&g.public_data[4..8]),
                octaves: float(&g.public_data[8..12]),
                key_tracking: g.public_data[12] != 0,
                reverse: g.public_data[13] != 0,
                fx: (
                    g.fx.version,
                    g.fx.items
                        .into_iter()
                        .map(|slot| slot.map(|c| (c.id, c.data)))
                        .collect(),
                ),
                modulation,
            })
        })
        .collect::<Result<Vec<_>, ni_file::Error>>()
        .map_err(|e| decode("snapshot modulation", e))?;
    let effects = snapshot
        .effect_children
        .into_iter()
        .map(|c| (c.id, c.data))
        .collect();
    Ok(SnapshotState {
        instrument,
        persistent: snapshot.persistent,
        effects,
        groups,
    })
}

/// Replace the instrument's saved script values with the snapshot's, slot by slot.
pub fn apply_snapshot(kontakt: &mut Kontakt, snapshot: &SnapshotState) -> Result<(), crate::Error> {
    // Validate all slots before changing any authored state.
    let decoded: Vec<_> = snapshot
        .persistent
        .iter()
        .map(|entries| crate::library::saved(entries))
        .collect::<Result<_, _>>()?;
    for behavior in &mut kontakt.instrument.behaviors {
        let Some(entries) = behavior.slot.and_then(|s| decoded.get(usize::from(s))) else {
            continue;
        };
        for (name, value) in entries.iter().cloned() {
            match behavior.state.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = value,
                None => behavior.state.push((name, value)),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod probe {
    use super::*;

    #[test]
    fn snapshot_modulation_replaces_both_arrays_and_preserves_other_children() {
        use ni_file::kontakt::{
            Chunk, StructuredObject,
            objects::{ExternalModArray32, Group, InternalModArray16},
        };
        let base = Group(StructuredObject {
            version: 0x95,
            private_data: vec![8],
            public_data: vec![9],
            children: vec![
                Chunk {
                    id: 0x3b,
                    data: vec![0, 0x12, 0, 1],
                },
                Chunk {
                    id: 0x4a,
                    data: vec![7],
                },
                Chunk {
                    id: 0x3c,
                    data: vec![0, 0x12, 0, 1],
                },
            ],
        });
        let mut internal = vec![0, 0x12, 0];
        internal.extend([0; 16]);
        let mut external = vec![0, 0x13, 0];
        external.extend(64u32.to_le_bytes());
        external.extend([0; 64]);
        let overlay = overlay_modulation(&base, &[(0x3b, internal), (0x3c, external)]);
        assert!(
            InternalModArray16::try_from(&overlay.0.children[0])
                .unwrap()
                .slots()
                .unwrap()
                .is_empty()
        );
        let external = ExternalModArray32::try_from(&overlay.0.children[2]).unwrap();
        assert_eq!(external.slot_count().unwrap(), 64);
        assert!(external.slots().unwrap().is_empty());
        assert_eq!(overlay.0.children[1].data, [7]);
        assert_eq!(overlay.0.private_data, [8]);
        assert_eq!(overlay.0.public_data, [9]);
        assert_eq!(
            base.0.children[0].data,
            [0, 0x12, 0, 1],
            "base remains unchanged"
        );
    }

    /// What differs between snapshots and their instrument: run with --ignored.
    #[test]
    #[ignore = "census"]
    fn native_state_probe() {
        let root = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_default();
        let Some(lib) = std::env::split_paths(&root)
            .map(|r| r.join("Una Corda Library"))
            .find(|p| p.is_dir())
        else {
            return;
        };
        let snaps = lib.join("Snapshots/Una Corda Cotton");
        let nki = lib.join("Instruments/Una Corda Cotton.nki");
        let base = crate::read_chunks(&nki).unwrap();
        let program =
            ni_file::kontakt::objects::Program::try_from(base.find_first(0x28).unwrap()).unwrap();
        let groups =
            ni_file::kontakt::objects::GroupList::try_from(program.0.find_first(0x33).unwrap())
                .unwrap();
        for (i, g) in groups.groups.iter().enumerate().take(3) {
            let p = g.params().unwrap();
            println!(
                "BASE group {i}: vol {} pan {} tune {} kt {} rev {} rt {} ch {}",
                p.volume,
                p.pan,
                p.tune,
                p.key_tracking,
                p.reverse,
                p.release_trigger,
                p.midi_channel
            );
        }
        let mut files: Vec<_> = std::fs::read_dir(&snaps)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        let base_effects: Vec<(u16, &Vec<u8>)> = program
            .0
            .children
            .iter()
            .filter(|c| c.id == 0x3a || c.id == 0x45)
            .map(|c| (c.id, &c.data))
            .collect();
        println!(
            "BASE effect chunks {:?}",
            base_effects
                .iter()
                .map(|(i, d)| (*i, d.len()))
                .collect::<Vec<_>>()
        );
        let mut distinct =
            std::collections::BTreeMap::<usize, std::collections::BTreeSet<Vec<u8>>>::new();
        let mut same_as_base = 0;
        let mut group_diff = std::collections::BTreeSet::new();
        for f in &files {
            let chunks = crate::read_chunks(f).unwrap();
            let s = Snapshot::try_from(chunks.find_first(SNAPSHOT).unwrap()).unwrap();
            for (i, c) in s.effect_children.iter().enumerate() {
                distinct.entry(i).or_default().insert(c.data.clone());
                if base_effects
                    .get(i)
                    .is_some_and(|(id, d)| *id == c.id && **d == c.data)
                {
                    same_as_base += 1;
                }
            }
            for (id, g) in s.group_snapshots().unwrap() {
                group_diff.insert((
                    g.public_data.to_vec(),
                    g.fx.items.iter().flatten().count(),
                    g.internal.items.iter().flatten().count(),
                ));
                let _ = id;
            }
        }
        let mut vals = std::collections::BTreeMap::<usize, std::collections::BTreeSet<u32>>::new();
        for f in &files {
            let chunks = crate::read_chunks(f).unwrap();
            let s = Snapshot::try_from(chunks.find_first(SNAPSHOT).unwrap()).unwrap();
            for (_, g) in s.group_snapshots().unwrap() {
                for at in [0usize, 4, 8] {
                    vals.entry(at).or_default().insert(u32::from_le_bytes(
                        g.public_data[at..at + 4].try_into().unwrap(),
                    ));
                }
                for at in 12..24 {
                    vals.entry(at)
                        .or_default()
                        .insert(u32::from(g.public_data[at]));
                }
            }
        }
        for (at, v) in &vals {
            println!(
                "PUBLIC byte {at}: {} distinct, e.g. {:?}",
                v.len(),
                v.iter()
                    .take(6)
                    .map(|x| if *at < 12 {
                        format!("{}", f32::from_bits(*x))
                    } else {
                        format!("{x}")
                    })
                    .collect::<Vec<_>>()
            );
        }
        println!(
            "{} files; distinct per effect child {:?}; equal-to-base {same_as_base}; distinct group (public,fx,internal) {}",
            files.len(),
            distinct
                .iter()
                .map(|(i, d)| (*i, d.len()))
                .collect::<Vec<_>>(),
            group_diff.len()
        );
        for f in files.iter().take(0) {
            let chunks = crate::read_chunks(f).unwrap();
            let s = Snapshot::try_from(chunks.find_first(SNAPSHOT).unwrap()).unwrap();
            println!(
                "{} v{} groups {} effects {}",
                f.file_name().unwrap().to_string_lossy(),
                s.version,
                s.group_count,
                s.effect_children.len()
            );
            for (id, g) in s.group_snapshots().unwrap().into_iter().take(3) {
                println!(
                    "  g{id} public {:02x?} fx used {} internal used {} external used {} flag {}",
                    g.public_data,
                    g.fx.items.iter().flatten().count(),
                    g.internal.items.iter().flatten().count(),
                    g.external.items.iter().flatten().count(),
                    g.trailing_flag
                );
            }
        }
    }
}
