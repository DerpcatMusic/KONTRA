use sampler_ui_ir::{Binding, Kind, Rect, WidgetRef};
use sampler_uvi::script::{Config, ScriptHost, control_id};
fn xml(script: &str) -> String {
    format!(
        "<UVI4><Program Name='P' Gain='1'><EventProcessors><ScriptProcessor Name='S'><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    )
}
fn host(script: &str) -> ScriptHost {
    ScriptHost::new(&xml(script), (), Config::default()).unwrap()
}

#[test]
fn geometry_and_typed_edits_reach_the_script_and_parameters() {
    let mut h = host(
        r#"
      setSize(600,300)
      local p=Panel{'Main',bounds={10,20,500,200}}
      local k=p:Knob('gain',0.25,0,1)
      k.bounds={5,6,40,40}; k.position={7,8}; k.size={50,60}
      k.changed=function(self) Program:setParameter('Gain',self.value) end
      local b=p:OnOffButton('Mute',false)
      b.changed=function(self) assert(type(self.value)=='boolean') end
      local m=p:Menu('Mode',{'A','B'},2)
      m:setItem(2,'C'); assert(m:addItem('D')==3)
      m.changed=function(self) assert(self.selectedText=='D') end
      local t=p:Table('Steps',3,0.25,0,1,false)
      t.changed=function(self,index) assert(index==2); assert(self:getValue(index)==0.375) end
      local v=Viewport{'Scroll',bounds={300,0,100,100}}
      v:setViewPosition(10,20); v:Panel('Contents')
      local positioned=Label{'Positioned',x=11,y=13}; assert(positioned.x==11 and positioned.y==13)
    "#,
    );
    let face = h.interface();
    face.validate().unwrap();
    assert_eq!(face.widgets[1].rect, Rect::new(7, 8, 50, 60));
    assert_eq!(face.widgets[1].parent, Some(WidgetRef(0)));
    assert_eq!(face.page_rect(WidgetRef(1)), Rect::new(17, 28, 50, 60));
    assert_eq!(face.widgets[1].binding, Binding::Control(control_id(2, 0)));
    h.set_control(control_id(2, 0), 0.625).unwrap();
    h.set_control(control_id(3, 0), 1.).unwrap();
    h.set_control(control_id(4, 0), 3.).unwrap();
    h.set_control(control_id(5, 2), 0.375).unwrap();
    let face = h.interface();
    assert_eq!(face.widgets[2].initial_value, 1.);
    assert!(matches!(&face.widgets[4].kind,Kind::Table{cells,..} if cells==&[0.25,0.375,0.25]));
    assert!(
        h.take_commands().iter().any(
            |c| matches!(c,sampler_uvi::script::Command::Parameter{value,..} if *value==0.625)
        )
    );
    assert!(h.set_control(control_id(5, 4), 0.3).is_err());
    assert!(h.set_control(control_id(2, 0), f64::NAN).is_err());
}

#[test]
fn callbacks_suppress_table_changes_and_ui_callbacks_cannot_yield() {
    let mut h = host(
        r#"
      calls=0
      local t=Table('Steps',2,0,0,1,false)
      t.changed=function(self,i) calls=calls+1 end
      t:setValue(1,0.125,false); assert(calls==0)
      t:setValue(2,0.25); assert(calls==1)
      local k=Knob('Gain',0,0,1)
      k.changed=function() wait(10) end
    "#,
    );
    assert_eq!(h.global_text("calls"), "1");
    assert!(h.set_control(control_id(2, 0), 0.5).is_err());
}

#[test]
fn persistence_restores_widgets_before_init_and_custom_data_after_init() {
    let script = r#"
      trace='body'
      local k=Knob('Gain',0,0,1)
      k.changed=function(self) trace=trace..'/widget'; assert(self.value==0.625) end
      local transient=Knob{'Scratch',0,0,1,persistent=false}
      function onInit() trace=trace..'/init' end
      function onSave() return {a=true,text='ok',[2]={0.125}} end
      function onLoad(data)
        assert(trace=='body/widget/init')
        assert(data.a and data.text=='ok' and data[2][1]==0.125)
        trace=trace..'/load'
      end
    "#;
    let mut a = host(script);
    a.set_control(control_id(1, 0), 0.625).unwrap();
    a.set_control(control_id(2, 0), 0.9).unwrap();
    let state = a.save_ui_state().unwrap();
    let encoded = serde_json::to_string(&state).unwrap();
    let state = serde_json::from_str(&encoded).unwrap();
    let b =
        ScriptHost::new_with_ui_state(&xml(script), (), Config::default(), Some(&state)).unwrap();
    assert_eq!(b.global_text("trace"), "body/widget/init/load");
    assert_eq!(b.control_values()[1].1, 0.);
    assert!(
        host("function onSave() local t={} t.self=t return t end")
            .save_ui_state()
            .is_err()
    );
    assert!(
        host("function onSave() return Program end")
            .save_ui_state()
            .is_err()
    );
}

#[test]
fn positional_widgets_restore_and_table_callbacks_keep_indices() {
    let source = xml(r#"
      calls=0
      local k=Knob('Gain',0,0,1); k.changed=function() calls=calls+1 end
      local t=Table('Steps',2,0,0,1,false)
      t.changed=function(self,i) calls=calls+1 end
      function onInit() assert(k.value==0.625); assert(t:getValue(2)==0.375); assert(calls==3) end
    "#)
    .replace("Name='S'", "Name='S' Gain='0.625' Steps='0.125 0.375'");
    let h = ScriptHost::new(&source, (), Config::default()).unwrap();
    assert!(
        h.findings().iter().all(|f| f.feature != "lua error"),
        "{:?}",
        h.findings()
    );
}

#[test]
fn script_thread_ui_edits_publish_values_text_and_saved_state() {
    let source = xml(r#"
      local k=Knob('Gain',0,0,1)
      local label=Label('State')
      k.changed=function(self) label.text=tostring(self.value) end
      function onSave() return {value=k.value} end
    "#);
    let (_thread, loaded) =
        sampler_uvi::scripted::ScriptThread::spawn(source, (), Config::default()).unwrap();
    assert!(loaded.ui.edit(control_id(1, 0), 0.625));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while loaded.ui.value(control_id(1, 0)) != Some(0.625)
        || loaded.ui.state().unwrap().widgets[0].2 != sampler_uvi::script::SavedValue::Number(0.625)
    {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(loaded.ui.interface().widgets[1].text, "0.625");
    assert!(!loaded.ui.edit(control_id(999, 0), 1.));
}

#[test]
fn engine_objects_are_not_widget_options_and_logical_values_keep_their_type() {
    let mut h = host(
        r#"
      local k=ParamKnob(Program,'Gain')
      k:setValue(0.625)
      assert(k.bound and k.value==0.625)
      local logical=ParameterValue(Program,'Name')
      logical.value='Renamed'
      assert(logical.value=='Renamed')
      local x=Knob('X',0,0,1)
      local y=Knob('Y',0,0,1)
      XY('X','Y')
    "#,
    );
    assert!(
        h.findings().iter().all(|f| f.feature != "lua error"),
        "{:?}",
        h.findings()
    );
    assert_eq!(h.interface().widgets[0].name, "Gain");
    assert_eq!(
        h.interface().widgets[4].components,
        vec![control_id(3, 0), control_id(4, 0)]
    );
    h.set_control(control_id(3, 0), 0.375).unwrap();
}
