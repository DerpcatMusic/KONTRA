//! Authored records: no commercial payloads, independent fixed byte lengths.
use ni_file::kontakt::{StructuredObject, objects::*};
use std::io::{Cursor, Read};

fn limit(name: &[u16], kill: i16, voices: i32) -> Vec<u8> {
    let mut b = vec![0, 0x60, 0];
    b.extend((name.len() as u32).to_le_bytes());
    for c in name {
        b.extend(c.to_le_bytes());
    }
    b.extend(kill.to_le_bytes());
    b.push(1);
    for v in [voices, -7, -1i32] {
        b.extend(v.to_le_bytes());
    }
    b
}

#[test]
fn sparse_voice_groups_read_every_slot_and_preserve_signed_values() {
    let mut b = limit(&[80], 1, 256);
    let mut mask = [0; 16];
    mask[0] = 1;
    mask[8] = 1;
    mask[15] = 128;
    b.extend(mask);
    for (kill, voices) in [(0, 0), (3, 64), (4, 7)] {
        b.extend(limit(&[65, 66], kill, voices));
    }
    let g = VoiceGroups::read(Cursor::new(&b)).unwrap();
    assert_eq!(g.voice_limit.max_num_voices, 256);
    assert_eq!(g.voice_limit.name, "P");
    assert_eq!(g.groups.len(), 128);
    assert_eq!(g.groups.iter().flatten().count(), 3);
    assert!(g.groups[1].is_none() && g.groups[63].is_none());
    assert_eq!(g.groups[0].as_ref().unwrap().0.max_num_voices, 0);
    assert_eq!(g.groups[64].as_ref().unwrap().0.kill_mode, 3);
    assert_eq!(g.groups[127].as_ref().unwrap().0.ms_fade_time, -7);
    for end in 0..b.len() {
        assert!(VoiceGroups::read(Cursor::new(&b[..end])).is_err());
    }
    let mut trailing = b.clone();
    trailing.push(0);
    assert!(VoiceGroups::read(Cursor::new(trailing)).is_err());
    let mut invalid = b;
    invalid[11] = 2;
    assert!(VoiceGroups::read(Cursor::new(invalid)).is_err());
}

fn source(version: u16, mode: u32) -> Vec<u8> {
    let mut b = vec![0];
    b.extend(version.to_le_bytes());
    b.extend(mode.to_le_bytes());
    b.extend(0.375f32.to_le_bytes());
    b.push(1);
    b.extend(2u32.to_le_bytes());
    b.push(0);
    b.extend(1f32.to_le_bytes());
    if version >= 0x102 {
        b.extend(2f32.to_le_bytes());
        b.extend(3f32.to_le_bytes());
        b.push(1);
    }
    match mode {
        1 | 2 => {
            b.extend(4f32.to_le_bytes());
            b.extend(5f32.to_le_bytes());
            b.push(1);
        }
        4 => {
            b.extend(4f32.to_le_bytes());
            b.extend(5f32.to_le_bytes());
            b.push(1);
            if version >= 0x106 {
                b.push(0);
            }
        }
        5 => {
            b.push(0);
            b.extend(64u32.to_le_bytes());
            b.extend(128u32.to_le_bytes());
        }
        8 => {
            b.push(1);
            if version == 0x100 {
                b.extend(1u32.to_le_bytes());
            } else {
                b.push(0);
            }
            b.extend(4f32.to_le_bytes());
            b.extend(5f32.to_le_bytes());
        }
        9 if version >= 0x103 => {
            for f in [0.25f32, 0.5, 0.125, 0.75] {
                b.extend(f.to_le_bytes());
            }
            b.extend(17u32.to_le_bytes());
            b.extend(3u32.to_le_bytes());
            if version >= 0x104 {
                b.push(0);
                b.extend(0.375f32.to_le_bytes());
            }
            if version >= 0x105 {
                b.extend(0.625f32.to_le_bytes());
                for n in [1u32, 6, 0] {
                    b.extend(n.to_le_bytes());
                }
                b.extend((-12f32).to_le_bytes());
                b.extend(0x7fc01234u32.to_le_bytes());
                b.extend([0xDE; 16]);
            }
        }
        _ => {}
    }
    b
}

#[test]
fn source_versions_and_modes_consume_only_their_bounded_record() {
    for version in 0x100..=0x106 {
        for mode in 0..=9 {
            let b = source(version, mode);
            let common = if version < 0x102 { 21 } else { 30 };
            let extra = match mode {
                1 | 2 | 5 => 9,
                4 => {
                    if version == 0x106 {
                        10
                    } else {
                        9
                    }
                }
                8 => {
                    if version == 0x100 {
                        13
                    } else {
                        10
                    }
                }
                9 => match version {
                    0x103 => 24,
                    0x104 => 29,
                    0x105 | 0x106 => 69,
                    _ => 0,
                },
                _ => 0,
            };
            assert_eq!(b.len(), common + extra, "v{version:x} mode{mode}");
            let mut with_tail = b.clone();
            with_tail.extend([0xFE; 22]);
            let mut r = Cursor::new(with_tail);
            let p = BParSrcMode::read(&mut r).unwrap();
            assert_eq!(
                (p.version, p.mode, p.bytes),
                (version, mode, b.len() as u16)
            );
            assert_eq!(r.position(), b.len() as u64);
            let mut tail = Vec::new();
            r.read_to_end(&mut tail).unwrap();
            assert_eq!(tail, [0xFE; 22]);
            assert_eq!(p.fields[0].offset, 7);
            if version == 0x106 && mode == 9 {
                let w = WavetableSource::read(Cursor::new(&b)).unwrap();
                assert_eq!(w.position, 0.25);
                assert_eq!(w.mod_tune.to_bits(), 0x7fc01234);
                assert_eq!(
                    p.fields
                        .iter()
                        .find(|f| f.name == "position")
                        .unwrap()
                        .offset,
                    30
                );
                let SourceValue::Float(tune) = p
                    .fields
                    .iter()
                    .find(|f| f.name == "mod_tune")
                    .unwrap()
                    .value
                else {
                    panic!("float expected")
                };
                assert_eq!(tune.to_bits(), w.mod_tune.to_bits());
            }
            for end in 0..b.len() {
                assert!(BParSrcMode::read(Cursor::new(&b[..end])).is_err());
            }
        }
    }
    let mut b = source(0x106, 0);
    b[11] = 2;
    assert!(BParSrcMode::read(Cursor::new(b)).is_err());
    assert!(BParSrcMode::read(Cursor::new(source(0x107, 0))).is_err());
    assert!(BParSrcMode::read(Cursor::new(source(0x106, 10))).is_err());
}

#[test]
fn zone_metadata_versions_and_loop_slot_identity_are_retained() {
    for version in [0x95, 0x96, 0x98, 0x99, 0x9a] {
        let mut b = vec![0; 42];
        if version >= 0x9a {
            b.extend([1, 2, 3, 4, 5, 6]);
        }
        for v in [7i32, 3, 48000] {
            b.extend(v.to_le_bytes());
        }
        b.push(2);
        b.extend(48001i32.to_le_bytes());
        b.extend((-1i32).to_le_bytes());
        if version < 0x96 {
            b.extend(123i32.to_le_bytes());
        }
        b.extend(60i32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.push(1);
        b.extend(9i32.to_le_bytes());
        let len = b.len();
        b.extend([0xDE, 0xAD]);
        let p = Zone(StructuredObject {
            version,
            public_data: b.clone(),
            private_data: vec![],
            children: vec![],
        })
        .params()
        .unwrap();
        assert_eq!(
            (p.filename_id, p.sample_rate, p.num_frames, p.root_note),
            (7, 48000, 48001, 60)
        );
        assert_eq!(p.reserved2, (version < 0x96).then_some(123));
        assert_eq!(
            p.filename_prefix,
            (version >= 0x9a).then_some([1, 2, 3, 4, 5, 6])
        );
        assert_eq!(p.unknown_tail, [0xDE, 0xAD]);
        for end in 0..len {
            assert!(
                Zone(StructuredObject {
                    version,
                    public_data: b[..end].to_vec(),
                    private_data: vec![],
                    children: vec![]
                })
                .params()
                .is_err()
            );
        }
    }
    let mut b = vec![0b10000100];
    for count in [0i32, 5] {
        b.extend([0, 0x60, 0]);
        for n in [1i32, 17, 31, count] {
            b.extend(n.to_le_bytes());
        }
        b.push(1);
        b.extend(1.5f32.to_le_bytes());
        b.extend(7i32.to_le_bytes());
    }
    let a = LoopArray::read(Cursor::new(b)).unwrap();
    assert_eq!(a.slots, [2, 7]);
    assert_eq!(a.items[1].loop_count, 5);
}
