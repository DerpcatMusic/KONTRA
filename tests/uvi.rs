#![cfg(feature = "uvi")]

use kontakto::uvi::{
    self,
    script::{self, Action, Input, InputKind},
};
use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn initialized_ui_snapshot_preserves_scope_without_running_callbacks() {
    let program = uvi::program::parse_program(
        r#"<Program><EventProcessors>
      <ScriptProcessor><script><![CDATA[
        function onInit()
          setSize(200,100)
          knob=Knob{name='same',value=0.25,bounds={1,2,20,30}}
          knob.changed=function()error('snapshot ran callback')end
        end
      ]]></script></ScriptProcessor>
      </EventProcessors><Layers><Layer><EventProcessors>
      <ScriptProcessor><script><![CDATA[
        function onInit()
          setSize(400,300)
          knob=Knob{name='same',value=0.75,bounds={5,6,40,50}}
          knob.changed=function()error('snapshot ran callback')end
        end
      ]]></script></ScriptProcessor>
      </EventProcessors></Layer></Layers></Program>"#,
    )
    .unwrap();
    let processors: Vec<_> = program
        .nodes
        .iter()
        .enumerate()
        .filter_map(|(id, node)| (node.kind == "ScriptProcessor").then_some(id))
        .collect();
    let mut session = script::Session::new_program_chain(
        &program,
        std::collections::BTreeMap::new(),
        None,
        48000,
    )
    .unwrap();
    let first = session.ui_snapshot(processors[0]).unwrap();
    let second = session.ui_snapshot(processors[1]).unwrap();
    assert!(first.processor != second.processor);
    assert!(first.widgets[0].id == second.widgets[0].id);
    assert!(first.widgets[0].name == second.widgets[0].name);
    assert!(first.root.width == 200. && second.root.width == 400.);
    assert!(matches!(
        first.widgets[0].value,
        Some(uvi::host::UiValue::Number(0.25))
    ));
    assert!(matches!(
        second.widgets[0].value,
        Some(uvi::host::UiValue::Number(0.75))
    ));
    assert!(first == session.ui_snapshot(processors[0]).unwrap());
    assert!(session.ui_snapshot(usize::MAX).is_err());
    assert_eq!(session.current_frame(), 0);
    let drained = session.drain().unwrap();
    assert!(
        drained.commands.is_empty() && drained.host_commands.is_empty() && drained.logs.is_empty()
    );
    drop(session);
    assert!(first.widgets[0].bounds.x == 1. && second.widgets[0].bounds.x == 5.);
}

#[test]
fn clear_mapping_lua_and_audio() {
    let dir = std::env::temp_dir().join(format!(
        "kontra-uvi-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // Original synthetic mono PCM [0.25, -0.5] repeated eight times, encoded
    // with libFLAC 1.5.0; no library content or external test tool required.
    let flac = [
        102, 76, 97, 67, 0, 0, 0, 34, 16, 0, 16, 0, 0, 0, 0, 0, 0, 0, 11, 184, 0, 240, 0, 0, 0, 16,
        73, 240, 157, 222, 104, 2, 252, 104, 192, 100, 81, 178, 128, 170, 31, 113, 132, 0, 0, 40,
        32, 0, 0, 0, 114, 101, 102, 101, 114, 101, 110, 99, 101, 32, 108, 105, 98, 70, 76, 65, 67,
        32, 49, 46, 53, 46, 48, 32, 50, 48, 50, 53, 48, 50, 49, 49, 0, 0, 0, 0, 255, 248, 106, 8,
        0, 15, 10, 3, 0, 9, 199, 28, 113, 199, 28, 112, 144, 62,
    ];
    let mut bytes = vec![42; 328];
    bytes.extend(flac);
    bytes.extend([42; 1024]);
    let bank = dir.join("synthetic.ufs");
    std::fs::write(&bank, &bytes).unwrap();
    let mut member = uvi::open_member(&bank, 328, flac.len() as u64).unwrap();
    assert_eq!(member.header().frames, 16);
    assert_eq!(member.header().rate, 48000);
    let mut decoded = [[0.; 2]; 16];
    member.read(0, &mut decoded).unwrap();
    for (i, frame) in decoded.iter().enumerate() {
        assert_eq!(*frame, [if i % 2 == 0 { 0.25 } else { -0.5 }; 2]);
    }
    assert!(uvi::open_member(&bank, u64::MAX, 100).is_err());
    let mut truncated = uvi::open_member(&bank, 328, flac.len() as u64 - 1).unwrap();
    assert!(truncated.read(0, &mut decoded).is_err());
    for (name, value) in [("a.wav", 0.8f32), ("b.wav", 0.4), ("offset.wav", 0.5)] {
        let mut wav = hound::WavWriter::create(
            dir.join(name),
            hound::WavSpec {
                channels: 2,
                sample_rate: 48000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for frame in 0..12000 {
            let value = if name == "offset.wav" && frame < 480 {
                0.
            } else {
                value
            };
            wav.write_sample(value).unwrap();
            wav.write_sample(value).unwrap();
        }
        wav.finalize().unwrap();
    }
    let mapping = uvi::parse_mapping(&dir.join("ours.dmap"), r#"<layers maxSampleStart="20" Future="preserved">
      <layer name="sustain"><zone path="a.wav" baseNote="60" lowVel="0" tune="1200" gain="-6"/></layer>
      <layer name="short" maxSampleStart="30">
        <zone path="a.wav" rr="1"/><zone path="b.wav" rr="2" maxSampleStart="40"/>
        <zone path="not-loaded.wav" rr="3" purged="1"/>
        <zone lowKey="80" highKey="20" path="bad.wav"/><zone/>
      </layer></layers>"#).unwrap();
    assert_eq!(mapping.attributes["Future"], "preserved");
    assert_eq!(mapping.zones[0].zone.low_velocity, 1);
    assert_eq!(mapping.zones[0].zone.tune, 2.);
    assert!((mapping.zones[0].zone.gain - 0.5011872).abs() < 1e-6);
    assert_eq!(mapping.zones[1].max_sample_start_ms, 30.);
    assert_eq!(mapping.zones[2].max_sample_start_ms, 40.);
    assert_eq!(mapping.warnings.len(), 2);
    let inputs = script::parse_notes("60@0-100:100").unwrap();
    let commands = script::process(
        r#"
      assert(_VERSION == 'Lua 5.1')
      assert(os == nil and io == nil and package == nil and debug == nil)
      assert(dofile == nil and loadfile == nil and coroutine == nil)
      function onNote(e) error('onEvent must take precedence') end
      function onEvent(e)
        if e.type == Event.NoteOn then
          e.dim1 = 1; e.dim2 = 1
          local id = postEvent(e)
          wait(10)
          changeVolume(id, 0.5, false, true)
        else postEvent(e) end
      end
    "#,
        "ours.lua",
        &inputs,
        14400,
    )
    .unwrap();
    assert_eq!(commands.len(), 3);
    assert_eq!(
        commands.iter().map(|c| c.frame).collect::<Vec<_>>(),
        [0, 480, 4800]
    );
    assert!(matches!(
        commands[2].action,
        Action::ReleaseNote {
            id: 1,
            note: 60,
            channel: 0,
            layer: None
        }
    ));
    let mut audio = Vec::new();
    uvi::render(&mapping, &commands, 14400, |f| {
        audio.push(f);
        Ok(())
    })
    .unwrap();
    assert!(audio.iter().all(|f| f.iter().all(|v| v.is_finite())));
    assert!(
        (audio[1200][0] - 0.2).abs() < 1e-5,
        "Selected dimension/variant, then Lua gain: {:?}",
        audio[1200]
    );
    assert_eq!(audio.last().unwrap(), &[0., 0.]);

    // Two physical roots of the same key retain separate generated children.
    let overlap = script::parse_notes("60@0-100:90,60@10-200:100").unwrap();
    let children = script::process(
        "function onNote(e) playNote(e.note+12,e.velocity) end\nfunction onRelease(e) end",
        "children.lua",
        &overlap,
        12000,
    )
    .unwrap();
    assert!(matches!(children[0].action, Action::Start(ref n) if n.id == 2));
    assert!(matches!(children[1].action, Action::Start(ref n) if n.id == 4));
    assert!(
        matches!(
            children[2].action,
            Action::ReleaseNote {
                id: 2,
                note: 72,
                layer: None,
                ..
            }
        ) && children[2].frame == 4800
    );
    assert!(
        matches!(
            children[3].action,
            Action::ReleaseNote {
                id: 4,
                note: 72,
                layer: None,
                ..
            }
        ) && children[3].frame == 9600
    );
    let delayed = script::process(
        "function onNote(e) wait(150);playNote(e.note,e.velocity) end\nfunction onRelease(e) end",
        "late.lua",
        &inputs,
        12000,
    )
    .unwrap();
    assert!(!delayed.iter().any(|c| matches!(c.action, Action::Start(_))));
    let delayed_change = script::process(
        "function onNote(e) local id=postEvent(e,20);changeVolume(id,0.25,false,true) end",
        "pending.lua",
        &inputs,
        12000,
    )
    .unwrap();
    assert!(
        matches!(delayed_change[0].action, Action::Start(ref n) if n.volume == 1.)
            && delayed_change[0].frame == 960
    );
    assert!(
        !delayed_change
            .iter()
            .any(|c| matches!(c.action, Action::Change { .. }))
    );
    script::process("function onNote(e) local id=playNote(e.note,e.velocity,10);wait(20);assert(not releaseVoice(id)) end", "finite.lua", &inputs, 12000).unwrap();

    let cc = [Input {
        frame: 480,
        kind: InputKind::Controller {
            channel: 1,
            controller: 11,
            value: 80,
        },
    }];
    let commands = script::process(
        "function onController(e) assert(getCC(11)==80);postEvent(e) end",
        "cc.lua",
        &cc,
        960,
    )
    .unwrap();
    assert!(matches!(
        commands[0].action,
        Action::Controller {
            channel: 1,
            controller: 11,
            value: 80
        }
    ));
    let transparent = script::process("", "empty.lua", &inputs, 12000).unwrap();
    assert_eq!(transparent.len(), 2);
    let fades = script::process(
        "function onNote(e) fadein(postEvent(e),20,true) end",
        "fade.lua",
        &inputs,
        12000,
    )
    .unwrap();
    let mut baseline = Vec::new();
    let mut faded = Vec::new();
    uvi::render(&mapping, &transparent, 12000, |frame| {
        baseline.push(frame);
        Ok(())
    })
    .unwrap();
    uvi::render(&mapping, &fades, 12000, |frame| {
        faded.push(frame);
        Ok(())
    })
    .unwrap();
    assert!(baseline[480][0].abs() > 0.1);
    assert!((faded[480][0] / baseline[480][0] - 0.5).abs() < 0.002);
    assert!((faded[960][0] - baseline[960][0]).abs() < 1e-6);
    // Native repeated forwarding retains one ID for multiple audible voices.
    let mut duplicated = transparent.clone();
    duplicated.insert(1, duplicated[0].clone());
    let mut doubled = Vec::new();
    uvi::render(&mapping, &duplicated, 12000, |frame| {
        doubled.push(frame);
        Ok(())
    })
    .unwrap();
    for at in [480, 1200] {
        assert!((doubled[at][0] - 2. * baseline[at][0]).abs() < 1e-6);
    }
    // One terminal NoteOff releases the oldest matching post; its sibling
    // remains held, as in the native duplicate-ID/key oracle.
    let mut held = Vec::new();
    uvi::render(&mapping, &transparent[..1], 12000, |frame| {
        held.push(frame);
        Ok(())
    })
    .unwrap();
    assert!(held[5500][0].abs() > baseline[5500][0].abs());
    assert!((doubled[5500][0] - baseline[5500][0] - held[5500][0]).abs() < 1e-6);
    // Relative changes apply to each duplicate's own base value.
    let mut initial = transparent[0].clone();
    if let Action::Start(ref mut note) = initial.action {
        note.volume = 0.25;
    }
    for (relative, expected) in [(true, 0.625), (false, 1.)] {
        let changes = [
            initial.clone(),
            transparent[0].clone(),
            script::Command {
                frame: 480,
                action: Action::Change {
                    id: 1,
                    gain: Some(0.5),
                    tune: None,
                    pan: None,
                    layer: None,
                    relative,
                },
            },
        ];
        let mut changed = Vec::new();
        uvi::render(&mapping, &changes, 1440, |frame| {
            changed.push(frame);
            Ok(())
        })
        .unwrap();
        assert!((changed[1200][0] - expected * held[1200][0]).abs() < 1e-6);
    }
    // A released engine voice remains controllable while its tail drains.
    let mut stopped = transparent.clone();
    stopped.push(script::Command {
        frame: 4801,
        action: Action::Change {
            id: 1,
            gain: Some(0.),
            tune: None,
            pan: None,
            layer: None,
            relative: false,
        },
    });
    let mut tail = Vec::new();
    uvi::render(&mapping, &stopped, 5000, |frame| {
        tail.push(frame);
        Ok(())
    })
    .unwrap();
    // The existing mapping engine ramps gain across its render block.
    assert!(baseline[4930][0].abs() > 0.);
    assert_eq!(tail[4930], [0., 0.]);
    let offset_mapping = uvi::parse_mapping(
        &dir.join("offset.dmap"),
        "<layers maxSampleStart='20'><layer><zone path='offset.wav'/></layer></layers>",
    )
    .unwrap();
    let shifted = script::process(
        "function onNote(e) setSampleOffset(postEvent(e),15) end",
        "offset.lua",
        &inputs,
        12000,
    )
    .unwrap();
    let mut start = Vec::new();
    uvi::render(&offset_mapping, &shifted, 240, |frame| {
        start.push(frame);
        Ok(())
    })
    .unwrap();
    assert!(
        start[120][0] > 0.49,
        "Authored maxSampleStart must admit the Lua sample offset"
    );
    start.clear();
    uvi::render(&offset_mapping, &transparent, 240, |frame| {
        start.push(frame);
        Ok(())
    })
    .unwrap();
    assert!(start.iter().all(|frame| *frame == [0.; 2]));
    assert!(
        script::process("function onNote(e) end", "swallow.lua", &inputs, 12000)
            .unwrap()
            .iter()
            .all(|c| !matches!(c.action, Action::Start(_)))
    );

    // Limits cover initialization, host-created coroutines and protected calls.
    for source in [
        "while true do end",
        "spawn(function() while true do end end)",
        "while true do pcall(function() while true do end end) end",
    ] {
        let error = script::process(source, "budget.lua", &[], 0).unwrap_err();
        assert!(
            format!("{error:#}").contains("instruction budget"),
            "{error:#}"
        );
    }
    assert!(script::parse_notes("60@NaN-100:100").is_err());
    assert!(
        script::process(
            "function onNote(e) changeTune(postEvent(e),1) end",
            "smooth.lua",
            &inputs,
            12000
        )
        .is_err()
    );
    assert!(
        uvi::parse_mapping(
            Path::new("bad.dmap"),
            "<!DOCTYPE layers><layers><layer><zone path='a.wav'/></layer></layers>"
        )
        .is_err()
    );
    let deep = format!(
        "<UVI4><Program>{}<x/>{}</Program></UVI4>",
        "<x>".repeat(65),
        "</x>".repeat(65)
    );
    assert!(uvi::inspect_preset(&deep).is_err());
    let preset = uvi::inspect_preset("<UVI4><Program Name='ours'><Layers><Layer><Keygroups><Keygroup><UnknownOscillator Mode='42'/></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script>function onNote(e) postEvent(e) end</script></ScriptProcessor></EventProcessors></Program></UVI4>").unwrap();
    assert_eq!(
        preset
            .nodes
            .iter()
            .filter(|n| n.kind == "ScriptProcessor")
            .count(),
        1
    );
    let module = preset
        .nodes
        .iter()
        .find(|n| n.kind == "UnknownOscillator")
        .unwrap();
    assert_eq!(module.attributes["Mode"], "42");
    assert!(module.parent.is_some());
    assert!(
        serde_json::to_string(&preset)
            .unwrap()
            .contains("UnknownOscillator")
    );
    assert!(
        !serde_json::to_string(&preset)
            .unwrap()
            .contains("function onNote")
    );
    std::fs::remove_dir_all(dir).unwrap();
}
