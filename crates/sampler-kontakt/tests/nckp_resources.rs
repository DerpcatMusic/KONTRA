use sampler_ir as ir;
use std::path::PathBuf;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kontra-authored-nckp-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("Instruments")).unwrap();
        std::fs::create_dir_all(root.join("Resources/performance_view")).unwrap();
        Self(root)
    }

    fn view(&self, file: &str, id: &str, width: i32) {
        let json = serde_json::json!({"value": {"performanceView": {
            "size": {"width": width, "height": 300},
            "controls": [{"index": 7, "value": {
                "common": {"id": id, "position": {"x": 10, "y": 10}},
                "value": {"min": 0, "max": 100, "default": 42}
            }}]
        }}});
        std::fs::write(
            self.0.join("Resources/performance_view").join(file),
            json.to_string(),
        )
        .unwrap();
    }

    fn compile(&self, source: &str) -> (ir::Instrument, Vec<sampler_ksp::Script>) {
        let mut instrument = ir::Instrument {
            behaviors: vec![ir::Behavior {
                name: "authored nckp".into(),
                language: ir::Language::Ksp,
                source: source.into(),
                slot: Some(0),
                state: Vec::new(),
                requires: Vec::new(),
            }],
            ..Default::default()
        };
        let (scripts, _, _) = sampler_kontakt::compile_ui(
            &mut instrument,
            &sampler_kontakt::Options {
                library: Some(self.0.join("Instruments/Authored.nki")),
                ..Default::default()
            },
        );
        (instrument, scripts)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn precompile_discovery_loads_real_controls_not_comment_or_message_decoys() {
    let fixture = Fixture::new();
    fixture.view("real.nckp", "Real", 640);
    fixture.view("decoy.nckp", "Decoy", 320);
    for source in [
        r#"{ load_performance_view("decoy") }
           on init load_performance_view("real")
           set_control_par(get_ui_id($Real), $CONTROL_PAR_VALUE, 17) end on"#,
        r#"on init message("load_performance_view(decoy)")
           load_performance_view("real")
           set_control_par(get_ui_id($Real), $CONTROL_PAR_VALUE, 17) end on"#,
        r#"USE_CODE_IF(OFF)
           on init load_performance_view("decoy") end on
           END_USE_CODE
           on init load_performance_view("real")
           set_control_par(get_ui_id($Real), $CONTROL_PAR_VALUE, 17) end on"#,
    ] {
        let (instrument, scripts) = fixture.compile(source);
        assert_eq!(scripts.len(), 1, "{:?}", instrument.unsupported);
        assert!(
            instrument.unsupported.is_empty(),
            "{:?}",
            instrument.unsupported
        );
        let interface = &scripts[0].model().interface;
        assert_eq!(interface.width_px, Some(640));
        assert_eq!(interface.widgets.len(), 1);
        let real = &interface.widgets[0];
        assert_eq!(real.name, "$Real");
        assert!(
            real.control.is_some(),
            "the actual resource declared a control"
        );
        assert_eq!(
            real.properties.get("$CONTROL_PAR_VALUE"),
            Some(&sampler_ksp::model::Value::Int(17))
        );
    }
}

#[test]
fn precompile_discovery_does_not_read_a_fragment_or_later_unrelated_literal() {
    let fixture = Fixture::new();
    fixture.view("unrelated.nckp", "Unrelated", 320);
    fixture.view("base.nckp", "Fragment", 321);
    fixture.view("basesuffix.nckp", "Complete", 640);
    for source in [
        r#"on init declare @view := "basesuffix"
           load_performance_view(@view) message("unrelated") end on"#,
        r#"on init load_performance_view("base" & "suffix") end on"#,
        r#"on init declare @suffix := "suffix"
           load_performance_view("base" & @suffix) end on"#,
    ] {
        let (instrument, scripts) = fixture.compile(source);
        assert_eq!(scripts.len(), 1, "{:?}", instrument.unsupported);
        let interface = &scripts[0].model().interface;
        assert!(
            interface.widgets.is_empty(),
            "no resource may be guessed: {source}"
        );
        assert_eq!(interface.width_px, None);
        assert!(
            !instrument
                .unsupported
                .iter()
                .any(|u| u.feature == "performance view"),
            "no guessed-resource read, including no missing-resource lookup"
        );
    }
}

#[test]
fn literal_resource_read_failures_keep_the_existing_load_diagnostic() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.0.join("Resources/performance_view/corrupt.nckp"),
        b"not JSON",
    )
    .unwrap();
    for (name, detail) in [
        ("missing", "not found in the library"),
        ("corrupt", "corrupt.nckp:"),
        ("../outside", "not found in the library"),
        ("explicit.nckp", "explicit.nckp.nckp:"),
    ] {
        let (instrument, _) =
            fixture.compile(&format!("on init load_performance_view(\"{name}\") end on"));
        assert!(
            instrument.unsupported.iter().any(|u| {
                u.feature == "performance view"
                    && u.reason == ir::Reason::InvalidValue
                    && u.value.contains(detail)
            }),
            "{:?}",
            instrument.unsupported
        );
    }
}
