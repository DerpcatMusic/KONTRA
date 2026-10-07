use sampler_kontakt::{ErrorKind, Limits, nis::Item};
use std::borrow::Cow;
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

const LIMITS: Limits = Limits {
    bytes: 1 << 20,
    records: 64,
};

#[path = "support/nis.rs"]
mod fixture;
use fixture::{encryption, item, layer};

#[test]
fn layers_children_descriptors_and_opaque_properties_retain_source_ownership() {
    let base = layer(b"NISD", 1, &[1, 2], &[]);
    let unknown = layer(b"TEST", 0xffffffff, &[3, 4], &base);
    let child = item(&unknown, &[]);
    let root = item(
        &layer(b"NISD", 0x76, &[5, 6], &base),
        &[child.clone(), child],
    );
    support::without_heap(|| {
        let decoded = Item::parse(&root, LIMITS).unwrap();
        assert_eq!(decoded.raw().data(), root);
        assert_eq!((decoded.flags, decoded.reserved), (0xaabbccdd, 0x11223344));
        assert_eq!(decoded.uuid.data(), [0x5a; 16]);
        assert_eq!(decoded.trailing.data(), [0xfe, 0xed]);
        assert_eq!(decoded.layers().next().unwrap().properties.data(), [5, 6]);
        assert_eq!(decoded.child_count(), 2);
        for child in decoded.children() {
            let child = child.unwrap();
            assert_eq!(child.descriptor.data(), [0xaa; 12]);
            let mut layers = child.item.layers();
            let outer = layers.next().unwrap();
            assert_eq!((outer.domain, outer.id), (*b"TEST", u32::MAX));
            assert_eq!(outer.properties.data(), [3, 4]);
            let offset = outer.properties.offset();
            assert_eq!(&root[offset..offset + 2], [3, 4]);
            assert_eq!(layers.next().unwrap().properties.data(), [1, 2]);
            assert!(layers.next().is_none());
        }
    });
    let mut deep = base;
    for _ in 0..4096 {
        deep = layer(b"TEST", 2, &[], &deep);
    }
    let deep = item(&deep, &[]);
    support::without_heap(|| {
        assert_eq!(
            Item::parse(&deep, LIMITS).unwrap_err().kind,
            ErrorKind::Limit
        );
        let decoded = Item::parse(
            &deep,
            Limits {
                records: 4097,
                ..LIMITS
            },
        )
        .unwrap();
        assert_eq!(decoded.layers().count(), 4097); // No recursive stack growth.
    });
}

#[test]
fn compressed_and_plain_subtrees_expose_the_same_preset_without_ignoring_access_state() {
    let chunks = [0xef, 0xbe, 1, 0, 0, 0, 0xab];
    let mut properties = 1u32.to_le_bytes().to_vec();
    properties.extend(0xdeadbeefu32.to_le_bytes());
    properties.extend(1u32.to_le_bytes());
    properties.extend((chunks.len() as u64).to_le_bytes());
    properties.extend(chunks);
    let payload = item(
        &layer(b"NISD", 0x6d, &properties, &layer(b"NISD", 1, &[], &[])),
        &[],
    );
    for compressed in [false, true] {
        let bytes = encryption(&payload, compressed, false);
        let source = Item::parse(&bytes, LIMITS).unwrap();
        let expanded = source.unencrypted_subtree(payload.len()).unwrap();
        assert_eq!(matches!(expanded, Cow::Owned(_)), compressed);
        assert_eq!(expanded.as_ref(), payload);
        support::without_heap(|| {
            let inner = Item::parse(&expanded, LIMITS).unwrap();
            let chunk = inner.layers().next().unwrap().preset_chunks().unwrap();
            assert_eq!(chunk.data(), chunks);
            assert_eq!(
                source
                    .unencrypted_subtree(payload.len() - 1)
                    .unwrap_err()
                    .kind,
                ErrorKind::Limit
            );
        });
        let protected = encryption(&payload, compressed, true);
        support::without_heap(|| {
            assert_eq!(
                Item::parse(&protected, LIMITS)
                    .unwrap()
                    .unencrypted_subtree(payload.len())
                    .unwrap_err()
                    .kind,
                ErrorKind::AccessRequired
            );
        });
    }
    let bytes = encryption(&payload, false, false);
    support::without_heap(|| {
        let expanded = Item::parse(&bytes, LIMITS)
            .unwrap()
            .unencrypted_subtree(payload.len())
            .unwrap();
        assert!(matches!(expanded, Cow::Borrowed(_)));
    });
}

#[test]
fn malformed_nis_lengths_layers_and_child_tables_are_not_silently_skipped() {
    let base = layer(b"NISD", 1, &[], &[]);
    let bytes = item(&base, &[]);
    for end in 0..bytes.len() {
        support::without_heap(|| {
            assert!(Item::parse(&bytes[..end], LIMITS).is_err());
        });
    }
    for (at, replacement) in [(0, 0u64), (0, u64::MAX), (40, 19), (40, u64::MAX)] {
        let mut bad = bytes.clone();
        bad[at..at + 8].copy_from_slice(&replacement.to_le_bytes());
        support::without_heap(|| {
            assert!(Item::parse(&bad, LIMITS).is_err());
        });
    }
    for at in [8, 56, 60] {
        // Item version, data version and child table version.
        let mut bad = bytes.clone();
        bad[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        support::without_heap(|| {
            let error = Item::parse(&bad, LIMITS).unwrap_err();
            assert_eq!(
                (error.offset, error.kind),
                (at, ErrorKind::UnsupportedVersion(u32::MAX))
            );
        });
    }
    let mut child = bytes.clone();
    child[12] = b'x';
    let root = item(&base, &[child]);
    support::without_heap(|| {
        let root = Item::parse(&root, LIMITS).unwrap();
        assert_eq!(
            root.children().next().unwrap().unwrap_err().kind,
            ErrorKind::InvalidMagic
        );
    });
    let mut child_count = bytes.clone();
    child_count[64..68].copy_from_slice(&u32::MAX.to_le_bytes());
    support::without_heap(|| {
        assert_eq!(
            Item::parse(&child_count, LIMITS).unwrap_err().kind,
            ErrorKind::Limit
        );
    });
}
