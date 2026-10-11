use sampler_ui_ir::Size;
use sampler_uvi::script::{Config, Files, ScriptHost};

struct Images;
impl Files for Images {
    fn script(&self, _: &str) -> Option<String> {
        None
    }
    fn image_size(&self, path: &str) -> Option<Size> {
        (path == "overlay.png").then_some(Size {
            width: 720,
            height: 18,
        })
    }
}

#[test]
fn image_geometry_uses_intrinsic_size_only_on_omitted_axes() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
      a=Image('overlay.png')
      b=Image('overlay.png'); b.width=100
      c=Image{'overlay.png',size={100,100}}
      d=Image('missing.png')
      e=Image('overlay.png'); e.bounds={10,20,120,30}
      f=Image('overlay.png'); f.height=40
      </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let host = ScriptHost::new(xml, Images, Config::default()).unwrap();
    assert!(
        host.fault_counts().init.is_empty(),
        "{:?}",
        host.fault_counts()
    );
    let face = host.interface();
    let sizes: Vec<_> = face
        .widgets
        .iter()
        .map(|w| (w.rect.width, w.rect.height))
        .collect();
    assert_eq!(
        sizes,
        [
            (720, 18),
            (100, 18),
            (100, 100),
            (100, 100),
            (120, 30),
            (720, 40)
        ]
    );
    assert_eq!((face.widgets[4].rect.x, face.widgets[4].rect.y), (10, 20));
    assert_eq!(
        face,
        host.interface(),
        "repeated snapshots retain image sizing"
    );
}
