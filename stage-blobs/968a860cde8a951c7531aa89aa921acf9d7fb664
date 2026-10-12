use sampler_kontakt::{ErrorKind, Nks42};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn container(compressed: &[u8], expanded: usize) -> Vec<u8> {
    let mut bytes = vec![0; 222];
    bytes[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    bytes[4..8].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    bytes[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    bytes[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    bytes[186..190].copy_from_slice(&(expanded as u32).to_le_bytes());
    bytes.extend(compressed);
    bytes.extend(0xb00ee1aeu32.to_le_bytes());
    bytes.extend([1, 1, 12, 0]);
    bytes.extend(b"unmodeled metadata");
    bytes
}

#[test]
fn pinned_reference_streams_decode_levels_lengths_overlap_and_far_distances() {
    let mut state = 0x12345678u32;
    let mut expected = Vec::new();
    for _ in 0..12000 {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        expected.push(state as u8);
    }
    expected.extend_from_within(..1024);
    for _ in 0..512 {
        expected.extend(b"abcd");
    }
    expected.extend([b'z'; 1024]);
    for encoded in [
        include_bytes!("fixtures/fastlz-level1.bin").as_slice(),
        include_bytes!("fixtures/fastlz-level2.bin").as_slice(),
    ] {
        let bytes = container(encoded, expected.len());
        let nks = Nks42::parse(&bytes, bytes.len()).unwrap();
        assert_eq!(nks.header.data(), &bytes[..222]);
        assert_eq!(nks.compressed().offset(), 222);
        assert_eq!(&nks.metadata.data()[8..], b"unmodeled metadata");
        assert_eq!(nks.expand(expected.len()).unwrap(), expected);
        support::without_heap(|| {
            assert_eq!(
                nks.expand(expected.len() - 1).unwrap_err().kind,
                ErrorKind::Limit
            );
        });
    }
    // Independently authored dictionary overlap, and the level-1 max extension.
    for (encoded, expected) in [
        (vec![0, b'a', 0x20, 0], b"aaaa".to_vec()),
        (vec![1, b'a', b'b', 0x60, 1], b"abababa".to_vec()),
        (vec![0, b'a', 0xe0, 255, 0], vec![b'a'; 265]),
        (vec![0x20, b'a', 0xe0, 255, 255, 1, 0], vec![b'a'; 521]),
    ] {
        let bytes = container(&encoded, expected.len());
        assert_eq!(
            Nks42::parse(&bytes, bytes.len())
                .unwrap()
                .expand(expected.len())
                .unwrap(),
            expected
        );
    }
}

#[test]
fn corrupt_streams_and_false_expansion_claims_fail_before_allocation() {
    for encoded in [
        vec![],
        vec![0],
        vec![0, b'a', 0x20],
        vec![0, b'a', 0xe0],
        vec![0, b'a', 0xe0, 255],
        vec![0x20, b'a', 0xe0, 255],
        vec![0x20, b'a', 0x3f, 255],
        vec![0x20, b'a', 0x3f, 255, 0],
        vec![0x20, b'a', 0x3f, 255, 0, 0],
        vec![0, b'a', 0x20, 1],
        vec![0x40, b'a'],
        vec![0, b'a', 0],
    ] {
        let bytes = container(&encoded, 65536);
        support::without_heap(|| {
            let nks = Nks42::parse(&bytes, bytes.len()).unwrap();
            assert!(nks.expand(65536).is_err());
        });
    }
    let bytes = container(&[0, b'a'], 1);
    for end in 0..232 {
        // Includes incomplete compressed body and metadata prefix.
        support::without_heap(|| {
            assert!(Nks42::parse(&bytes[..end], bytes.len()).is_err());
        });
    }
    for (at, value, kind) in [
        (0, 0, ErrorKind::InvalidMagic),
        (8, 0x11, ErrorKind::UnsupportedVersion(0x111)),
        (10, 0, ErrorKind::InvalidMagic),
        (42, 1, ErrorKind::UnsupportedLayout),
    ] {
        let mut bad = bytes.clone();
        bad[at] = value;
        support::without_heap(|| {
            let error = Nks42::parse(&bad, bad.len()).unwrap_err();
            assert_eq!((error.offset, error.kind), (at, kind));
        });
    }
    for expected in [0, 2, 0x7fffffff] {
        let bytes = container(&[0, b'a'], expected);
        support::without_heap(|| {
            let nks = Nks42::parse(&bytes, bytes.len()).unwrap();
            assert_eq!(
                nks.expand(expected).unwrap_err().kind,
                ErrorKind::LengthMismatch
            );
        });
    }
}
