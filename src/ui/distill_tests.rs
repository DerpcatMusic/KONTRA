//! Chrome copy and layout receipts. No instrument face is rendered here.
use super::*;
use tests::{Harness, pixels};

#[test]
fn empty_browser_keeps_help_on_its_action() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = Arc::new(SamplerParams::new());
        let h = Harness::new(&p, width, height);
        let scene = h.ui.scene().unwrap();
        let heading = scene.surfaces().find(|s| s.text_value.as_deref() == Some("No libraries yet")).unwrap().frame;
        let last = scene.surface("empty-add-one").unwrap().frame;
        assert!(last.y + last.size.height - heading.y <= CONTROL * 3. + SPACE * 2.,
            "empty browser keeps its two actions together without a repeated paragraph");
        assert!(scene.surface("empty-add-many").unwrap().tip.as_deref()
            .is_some_and(|tip| tip.contains("without a library file")), "discovery help remains available");
    }
}

#[test]
fn empty_rack_has_one_prompt_and_keeps_multi_help_on_hover() {
    let p = Arc::new(SamplerParams::new());
    let h = Harness::new(&p, 900., 600.);
    let scene = h.ui.scene().unwrap();
    assert!(!scene.surfaces().any(|s| s.text_value.as_deref()
        .is_some_and(|text| text.starts_with("Choose a library on the left"))), "one visible loading prompt is enough");
    let prompt = scene.surfaces().find(|s| s.text_value.as_deref() == Some("Pick an instrument")).unwrap();
    assert!(scene.surface("rack-drop").unwrap().tip.as_deref()
        .is_some_and(|tip| tip.contains("Multis load the whole rack")));
    let rack = scene.surface("rack-view").unwrap().frame;
    assert!(prompt.frame.x >= rack.x && prompt.frame.x + prompt.frame.size.width <= rack.x + rack.size.width);
}

#[test]
fn empty_settings_uses_the_add_actions_as_instructions() {
    let p = Arc::new(SamplerParams::new());
    let mut h = Harness::new(&p, 900., 600.);
    h.press("app-menu"); h.press("menu-item-5");
    let scene = h.ui.scene().unwrap();
    assert!(!scene.surfaces().any(|s| s.text_value.as_deref()
        .is_some_and(|text| text.starts_with("No library folders yet."))), "the folder actions already explain the empty state");
    assert!(scene.surface("root-pick-many").unwrap().tip.as_deref()
        .is_some_and(|tip| tip.contains("each library")));
    let field = scene.surface("root").unwrap().frame;
    let toolbar = scene.surface("root-pick-many").unwrap().frame;
    assert!(field.y - toolbar.y <= CONTROL + SPACE * 2., "typed folder follows the toolbar directly");
}

#[test]
#[cfg(feature = "shots")]
fn distill_startup_shots() {
    let Some(out) = std::env::var_os("KONTRA_DISTILL_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = Arc::new(SamplerParams::new());
        let mut h = Harness::new(&p, width as f64, height as f64);
        let save = |name: &str, h: &mut Harness| {
            h.idle(20);
            moose::core::screenshot::save_png(&out.join(format!("{name}-{width}.png")),
                &pixels(&h.ui, width, height), width.into(), height.into());
        };
        save("browser-rack-empty", &mut h);
        h.press("app-menu"); h.press("menu-item-5");
        save("settings-empty", &mut h);
    }
}
