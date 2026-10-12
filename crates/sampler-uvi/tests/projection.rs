use sampler_ui_ir::{PageRef, WidgetRef};
use sampler_uvi::script::{Config, ScriptHost};
#[test]
fn authored_fractional_geometry_and_child_order_preserve_declaration_identity() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      setSize(320.5,160.25)
      a=Panel{'a',bounds={10.25,20.5,60.75,30.125}}
      b=Panel{'b',bounds={100.5,0,100,90}}
      a1=a:Knob{'a1',bounds={1.5,2.25,10.125,9.75}}
      b1=b:Table{'b1',4,0,0,1};a2=a:Knob{'a2',0,0,1}
      stage=0
      function onController(e)
        stage=stage+1
        if stage==1 then a.children={a2,a1} else a1.parent=b end
      end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    let before = host.interface();
    let r = before.widgets[0].rect;
    assert_eq!(
        [r.x as f64, r.y as f64, r.width as f64, r.height as f64],
        [10.25, 20.5, 60.75, 30.125]
    );
    assert_eq!(
        [
            before.pages[0].size.width as f64,
            before.pages[0].size.height as f64
        ],
        [320.5, 160.25]
    );
    let order = |face: &sampler_ui_ir::Interface| {
        face.draw_order(PageRef(0))
            .iter()
            .map(|w| w.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&before), [0, 2, 4, 1, 3]);
    host.controller(1, 64, 0);
    let reordered = host.interface();
    assert_eq!(order(&reordered), [0, 4, 2, 1, 3]);
    host.controller(1, 65, 0);
    let moved = host.interface();
    assert_eq!(order(&moved), [0, 4, 1, 3, 2]);
    assert_eq!(moved.widgets[2].parent, Some(WidgetRef(1)));
    assert_eq!(moved.page_rect(WidgetRef(2)).x as f64, 102.);
    assert_eq!(
        moved
            .widgets
            .iter()
            .map(|w| w.source_id)
            .collect::<Vec<_>>(),
        before
            .widgets
            .iter()
            .map(|w| w.source_id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn authored_child_order_changes_without_reassigning_control_ids() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      a=Panel{'a'}; b=Panel{'b'}
      a1=a:Knob{'a1',0,0,1}; b1=b:Table{'b1',4,0,0,1}; a2=a:Knob{'a2',0,0,1}
      stage=0
      function onController(e)
        stage=stage+1
        if stage==1 then a.children={a2,a1}
        elseif stage==2 then a1.parent=b
        else a.id=999; a1.parent=a end
      end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let before = host.interface();
    let order = |face: &sampler_ui_ir::Interface| {
        face.draw_order(PageRef(0))
            .iter()
            .map(|w| w.0)
            .collect::<Vec<_>>()
    };
    assert_eq!(order(&before), [0, 2, 4, 1, 3]);
    host.controller(1, 64, 0);
    assert_eq!(order(&host.interface()), [0, 4, 2, 1, 3]);
    host.controller(1, 65, 0);
    assert_eq!(order(&host.interface()), [0, 4, 1, 3, 2]);
    host.controller(1, 66, 0);
    let moved = host.interface();
    assert_eq!(moved.widgets[2].parent, Some(WidgetRef(0)));
    assert_eq!(
        moved
            .widgets
            .iter()
            .map(|w| w.binding.clone())
            .collect::<Vec<_>>(),
        before
            .widgets
            .iter()
            .map(|w| w.binding.clone())
            .collect::<Vec<_>>()
    );
}

#[test]
fn invalid_authored_trees_report_fixed_diagnostics_without_painting() {
    for mutation in [
        "a.parent=b; b.parent=a",
        "a.children={b,b}",
        "a.children={a}",
        "a.children={false}",
        "a.children={foreign}",
        "a.parent=foreign",
        "a.children={[2]=b}",
        "a.children={b,named=b}",
    ] {
        let xml = format!(
            "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[a=Panel{{'a'}};b=a:Knob{{'b',0,0,1}};foreign={{}};{mutation}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
        let face = host.interface();
        assert!(face.widgets.is_empty(), "invalid tree painted: {mutation}");
        assert!(
            host.findings()
                .iter()
                .any(|f| f.feature == "lua UI snapshot"),
            "missing diagnostic: {mutation}"
        );
    }
}

#[test]
fn authored_tree_above_4096_nodes_keeps_every_widget() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      p=Panel{'Root'};for i=1,6480 do p:Knob{tostring(i),0,0,1} end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let face = host.interface();
    assert_eq!(face.widgets.len(), 6481);
    assert_eq!(face.draw_order(PageRef(0)).len(), 6481);
    assert!(host.findings().is_empty(), "{:?}", host.findings());
}

#[test]
fn invalid_geometry_is_rejected_before_snapshot_publication() {
    for mutation in [
        "a.x=0/0",
        "a.y=math.huge",
        "a.width=-1",
        "a.height=math.huge",
        "setSize(0/0,100)",
        "setSize(100,-1)",
        "a.x=1e308;b.x=1e308",
    ] {
        let xml = format!(
            "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[a=Panel{{'a'}};b=a:Panel{{'b'}};{mutation}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
        );
        let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
        let face = host.interface();
        assert!(
            face.widgets.is_empty(),
            "invalid geometry published: {mutation}"
        );
        assert!(
            host.findings()
                .iter()
                .any(|f| f.feature == "lua UI snapshot"),
            "missing diagnostic: {mutation}"
        );
    }
}

#[test]
fn fractional_nested_widget_edit_readback_and_restore_keep_physical_identities() {
    use sampler_uvi::script::control_id;
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      setSize(320.5,160.25)
      outer=Panel{'outer',bounds={10.25,20.5,100.75,80.125}}
      inner=outer:Panel{'inner',bounds={1.5,2.25,60.5,50.25}}
      knob=inner:Knob{'knob',0.25,0,1,bounds={3.125,4.5,24.75,24.125}}
      calls=0;knob.changed=function(self) calls=calls+1; inner.x=1.75 end
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let before = host.interface();
    assert_eq!(before.page_rect(WidgetRef(2)).x, 14.875);
    let identity = before.widgets[2].binding.clone();
    host.set_control(control_id(3, 0), 0.625).unwrap();
    let edited = host.interface();
    assert_eq!(edited.widgets[2].initial_value, 0.625);
    assert_eq!(edited.page_rect(WidgetRef(2)).x, 15.125);
    assert_eq!(edited.widgets[2].binding, identity);
    assert_eq!(host.global_text("calls"), "1");
    let saved = host.save_ui_state().unwrap();
    let restored = ScriptHost::new_with_ui_state(xml, (), Config::default(), Some(&saved)).unwrap();
    let after = restored.interface();
    assert_eq!(
        after.page_rect(WidgetRef(2)),
        edited.page_rect(WidgetRef(2))
    );
    assert_eq!(after.widgets[2].initial_value, 0.625);
    assert_eq!(after.widgets[2].binding, identity);
    assert!(restored.findings().is_empty(), "{:?}", restored.findings());
}

#[test]
fn finite_negative_parent_relative_positions_remain_admissible() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[
      setSize(320.5,160.25)
      outer=Panel{'outer',bounds={-10.25,-20.5,100.75,80.125}}
      inner=outer:Panel{'inner',bounds={3.5,4.25,60.5,50.25}}
      inner:Knob{'knob',0.25,0,1,bounds={2.5,5.125,24.75,24.125}}
    ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let host = ScriptHost::new(xml, (), Config::default()).unwrap();
    let face = host.interface();
    assert_eq!(face.validate(), Ok(()));
    let rect = face.page_rect(WidgetRef(2));
    assert_eq!((rect.x, rect.y), (-4.25, -11.125));
    assert_eq!((rect.width, rect.height), (24.75, 24.125));
    assert!(host.findings().is_empty(), "{:?}", host.findings());
}
