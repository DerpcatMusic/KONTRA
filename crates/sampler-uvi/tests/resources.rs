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
    assert_eq!(
        resources.read_result("Assistant-Regular.ttf"),
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

#[test]
fn authored_host_font_identity_is_exact_and_loaded_only_on_request() {
    let root = std::env::temp_dir().join(format!("kontra-uvi-host-font-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let resources = Resources::of(&root.join("preset.uvip"));
    let font = resources
        .read_result("Artwork/Assistant-Regular.ttf")
        .unwrap()
        .expect("authored host font");
    assert_eq!(font.len(), 75500);
    assert_eq!(&font[..4], &[0, 1, 0, 0]);
    assert_eq!(
        resources
            .read_result("Artwork/assistant-regular.TTF")
            .unwrap(),
        Some(font)
    );
    for name in [
        "Assistant-Bold.ttf",
        "Assistant-Light.ttf",
        "Other-Regular.ttf",
        "Assistant-Regular.png",
    ] {
        assert_eq!(resources.read_result(name).unwrap(), None);
    }
    assert_eq!(
        resources.read_result("../Assistant-Regular.ttf"),
        Err(ResourceError::InvalidPath)
    );
    std::fs::write(root.join("Assistant-Regular.ttf"), b"authored bank font").unwrap();
    assert_eq!(
        resources.read_result("Assistant-Regular.ttf").unwrap(),
        Some(b"authored bank font".to_vec())
    );
    std::fs::remove_dir_all(root).unwrap();
}
