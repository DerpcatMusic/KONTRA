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
fn performance_intent_reaches_ir_from_init_and_widget_callbacks() {
    assert!(!host("Panel{'Auxiliary'}").interface().performance);
    let face = host("makePerformanceView(); setBackground('wallpaper.png')").interface();
    assert!(face.performance);
    assert!(face.widgets.is_empty());
    assert!(face.pages[0].background.image.is_some());
    let mut h = host("local k=Knob{'K',0,0,1}; k.changed=function() makePerformanceView() end");
    assert!(!h.interface().performance);
    h.set_control(control_id(1, 0), 0.5).unwrap();
    assert!(h.interface().performance);
}

#[test]
fn v1_widgets_keep_script_coordinates_without_constructor_grid() {
    let h = host(r#"
      local p=Panel('Root')
      local first=p:Knob('First')
      local second=p:Knob('Second')
      assert(first.x==0 and first.y==0 and second.x==0 and second.y==0)
      p.position={40,60}; second.position={13,17}
      Label{'Origin',x=0,y=0}
      loaded=true
    "#);
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
    let face=h.interface();
    let first=face.page_rect(WidgetRef(1));
    let second=face.page_rect(WidgetRef(2));
    assert_eq!((first.x,first.y),(40.,60.));
    assert_eq!((second.x,second.y),(53.,77.));
    assert_eq!((face.widgets[3].rect.x,face.widgets[3].rect.y),(0.,0.));
}

#[test]
fn v1_set_height_keeps_width_and_loads_following_widgets() {
    let h = host("setSize(600,300); setHeight(240); Label('AfterHeight'); loaded=true");
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
    let face = h.interface();
    let size = face.pages[0].size;
    assert_eq!((size.width, size.height), (600., 240.));
    assert_eq!(face.widgets[0].name, "AfterHeight");
}

#[test]
fn v1_button_push_calls_changed_with_optional_modifiers_without_latching() {
    let mut h = host(r#"
      calls=0; modifiers='unset'
      local b=Button('Trigger')
      b.changed=function(self,mods) calls=calls+1; modifiers=mods end
      b:push(false); assert(calls==0)
      b:push(true,7); assert(calls==1 and modifiers==7)
      b:push(true); assert(calls==2 and modifiers==nil)
      assert(not pcall(function() b:push() end))
      assert(not pcall(function() Knob('K'):push(true) end))
      loaded=true
    "#);
    assert!(h.findings().is_empty(), "{:?}", h.findings());
    assert_eq!(h.global_text("loaded"), "true");
    h.set_control(control_id(1, 0), 1.).unwrap();
    h.set_control(control_id(1, 0), 0.).unwrap();
    h.set_control(control_id(1, 0), 1.).unwrap();
    assert_eq!(h.global_text("calls"), "4");
    assert_eq!(h.interface().widgets[0].initial_value, 0.);
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
      local mapped=Knob{'Mapped',100,1,10000,mapper=Mapper.Exponential}
      mapped:setValueNormalized(0.5,false); assert(math.abs(mapped.value-100)<1e-9)
      mapped.mapper=Mapper.QuinticRoot; mapped.min=0; mapped.max=1
      mapped:setValueNormalized(0.03125,false); assert(math.abs(mapped.value-0.5)<1e-9)
      assert(mapped.default==100)
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
fn initial_on_load_sees_restored_widget_value_in_visible_ui() {
    let script = r#"
      local gain=Knob('Gain',0,0,1)
      local readout=Label('Restored gain')
      function onSave() return {suffix=' loaded'} end
      function onLoad(data) readout.text=tostring(gain.value)..data.suffix end
    "#;
    let mut original = host(script);
    original.set_control(control_id(1, 0), 0.625).unwrap();
    let state = original.save_ui_state().unwrap();

    let restored =
        ScriptHost::new_with_ui_state(&xml(script), (), Config::default(), Some(&state)).unwrap();
    assert_eq!(restored.interface().widgets[1].text, "0.625 loaded");
}

#[test]
fn persistence_restores_widgets_before_custom_data_and_init() {
    let script = r#"
      trace='body'
      local k=Knob('Gain',0,0,1)
      k.changed=function(self) trace=trace..'/widget'; assert(self.value==0.625) end
      local transient=Knob{'Scratch',0,0,1,persistent=false}
      function onInit() trace=trace..'/init' end
      function onSave() return {a=true,text='ok',[2]={0.125}} end
      function onLoad(data)
        assert(trace=='body/widget' and k.value==0.625)
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
    assert_eq!(b.global_text("trace"), "body/widget/load/init");
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
fn ready_snapshot_includes_widget_changes_from_on_save() {
    let source = xml(
        "local k=Knob('Gain',0.25,0,1); function onSave() k:setValue(0.75,false); return {} end",
    );
    let (_thread, loaded) =
        sampler_uvi::scripted::ScriptThread::spawn(source, (), Config::default()).unwrap();
    assert_eq!(loaded.ui.value(control_id(1, 0)), Some(0.75));
    assert_eq!(*loaded.ui.interface(), loaded.interface);
    assert_eq!(
        loaded.ui.state().unwrap().widgets[0].2,
        sampler_uvi::script::SavedValue::Number(0.75)
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

#[test]
fn script_thread_publishes_runtime_faults_without_widget_changes() {
    use sampler_uvi::scripted::Script;
    let source = xml("function onNote(e) error('first runtime fault') end");
    let (mut thread, loaded) =
        sampler_uvi::scripted::ScriptThread::spawn(source, (), Config::default()).unwrap();
    let revision = loaded.ui.revision();
    thread.note_on(1, 60, 100);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while loaded.ui.revision() == revision && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        loaded.ui.revision() > revision,
        "runtime findings must wake report consumers"
    );
    assert_eq!(loaded.ui.runtime_faults(), (1, 0));
    assert_eq!(
        loaded.ui.fault_counts().runtime[&sampler_uvi::script::FaultCategory::Lua],
        1
    );
    assert!(loaded.ui.findings().iter().any(|f| f.feature == "lua error"
        && f.count == 1
        && f.value.contains("first runtime fault")));
}

fn live_thread_note_state(burst: usize) {
    use sampler_uvi::{script::SavedValue, scripted::Script};
    let source = xml(&format!(
        "local notes=0; function onNote(e) notes=notes+1; for i=1,{burst} do postEvent{{type=Event.Controller,controller=1,value=17}} end end; function onSave() return {{notes=notes}} end"
    ));
    let (mut owner, loaded) =
        sampler_uvi::scripted::ScriptThread::spawn(source, (), Config::default()).unwrap();
    let revision = loaded.ui.revision();
    owner.note_on(1, 60, 100);
    let mut commands = Vec::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while commands.is_empty() {
        owner.drain(&mut commands);
        assert!(
            std::time::Instant::now() < deadline,
            "calibrate that onNote completed"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if burst > 1024 {
        assert!(commands.len() < burst, "calibrate undrained audio commands");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        loaded.ui.revision(),
        revision,
        "the state change must have no UI revision: {:?}",
        loaded.ui.findings()
    );
    let state = loaded
        .ui
        .state()
        .expect("host saves must complete even when audio is paused");
    assert_eq!(
        state.custom,
        Some(SavedValue::Table(vec![(
            SavedValue::String("notes".into()),
            SavedValue::Number(1.)
        )])),
        "onSave must read live note state"
    );
}

#[test]
fn script_thread_state_runs_on_save_after_note_without_ui_revision() {
    live_thread_note_state(1);
}

#[test]
fn script_thread_state_survives_command_backpressure() {
    live_thread_note_state(4096);
}

#[test]
fn skinned_widgets_caption_only_what_the_script_asks_for() {
    let face = host(r#"
      local a=Knob{'Plain',0,0,1,unit=Unit.Percent}
      local b=Knob{'Skinned',0,0,1,unit=Unit.Percent}; b:setStripImage('k.png',10)
      local c=Knob{'Asked',0,0,1,showLabel=true,showValue=true}; c:setStripImage('k.png',10)
      local d=Knob{'Text',0,0,1,displayText='x'}; d:setStripImage('k.png',10)
    "#).interface();
    let hidden = |i: usize| (face.widgets[i].hide.title, face.widgets[i].hide.value);
    assert_eq!(hidden(0), (false, false));
    assert_eq!(hidden(1), (true, true));
    assert_eq!(hidden(2), (false, false));
    assert_eq!(hidden(3), (true, false));
}

#[test]
fn snapshot_of_a_large_ui_stays_cheap() {
    // Absent optional properties used to scan every widget kind in Lua, which
    // made each snapshot of a 300-widget UI cost several frames.
    let h = host("for i=1,300 do local k=Knob{'K'..i,0,0,1}; k:setStripImage('k.png',10) end");
    let start = std::time::Instant::now();
    for _ in 0..20 {
        assert_eq!(h.interface().widgets.len(), 300);
    }
    let each = start.elapsed() / 20;
    assert!(each < std::time::Duration::from_millis(40), "{each:?}");
}


#[test]
fn menu_text_is_current_selected_text_in_real_changed_parameter_callback() {
    use sampler_uvi::script::{Command,Param,Scope};
    let mut h=host(r#"
      local menu=Menu('Dynamics',{'Soft','Loud'},2)
      menu.changed=function(self)
        chosen=self.text
        Program:setParameter('Gain',self.text=='Soft' and 0.25 or 0.75)
      end
    "#);
    assert!(h.take_commands().is_empty());
    h.set_control(control_id(1,0),1.).unwrap();
    assert_eq!(h.global_text("chosen"),"Soft");
    let commands=h.take_commands();
    assert!(matches!(&commands[..],[Command::Parameter{scope:Scope::Program,param:Param::Gain,value,authored}] if *value==0.25&&*authored==1.));
    h.set_control(control_id(1,0),2.).unwrap();
    assert_eq!(h.global_text("chosen"),"Loud");
    assert!(matches!(&h.take_commands()[..],[Command::Parameter{scope:Scope::Program,param:Param::Gain,value,..}] if *value==0.75));
    assert!(h.findings().is_empty(),"{:?}",h.findings());
}

#[test]
fn menu_text_choice_changes_normal_driver_runtime_pcm_from_same_held_source() {
    use sampler_core::{Envelope,Input,Limits,Pcm,Playback,Prepared,Protocol,Region,Runtime};
    use sampler_uvi::scripted::Driver;
    fn runtime()->Runtime {
        let pcm=Pcm::new(48_000,vec![[0.25,-0.125];1024].into_boxed_slice()).unwrap();
        let region=Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,
            envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default()};
        let plan=Prepared::new(48_000,vec![pcm],vec![region],1).unwrap();
        let limits=Limits::for_plan(&plan,4,4);Runtime::new(plan,limits).unwrap()
    }
    let mut candidate=runtime();let mut reference=runtime();
    let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(1)};
    let note=candidate.trigger(input,60,1.).unwrap();reference.trigger(input,60,1.).unwrap();
    let mut before=[[0.;2];32];let mut expected_before=[[0.;2];32];
    candidate.render(&mut before).unwrap();reference.render(&mut expected_before).unwrap();assert_eq!(before,expected_before);
    let mut h=host(r#"
      local menu=Menu('Dynamics',{'Soft','Loud'},2)
      menu.changed=function(self) Program:setParameter('Gain',self.text=='Soft' and 0.25 or 0.75) end
    "#);
    h.set_control(control_id(1,0),1.).unwrap();
    let mut driver=Driver::new(h,vec![],48_000);driver.wake(&mut candidate).unwrap();
    let mut got=[[0.;2];64];let mut wanted=[[0.;2];64];candidate.render(&mut got).unwrap();reference.render(&mut wanted).unwrap();
    assert!(wanted.iter().flatten().any(|sample|sample.abs()>0.01));
    // Existing Program gain transport quantizes to one millidecibel; the
    // tolerance admits that law while strongly separating quarter from 3/4.
    for (a,b) in got.iter().flatten().zip(wanted.iter().flatten()){
        assert!((f64::from(*a)-f64::from(*b)*0.25).abs()<2e-5,"selected menu text must choose quarter-gain PCM, got {a}, unity reference {b}");
    }
    assert_eq!(candidate.input_held(note),Ok(true));assert_eq!(candidate.voice_count(),1);
    assert_eq!(candidate.stats().nonfinite_frames,0);assert_eq!(candidate.stats().voice_pool_refusals,0);
}

#[test]
fn menu_text_and_get_text_follow_current_items_and_return_empty_for_missing_entry() {
    let h=host(r#"
      local menu=Menu('Mode',{'One','Two'},2)
      initial=menu.text
      menu:setItem(2,'Updated')
      updated=menu.text
      matching=menu.text==menu.selectedText and menu.text==menu:getText(menu.value)
      missingLow=type(menu:getText(0))..':'..tostring(menu:getText(0))
      missingHigh=type(menu:getText(3))..':'..tostring(menu:getText(3))
      menu:clear()
      cleared=menu.text
      local label=Label('Status');label.text='authored text';labelText=label.text
    "#);
    assert_eq!(h.global_text("initial"),"Two");assert_eq!(h.global_text("updated"),"Updated");
    assert_eq!(h.global_text("matching"),"true");assert_eq!(h.global_text("missingLow"),"string:");
    assert_eq!(h.global_text("missingHigh"),"string:");assert_eq!(h.global_text("cleared"),"");
    assert_eq!(h.global_text("labelText"),"authored text");assert!(h.findings().is_empty(),"{:?}",h.findings());
}


#[test]
fn widget_w_h_aliases_share_real_dimensions_and_authored_parent_geometry() {
    let h=host(r#"
      local p=Panel{'Parent',bounds={10,20,200,100}}
      local k=p:Knob('Gain',0,0,1)
      k.w=31;k.h=47;k.position={3,5}
      aliasReads=k.w==k.width and k.h==k.height and k.size[1]==31 and k.bounds[4]==47
      k.width=42;k.height=53
      reverseReads=k.w==42 and k.h==53
      local label=Label{'Named',w=19,h=23,x=60,y=7}
      local winner=Label{'Winner',w=9,h=11,bounds={70,8,29,33}}
      local image=Image('authored.png');image.w=91;image.h=37
    "#);
    assert_eq!(h.global_text("aliasReads"),"true");assert_eq!(h.global_text("reverseReads"),"true");
    let face=h.interface();face.validate().unwrap();
    assert_eq!(face.widgets[1].rect,Rect::new(3,5,42,53));assert_eq!(face.page_rect(WidgetRef(1)),Rect::new(13,25,42,53));
    assert_eq!(face.widgets[2].rect,Rect::new(60,7,19,23));assert_eq!(face.widgets[3].rect,Rect::new(70,8,29,33));
    assert_eq!(face.widgets[4].rect.width,91.);assert_eq!(face.widgets[4].rect.height,37.);
    assert!(h.findings().is_empty(),"{:?}",h.findings());
}

#[test]
fn table_length_growth_publishes_default_cells_bindings_and_real_new_cell_callback() {
    use sampler_uvi::script::{Command,Param,Scope};
    let mut h=host(r#"
      local steps=Table('Steps',2,0.25,0,1,false);steps:setValue(1,0.75,false);steps.default=0.5
      local grow=Button('Grow');grow.changed=function() steps.length=4 end
      steps.changed=function(self,index) changedIndex=index;Program:setParameter('Gain',self:getValue(index)) end
    "#);
    let before=h.ui_revision();h.set_control(control_id(2,0),1.).unwrap();assert!(h.ui_revision()>before);
    let face=h.interface();face.validate().unwrap();
    assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:4,cells,..}if cells==&[0.75,0.25,0.5,0.5]));
    assert_eq!(face.widgets[0].components,(1..=4).map(|i|control_id(1,i)).collect::<Vec<_>>());
    assert!(h.control_values().contains(&(control_id(1,4),0.5)));
    h.set_control(control_id(1,4),0.625).unwrap();assert_eq!(h.global_text("changedIndex"),"4");
    assert!(matches!(&h.take_commands()[..],[Command::Parameter{scope:Scope::Program,param:Param::Gain,value,..}]if *value==0.625));
    assert!(h.findings().is_empty(),"{:?}",h.findings());
}

#[test]
fn table_length_shrink_and_regrow_discard_removed_cells_and_save_only_current_shape() {
    let mut h=host(r#"
      local steps=Table('Steps',4,0.125,0,1,false);steps:setValue(1,0.75,false);steps:setValue(4,0.875,false)
      local resize=Button('Resize');resize.changed=function()
        if steps.length==4 then steps.length=2 else steps.default=0.375;steps.length=4 end
      end
    "#);
    h.set_control(control_id(2,0),1.).unwrap();let face=h.interface();
    assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:2,cells,..}if cells==&[0.75,0.125]));
    assert_eq!(face.widgets[0].components,vec![control_id(1,1),control_id(1,2)]);
    let saved=h.save_ui_state().unwrap();let (_,_,sampler_uvi::script::SavedValue::Table(values))=&saved.widgets[0]else{panic!("actual Table saved state")};
    assert_eq!(values.len(),2,"removed cells cannot survive in the saved value array");
    h.set_control(control_id(2,0),1.).unwrap();let face=h.interface();
    assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:4,cells,..}if cells==&[0.75,0.125,0.375,0.375]));
    assert_eq!(h.control_values().iter().filter(|(id,_)|(1..=4).any(|i|*id==control_id(1,i))).count(),4);
}

#[test]
fn table_length_zero_and_invalid_host_shapes_do_not_publish_partial_value_arrays() {
    let h=host(r#"
      local steps=Table('Steps',2,0.25,0,1,false);steps:setValue(2,0.75,false)
      local original=steps.values
      for _,bad in ipairs({-1,0.5,math.huge,0/0,'3',4294967296}) do
        assert(not pcall(function() steps.length=bad end))
        assert(steps.length==2 and steps.values==original and steps:getValue(1)==0.25 and steps:getValue(2)==0.75)
      end
      steps.length=0;emptyLength=steps.length;steps.default=0.5;steps.length=2
      completed=true
    "#);
    assert_eq!(h.global_text("completed"),"true");assert_eq!(h.global_text("emptyLength"),"0");
    let face=h.interface();assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:2,cells,..}if cells==&[0.5,0.5]));
    assert!(h.findings().is_empty(),"{:?}",h.findings());
}


#[test]
fn resized_table_values_roundtrip_through_real_save_and_restore_callbacks() {
    for (initial,target) in [(2,4),(4,2)] {
        let script=format!(r#"
          calls=0
          local steps=Table('Steps',{initial},0.25,0,1,false);steps.length={target}
          steps.changed=function(self,index) calls=calls+1;lastIndex=index;lastValue=self:getValue(index) end
        "#);
        let source=xml(&script);let mut original=ScriptHost::new(&source,(),Config::default()).unwrap();
        original.set_control(control_id(1,1),0.5).unwrap();original.set_control(control_id(1,target),0.625).unwrap();
        let saved=original.save_ui_state().unwrap();let bytes=serde_json::to_vec(&saved).unwrap();
        let state:sampler_uvi::script::UiState=serde_json::from_slice(&bytes).unwrap();
        let restored=ScriptHost::new_with_ui_state(&source,(),Config::default(),Some(&state)).unwrap();
        let face=restored.interface();face.validate().unwrap();let mut expected=vec![0.25;target];expected[0]=0.5;expected[target-1]=0.625;
        assert!(matches!(&face.widgets[0].kind,Kind::Table{columns,cells,..}if *columns==target as u32&&cells==&expected));
        assert_eq!(restored.global_text("calls"),"2");assert_eq!(restored.global_text("lastIndex"),target.to_string());assert_eq!(restored.global_text("lastValue"),"0.625");
        assert_eq!(restored.control_values(),expected.iter().enumerate().map(|(i,value)|(control_id(1,i+1),*value)).collect::<Vec<_>>());
        assert_eq!(restored.save_ui_state().unwrap(),saved);assert!(restored.findings().is_empty(),"{:?}",restored.findings());
        assert_ne!(control_id(1,u32::MAX as usize),control_id(2,0),"component occupies exactly the low32 bits without a widget alias");
    }
}

#[test]
fn table_resize_work_exhaustion_keeps_prior_shape_and_cells() {
    let source=xml(r#"
      local steps=Table('Steps',2,0.25,0,1,false);steps:setValue(2,0.75,false)
      local resize=Button('Resize');resize.changed=function() entered=true;steps.length=1000000;finished=true end
    "#);
    let mut h=ScriptHost::new(&source,(),Config{callback_work:4096,..Config::default()}).unwrap();
    let error=h.set_control(control_id(2,0),1.).unwrap_err();assert!(error.contains("work budget exceeded"),"{error}");
    assert_eq!(h.global_text("entered"),"true");assert_eq!(h.global_text("finished"),"nil");
    let face=h.interface();assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:2,cells,..}if cells==&[0.25,0.75]));
    assert_eq!(face.widgets[0].components,vec![control_id(1,1),control_id(1,2)]);
}

#[test]
fn table_resize_vm_memory_failure_keeps_prior_shape_and_cells() {
    let source=xml(r#"
      local steps=Table('Steps',2,0.25,0,1,false);steps:setValue(2,0.75,false)
      local resize=Button('Resize');resize.changed=function() entered=true;steps.length=16777216;finished=true end
    "#);
    let mut h=ScriptHost::new(&source,(),Config{memory:16<<20,callback_work:1<<25,..Config::default()}).unwrap();
    let error=h.set_control(control_id(2,0),1.).unwrap_err();assert!(error.to_lowercase().contains("memory"),"{error}");
    assert_eq!(h.global_text("entered"),"true");assert_eq!(h.global_text("finished"),"nil");
    let face=h.interface();assert!(matches!(&face.widgets[0].kind,Kind::Table{columns:2,cells,..}if cells==&[0.25,0.75]));
    assert_eq!(face.widgets[0].components,vec![control_id(1,1),control_id(1,2)]);
}
