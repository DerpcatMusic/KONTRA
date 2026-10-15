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
use fixture::{encryption, item, item_with_trailing, layer};

// Authored properties are zero-filled opaque bytes. Only the public NIS framing
// and lengths model the observed shape; no library payload is included here.
fn unaligned_repository() -> Vec<u8> {
    let base = layer(b"NISD", 1, &[0; 4], &[]);
    let leaf = |domain, id, properties: usize| {
        item_with_trailing(&layer(domain, id, &vec![0; properties], &base), &[], &[])
    };
    let sound_info = leaf(b"NISD", 0x6c, 642);
    let controllers = leaf(b"NISD", 0x79, 3714);
    let subtree = layer(b"NISD", 0x73, &[0; 2572], &base);
    let encryption = item_with_trailing(&layer(b"NISD", 0x74, &[0; 5], &subtree), &[], &[]);
    let header = leaf(b"NIK4", 4, 222);
    let auth = layer(b"NISD", 0x6a, &[0; 38], &base);
    let preset = layer(b"NISD", 0x65, &[0; 31], &auth);
    let preset = item_with_trailing(
        &layer(b"NIK4", 3, &[0; 2], &preset),
        &[sound_info, controllers, encryption, header],
        &[],
    );
    let auth = layer(b"NISD", 0x6a, &[0; 28], &base);
    item_with_trailing(&layer(b"NISD", 0x76, &[0; 58], &auth), &[preset], &[])
}

#[test]
fn unaligned_nested_tables_and_large_opaque_layers_keep_exact_boundaries() {
    use ni_file::nis::ItemContainer;
    use std::io::Cursor;

    let bytes = unaligned_repository();
    assert_eq!(bytes.len(), 8004);
    let root = Item::parse(&bytes, LIMITS).unwrap();
    let preset = root.children().next().unwrap().unwrap().item;
    assert_eq!(preset.raw().offset(), 210);
    assert_eq!(preset.raw().data().len(), 7794);
    let children = preset.children().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(
        children
            .iter()
            .map(|c| c.item.raw().offset())
            .collect::<Vec<_>>(),
        [425, 1171, 4989, 7690]
    );
    let controllers = children[1].item;
    assert_eq!(controllers.raw().data().len(), 3806);
    let layers = controllers.layers().collect::<Vec<_>>();
    assert_eq!((layers[0].domain, layers[0].id), (*b"NISD", 0x79));
    assert_eq!(layers[0].raw().offset(), 1211);
    assert_eq!(layers[0].raw().data().len(), 3758);
    assert_eq!(layers[0].properties.data(), [0; 3714]);
    assert_eq!(layers[1].raw().data().len(), 24);
    assert!(controllers.trailing.data().is_empty());
    assert_eq!(&bytes[4969..4977], [1, 0, 0, 0, 0, 0, 0, 0]);

    let owned = ItemContainer::read(Cursor::new(&bytes)).unwrap();
    assert_eq!(owned.children[0].children[1].data.data, [0; 3714]);
    let mut roundtrip = Vec::new();
    owned.write(&mut roundtrip).unwrap();
    assert_eq!(roundtrip, bytes);
}

#[test]
fn overstated_item_lengths_and_shifted_child_table_versions_remain_errors() {
    use ni_file::nis::ItemContainer;
    use std::io::Cursor;

    let bytes = unaligned_repository();
    let mut overrun = bytes.clone();
    overrun[..8].copy_from_slice(&((bytes.len() + 110) as u64).to_le_bytes());
    assert_eq!(
        Item::parse(&overrun, LIMITS).unwrap_err().kind,
        ErrorKind::Truncated
    );
    let error = ItemContainer::read(Cursor::new(&overrun))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("declared body length 8074/version 1"),
        "{error}"
    );
    assert!(error.contains("available 7964 bytes"), "{error}");

    // Shorten only the outer controller layer, leaving its inner layer valid.
    // This makes the child table read start two bytes early: 00 00 01 00.
    let mut shifted = bytes.clone();
    shifted[1211..1219].copy_from_slice(&3756u64.to_le_bytes());
    let borrowed = Item::parse(&shifted, LIMITS)
        .unwrap()
        .children()
        .next()
        .unwrap()
        .unwrap()
        .item
        .children()
        .nth(1)
        .unwrap()
        .unwrap_err();
    assert_eq!(
        (borrowed.offset, borrowed.kind),
        (4967, ErrorKind::UnsupportedVersion(65536))
    );
    let error = ItemContainer::read(Cursor::new(&shifted))
        .unwrap_err()
        .to_string();
    assert!(error.contains("expected 1, got 65536"), "{error}");
}

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
