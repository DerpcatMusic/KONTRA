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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nis::{ItemData, ItemDataHeader, ItemHeader, SubtreeItem};

    #[test]
    fn decoded_subtrees_are_exact_and_nested_extraction_is_bounded() {
        fn layer(id: u32, data: Vec<u8>, inner: Option<ItemData>) -> ItemData {
            ItemData { header: ItemDataHeader { length: 0, domain_id: *b"NISD", item_id: id, version: 1 },
                inner: inner.map(Box::new), data }
        }
        fn item(data: ItemData) -> ItemContainer {
            ItemContainer { header: ItemHeader { length: 0, magic: b"hsin".to_vec(), header_flags: 0,
                reserved: 0, uuid: vec![0;16] }, data, children: vec![], child_headers: vec![], trailing_data: vec![] }
        }
        let base = || layer(1, 1u32.to_le_bytes().to_vec(), None);
        let mut nested = item(base());
        let mut bytes = Vec::new(); nested.write(&mut bytes).unwrap();
        let subtree = SubtreeItem { inner_data: bytes.clone() };
        assert!(subtree.item().is_ok());
        bytes.push(0xff);
        assert!(SubtreeItem { inner_data: bytes }.item().unwrap_err().to_string().contains("Trailing data"));
        for depth in 1..=64 {
            let mut encoded = Vec::new(); nested.write(&mut encoded).unwrap();
            let mut frame = 1u32.to_le_bytes().to_vec(); frame.push(0); frame.extend(encoded);
            let mut properties = 1u32.to_le_bytes().to_vec();
            properties.extend(0u32.to_le_bytes()); properties.extend(0u32.to_le_bytes());
            nested = item(layer(0x75, properties, Some(layer(0x73, frame, Some(base())))));
            let error = NIFile::NISoundContainer(nested.clone()).inner_preset().unwrap_err().to_string();
            assert_eq!(error.contains("Too many nested"), depth == 64, "depth {depth}: {error}");
        }
    }
}

pub enum NIPreset {
    KontaktInstrument,
}

impl NIFile {
    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        let (filetype, nis) = NIFileType::read_with_nis(&mut reader)?;
        if nis.is_none() {
            reader.rewind()?;
        }

        Ok(match filetype {
            NIFileType::NISContainer => NIFile::NISoundContainer(
                nis.ok_or(Error::Static("Missing detected NIS container"))?,
            ),
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

    /// Serialize a supported raw container without interpreting its preset data.
    /// Currently only NIS containers have a writer; other formats return an error.
    pub fn write<W: std::io::Write + ?Sized>(&self, writer: &mut W) -> Result<(), Error> {
        match self {
            Self::NISoundContainer(item) => item.write(writer),
            _ => Err(Error::Static("Writing this NI file type is unsupported")),
        }
    }

    /// Extract raw preset data from this container (if applicable).
    pub fn inner_preset(&self) -> Result<Vec<u8>, Error> {
        self.inner_preset_with_key(None)
    }

    /// Extract a preset using access data supplied by the caller when required.
    pub fn inner_preset_with_key(
        &self,
        key: Option<&dyn crate::nis::LibraryKey>,
    ) -> Result<Vec<u8>, Error> {
        self.inner_preset_at_depth(key, 0)
    }

    fn inner_preset_at_depth(
        &self,
        key: Option<&dyn crate::nis::LibraryKey>,
        depth: usize,
    ) -> Result<Vec<u8>, Error> {
        if depth >= 64 {
            return Err(Error::Static("Too many nested NIS preset wrappers"));
        }
        match self {
            Self::NKSContainer(nks) => nks.decompressed_preset(),
            Self::NISoundContainer(nis) => {
                if let Some(preset) = nis.find(&ItemType::BNISoundPreset) {
                    let frame = preset
                        .find_data(&ItemType::EncryptionItem)
                        .ok_or(Error::Static("No EncryptionItem"))?;
                    let inner = EncryptionItem::read_with_key(frame, key)?.subtree.item()?;
                    return Ok(inner
                        .find_item::<PresetChunkItemProperties>(&ItemType::PresetChunkItem)
                        .ok_or(Error::Static("No PresetChunkItem"))??
                        .0);
                }
                if let Some(app) = nis.find_item::<AppSpecificProperties>(&ItemType::AppSpecific) {
                    return Self::NISoundContainer(app?.subtree_item.item()?)
                        .inner_preset_at_depth(key, depth + 1);
                }
                Err(Error::Static("No supported Kontakt preset detected"))
            }
            _ => Err(Error::Static("No supported preset detected")),
        }
    }
}
