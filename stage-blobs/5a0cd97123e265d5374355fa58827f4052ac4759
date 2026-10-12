use crate::{
    nis::{ItemContainer, ItemType, Preset, PresetChunkItemProperties},
    Error,
};

/// Absence of an EncryptionItem is distinct from an invalid present payload.
pub(crate) fn preset_chunk_data(container: &ItemContainer) -> Option<Result<Vec<u8>, Error>> {
    Some(
        container
            .find_encryption_item()?
            .and_then(|enc| {
                let item = enc.subtree.item()?;
                Ok(item
                    .find_item::<PresetChunkItemProperties>(&ItemType::PresetChunkItem)
                    .ok_or(Error::Static("Missing PresetChunkItem in preset subtree"))??
                    .0)
            })
            .map_err(|error| Error::context("NIS preset payload".into(), error)),
    )
}

#[derive(Debug)]
pub struct PresetContainer(ItemContainer);

impl PresetContainer {
    pub fn properties(&self) -> Result<Preset, Error> {
        Preset::try_from(&self.0.data)
    }

    /// Attempts to fetch the raw inner preset chunk data
    pub fn preset_data(&self) -> Option<Result<Vec<u8>, Error>> {
        preset_chunk_data(&self.0)
    }
}

impl TryFrom<&ItemContainer> for PresetContainer {
    type Error = Error;

    fn try_from(container: &ItemContainer) -> Result<Self, Self::Error> {
        let id = container.id();
        if id != ItemType::Preset {
            return Err(Error::ItemWrapError {
                expected: ItemType::Preset,
                got: id.clone(),
            });
        }
        Ok(Self(container.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kontakt::schemas::KontaktPreset;
    use crate::nis::{
        schemas::kontakt::BNISoundPresetContainer, ItemData, ItemDataHeader, ItemHeader,
    };

    fn item(domain_id: [u8; 4], item_id: u32, data: Vec<u8>) -> ItemContainer {
        let base = ItemData {
            header: ItemDataHeader {
                length: 0,
                domain_id: *b"NISD",
                item_id: 1,
                version: 1,
            },
            inner: None,
            data: 1u32.to_le_bytes().to_vec(),
        };
        ItemContainer {
            header: ItemHeader {
                length: 0,
                magic: b"hsin".to_vec(),
                header_flags: 0,
                reserved: 0,
                uuid: vec![0; 16],
            },
            data: ItemData {
                header: ItemDataHeader {
                    length: 0,
                    domain_id,
                    item_id,
                    version: 1,
                },
                inner: (item_id != 1).then_some(Box::new(base)),
                data,
            },
            children: vec![],
            child_headers: vec![],
            trailing_data: vec![],
        }
    }

    fn encoded(inner: &ItemContainer) -> Vec<u8> {
        let mut bytes = Vec::new();
        inner.write(&mut bytes).unwrap();
        bytes
    }

    fn encryption(inner: Vec<u8>, protected: bool) -> ItemContainer {
        let mut subtree = 1u32.to_le_bytes().to_vec();
        subtree.push(0); // uncompressed; no key or protected-payload decoding
        subtree.extend(inner);
        let mut enc = item(*b"NISD", 0x74, vec![1, 0, 0, 0, protected as u8]);
        enc.data.inner = Some(Box::new(item(*b"NISD", 0x73, subtree).data));
        enc
    }

    fn chunk(data: &[u8]) -> ItemContainer {
        let mut properties = 1u32.to_le_bytes().to_vec();
        properties.extend(0u32.to_le_bytes()); // unchanged checksum field
        properties.extend(1u32.to_le_bytes());
        properties.extend((data.len() as u64).to_le_bytes());
        properties.extend(data);
        item(*b"NISD", 0x6d, properties)
    }

    fn preset_root(enc: Option<ItemContainer>, kontakt: bool) -> ItemContainer {
        let mut root = if kontakt {
            item(*b"NIK4", 3, vec![])
        } else {
            item(*b"NISD", 0x65, vec![])
        };
        root.children.extend(enc);
        root
    }

    // A minimal header follows the existing fixed-size BPatchHeaderV42 reader.
    fn header(id: &[u8; 4]) -> ItemContainer {
        let mut bytes = vec![0; 222];
        bytes[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
        bytes[8..10].copy_from_slice(&0x0110u16.to_le_bytes());
        bytes[10..14].copy_from_slice(&0xEA37631Au32.to_le_bytes());
        bytes[14..16].copy_from_slice(&1u16.to_le_bytes()); // NKI
        for (byte, value) in bytes[20..24].iter_mut().zip(id.iter().rev()) {
            *byte = *value;
        }
        item(*b"NIK4", 4, bytes)
    }

    #[test]
    fn nis_readers_preset_extraction_preserves_absence_and_present_errors() {
        let payload = b"opaque preset bytes";
        for kontakt in [false, true] {
            let extract = |enc| {
                let root = preset_root(enc, kontakt);
                if kontakt {
                    BNISoundPresetContainer::try_from(&root)
                        .unwrap()
                        .preset_data()
                } else {
                    PresetContainer::try_from(&root).unwrap().preset_data()
                }
            };
            assert!(extract(None).is_none());
            assert_eq!(
                extract(Some(encryption(encoded(&chunk(payload)), false)))
                    .unwrap()
                    .unwrap(),
                payload
            );

            let mut bad_version = encryption(vec![], false);
            bad_version.data.data[..4].copy_from_slice(&2u32.to_le_bytes());
            let mut missing_subtree = encryption(vec![], false);
            missing_subtree.data.inner = None;
            let mut bad_subtree = encryption(vec![], false);
            bad_subtree.data.inner.as_mut().unwrap().data[..4].copy_from_slice(&2u32.to_le_bytes());
            let cases = [
                (encryption(vec![], true), "Encrypted preset"),
                (bad_version, "Unsupported encryption item version"),
                (missing_subtree, "Missing preset subtree"),
                (bad_subtree, "Unsupported subtree version"),
                (
                    encryption(8u64.to_le_bytes().to_vec(), false),
                    "NIS child header",
                ),
                (
                    encryption(encoded(&item(*b"NISD", 1, vec![])), false),
                    "Missing PresetChunkItem",
                ),
                (
                    encryption(encoded(&item(*b"NISD", 0x6d, vec![])), false),
                    "",
                ),
            ];
            for (enc, diagnostic) in cases {
                let error = extract(Some(enc))
                    .expect("present payload")
                    .unwrap_err()
                    .to_string();
                assert!(error.contains("NIS preset payload"), "{error}");
                assert!(error.contains(diagnostic), "{error}");
            }
        }
    }

    #[test]
    fn nis_readers_property_guards_reject_invalid_frames() {
        use crate::nis::{BNISoundHeader, BNISoundPresetProperties};

        let mut root = preset_root(None, true);
        root.data.inner = None;
        assert!(matches!(root.find_kontakt_preset_item(), Some(Err(_))));

        let mut properties = vec![1, 0, 0, 0, 0];
        properties.extend(0xdeadbeefu32.to_le_bytes());
        properties.extend(1u32.to_le_bytes());
        properties.extend(0u32.to_le_bytes());
        assert_eq!(
            Preset::read(std::io::Cursor::new(&properties))
                .unwrap()
                .authoring_app,
            crate::nis::AuthoringApplication::Unknown(0xdeadbeef)
        );
        for offset in [0, 9] {
            let mut invalid = properties.clone();
            invalid[offset..offset + 4].copy_from_slice(&2u32.to_le_bytes());
            assert!(Preset::read(std::io::Cursor::new(invalid)).is_err());
        }
        for end in 0..properties.len() {
            assert!(Preset::read(std::io::Cursor::new(&properties[..end])).is_err());
        }

        let valid = header(b"Kon7");
        assert!(BNISoundHeader::try_from(&valid.data).is_ok());
        for offset in [0, 8] {
            let mut invalid = valid.clone();
            invalid.data.data[offset] ^= 1;
            assert!(BNISoundHeader::try_from(&invalid.data).is_err());
        }
        let wrong = item(*b"NISD", 1, vec![]);
        assert!(matches!(
            Preset::try_from(&wrong.data),
            Err(Error::ItemWrapError { .. })
        ));
        assert!(matches!(
            BNISoundHeader::try_from(&wrong.data),
            Err(Error::ItemWrapError { .. })
        ));
        assert!(matches!(
            BNISoundPresetProperties::try_from(&wrong.data),
            Err(Error::ItemWrapError { .. })
        ));
    }

    #[test]
    fn nis_readers_kontakt_lookup_and_instrument_extraction_propagate_errors() {
        let absent = item(*b"NISD", 1, vec![]);
        assert!(absent.find_kontakt_preset_item().is_none());
        assert!(absent.extract_kontakt_preset().is_none());
        let mut root = preset_root(None, true);
        // Correct wrapper type, truncated properties: formerly discarded as None.
        root.data.inner = Some(Box::new(item(*b"NISD", 0x65, vec![]).data));
        assert!(matches!(root.find_kontakt_preset_item(), Some(Err(_))));
        let mut properties = vec![1, 0, 0, 0, 0];
        properties.extend(2u32.to_le_bytes()); // Kontakt authoring app
        properties.extend(1u32.to_le_bytes());
        properties.extend(0u32.to_le_bytes()); // empty version string
        root.data.inner.as_mut().unwrap().data = properties;
        assert!(matches!(root.find_kontakt_preset_item(), Some(Ok(_))));

        root.children.push(header(b"Kon7"));
        assert!(root.extract_kontakt_preset().is_none());
        root.children.push(encryption(vec![], true));
        assert!(root
            .extract_kontakt_preset()
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("Encrypted preset"));
        root.children[1] = encryption(encoded(&chunk(&[0xff])), false);
        assert!(root.extract_kontakt_preset().unwrap().is_err()); // truncated chunk
        root.children[1] = encryption(encoded(&chunk(&[])), false);
        assert!(root.extract_kontakt_preset().unwrap().is_err()); // missing Program
        root.children[0] = header(b"Kon8");
        assert!(matches!(
            root.extract_kontakt_preset().unwrap().unwrap().preset,
            KontaktPreset::Unsupported(_)
        ));
        root.children[0].data.data.clear();
        assert!(root.extract_kontakt_preset().unwrap().is_err());
    }
}
