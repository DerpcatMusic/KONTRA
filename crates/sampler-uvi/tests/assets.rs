#![cfg(feature = "library-access")]
use sampler_uvi::{
    Bank,
    script::{Config, ScriptHost},
};

#[test]
#[ignore = "installed banks and approved KONTRA_UVI_READER required"]
fn authored_images_resolve_from_their_script_package() {
    for (path, program) in [
        (
            "/mnt/MAIN_STORAGE/Libraries/UVI/UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs",
            "Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip",
        ),
        (
            "/mnt/MAIN_STORAGE/Libraries/UVI/VWinds - Clarinets/VWinds-AClarinet.ufs",
            "Presets/Clarinet A.uvip",
        ),
    ] {
        let bank = Bank::open(std::path::Path::new(path)).unwrap();
        let (xml, program) = bank.program(program).unwrap();
        let host = ScriptHost::new(&xml, bank.scripts(), Config::default()).unwrap();
        assert!(
            host.fault_counts().init.is_empty(),
            "initialization must complete without any Lua/budget fault"
        );
        let face = host.interface();
        let mut images = 0;
        let mut missing_fonts = 0;
        for asset in &face.assets {
            let found = bank.ui_resource(&program, &asset.path).is_ok();
            if asset.kind == sampler_ui_ir::AssetKind::TrueTypeFont {
                missing_fonts += usize::from(!found);
            } else {
                assert!(found, "image authority failed");
                images += 1;
            }
        }
        assert!(images > 0);
        eprintln!(
            "widgets={} images={images} missing_fonts={missing_fonts}",
            face.widgets.len()
        );
    }
}
