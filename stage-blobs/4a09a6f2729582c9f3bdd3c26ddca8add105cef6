use sampler_ui_ir::{Rect, WidgetRef};
use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn named_geometry_uses_v1_size_position_pos_bounds_order() {
    let script = r#"for i=1,64 do
      Knob{'Authored'..i, x=1,y=2,width=3,height=4,
        size={20,21},position={30,31},pos={40,41},bounds={50,51,60,61}}
    end"#;
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    let face = host.interface();
    for i in 0..64 {
        assert_eq!(face.page_rect(WidgetRef(i)), Rect::new(50, 51, 60, 61));
    }
}

#[test]
fn authored_named_parent_and_container_children_reach_shared_geometry() {
    let script = r#"
      local root=Panel{'Root',bounds={10,20,200,150}}
      local nested=Viewport{'Nested',parent=root,bounds={30,40,100,90}}
      local child=root:Knob{'Child',parent=nested,bounds={5,6,20,21}}
      assert(nested.parent==root and child.parent==nested)
      assert(#root.children==1 and root.children[1]==nested)
      assert(#nested.children==1 and nested.children[1]==child)
      local plain=root:Label{'Plain',bounds={7,8,30,31}}
      assert(plain.parent==root and root.children[2]==plain)
      assert(not pcall(function() child:Label('Invalid') end))
      assert(not pcall(root.Knob,child,'InvalidFactory'))
      loaded=true
    "#;
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
    let face = host.interface();
    assert_eq!(face.widgets[1].parent, Some(WidgetRef(0)));
    assert_eq!(face.widgets[2].parent, Some(WidgetRef(1)));
    assert_eq!(face.page_rect(WidgetRef(2)), Rect::new(45, 66, 20, 21));
    assert_eq!(face.page_rect(WidgetRef(3)), Rect::new(17, 28, 30, 31));
}
