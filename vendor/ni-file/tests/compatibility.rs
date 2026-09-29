use ni_file::{
    kontakt::{
        objects::{LoopArray, ZoneList},
        KontaktChunks,
    },
    nkr::Archive,
};
use std::io::Cursor;
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

#[test]
fn resource_cipher_and_invalid_compression() {
    use ni_file::nis::{LibraryKey, SubtreeItem};
    let key = LibraryKey::new([0; 32], [0; 16]);
    let mut bytes = vec![0; 65568];
    key.apply(&mut bytes);
    assert_eq!(
        &bytes[..32],
        &[
            0xd2, 0x49, 0xf2, 0x9d, 0x7b, 0x04, 0x69, 0xed, 0x56, 0x08, 0xaa, 0xcc, 0x8f, 0x26,
            0xe2, 0xce, 0x60, 0x69, 0x27, 0xdf, 0xce, 0xaa, 0x1d, 0x20, 0xb7, 0xf7, 0x7a, 0xb0,
            0x72, 0x90, 0x7e, 0x8f
        ]
    );
    assert_eq!(&bytes[..32], &bytes[65536..]);
    key.apply(&mut bytes);
    assert!(bytes.iter().all(|b| *b == 0));
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
    let key = LibraryKey::new([0; 32], [0; 16]);
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
        Some(&LibraryKey::new([1; 32], [0; 16]))
    )
    .is_err());
}

#[test]
fn plain_offsets_and_encrypted_members() {
    use ni_file::nis::LibraryKey;
    for encrypted in [false, true] {
        let key = LibraryKey::new([7; 32], [9; 16]);
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
        b.extend(0x100u32.to_le_bytes());
        b.extend([0; 5]);
        b.extend(6u32.to_le_bytes());
        b.extend(vec![0; if encrypted { 8 } else { 4 }]);
        b.extend(payload);
        let a = Archive::read(Cursor::new(&b)).unwrap();
        assert_eq!(
            a.read_entry_with_key(Cursor::new(&b), "x", Some(&key))
                .unwrap(),
            b"sample"
        );
        if encrypted {
            assert!(a.read_entry(Cursor::new(&b), "x").is_err());
        }
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
