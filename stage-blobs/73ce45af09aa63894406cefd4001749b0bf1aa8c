//! Synthetic resource failures remain typed without exposing library paths.
use sampler_uvi::{ResourceError, Resources};

#[test]
fn resource_result_distinguishes_absent_invalid_and_bounded_reads() {
    let root =
        std::env::temp_dir().join(format!("kontra-uvi-resource-result-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("small.png"), b"authored resource").unwrap();
    let large = std::fs::File::create(root.join("large.png")).unwrap();
    large.set_len((32 << 20) + 1).unwrap();
    std::fs::create_dir(root.join("directory.png")).unwrap();
    let resources = Resources::of(&root.join("preset.uvip"));
    assert_eq!(
        resources.read_result("small.png").unwrap(),
        Some(b"authored resource".to_vec())
    );
    assert_eq!(resources.read_result("absent.png").unwrap(), None);
    for path in [
        "",
        "../escape.png",
        "/absolute.png",
        "$Other.ufs/asset.png",
        "bad\0path",
        "bad:path",
    ] {
        assert_eq!(resources.read_result(path), Err(ResourceError::InvalidPath));
    }
    assert_eq!(
        resources.read_result("large.png"),
        Err(ResourceError::Limit)
    );
    assert_eq!(
        resources.read_result("directory.png"),
        Err(ResourceError::Read)
    );
    assert_eq!(
        resources.read_result(&"a".repeat(4097)),
        Err(ResourceError::InvalidPath)
    );
    #[cfg(unix)]
    {
        let outside = root.with_extension("outside");
        std::fs::write(&outside, b"authored outside resource").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("escape.png")).unwrap();
        assert_eq!(
            resources.read_result("escape.png"),
            Err(ResourceError::InvalidPath)
        );
        std::fs::remove_file(outside).unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_bank_admission_is_not_a_missing_loose_asset() {
    let root =
        std::env::temp_dir().join(format!("kontra-uvi-resource-bank-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let bank = root.join("broken.ufs");
    std::fs::write(&bank, b"not an authored UFS header").unwrap();
    let resources = Resources::of(&bank.join("Presets/preset.uvip"));
    assert_eq!(
        resources.read_result("image.png"),
        Err(ResourceError::Unavailable)
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn empty_resource_assignments_clear_images_fonts_and_backgrounds() {
    use sampler_uvi::script::{Config, ScriptHost, Scripts};
    let mut scripts = Scripts::default();
    scripts.insert(
        "Scripts/UI/panel.lua",
        r#"return function()
      local image=Image('face.png')
      image.image=''
      image.overImage=''
      image.font=''
      setBackground('')
    end"#
            .into(),
    );
    let host=ScriptHost::new("<UVI4><Program><EventProcessors><ScriptProcessor><script>require('UI/panel')()</script></ScriptProcessor></EventProcessors></Program></UVI4>",scripts,Config::default()).unwrap();
    assert!(host.fault_counts().init.is_empty());
    assert!(
        host.interface().assets.is_empty(),
        "empty resources must remain empty, not become script directories"
    );
}
