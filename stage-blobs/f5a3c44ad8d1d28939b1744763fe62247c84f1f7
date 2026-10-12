#![cfg(target_os = "linux")]
use sampler_kontakt::Samples;

#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

#[test]
fn repeated_archive_resolution_reuses_path_checks() {
    let root =
        std::env::temp_dir().join(format!("kontra-archive-path-cache-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let name = "clear.wav";
    let offset = 22 + 8 + (name.len() + 1) * 2;
    let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
    bytes.extend(0x110u16.to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend(1u32.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(((8 + (name.len() + 1) * 2) as u16).to_le_bytes());
    bytes.extend((offset as u32).to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    for c in name.encode_utf16().chain([0]) {
        bytes.extend(c.to_le_bytes());
    }
    let mut header = [0u8; 22];
    header[..4].copy_from_slice(&0x2ae905fau32.to_le_bytes());
    header[4..6].copy_from_slice(&0x110u16.to_le_bytes());
    header[10..14].copy_from_slice(&0xffu32.to_le_bytes());
    header[14..18].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend(header);
    bytes.push(0);
    let archive = root.join("authored.nkx");
    std::fs::write(&archive, bytes).unwrap();
    let expected = root.canonicalize().unwrap().join("authored.nkx/clear.wav");
    let mut samples = Samples::new(&root);
    for _ in 0..64 {
        assert_eq!(
            samples.resolve(&root, "authored.nkx/clear.wav").unwrap(),
            Some(expected.clone())
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn archive_path_cache_keeps_library_boundaries_and_fresh_loads() {
    let scope =
        std::env::temp_dir().join(format!("kontra-archive-boundary-{}", std::process::id()));
    let root = scope.join("library");
    std::fs::create_dir_all(&root).unwrap();
    let outside = scope.join("outside.nkx");
    let inside = root.join("inside.nkx");
    std::fs::write(
        &outside,
        b"invalid archive must never be opened outside the library",
    )
    .unwrap();
    std::fs::write(
        &inside,
        b"invalid inside archive must report its parse error",
    )
    .unwrap();
    let alias = root.join("alias.nkx");
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    let mut samples = Samples::new(&root);
    for _ in 0..2 {
        assert!(
            samples
                .resolve(&root, "alias.nkx/member.wav")
                .unwrap()
                .is_none()
        );
    }
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&inside, &alias).unwrap();
    assert!(
        Samples::new(&root)
            .resolve(&root, "alias.nkx/member.wav")
            .is_err()
    );
    std::fs::remove_dir_all(scope).unwrap();
}

#[test]
fn loose_sources_do_not_allocate_virtual_members_for_each_parent() {
    let root = std::env::temp_dir().join(format!("kontra-source-alloc-{}", std::process::id()));
    let folder = root
        .join("a-repeated-directory-component")
        .join("another-repeated-directory-component")
        .join("a-third-repeated-directory-component");
    std::fs::create_dir_all(&folder).unwrap();
    let mut samples = Samples::new(&root);
    let mut measured = Vec::new();
    for parent in [&folder, &folder.join("directory.nkx")] {
        std::fs::create_dir_all(parent).unwrap();
        let path = parent.join("loose.wav");
        std::fs::write(&path, b"source metadata fixture").unwrap();
        // Archive-named directories retain one negative path check per load.
        samples.source(&path).unwrap();
        let bytes = support::allocated_bytes(|| {
            std::hint::black_box(samples.source(&path).unwrap());
        });
        measured.push((bytes, path.as_os_str().len()));
    }
    std::fs::remove_dir_all(&root).unwrap();
    for (bytes, path_bytes) in measured {
        println!("source_lookup_alloc_bytes={bytes} path_bytes={path_bytes}");
        assert!(
            bytes <= 2 * path_bytes,
            "loose source should only retain its path, not construct a member for every parent: {bytes} bytes"
        );
    }
}
