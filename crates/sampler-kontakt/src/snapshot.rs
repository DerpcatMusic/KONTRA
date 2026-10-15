//! Kontakt snapshots (`.nksn`): the saved state of an existing instrument.
//! Applied: script persistent values, instrument racks and buses, and each
//! group's level, pan, tune, flags, insert rack and modulation arrays.
use crate::{Kontakt, LoadError};
use anyhow::{Context, ensure};
use ni_file::kontakt::objects::{FNTableImpl, FileNameListPreK51, Snapshot, snapshot_metadata_names};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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
    /// Compact record version (1..=4).
    pub version: u16,
    /// The saved `BParSrcMode` record plus any retained bytes after it
    /// (Kontakt8131 0x140cff070 reads it into the group source state).
    pub source: Vec<u8>,
    /// Compact public bytes 14..24: release-trigger flags, release counter,
    /// MIDI channel and voice-group state. Native applies them at group
    /// +0x671..0x674, +0x678, +0xd8 and +0xba0 (0x140cff070); their laws are
    /// unproved, so they are retained and reported, never applied.
    pub compact_state: [u8; 10],
    /// Trailing selection byte and the v4 words (group +0x120/+0x124, selection
    /// via 0x1409a8650/0x1409a8c60); ownership unproved, retained and reported.
    pub selection: u8,
    pub v4_words: Vec<u8>,
}

/// One insert slot: chunk id and bytes.
pub type RackSlot = (u16, Vec<u8>);

/// Replace the two modulation arrays and the source-mode record (Kontakt8131
/// 0x140cff070), retaining every other child of the original group.
pub(crate) fn overlay_group(
    group: &ni_file::kontakt::objects::Group,
    state: &GroupState,
) -> Result<ni_file::kontakt::objects::Group, ni_file::Error> {
    let mut overlaid = overlay_modulation(group, &state.modulation);
    let record = ni_file::kontakt::objects::BParSrcMode::read(std::io::Cursor::new(&state.source))?;
    overlaid.set_source_record(&state.source[..usize::from(record.bytes)])?;
    Ok(overlaid)
}

fn overlay_modulation(
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
    /// Snapshot-owned other-file indices (including convolution responses).
    /// An absent/empty table never borrows the base instrument's indices.
    pub other_files: HashMap<u32, String>,
    /// Per group, in group order; empty for script-only snapshots.
    pub groups: Vec<GroupState>,
}

impl SnapshotState {
    /// Decode saved rows without initializing scripts or translating DSP.
    /// Physical slots and duplicate names remain intact for whole-batch
    /// validation by the existing setup's schema.
    pub fn saved_values(&self) -> Result<Vec<Vec<(String, sampler_ir::Saved)>>, crate::Error> {
        self.persistent.iter().map(|rows| crate::library::saved(rows)).collect()
    }
}

pub fn read_snapshot(path: &Path) -> Result<SnapshotState, LoadError> {
    let chunks = crate::read_chunks(path).map_err(|e| e.at(crate::Stage::Container))?;
    decode_snapshot(path, &chunks)
}

/// Validate identity and decode saved state from the same immutable chunk read.
/// A path replacement cannot mix one file's binding with another file's rows.
pub fn read_snapshot_for_base(path: &Path, base: &Path) -> Result<SnapshotState, LoadError> {
    let chunks = crate::read_chunks(path).map_err(|e| e.at(crate::Stage::Container))?;
    validate_snapshot_chunks(&chunks, base).map_err(|error| LoadError::Invalid {
        path: path.into(), reason: error.to_string(),
    })?;
    decode_snapshot(path, &chunks)
}

fn decode_snapshot(path: &Path, chunks: &ni_file::kontakt::KontaktChunks) -> Result<SnapshotState, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error).at(crate::Stage::Parse);
    let other_files = other_files(chunks).map_err(|e| decode("snapshot other-files table", e))?;
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
                version: g.version,
                source: g.source_data.clone(),
                compact_state: g.public_data[14..24].try_into().expect("ten bytes"),
                selection: g.trailing_flag,
                v4_words: g.trailing_data.clone(),
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
        other_files,
        groups,
    })
}

fn other_files(chunks: &ni_file::kontakt::KontaktChunks) -> Result<HashMap<u32, String>, ni_file::Error> {
    if let Some(chunk) = chunks.find_first(0x4b) {
        return FNTableImpl::try_from(chunk).map(|table| table.other_filetable);
    }
    if let Some(chunk) = chunks.find_first(0x3d) {
        return FileNameListPreK51::try_from(chunk).map(|table| table.other_filetable);
    }
    Ok(HashMap::new())
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
    fn snapshot_other_files_preserve_modern_legacy_absent_and_empty_namespaces() {
        use ni_file::kontakt::{Chunk, KontaktChunks};
        fn filename(bytes: &mut Vec<u8>, name: &str) {
            bytes.extend(1i32.to_le_bytes());
            bytes.push(4);
            let units: Vec<_> = name.encode_utf16().collect();
            bytes.extend((units.len() as u32).to_le_bytes());
            for unit in units { bytes.extend(unit.to_le_bytes()); }
        }
        fn table(id: u16, names: &[&str]) -> Chunk {
            let mut data = Vec::new();
            if id == 0x4b { data.extend(2u16.to_le_bytes()); }
            data.extend(0u32.to_le_bytes()); // special
            data.extend(0u32.to_le_bytes()); // samples; no dates/offsets
            data.extend((names.len() as u32).to_le_bytes());
            for name in names { filename(&mut data, name); }
            Chunk { id, data }
        }
        assert!(other_files(&KontaktChunks(vec![])).unwrap().is_empty());
        for id in [0x4b, 0x3d] {
            assert!(other_files(&KontaktChunks(vec![table(id, &[])])).unwrap().is_empty());
            let files = other_files(&KontaktChunks(vec![table(id, &["own.wav", "second.wav"])])).unwrap();
            assert_eq!(files, [(0,"own.wav".into()),(1,"second.wav".into())].into());
        }
        // A corrupt modern authority cannot silently inherit a valid legacy map.
        assert!(other_files(&KontaktChunks(vec![Chunk { id: 0x4b, data: vec![0xff] }, table(0x3d, &["foreign.wav"])])).is_err());
    }

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

    #[test]
    fn snapshot_source_mode_record_replaces_the_base_source() {
        use ni_file::kontakt::{StructuredObject, objects::Group};
        let record = |mode: u8| {
            let mut r = vec![0, 0, 1, mode, 0, 0, 0];
            r.extend([0; 40]);
            let n = ni_file::kontakt::objects::BParSrcMode::read(std::io::Cursor::new(&r)).unwrap();
            r.truncate(usize::from(n.bytes));
            r
        };
        let mut private = Vec::new();
        for _ in 0..136 {
            private.extend(8u32.to_le_bytes());
            private.extend([0; 8]);
        }
        private.extend([0; 24]);
        private.extend([0, 0x13, 0]);
        private.extend(8u32.to_le_bytes());
        private.extend([0; 8]);
        private.push(0);
        private.extend(record(0));
        private.extend([0xde, 0xad]);
        let base = Group(StructuredObject {
            version: 1,
            private_data: private,
            public_data: Vec::new(),
            children: Vec::new(),
        });
        let state = GroupState {
            volume: 1.0,
            pan: 0.0,
            octaves: 0.0,
            key_tracking: true,
            reverse: false,
            fx: (0x13, Vec::new()),
            modulation: Vec::new(),
            version: 3,
            source: record(5),
            compact_state: [0; 10],
            selection: 0,
            v4_words: Vec::new(),
        };
        let overlaid = overlay_group(&base, &state).unwrap();
        assert_eq!(overlaid.source_identity().unwrap().mode, 5);
        assert!(overlaid.0.private_data.ends_with(&[0xde, 0xad]));
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

/// Resolve one snapshot against its embedded identity and bounded local library tree.
/// Ambiguous bases require explicit selection; directory symlinks are never followed.
pub fn resolve_snapshot_base(snapshot: &Path) -> anyhow::Result<Option<PathBuf>> {
    let identity = snapshot_identity(snapshot)?;
    let mut wanted = vec![identity];
    // Snapshots/<instrument>/<category>/...: category names are not base identities.
    if let Some(folder) = snapshot
        .ancestors()
        .find(|p| {
            p.parent()
                .and_then(Path::file_name)
                .is_some_and(|s| s.eq_ignore_ascii_case("Snapshots"))
        })
        .and_then(Path::file_name)
    {
        wanted.push(folder.to_string_lossy().into_owned());
    }
    find_snapshot_base(snapshot, &wanted, |base| {
        validate_snapshot_base(snapshot, base)
    })
}

fn find_snapshot_base(
    snapshot: &Path,
    wanted: &[String],
    validate: impl Fn(&Path) -> anyhow::Result<()>,
) -> anyhow::Result<Option<PathBuf>> {
    fn find(
        dir: &Path,
        wanted: &[String],
        depth: usize,
        left: &mut usize,
        found: &mut Vec<PathBuf>,
    ) -> anyhow::Result<()> {
        let mut folders = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            ensure!(
                *left > 0,
                "Snapshot base search exceeded its directory budget"
            );
            *left -= 1;
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            // Do not follow directory symlinks out of this library or around cycles.
            if kind.is_dir() {
                folders.push(path);
            } else if kind.is_file()
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
                && path
                    .file_stem()
                    .is_some_and(|s| wanted.iter().any(|w| s.eq_ignore_ascii_case(w.as_str())))
            {
                found.push(path);
            }
        }
        if depth == 0 {
            return Ok(());
        }
        folders.sort();
        for dir in folders {
            find(&dir, wanted, depth - 1, left, found)?;
        }
        Ok(())
    }
    let library = snapshot
        .ancestors()
        .find(|p| {
            p.file_name()
                .is_some_and(|s| s.eq_ignore_ascii_case("Snapshots"))
        })
        .and_then(Path::parent);
    let mut left = 16_384;
    let directories: Vec<_> = if let Some(library) = library {
        snapshot
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(library))
            .collect()
    } else {
        snapshot.ancestors().skip(1).take(4).collect()
    };
    for dir in directories {
        let mut found = Vec::new();
        find(dir, wanted, 3, &mut left, &mut found)?;
        found.sort();
        found.dedup();
        ensure!(
            found.len() <= 1,
            "Snapshot base is ambiguous; select its NKI explicitly"
        );
        if let Some(base) = found.pop() {
            validate(&base)?;
            return Ok(Some(base));
        }
    }
    Ok(None)
}

// Port from v1 0cb7a8a0:src/import.rs; v2 containers retain native metadata.
/// Container identity only, for the off-thread snapshot catalog. Snapshot
/// metadata names its base instrument, not the snapshot itself; the latter's
/// authored name is its file stem (as in `read_snapshot`).
pub fn snapshot_identity(path: &Path) -> anyhow::Result<String> {
    std::panic::catch_unwind(|| {
        let c = crate::read_chunks(path)?;
        c.find_first(0x4f).context("Snapshot state missing")?;
        let names = ni_file::kontakt::objects::snapshot_metadata_names(
            c.find_first(0x51).context("Snapshot metadata missing")?,
        )?;
        Ok(snapshot_binding_name(&names, None)?.to_owned())
    })
    .map_err(|_| anyhow::anyhow!("Malformed snapshot metadata"))?
}

/// The same exact base identity used by snapshot validation, without loading
/// zones, samples, scripts or artwork.
pub fn snapshot_base_name(path: &Path) -> anyhow::Result<String> {
    std::panic::catch_unwind(|| {
        let c = crate::read_chunks(path)?;
        let program = ni_file::kontakt::objects::Program::try_from(
            c.find_first(0x28).context("Base program missing")?,
        )?;
        Ok(program.params()?.name)
    })
    .map_err(|_| anyhow::anyhow!("Malformed base instrument metadata"))?
}

/// Both direct snapshot loads and explicit base selection use the same metadata
/// contract. Generic template names require the second name to identify the file,
/// and the native program must still match either of the embedded identities.
pub fn validate_snapshot_base(snapshot: &Path, base: &Path) -> anyhow::Result<()> {
    let chunks = crate::read_chunks(snapshot)?;
    validate_snapshot_chunks(&chunks, base)
}

fn validate_snapshot_chunks(chunks: &ni_file::kontakt::KontaktChunks, base: &Path) -> anyhow::Result<()> {
    let names = ni_file::kontakt::objects::snapshot_metadata_names(
        chunks
            .find_first(0x51)
            .context("Snapshot metadata missing")?,
    )?;
    let wanted = snapshot_binding_name(&names, Some(base))?;
    let actual = snapshot_base_name(base)?;
    ensure!(
        actual == wanted || (names.0 == "Kontakt" && actual == names.0),
        "Snapshot requires base instrument {wanted:?}, got {actual:?}"
    );
    Ok(())
}

// A factory snapshot may retain Kontakt's generic template name. Only use
// its second embedded name when it also exactly names the supplied NKI file;
// the program name, group/source/slot identities are still validated below.
fn snapshot_binding_name<'a>(
    names: &'a (String, String),
    base: Option<&Path>,
) -> anyhow::Result<&'a str> {
    if names.0 != "Kontakt" || names.1.is_empty() {
        return Ok(&names.0);
    }
    if let Some(base) = base {
        ensure!(
            base.file_stem().and_then(|s| s.to_str()) == Some(names.1.as_str()),
            "Generic snapshot requires base filename {:?}",
            names.1
        );
    }
    Ok(&names.1)
}

#[cfg(test)]
mod snapshot_resolution_tests {
    use super::*;

    #[test]
    fn generic_template_uses_embedded_content_identity_and_checks_selected_file() {
        let names = ("Kontakt".into(), "Example Orchestra".into());
        assert_eq!(
            snapshot_binding_name(&names, None).unwrap(),
            "Example Orchestra"
        );
        assert!(snapshot_binding_name(&names, Some(Path::new("Example Orchestra.nki"))).is_ok());
        assert!(snapshot_binding_name(&names, Some(Path::new("Kontakt.nki"))).is_err());
        let ordinary = ("Named Program".into(), "Other Content".into());
        assert_eq!(
            snapshot_binding_name(&ordinary, None).unwrap(),
            "Named Program"
        );
    }

    #[test]
    fn nested_categories_find_base_and_ambiguity_is_never_guessed() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let snapshots = root.join("Snapshots/Example Orchestra/Category/More/Deep");
        std::fs::create_dir_all(&snapshots).unwrap();
        let snapshot = snapshots.join("Preset.nksn");
        std::fs::write(&snapshot, []).unwrap();
        let instruments = root.join("Instruments");
        std::fs::create_dir(&instruments).unwrap();
        let base = instruments.join("Example Orchestra.NKI");
        std::fs::write(&base, []).unwrap();
        let wanted = ["Example Orchestra".into()];
        assert_eq!(
            find_snapshot_base(&snapshot, &wanted, |_| Ok(())).unwrap(),
            Some(base)
        );
        std::fs::create_dir(root.join("Other")).unwrap();
        std::fs::write(root.join("Other/Example Orchestra.nki"), []).unwrap();
        assert!(
            find_snapshot_base(&snapshot, &wanted, |_| Ok(()))
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }

    #[test]
    fn base_identity_validation_errors_are_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = directory.path().join("Preset.nksn");
        std::fs::write(&snapshot, []).unwrap();
        std::fs::write(directory.path().join("Named.nki"), []).unwrap();
        assert!(
            find_snapshot_base(&snapshot, &["Named".into()], |_| anyhow::bail!(
                "wrong identity"
            ))
            .unwrap_err()
            .to_string()
            .contains("wrong identity")
        );
    }

    #[test]
    #[ignore = "installed library: set KONTRA_SNAPSHOT_FIXTURE to a native snapshot"]
    fn installed_snapshot_resolves_and_applies_state() {
        let snapshot =
            PathBuf::from(std::env::var_os("KONTRA_SNAPSHOT_FIXTURE").expect("snapshot fixture"));
        let state = crate::read_snapshot(&snapshot).unwrap();
        let base = resolve_snapshot_base(&snapshot)
            .unwrap()
            .expect("installed base");
        crate::read_with_snapshot(&base, &state).unwrap();
    }

    #[test]
    #[ignore = "requires KONTRA_PROFILE_SNAPSHOT_IR and expected impulse count; no sample playback"]
    fn owned_snapshot_impulses_resolve_their_own_file_indices() {
        let path = PathBuf::from(std::env::var_os("KONTRA_PROFILE_SNAPSHOT_IR").expect("snapshot"));
        let expected: usize = std::env::var("KONTRA_PROFILE_SNAPSHOT_IR_COUNT").expect("expected count").parse().unwrap();
        let state = crate::read_snapshot(&path).unwrap();
        let base = resolve_snapshot_base(&path).unwrap().expect("unambiguous owned base");
        let mut translated = crate::read_with_snapshot(&base, &state).unwrap();
        // Independently resolve/decode the exact snapshot-owned indices and
        // compare shaped PCM in memory. Keep base NKR authority for resource
        // members; never search the base other-files namespace for an index.
        let base_chunks = crate::read_chunks(&base).unwrap();
        let containers: Vec<_> = base_chunks.filename_tables().unwrap().unwrap().special_filetable
            .into_values().filter(|name| name.to_ascii_lowercase().ends_with(".nkr")).collect();
        let effects = ni_file::kontakt::objects::Program(ni_file::kontakt::StructuredObject {
            version: 0xaf, public_data: vec![], private_data: vec![],
            children: state.effects.iter().map(|(id, data)| ni_file::kontakt::Chunk { id: *id, data: data.clone() }).collect(),
        });
        let racks = crate::effects::program_racks(&effects, &[], |_, error| panic!("snapshot rack: {error}"));
        let mut verified = 0usize;
        let mut missing = 0usize;
        for (_, slots) in racks {
            for slot in slots {
                let Some(crate::effects::Params::Convolution(params)) = slot.params() else { continue; };
                let name = state.other_files.get(&u32::try_from(params.ir_index).unwrap()).expect("snapshot-owned index");
                match translated.samples.resolve_impulse(base.parent().unwrap(), name, &containers).unwrap() {
                    Some(source) => {
                        let pcm = translated.samples.decode(&source).unwrap();
                        let expected = crate::effects::restore_impulse(&params, (pcm.rate, pcm.frames)).unwrap();
                        assert!(translated.instrument.impulses.contains(&expected), "snapshot response must equal the exact independently decoded source");
                        verified += 1;
                    }
                    None => missing += 1,
                }
            }
        }
        let failures: Vec<_> = translated.instrument.unsupported.iter().filter(|u| u.feature == "Convolution: impulse response").collect();
        assert!(!failures.iter().any(|u| u.value.contains("not in the") || u.value.contains("unsupported audio") || u.value.contains("file table")), "file index must resolve only in the snapshot authority");
        assert_eq!(translated.instrument.impulses.len(), expected);
        if expected == 0 { assert!(missing > 0 && failures.iter().any(|u| u.value.contains("was not found")), "missing external dependency remains explicit"); }
        else { assert!(verified > 0); }
        let mut hash = 0xcbf29ce484222325u64;
        let mut frames = 0usize;
        for impulse in &translated.instrument.impulses {
            frames += impulse.left.len();
            for sample in impulse.left.iter().chain(&impulse.right) {
                for byte in sample.to_bits().to_le_bytes() { hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3); }
            }
        }
        println!("SNAPSHOT_IR_AUDIT {}", serde_json::json!({
            "snapshot_other_files": state.other_files.len(), "decoded_impulses": expected,
            "impulse_frames": frames, "impulse_pcm_fnv64": format!("{hash:016x}"),
            "explicit_missing_dependencies": failures.iter().filter(|u|u.value.contains("was not found")).count(),
            "independently_verified_snapshot_responses": verified,
            "cross_namespace_fallback": false, "payload_written": false,
        }));
    }
}
