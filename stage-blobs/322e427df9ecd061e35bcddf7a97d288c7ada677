#![cfg(feature = "library-access")]
use sampler_uvi::{
    Bank,
    script::{Config, ScriptHost},
};

#[test]
#[ignore = "installed banks and approved KONTRA_UVI_READER required"]
fn visible_authored_images_resolve_and_count_missing_inactive_resources() {
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
        let mut failed_images = 0;
        let mut failed_visible_images = 0;
        let members = bank.members();
        for asset in &face.assets {
            let result = bank.ui_resource_result(&program, &asset.path);
            let found = matches!(&result, Ok(Some(_)));
            if asset.kind == sampler_ui_ir::AssetKind::TrueTypeFont {
                missing_fonts += usize::from(!found);
            } else {
                if found {
                    images += 1
                } else {
                    failed_images += 1;
                    let reason = match result {
                        Ok(None) => "missing",
                        Err(e) => match e {
                            sampler_uvi::ResourceError::InvalidPath => "invalid",
                            sampler_uvi::ResourceError::Ambiguous => "ambiguous",
                            sampler_uvi::ResourceError::Corrupt => "corrupt",
                            sampler_uvi::ResourceError::Limit => "limit",
                            sampler_uvi::ResourceError::Read => "read",
                            sampler_uvi::ResourceError::Unavailable => "unavailable",
                        },
                        _ => unreachable!(),
                    };
                    let basename = asset.path.rsplit('/').next().unwrap_or("");
                    let matches = members
                        .iter()
                        .filter(|m| {
                            m.rsplit('/')
                                .next()
                                .is_some_and(|n| n.eq_ignore_ascii_case(basename))
                        })
                        .count();
                    let refs: Vec<_> = face
                        .widgets
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| {
                            w.images
                                .iter()
                                .any(|i| face.assets[i.asset.0].path == asset.path)
                        })
                        .collect();
                    failed_visible_images += usize::from(
                        refs.iter()
                            .any(|(i, _)| face.visible(sampler_ui_ir::WidgetRef(*i))),
                    );
                    let extension = match basename
                        .rsplit_once('.')
                        .map(|(_, e)| e.to_ascii_lowercase())
                        .as_deref()
                    {
                        Some("png") => "png",
                        Some("jpg" | "jpeg") => "jpeg",
                        Some("svg") => "svg",
                        Some(_) => "other",
                        None => "none",
                    };
                    eprintln!(
                        "image_failure={reason} basename_matches={matches} empty_basename={} extension={extension} widget_refs={} visible_refs={} image_widgets={} rooted={} volume={} parent={} segments={}",
                        basename.is_empty(),
                        refs.len(),
                        refs.iter()
                            .filter(|(i, _)| face.visible(sampler_ui_ir::WidgetRef(*i)))
                            .count(),
                        refs.iter()
                            .filter(|(_, w)| matches!(w.kind, sampler_ui_ir::Kind::Image))
                            .count(),
                        asset.path.starts_with('/'),
                        asset.path.starts_with('$'),
                        asset.path.contains(".."),
                        asset.path.split('/').count()
                    );
                }
            }
        }
        assert!(images > 0);
        assert_eq!(failed_visible_images, 0, "visible image authority failed");
        eprintln!(
            "widgets={} images={images} missing_inactive_images={failed_images} missing_fonts={missing_fonts}",
            face.widgets.len()
        );
    }
}
