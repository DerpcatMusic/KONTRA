//! An explicit authored-IR bridge to the existing production renderer.
//! The sibling UVI benchmark creates both inputs; no installed library is read.
use super::*;

#[path = "../../../crates/sampler-uvi/src/script/ui/inventory_fixture.rs"]
mod fixture;

#[test]
#[ignore = "requires authored benchmark IR in KONTRA_UVI_INVENTORY_ORACLE_DIR"]
fn authored_inventory_full_model_geometry_and_pixels_match() {
    let dir = std::path::PathBuf::from(
        std::env::var_os("KONTRA_UVI_INVENTORY_ORACLE_DIR")
            .expect("set the private authored fixture directory"),
    );
    let dir = dir.canonicalize().unwrap();
    assert!(dir.starts_with("/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts"));
    let read = |name: &str| {
        let path = dir.join(format!("{name}.json"));
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(metadata.is_file() && metadata.len() <= 64 << 20);
        let bytes = std::fs::read(path).unwrap();
        fixture::decode(&serde_json::from_slice::<Value>(&bytes).unwrap()).unwrap()
    };
    let reference = read("reference");
    let candidate = read("candidate");
    assert_eq!(reference, candidate);
    assert_eq!(reference.widgets.len(), 10_145);
    assert!(
        reference.assets.is_empty(),
        "this fixture carries no artwork or external resources"
    );
    let paint = |face: &ir::Interface| {
        render(
            face,
            &dir.join("authored.uvip"),
            &[],
            0,
            std::slice::from_ref(face),
        )
    };
    let expected = paint(&reference);
    let actual = paint(&candidate);
    assert_eq!(expected["status"], "ok");
    assert_eq!(actual["status"], "ok");
    for field in [
        "widgets",
        "pages",
        "passive_value_changes",
        "font_fallbacks",
    ] {
        assert_eq!(actual[field], expected[field], "{field}");
    }
    let a = actual["shots"].as_array().unwrap();
    let b = expected["shots"].as_array().unwrap();
    assert_eq!(a.len(), 2);
    for (index, (actual, expected)) in a.iter().zip(b).enumerate() {
        assert_eq!(actual["status"], "ok");
        assert_eq!(
            actual["presentation"],
            if index == 0 { "Bitmap" } else { "Vector" }
        );
        for field in [
            "presentation",
            "page",
            "width",
            "height",
            "scale",
            "pending_assets",
            "asset_failures",
            "png_hex",
        ] {
            assert_eq!(actual[field], expected[field], "{field}");
        }
        assert!(
            actual["png_hex"]
                .as_str()
                .unwrap()
                .starts_with("89504e470d0a1a0a")
        );
    }
    println!(
        "authored_inventory_pixels widgets=10145 presentations=2 full_model_equal=true resolved_geometry_equal=true png_equal=true"
    );
}
