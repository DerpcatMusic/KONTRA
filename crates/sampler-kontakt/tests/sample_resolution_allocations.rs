#![cfg(target_os = "linux")]
use sampler_kontakt::Samples;

#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

#[test]
fn loose_sources_do_not_allocate_virtual_members_for_each_parent() {
    let root = std::env::temp_dir().join(format!("kontra-source-alloc-{}", std::process::id()));
    let folder = root.join("a-repeated-directory-component")
        .join("another-repeated-directory-component")
        .join("a-third-repeated-directory-component");
    std::fs::create_dir_all(&folder).unwrap();
    let mut samples = Samples::new(&root);
    let mut measured = Vec::new();
    for parent in [&folder, &folder.join("directory.nkx")] {
        std::fs::create_dir_all(parent).unwrap();
        let path = parent.join("loose.wav");
        std::fs::write(&path, b"source metadata fixture").unwrap();
        let bytes = support::allocated_bytes(|| {
            std::hint::black_box(samples.source(&path).unwrap());
        });
        measured.push((bytes, path.as_os_str().len()));
    }
    std::fs::remove_dir_all(&root).unwrap();
    for (bytes, path_bytes) in measured {
        assert!(bytes <= 2 * path_bytes,
            "loose source should only retain its path, not construct a member for every parent: {bytes} bytes");
    }
}
