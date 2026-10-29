use kontra_format_kontakt::Resources;
use std::{fs, path::PathBuf};

#[test]
fn loose_resource_paths_match_without_case() {
    let root: PathBuf =
        std::env::temp_dir().join(format!("kontra-resources-case-{}", std::process::id()));
    fs::create_dir_all(root.join("Resources/Pictures")).unwrap();
    fs::write(root.join("Resources/Pictures/Control.PNG"), b"picture").unwrap();
    let mut resources = Resources::open(&root.join("instrument.nki"));
    assert_eq!(
        resources.read("pictures/control.png"),
        Some(b"picture".to_vec())
    );
    fs::remove_dir_all(root).unwrap();
}
