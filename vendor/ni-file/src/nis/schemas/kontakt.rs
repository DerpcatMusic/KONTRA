use std::io::Cursor;

use crate::nis::items::preset_chunk_data;

use crate::{
    kontakt::{
        objects::{BPatchHeader, BPatchHeaderV42},
        schemas::KontaktPreset,
        KontaktInstrument,
    },
    nis::{
        BNISoundHeader, BNISoundPresetProperties, EncryptionItem, ItemContainer, ItemType, Preset,
    },
    Error,
};

impl ItemContainer {
    pub fn find_kontakt_preset_item(&self) -> Option<Result<Preset, Error>> {
        self.find_item::<BNISoundPresetProperties>(&ItemType::BNISoundPreset)
            .map(|result| result.map(|properties| properties.preset))
    }

    pub fn find_kontakt_header(&self) -> Option<Result<BPatchHeaderV42, Error>> {
        self.find_item::<BNISoundHeader>(&ItemType::BNISoundHeader)
            .map(|r| r.map(|h| h.0))
    }

    pub fn find_encryption_item(&self) -> Option<Result<EncryptionItem, Error>> {
        self.find_data(&ItemType::EncryptionItem)
            .map(EncryptionItem::try_from)
    }

    pub fn extract_kontakt_preset(&self) -> Option<Result<KontaktInstrument, Error>> {
        let header = match self.find_kontakt_header()? {
            Ok(header) => header,
            Err(e) => return Some(Err(e)),
        };

        Some(preset_chunk_data(self)?.and_then(|data| {
            let preset = KontaktPreset::read(
                Cursor::new(data),
                &header.app_signature,
                &header.patch_type,
                &header.min_supported_version,
            )?;
            Ok(KontaktInstrument {
                header: BPatchHeader::BPatchHeaderV42(header),
                preset,
            })
        }))
    }
}

#[derive(Debug)]
pub struct BNISoundPresetContainer(ItemContainer);

impl BNISoundPresetContainer {
    /// Parses the BNISoundPreset Items property data
    pub fn properties(&self) -> Result<BNISoundPresetProperties, Error> {
        BNISoundPresetProperties::try_from(&self.0.data)
    }

    /// Attempts to fetch and parse the BNISoundHeader object
    pub fn header(&self) -> Option<Result<BNISoundHeader, Error>> {
        self.0
            .find_data(&ItemType::BNISoundHeader)
            .map(|item_data| BNISoundHeader::try_from(item_data))
    }

    /// Attempts to fetch the raw inner preset chunk data
    pub fn preset_data(&self) -> Option<Result<Vec<u8>, Error>> {
        preset_chunk_data(&self.0)
    }

    pub fn preset(&self) -> Option<Result<KontaktPreset, Error>> {
        Some(match self.header()? {
            Ok(header) => self.preset_data()?.and_then(|preset_data| {
                KontaktPreset::read(
                    &mut Cursor::new(preset_data),
                    &header.0.app_signature,
                    &header.0.patch_type,
                    &header.0.min_supported_version,
                )
            }),
            Err(e) => return Some(Err(e)),
        })
    }
}

impl TryFrom<&ItemContainer> for BNISoundPresetContainer {
    type Error = Error;

    fn try_from(container: &ItemContainer) -> Result<Self, Self::Error> {
        let id = &container.id();
        if id != &ItemType::BNISoundPreset {
            return Err(Error::ItemWrapError {
                expected: ItemType::BNISoundPreset,
                got: id.clone(),
            });
        }
        Ok(Self(container.clone()))
    }
}
