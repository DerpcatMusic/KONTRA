use super::*;

#[test]
fn decoded_filmstrip_demand_releases_pixels_rejects_stale_publication_and_restores_original() {
    let root = std::env::temp_dir().join(format!("kontra-visual-demand-{}", std::process::id()));
    let folder = root.join("Resources/pictures");
    std::fs::create_dir_all(&folder).unwrap();
    // Three physically distinct, opaque frames, decoded through the actual PNG
    // resource path rather than repeated Arcs or synthetic memory statistics.
    let rgba: Vec<u8> = (0..48 * 16)
        .flat_map(|i| {
            let frame = i % 48 / 16;
            [32 + frame as u8 * 64, 96, 176, 255]
        })
        .collect();
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, 48, 16);
        encoder.set_color(png::ColorType::Rgba);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&rgba)
            .unwrap();
    }
    std::fs::write(folder.join("strip.png"), png).unwrap();
    std::fs::write(
        folder.join("strip.txt"),
        "Number of Animations: 3\nHorizontal Animation: yes\n",
    )
    .unwrap();
    let instrument = Arc::new(Instrument {
        path: root.join("Patch.nki"),
        ..Default::default()
    });
    let interface = Arc::new(crate::ksp::initialize("on init\nmake_perfview\ndeclare ui_slider $level(0,100)\n$level := 37\nset_control_par_str(get_ui_id($level), $CONTROL_PAR_PICTURE, \"strip\")\nend on", 0, 8).unwrap());
    let p = Arc::new(SamplerParams::new());
    p.shared.ensure_parts(2);
    let original = VisualDemand {
        pictures: vec!["strip".into()],
        metadata: vec!["strip".into()],
        ..Default::default()
    };
    {
        let mut view = p.shared.view.lock().unwrap();
        for slot in 0..2 {
            view.parts[slot].instrument = Some(instrument.clone());
            view.parts[slot].interface = Some(interface.clone());
            view.parts[slot].visual_demand = original.clone();
            view.parts[slot].visual_revision = 1;
        }
    }
    let request = |slot, revision, demand: VisualDemand| Request {
        slot,
        path: instrument.path.clone(),
        program: 0,
        generation: p
            .shared
            .part(slot)
            .unwrap()
            .generation
            .load(Ordering::Acquire),
        epoch: 0,
        revision,
        demand,
        instrument: instrument.clone(),
        interface: Some(interface.clone()),
        missing_metadata: vec!["strip".into()],
    };
    let first = request(0, 1, original.clone());
    let loaded = load(&first);
    assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
    let picture = Arc::downgrade(&loaded.pictures["strip"]);
    let frames: Vec<_> = loaded.pictures["strip"]
        .frames
        .iter()
        .map(Arc::downgrade)
        .collect();
    assert_eq!(frames.len(), 3);
    assert!(publish(&p, &first, loaded));
    assert!(publish(&p, &request(1, 1, original.clone()), load(&first)));
    let stale = load(&first);
    let stale_frames: Vec<_> = stale.pictures["strip"]
        .frames
        .iter()
        .map(Arc::downgrade)
        .collect();
    // Empty demand is the actual hidden/Kontra trim entry point and spawns no
    // worker. Other slots retain their independently decoded artwork.
    p.request_visual_assets(0, VisualDemand::default());
    assert!(picture.upgrade().is_none());
    assert!(frames.iter().all(|frame| frame.upgrade().is_none()));
    assert!(!publish(&p, &first, stale));
    assert!(stale_frames.iter().all(|frame| frame.upgrade().is_none()));
    {
        let view = p.shared.view.lock().unwrap();
        assert_eq!(view.parts[0].visual_bytes().0, 0);
        assert_eq!(view.parts[1].visual_bytes().0, 3 * 16 * 16 * 4);
        let meta = &view.parts[0].picture_metadata["strip"];
        assert_eq!((meta.size, meta.frames, meta.clear), ([16, 16], 3, false));
        assert_eq!(meta.size(80., 20.), (16., 16.));
        assert!(Arc::ptr_eq(
            view.parts[0].interface.as_ref().unwrap(),
            &interface
        ));
    }
    // Deterministic worker completion for the next Original generation.
    let revision = {
        let mut view = p.shared.view.lock().unwrap();
        let part = &mut view.parts[0];
        part.visual_demand = original.clone();
        part.visual_revision += 1;
        part.visual_revision
    };
    let restored = request(0, revision, original);
    assert!(publish(&p, &restored, load(&restored)));
    {
        let view = p.shared.view.lock().unwrap();
        let strip = &view.parts[0].pictures["strip"];
        assert_eq!(strip.size(80., 20.), (16., 16.));
        assert_eq!(strip.frames.len(), 3);
        for (i, frame) in strip.frames.iter().enumerate() {
            assert!(frame
                .rgba
                .chunks_exact(4)
                .all(|pixel| pixel == [32 + i as u8 * 64, 96, 176, 255]));
        }
        assert_eq!(
            view.parts[0].interface.as_ref().unwrap().controls[0].properties["$CONTROL_PAR_VALUE"],
            crate::ksp::Value::Int(37)
        );
    }
    let retained: Vec<_> = {
        let view = p.shared.view.lock().unwrap();
        view.parts[0].pictures["strip"]
            .frames
            .iter()
            .map(Arc::downgrade)
            .collect()
    };
    crate::ui::asset_restoration_harness(&p, |mode| {
        if mode != 1 {
            assert!(
                retained.iter().all(|frame| frame.upgrade().is_none()),
                "compact scene retained original decoded frames"
            );
            assert_eq!(p.shared.view.lock().unwrap().parts[0].visual_bytes().0, 0);
        } else {
            let (revision, demand) = {
                let view = p.shared.view.lock().unwrap();
                (
                    view.parts[0].visual_revision,
                    view.parts[0].visual_demand.clone(),
                )
            };
            let restore = request(0, revision, demand);
            assert!(publish(&p, &restore, load(&restore)));
            assert_eq!(
                p.shared.view.lock().unwrap().parts[0].visual_bytes().0,
                3 * 16 * 16 * 4
            );
        }
    });
    // NativeUI keeps the real Lua module state while decoded images and their
    // original compressed payload can lose every visual owner.
    let native = root.join("Resources/native_ui");
    std::fs::create_dir_all(&native).unwrap();
    std::fs::copy(folder.join("strip.png"), native.join("strip.png")).unwrap();
    std::fs::write(
        native.join("root.nui"),
        "local count = 0\nreturn function() count = count + 1; native_count = count; return {} end",
    )
    .unwrap();
    let sources = crate::native_ui::Package::load_sources(&instrument.path).unwrap();
    assert!(sources.images.is_empty());
    assert!(!sources.members.contains_key("strip.png"));
    assert_eq!(sources.image_metadata["strip.png"], [48, 16]);
    let full = Arc::new(crate::native_ui::Package::load(&instrument.path).unwrap());
    let native_image = Arc::downgrade(&full.images["strip.png"]);
    let compressed = Arc::downgrade(&full.members["strip.png"]);
    let session = crate::native_ui::Session::new(full.clone(), "root", Arc::from([])).unwrap();
    let require = session
        .lua()
        .globals()
        .get::<mlua::Function>("require")
        .unwrap();
    let component: mlua::Function = require.call("root").unwrap();
    session.call(component.clone(), ()).unwrap();
    assert_eq!(
        session.lua().globals().get::<i64>("native_count").unwrap(),
        1
    );
    let mut reduced = full.retain_visuals(&[], &[]);
    assert!(sources.same_sources(&reduced));
    assert_eq!(reduced.image_metadata["strip.png"], [48, 16]);
    drop(full);
    assert!(
        native_image.upgrade().is_none(),
        "Lua session retained decoded NativeUI pixels"
    );
    assert!(
        compressed.upgrade().is_none(),
        "Lua session retained compressed NativeUI picture bytes"
    );
    reduced.merge_visuals(
        crate::native_ui::Package::load_visuals(&instrument.path, &["strip.png".into()], &[])
            .unwrap(),
    );
    assert_eq!(reduced.images["strip.png"].rgba.as_ref(), rgba.as_slice());
    assert!(!reduced.members.contains_key("strip.png"));
    let restored_image = Arc::downgrade(&reduced.images["strip.png"]);
    reduced = reduced.retain_visuals(&[], &[]);
    assert!(restored_image.upgrade().is_none());
    assert!(sources.same_sources(&reduced));
    session.call(component, ()).unwrap();
    assert_eq!(
        session.lua().globals().get::<i64>("native_count").unwrap(),
        2,
        "same real Lua closure survives pruning and restoration"
    );
    std::fs::remove_dir_all(root).unwrap();
}
