//! W6 fixture cases consolidated into W12; no library bytes.
//! Bounded revisions, adjacent compact groups, nullable names and opaque envelope tails.
use ni_file::kontakt::{
    objects::{
        BParamArrayBParFX8, EnvelopeAhdsr, ExternalMod, ExternalModArray32, InternalMod,
        InternalModArray16, Snapshot,
    },
    Chunk, StructuredObject,
};
use std::io::Cursor;

fn structured(id: u16, version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Chunk {
    let mut data = vec![1];
    data.extend(version.to_le_bytes());
    for part in [private, public, children] {
        data.extend((part.len() as u32).to_le_bytes());
        data.extend(part);
    }
    Chunk { id, data }
}
fn name(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend((value.len() as u32).to_le_bytes());
    bytes.extend(value.as_bytes());
}
fn target(private: &mut Vec<u8>, legacy: bool) {
    private.extend(1u32.to_le_bytes());
    name(private, "volume");
    private.extend(0.5f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0);
    private.extend(12u16.to_le_bytes());
    if legacy {
        private.extend(u32::MAX.to_le_bytes());
    } else {
        name(private, "");
    }
    private.extend([1, 0]); // Invert; no shaper.
}
fn roundtrip(chunk: &Chunk) {
    let mut bytes = Vec::new();
    chunk.write(&mut bytes).unwrap();
    let parsed = Chunk::read(Cursor::new(&bytes)).unwrap();
    let mut rewritten = Vec::new();
    parsed.write(&mut rewritten).unwrap();
    assert_eq!(rewritten, bytes);
}

#[test]
fn reader_versions_fixed_arrays_keep_v11_slot_identities() {
    for version in [0x10u16, 0x11, 0x12, 0x13] {
        for (id, count, child_id) in [(0x3a, 8, 0x25), (0x3b, 16, 0x0d), (0x3c, 32, 0x0c)] {
            let child = structured(child_id, 0x80, &[0xde, 0xad], &[], &[]);
            let mut slots = Vec::new();
            if version == 0x13 {
                slots.extend((count as u32).to_le_bytes());
            }
            slots.extend(vec![0; count - 1]);
            slots.push(1);
            child.write(&mut slots).unwrap();
            let mut data = vec![0];
            data.extend(version.to_le_bytes());
            data.extend(&slots);
            let chunk = Chunk { id, data };
            if id == 0x3a {
                let parsed = BParamArrayBParFX8::try_from(&chunk).unwrap();
                assert!(parsed.items[count - 1].is_some());
            } else {
                let obj = StructuredObject::try_from(&chunk).unwrap();
                let parsed = ni_file::kontakt::objects::read_param_slots(&obj, count).unwrap();
                assert_eq!(parsed[0].0, count - 1);
                assert_eq!(parsed[0].1.data, child.data);
                if id == 0x3b {
                    assert_eq!(
                        InternalModArray16::try_from(&chunk)
                            .unwrap()
                            .slots()
                            .unwrap()[0]
                            .0,
                        15
                    );
                } else {
                    assert_eq!(
                        ExternalModArray32::try_from(&chunk)
                            .unwrap()
                            .slots()
                            .unwrap()[0]
                            .0,
                        31
                    );
                }
            }
            roundtrip(&chunk);
            for end in 0..slots.len() {
                let obj = StructuredObject {
                    version,
                    private_data: vec![],
                    public_data: slots[..end].to_vec(),
                    children: vec![],
                };
                assert!(ni_file::kontakt::objects::read_param_slots(&obj, count).is_err());
            }
            let mut damaged = slots.clone();
            damaged[usize::from(version == 0x13) * 4] = 2;
            let obj = StructuredObject {
                version,
                private_data: vec![],
                public_data: damaged,
                children: vec![],
            };
            assert!(ni_file::kontakt::objects::read_param_slots(&obj, count).is_err());
        }
    }
}

#[test]
fn reader_versions_legacy_nullable_names_do_not_shift_assignment_fields() {
    let mut private = Vec::new();
    target(&mut private, true);
    private.extend(u32::MAX.to_le_bytes()); // Nullable legacy assignment name.
    private.extend(2u32.to_le_bytes());
    private.extend([0; 2]);
    private.extend(29u32.to_le_bytes());
    let external = structured(0x0c, 0x100, &private, &[], &[]);
    let parsed = ExternalMod::try_from(&external).unwrap().params().unwrap();
    assert_eq!(parsed.name, "");
    assert_eq!(parsed.targets[0].name, "");
    assert_eq!(
        (
            parsed.targets[0].invert,
            parsed.targets[0].lag_ms,
            parsed.unknown_id
        ),
        (true, 12, 29)
    );
    roundtrip(&external);
    let mut invalid = private.clone();
    // Target invert follows count, parameter, depth, i16, flags, lag and name.
    invalid[27] = 2;
    let error = ExternalMod::try_from(&structured(0x0c, 0x100, &invalid, &[], &[]))
        .unwrap()
        .params()
        .unwrap_err()
        .to_string();
    assert!(error.contains("volume invert"), "{error}");
    let modern = structured(0x0c, 0x101, &private, &[], &[]);
    assert!(ExternalMod::try_from(&modern).unwrap().params().is_err());
    for end in 0..private.len() {
        assert!(
            ExternalMod::try_from(&structured(0x0c, 0x100, &private[..end], &[], &[]))
                .unwrap()
                .params()
                .is_err()
        );
    }
    let mut internal = Vec::new();
    target(&mut internal, true);
    internal.extend([0, 1, 0, 0]);
    internal.extend(19u32.to_le_bytes());
    internal.extend(u32::MAX.to_le_bytes());
    internal.extend(1u32.to_le_bytes());
    let child = Chunk {
        id: 0x55,
        data: vec![0xde, 0xad],
    };
    let mut children = Vec::new();
    child.write(&mut children).unwrap();
    let chunk = structured(0x0d, 0x80, &internal, &[], &children);
    let parsed = InternalMod::try_from(&chunk).unwrap().params().unwrap();
    assert_eq!(parsed.name, "");
    assert_eq!(parsed.unknown_flags, [0, 1, 0, 0]);
    assert_eq!(parsed.unknown_id, 19);
    assert!(parsed.targets[0].invert);
    roundtrip(&chunk);
    assert!(
        InternalMod::try_from(&structured(0x0d, 0x81, &internal, &[], &children))
            .unwrap()
            .params()
            .is_err()
    );
}

#[test]
fn reader_versions_ahdsr_v10_uses_four_float_metadata_tail() {
    let mut data = vec![0, 0x10, 0];
    for value in [-0.25f32, 12., 500., 20., 320., 0.75] {
        data.extend(value.to_le_bytes());
    }
    data.push(1);
    for bits in [0x8000_0000u32, 0xbf80_0000, 0x7fc0_1234, 0x3f00_0000] {
        data.extend(bits.to_le_bytes());
    }
    let chunk = Chunk { id: 0x3f, data };
    let mut parsed = EnvelopeAhdsr::try_from(&chunk).unwrap();
    assert_eq!(
        (
            parsed.attack_curve,
            parsed.attack_ms,
            parsed.hold_ms,
            parsed.sustain
        ),
        (-0.25, 12., 20., 0.75)
    );
    assert_eq!(parsed.unknown_flag, 1);
    assert_eq!(parsed.unknown_tail.len(), 16);
    assert_eq!(parsed.to_chunk().unwrap().data, chunk.data);
    parsed.attack_ms = 40.;
    let edited = parsed.to_chunk().unwrap();
    assert_eq!(edited.data[1], 0x10);
    assert_eq!(&edited.data[27..], &chunk.data[27..]);
    assert_eq!(EnvelopeAhdsr::try_from(&edited).unwrap().attack_ms, 40.);
    for end in 0..chunk.data.len() {
        assert!(EnvelopeAhdsr::try_from(&Chunk {
            id: 0x3f,
            data: chunk.data[..end].to_vec()
        })
        .is_err());
    }
    let mut invalid = Chunk {
        id: chunk.id,
        data: chunk.data.clone(),
    };
    invalid.data[1] = 0x12;
    assert!(EnvelopeAhdsr::try_from(&invalid).is_err());
    invalid = chunk;
    invalid.data.push(0);
    assert!(EnvelopeAhdsr::try_from(&invalid).is_err());
}

fn empty_array(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend([0, 0x11, 0]);
    bytes.extend(vec![0; count]);
}
fn group(version: u16, source_version: u16, mode: u32) -> Vec<u8> {
    let mut data = vec![0];
    data.extend(version.to_le_bytes());
    data.extend([0; 24]);
    empty_array(&mut data, 8);
    data.push(0);
    data.extend(source_version.to_le_bytes());
    data.extend(mode.to_le_bytes());
    // Common source state and the two following group flags.
    data.extend([0; 16]);
    if source_version >= 0x102 {
        data.extend([0; 9]);
    }
    if mode == 5 {
        data.extend([0; 9]);
    }
    empty_array(&mut data, 16);
    empty_array(&mut data, 32);
    if version >= 2 {
        data.push(1);
    }
    if version == 4 {
        data.extend([0x57; 8]);
    }
    data
}
#[test]
fn reader_versions_compact_groups_chain_all_four_versions_and_source_lengths() {
    for version in 1u16..=4 {
        for source_version in [0x100u16, 0x102, 0x104, 0x106] {
            for mode in [3, 5] {
                let raw = group(version, source_version, mode);
                let mut bytes = 2u32.to_le_bytes().to_vec();
                bytes.extend(&raw);
                bytes.extend(&raw);
                let mut saved = Snapshot {
                    groups: Some(Chunk {
                        id: 0x33,
                        data: bytes,
                    }),
                    version: 1,
                    group_count: 2,
                    effect_children: vec![],
                    persistent: vec![vec![]; 5],
                };
                let groups = saved.group_snapshots().unwrap();
                assert_eq!(groups.len(), 2);
                assert_eq!(groups[1].0, 1);
                for (_, decoded) in groups {
                    assert_eq!(decoded.version, version);
                    assert_eq!(
                        decoded.source_data.len(),
                        if source_version >= 0x102 { 32 } else { 23 }
                            + if mode == 5 { 9 } else { 0 }
                    );
                    let mut written = Vec::new();
                    decoded.write(&mut written).unwrap();
                    assert_eq!(written, raw);
                }
                let complete = saved.groups.as_ref().unwrap().data.clone();
                for end in 0..complete.len() {
                    saved.groups.as_mut().unwrap().data = complete[..end].to_vec();
                    assert!(saved.group_snapshots().is_err());
                }
                saved.groups.as_mut().unwrap().data = complete;
                saved.groups.as_mut().unwrap().data[5] = 5;
                assert!(saved.group_snapshots().is_err());
            }
        }
    }
}
