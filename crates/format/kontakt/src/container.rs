//! From a file to Kontakt's chunk stream: NKS (FastLZ), NIS (maybe encrypted)
//! or a monolith holding one of them.

use std::io::{Cursor, Read, Seek};
use std::path::Path;

use ni_file::NIFile;
use ni_file::kontakt::KontaktChunks;
use ni_file::kontakt::objects::BPatchHeader;
use ni_file::nis::schema::{NISObject, PresetChunkItem, Repository};
use ni_file::nis::{AppSpecificProperties, ItemContainer, ItemType};

use crate::Error;

/// A preset larger than this is not a preset.
const MAX_PRESET: usize = 128 << 20;

/// The raw Kontakt chunk stream of a preset file (NKI, NKM, snapshot,
/// monolith), decompressed and decrypted.
///
/// # Errors
/// When the file is not a Kontakt container this reader understands.
pub fn chunks(path: &Path) -> Result<KontaktChunks, Error> {
    let mut file = std::fs::File::open(path).map_err(|e| Error::new(path, "open", e))?;
    let bytes = payload(&mut file, path, 0)?;
    KontaktChunks::read(Cursor::new(bytes)).map_err(|e| Error::new(path, "Kontakt chunks", e))
}

fn payload<R: Read + Seek>(reader: &mut R, path: &Path, depth: usize) -> Result<Vec<u8>, Error> {
    if depth > 3 {
        return Err(Error::new(path, "container", "nested too deep"));
    }
    let container = NIFile::read(&mut *reader).map_err(|e| Error::new(path, "container", e))?;
    match container {
        NIFile::NKSContainer(nks) => match &nks.header {
            BPatchHeader::BPatchHeaderV42(h) => {
                let expected = h.decompressed_length as usize;
                if expected > MAX_PRESET {
                    return Err(Error::new(path, "NKS preset", "too large"));
                }
                crate::fastlz::expand(&nks.compressed_data, expected)
                    .map_err(|e| Error::new(path, "NKS preset", e))
            }
            _ => Err(Error::new(
                path,
                "NKS preset",
                "legacy XML instruments are not read",
            )),
        },
        NIFile::NISoundContainer(nis) => nis_payload(nis, path, 0),
        NIFile::Monolith(container) => {
            let is_preset = |name: &str| {
                Path::new(name).extension().is_some_and(|e| {
                    ["nki", "nkm", "nkb"]
                        .iter()
                        .any(|x| e.eq_ignore_ascii_case(x))
                })
            };
            let mut presets = container.items.iter().filter(|i| is_preset(&i.filename));
            let (Some(preset), None) = (presets.next(), presets.next()) else {
                return Err(Error::new(
                    path,
                    "monolith",
                    "not exactly one preset member",
                ));
            };
            let bytes = container
                .read_member(reader, preset.index, MAX_PRESET as u64)
                .map_err(|e| Error::new(path, "monolith preset", e))?;
            payload(&mut Cursor::new(bytes), path, depth + 1)
        }
        _ => Err(Error::new(path, "container", "not an instrument")),
    }
}

fn nis_payload(container: ItemContainer, path: &Path, depth: usize) -> Result<Vec<u8>, Error> {
    if depth > 3 {
        return Err(Error::new(path, "NIS", "nested too deep"));
    }
    if let Some(data) = container.find_data(&ItemType::AppSpecific) {
        let app =
            AppSpecificProperties::try_from(data).map_err(|e| Error::new(path, "NIS app", e))?;
        let inner = app
            .subtree_item
            .item()
            .map_err(|e| Error::new(path, "NIS subtree", e))?;
        return nis_payload(inner, path, depth + 1);
    }
    let NISObject::BNISoundPreset(preset) = Repository::from(container).infer_schema() else {
        return Err(Error::new(path, "NIS", "not a sound preset"));
    };
    let key = if preset
        .is_encrypted()
        .map_err(|e| Error::new(path, "NIS preset", e))?
    {
        Some(crate::access::library_key(path).map_err(|e| Error::new(path, "library access", e))?)
    } else {
        None
    };
    let item = preset
        .encryption_item_with_key(key.as_deref())
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    let subtree = item
        .subtree
        .item()
        .map_err(|e| Error::new(path, "NIS preset", e))?;
    let chunk = PresetChunkItem::from(subtree)
        .properties()
        .map_err(|e| Error::new(path, "NIS preset chunk", e))?;
    Ok(chunk.0)
}
