//! Owned serialized fixtures for bounded framing, not Kontakt 8 acceptance or DSP laws.
use ni_file::kontakt::{
    Chunk, KontaktChunks,
    objects::{GroupList, Program},
};
use std::io::Cursor;

fn structured(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(version.to_le_bytes());
    for section in [private, public, children] {
        bytes.extend(u32::try_from(section.len()).unwrap().to_le_bytes());
        bytes.extend(section);
    }
    bytes
}

fn opaque_child() -> Vec<u8> {
    let mut bytes = Vec::new();
    Chunk {
        id: 0xf123,
        data: vec![0, 0xff, 0x80],
    }
    .write(&mut bytes)
    .unwrap();
    bytes
}

fn group_list() -> Chunk {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend(structured(0x90, &[0xff], &[0x80], &opaque_child()));
    data.extend(structured(0x95, &[0xde, 0xad], &[0, 0xff], &[]));
    Chunk { id: 0x33, data }
}

fn assert_raw_roundtrip(chunk: &Chunk) {
    let mut original = Vec::new();
    chunk.write(&mut original).unwrap();
    let parsed = Chunk::read(Cursor::new(&original)).unwrap();
    let mut written = Vec::new();
    parsed.write(&mut written).unwrap();
    assert_eq!(written, original);
    assert_eq!(parsed.data, chunk.data);
}

#[test]
fn program_framing_preserves_old_and_unknown_versions_without_semantic_admission() {
    for version in [0x80, 0x82, 0x90, 0xae, 0xb5, 0xffff] {
        let chunk = Chunk {
            id: 0x28,
            data: structured(version, &[0xff, 0x80], &[0xde, 0xad], &opaque_child()),
        };
        let program = Program::try_from(&chunk).unwrap();
        assert_eq!(program.version(), version);
        assert_eq!(program.0.private_data, [0xff, 0x80]);
        assert_eq!(program.0.public_data, [0xde, 0xad]);
        assert_eq!(program.children()[0].id, 0xf123);
        assert_eq!(program.children()[0].data, [0, 0xff, 0x80]);
        // Opaque public bytes are not made valid by framing success.
        assert!(program.public_record().is_err());
        assert_raw_roundtrip(&chunk);
    }
}

#[test]
fn bounded_program_rejects_envelope_suffix_through_chunk_set_consumer() {
    for version in [0x80, 0xb5, 0xffff] {
        let body = structured(version, &[0xff], &[0x80], &opaque_child());
        let at = body.len();
        let mut data = body;
        data.extend([0xde, 0xad]);
        let chunks = KontaktChunks(vec![Chunk { id: 0x28, data }]);
        let mut serialized = Vec::new();
        chunks.write(&mut serialized).unwrap();
        let parsed = KontaktChunks::read(Cursor::new(&serialized)).unwrap();
        let error = parsed.program().unwrap().unwrap_err().to_string();
        assert!(error.contains(&format!("version 0x{version:04x}")), "{error}");
        assert!(error.contains("2 trailing bytes"), "{error}");
        assert!(error.contains(&format!("body offset {at}")), "{error}");
        assert_raw_roundtrip(parsed.first().unwrap());
    }
}

#[test]
fn program_stream_reader_stops_before_following_record() {
    let body = structured(0x80, &[], &[], &[]);
    let at = body.len();
    let mut stream = body;
    stream.extend(structured(0xb5, &[], &[], &[]));
    let mut reader = Cursor::new(&stream);
    assert_eq!(Program::read(&mut reader).unwrap().version(), 0x80);
    assert_eq!(reader.position(), at as u64);
    assert_eq!(Program::read(&mut reader).unwrap().version(), 0xb5);
    assert_eq!(reader.position(), stream.len() as u64);
}

#[test]
fn group_list_preserves_count_order_sections_and_children() {
    let chunk = group_list();
    let groups = GroupList::try_from(&chunk).unwrap().groups;
    assert_eq!(groups.len(), 2);
    assert_eq!((groups[0].0.version, groups[1].0.version), (0x90, 0x95));
    assert_eq!(groups[0].0.private_data, [0xff]);
    assert_eq!(groups[1].0.private_data, [0xde, 0xad]);
    assert_eq!(groups[0].0.public_data, [0x80]);
    assert_eq!(groups[1].0.public_data, [0, 0xff]);
    assert_eq!(groups[0].0.children[0].data, [0, 0xff, 0x80]);
    assert!(groups[1].0.children.is_empty());
    assert_raw_roundtrip(&chunk);
}

#[test]
fn bounded_group_list_rejects_uncounted_suffix_including_empty_list() {
    for mut chunk in [
        group_list(),
        Chunk {
            id: 0x33,
            data: vec![0; 4],
        },
    ] {
        let at = chunk.data.len();
        chunk.data.extend([0xff, 0x80]);
        let error = GroupList::try_from(&chunk).unwrap_err().to_string();
        assert!(error.contains("GroupList chunk 0x0033"), "{error}");
        assert!(error.contains("2 trailing bytes"), "{error}");
        assert!(error.contains(&format!("body offset {at}")), "{error}");
        assert_raw_roundtrip(&chunk);
    }
}

#[test]
fn group_list_count_checks_framing_lower_bound_without_overflow() {
    for count in [1u32, 2, u32::MAX] {
        let mut data = count.to_le_bytes().to_vec();
        data.extend([1, 0x90]); // Fewer bytes than even the flag/version header.
        let chunk = Chunk { id: 0x33, data };
        let error = GroupList::try_from(&chunk).unwrap_err().to_string();
        assert!(
            error.contains(&format!("GroupList count {count} at offset 0")),
            "{error}"
        );
        assert!(error.contains("2 available"), "{error}");
        assert_raw_roundtrip(&chunk);
    }
}

#[test]
fn group_list_missing_record_error_identifies_index_and_offset() {
    let mut data = 2u32.to_le_bytes().to_vec();
    data.extend(structured(0x90, &[], &[], &[]));
    let at = data.len();
    let chunk = Chunk { id: 0x33, data };
    let error = GroupList::try_from(&chunk).unwrap_err().to_string();
    assert!(
        error.contains(&format!("GroupList group 1 of 2 at offset {at}")),
        "{error}"
    );
    assert_raw_roundtrip(&chunk);
}

#[test]
fn group_list_stream_reader_leaves_following_bytes_to_caller() {
    let chunk = group_list();
    let at = chunk.data.len();
    let mut data = chunk.data;
    data.extend([0xde, 0xad]);
    let mut reader = Cursor::new(data);
    assert_eq!(GroupList::read(&mut reader).unwrap().groups.len(), 2);
    assert_eq!(reader.position(), at as u64);
}

#[test]
fn bounded_program_and_group_list_reject_every_truncation() {
    for chunk in [
        Chunk {
            id: 0x28,
            data: structured(0xb5, &[0xff], &[0x80], &opaque_child()),
        },
        group_list(),
    ] {
        for end in 0..chunk.data.len() {
            let truncated = Chunk {
                id: chunk.id,
                data: chunk.data[..end].to_vec(),
            };
            let rejected = match chunk.id {
                0x28 => Program::try_from(&truncated).is_err(),
                0x33 => GroupList::try_from(&truncated).is_err(),
                _ => unreachable!(),
            };
            assert!(rejected, "chunk 0x{:04x} truncation {end}", chunk.id);
            assert_raw_roundtrip(&truncated);
        }
    }
}
