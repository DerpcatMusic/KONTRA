use sampler_kontakt::{Chunks, ErrorKind, Group, Limits, Loops, Record, Zone};
#[path = "support/chunks.rs"]
mod fixture;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;
use fixture::{chunk, object};

const LIMITS: Limits = Limits {
    bytes: 65536,
    records: 8,
};

fn record_bytes(group: Option<u32>, version: u16, public: &[u8]) -> Vec<u8> {
    let mut list = 1u32.to_le_bytes().to_vec();
    if let Some(group) = group {
        list.extend(group.to_le_bytes());
    }
    list.extend(object(version, &[0xca, 0xfe], public, &[]));
    chunk(if group.is_some() { 0x34 } else { 0x33 }, &list)
}
fn record(bytes: &[u8]) -> Record<'_> {
    Chunks::parse(bytes, LIMITS)
        .unwrap()
        .iter()
        .next()
        .unwrap()
        .records(LIMITS)
        .unwrap()
        .iter()
        .next()
        .unwrap()
}

#[test]
fn source_mapping_preserves_signed_ranges_units_versions_and_opaque_metadata() {
    let mut fields = 2u32.to_le_bytes().to_vec();
    fields.extend([0, 0xd8, b'a', 0]); // Unpaired UTF-16 surrogate stays unchanged.
    for value in [2f32, -0.25, 0.5] {
        fields.extend(value.to_le_bytes());
    }
    fields.extend([1, 0, 1, 1]);
    fields.extend(7i32.to_le_bytes());
    fields.extend((-1i16).to_le_bytes());
    fields.extend((-1i32).to_le_bytes());
    fields.extend(4i32.to_le_bytes());
    fields.extend([0, 1]);
    fields.extend(999i32.to_le_bytes());
    let required = fields.len();
    fields.extend([0xde, 0xad]);
    let bytes = record_bytes(None, 0x95, &fields);
    support::without_heap(|| {
        let group = Group::parse(record(&bytes)).unwrap();
        assert_eq!(group.name.data(), [0, 0xd8, b'a', 0]);
        assert_eq!((group.gain, group.pan, group.tune), (2., -0.25, 0.5));
        assert!(
            group.key_tracking && group.release_trigger && group.release_monophonic && group.soloed
        );
        assert!(!group.reverse && !group.muted);
        assert_eq!(
            (
                group.release_counter,
                group.midi_channel,
                group.voice_group,
                group.amp_split,
                group.interpolation
            ),
            (7, -1, -1, 4, 999)
        );
        assert_eq!(group.extension.data(), [0xde, 0xad]);
        assert_eq!(group.object.private.data(), [0xca, 0xfe]);
    });
    for end in 0..required {
        let bytes = record_bytes(None, 0x95, &fields[..end]);
        support::without_heap(|| {
            assert!(Group::parse(record(&bytes)).is_err());
        });
    }
    for version in [0x95, 0x98, 0x9a] {
        let mut fields = Vec::new();
        for value in [3i32, -2, -1] {
            fields.extend(value.to_le_bytes());
        }
        for value in [1i16, 127, -3, 128, 2, 3, 4, 5, 60] {
            fields.extend(value.to_le_bytes());
        }
        // Preserve even invalid semantics for diagnostics; decoding never authorizes audio.
        for bits in [2.5f32.to_bits(), 1.5f32.to_bits(), 0x7fc01234] {
            fields.extend(bits.to_le_bytes());
        }
        if version == 0x9a {
            fields.extend([9, 8, 7, 6, 5, 4]);
        }
        fields.extend(0x80000042u32.to_le_bytes());
        let required = fields.len();
        fields.extend([0xb0, 0x0b]);
        let bytes = record_bytes(Some(42), version, &fields);
        support::without_heap(|| {
            let zone = Zone::parse(record(&bytes)).unwrap();
            assert_eq!(
                (zone.group, zone.start, zone.end, zone.start_modulation),
                (42, 3, -2, -1)
            );
            assert_eq!(
                (zone.velocity, zone.keys, zone.fades, zone.root_key),
                ([1, 127], [-3, 128], [2, 3, 4, 5], 60)
            );
            assert_eq!(
                (zone.gain, zone.pan, zone.tune.to_bits()),
                (2.5, 1.5, 0x7fc01234)
            );
            assert_eq!(zone.filename_id as u32, 0x80000042);
            assert_eq!(
                zone.filename_prefix.map(|b| b.data()),
                (version == 0x9a).then_some([9, 8, 7, 6, 5, 4].as_slice())
            );
            assert_eq!(zone.metadata.data(), [0xb0, 0x0b]);
        });
        for end in 0..required {
            let bytes = record_bytes(Some(42), version, &fields[..end]);
            support::without_heap(|| {
                assert!(Zone::parse(record(&bytes)).is_err());
            });
        }
    }
    let bytes = record_bytes(Some(42), 0x9b, &[]);
    assert_eq!(
        Zone::parse(record(&bytes)).unwrap_err().kind,
        ErrorKind::UnsupportedVersion(0x9b)
    );
}

#[test]
fn loop_slots_keep_holes_disabled_modes_counts_tuning_and_crossfade_without_fallback() {
    fn fields(mode: i32, count: i32) -> Vec<u8> {
        let mut out = Vec::new();
        for value in [mode, 17, 31, count] {
            out.extend(value.to_le_bytes());
        }
        out.push(1);
        out.extend(1.5f32.to_le_bytes());
        out.extend((-7i32).to_le_bytes());
        out
    }
    let mut loops = vec![0b10000100];
    loops.extend([0, 0x60, 0]);
    loops.extend(fields(0, 0));
    let mut public = fields(123, 5);
    public.extend([0xa5]);
    loops.extend(object(0x60, &[0xb6], &public, &chunk(0xbeef, &[1])));
    let bytes = chunk(0x39, &loops);
    support::without_heap(|| {
        let raw = Chunks::parse(&bytes, LIMITS)
            .unwrap()
            .iter()
            .next()
            .unwrap();
        let loops = Loops::parse(raw, LIMITS).unwrap();
        assert!(loops.slots()[0].is_none() && loops.slots()[3].is_none());
        let disabled = loops.slots()[2].unwrap();
        assert_eq!((disabled.slot, disabled.mode, disabled.count), (2, 0, 0));
        assert!(disabled.object.is_none());
        let active = loops.slots()[7].unwrap();
        assert_eq!(
            (
                active.slot,
                active.mode,
                active.start,
                active.length,
                active.count
            ),
            (7, 123, 17, 31, 5)
        );
        assert!(active.alternating);
        assert_eq!((active.tune, active.crossfade), (1.5, -7));
        assert_eq!(active.extension.data(), [0xa5]);
        let object = active.object.unwrap();
        assert_eq!(object.private.data(), [0xb6]);
        assert_eq!(
            object.children(LIMITS).unwrap().iter().next().unwrap().id,
            0xbeef
        );
        assert_eq!(
            Loops::parse(
                raw,
                Limits {
                    records: 1,
                    ..LIMITS
                }
            )
            .unwrap_err()
            .kind,
            ErrorKind::Limit
        );
    });
    for end in 0..loops.len() {
        let bytes = chunk(0x39, &loops[..end]);
        support::without_heap(|| {
            let raw = Chunks::parse(&bytes, LIMITS)
                .unwrap()
                .iter()
                .next()
                .unwrap();
            assert!(Loops::parse(raw, LIMITS).is_err());
        });
    }
}
