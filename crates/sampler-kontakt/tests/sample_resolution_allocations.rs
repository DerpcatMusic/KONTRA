#![cfg(target_os = "linux")]
use sampler_kontakt::Samples;

#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn archive(root: &std::path::Path, filename: &str, member: &str) {
    let offset = 22 + 8 + (member.encode_utf16().count() + 1) * 2;
    let mut bytes = 0x5e70ac54u32.to_le_bytes().to_vec();
    bytes.extend(0x110u16.to_le_bytes());
    bytes.extend([0; 8]);
    bytes.extend(1u32.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.extend(((offset - 22) as u16).to_le_bytes());
    bytes.extend((offset as u32).to_le_bytes());
    bytes.extend(0u16.to_le_bytes());
    for c in member.encode_utf16().chain([0]) {
        bytes.extend(c.to_le_bytes());
    }
    let mut header = [0; 22];
    header[..4].copy_from_slice(&0x2ae905fau32.to_le_bytes());
    header[4..6].copy_from_slice(&0x110u16.to_le_bytes());
    header[10..14].copy_from_slice(&0xffu32.to_le_bytes());
    header[14..18].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend(header);
    bytes.push(0);
    std::fs::write(root.join(filename), bytes).unwrap();
}

#[test]
fn archive_spelling_aliases_share_the_actual_member_identity() {
    let root = std::env::temp_dir().join(format!("kontra-member-identity-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    archive(&root, "authored.nkx", "Clear.wav");
    let expected = root.canonicalize().unwrap().join("authored.nkx/Clear.wav");
    let mut samples = Samples::new(&root);
    let found: Vec<_> = [
        "authored.nkx/Clear.wav",
        "authored.nkx/clear.wav",
        "authored.nkx/CLEAR.WAV",
    ]
    .into_iter()
    .map(|name| samples.resolve(&root, name).unwrap())
    .collect();
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        found.iter().all(|p| p.as_ref() == Some(&expected)),
        "{found:?}"
    );
}

#[test]
fn moved_archive_prefixes_resolve_inside_the_library_and_keep_mic_identity() {
    let root = std::env::temp_dir().join(format!("kontra-moved-archive-{}", std::process::id()));
    let parent = root.join("Instruments");
    let close = root.join("Samples/Close");
    let far = root.join("Samples/Far");
    for dir in [&parent, &close, &far] {
        std::fs::create_dir_all(dir).unwrap();
    }
    archive(&close, "authored.nkx", "Clear.wav");
    archive(&far, "authored.nkx", "Clear.wav");
    let expected = close.canonicalize().unwrap().join("authored.nkx/Clear.wav");
    let mut samples = Samples::new(&root);
    let relocated = samples
        .resolve(&parent, "D:\\old\\Samples\\Close\\AUTHORED.NKX\\CLEAR.WAV")
        .unwrap();
    let ambiguous = samples.resolve(&parent, "D:/old/authored.nkx/Clear.wav");
    let missing_member = samples
        .resolve(&parent, "D:/old/Close/authored.nkx/missing.wav")
        .unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    assert_eq!(relocated, Some(expected));
    assert!(
        ambiguous.is_err(),
        "a duplicate mic archive must not be selected silently"
    );
    assert_eq!(missing_member, None);
}

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
