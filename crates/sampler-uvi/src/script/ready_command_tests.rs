use super::*;

fn host(script: &str) -> ScriptHost {
    let xml = format!(
        "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"
    );
    ScriptHost::new(&xml, (), Config::default()).unwrap()
}

fn midi(value: u8) -> Command {
    Command::Midi(MidiOut {
        status: 0xb0,
        a: 1,
        b: value,
    })
}

#[test]
fn ready_batch_requires_all_commands_to_fit_and_never_mutates_a_fallback() {
    for (count, free) in [(0, 1024), (3, 0), (3, 2), (1025, 2048)] {
        let h = host("");
        let expected = vec![midi(17); count];
        *h.shared.commands.borrow_mut() = expected.clone();
        assert!(h.take_ready_command_batch(free).is_empty());
        assert_eq!(*h.shared.commands.borrow(), expected);
        assert_eq!(h.shared.ready_command_charge.get(), 0);
    }
    let mut h = host("");
    let expected = vec![midi(17), midi(18), midi(19)];
    *h.shared.commands.borrow_mut() = expected.clone();
    assert_eq!(h.take_ready_command_batch(3), expected);
    assert!(h.shared.commands.borrow().is_empty());
    assert_eq!(h.shared.ready_command_charge.get(), 3);
    assert!(h.shared.command(midi(20)));
    assert!(
        h.take_ready_command_batch(1024).is_empty(),
        "one lease per collector boundary"
    );
    assert_eq!(h.take_commands(), vec![midi(20)]);
    assert_eq!(h.shared.ready_command_charge.get(), 0);
    assert!(h.shared.command(midi(21)));
    assert_eq!(h.take_ready_command_batch(1024), vec![midi(21)]);
}

#[test]
fn unsafe_command_at_any_position_keeps_the_entire_ready_batch_untouched() {
    let unsafe_commands = [
        Command::EngineParameter {
            address: sampler_core::EngineParameterAddress {
                parameter: sampler_core::engine_parameter_id("ENGINE_PAR_CUTOFF").unwrap(),
                group: -1,
                slot: -1,
                generic: 0,
            },
            value: 123,
        },
        Command::Impulse {
            slot: 0,
            upload: ImpulseUpload(std::sync::Arc::new(std::sync::Mutex::new(None))),
        },
    ];
    for unsafe_command in unsafe_commands {
        for position in 0..3 {
            let mut h = host("");
            let mut expected = vec![midi(17), midi(18)];
            expected.insert(position, unsafe_command.clone());
            *h.shared.commands.borrow_mut() = expected.clone();
            assert!(h.take_ready_command_batch(1024).is_empty());
            assert_eq!(*h.shared.commands.borrow(), expected);
            assert_eq!(h.shared.ready_command_charge.get(), 0);
            assert_eq!(h.take_commands(), expected);
        }
    }
}

#[test]
fn ready_batch_charge_preserves_getter_command_capacity_findings_and_fifo() {
    let script = r#"
      armed=false
      local label=Label('Readback')
      setmetatable(rawget(label,'__data'),{__index=function(_,key)
        if armed and key=='fontSize' then
          armed=false
          for i=1,65536 do postEvent{type=Event.Controller,controller=1,value=i%128} end
          return 12
        end
      end})
      function onNote(e) playNote(e.note,e.velocity) armed=true end
    "#;
    let mut reference = host(script);
    let mut candidate = host(script);
    reference.note_on(1, 60, 64, 0);
    candidate.note_on(1, 60, 64, 0);
    let mut commands = candidate.take_ready_command_batch(1024);
    assert!(matches!(&commands[..], [Command::Play(_)]));
    assert_eq!(candidate.interface(), reference.interface());
    commands.extend(candidate.take_commands());
    let expected = reference.take_commands();
    assert_eq!(commands.len(), 65536);
    assert_eq!(commands, expected);
    assert_eq!(candidate.findings(), reference.findings());
    assert!(
        candidate
            .findings()
            .iter()
            .any(|f| f.feature == "command queue full, commands dropped")
    );
    assert_eq!(candidate.shared.ready_command_charge.get(), 0);
    assert!(
        candidate.shared.command(midi(22)),
        "normal collector releases the charge"
    );
}

#[test]
fn stale_convolver_fields_keep_the_note_with_the_original_late_collector() {
    struct Convolvers;
    impl Files for Convolvers {
        fn script(&self, _: &str) -> Option<String> {
            None
        }
        fn convolver_slot(&self, _: usize) -> Option<usize> {
            Some(0)
        }
    }
    let xml = r#"<UVI4><Program><Auxs><AuxEffect><Inserts><Convolver Dry='0' Wet='1'/></Inserts></AuxEffect></Auxs>
      <EventProcessors><ScriptProcessor><script><![CDATA[
        local c=Program.auxs[1].inserts[1]
        function onNote(e) c:setParameter('Wet',0.5) c:setParameter('Dry',0.25) playNote(e.note,e.velocity) end
      ]]></script></ScriptProcessor></EventProcessors></Program></UVI4>"#;
    let mut h = ScriptHost::new(xml, Convolvers, Config::default()).unwrap();
    h.take_commands();
    h.note_on(1, 60, 64, 0);
    assert_eq!(h.shared.stale_impulses.borrow().len(), 1);
    let queued = h.shared.commands.borrow().clone();
    assert!(matches!(&queued[..], [Command::Play(_)]));
    assert!(h.take_ready_command_batch(1024).is_empty());
    assert_eq!(*h.shared.commands.borrow(), queued);
    assert_eq!(
        h.shared.stale_impulses.borrow().len(),
        1,
        "no early rebuild"
    );
    let collected = h.take_commands();
    assert!(matches!(
        &collected[..],
        [Command::Play(_), Command::Impulse { slot: 0, .. }]
    ));
    assert!(
        h.take_commands().is_empty(),
        "all stale fields coalesce to one upload"
    );
}
