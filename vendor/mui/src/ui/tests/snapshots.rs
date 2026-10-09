use super::*;

fn font_ui() -> Ui {
    Ui::default().font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
}

#[test]
fn snapshots_share_the_current_allocation_and_direct_updates_preserve_old_frames() {
    let mut ui = font_ui();
    assert!(ui.scene_snapshot().is_none());
    ui.frame(
        text("before")
            .reserve("before after")
            .a11y(A11y::Label)
            .id("label"),
        None,
        Input::default(),
        0.016,
    )
    .unwrap();
    let old = ui.scene_snapshot().unwrap();
    assert!(std::ptr::eq(ui.scene().unwrap(), old.as_ref()));
    assert!(Arc::ptr_eq(&old, &ui.scene_snapshot().unwrap()));
    let old_paint = old.paint.clone();
    ui.set_text("label", "after").unwrap();
    let new = ui.scene_snapshot().unwrap();
    assert!(!Arc::ptr_eq(&old, &new));
    assert_eq!(old.paint, old_paint);
    let label = |scene: &ResolvedScene| {
        scene
            .surface("label")
            .unwrap()
            .semantics
            .as_ref()
            .unwrap()
            .label
            .clone()
    };
    assert_eq!(label(&old).as_deref(), Some("before"));
    assert_eq!(label(&new).as_deref(), Some("after"));
    drop(new);
    let address = ui.scene().unwrap() as *const ResolvedScene;
    ui.set_text("label", "again").unwrap();
    assert_eq!(
        address,
        ui.scene().unwrap() as *const ResolvedScene,
        "an unshared direct update stays in place"
    );
}

#[test]
fn retaining_snapshots_preserves_layout_and_text_cache_reuse() {
    let font = Font::new(epaint_default_fonts::HACK_REGULAR).unwrap();
    let (mut retained, mut recycled) = (Ui::default().font(font.clone()), Ui::default().font(font));
    let tree = || row([text("cached text").id("label"), block(20., 20.).id("pad")]);
    let mut snapshots = Vec::new();
    for _ in 0..8 {
        for ui in [&mut retained, &mut recycled] {
            ui.frame(tree(), Some(Size::new(300., 80.)), Input::default(), 0.016)
                .unwrap();
        }
        assert_eq!(
            retained.scene().unwrap().paint,
            recycled.scene().unwrap().paint
        );
        assert_eq!(retained.layout_stats(), recycled.layout_stats());
        assert_eq!(retained.resolver.text_runs(), recycled.resolver.text_runs());
        let snapshot = retained.scene_snapshot().unwrap();
        if let Some(previous) = snapshots.last() {
            assert!(
                !Arc::ptr_eq(previous, &snapshot),
                "a build owns a new scene"
            );
        }
        snapshots.push(snapshot);
    }
    assert_eq!(retained.layout_stats().measured_nodes, 0);
    assert_eq!(retained.layout_stats().arranged_nodes, 0);
    assert!(retained.resolver.text_runs() > 0);
    for snapshot in snapshots {
        assert_eq!(snapshot.paint, retained.scene().unwrap().paint);
    }
}

#[test]
fn direct_weld_updates_do_not_change_retained_uniforms() {
    let mut ui = Ui::default().gpu_welding();
    ui.frame(
        mui_scene::weld![Weld::default(); block(20., 20.).fill(Role::Raised), block(20., 20.).fill(Role::Primary)].id("w"),
        None,
        Input::default(),
        0.016,
    )
    .unwrap();
    let old = ui.scene_snapshot().unwrap();
    let uniforms = old.external_weld("w").unwrap().material.uniform_bytes();
    assert!(ui.set_weld_morph("w", 0.5).unwrap());
    assert!(ui.set_weld_material_blend("w", 0.25).unwrap());
    assert!(
        ui.set_weld_solid_material("w", 0, Some(Color::srgb(1., 0., 0.)), None, 0.)
            .unwrap()
    );
    assert_eq!(
        old.external_weld("w").unwrap().material.uniform_bytes(),
        uniforms
    );
    assert_ne!(
        ui.scene()
            .unwrap()
            .external_weld("w")
            .unwrap()
            .material
            .uniform_bytes(),
        uniforms
    );
}
