//! Container access shared by translation and the installed-library census.
//! Presets and decoded data remain in memory; local access values are never exposed.
use crate::LoadError;
use ni_file::{
    NIFile,
    kontakt::KontaktChunks,
    nis::schema::{NISObject, PresetChunkItem, Repository},
};
use std::{io::Cursor, path::Path};

/// The Kontakt chunk stream inside an NKS or NIS (Kontakt 5+) container.
pub fn read_chunks(path: &Path) -> Result<KontaktChunks, LoadError> {
    let decode = |what, error| LoadError::decode(path, what, error);
    let mut file = std::fs::File::open(path).map_err(|e| LoadError::io(path, e))?;
    if file.metadata().map_err(|e| LoadError::io(path, e))?.len() > 128 << 20 {
        return Err(LoadError::Invalid {
            path: path.into(),
            reason: "instrument exceeds 128 MiB".into(),
        });
    }
    let bytes = match NIFile::read(&mut file).map_err(|e| decode("container", e))? {
        NIFile::NKSContainer(nks) => nks
            .decompressed_preset()
            .map_err(|e| decode("NKS preset", e))?,
        NIFile::NISoundContainer(nis) => nis_payload(nis, path, 0)?,
        _ => {
            return Err(LoadError::Invalid {
                path: path.into(),
                reason: "not an instrument container".into(),
            });
        }
    };
    KontaktChunks::read(Cursor::new(bytes)).map_err(|e| decode("Kontakt chunks", e))
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
