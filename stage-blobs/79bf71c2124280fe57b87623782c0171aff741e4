//! Authored records reconstructed from the native serializer contracts; no library payloads.
use ni_file::kontakt::{
    objects::{
        BParamArrayBParFX8, ExternalMod, ExternalModArray32, Group, InternalModArray16, Modulator,
        Zone,
    },
    Chunk, StructuredObject,
};
use std::io::Cursor;

fn object(version: u16, private: &[u8], public: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(version.to_le_bytes());
    bytes.extend((private.len() as u32).to_le_bytes());
    bytes.extend(private);
    bytes.extend((public.len() as u32).to_le_bytes());
    bytes.extend(public);
    bytes.extend(0u32.to_le_bytes());
    bytes
}

fn roundtrip(chunk: &Chunk) {
    let mut original = Vec::new();
    chunk.write(&mut original).unwrap();
    let decoded = Chunk::read(Cursor::new(&original)).unwrap();
    let mut written = Vec::new();
    decoded.write(&mut written).unwrap();
    assert_eq!(written, original);
}

#[test]
fn tester_parameter_array_v11_keeps_holes_and_opaque_slot_words() {
    for (id, capacity, child) in [(0x3a, 8, 0x2a), (0x3b, 16, 0x0d), (0x3c, 32, 0x0c)] {
        let mut bytes = vec![0, 0x11, 0];
        for slot in 0..capacity {
            let present = slot == capacity - 1;
            bytes.push(u8::from(present));
            bytes.extend((0xdead0000u32 + slot).to_le_bytes());
            if present {
                Chunk {
                    id: child,
                    data: object(0x81, &[], &[0xaa]),
                }
                .write(&mut bytes)
                .unwrap();
            }
        }
        let chunk = Chunk { id, data: bytes };
        let slots: Vec<_> = match id {
            0x3a => BParamArrayBParFX8::try_from(&chunk)
                .unwrap()
                .items
                .into_iter()
                .enumerate()
                .filter_map(|(slot, c)| c.map(|_| slot))
                .collect(),
            0x3b => InternalModArray16::try_from(&chunk)
                .unwrap()
                .slots()
                .unwrap()
                .into_iter()
                .map(|(slot, _)| slot)
                .collect(),
            _ => ExternalModArray32::try_from(&chunk)
                .unwrap()
                .slots()
                .unwrap()
                .into_iter()
                .map(|(slot, _)| slot)
                .collect(),
        };
        assert_eq!(slots, [capacity as usize - 1]);
        roundtrip(&chunk);
    }
}

#[test]
fn tester_parameter_array_v11_presence_only_is_bounded_and_inline() {
    for (id, capacity, child) in [(0x3a, 8, 0x2a), (0x3b, 16, 0x0d), (0x3c, 32, 0x0c)] {
        let mut bytes = vec![0, 0x11, 0];
        bytes.extend(vec![0; capacity - 1]);
        bytes.push(1);
        Chunk {
            id: child,
            data: object(0x81, &[], &[0xaa]),
        }
        .write(&mut bytes)
        .unwrap();
        let chunk = Chunk { id, data: bytes };
        let slots: Vec<_> = match id {
            0x3a => BParamArrayBParFX8::try_from(&chunk)
                .unwrap()
                .items
                .into_iter()
                .enumerate()
                .filter_map(|(slot, c)| c.map(|_| slot))
                .collect(),
            0x3b => InternalModArray16::try_from(&chunk)
                .unwrap()
                .slots()
                .unwrap()
                .into_iter()
                .map(|(slot, _)| slot)
                .collect(),
            _ => ExternalModArray32::try_from(&chunk)
                .unwrap()
                .slots()
                .unwrap()
                .into_iter()
                .map(|(slot, _)| slot)
                .collect(),
        };
        assert_eq!(slots, [capacity - 1]);
        roundtrip(&chunk);
        if id == 0x3a {
            let mut inline = chunk.data.clone();
            inline.extend([0xfe; 4]);
            let mut reader = Cursor::new(inline);
            assert_eq!(BParamArrayBParFX8::read(&mut reader, 8).unwrap().len(), 1);
            assert_eq!(reader.position(), chunk.data.len() as u64);
        }
        let mut trailing = Chunk {
            id,
            data: chunk.data.clone(),
        };
        trailing.data.push(0xfe);
        let rejected = match id {
            0x3a => BParamArrayBParFX8::try_from(&trailing).is_err(),
            0x3b => InternalModArray16::try_from(&trailing)
                .and_then(|a| a.slots())
                .is_err(),
            _ => ExternalModArray32::try_from(&trailing)
                .and_then(|a| a.slots())
                .is_err(),
        };
        assert!(rejected);
    }
}

#[test]
fn tester_legacy_modulation_nullable_names_keep_target_alignment() {
    let mut private = 1u32.to_le_bytes().to_vec();
    private.extend(6u32.to_le_bytes());
    private.extend(b"volume");
    private.extend(0.5f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0);
    private.extend(0u16.to_le_bytes());
    private.extend(u32::MAX.to_le_bytes()); // Legacy nullable target name.
    private.extend([1, 0]); // Invert, no shaper.
    private.extend(u32::MAX.to_le_bytes()); // Legacy nullable assignment name.
    private.extend(1u32.to_le_bytes());
    private.extend(6u32.to_le_bytes());
    private.extend([0; 4]);
    private.extend(2u32.to_le_bytes());
    let chunk = Chunk {
        id: 0x0c,
        data: object(0x100, &private, &[]),
    };
    let params = ExternalMod::try_from(&chunk).unwrap().params().unwrap();
    assert!(params.name.is_empty());
    assert!(params.targets[0].name.is_empty());
    assert!(params.targets[0].invert);
    assert_eq!(params.unknown_id, 2);
    roundtrip(&chunk);
    let modern = Chunk {
        id: 0x0c,
        data: object(0x101, &private, &[]),
    };
    assert!(ExternalMod::try_from(&modern).unwrap().params().is_err());
}

#[test]
fn tester_zone_without_sample_stops_at_native_presence_flag() {
    let mut public = vec![0; 42];
    public[16..18].copy_from_slice(&36i16.to_le_bytes());
    public[18..20].copy_from_slice(&81i16.to_le_bytes());
    public[38..42].copy_from_slice(&1f32.to_le_bytes());
    public.extend([1, 0, 0x12, 0x34, 0x56, 0x78]); // Native sample flag clear, opaque word retained.
    let chunk = Chunk {
        id: 0x04,
        data: object(0x9a, &[], &public),
    };
    let zone = Zone(StructuredObject::try_from(&chunk).unwrap());
    let params = zone.params().unwrap();
    assert!(!params.sample_present);
    assert_eq!(params.filename_id, -1);
    assert_eq!(zone.filename_id().unwrap(), -1);
    assert_eq!((params.low_key, params.high_key), (36, 81));
    assert_eq!(zone.0.public_data, public);
    roundtrip(&chunk);
}

#[test]
fn tester_revision_fixes_reject_unknown_versions_and_truncated_records() {
    let mut private = 1u32.to_le_bytes().to_vec();
    private.extend(6u32.to_le_bytes());
    private.extend(b"volume");
    private.extend(0.5f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0);
    private.extend(0u16.to_le_bytes());
    private.extend(u32::MAX.to_le_bytes());
    let invert = private.len();
    private.extend([0, 0]);
    private.extend(u32::MAX.to_le_bytes());
    private.extend(1u32.to_le_bytes());
    private.extend(6u32.to_le_bytes());
    private.extend([0; 4]);
    private.extend(2u32.to_le_bytes());
    for end in 0..private.len() {
        let chunk = Chunk {
            id: 0x0c,
            data: object(0x100, &private[..end], &[]),
        };
        assert!(
            ExternalMod::try_from(&chunk).unwrap().params().is_err(),
            "truncated legacy mod {end}"
        );
    }
    let mut bad = private.clone();
    bad[invert] = 2;
    assert!(ExternalMod::try_from(&Chunk {
        id: 0x0c,
        data: object(0x100, &bad, &[])
    })
    .unwrap()
    .params()
    .is_err());
    for version in [0x0f, 0x14, 0xffff] {
        let array = ExternalModArray32(StructuredObject {
            version,
            private_data: vec![],
            public_data: vec![0; 160],
            children: vec![],
        });
        assert!(array.slots().is_err());
    }
    let mut array = vec![0, 0x11, 0];
    for _ in 0..32 {
        array.push(0);
        array.extend(0xaabbccddu32.to_le_bytes());
    }
    for end in 0..array.len() {
        let chunk = Chunk {
            id: 0x3c,
            data: array[..end].to_vec(),
        };
        assert!(ExternalModArray32::try_from(&chunk)
            .and_then(|a| a.slots())
            .is_err());
    }
    array.push(0);
    assert!(ExternalModArray32::try_from(&Chunk {
        id: 0x3c,
        data: array
    })
    .unwrap()
    .slots()
    .is_err());
    let mut public = vec![0; 48];
    for end in 0..48 {
        let zone = Zone(StructuredObject {
            version: 0x9a,
            private_data: vec![],
            public_data: public[..end].to_vec(),
            children: vec![],
        });
        assert!(zone.params().is_err());
        assert!(zone.filename_id().is_err());
    }
    for version in [0x91, 0x96, 0x9d, 0xffff] {
        assert!(Zone(StructuredObject {
            version,
            private_data: vec![],
            public_data: public.clone(),
            children: vec![]
        })
        .params()
        .is_err());
    }
    public[43] = 1;
    let mut zone = Zone(StructuredObject {
        version: 0x9a,
        private_data: vec![],
        public_data: public,
        children: vec![],
    });
    assert!(
        zone.params().is_err(),
        "sample-present flag requires the sample suffix"
    );
    zone.0.public_data[43] = 2;
    assert!(
        zone.params().is_err(),
        "unknown presence flag cannot become an empty zone"
    );
}

#[test]
fn tester_source_targets_do_not_consume_graphical_shaper_kind_as_boolean() {
    for target in [
        "formantShift",
        "overlap",
        "grainSize",
        "grainSpeed",
        "playDirection",
        "legacyAddIntensity",
    ] {
        let mut private = 1u32.to_le_bytes().to_vec();
        private.extend((target.len() as u32).to_le_bytes());
        private.extend(target.as_bytes());
        private.extend(0.5f32.to_le_bytes());
        private.extend((-1i16).to_le_bytes());
        private.push(0);
        private.extend(0u16.to_le_bytes());
        private.extend(0u32.to_le_bytes());
        private.extend([0, 2, 1, 2]); // Invert false; enabled graphical shaper with two points.
        for f in [0f32, 0., 0., 1., 1., 0.] {
            private.extend(f.to_le_bytes());
        }
        private.extend(0u32.to_le_bytes());
        private.extend(1u32.to_le_bytes());
        private.extend(6u32.to_le_bytes());
        private.extend([0; 4]);
        private.extend(7u32.to_le_bytes());
        let chunk = Chunk {
            id: 0x0c,
            data: object(0x101, &private, &[]),
        };
        let params = ExternalMod::try_from(&chunk).unwrap().params().unwrap();
        assert_eq!(params.targets[0].slot, None);
        assert!(!params.targets[0].invert);
        assert!(params.targets[0].shaper.as_ref().unwrap().enabled);
        assert_eq!(params.unknown_id, 7);
        roundtrip(&chunk);
    }
}

#[test]
fn tester_group_nested_ahdsr_v10_keeps_revision_and_physical_slot() {
    let mut envelope = vec![0, 0x10, 0];
    for value in [0f32, 2., 3., 0., 4., 1.] {
        envelope.extend(value.to_le_bytes());
    }
    envelope.push(0);
    envelope.extend([0xab; 16]);
    let envelope = Chunk {
        id: 0x3f,
        data: envelope,
    };
    let mut child_bytes = Vec::new();
    envelope.write(&mut child_bytes).unwrap();
    let mut wrapper = object(0x90, &[], &0u32.to_le_bytes());
    wrapper.truncate(wrapper.len() - 4);
    wrapper.extend((child_bytes.len() as u32).to_le_bytes());
    wrapper.extend(child_bytes);
    let mut targets = 1u32.to_le_bytes().to_vec();
    targets.extend(6u32.to_le_bytes());
    targets.extend(b"volume");
    targets.extend(1f32.to_le_bytes());
    targets.extend((-1i16).to_le_bytes());
    targets.push(0);
    targets.extend(0u16.to_le_bytes());
    targets.extend(0u32.to_le_bytes());
    targets.extend([0, 0]);
    targets.extend([0, 0, 1, 0]);
    targets.extend(0u32.to_le_bytes());
    targets.extend(0u32.to_le_bytes());
    targets.extend(2u32.to_le_bytes());
    let mut modulator = object(0x81, &targets, &[]);
    let mut child_bytes = Vec::new();
    Chunk {
        id: 7,
        data: wrapper,
    }
    .write(&mut child_bytes)
    .unwrap();
    modulator.truncate(modulator.len() - 4);
    modulator.extend((child_bytes.len() as u32).to_le_bytes());
    modulator.extend(child_bytes);
    let mut array = vec![0, 0x12, 0];
    array.extend([0; 3]);
    array.push(1);
    Chunk {
        id: 0x0d,
        data: modulator,
    }
    .write(&mut array)
    .unwrap();
    array.extend([0; 12]);
    let group = Group(StructuredObject {
        version: 0x95,
        private_data: vec![],
        public_data: vec![],
        children: vec![Chunk {
            id: 0x3b,
            data: array,
        }],
    });
    let assignments = InternalModArray16::try_from(group.0.find_first(0x3b).unwrap())
        .unwrap()
        .slots()
        .unwrap();
    assert_eq!(assignments[0].0, 3);
    let params = assignments[0].1.params().unwrap();
    let Modulator::Ahdsr(decoded) = params.modulator else {
        panic!("wrong envelope kind")
    };
    assert_eq!((decoded.attack_ms, decoded.release_ms), (2., 4.));
    assert_eq!(decoded.to_chunk().unwrap().data, envelope.data);
}
