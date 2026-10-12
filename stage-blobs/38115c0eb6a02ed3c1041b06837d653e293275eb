use sampler_ui_ir::{AssetKind, Orientation};
use sampler_uvi::script::{Config, ScriptHost, control_id};

fn host(script: &str) -> ScriptHost {
    ScriptHost::new(
        &format!("<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"),
        (),
        Config::default(),
    ).unwrap()
}

#[test]
fn v1_strip_images_keep_authored_frame_count_and_orientation() {
    let h = host(
        r#"
      Knob('Horizontal'):setStripImage('horizontal.png',17,true)
      Slider('Vertical'):setStripImage('vertical.png',23,false)
      Knob('Default'):setStripImage('default.png',31)
    "#,
    );
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    let face = h.interface();
    for (widget, frames, axis) in [
        (0, 17, Orientation::Horizontal),
        (1, 23, Orientation::Vertical),
        (2, 31, Orientation::Vertical),
    ] {
        let image = &face.assets[face.widgets[widget].images[0].asset.0];
        let AssetKind::Image(meta) = image.kind else {
            panic!("strip must be an image")
        };
        assert_eq!(meta.frames, frames);
        assert_eq!(meta.axis, axis);
    }
}

#[test]
fn root_ui_changes_in_authored_callbacks_publish_a_new_revision() {
    let mut h = host(
        r#"
      local b=Button('Update')
      b.changed=function()
        setSize(640,480)
        setBackground('authored.png')
        setBackgroundColour('#112233')
        makePerformanceView()
      end
    "#,
    );
    let before = h.ui_revision();
    h.set_control(control_id(1, 0), 1.).unwrap();
    assert!(
        h.ui_revision() > before,
        "root mutations must publish to the UI owner"
    );
    let face = h.interface();
    assert_eq!(
        (face.pages[0].size.width, face.pages[0].size.height),
        (640, 480)
    );
    assert!(
        face.assets
            .iter()
            .any(|asset| asset.path.ends_with("authored.png"))
    );
}

#[test]
fn widgets_created_in_authored_callbacks_publish_a_new_revision() {
    let mut h = host("Label('Initial'); function onController(e) Label('Created') end");
    let before = h.ui_revision();
    h.controller(1, 64, 0);
    assert!(
        h.ui_revision() > before,
        "new authored widgets must publish to the UI owner"
    );
    assert_eq!(h.interface().widgets[1].name, "Created");
    assert!(h.findings().is_empty(), "{:?}", h.findings());
}
