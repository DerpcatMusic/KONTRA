use sampler_uvi::script::{Config, ScriptHost, control_id};

#[test]
fn v1_programmatic_values_preserve_display_range_independence_and_integer_truncation() {
    let script = r#"
      local ints=Knob{'ints',0,-5,5,true}
      ints:setValue(-1.8,false); assert(ints.value==-1)
      ints:setValue(6,false); assert(ints.value==6)
      local k=Knob{'range',0,0,1}; k:setValue(2,false); assert(k:getValue()==2)
      k.value=-2; assert(k.value==-2)
      local b=Button('Momentary')
      assert(b.value==nil)
      assert(b.setValue==nil and b.getValue==nil and b.setRange==nil)
      assert(not pcall(function() b.value=true end))
      local toggle=OnOffButton('Toggle',false)
      assert(not pcall(function() toggle:setValue(1) end))
      toggle.changed=function(self) assert(type(self.value)=='boolean') end
      loaded=true
    "#;
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let mut host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
    host.set_control(control_id(4, 0), 1.).unwrap();
    assert_eq!(host.interface().widgets[3].initial_value, 1.);
    assert!(host.findings().is_empty(), "{:?}", host.findings());
}

#[test]
fn v1_float32_values_defaults_and_writes_preserve_precision_and_error_atomicity() {
    let script = r#"
      local k=Knob{'Float',0.1,0.1,0.9}
      assert(k.value==0.10000000149011612 and k.default==k.value)
      assert(k.min==0.10000000149011612 and k.max==0.8999999761581421)
      calls=0; k.changed=function() calls=calls+1 end
      k:setValue(0.1); assert(calls==0)
      k:setValue(0.2); assert(k.value==0.20000000298023224 and calls==1)
      assert(not pcall(function() k:setValue(1e100) end))
      assert(k.value==0.20000000298023224 and calls==1)
      local t=Table{'Cells',2,0.1,0,1}
      assert(t:getValue(1)==0.10000000149011612 and t:getValue(0)==0.10000000149011612)
      t:setValue(1,0.2,false); assert(t:getValue(1)==0.20000000298023224)
      assert(not pcall(function() t:setValue(1,1e100,false) end))
      assert(t:getValue(1)==0.20000000298023224)
      loaded=true
    "#;
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
}

#[test]
fn parameter_widgets_reuse_catalog_default_instead_of_parameter_name() {
    let script = r#"
      local w=ParamKnob(Program,'Gain')
      local definition
      for _,p in ipairs(Program.parameterDefinitions) do if p.name=='Gain' then definition=p end end
      assert(definition and w.bound and w.default==definition.default)
      expected=definition.default; loaded=true
    "#;
    let xml = format!(
        "<UVI4><Program Gain='0.625'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    let host = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert_eq!(host.global_text("loaded"), "true");
    let face = host.interface();
    assert_eq!(face.widgets[0].initial_value, 0.625);
    assert!(
        matches!(&face.widgets[0].kind,sampler_ui_ir::Kind::Knob{range,..} if range.default==host.global_text("expected").parse::<f64>().unwrap())
    );
}
