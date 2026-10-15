use sampler_ui_ir::{InterfacePatch, Kind, Rgba, TableFill, TableInnerEdge};
use sampler_uvi::script::{ScriptHost, control_id};

const XML: &str = include_str!("fixtures/table_inner_edge.uvip");

fn host(xml: &str) -> ScriptHost {
    ScriptHost::new(xml, (), Default::default()).unwrap()
}

#[test]
fn table_inner_edge_default_getter_colours_and_live_properties_export_exactly() {
    for (value, expected) in [
        ("red", Rgba::rgb(0xff0000)),
        ("#11cc33", Rgba::rgb(0x11cc33)),
        (
            "#8011cc33",
            Rgba {
                a: 0x80,
                ..Rgba::rgb(0x11cc33)
            },
        ),
    ] {
        let mut host = host(&XML.replace(
            "innerEdgeColour='red'",
            &format!("innerEdgeColour='{value}'"),
        ));
        assert_eq!(host.global_text("default_edge"), "true");
        let base = host.interface();
        assert!(base.unsupported.is_empty(), "{:?}", base.unsupported);
        assert_eq!(
            base.widgets[0].table_inner_edge,
            Some(TableInnerEdge {
                visible: true,
                color: Some(expected)
            })
        );
        let components = base.widgets[0].components.clone();
        for (value, visible) in [(1., false), (0., true)] {
            host.set_control(control_id(2, 0), value).unwrap();
            let changed = host.interface();
            assert_eq!(
                changed.widgets[0].table_inner_edge,
                Some(TableInnerEdge {
                    visible,
                    color: Some(Rgba::rgb(0x0000ff))
                })
            );
            assert_eq!(changed.widgets[0].components, components);
            let patch = InterfacePatch::between(&base, &changed);
            assert!(patch.widgets.iter().any(|(n, _)| *n == 0));
            let mut published = base.clone();
            patch.apply(&base, &Default::default(), &mut published);
            assert_eq!(published, changed);
            assert!(changed.unsupported.is_empty(), "{:?}", changed.unsupported);
        }
        host.set_control(components[2], 0.375).unwrap();
        assert_eq!(host.global_text("calls"), "1");
        assert_eq!(host.global_text("last_index"), "3");
        assert_eq!(host.global_text("last_value"), "0.375");
        assert!(
            matches!(&host.interface().widgets[0].kind, Kind::Table { cells, .. } if cells == &[1.,1.,0.375,1.])
        );
    }
}

#[test]
fn invalid_table_edges_remain_findings_and_do_not_mask_other_table_gaps() {
    for invalid in ["1", "'false'", "{}"] {
        let xml = XML.replace(
            "bars.innerEdgeColour='red'",
            &format!(
                "bars.drawInnerEdge={invalid}; bars.innerEdgeColour=123; bars.fillStyle='gloss'"
            ),
        );
        let face = host(&xml).interface();
        for feature in [
            "Table.drawInnerEdge",
            "Table.innerEdgeColour",
            "Table.fillStyle",
        ] {
            assert!(
                face.unsupported.iter().any(|row| row.feature == feature),
                "missing {feature}: {:?}",
                face.unsupported
            );
        }
        assert_eq!(
            face.widgets[0].table_inner_edge,
            Some(TableInnerEdge {
                visible: true,
                color: None
            })
        );
    }
    for invalid in ["false", "'not-a-colour'", "{}"] {
        let face = host(&XML.replace(
            "innerEdgeColour='red'",
            &format!("innerEdgeColour={invalid}"),
        ))
        .interface();
        assert!(
            face.unsupported
                .iter()
                .any(|row| row.feature == "Table.innerEdgeColour")
        );
    }
    let face = host(&XML.replace(
        "local bars=Table('Bars',4,1,0,1,false)",
        "local bars=Knob('Bars',1,0,1); bars.drawInnerEdge=false",
    ))
    .interface();
    assert_eq!(face.widgets[0].table_inner_edge, None);
    assert!(
        face.unsupported
            .iter()
            .any(|row| row.feature == "Knob.drawInnerEdge")
    );
    assert!(
        face.unsupported
            .iter()
            .any(|row| row.feature == "Knob.innerEdgeColour")
    );
}

#[test]
fn missing_native_edge_colour_uses_existing_fallback_without_inventing_ink() {
    let host = host(&XML.replace("bars.innerEdgeColour='red'", ""));
    assert_eq!(host.global_text("default_edge"), "true");
    let face = host.interface();
    assert_eq!(
        face.widgets[0].table_inner_edge,
        Some(TableInnerEdge {
            visible: true,
            color: None
        })
    );
    assert!(face.unsupported.is_empty(), "{:?}", face.unsupported);
    assert_eq!(face.widgets[0].table_fill, None);
}

#[test]
fn explicit_solid_fill_exports_and_live_gloss_remains_an_honest_finding() {
    let xml = XML
        .replace("bars.innerEdgeColour='red'", "bars.fillStyle='solid'; initial_fill=bars.fillStyle; bars.innerEdgeColour='red'")
        .replace("bars.drawInnerEdge=not self.value", "bars.fillStyle=self.value and 'gloss' or 'solid'; current_fill=bars.fillStyle; bars.drawInnerEdge=not self.value");
    let mut host = host(&xml);
    assert_eq!(host.global_text("initial_fill"), "solid");
    let base = host.interface();
    assert_eq!(base.widgets[0].table_fill, Some(TableFill::Solid));
    assert!(base.unsupported.is_empty(), "{:?}", base.unsupported);
    for (value, expected, readback) in [(1., None, "gloss"), (0., Some(TableFill::Solid), "solid")]
    {
        host.set_control(control_id(2, 0), value).unwrap();
        let face = host.interface();
        assert_eq!(host.global_text("current_fill"), readback);
        assert_eq!(face.widgets[0].table_fill, expected);
        assert_eq!(face.widgets[0].components, base.widgets[0].components);
        assert_eq!(
            host.global_text("calls"),
            "0",
            "a style write must not edit a table cell"
        );
        assert_eq!(
            face.unsupported
                .iter()
                .any(|row| row.feature == "Table.fillStyle"),
            expected.is_none()
        );
        let patch = InterfacePatch::between(&base, &face);
        let mut published = base.clone();
        patch.apply(&base, &Default::default(), &mut published);
        assert_eq!(published, face);
    }
    host.set_control(base.widgets[0].components[1], 0.375)
        .unwrap();
    assert_eq!(host.global_text("calls"), "1");
    assert_eq!(host.global_text("last_index"), "2");
    assert_eq!(host.global_text("last_value"), "0.375");
}

#[test]
fn gloss_unknown_fill_types_and_other_widget_kinds_remain_findings() {
    for invalid in ["'gloss'", "'unknown'", "true", "123", "{}"] {
        let xml = XML.replace(
            "bars.innerEdgeColour='red'",
            &format!("bars.fillStyle={invalid}; bars.innerEdgeColour='red'"),
        );
        let face = host(&xml).interface();
        assert_eq!(face.widgets[0].table_fill, None);
        assert!(
            face.unsupported
                .iter()
                .any(|row| row.feature == "Table.fillStyle")
        );
    }
    let xml = XML
        .replace(
            "local bars=Table('Bars',4,1,0,1,false)",
            "local bars=Knob('Bars',1,0,1)",
        )
        .replace(
            "bars.innerEdgeColour='red'",
            "bars.fillStyle='solid'; bars.innerEdgeColour='red'",
        );
    let face = host(&xml).interface();
    assert_eq!(face.widgets[0].table_fill, None);
    assert!(
        face.unsupported
            .iter()
            .any(|row| row.feature == "Knob.fillStyle")
    );
}
