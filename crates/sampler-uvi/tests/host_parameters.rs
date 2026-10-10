use sampler_uvi::script::{Config, ScriptHost};

#[test]
fn named_parameter_checks_and_writes_do_not_build_full_catalogs() {
    let xml = r#"<UVI4><Program><Inserts><OnePole/><OnePole Custom='text'/></Inserts><EventProcessors><ScriptProcessor><script>
      local e=Program.inserts[1]
      assert(e:hasParameter('Freq') and not e:hasParameter('Missing'))
      e:setParameter('Freq',99999); assert(e:getParameter('Freq')==20000)
      e:setParameter('Bypass',1); assert(e:getParameter('Bypass')==false)
      assert(e.numParams==4)
      local id=Program.inserts[2].parameterDefinitions.Freq.id
      e:setParameter(id,100); assert(e:getParameter(id)==100 and e:hasParameter(id))
      for _,invalid in ipairs({0,-1,0.5,1000000}) do
        assert(not e:hasParameter(invalid) and not pcall(function() e:getParameter(invalid) end))
      end
      assert(rawget(e,'parameterDefinitions')==nil)
      local custom=Program
      assert(custom:hasParameter('Custom')); custom:setParameter('Custom','retained')
      assert(custom:getParameter('Custom')=='retained' and rawget(custom,'parameterDefinitions')==nil)
      local defs=e.parameterDefinitions
      assert(defs.Freq.type=='float' and defs.Bypass.type=='bool')
      local originalMin=defs.Freq.min
      Program.inserts[2].parameterDefinitions.Freq.min=originalMin+1
      assert(defs.Freq.min==originalMin)
      e:setParameter(defs.Freq.id,100); assert(e:getParameter(defs.Freq.id)==100)
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let xml = xml.replace("<Program>", "<Program Custom='text'>");
    let h = ScriptHost::new(&xml, (), Config::default()).unwrap();
    assert!(h.fault_counts().init.is_empty(), "{:?}", h.fault_counts());
}

#[test]
fn unknown_parameter_names_and_ids_are_silently_ignored_on_cold_and_cached_elements() {
    let xml = r#"<UVI4><Program><Inserts><OnePole/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>
      local elements={Program,Program.layers[1],Program.layers[1].keygroups[1],
        Program.layers[1].keygroups[1].oscillators[1],Program.inserts[1]}
      for _,cached in ipairs({false,true}) do
        for _,e in ipairs(elements) do
          if cached then local defs=e.parameterDefinitions end
          e:setParameter('Missing',0.25)
          e:setParameter(nil,1)
          for _,id in ipairs({0,-1,0.5,1000000,math.huge}) do e:setParameter(id,1) end
          assert(not e:hasParameter('Missing') and rawget(e,'__set')==nil)
          assert(cached or rawget(e,'parameterDefinitions')==nil)
        end
      end
      completed=true
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("completed"), "true");
    assert!(host.findings().is_empty(), "{:?}", host.findings());
    assert!(host.take_commands().is_empty());
    assert!(host.insert_overrides().is_empty());
}

#[test]
fn unsupported_catalog_writes_leave_authored_readback_and_production_queue_unchanged() {
    let xml = r#"<UVI4><Program TransposeSemiTones='2'><Inserts><OnePole Mode='1'/></Inserts><Layers><Layer Mute='1'><Keygroups><Keygroup LowKey='12'><Oscillators><SamplePlayer BaseNote='62'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>
      local cases={{Program,'TransposeSemiTones',2,3},
        {Program.layers[1],'Mute',true,false},
        {Program.layers[1].keygroups[1],'LowKey',12,13},
        {Program.layers[1].keygroups[1].oscillators[1],'BaseNote',62,63},
        {Program.inserts[1],'Mode',1,0}}
      for _,cached in ipairs({false,true}) do
        for _,c in ipairs(cases) do
          local e,name,before,after=c[1],c[2],c[3],c[4]
          assert(e:hasParameter(name) and e:getParameter(name)==before)
          if cached then name=e.parameterDefinitions[name].id end
          e:setParameter(name,after)
          assert(e:getParameter(name)==before and rawget(e,'__set')==nil)
        end
      end
      completed=true
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("completed"), "true");
    assert!(
        host.fault_counts().init.is_empty(),
        "{:?}",
        host.fault_counts()
    );
    assert!(host.take_commands().is_empty());
    assert!(host.insert_overrides().is_empty());
    assert_eq!(host.findings().len(), 5);
}

#[test]
fn accepted_parameters_and_rejected_types_share_the_production_queue_seam() {
    use sampler_uvi::script::{Command, Param, Scope};
    let xml = r#"<UVI4><Program Gain='1'><Inserts><OnePole Freq='500'/><Gain Volume='1'/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer Pitch='1'/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>
      local layer=Program.layers[1]
      local group=layer.keygroups[1]
      local osc=group.oscillators[1]
      local cases={{Program,'Gain',0.5},{Program,'Pan',0.25},{Program,'Polyphony',4},
        {layer,'Gain',0.5},{layer,'Pan',0.25},{group,'Gain',0.5},{group,'Pan',0.25},
        {osc,'Gain',0.5},{osc,'Pitch',2},{Program.inserts[1],'Freq',600},
        {Program.inserts[2],'Volume',0.25}}
      for i,c in ipairs(cases) do
        local e,name,value=c[1],c[2],c[3]
        if i%2==0 then name=e.parameterDefinitions[name].id end
        e:setParameter(name,value)
        assert(e:getParameter(name)==value)
        e:setParameter(name,'wrong type')
        assert(e:getParameter(name)==value)
      end
      Program:setParameter('Polyphony',3.5)
      assert(Program:getParameter('Polyphony')==4)
      completed=true
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("completed"), "true");
    assert!(
        host.fault_counts().init.is_empty(),
        "{:?}",
        host.fault_counts()
    );
    let commands = host.take_commands();
    assert_eq!(commands.len(), 11, "{commands:?}");
    let parameters: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::Parameter {
                scope,
                param,
                value,
                authored,
            } => Some((
                match scope {
                    Scope::Program => "Program",
                    Scope::Layer(1) => "Layer",
                    Scope::Keygroup(_) => "Keygroup",
                    Scope::Oscillator(_) => "Oscillator",
                    other => panic!("unexpected scope: {other:?}"),
                },
                *param,
                *value,
                *authored,
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        parameters,
        [
            ("Program", Param::Gain, 0.5, 1.),
            ("Program", Param::Pan, 0.25, 0.),
            ("Program", Param::Polyphony, 4., 16.),
            ("Layer", Param::Gain, 0.5, 1.),
            ("Layer", Param::Pan, 0.25, 0.),
            ("Keygroup", Param::Gain, 0.5, 1.),
            ("Keygroup", Param::Pan, 0.25, 0.),
            ("Oscillator", Param::Gain, 0.5, 1.),
            ("Oscillator", Param::Pitch, 2., 1.),
        ]
    );
    for (command, name, value, law) in [
        (
            &commands[9],
            "ENGINE_PAR_CUTOFF",
            600.,
            sampler_core::EngineParameterLaw::Exponential {
                low: 20.,
                high: 20000.,
            },
        ),
        (
            &commands[10],
            "ENGINE_PAR_VOLUME",
            0.25,
            sampler_core::EngineParameterLaw::Linear {
                low: 0.,
                high: 3.981,
            },
        ),
    ] {
        assert!(
            matches!(command, Command::EngineParameter { address, value: actual }
            if address.parameter == sampler_core::engine_parameter_id(name).unwrap()
            && address.group == -1 && address.slot == -1 && address.generic >= 0
            && *actual == law.normalized_value(value).unwrap()),
            "{command:?}"
        );
    }
}

#[test]
fn a_full_production_queue_does_not_accept_parameter_readback_or_insert_overrides() {
    use sampler_uvi::script::{Command, Param};
    let xml = r#"<UVI4><Program Gain='1'><Inserts><OnePole Freq='500'/></Inserts><EventProcessors><ScriptProcessor><script>
      local filter=Program.inserts[1]
      Program:setParameter('Gain',0.5)
      filter:setParameter('Freq',600)
      for i=1,65533 do controlChange(1,1) end
      filter:setParameter('Freq',650)
      Program:setParameter('Gain',0.25)
      filter:setParameter('Freq',800)
      assert(Program:getParameter('Gain')==0.5 and filter:getParameter('Freq')==650)
      function onController(e)
        Program:setParameter('Gain',0.75)
        filter:setParameter('Freq',900)
        assert(Program:getParameter('Gain')==0.75 and filter:getParameter('Freq')==900)
        resumed=true
      end
      completed=true
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("completed"), "true");
    assert!(
        host.fault_counts().init.is_empty(),
        "{:?}",
        host.fault_counts()
    );
    let commands = host.take_commands();
    assert_eq!(commands.len(), 65536);
    assert!(matches!(
        commands[0],
        Command::Parameter {
            param: Param::Gain,
            value: 0.5,
            ..
        }
    ));
    assert!(matches!(commands[1], Command::EngineParameter { .. }));
    assert!(
        commands[2..65535]
            .iter()
            .all(|c| matches!(c, Command::Midi(_)))
    );
    let law = sampler_core::EngineParameterLaw::Exponential {
        low: 20.,
        high: 20000.,
    };
    assert!(
        matches!(commands[65535], Command::EngineParameter { value, .. }
        if value == law.normalized_value(650.).unwrap())
    );
    let overrides = host.insert_overrides();
    assert_eq!(overrides.len(), 1);
    assert_eq!((&*overrides[0].1, &*overrides[0].2), ("Freq", "650"));
    let dropped = host
        .findings()
        .into_iter()
        .find(|f| f.feature.contains("command queue full"))
        .unwrap();
    assert_eq!(dropped.count, 2);
    host.controller(1, 1, 0);
    assert_eq!(host.global_text("resumed"), "true");
    let resumed = host.take_commands();
    assert_eq!(resumed.len(), 2, "{resumed:?}");
    assert!(matches!(
        resumed[0],
        Command::Parameter {
            param: Param::Gain,
            value: 0.75,
            authored: 0.5,
            ..
        }
    ));
    assert!(matches!(resumed[1], Command::EngineParameter { value, .. }
        if value == law.normalized_value(900.).unwrap()));
}

#[test]
fn unchanged_scalar_writes_and_restoration_do_not_notify() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor K='0'><script>
      local k=Knob('K',0,0,1)
      calls=0; function k:changed() calls=calls+1 end
      function onInit()
        assert(calls==0); k:setValue(0); assert(calls==0)
        k:setValue(1); k:setValue(1); assert(calls==1)
      end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(h.fault_counts().init.is_empty());
    assert_eq!(h.global_text("calls"), "1");
}

#[test]
fn table_boundary_writes_are_ignored_and_reads_return_the_constructor_default() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
      local t=Table('T',2,0.25,0,1)
      calls=0; function t:changed(i) calls=calls+1 end
      t:setValue(0,1); t:setValue(3,1)
      assert(t:getValue(0)==0.25 and t:getValue(3)==0.25)
      assert(t:getValue(1)==0.25 and calls==0)
      t:setValue(1.8,2,false); assert(t:getValue(1)==2 and calls==0)
      t:setValue(1,2); assert(calls==0)
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(h.fault_counts().init.is_empty());
    let f = h
        .findings()
        .into_iter()
        .find(|f| f.feature == "lua widget_index_out_of_range")
        .unwrap();
    assert_eq!(f.count, 2);
    assert!(f.value.is_empty());
}

#[test]
fn initialization_uses_the_load_budget_and_live_callbacks_keep_their_budget() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
      function onInit() for n=1,10000 do local x=n*n end; initialized=true end
      function onNote(e) for n=1,10000 do local x=n*n end; played=true end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut host = ScriptHost::new(
        xml,
        (),
        Config {
            callback_work: 0,
            ..Config::default()
        },
    )
    .unwrap();
    assert_eq!(host.global_text("initialized"), "true");
    assert!(host.fault_counts().init.is_empty());
    host.note_on(1, 60, 64, 0);
    assert_eq!(host.global_text("played"), "nil");
    assert_eq!(
        host.fault_counts()
            .runtime
            .get(&sampler_uvi::script::FaultCategory::Budget),
        Some(&1)
    );
}

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
    let mut host = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(host.global_text("order"), "L0W7I7");
    let state = host.save_ui_state().unwrap();
    host.set_control(sampler_uvi::script::control_id(1, 0), 2.)
        .unwrap();
    host.restore_ui_state(&state).unwrap();
    assert_eq!(host.global_text("order"), "L0W7I7W2W7L7");
    host.restore_ui_state(&state).unwrap();
    assert_eq!(host.global_text("order"), "L0W7I7W2W7L7L7");
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
    assert!(
        !h.findings().iter().any(|f| f.feature.contains("error")),
        "{:?}",
        h.findings()
    );
}

#[test]
fn children_name_lookup_and_synthesis_children_do_not_mix_inserts() {
    let xml = r#"<UVI4><Program><Layers><Layer Name='L'/></Layers><Inserts><OnePole Name='F'/></Inserts>
      <ControlSignalSources><LFO Name='M'/></ControlSignalSources><EventProcessors><ScriptProcessor><script>
      assert(Program.children.L==Program.layers[1])
      assert(Program.children.F==Program.inserts[1])
      assert(#Program.synthChildren==1 and Program.synthChildren[1]==Program.layers[1])
      assert(Program.mods[1]==Program.modulations[1])
      </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(
        !h.findings().iter().any(|f| f.feature.contains("error")),
        "{:?}",
        h.findings()
    );
}

#[test]
fn returned_module_builders_keep_the_resource_source_directory() {
    let mut scripts = sampler_uvi::script::Scripts::default();
    scripts.insert(
        "Scripts/UI/panel.lua",
        "return function() Image('../Textures/face.png') end".into(),
    );
    let h = ScriptHost::new("<UVI4><Program><EventProcessors><ScriptProcessor><script>local build = require('UI/panel'); build()</script></ScriptProcessor></EventProcessors></Program></UVI4>", scripts, Config::default()).unwrap();
    assert_eq!(
        h.interface().assets[0].path,
        "/scripts/ui/../Textures/face.png"
    );
}

#[test]
fn lua_faults_keep_the_first_message_and_count_each_phase_and_category() {
    use sampler_uvi::script::FaultCategory;
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><script>
      function onInit() error('first init fault') end
      function onNote(e) if e.velocity>80 then while true do end else error('later runtime fault') end end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut h = ScriptHost::new(
        xml,
        (),
        Config {
            callback_work: 1000,
            ..Config::default()
        },
    )
    .unwrap();
    h.note_on(1, 60, 64, 0);
    h.note_on(2, 60, 64, 0);
    h.note_on(3, 60, 100, 0);
    let counts = h.fault_counts();
    assert_eq!(counts.first, Some(FaultCategory::Lua));
    assert_eq!(counts.init[&FaultCategory::Lua], 1);
    assert_eq!(counts.runtime[&FaultCategory::Lua], 2);
    assert_eq!(counts.runtime[&FaultCategory::Budget], 1);
    let f = h
        .findings()
        .into_iter()
        .find(|f| f.feature == "lua error")
        .unwrap();
    assert_eq!(f.count, 4);
    assert!(f.value.contains("first init fault"));
}

#[test]
fn retained_connection_and_string_parameters_have_typed_descriptor_identities() {
    let xml = r#"<UVI4><Program><Inserts><OnePole><Connections>
      <SignalConnection Destination='Freq' Ratio='0.3'/>
    </Connections></OnePole></Inserts><EventProcessors><ScriptProcessor><script>
      local e=Program.inserts[1].connections[1]
      local function definition(e,name)
        for _,d in ipairs(e.parameterDefinitions) do if d.name==name then return d end end
      end
      local ratio=definition(e,'Ratio')
      assert(type(ratio.id)=='number' and ratio.type=='float')
      assert(ratio.min==nil and ratio.max==nil)
      assert(e:getParameter(ratio.id)==0.3)
      local destination=definition(e,'Destination')
      assert(destination.type=='string' and e:getParameter(destination.id)=='Freq')
      e:setParameter(destination.id,'Gain')
      assert(e:getParameter('Destination')=='Gain')
      e:setParameter(destination.id,1)
      assert(e:getParameter('Destination')=='Gain')
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(
        !h.findings().iter().any(|f| f.feature == "lua error"),
        "{:?}",
        h.findings()
    );
    assert!(
        h.take_commands().is_empty(),
        "retained XML fields are not DSP writes"
    );
    assert!(h.insert_overrides().is_empty());
}

#[test]
fn restored_widget_callback_faults_are_typed_lua_faults() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor K='7'><script>
      local k=Knob('K',0,0,10)
      function k:changed() error('restore callback fault') end
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(
        h.fault_counts()
            .init
            .get(&sampler_uvi::script::FaultCategory::Lua),
        Some(&1)
    );
}

#[test]
fn mismatched_parameter_scalar_types_are_ignored_without_aborting_lua() {
    let xml = r#"<UVI4><Program><Inserts><OnePole Freq='500'/></Inserts><EventProcessors><ScriptProcessor><script>
      local e=Program.inserts[1]
      e:setParameter('Freq','wrong type')
      assert(e:getParameter('Freq')==500)
      e:setParameter('Bypass',1)
      e:setParameter('Mode',0.5)
      assert(e:getParameter('Mode')==0)
      e:setParameter('Freq',600)
      assert(e:getParameter('Freq')==600)
      assert(e:getParameter('Bypass')==false)
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(!h.findings().iter().any(|f| f.feature == "lua error"));
    let findings = h.fault_counts().setter_type_mismatches;
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().all(|f| f.count == 1));
    assert!(
        h.findings()
            .iter()
            .filter(|f| f.setter_type_mismatch.is_some())
            .all(|f| f.value.is_empty())
    );
}

#[test]
fn unused_leaf_collections_are_lazy_and_keep_identity_when_requested() {
    let xml = r#"<UVI4><Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>
      local leaf=Program.layers[1].keygroups[1].oscillators[1]
      assert(rawget(leaf,'inserts')==nil)
      assert(#leaf.inserts==0 and leaf.inserts==leaf.inserts)
      assert(leaf.mods==leaf.modulations and #leaf.mods==0)
      assert(leaf.inserts~=leaf.parent.inserts)
      assert(Program.layers[1].parent==Program and leaf.parent==Program.layers[1].keygroups[1])
      assert(Program.children[1]==Program.layers[1] and Program.synthChildren[1]==Program.layers[1])
    </script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert!(h.fault_counts().init.is_empty(), "{:?}", h.fault_counts());
}

#[test]
fn finite_live_callback_survives_expired_elapsed_observation() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onNote(e) for n=1,10000 do local x=n*n end; played=true end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut h = ScriptHost::new(
        xml,
        (),
        Config {
            callback: std::time::Duration::ZERO,
            ..Config::default()
        },
    )
    .unwrap();
    h.note_on(1, 60, 64, 0);
    assert_eq!(h.global_text("played"), "true");
    assert!(h.fault_counts().runtime.is_empty());
}
