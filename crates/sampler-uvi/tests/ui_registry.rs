//! Clear authored normal ScriptThread UI actions. No bank/native/source export.
use sampler_ui_ir::{Binding,Interface,Kind};
use sampler_uvi::script::{Command,Config,Param,Scope};
use sampler_uvi::scripted::{Script,ScriptThread,UiBridge};
use std::{sync::Arc,time::{Duration,Instant}};
fn source(script:&str)->String{format!("<UVI4><Program Name='UI' Gain='1'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>")}
fn wait_face(ui:&UiBridge,predicate:impl Fn(&Interface)->bool)->Arc<Interface>{
    let deadline=Instant::now()+Duration::from_secs(3);
    loop{let face=ui.interface();if predicate(&face){return face;}assert!(Instant::now()<deadline,"actual owner did not publish expected UI");std::thread::sleep(Duration::from_millis(2));}
}
#[test]
fn callback_created_published_knob_accepts_real_ui_edit_and_emits_its_original_gain_command(){
    let (mut thread,loaded)=ScriptThread::spawn(source(r#"
      local create=Button('Create');local state=Label('State');state.text='before'
      create.changed=function()
        if made then return end;made=true
        local gain=Knob('MadeGain',0,0,1)
        gain.changed=function(self) state.text=tostring(self.value);Program:setParameter('Gain',self.value) end
      end
    "#),(),Config::default()).unwrap();
    let initial=loaded.ui.interface();let Binding::Control(create)=initial.widgets[0].binding else{panic!("actual initial Button binding")};
    assert!(loaded.ui.edit(create,1.));
    let face=wait_face(&loaded.ui,|face|face.widgets.iter().any(|w|w.name=="MadeGain"));face.validate().unwrap();
    let made=face.widgets.iter().find(|w|w.name=="MadeGain").unwrap();let Binding::Control(gain)=made.binding else{panic!("actual published Knob binding")};
    assert_eq!(loaded.ui.value(gain),Some(0.),"newly visible control must belong to the actual owner readback roster");
    assert!(loaded.ui.edit(gain,0.625),"no guessed ID bypass: edit uses actual published Binding");
    wait_face(&loaded.ui,|face|face.widgets.iter().any(|w|w.name=="State"&&w.text=="0.625"));
    let deadline=Instant::now()+Duration::from_secs(3);let mut commands=Vec::new();
    loop{thread.drain(&mut commands);if commands.iter().any(|c|matches!(c,Command::Parameter{scope:Scope::Program,param:Param::Gain,value,..}if *value==0.625)){break;}
        assert!(Instant::now()<deadline,"original dynamic widget changed did not emit its real Gain command");std::thread::sleep(Duration::from_millis(2));}
}
#[test]
fn callback_grown_table_published_new_component_is_actionable_through_normal_ui_bridge(){
    let (_thread,loaded)=ScriptThread::spawn(source(r#"
      local steps=Table('Steps',2,0.25,0,1,false);local grow=Button('Grow');local state=Label('State')
      grow.changed=function() steps.length=3 end
      steps.changed=function(self,index) state.text=tostring(index)..':'..tostring(self:getValue(index)) end
    "#),(),Config::default()).unwrap();
    let initial=loaded.ui.interface();let Binding::Control(grow)=initial.widgets[1].binding else{panic!("actual initial Button binding")};assert!(loaded.ui.edit(grow,1.));
    let face=wait_face(&loaded.ui,|face|matches!(face.widgets[0].kind,Kind::Table{columns:3,..}));
    assert!(matches!(&face.widgets[0].kind,Kind::Table{cells,..}if cells==&[0.25,0.25,0.25]),"requires independently accepted Table resizing before classifying a registry failure");
    let component=*face.widgets[0].components.last().unwrap();assert_eq!(loaded.ui.value(component),Some(0.25));
    assert!(loaded.ui.edit(component,0.625));
    wait_face(&loaded.ui,|face|face.widgets.iter().any(|w|w.name=="State"&&w.text=="3:0.625"));
}

fn named_control(face:&Interface,name:&str)->sampler_ui_ir::ControlId {
    let widget=face.widgets.iter().find(|widget|widget.name==name).unwrap();
    let Binding::Control(id)=widget.binding else{panic!("real scalar binding")};id
}
#[test]
fn drawn_snapshot_rejects_reused_table_cell_after_shrink_and_regrow(){
    let (_thread,loaded)=ScriptThread::spawn(source(r#"
      local steps=Table('Steps',3,0.25,0,1,false)
      local shrink=Button('Shrink');local grow=Button('Grow');local result=Label('Result')
      shrink.changed=function() steps.length=1;result.text='shrunk' end
      grow.changed=function() steps.length=3;result.text='regrown' end
      steps.changed=function(self,index) result.text='edited' end
    "#),(),Config::default()).unwrap();
    let drawn=loaded.ui.snapshot();let cell=drawn.interface().widgets[0].components[2];
    assert!(loaded.ui.edit(named_control(drawn.interface(),"Shrink"),1.));
    wait_face(&loaded.ui,|f|matches!(f.widgets[0].kind,Kind::Table{columns:1,..}));
    assert!(loaded.ui.edit(named_control(&loaded.ui.interface(),"Grow"),1.));
    wait_face(&loaded.ui,|f|matches!(f.widgets[0].kind,Kind::Table{columns:3,..}));
    assert!(!drawn.edit(cell,0.9),"a retired drawn cell cannot target a reused atomic slot");
    assert_eq!(loaded.ui.value(cell),Some(0.25));
    let current=loaded.ui.snapshot();assert!(!drawn.same_schema(&current));assert!(current.edit(cell,0.625));
    wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.text=="edited"));
    assert_eq!(loaded.ui.value(cell),Some(0.625));
}
#[test]
fn identical_local_control_ids_in_different_bridges_do_not_share_snapshot_authority(){
    let xml=source("local gain=Knob('Gain',0,0,1)");
    let (_a,first)=ScriptThread::spawn(xml.clone(),(),Config::default()).unwrap();
    let (_b,second)=ScriptThread::spawn(xml,(),Config::default()).unwrap();
    let old=first.ui.snapshot();let id=named_control(old.interface(),"Gain");
    assert_eq!(second.ui.value(id),Some(0.));assert!(!second.ui.owns_snapshot(&old));
    assert!(!second.ui.edit_snapshot(&old,id,0.75));assert_eq!(second.ui.value(id),Some(0.));
    assert!(second.ui.snapshot().edit(id,0.5));
    wait_face(&second.ui,|f|f.widgets[0].initial_value==0.5);
    assert_eq!(first.ui.value(id),Some(0.));
}
#[test]
fn queued_drawn_action_checks_actual_parent_visibility_and_enable_state(){
    let (_thread,loaded)=ScriptThread::spawn(source(r#"
      local parent=Panel('Parent');local gain=parent:Knob('Gain',0,0,1)
      local hide=Button('Hide');local enable=Button('Enable');local state=Label('State')
      gain.changed=function() state.text='wrong callback' end
      hide.changed=function() parent.visible=false;parent.enabled=false end
      enable.changed=function() parent.visible=true;parent.enabled=true end
    "#),(),Config::default()).unwrap();
    let drawn=loaded.ui.snapshot();let gain=named_control(drawn.interface(),"Gain");
    // Both requests refer to the same drawn presentation. The owner executes
    // Hide first and MUST inspect fresh Lua state before admitting Gain.
    assert!(drawn.edit(named_control(drawn.interface(),"Hide"),1.));
    let _queued=drawn.edit(gain,0.75);
    wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.name=="Parent"&&w.hidden));
    let hidden=loaded.ui.snapshot();assert!(!hidden.same_schema(&drawn));
    // A current hidden snapshot also cannot issue a renderer gesture.
    let _queued_hidden=hidden.edit(gain,0.5);
    assert!(loaded.ui.edit(named_control(&loaded.ui.interface(),"Enable"),1.));
    wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.name=="Parent"&&!w.hidden));
    assert_eq!(loaded.ui.value(gain),Some(0.));
    assert!(!loaded.ui.interface().widgets.iter().any(|w|w.text=="wrong callback"));
}
#[test]
fn bounded_registry_reuses_concurrent_cell_slots_without_a_created_id_lifetime_limit(){
    use sampler_uvi::scripted::UiRegistryBudget;
    let (_thread,loaded)=ScriptThread::spawn_with_ui_registry_budget(source(r#"
      local steps=Table('Steps',1,0.25,0,1,false);local flip=Button('Flip')
      flip.changed=function() if steps.length==1 then steps.length=3 else steps.length=1 end end
    "#),(),Config::default(),None,UiRegistryBudget{max_controls:4,max_retained_snapshots:2,max_payload_bytes:64*1024}).unwrap();
    for round in 0..40 {
        let before=loaded.ui.snapshot();assert!(before.edit(named_control(before.interface(),"Flip"),1.));drop(before);
        let columns=if round%2==0{3}else{1};
        wait_face(&loaded.ui,|f|matches!(f.widgets[0].kind,Kind::Table{columns:got,..}if got==columns));
        let storage=loaded.ui.registry_storage().unwrap();assert_eq!(storage.prepared_cells,4);
        assert_eq!(storage.current_controls,columns as usize+1);
        assert!(storage.retained_payload_bytes<=64*1024);
    }
}
#[test]
fn concurrent_limit_refuses_new_schema_without_publishing_visible_inert_control(){
    use sampler_uvi::scripted::UiRegistryBudget;
    let (_thread,loaded)=ScriptThread::spawn_with_ui_registry_budget(source(r#"
      local create=Button('Create');create.changed=function() Knob('OverBudget',0,0,1) end
    "#),(),Config::default(),None,UiRegistryBudget{max_controls:1,max_retained_snapshots:2,max_payload_bytes:64*1024}).unwrap();
    let first=loaded.ui.snapshot();assert!(first.edit(named_control(first.interface(),"Create"),1.));
    let deadline=Instant::now()+Duration::from_secs(3);
    loop { if loaded.ui.findings().iter().any(|f|format!("{f:?}").contains("concurrent-control budget")){break;}
        assert!(Instant::now()<deadline,"missing explicit registry admission finding");std::thread::sleep(Duration::from_millis(2)); }
    assert_eq!(loaded.ui.interface().widgets.len(),1,"last valid presentation retained; rejected new control never published");
    assert_eq!(loaded.ui.registry_storage().unwrap().current_controls,1);
    assert!(!first.edit(named_control(first.interface(),"Create"),1.));
}
#[test]
fn initial_registry_budget_refusal_happens_before_a_host_is_published(){
    use sampler_uvi::scripted::UiRegistryBudget;
    let result=ScriptThread::spawn_with_ui_registry_budget(source("Knob('A',0,0,1);Knob('B',0,0,1)"),(),Config::default(),None,UiRegistryBudget{max_controls:1,max_retained_snapshots:1,max_payload_bytes:64*1024});
    assert!(result.is_err());let error=result.err().unwrap();assert!(error.contains("concurrent-control budget"),"{error}");
}
#[test]
fn retained_schema_budget_recovers_after_the_actual_old_snapshot_is_released(){
    use sampler_uvi::scripted::UiRegistryBudget;
    let (mut thread,loaded)=ScriptThread::spawn_with_ui_registry_budget(source(r#"
      local gain=Knob('Gain',0,0,1);local switch=Button('Switch');local state=Label('State')
      switch.changed=function() if gain.max==1 then gain.max=2 else gain.max=1 end end
      function onController(e) state.text='owner resumed' end
    "#),(),Config::default(),None,UiRegistryBudget{max_controls:2,max_retained_snapshots:0,max_payload_bytes:64*1024}).unwrap();
    let old=loaded.ui.snapshot();let switch=named_control(old.interface(),"Switch");assert!(old.edit(switch,1.));
    let deadline=Instant::now()+Duration::from_secs(3);
    while loaded.ui.findings().is_empty(){assert!(Instant::now()<deadline);std::thread::sleep(Duration::from_millis(2));}
    assert_eq!(loaded.ui.snapshot().generation(),old.generation());
    drop(old);
    // Real later owner work retries its declaration after the actual old
    // snapshot has retired. There is no audio allocation or guessed ID retry.
    thread.input(sampler_uvi::scripted::HostInput::Controller{cc:1,value:64,channel:0});
    wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.text=="owner resumed"));
    let current=loaded.ui.snapshot();assert!(loaded.ui.value(switch).is_some());
    let gain=named_control(current.interface(),"Gain");drop(current);
    assert!(loaded.ui.edit(gain,0.75));
    wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.name=="Gain"&&w.initial_value==0.75));
}

#[test]
fn dynamically_created_real_widget_command_changes_same_held_runtime_pcm(){
    use sampler_core::{Envelope,Input,Limits,Pcm,Playback,Prepared,Protocol,Region,Runtime};
    use sampler_uvi::scripted::Driver;
    use std::sync::atomic::{AtomicBool,Ordering};
    struct Observed { thread:ScriptThread, seen:Arc<AtomicBool> }
    impl Script for Observed {
        fn handles_notes(&self)->bool{self.thread.handles_notes()}
        fn set_time(&mut self,ms:f64){self.thread.set_time(ms)}
        fn note_on(&mut self,id:u64,key:u8,velocity:u8){self.thread.note_on(id,key,velocity)}
        fn note_off(&mut self,id:u64,key:u8){self.thread.note_off(id,key)}
        fn advance(&mut self,ms:f64){self.thread.advance(ms)}
        fn next_due(&mut self)->Option<f64>{self.thread.next_due()}
        fn drain(&mut self,out:&mut Vec<Command>){
            let begin=out.len();self.thread.drain(out);
            if out[begin..].iter().any(|command|matches!(command,Command::Parameter{scope:Scope::Program,param:Param::Gain,value,..}if *value==0.25)){self.seen.store(true,Ordering::Release);}
        }
    }
    fn runtime()->Runtime{
        let pcm=Pcm::new(48_000,vec![[0.25,-0.125];1024].into_boxed_slice()).unwrap();
        let region=Region{sample:0,key_low:60,key_high:60,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,0).unwrap(),playback:Playback::default()};
        let plan=Prepared::new(48_000,vec![pcm],vec![region],1).unwrap();let limits=Limits::for_plan(&plan,4,4);Runtime::new(plan,limits).unwrap()
    }
    let (thread,loaded)=ScriptThread::spawn(source(r#"
      local create=Button('Create');create.changed=function()
        if made then return end;made=true;local gain=Knob('DynamicGain',1,0,1)
        gain.changed=function(self) Program:setParameter('Gain',self.value) end
      end
    "#),(),Config::default()).unwrap();
    assert!(loaded.ui.edit(named_control(&loaded.ui.interface(),"Create"),1.));
    let face=wait_face(&loaded.ui,|f|f.widgets.iter().any(|w|w.name=="DynamicGain"));let gain=named_control(&face,"DynamicGain");
    let seen=Arc::new(AtomicBool::new(false));let mut driver=Driver::new(Observed{thread,seen:seen.clone()},vec![],48_000);
    let mut actual=runtime();let mut reference=runtime();let input=Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(1)};
    let note=actual.trigger(input,60,1.).unwrap();reference.trigger(input,60,1.).unwrap();
    let mut before=[[0.;2];32];let mut reference_before=[[0.;2];32];actual.render(&mut before).unwrap();reference.render(&mut reference_before).unwrap();assert_eq!(before,reference_before);
    assert!(loaded.ui.snapshot().edit(gain,0.25));
    let deadline=Instant::now()+Duration::from_secs(3);
    while !seen.load(Ordering::Acquire){driver.wake(&mut actual).unwrap();assert!(Instant::now()<deadline,"actual dynamic changed command never reached ordinary Driver");std::thread::sleep(Duration::from_millis(1));}
    let mut got=[[0.;2];64];let mut wanted=[[0.;2];64];actual.render(&mut got).unwrap();reference.render(&mut wanted).unwrap();
    assert!(wanted.iter().flatten().any(|value|value.abs()>0.01));
    for (a,b) in got.iter().flatten().zip(wanted.iter().flatten()){assert!((f64::from(*a)-f64::from(*b)*0.25).abs()<2e-5,"dynamic original callback must set quarter-gain held PCM");}
    assert_eq!(actual.input_held(note),Ok(true));assert_eq!(actual.voice_count(),1);
    assert_eq!(actual.stats().nonfinite_frames,0);assert_eq!(actual.stats().voice_pool_refusals,0);
}

#[test]
fn registry_schema_work_exact_initial_boundary_and_one_more_are_explicit(){
    use sampler_uvi::scripted::{UiRegistryBudget,UiSchemaWorkBudget};
    let xml=source("local p=Panel('Root');p:Knob('Child',0.25,0,1)");
    // Discovery=2, prepaid component=1, build=2, ancestor visits=1+2,
    // ordering check=0. These are charged credits, not a CPU-time claim.
    let (_thread,loaded)=ScriptThread::spawn_with_ui_registry_budgets(xml.clone(),(),Config::default(),None,
        UiRegistryBudget{max_controls:1,max_retained_snapshots:1,max_payload_bytes:64*1024},UiSchemaWorkBudget{max_inspections:8}).unwrap();
    let report=loaded.ui.initial_schema_work();assert_eq!((report.requested,report.admitted,report.exhausted),(8,8,false));
    let refused=ScriptThread::spawn_with_ui_registry_budgets(xml,(),Config::default(),None,
        UiRegistryBudget{max_controls:1,max_retained_snapshots:1,max_payload_bytes:64*1024},UiSchemaWorkBudget{max_inspections:7});
    let error=refused.err().expect("one-over work must refuse before Loaded publication");
    assert!(error.contains("schema inspection work budget"),"{error}");assert!(error.contains("requested=8, admitted=7, limit=7"),"{error}");
}
#[test]
fn zero_control_panel_population_still_spends_real_registry_inspection_credit(){
    use sampler_uvi::scripted::{UiRegistryBudget,UiSchemaWorkBudget};
    let result=ScriptThread::spawn_with_ui_registry_budgets(source("for i=1,17 do Panel('P'..i) end"),(),Config::default(),None,
        UiRegistryBudget{max_controls:1,max_retained_snapshots:1,max_payload_bytes:64*1024},UiSchemaWorkBudget{max_inspections:16});
    let error=result.err().expect("zero controls do not exempt widget inspection");
    assert!(error.contains("schema inspection work budget"),"{error}");assert!(error.contains("requested=17, admitted=16, limit=16"),"{error}");
}
#[test]
fn deep_real_lua_parent_chain_is_work_refused_without_a_control_or_metadata_limit_hit(){
    use sampler_uvi::scripted::{UiRegistryBudget,UiSchemaWorkBudget};
    let result=ScriptThread::spawn_with_ui_registry_budgets(source("local p=Panel('Root');for i=1,128 do p=p:Panel('P'..i) end;p:Knob('OnlyControl',0.25,0,1)"),(),Config::default(),None,
        UiRegistryBudget{max_controls:1,max_retained_snapshots:1,max_payload_bytes:64*1024},UiSchemaWorkBudget{max_inspections:512});
    let error=result.err().expect("deep ancestry must stop on the work bound");
    assert!(error.contains("schema inspection work budget"),"not a control/payload refusal: {error}");
    assert!(error.contains("limit=512"),"{error}");
}
#[test]
fn dynamic_work_refusal_keeps_last_admitted_cells_and_publication_unchanged(){
    use sampler_uvi::scripted::{UiRegistryBudget,UiSchemaWorkBudget};
    let (_thread,loaded)=ScriptThread::spawn_with_ui_registry_budgets(source(r#"
      local gain=Knob('Gain',0.25,0,1);local create=Button('Create')
      create.changed=function()
        gain.value=0.75
        local p=Panel('Root');for i=1,48 do p=p:Panel('P'..i) end
      end
    "#),(),Config::default(),None,UiRegistryBudget{max_controls:2,max_retained_snapshots:2,max_payload_bytes:64*1024},UiSchemaWorkBudget{max_inspections:200}).unwrap();
    let before=loaded.ui.snapshot();let gain=named_control(before.interface(),"Gain");let create=named_control(before.interface(),"Create");
    let original=loaded.ui.values();assert!(before.edit(create,1.));
    let deadline=Instant::now()+Duration::from_secs(3);
    loop{if loaded.ui.schema_work().exhausted{break;}assert!(Instant::now()<deadline,"owner never hit the actual schema work gate");std::thread::sleep(Duration::from_millis(2));}
    let report=loaded.ui.schema_work();assert!(report.requested>report.limit);assert!(report.admitted<=report.limit);
    assert_eq!(loaded.ui.snapshot().generation(),before.generation());
    assert_eq!(loaded.ui.interface().widgets.len(),2,"unadmitted deep face must not replace old presentation");
    assert_eq!(loaded.ui.values(),original,"inspection refusal cannot poison or partially replace old cells");assert_eq!(loaded.ui.value(gain),Some(0.25));
}
