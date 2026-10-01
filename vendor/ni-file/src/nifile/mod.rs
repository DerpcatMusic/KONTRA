use crate::{
    detect::NIFileType,
    file_container::NIFileContainer,
    nis::ItemContainer,
    nis::{AppSpecificProperties, EncryptionItem, ItemType, PresetChunkItemProperties},
    nks::container::NKSContainer,
    read_bytes::*,
    Error,
};

pub enum NIFile {
    NKSContainer(NKSContainer),
    NISoundContainer(ItemContainer),
    Monolith(NIFileContainer),
    KontaktResource,
    NICompressedWave,
    NICache,
    FM8Preset,
}

pub enum NIPreset {
    KontaktInstrument,
}

impl NIFile {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let filetype = NIFileType::read(&mut reader)?;
        reader.rewind()?;

        Ok(match filetype {
            NIFileType::NISContainer => NIFile::NISoundContainer(ItemContainer::read(reader)?),
            NIFileType::Monolith => NIFile::Monolith(NIFileContainer::read(reader)?),
            NIFileType::NICompressedWave => NIFile::NICompressedWave,
            NIFileType::NKSContainer(_) | NIFileType::KontaktMultiV1 => {
                NIFile::NKSContainer(NKSContainer::read(reader)?)
            }
            NIFileType::KontaktResource => NIFile::KontaktResource,
            NIFileType::NICache => NIFile::NICache,
            NIFileType::FM8LE => NIFile::FM8Preset,

            // Unknown also covers damaged files (e.g. zero-filled reads).
            other => {
                return Err(Error::Generic(format!(
                    "Unsupported NI file type: {other:?}"
                )))
            }
        })
    }

    /// Extract raw preset data from this container (if applicable).
    pub fn inner_preset(&self) -> Result<Vec<u8>, Error> {
        match self {
            Self::NKSContainer(nks) => nks.decompressed_preset(),
            Self::NISoundContainer(nis) => {
                if let Some(preset) = nis.find(&ItemType::BNISoundPreset) {
                    let frame = preset
                        .find_data(&ItemType::EncryptionItem)
                        .ok_or(Error::Static("No EncryptionItem"))?;
                    let inner = EncryptionItem::try_from(frame)?.subtree.item()?;
                    return Ok(inner
                        .find_item::<PresetChunkItemProperties>(&ItemType::PresetChunkItem)
                        .ok_or(Error::Static("No PresetChunkItem"))??
                        .0);
                }
                if let Some(app) = nis.find_item::<AppSpecificProperties>(&ItemType::AppSpecific) {
                    return Self::NISoundContainer(app?.subtree_item.item()?).inner_preset();
                }
                Err(Error::Static("No supported Kontakt preset detected"))
            }
            _ => Err(Error::Static("No supported preset detected")),
        }
    }
}
