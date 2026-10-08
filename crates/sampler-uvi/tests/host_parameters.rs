use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn initial_and_explicit_state_load_preserve_their_distinct_callback_orders() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor K='7'>
      <state>{"counter":42}</state><script>
      local k=Knob('K',0,0,10)
      order=''
      function onLoad(s) assert(s.counter==42); order=order..'L'..k.value end
      function k:changed() order=order..'W'..self.value end
      function onInit() order=order..'I'..k.value end
      function onSave() return {counter=42} end
      </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("order"), "L0W7I7");
    let state = host.save_ui_state().unwrap();
    host.restore_ui_state(&state).unwrap();
    assert_eq!(host.global_text("order"), "L0W7I7W7L7");
}

#[test]
fn numeric_parameter_ids_name_lookup_and_real_connections_share_identity() {
    let xml = r#"<UVI4><Program><Inserts><OnePole><Connections>
      <SignalConnection Destination='Freq' Ratio='0.3'/>
    </Connections></OnePole></Inserts><EventProcessors><ScriptProcessor><script>
      local e = Program.inserts[1]
      local d = e.parameterDefinitions.Freq
      assert(type(d.id) == 'number' and d.max == 20000)
      assert(d.type=='float' and e.parameterDefinitions.Bypass.type=='bool')
      assert(e.parameterDefinitions.Mode.type=='int' and e.numParams==4)
      assert(e:getParameter(d.id) == d.default)
      assert(#e:getParameterConnections(d.id) == 1)
      assert(e:getParameterConnections(d.id)[1]:getParameter('Ratio') == 0.3)
      assert(#e:getParameterConnections('Bypass') == 0)
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(!h.findings().iter().any(|f| f.feature.contains("error")), "{:?}", h.findings());
}

#[test]
fn children_name_lookup_and_synthesis_children_do_not_mix_inserts() {
    let xml=r#"<UVI4><Program><Layers><Layer Name='L'/></Layers><Inserts><OnePole Name='F'/></Inserts>
      <ControlSignalSources><LFO Name='M'/></ControlSignalSources><EventProcessors><ScriptProcessor><script>
      assert(Program.children.L==Program.layers[1])
      assert(Program.children.F==Program.inserts[1])
      assert(#Program.synthChildren==1 and Program.synthChildren[1]==Program.layers[1])
      assert(Program.mods[1]==Program.modulations[1])
      </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h=ScriptHost::new(xml,(),Config::default()).unwrap();
    assert!(!h.findings().iter().any(|f| f.feature.contains("error")),"{:?}",h.findings());
}

#[test]
fn returned_module_builders_keep_the_resource_source_directory() {
    let mut scripts = sampler_uvi::script::Scripts::default();
    scripts.insert("Scripts/UI/panel.lua", "return function() Image('../Textures/face.png') end".into());
    let h = ScriptHost::new("<UVI4><Program><EventProcessors><ScriptProcessor><script>local build = require('UI/panel'); build()</script></ScriptProcessor></EventProcessors></Program></UVI4>", scripts, Config::default()).unwrap();
    assert_eq!(h.interface().assets[0].path, "/scripts/ui/../Textures/face.png");
}

#[test]
fn lua_faults_keep_the_first_message_and_count_each_phase_and_category() {
    use sampler_uvi::script::FaultCategory;
    let xml=r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
      function onInit() error('first init fault') end
      function onNote(e) if e.velocity>80 then while true do end else error('later runtime fault') end end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut h=ScriptHost::new(xml,(),Config {callback:std::time::Duration::from_millis(1),..Config::default()}).unwrap();
    h.note_on(1,60,64,0); h.note_on(2,60,64,0); h.note_on(3,60,100,0);
    let counts=h.fault_counts();
    assert_eq!(counts.first,Some(FaultCategory::Lua));
    assert_eq!(counts.init[&FaultCategory::Lua],1);
    assert_eq!(counts.runtime[&FaultCategory::Lua],2);
    assert_eq!(counts.runtime[&FaultCategory::Budget],1);
    let f=h.findings().into_iter().find(|f|f.feature=="lua error").unwrap();
    assert_eq!(f.count,4);
    assert!(f.value.contains("first init fault"));
}
