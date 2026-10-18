//! Authored ordinary UI owner/transport tests. No bank/native artifact.
use sampler_ui_ir::{Binding,UiActivation,UiActivationOrigin as Origin,UiModifiers};
use sampler_uvi::script::{Config,ScriptHost,control_id};
use sampler_uvi::scripted::{Script,ScriptThread,UiBridge,HostInput};
use std::time::{Duration,Instant};
fn source(script:&str)->String{format!("<UVI4><Program Name='UI' Gain='1'><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>")}
fn host(script:&str)->ScriptHost{ScriptHost::new(&source(script),(),Config::default()).unwrap()}
fn action(origin:Origin,mods:UiModifiers)->UiActivation{UiActivation{origin,modifiers:if origin==Origin::Accessibility{None}else{Some(mods)}}}
fn ordinary()->UiActivation{action(Origin::PointerUp,UiModifiers::default())}
fn until(mut ready:impl FnMut()->bool){let deadline=Instant::now()+Duration::from_secs(3);while !ready(){assert!(Instant::now()<deadline,"actual owner did not complete expected UI work");std::thread::sleep(Duration::from_millis(2));}}
fn named(ui:&UiBridge,name:&str)->sampler_ui_ir::ControlId{let face=ui.interface();let w=face.widgets.iter().find(|w|w.name==name).unwrap();let Binding::Control(id)=w.binding else{panic!("actual scalar binding")};id}
#[test]
fn programmatic_push_flag_and_nil_modifier_semantics_remain_unchanged(){
    let h=host("calls=0;nilmods=true;local b=Button('B');b.changed=function(self,mods) calls=calls+1;nilmods=nilmods and mods==nil end;b:push(false);b:push(true);b:push(false)");
    assert_eq!(h.global_text("calls"),"1");assert_eq!(h.global_text("nilmods"),"true");
}
#[test]
fn programmatic_onoff_notifications_and_save_restore_stay_independent_of_click_policy(){
    let script="calls=0;nilmods=true;local b=OnOffButton('B',false);b.triggeredOnMouseDown=true;b.changed=function(self,mods) calls=calls+1;nilmods=nilmods and mods==nil end;function onController(e) b:setValue(true,false);b:setValue(false);b:setValue(false) end";
    let mut h=host(script);h.controller(1,64,0);assert_eq!(h.global_text("calls"),"1");assert_eq!(h.global_text("nilmods"),"true");
    let state=h.save_ui_state().unwrap();let restored=ScriptHost::new_with_ui_state(&source(script),(),Config::default(),Some(&state)).unwrap();
    assert_eq!(restored.interface().widgets[0].initial_value,0.);assert_eq!(restored.global_text("calls"),"0");
}
#[test]
fn real_click_modifier_fields_are_exact_booleans_with_platform_command_mapping(){
    let mut h=host("calls=0;local b=Button('B');b.changed=function(self,mods) assert(type(mods)=='table' and type(mods.altDown)=='boolean' and type(mods.shiftDown)=='boolean' and type(mods.commandDown)=='boolean');calls=calls+1;seen=tostring(mods.shiftDown)..'|'..tostring(mods.commandDown)..'|'..tostring(mods.altDown) end");
    for bits in 0..16{let raw=UiModifiers{shift:bits&1!=0,ctrl:bits&2!=0,alt:bits&4!=0,cmd:bits&8!=0};h.activate_control(control_id(1,0),action(Origin::PointerUp,raw)).unwrap();let command=if cfg!(target_os="macos"){raw.cmd}else{raw.ctrl};assert_eq!(h.global_text("seen"),format!("{}|{}|{}",raw.shift,command,raw.alt));}
    assert_eq!(h.global_text("calls"),"16");
}
#[test]
fn retained_previous_modifier_object_does_not_corrupt_a_later_input(){
    let mut h=host("calls=0;local previous;local b=Button('B');b.changed=function(self,mods) if previous then previous.shiftDown=true;assert(mods~=previous and mods.shiftDown==false) end;previous=mods;calls=calls+1 end");
    h.activate_control(control_id(1,0),ordinary()).unwrap();h.activate_control(control_id(1,0),ordinary()).unwrap();assert_eq!(h.global_text("calls"),"2");
}
#[test]
fn onoff_activation_toggles_actual_owner_state_not_a_stale_drawn_target(){
    let mut h=host("calls=0;local b=OnOffButton('B',false);b.changed=function(self,mods) calls=calls+1;seen=tostring(self.value) end;function onController(e) b:setValue(true,false) end");
    h.controller(1,64,0);assert_eq!(h.interface().widgets[0].initial_value,1.);
    h.activate_control(control_id(1,0),ordinary()).unwrap();assert_eq!(h.global_text("seen"),"false");
    h.activate_control(control_id(1,0),ordinary()).unwrap();assert_eq!(h.global_text("seen"),"true");
    h.activate_control(control_id(1,0),ordinary()).unwrap();assert_eq!(h.global_text("seen"),"false");assert_eq!(h.global_text("calls"),"3");
}
#[test]
fn phase_gate_and_nonphysical_primary_actions_preserve_single_widget_creator(){
    let mut h=host("calls=0;local b=Button('B');b.triggeredOnMouseDown=true;b.changed=function(self,mods) calls=calls+1;nilmods=mods==nil end");
    assert!(h.interface().widgets[0].activate_on_press);
    assert!(h.activate_control(control_id(1,0),ordinary()).is_err());assert_eq!(h.global_text("calls"),"0");
    h.activate_control(control_id(1,0),action(Origin::PointerDown,UiModifiers::default())).unwrap();
    h.activate_control(control_id(1,0),action(Origin::Keyboard,UiModifiers::default())).unwrap();assert_eq!(h.global_text("nilmods"),"false");
    h.activate_control(control_id(1,0),action(Origin::Accessibility,UiModifiers::default())).unwrap();assert_eq!(h.global_text("calls"),"3");assert_eq!(h.global_text("nilmods"),"true");
}
#[test]
fn direct_owner_activation_checks_current_ancestor_enabled_visible_and_intercept_state(){
    for setup in ["p.enabled=false","p.visible=false","b.interceptsMouseClicks=false"]{
        let mut h=host(&format!("calls=0;local p=Panel('P');local b=p:Button('B');b.changed=function() calls=calls+1 end;{setup}"));
        assert!(h.activate_control(control_id(2,0),ordinary()).is_err());assert_eq!(h.global_text("calls"),"0");
    }
}
#[test]
fn multi_state_activation_cycles_once_and_passes_actual_modifiers(){
    let mut h=host("calls=0;local m=MultiStateButton('M',{'A','B','C'},1);m.changed=function(self,mods) calls=calls+1;seen=tostring(self.value)..'|'..tostring(mods.altDown) end");
    let click=action(Origin::PointerUp,UiModifiers{alt:true,..Default::default()});
    h.activate_control(control_id(1,0),click).unwrap();assert_eq!(h.global_text("seen"),"2|true");h.activate_control(control_id(1,0),click).unwrap();assert_eq!(h.global_text("seen"),"3|true");assert_eq!(h.global_text("calls"),"2");
}
#[test]
fn param_onoff_gesture_never_calls_ordinary_onoff_changed(){
    let mut h=host("calls=0;local b=ParamOnOffButton(Program,'BypassInsertFX');b.changed=function() calls=calls+1 end");
    h.activate_control(control_id(1,0),ordinary()).unwrap();assert_eq!(h.global_text("calls"),"0");
    // Existing runtime Boolean binding/acceptance may explicitly refuse; this
    // test proves callback classification, not invented DSP Boolean success.
}
#[test]
fn widget_playnote_default_error_and_callback_fault_are_not_replayed(){
    let mut h=host("calls=0;local b=Button('B');b.changed=function() calls=calls+1;playNote(60,100) end");
    assert!(h.activate_control(control_id(1,0),ordinary()).is_err());assert_eq!(h.global_text("calls"),"1");assert!(h.take_commands().is_empty());
    let mut h=host("calls=0;local b=Button('B');b.changed=function() calls=calls+1;error('authored UI fault') end");
    assert!(h.activate_control(control_id(1,0),ordinary()).is_err());assert_eq!(h.global_text("calls"),"1");
}
#[test]
fn current_handler_phase_and_actual_bridge_generation_guard_queued_activations(){
    let xml=source("local b=Button('B');local swap=Button('Swap');local state=Label('State');state.text='before';b.changed=function() state.text='old' end;swap.changed=function() b.triggeredOnMouseDown=true;b.changed=function(self,mods) state.text='new:'..tostring(mods.shiftDown) end end");
    let (_thread,loaded)=ScriptThread::spawn(xml.clone(),(),Config::default()).unwrap();let drawn=loaded.ui.snapshot();let b=named(&loaded.ui,"B");
    assert!(drawn.edit(named(&loaded.ui,"Swap"),1.));let _queued=drawn.activate(b,ordinary());
    until(||loaded.ui.interface().widgets[0].activate_on_press);assert!(loaded.ui.interface().widgets.iter().any(|w|w.text=="before"));
    let current=loaded.ui.snapshot();assert!(!drawn.same_schema(&current));assert!(!drawn.activate(b,ordinary()));
    assert!(current.activate(b,action(Origin::PointerDown,UiModifiers{shift:true,..Default::default()})));until(||loaded.ui.interface().widgets.iter().any(|w|w.text=="new:true"));
    let (_other_thread,other)=ScriptThread::spawn(xml,(),Config::default()).unwrap();assert!(!other.ui.activate_snapshot(&current,b,ordinary()));
}
#[test]
fn stopped_real_script_owner_does_not_accept_an_old_drawn_activation(){
    let (thread,loaded)=ScriptThread::spawn(source("Button('B')"),(),Config::default()).unwrap();let drawn=loaded.ui.snapshot();let b=named(&loaded.ui,"B");drop(thread);assert!(!drawn.activate(b,ordinary()));
}
#[test]
fn bounded_activation_queue_refuses_capacity_plus_one_without_retry_or_extra_callback(){
    use std::sync::mpsc;
    struct Held {entered:mpsc::Sender<()>,release:mpsc::Receiver<()>}
    impl sampler_uvi::script::Files for Held{fn script(&self,name:&str)->Option<String>{assert_eq!(name,"hold");self.entered.send(()).unwrap();self.release.recv_timeout(Duration::from_secs(3)).unwrap();Some("return {}".into())}}
    let (entered,waiting)=mpsc::channel();let (release,held)=mpsc::channel();
    let (mut thread,loaded)=ScriptThread::spawn(source("local b=Button('B');local state=Label('State');local calls=0;b.changed=function() calls=calls+1;state.text=tostring(calls) end;function onController(e) require('hold') end"),Held{entered,release:held},Config::default()).unwrap();
    let snapshot=loaded.ui.snapshot();let id=named(&loaded.ui,"B");thread.input(HostInput::Controller{cc:1,value:64,channel:0});waiting.recv_timeout(Duration::from_secs(3)).unwrap();
    for _ in 0..256{assert!(snapshot.activate(id,ordinary()));}assert!(!snapshot.activate(id,ordinary()),"the actual fixed queue must refuse its 257th request");
    release.send(()).unwrap();until(||loaded.ui.interface().widgets.iter().any(|w|w.name=="State"&&w.text=="256"));
    assert!(!loaded.ui.interface().widgets.iter().any(|w|w.text=="257"));
}
