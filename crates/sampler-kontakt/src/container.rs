//! Container access shared by translation and the installed-library census.
//! Presets and decoded data remain in memory; local access values are never exposed.
use crate::LoadError;
use ni_file::{
    NIFile,
    kontakt::KontaktChunks,
    nis::schema::{NISObject, PresetChunkItem, Repository},
};
use std::{
    io::{Cursor, Read, Seek},
    path::Path,
};

/// Programs and sample names in a Kontakt multi. Slot order is retained;
/// translation and MIDI-channel routing belong to the IR frontend.
pub struct Multi {
    pub programs: Vec<(u16, ni_file::kontakt::objects::Program)>,
    pub sample_names: Vec<String>,
}

pub fn read_multi(path: &Path) -> Result<Multi, LoadError> {
    use ni_file::kontakt::objects::{Bank, GroupList, ZoneList};
    let chunks = read_chunks(path)?;
    let error = |e| LoadError::decode(path, "multi", e);
    let bank = Bank::try_from(chunks.find_first(3).ok_or_else(|| LoadError::Invalid {
        path: path.into(),
        reason: "missing multi bank".into(),
    })?)
    .map_err(error)?;
    let mut slots: Vec<_> = bank.slot_list().map_err(error)?.slots.into_iter().collect();
    slots.sort_by_key(|(slot, _)| *slot);
    let mut programs = Vec::new();
    for (slot, container) in slots {
        for program in container.program_list().map_err(error)?.programs {
            programs.push((slot, program));
        }
    }
    let table = chunks
        .filename_table()
        .ok_or_else(|| LoadError::Invalid {
            path: path.into(),
            reason: "missing multi sample table".into(),
        })?
        .map_err(error)?;
    let mut sample_names = Vec::new();
    for (_, program) in &programs {
        let groups = GroupList::try_from(
            program
                .0
                .find_first(0x33)
                .ok_or_else(|| error(ni_file::Error::Static("Missing multi program groups")))?,
        )
        .map_err(error)?;
        let muted = groups
            .groups
            .iter()
            .map(|g| g.params().map(|p| p.muted))
            .collect::<Result<Vec<_>, _>>()
            .map_err(error)?;
        let zones = ZoneList::try_from(
            program
                .0
                .find_first(0x34)
                .ok_or_else(|| error(ni_file::Error::Static("Missing multi program zones")))?,
        )
        .map_err(error)?;
        for (zone, group) in zones.zones().iter().zip(&zones.group_ids) {
            if *muted
                .get(*group as usize)
                .ok_or_else(|| error(ni_file::Error::Static("Invalid multi zone group")))?
            {
                continue;
            }
            if !zone.has_sample().map_err(error)? {
                continue;
            }
            let id = zone.filename_id().map_err(error)? as u32;
            let name = table.get(&id).ok_or_else(|| {
                error(ni_file::Error::Static(
                    "Missing multi zone sample reference",
                ))
            })?;
            sample_names.push(name.clone());
        }
    }
    sample_names.sort();
    sample_names.dedup();
    Ok(Multi {
        programs,
        sample_names,
    })
}

/// The Kontakt chunk stream inside an NKS or NIS (Kontakt 5+) container.
pub fn read_chunks(path: &Path) -> Result<KontaktChunks, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    let mut file = std::fs::File::open(path).map_err(|e| LoadError::io(path, e))?;
    let mut magic = [0; 16];
    file.read_exact(&mut magic)
        .map_err(|e| LoadError::io(path, e))?;
    file.rewind().map_err(|e| LoadError::io(path, e))?;
    // The preset is bounded, not the sample section of a monolith.
    if &magic != b"/\\ NI FC MTD  /\\"
        && file.metadata().map_err(|e| LoadError::io(path, e))?.len() > 128 << 20
    {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "instrument exceeds 128 MiB".into(),
        });
    }
    let bytes = payload(&mut file, path, 0)?;
    let _span = crate::audit::Span::new("ni_chunk_parse");
    KontaktChunks::read(Cursor::new(bytes)).map_err(|e| decode("Kontakt chunks", e))
}

fn payload<R: Read + Seek>(
    reader: &mut R,
    path: &Path,
    depth: usize,
) -> Result<Vec<u8>, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    if depth > 3 {
        return Err(decode(
            "monolith preset",
            ni_file::Error::Static("Too many nested FileContainers"),
        ));
    }
    let span = crate::audit::Span::new("file_read_ni_container");
    let container = NIFile::read(&mut *reader).map_err(|e| decode("container", e))?;
    drop(span);
    let _span = crate::audit::Span::new("decrypt_expand");
    match container {
        NIFile::NKSContainer(nks) => match &nks.header {
            ni_file::kontakt::objects::BPatchHeader::BPatchHeaderV42(h) => {
                // Reuse the checked FastLZ decoder used by borrowed NKS views.
                crate::nks::expand(
                    crate::Bytes {
                        data: &nks.compressed_data,
                        offset: 0,
                    },
                    h.decompressed_length as usize,
                    128 << 20,
                )
                .map_err(|e| decode("NKS preset", ni_file::Error::Generic(e.to_string())))
            }
            _ => {
                nks.decompressed_preset()
                    .map_err(|e| decode("NKS XML", e))?;
                Err(LoadError::Invalid {
                    path: path.into(),
                    reason: "legacy Kontakt XML requires an XML-to-IR translator".into(),
                })
            }
        },
        NIFile::NISoundContainer(nis) => nis_payload(nis, path, 0),
        NIFile::Monolith(container) => {
            let mut presets = container.items.iter().filter(|item| {
                Path::new(&item.filename).extension().is_some_and(|e| {
                    ["nki", "nkm", "nkb"]
                        .iter()
                        .any(|ext| e.eq_ignore_ascii_case(ext))
                })
            });
            let preset = presets.next().ok_or_else(|| {
                decode(
                    "monolith preset",
                    ni_file::Error::Static("FileContainer has no preset member"),
                )
            })?;
            if presets.next().is_some() {
                return Err(decode(
                    "monolith preset",
                    ni_file::Error::Static("FileContainer has multiple preset members"),
                ));
            }
            let bytes = container
                .read_member(reader, preset.index, 128 << 20)
                .map_err(|e| decode("monolith preset member", e))?;
            payload(&mut Cursor::new(bytes), path, depth + 1)
        }
        _ => Err(LoadError::Invalid {
            path: path.into(),
            reason: "not an instrument container".into(),
        }),
    }
}

fn nis_payload(
    container: ni_file::nis::ItemContainer,
    path: &Path,
    depth: usize,
) -> Result<Vec<u8>, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    if depth > 3 {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "too many nested NIS wrappers".into(),
        });
    }
    if let Some(data) = container.find_data(&ni_file::nis::ItemType::AppSpecific) {
        let app = ni_file::nis::AppSpecificProperties::try_from(data)
            .map_err(|e| decode("NIS app wrapper", e))?;
        return nis_payload(
            app.subtree_item
                .item()
                .map_err(|e| decode("NIS subtree", e))?,
            path,
            depth + 1,
        );
    }
    let NISObject::BNISoundPreset(preset) = Repository::from(container).infer_schema() else {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "unsupported NIS preset structure".into(),
        });
    };
    let key = match preset.is_encrypted().map_err(|e| decode("NIS preset", e))? {
        true => Some(
            crate::library_key(path).map_err(|reason| LoadError::Access {
                path: path.into(),
                reason,
            })?,
        ),
        false => None,
    };
    let item = preset
        .encryption_item_with_key(key.as_deref())
        .map_err(|e| decode("NIS preset subtree", e))?;
    let chunk = PresetChunkItem::from(
        item.subtree
            .item()
            .map_err(|e| decode("NIS preset subtree", e))?,
    );
    Ok(chunk
        .properties()
        .map_err(|e| decode("NIS preset chunk", e))?
        .0)
}
