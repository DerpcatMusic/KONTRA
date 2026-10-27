//! Authored conformance regressions, exercised in normal CI. The original
//! 2026-10-08 audit probes were verified green and promoted on 2026-10-12.
use sampler_uvi::script::{Command, Config, ScriptHost};

fn host(script: &str, saved: &str) -> ScriptHost {
    ScriptHost::new(
        &format!(
            r#"<UVI4><Program Name="P"><Layers><Layer Name="L"><Keygroups>
        <Keygroup Name="K"><Oscillators><SamplePlayer Name="O"/></Oscillators></Keygroup>
        </Keygroups></Layer></Layers><Inserts><OnePole Name="F"/></Inserts>
        <EventProcessors><ScriptProcessor Name="S" {saved}><script><![CDATA[{script}]]></script>
        </ScriptProcessor></EventProcessors></Program></UVI4>"#
        ),
        (),
        Config::default(),
    )
    .unwrap()
}

#[test]
fn forwarded_note_keeps_identity() {
    let mut h = host("function onNote(e) result = postEvent(e) end", "");
    h.note_on(42, 60, 100, 0);
    assert_eq!(h.global_text("result"), "42");
}

#[test]
fn delayed_event_returns_its_actual_voice_id() {
    let mut h = host("function onNote(e) result = postEvent(e, 10) end", "");
    h.note_on(42, 60, 100, 0);
    h.advance(10.0);
    let id = h.global_text("result").parse::<u64>().unwrap();
    assert!(
        h.take_commands()
            .iter()
            .any(|c| matches!(c, Command::Play(p) if p.id == id))
    );
}

#[test]
fn release_nonexistent_voice_is_false() {
    let h = host("result = releaseVoice(987654)", "");
    assert_eq!(h.global_text("result"), "false");
}

#[test]
fn playnote_numeric_table_keeps_layer_and_oscillator() {
    let mut h = host(
        "playNote{60, 100, 0, 1, nil, nil, 0.5, -0.2, 2, nil, 3}",
        "",
    );
    let p = h
        .take_commands()
        .into_iter()
        .find_map(|c| match c {
            Command::Play(p) => Some(p),
            _ => None,
        })
        .unwrap();
    assert!(p.layers.contains(1));
    assert_eq!((p.vol, p.pan, p.tune, p.osc), (0.5, -0.2, 2.0, Some(3)));
}

#[test]
fn pitch_callback_exposes_bend() {
    let mut h = host("function onPitchBend(e) result = e.bend end", "");
    h.pitch_bend(0.5, 0);
    assert_eq!(h.global_text("result"), "0.5");
}

#[test]
fn program_callback_exposes_program() {
    let mut h = host("function onProgramChange(e) result = e.program end", "");
    h.program_change(7, 0);
    assert_eq!(h.global_text("result"), "7");
}

#[test]
fn note_held_does_not_follow_another_key() {
    let mut h = host(
        "function onNote(e) if e.note == 60 then wait(10); result = isNoteHeld() end end",
        "",
    );
    h.note_on(1, 60, 100, 0);
    h.note_on(2, 62, 100, 0);
    h.note_off(1, 60, 64, 0);
    h.advance(10.0);
    assert_eq!(h.global_text("result"), "false");
}

#[test]
fn spawn_does_not_inherit_release_wait() {
    let mut h = host(
        "function onNote(e) spawn(function() held = isNoteHeld(); waitForRelease(); result = true end) end",
        "",
    );
    h.note_on(1, 60, 100, 0);
    assert_eq!(h.global_text("held"), "false");
    assert_eq!(h.global_text("result"), "true");
    assert!(h.next_due().is_none());
    h.note_off(1, 60, 64, 0);
    assert_eq!(h.global_text("result"), "true");
    assert!(h.fault_counts().runtime.is_empty());
}

#[test]
fn run_has_independent_note_context_and_preserves_immediate_callback_order() {
    let mut h = host(
        "trace = ''; function onNote(e)\n\
           trace = trace .. 'note' .. e.id .. ':'\n\
           run(function(id) assert(not isNoteHeld()); waitForRelease(); trace = trace .. 'run' .. id .. ':' end, e.id)\n\
           assert(isNoteHeld()); trace = trace .. 'caller:'; waitForRelease()\n\
           assert(not isNoteHeld()); trace = trace .. 'released' .. e.id .. ':'\n\
         end\n\
         function onRelease(e) trace = trace .. 'off' .. e.id .. ':' end",
        "",
    );
    h.note_on(42, 60, 100, 0);
    assert_eq!(h.global_text("trace"), "note42:run42:caller:");
    h.note_off(99, 60, 64, 0);
    assert_eq!(h.global_text("trace"), "note42:run42:caller:off99:");
    h.note_off(42, 60, 64, 0);
    assert_eq!(
        h.global_text("trace"),
        "note42:run42:caller:off99:released42:off42:"
    );
    assert!(h.fault_counts().runtime.is_empty());
}

#[test]
fn release_wait_returns_immediately_in_controller_and_after_its_note_was_released() {
    let mut h = host(
        "function onController(e) assert(not isNoteHeld()); waitForRelease(); controllerDone = true end\n\
         function onNote(e) wait(10); waitForRelease(); releasedId = e.id; playNote(e.note + 12, 100) end",
        "",
    );
    h.controller(64, 127, 0);
    assert_eq!(h.global_text("controllerDone"), "true");
    h.note_on(7, 60, 100, 0);
    h.note_off(7, 60, 64, 0);
    h.advance(10.);
    assert_eq!(h.global_text("releasedId"), "7");
    assert!(h.take_commands().iter().any(
        |command| matches!(command, Command::Play(play) if play.key == 72 && play.at_ms == 10.)
    ));
    assert!(h.next_due().is_none());
    assert!(h.fault_counts().runtime.is_empty());
}

#[test]
fn release_wait_stays_forbidden_in_synchronous_widget_callbacks_but_spawn_is_allowed() {
    let mut direct = host(
        "local knob = Knob('Value', 0, 0, 1); function knob:changed() waitForRelease(); after = true end",
        "",
    );
    let id = sampler_uvi::script::control_id(1, 0);
    assert!(direct.set_control(id, 1.).is_err());
    direct.advance(10.);
    assert_eq!(direct.global_text("after"), "nil");

    let mut spawned = host(
        "local knob = Knob('Value', 0, 0, 1); function knob:changed() spawn(function() waitForRelease(); after = true end) end",
        "",
    );
    assert!(spawned.set_control(id, 1.).is_ok());
    assert_eq!(spawned.global_text("after"), "true");
    assert!(spawned.fault_counts().runtime.is_empty());
}

#[test]
fn spawn_runs_in_creation_order() {
    let h = host(
        "result = ''; spawn(function() result = result .. 'a' end); spawn(function() result = result .. 'b' end); spawn(function() result = result .. 'c' end)",
        "",
    );
    assert_eq!(h.global_text("result"), "abc");
}

#[test]
fn positional_widget_restores_saved_value() {
    let h = host(
        "local k = Knob('K', 0, 0, 10); function onInit() result = k.value end",
        "K=\"7\"",
    );
    assert_eq!(h.global_text("result"), "7");
}

#[test]
fn unset_widget_changed_is_nil() {
    let h = host(
        "local k = Knob('K', 0, 0, 10); result = (k.changed == nil)",
        "",
    );
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn filter_definition_uses_catalog_range() {
    let h = host(
        "for _, d in ipairs(Program.inserts[1].parameterDefinitions) do if d.name == 'Freq' then result = d.max; idtype = type(d.id) end end",
        "",
    );
    assert_eq!(h.global_text("result"), "20000");
    assert_eq!(h.global_text("idtype"), "number");
}

#[test]
fn omitted_gain_defaults_to_unity() {
    let h = host("result = Program.layers[1]:getParameter('Gain')", "");
    assert_eq!(h.global_text("result"), "1");
}

#[test]
fn parameter_boolean_type_is_preserved() {
    let xml = "<UVI4><Program Bypass='0'><EventProcessors><ScriptProcessor><script>result = type(Program:getParameter('Bypass'))</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(h.global_text("result"), "boolean");
}

#[test]
fn connections_do_not_fabricate_entries() {
    let h = host(
        "result = (Program.layers[1]:getParameterConnections('Gain') == nil)",
        "",
    );
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn synthesis_tree_aliases_exist() {
    let h = host(
        "result = (Program.part == Program.parent and Program.part ~= nil and Program.synthChildren ~= nil and Program.children ~= nil and Program.eventProcessors ~= nil)",
        "",
    );
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn part_gain_connections_are_iterable_by_scripts() {
    // hornScript3 loops `pairs(Program.parent:getParameterConnections("Gain"))` unguarded.
    let h = host(
        "n = 0; for _, c in pairs(Program.parent:getParameterConnections('Gain')) do c:setParameter('Bypass', true); n = n + 1 end; result = n",
        "",
    );
    assert_eq!(h.global_text("result"), "2");
    assert!(
        !h.findings().iter().any(|f| f.feature.contains("error")),
        "{:?}",
        h.findings()
    );
}

#[test]
fn missing_load_data_does_not_call_callback() {
    // Vendor loadData explicitly excludes unreadable files from callback delivery.
    // This owned-file characterization is not an executed native parity receipt.
    let dir =
        std::env::temp_dir().join(format!("kontra-owned-missing-data-{}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let path = serde_json::to_string(dir.join("missing.json").to_str().unwrap()).unwrap();
    let mut h = host(
        &format!(
            "result = false; task = loadData({path}, function(data) result = true end); spawn(function() while not task.finished do wait(1) end; done = task.success end)"
        ),
        "",
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut time = 0.0;
    while h.global_text("done") != "true" {
        assert!(std::time::Instant::now() < deadline, "{:?}", h.findings());
        time += 1.0;
        h.advance(time);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(h.global_text("result"), "false");
    drop(h);
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn stopped_transport_beat_is_not_elapsed_time() {
    let mut h = host("function onController(e) result = getBeatTime() end", "");
    h.transport(false);
    h.advance(1000.0);
    h.controller(1, 100, 0);
    assert_eq!(h.global_text("result"), "0");
}

#[test]
fn undefined_global_remains_nil_even_when_called_later() {
    let h = host("result = (Missing == nil); if false then Missing() end", "");
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn uvi_bit_library_is_available() {
    let h = host("result = bit.band(7, 3)", "");
    assert_eq!(h.global_text("result"), "3");
}

#[test]
fn milliseconds_beats_and_run_order_baseline() {
    let mut h = host(
        "result = ''; run(function() result = result .. 'a'; waitBeat(0.25); result = result .. 'c'; at = getTime() end); result = result .. 'b'",
        "",
    );
    assert_eq!(h.global_text("result"), "ab");
    h.advance(124.9);
    assert_eq!(h.global_text("result"), "ab");
    h.advance(125.0);
    assert_eq!(h.global_text("result"), "abc");
    assert_eq!(h.global_text("at"), "125");
}

fn runtime() -> sampler_core::Runtime {
    use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
    let plan = Prepared::new(
        48000,
        vec![Pcm::new(48000, Box::from([[0.25; 2]; 256])).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap();
    let limits = Limits::for_plan(&plan, 32, 32);
    Runtime::new(plan, limits).unwrap()
}

fn input(key: u8) -> sampler_core::Input {
    sampler_core::Input {
        protocol: sampler_core::Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}

#[test]
fn driver_noteoff_closes_original_physical_note() {
    let h = host("function onNote(e) playNote(e.note, e.velocity, 0) end", "");
    let mut driver = sampler_uvi::scripted::Driver::new(h, vec![], 48000);
    let mut rt = runtime();
    let physical = rt.note_on(input(60), 60, 1.0).unwrap();
    driver.note_on(&mut rt, physical, 60, 1.0).unwrap();
    driver.note_off(&mut rt, 60).unwrap();
    assert!(
        !rt.note(physical).unwrap().2,
        "physical gate leaked after child id overwrote it"
    );
}

#[test]
fn driver_releases_both_repeated_notes() {
    let h = host("function onNote(e) end", "");
    let mut driver = sampler_uvi::scripted::Driver::new(h, vec![], 48000);
    let mut rt = runtime();
    let a = rt.note_on(input(60), 60, 1.0).unwrap();
    driver.note_on(&mut rt, a, 60, 1.0).unwrap();
    let b = rt.note_on(input(60), 60, 1.0).unwrap();
    driver.note_on(&mut rt, b, 60, 1.0).unwrap();
    driver.note_off(&mut rt, 60).unwrap();
    driver.note_off(&mut rt, 60).unwrap();
    assert!(!rt.note(a).unwrap().2 && !rt.note(b).unwrap().2);
}

#[test]
fn oscillator_gain_emits_an_engine_write() {
    let mut h = host(
        "Program.layers[1].keygroups[1].oscillators[1]:setParameter('Gain', 0.5)",
        "",
    );
    assert!(!h.take_commands().is_empty());
}

#[test]
fn keygroup_writes_keep_distinct_scopes() {
    let xml = "<UVI4><Program><Layers><Layer><Keygroups><Keygroup/><Keygroup/></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>for _, k in ipairs(Program.layers[1].keygroups) do k:setParameter('Gain', 0.5) end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut h = ScriptHost::new(xml, (), Config::default()).unwrap();
    let commands = h.take_commands();
    assert_eq!(commands.len(), 2);
    assert_ne!(commands[0], commands[1]);
}

#[test]
fn fade_layer_argument_is_preserved() {
    let mut h = host("fade(42, 0.5, 100, 1); fade(42, 0.5, 100, 2)", "");
    let commands = h.take_commands();
    assert_ne!(commands[0], commands[1]);
}

#[test]
fn change_immediate_flag_is_preserved() {
    let mut h = host(
        "changeTune(42, 1, false, false); changeTune(42, 1, false, true)",
        "",
    );
    let commands = h.take_commands();
    assert_ne!(commands[0], commands[1]);
}

#[test]
fn table_positional_constructor_preserves_values() {
    let h = host(
        "local t = Table('T', 2, 0.25, -1, 1, false); result = t:getValue(1); lo = t.min",
        "",
    );
    assert_eq!(h.global_text("result"), "0.25");
    assert_eq!(h.global_text("lo"), "-1");
}

#[test]
fn table_setvalue_can_suppress_callback() {
    let h = host(
        "local t = Table{'T', 2, 0, 0, 1}; result = 0; function t:changed(i) result = result + 1 end; t:setValue(1, 0.5, false)",
        "",
    );
    assert_eq!(h.global_text("result"), "0");
}

#[test]
fn persistent_false_assignment_prevents_restore() {
    let h = host(
        "local k = Knob{'K', 0, 0, 10}; k.persistent = false; function onInit() result = k.value end",
        "K=\"7\"",
    );
    assert_eq!(h.global_text("result"), "0");
}

#[test]
fn public_controlchange_event_constant_exists() {
    let mut h = host(
        "function onController(e) result = (e.type == Event.ControlChange) end",
        "",
    );
    h.controller(1, 100, 0);
    assert_eq!(h.global_text("result"), "true");
}

#[test]
fn custom_saved_state_invokes_onload() {
    let xml = r#"<UVI4><Program><EventProcessors><ScriptProcessor><state>{"counter":42}</state><script>loaded = false; function onLoad(data) assert(data.counter == 42); loaded = true end</script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(h.global_text("loaded"), "true");
}

#[test]
fn automatic_scriptdata_does_not_invoke_onload() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><ScriptData counter='42'/><script>loaded = false; function onLoad(data) loaded = true end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let h = ScriptHost::new(xml, (), Config::default()).unwrap();
    assert_eq!(h.global_text("loaded"), "false");
}

#[test]
fn bypassed_processor_does_not_swallow_notes() {
    let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script>function onNote(e) postEvent(e) end</script></ScriptProcessor><ScriptProcessor Bypass='1'><script>function onNote(e) end</script></ScriptProcessor></EventProcessors></Program></UVI4>";
    let mut h = ScriptHost::new(xml, (), Config::default()).unwrap();
    h.note_on(42, 60, 100, 0);
    assert!(
        h.take_commands()
            .iter()
            .any(|c| matches!(c, Command::Play(_)))
    );
}
