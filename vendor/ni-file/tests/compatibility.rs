use ni_file::{
    kontakt::{
        objects::{LoopArray, ZoneList},
        KontaktChunks,
    },
    nkr::Archive,
};
use std::io::Cursor;

#[test]
fn truncated_containers_preserve_decoder_context_and_valid_roundtrips() {
    use ni_file::{NIFile, NIFileError, nis::{ItemContainer, ItemData, ItemDataHeader, ItemHeader},
        kontakt::{Chunk, objects::BPatchMetaInfoHeader}};
    let item = ItemContainer {
        header: ItemHeader { length: 40, magic: b"hsin".to_vec(), header_flags: 0, reserved: 0, uuid: vec![0; 16] },
        data: ItemData { header: ItemDataHeader { length: 20, domain_id: *b"NISD", item_id: 1, version: 1 }, inner: None, data: vec![] },
        children: vec![], child_headers: vec![], trailing_data: vec![],
    };
    let mut bytes = Vec::new();
    item.write(&mut bytes).unwrap();
    let valid = NIFile::read(Cursor::new(&bytes)).unwrap();
    let mut encoded = Vec::new();
    valid.write(&mut encoded).unwrap();
    assert_eq!(encoded, bytes, "a supported NIS roundtrip preserves every byte");
    let error = NIFile::read(Cursor::new(&bytes[..bytes.len()-1])).err().unwrap();
    let message = error.to_string();
    assert!(message.contains("NIS item body at offset 40") && message.contains("declared body length 28/version 1")
        && message.contains("available 27"), "{message}");
    assert!(!message.contains("Unknown"), "a recognized NIS signature retains the decoder cause");
    let mut error = NIFile::read(Cursor::new(&bytes[..21])).err().unwrap();
    assert!(error.to_string().contains("NIS item header at offset 0"));
    while let NIFileError::Context { source, .. } = error { error = *source; }
    assert!(matches!(error, NIFileError::IO(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof),
        "context preserves the original EOF error: {error:?}");

    let chunks = KontaktChunks(vec![Chunk { id: 0x28, data: vec![1,2,3] }]);
    let mut bytes = Vec::new();
    chunks.write(&mut bytes).unwrap();
    let mut encoded = Vec::new();
    KontaktChunks::read(Cursor::new(&bytes)).unwrap().write(&mut encoded).unwrap();
    assert_eq!(encoded, bytes);
    for (len, expected) in [(4, "chunk 0x0028 length at offset 2"), (8, "chunk 0x0028 body at offset 6, declared length 3")] {
        let message = KontaktChunks::read(Cursor::new(&bytes[..len])).unwrap_err().to_string();
        assert!(message.contains(expected), "{message}");
    }
    let mut bytes = 0x7FA89012u32.to_le_bytes().to_vec();
    bytes.extend(5u32.to_le_bytes());
    bytes.extend(0x1000u16.to_le_bytes());
    let message = NIFile::read(Cursor::new(bytes)).err().unwrap().to_string();
    assert!(message.contains("NKS patch header at offset 8, format word 0x1000") && message.contains("declared 212"), "{message}");
    assert!(BPatchMetaInfoHeader::read(Cursor::new([0;8])).is_err(), "invalid metadata returns an error rather than panicking");
}

#[test]
fn malformed_nis_lengths_and_children_return_errors() {
    use ni_file::nis::{ItemContainer, ItemData, ItemDataHeader, ItemType};
    let mut frame = 39u64.to_le_bytes().to_vec();
    frame.extend(1u32.to_le_bytes());
    frame.extend(b"hsin");
    frame.extend([0; 24]);
    assert!(ItemContainer::read(Cursor::new(&frame)).is_err());
    let mut data = 19u64.to_le_bytes().to_vec();
    data.extend(b"DSIN");
    data.extend(1u32.to_le_bytes());
    data.extend(1u32.to_le_bytes());
    assert!(ItemData::read(Cursor::new(&data)).is_err());
    let header = ItemDataHeader { length: 20, domain_id: [0xff; 4], item_id: 1, version: 1 };
    assert!(matches!(header.item_type(), ItemType::Unknown(1, _)));
    // A minimal empty item followed by an unsupported child-list version.
    data[..8].copy_from_slice(&20u64.to_le_bytes());
    frame[..8].copy_from_slice(&68u64.to_le_bytes());
    frame.extend(data);
    frame.extend(2u32.to_le_bytes());
    frame.extend(0u32.to_le_bytes());
    assert!(ItemContainer::read(Cursor::new(frame)).is_err());
}

#[test]
fn nks_extraction_checks_decompressed_length_and_returns_errors() {
    use ni_file::{NIFile, kontakt::objects::{BPatchHeader, BPatchHeaderV42}, nks::container::NKSContainer};
    let mut header = vec![0; 212];
    header[..4].copy_from_slice(&0xEA37631Au32.to_le_bytes());
    let mut h = BPatchHeaderV42::read_le(Cursor::new(&header)).unwrap();
    h.decompressed_length = 4;
    let mut nks = NKSContainer { header: BPatchHeader::BPatchHeaderV42(h), compressed_data: vec![3,b't',b'e',b's',b't'], meta_info: None };
    assert_eq!(nks.decompressed_preset().unwrap(), b"test");
    if let BPatchHeader::BPatchHeaderV42(h) = &mut nks.header { h.decompressed_length = 5; }
    assert!(nks.decompressed_preset().is_err());
    if let BPatchHeader::BPatchHeaderV42(h) = &mut nks.header { h.decompressed_length = u32::MAX; }
    assert!(nks.decompressed_preset().is_err());
    nks.compressed_data.clear();
    assert!(nks.preset().is_err());
    assert!(NKSContainer::read(Cursor::new([0;4])).is_err());
    assert!(BPatchHeaderV42::read_le(Cursor::new([0;212])).is_err());
    assert!(NIFile::NICompressedWave.inner_preset().is_err());
    if let BPatchHeader::BPatchHeaderV42(h) = &mut nks.header { h.decompressed_length = 4; }
    nks.compressed_data = vec![3,b't',b'e',b's',b't'];
    assert_eq!(NIFile::NKSContainer(nks).inner_preset().unwrap(), b"test");
}

#[test]
fn generic_nis_extraction_uses_the_existing_subtree_reader() {
    use ni_file::{NIFile, nis::{ItemContainer, ItemData, ItemDataHeader, ItemHeader, PresetChunkItemProperties}};
    let data_header = |item_id| ItemDataHeader { length: 20, domain_id: *b"NISD", item_id, version: 1 };
    let container_header = ItemHeader { length: 40, magic: b"hsin".to_vec(), header_flags: 0, reserved: 0, uuid: vec![0;16] };
    let mut props = 1u32.to_le_bytes().to_vec();
    props.extend(0u32.to_le_bytes());
    props.extend(1u32.to_le_bytes());
    props.extend(4u64.to_le_bytes());
    props.extend(b"test");
    // Encoded PresetChunkItem with its empty base Item and no child containers.
    let mut inner = (40u64 + 40 + props.len() as u64 + 8).to_le_bytes().to_vec();
    inner.extend(1u32.to_le_bytes());inner.extend(b"hsin");inner.extend([0;24]);
    inner.extend((40u64 + props.len() as u64).to_le_bytes());inner.extend(b"DSIN");inner.extend(0x6du32.to_le_bytes());inner.extend(1u32.to_le_bytes());
    inner.extend(20u64.to_le_bytes());inner.extend(b"DSIN");inner.extend(1u32.to_le_bytes());inner.extend(1u32.to_le_bytes());inner.extend(&props);
    inner.extend(1u32.to_le_bytes());inner.extend(0u32.to_le_bytes());
    let mut subtree = 1u32.to_le_bytes().to_vec();subtree.push(0);subtree.extend(inner);
    let encryption = ItemData { header: data_header(0x74), inner: Some(Box::new(ItemData { header: data_header(0x73), inner: None, data: subtree })), data: vec![1,0,0,0,0] };
    let preset = ItemContainer { header: container_header.clone(), data: ItemData { header: ItemDataHeader { domain_id: *b"NIK4", item_id: 3, ..data_header(3) }, inner: None, data: vec![] }, child_headers: vec![[0;12]], trailing_data: vec![], children: vec![ItemContainer { header: container_header, data: encryption, children: vec![], child_headers: vec![], trailing_data: vec![] }] };
    assert_eq!(NIFile::NISoundContainer(preset).inner_preset().unwrap(), b"test");
    props[..4].copy_from_slice(&2u32.to_le_bytes());
    assert!(PresetChunkItemProperties::read(Cursor::new(&props)).is_err());
    props[..4].copy_from_slice(&1u32.to_le_bytes());props[8..12].copy_from_slice(&2u32.to_le_bytes());
    assert!(PresetChunkItemProperties::read(Cursor::new(props)).is_err());
}
#[test]
fn loop_slots_are_a_mask_not_a_count() {
    let mut data = vec![0b10001];
    for start in [12i32, 500] {
        data.extend([0, 0x60, 0]);
        for v in [1i32, start, 100, 0] {
            data.extend(v.to_le_bytes());
        }
        data.push(0);
        data.extend(1f32.to_le_bytes());
        data.extend(10i32.to_le_bytes());
    }
    let loops = LoopArray::read(Cursor::new(&data)).unwrap();
    assert_eq!(loops.items.len(), 2);
    assert_eq!(loops.items[1].loop_start, 500);
    data.pop();
    assert!(LoopArray::read(Cursor::new(data)).is_err());
}

#[test]
fn filename_table_records_preserve_native_metadata_and_edit_paths() {
    use ni_file::kontakt::objects::{FNTableRecord, BFileNameSegmentRecord};
    // Authored v2 table: all supported native segment kinds, exact UTF-16 code
    // units (including an unpaired surrogate), full-width metadata and an
    // uninterpreted extension. No proprietary table or filename is included.
    let mut data = 2u16.to_le_bytes().to_vec();
    data.extend(1u32.to_le_bytes()); // special files
    data.extend(9i32.to_le_bytes());
    for kind in [1, 2, 3, 4, 5, 6, 8, 9, 11] {
        data.push(kind);
        if matches!(kind, 1 | 2 | 4 | 5 | 8 | 9) {
            data.extend(2u32.to_le_bytes());
            data.extend(0xd800u16.to_le_bytes());
            data.extend(0u16.to_le_bytes());
        }
    }
    let sample_count_offset = data.len();
    data.extend(1u32.to_le_bytes());
    let sample_segments_offset = data.len();
    data.extend(1i32.to_le_bytes());
    data.push(4);
    let sample_text_length_offset = data.len();
    data.extend(1u32.to_le_bytes());
    data.extend(65u16.to_le_bytes());
    data.extend(u64::MAX.to_le_bytes());
    data.extend(0x12345678u32.to_le_bytes());
    data.extend(0u32.to_le_bytes()); // other files
    let known_length = data.len();
    data.extend([0xab, 0xcd, 0xef]);
    let chunk = ni_file::kontakt::Chunk { id: 0x4b, data };
    let mut record = FNTableRecord::try_from(&chunk).unwrap();
    assert_eq!(record.samples[0].timestamp, u64::MAX);
    assert_eq!(record.samples[0].unknown_record, 0x12345678);
    assert_eq!(record.trailing_data, [0xab, 0xcd, 0xef]);
    assert_eq!(record.to_chunk().unwrap().data, chunk.data);
    let mut encoded = Vec::new();
    record.write(&mut encoded).unwrap();
    assert_eq!(ni_file::kontakt::Chunk::read(Cursor::new(encoded)).unwrap().data, chunk.data);
    for end in 0..known_length {
        let truncated = ni_file::kontakt::Chunk { id: 0x4b, data: chunk.data[..end].to_vec() };
        assert!(FNTableRecord::try_from(&truncated).is_err(), "end={end}");
    }
    for (offset, bytes) in [
        (sample_count_offset, u32::MAX.to_le_bytes()),
        (sample_segments_offset, (-1i32).to_le_bytes()),
        (sample_text_length_offset, u32::MAX.to_le_bytes()),
    ] {
        let mut malformed = chunk.data.clone();
        malformed[offset..offset + 4].copy_from_slice(&bytes);
        assert!(FNTableRecord::try_from(&ni_file::kontakt::Chunk { id: 0x4b, data: malformed }).is_err());
    }
    record.samples[0].filename.segments[0].text = Some("Authored.ncw".encode_utf16().collect());
    let edited = FNTableRecord::try_from(&record.to_chunk().unwrap()).unwrap();
    assert_eq!(edited, record);
    assert_eq!(edited.special_files[0], FNTableRecord::try_from(&chunk).unwrap().special_files[0]);
    record.samples[0].filename.segments.push(BFileNameSegmentRecord { kind: 11, text: Some(vec![65]) });
    let mut output = Vec::new();
    assert!(record.write(&mut output).is_err());
    assert!(output.is_empty());
    let mut wrong_version = chunk.data.clone();
    wrong_version[0] = 3;
    assert!(FNTableRecord::try_from(&ni_file::kontakt::Chunk { id: 0x4b, data: wrong_version }).is_err());
    let mut unknown_segment = chunk.data.clone();
    unknown_segment[10] = 255;
    assert!(FNTableRecord::try_from(&ni_file::kontakt::Chunk { id: 0x4b, data: unknown_segment }).is_err());
    assert!(FNTableRecord::try_from(&ni_file::kontakt::Chunk { id: 0x3d, data: chunk.data }).is_err());
}
#[test]
fn zone_list_keeps_group_id() {
    let mut bytes = 1u32.to_le_bytes().to_vec();
    bytes.extend(17u32.to_le_bytes());
    bytes.extend([1, 0x99, 0]);
    for _ in 0..3 {
        bytes.extend(0u32.to_le_bytes());
    }
    let list = ZoneList::read(Cursor::new(bytes)).unwrap();
    assert_eq!(list.group_ids, [17]);
    assert_eq!(list.zones().len(), 1);
}
#[test]
fn truncated_chunk_is_an_error_not_partial_success() {
    assert!(KontaktChunks::read(Cursor::new([0x28, 0, 16, 0, 0, 0, 1, 2])).is_err());
    assert!(KontaktChunks::read(Cursor::new([0x28])).is_err());
    assert!(KontaktChunks::read(Cursor::new(Vec::<u8>::new()))
        .unwrap()
        .0
        .is_empty());
}
#[test]
fn clear_nkx_member_and_bad_sibling_are_independent() {
    let mut b = Vec::new();
    b.extend(0x5e70ac54u32.to_le_bytes());
    b.extend(0x110u16.to_le_bytes());
    b.extend([0; 8]);
    b.extend(2u32.to_le_bytes());
    b.extend([0; 4]);
    for (name, offset) in [("piano.ncw", 128u32), ("broken.ncw", 200u32)] {
        let name: Vec<u8> = name
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        b.extend(((name.len() + 8) as u16).to_le_bytes());
        b.extend((offset ^ 0x1f4e0c8d).to_le_bytes());
        b.extend(2u16.to_le_bytes());
        b.extend(name);
    }
    b.resize(128, 0);
    b.extend(0x4916e63cu32.to_le_bytes());
    b.extend(0x110u16.to_le_bytes());
    b.extend([0; 13]);
    b.extend(4u32.to_le_bytes());
    b.extend([0; 4]);
    b.extend(*b"test");
    b.resize(231, 0);
    let archive = Archive::read(Cursor::new(&b)).unwrap();
    assert!(archive.issues.iter().any(|s|s.contains("1 archive members have missing/corrupt headers")));
    assert_eq!(
        archive.read_entry(Cursor::new(&b), "PIANO.NCW").unwrap(),
        b"test"
    );
    assert!(archive.read_entry(Cursor::new(&b), "broken.ncw").is_err());
    assert_eq!(archive.find("broken.ncw").unwrap().issue,Some("Zero-filled NKX member header"));
    // A truncated or unsupported sibling must not hide intact members.
    for (magic,version,size,length,issue) in [
        (0x4916e63cu32,0x110u16,40u32,231,"Truncated NKX member payload"),
        (0x4916e63c,0x999,4,231,"Unsupported NKX member version"),
        (0x4916e63c,0x110,4,221,"Truncated NKX member header"),
    ] {
        let mut damaged=b.clone();damaged[200..204].copy_from_slice(&magic.to_le_bytes());damaged[204..206].copy_from_slice(&version.to_le_bytes());damaged[219..223].copy_from_slice(&size.to_le_bytes());damaged.truncate(length);
        let a=Archive::read(Cursor::new(&damaged)).unwrap();assert_eq!(a.find("broken.ncw").unwrap().issue,Some(issue));assert_eq!(a.read_entry(Cursor::new(&damaged),"piano.ncw").unwrap(),b"test");assert!(a.read_entry(Cursor::new(&damaged),"broken.ncw").is_err());
    }
    b.truncate(10);
    assert!(Archive::read(Cursor::new(b)).is_err());
}

/// A stand-in keystream: ni-file only plumbs a caller-supplied key through.
struct XorKey(u8);
impl ni_file::nis::LibraryKey for XorKey {
    fn apply_at(&self, offset: u64, bytes: &mut [u8]) {
        for (i, b) in bytes.iter_mut().enumerate() {
            *b ^= self.0.wrapping_add((offset + i as u64) as u8);
        }
    }
}

#[test]
fn invalid_compression() {
    use ni_file::nis::SubtreeItem;
    let mut bad = 1u32.to_le_bytes().to_vec();
    bad.push(1);
    bad.extend(100u32.to_le_bytes());
    bad.extend(1u32.to_le_bytes());
    bad.push(0xe0);
    assert!(SubtreeItem::read(Cursor::new(&bad)).is_err());
    bad[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(SubtreeItem::read(Cursor::new(&bad)).is_err());
}

#[test]
fn encrypted_subtree_requires_the_matching_key() {
    use ni_file::nis::{LibraryKey, SubtreeItem};
    let key = XorKey(0x5a);
    let mut payload = vec![3, b't', b'e', b's', b't'];
    key.apply(&mut payload);
    let mut frame = 1u32.to_le_bytes().to_vec();
    frame.push(1);
    frame.extend(4u32.to_le_bytes());
    frame.extend(5u32.to_le_bytes());
    frame.extend(payload);
    assert_eq!(
        SubtreeItem::read_with_key(Cursor::new(&frame), Some(&key))
            .unwrap()
            .inner_data,
        b"test"
    );
    assert!(SubtreeItem::read_with_key(
        Cursor::new(&frame),
        Some(&XorKey(0x33))
    )
    .is_err());
}

#[test]
fn plain_offsets_and_encrypted_members() {
    use ni_file::nis::LibraryKey;
    for encrypted in [false, true] {
        let key = XorKey(7);
        let mut payload = b"sample".to_vec();
        if encrypted {
            key.apply(&mut payload);
        }
        let mut b = Vec::new();
        b.extend(0x5e70ac54u32.to_le_bytes());
        b.extend(0x110u16.to_le_bytes());
        b.extend([0; 8]);
        b.extend(1u32.to_le_bytes());
        b.extend([0; 4]);
        b.extend(12u16.to_le_bytes());
        b.extend((if encrypted { 64u32 ^ 0x1f4e0c8d } else { 64u32 }).to_le_bytes());
        b.extend((if encrypted { 2u16 } else { 0u16 }).to_le_bytes());
        b.extend([b'x', 0, 0, 0]);
        b.resize(64, 0);
        b.extend(
            (if encrypted {
                0x16ccf80au32
            } else {
                0x4916e63cu32
            })
            .to_le_bytes(),
        );
        b.extend(0x110u16.to_le_bytes());
        b.extend([0; 4]);
        b.extend((if encrypted { 0x100u32 } else { 0xffu32 }).to_le_bytes());
        b.extend([0; 5]);
        b.extend(6u32.to_le_bytes());
        b.extend(vec![0; if encrypted { 8 } else { 4 }]);
        b.extend(payload);
        for a in [
            Archive::read(Cursor::new(&b)).unwrap(),
            Archive::read_index(Cursor::new(&b)).unwrap(),
        ] {
            assert_eq!(
                a.read_entry_with_key(Cursor::new(&b), "x", Some(&key))
                    .unwrap(),
                b"sample"
            );
            if encrypted {
                assert!(a.read_entry(Cursor::new(&b), "x").is_err());
            } else {
                assert_eq!(a.read_entry(Cursor::new(&b), "x").unwrap(), b"sample");
            }
        }
        b.pop();
        let a = Archive::read_index(Cursor::new(&b)).unwrap();
        assert!(a.read_entry_with_key(Cursor::new(&b), "x", Some(&key)).is_err());
    }
}

#[test]
fn nkr_picture_resource_uses_its_22_byte_header() {
    let mut b=Vec::new();
    b.extend(0x5e70ac54u32.to_le_bytes());b.extend(0x111u16.to_le_bytes());b.extend([0;8]);b.extend(1u32.to_le_bytes());b.extend([0;4]);
    let name:Vec<u8>="wallpaper.png".encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect();
    b.extend(((name.len()+8) as u16).to_le_bytes());b.extend(96u32.to_le_bytes());b.extend(4u16.to_le_bytes());b.extend(name);
    b.resize(96,0);b.extend(0x2ae905fau32.to_le_bytes());b.extend(0x111u16.to_le_bytes());b.extend([0;4]);b.extend(255u32.to_le_bytes());b.extend(4u32.to_le_bytes());b.extend([0;4]);b.extend(*b"test");
    let archive=Archive::read(Cursor::new(&b)).unwrap();
    assert_eq!(archive.read_entry(Cursor::new(&b),"wallpaper.png").unwrap(),b"test");
    b.pop();let damaged=Archive::read(Cursor::new(&b)).unwrap();assert_eq!(damaged.find("wallpaper.png").unwrap().issue,Some("Truncated NKX member payload"));assert!(damaged.read_entry(Cursor::new(&b),"wallpaper.png").is_err());
}

#[test]
fn app_specific_missing_subtree_is_an_error() {
    use ni_file::nis::{AppSpecificProperties,ItemData,ItemDataHeader};
    let item=ItemData{header:ItemDataHeader{length:24,domain_id:*b"NISD",item_id:0x75,version:1},inner:None,data:1u32.to_le_bytes().to_vec()};
    assert!(AppSpecificProperties::try_from(&item).is_err());
}

#[test]
fn malformed_start_criteria_returns_error() {
    use ni_file::kontakt::objects::StartCriteriaList;
    for data in [vec![1,1,0x70,0],vec![1,0,0x71,0],vec![1,0,0x70,0]] {
        assert!(StartCriteriaList::read(Cursor::new(data)).is_err());
    }
    assert!(StartCriteriaList::read(Cursor::new([0])).unwrap().items.is_empty());
}

#[test]
fn group_conditions_are_found_by_id() {
    use ni_file::kontakt::{Chunk, StructuredObject, objects::Group};
    let mut group=Group(StructuredObject{version:0x90,public_data:vec![0;64],private_data:vec![],children:vec![Chunk{id:0x38,data:vec![0]}]});
    assert!(group.params().unwrap().start_criteria.items.is_empty());
    group.0.children.clear();assert!(group.params().is_err());
}

/// Synthetic modulation records built from the layout in `audits/MODULATION.md`.
mod modulation {
    use ni_file::kontakt::{
        Chunk,
        objects::{
            EnvelopeAhdsr, ExternalMod, ExternalModArray32, InternalMod, InternalModArray16,
            ModSource, Modulator, ShaperCurve,
        },
    };

    fn name(out: &mut Vec<u8>, text: &str) {
        out.extend((text.len() as u32).to_le_bytes());
        out.extend(text.as_bytes());
    }

    fn target_header(out: &mut Vec<u8>, param: &str, slot: Option<u8>, invert: bool) {
        name(out, param);
        out.extend(0.5f32.to_le_bytes());
        out.extend((-1i16).to_le_bytes());
        out.push(0x10);
        out.extend(250u16.to_le_bytes());
        name(out, "<none>");
        out.extend(slot);
        out.push(invert.into());
    }

    fn structured(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
        let mut data = vec![1];
        data.extend(version.to_le_bytes());
        for part in [private, public, children] {
            data.extend((part.len() as u32).to_le_bytes());
            data.extend(part);
        }
        Chunk { id, data }
    }

    fn chunk_bytes(chunk: &Chunk) -> Vec<u8> {
        let mut out = chunk.id.to_le_bytes().to_vec();
        out.extend((chunk.data.len() as u32).to_le_bytes());
        out.extend(&chunk.data);
        out
    }

    fn velocity_to_volume() -> Vec<u8> {
        let mut data = 1u32.to_le_bytes().to_vec();
        target_header(&mut data, "volume", None, false);
        data.extend([2, 1, 2]);
        for value in [0.0f32, 0.0, 0.0, 1.0, 1.0, 0.0] {
            data.extend(value.to_le_bytes());
        }
        name(&mut data, "VEL_VOLUME");
        data.extend(1u32.to_le_bytes());
        data.extend(6u32.to_le_bytes());
        data.extend([0; 4]);
        data.extend(7u32.to_le_bytes());
        data
    }

    fn cc_to_module() -> Vec<u8> {
        let mut data = 1u32.to_le_bytes().to_vec();
        target_header(&mut data, "eqGain1", Some(2), true);
        data.extend([1, 0]);
        for step in 0..128 {
            data.extend((step as f32 / 127.0).to_le_bytes());
        }
        name(&mut data, "CC_EQ");
        data.extend(1u32.to_le_bytes());
        data.extend(4u32.to_le_bytes());
        data.push(11);
        data.extend([0x7f, 0, 0, 0]);
        data.extend(9u32.to_le_bytes());
        data
    }

    fn ahdsr(sustain: f32) -> Chunk {
        let mut data = vec![0];
        data.extend(0x11u16.to_le_bytes());
        for value in [0.25f32, 10.0, 500.0, 0.0, 300.0, sustain] {
            data.extend(value.to_le_bytes());
        }
        data.push(0);
        data.extend([0; 52]);
        Chunk { id: 0x3F, data }
    }

    #[test]
    fn external_assignments_decode_source_target_and_shaper() {
        let velocity = structured(0x0C, 0x102, &velocity_to_volume(), &[], &[]);
        let cc = structured(0x0C, 0x102, &cc_to_module(), &[], &[]);
        let mut slots = vec![0, 1];
        slots.extend(chunk_bytes(&velocity));
        slots.push(1);
        slots.extend(chunk_bytes(&cc));
        slots.resize(slots.len() + 29, 0);
        let array =
            ExternalModArray32::try_from(&structured(0x3C, 0x12, &[], &slots, &[])).unwrap();

        let items = array.slots().unwrap();
        let indices: Vec<_> = items.iter().map(|(slot, _)| *slot).collect();
        assert_eq!(indices, [1, 2]);

        let velocity = items[0].1.params().unwrap();
        assert_eq!(velocity.name, "VEL_VOLUME");
        assert_eq!(velocity.source, ModSource::Velocity);
        let target = &velocity.targets[0];
        assert_eq!(
            (target.param.as_str(), target.slot, target.invert),
            ("volume", None, false)
        );
        assert_eq!((target.intensity, target.lag_ms), (0.5, 250));
        let shaper = target.shaper.as_ref().unwrap();
        assert!(shaper.enabled);
        assert!(matches!(&shaper.curve, ShaperCurve::Breakpoints(points) if points.len() == 2));

        let cc = items[1].1.params().unwrap();
        assert_eq!(cc.source, ModSource::MidiCc(11));
        let target = &cc.targets[0];
        assert_eq!(
            (target.param.as_str(), target.slot, target.invert),
            ("eqGain1", Some(2), true)
        );
        let shaper = target.shaper.as_ref().unwrap();
        assert!(!shaper.enabled);
        assert!(matches!(&shaper.curve, ShaperCurve::Table(table) if table.len() == 128));
    }

    #[test]
    fn unassigned_external_mod_has_a_shorter_tail() {
        let mut data = 1u32.to_le_bytes().to_vec();
        target_header(&mut data, "volume", None, false);
        data.push(0);
        name(&mut data, "<none>");
        data.extend(2u32.to_le_bytes());
        data.extend([0, 0]);
        data.extend(33u32.to_le_bytes());
        let chunk = structured(0x0C, 0x101, &data, &[], &[]);
        let params = ExternalMod::try_from(&chunk).unwrap().params().unwrap();
        assert_eq!(params.source, ModSource::Unassigned);
        assert_eq!(params.unknown_id, 33);
    }

    #[test]
    fn malformed_external_assignments_are_errors() {
        let valid = velocity_to_volume();
        let mut trailing = valid.clone();
        trailing.push(0);
        let mut truncated = valid.clone();
        truncated.truncate(valid.len() - 3);
        // count, "volume", intensity, i16, flags, lag, "<none>"
        let invert_offset = 4 + (4 + 6) + 4 + 2 + 1 + 2 + (4 + 6);
        let mut bad_invert = valid.clone();
        bad_invert[invert_offset] = 7;
        for data in [trailing, truncated, bad_invert, vec![0; 4]] {
            let chunk = structured(0x0C, 0x102, &data, &[], &[]);
            assert!(ExternalMod::try_from(&chunk).unwrap().params().is_err());
        }
        let unknown_version = structured(0x0C, 0x0FF, &valid, &[], &[]);
        assert!(
            ExternalMod::try_from(&unknown_version)
                .unwrap()
                .params()
                .is_err()
        );
    }

    #[test]
    fn internal_modulator_reads_ahdsr_envelope() {
        let mut private = 1u32.to_le_bytes().to_vec();
        target_header(&mut private, "volume", None, false);
        private.push(0);
        private.extend([0, 0, 1, 0]);
        private.extend(0u32.to_le_bytes());
        name(&mut private, "ENV_AHDSR");
        private.extend(2u32.to_le_bytes());
        let envelope = chunk_bytes(&ahdsr(0.5));
        let wrapper = structured(0x07, 0x90, &[], &0u32.to_le_bytes(), &envelope);
        let modulator = structured(0x0D, 0x81, &private, &[], &chunk_bytes(&wrapper));
        let mut slots = vec![1];
        slots.extend(chunk_bytes(&modulator));
        slots.resize(slots.len() + 15, 0);
        let array =
            InternalModArray16::try_from(&structured(0x3B, 0x10, &[], &slots, &[])).unwrap();

        let (slot, modulator) = array.slots().unwrap().pop().unwrap();
        assert_eq!(slot, 0);
        let params = modulator.params().unwrap();
        assert_eq!(params.name, "ENV_AHDSR");
        assert_eq!(params.targets[0].param, "volume");
        let Modulator::Ahdsr(env) = params.modulator else {
            panic!("expected an AHDSR modulator");
        };
        let fields = (
            env.attack_curve,
            env.attack_ms,
            env.decay_ms,
            env.hold_ms,
            env.release_ms,
            env.sustain,
        );
        assert_eq!(fields, (0.25, 10.0, 500.0, 0.0, 300.0, 0.5));

        let wrong_id = structured(0x0C, 0x81, &private, &[], &[]);
        assert!(InternalMod::try_from(&wrong_id).is_err());
    }

    #[test]
    fn shaper_curves_interpolate_linearly() {
        use ni_file::kontakt::objects::Breakpoint;
        let table = ShaperCurve::Table((0..128).map(|i| 1.0 - i as f32 / 127.0).collect());
        assert_eq!(table.evaluate(0.0), 1.0);
        assert!((table.evaluate(0.5) - 0.5).abs() < 1e-6);
        assert_eq!(table.evaluate(2.0), 0.0);
        let point = |x, y| Breakpoint { x, y, curve: 0.0 };
        let knee =
            ShaperCurve::Breakpoints(vec![point(0.0, 0.0), point(0.5, 1.0), point(1.0, 1.0)]);
        assert_eq!(knee.evaluate(0.25), 0.5);
        assert_eq!(knee.evaluate(0.75), 1.0);
        assert_eq!(ShaperCurve::Breakpoints(vec![]).evaluate(0.3), 0.3);
    }

    #[test]
    fn out_of_range_envelope_is_an_error() {
        assert!(EnvelopeAhdsr::try_from(&ahdsr(0.5)).is_ok());
        assert!(EnvelopeAhdsr::try_from(&ahdsr(2.0)).is_err());
        let mut truncated = ahdsr(0.5);
        truncated.data.truncate(20);
        assert!(EnvelopeAhdsr::try_from(&truncated).is_err());
    }
}

#[test]
fn nis_and_raw_chunks_roundtrip_without_losing_opaque_metadata() {
    use ni_file::{NIFile, nis::{ItemContainer, ItemData, ItemDataHeader, ItemHeader, SubtreeItem}};
    use ni_file::kontakt::Chunk;
    fn layer(domain_id: [u8;4], item_id: u32, data: Vec<u8>, inner: Option<ItemData>) -> ItemData {
        ItemData { header: ItemDataHeader { length: 0, domain_id, item_id, version: 1 }, inner: inner.map(Box::new), data }
    }
    fn base() -> ItemData { layer(*b"NISD", 1, vec![1,0,0,0], None) }
    fn item(data: ItemData) -> ItemContainer {
        ItemContainer {
            header: ItemHeader { length: 0, magic: b"hsin".to_vec(), header_flags: 0xa501, reserved: 0x12345678, uuid: (0..16).collect() },
            data, children: vec![], child_headers: vec![], trailing_data: vec![0xc1,0xc2],
        }
    }
    let chunks = KontaktChunks(vec![
        Chunk { id: 0xf123, data: vec![0,1,2,0xff] },
        Chunk { id: 6, data: b"uninterpreted script".to_vec() },
        Chunk { id: 0xf123, data: vec![3,4] },
    ]);
    let mut preset = Vec::new();
    chunks.write(&mut preset).unwrap();
    let mut again = Vec::new();
    KontaktChunks::read(Cursor::new(&preset)).unwrap().write(&mut again).unwrap();
    assert_eq!(again, preset);
    let mut properties = 1u32.to_le_bytes().to_vec();
    properties.extend(0u32.to_le_bytes());
    properties.extend(1u32.to_le_bytes());
    properties.extend((preset.len() as u64).to_le_bytes());
    properties.extend(&preset);
    properties.extend([0x75,0x76,0x77]); // opaque preset property trailer
    let inner = item(layer(*b"NISD", 0x6d, properties, Some(base())));
    let mut inner_bytes = Vec::new();
    inner.write(&mut inner_bytes).unwrap();
    let subtree = SubtreeItem { inner_data: inner_bytes.clone() };
    for (compressed, encrypted) in [(false,false), (true,false), (true,true)] {
        let key = XorKey(0x39);
        let access = encrypted.then_some(&key as &dyn ni_file::nis::LibraryKey);
        let mut encoded_subtree = Vec::new();
        subtree.write_with_key(&mut encoded_subtree, compressed, access).unwrap();
        assert_eq!(SubtreeItem::read_with_key(Cursor::new(&encoded_subtree), access).unwrap().inner_data, inner_bytes);
        let subtree_layer = layer(*b"NISD", 0x73, encoded_subtree.clone(), Some(base()));
        let enc_layer = layer(*b"NISD", 0x74, vec![1,0,0,0, encrypted as u8], Some(subtree_layer));
        let mut root = item(layer(*b"NIK4", 3, vec![0,0], Some(layer(*b"TEST", 0x9876, vec![9,8,7], Some(base())))));
        root.children.push(item(enc_layer));
        // Preserve deliberately noncanonical sibling index/domain/id, not inferred values.
        let descriptor = [0xe9,3,0,0,b'4',b'K',b'I',b'N',0xde,0xad,0xbe,0xef];
        root.child_headers.push(descriptor);
        let mut bytes = Vec::new();
        root.write(&mut bytes).unwrap();
        let mut parsed = ItemContainer::read(Cursor::new(&bytes)).unwrap();
        assert_eq!(parsed.header.reserved, 0x12345678);
        assert_eq!(parsed.child_headers, [descriptor]);
        assert_eq!(parsed.trailing_data, [0xc1,0xc2]);
        let mut roundtrip = Vec::new();
        NIFile::NISoundContainer(parsed.clone()).write(&mut roundtrip).unwrap();
        assert_eq!(roundtrip, bytes);
        assert_eq!(NIFile::NISoundContainer(parsed.clone()).inner_preset_with_key(access).unwrap(), preset);
        if encrypted { assert!(NIFile::NISoundContainer(parsed.clone()).inner_preset().is_err()); }
        // Changing opaque metadata recomputes nested lengths, preserving all children.
        parsed.data.data.extend([0xf1; 21]);
        let mut edited = Vec::new();
        parsed.write(&mut edited).unwrap();
        let reparsed = ItemContainer::read(Cursor::new(&edited)).unwrap();
        assert_eq!(reparsed.header.length, bytes.len() as u64 + 21);
        assert_eq!(reparsed.child_headers, [descriptor]);
        assert_eq!(NIFile::NISoundContainer(reparsed).inner_preset_with_key(access).unwrap(), preset);
        parsed.child_headers.clear();
        assert!(parsed.write(&mut Vec::new()).is_err());
    }
    let mut output = Vec::new();
    assert!(subtree.write_with_key(&mut output, false, Some(&XorKey(0x39))).is_err());
    assert!(output.is_empty());
    assert!(NIFile::NICompressedWave.write(&mut output).is_err());
    assert!(output.is_empty());
    let invalid = SubtreeItem { inner_data: vec![] };
    assert!(invalid.write_with_key(&mut output, true, None).is_err());
    assert!(output.is_empty());
    // Declared lengths outside the C codec range fail before allocating output.
    for (expanded, packed) in [(u32::MAX, 1u32), (1, u32::MAX)] {
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.push(1);
        bytes.extend(expanded.to_le_bytes());
        bytes.extend(packed.to_le_bytes());
        assert!(SubtreeItem::read(Cursor::new(bytes)).is_err());
    }
}

#[test]
fn envelope_records_preserve_metadata_and_validate_edits() {
    use ni_file::kontakt::{
        Chunk,
        objects::{EnvelopeAhdsr, EnvelopeFlex},
    };
    let mut bytes = vec![0, 0x11, 0];
    for value in [-0.0f32, 10.0, 500.0, 0.0, 300.0, 0.5] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.push(0x93); // Opaque mode bits are not interpreted by the writer.
    bytes.extend(0..52);
    let original = Chunk {
        id: 0x3f,
        data: bytes,
    };
    let mut envelope = EnvelopeAhdsr::try_from(&original).unwrap();
    let mut framed = Vec::new();
    original.write(&mut framed).unwrap();
    let mut rewritten = Vec::new();
    envelope.write(&mut rewritten).unwrap();
    assert_eq!(rewritten, framed);
    for end in 0..original.data.len() {
        assert!(
            EnvelopeAhdsr::try_from(&Chunk {
                id: 0x3f,
                data: original.data[..end].to_vec()
            })
            .is_err()
        );
    }
    envelope.attack_ms = 1234.5;
    envelope.sustain = 0.25;
    envelope.unknown_tail.extend([0xde, 0xad, 0xbe]);
    assert_eq!(
        EnvelopeAhdsr::try_from(&envelope.to_chunk().unwrap()).unwrap(),
        envelope
    );
    envelope.attack_ms = f32::INFINITY;
    assert!(envelope.to_chunk().is_err());
    envelope.attack_ms = 0.0;
    envelope.unknown_tail.clear();
    let mut output = Vec::new();
    assert!(envelope.write(&mut output).is_err());
    assert!(output.is_empty());

    for (version, tail) in [(0x11u16, 13), (0x12, 15)] {
        let mut data = vec![0];
        data.extend(version.to_le_bytes());
        data.extend(1u32.to_le_bytes()); // Last point index.
        data.extend(0u32.to_le_bytes()); // Opaque index.
        data.extend(1u32.to_le_bytes()); // Sustain point.
        for value in [-0.0f32, 0.25, 0.5, 800.0, 1.0, 0.75] {
            data.extend(value.to_le_bytes());
        }
        data.extend((0..tail).map(|i| 0xf0 | i as u8));
        let original = Chunk { id: 0x40, data };
        let mut envelope = EnvelopeFlex::try_from(&original).unwrap();
        let mut expected = Vec::new();
        original.write(&mut expected).unwrap();
        let mut rewritten = Vec::new();
        envelope.write(&mut rewritten).unwrap();
        assert_eq!(rewritten, expected);
        for end in 0..original.data.len() {
            assert!(
                EnvelopeFlex::try_from(&Chunk {
                    id: 0x40,
                    data: original.data[..end].to_vec()
                })
                .is_err()
            );
        }
        envelope.points[1].time_ms = 90.5;
        envelope.points[1].level = 0.6;
        envelope.sustain = 0;
        assert_eq!(
            EnvelopeFlex::try_from(&envelope.to_chunk().unwrap()).unwrap(),
            envelope
        );
        envelope.sustain = 2;
        assert!(envelope.to_chunk().is_err());
        envelope.sustain = 0;
        envelope.points[0].curve = f32::NAN;
        assert!(envelope.to_chunk().is_err());
        envelope.points[0].curve = 0.5;
        envelope.unknown_tail.clear();
        let mut output = Vec::new();
        assert!(envelope.write(&mut output).is_err());
        assert!(output.is_empty());
    }
}

#[test]
fn nested_nis_lengths_cannot_consume_bytes_outside_their_declared_body() {
    use ni_file::nis::{ItemContainer, SubtreeItem};
    fn header(length: u64) -> Vec<u8> {
        let mut out = length.to_le_bytes().to_vec();
        out.extend(1u32.to_le_bytes());out.extend(b"hsin");out.extend([0;24]);out
    }
    fn data_header(length: u64, id: u32) -> Vec<u8> {
        let mut out = length.to_le_bytes().to_vec();
        out.extend(b"DSIN");out.extend(id.to_le_bytes());out.extend(1u32.to_le_bytes());out
    }
    let empty_children = [1u32.to_le_bytes(), 0u32.to_le_bytes()].concat();
    let mut invalid_layer = header(88);
    invalid_layer.extend(data_header(40, 0x9876));
    invalid_layer.extend(data_header(21, 1)); // its enclosing layer permits only the 20-byte header
    invalid_layer.extend(&empty_children);
    assert!(ItemContainer::read(Cursor::new(&invalid_layer)).is_err());
    assert!(SubtreeItem { inner_data: invalid_layer }.item().is_err());

    let mut child = header(72); // only 68 child bytes are actually inside the parent
    child.extend(data_header(20, 1));child.extend(&empty_children);
    let mut parent = header(148);
    parent.extend(data_header(20, 1));parent.extend(1u32.to_le_bytes());parent.extend(1u32.to_le_bytes());
    parent.extend([0;12]);parent.extend(child);
    parent.extend([0xaa;4]); // bytes after the declared parent must not satisfy the child
    assert!(ItemContainer::read(Cursor::new(&parent)).is_err());
    assert!(SubtreeItem { inner_data: parent }.item().is_err());

    let mut valid = header(68);valid.extend(data_header(20, 1));valid.extend(empty_children);
    let mut cursor = Cursor::new(&valid);
    ItemContainer::read(&mut cursor).unwrap();assert_eq!(cursor.position(), 68);
    for length in 0..valid.len() {
        assert!(SubtreeItem { inner_data: valid[..length].to_vec() }.item().is_err());
    }
}

#[test]
fn generic_nis_read_reuses_detection_and_preserves_stream_consumption() {
    use ni_file::NIFile;
    use std::io::{Read, Seek, SeekFrom};
    struct Counted { bytes: Cursor<Vec<u8>>, read: usize }
    impl Read for Counted {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            let n = self.bytes.read(out)?;self.read += n;Ok(n)
        }
    }
    impl Seek for Counted {
        fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> { self.bytes.seek(from) }
    }
    let mut bytes = 68u64.to_le_bytes().to_vec();
    bytes.extend(1u32.to_le_bytes());bytes.extend(b"hsin");bytes.extend([0;24]);
    bytes.extend(20u64.to_le_bytes());bytes.extend(b"DSIN");bytes.extend(1u32.to_le_bytes());bytes.extend(1u32.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());bytes.extend(0u32.to_le_bytes());
    let mut reader = Counted { bytes: Cursor::new(bytes.clone()), read: 0 };
    assert!(matches!(NIFile::read(&mut reader).unwrap(), NIFile::NISoundContainer(_)));
    assert_eq!(reader.read, bytes.len() + 4); // one signature probe and one complete parse
    assert_eq!(reader.bytes.position(), bytes.len() as u64);
    bytes[60..64].copy_from_slice(&2u32.to_le_bytes()); // corrupt child-list version
    assert!(NIFile::read(Cursor::new(&bytes)).is_err());
}
