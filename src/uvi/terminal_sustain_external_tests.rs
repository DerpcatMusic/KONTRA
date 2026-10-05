// External review fixture; parent validates it on frozen before/after checkouts.
// Intended as a child test module of src/uvi/playback.rs.
// Retained native DLL version/hash is unbound: this is a historical capture oracle.
use super::*;
use std::collections::BTreeMap;

const ROOT: &str = "/tmp/kontakto-overnight-handoff/uvi-terminal-sustain-regression";

#[test]
fn terminal_layer_sustain_matches_retained_complete_native_pcm() {
    for case in [
        "pedal-channel0",
        "pedal-layer0-drop64",
        "pedal-layer0-synthetic64",
    ] {
        compare_case(case, false);
    }
}

#[test]
fn pedal_down_before_note_on_preserves_attack_and_complete_held_pcm() {
    compare_case("pedal-channel0", true);
}

fn compare_case(case: &str, pedal_before_on: bool) {
    let xml = std::fs::read_to_string(format!("{ROOT}/{case}.xml")).unwrap();
    let program = super::super::program::parse_program(&xml).unwrap();
    assert!(preflight(&program).is_empty(), "{case}: admission");
    let samples = [440, 880]
        .into_iter()
        .map(|hz| {
            let name = format!("tone-{hz}.wav");
            let bytes = std::fs::read(format!("{ROOT}/{name}")).unwrap();
            (
                name,
                Arc::new(super::super::sample::decode(&bytes).unwrap()),
            )
        })
        .collect();
    let mut renderer = Renderer::new(&program, samples, 48_000).unwrap();
    let mut session =
        script::Session::new_program_chain(&program, BTreeMap::new(), None, 48_000).unwrap();
    let events = std::fs::read_to_string(format!("{ROOT}/{case}-events.txt")).unwrap();
    let mut inputs: Vec<_> = events
        .lines()
        .map(|line| {
            let row: Vec<u64> = line
                .split_whitespace()
                .map(|v| v.parse().unwrap())
                .collect();
            let channel = (row[1] & 15) as u8;
            let kind = match row[1] & 0xf0 {
                0x90 => script::InputKind::NoteOn {
                    channel,
                    note: row[2] as u8,
                    velocity: row[3] as u8,
                },
                0x80 => script::InputKind::NoteOff {
                    channel,
                    note: row[2] as u8,
                    velocity: row[3] as u8,
                },
                0xb0 => script::InputKind::Controller {
                    channel,
                    controller: row[2] as u8,
                    value: row[3] as u8,
                },
                _ => panic!("unexpected authored input"),
            };
            script::Input {
                frame: row[0],
                kind,
            }
        })
        .collect();
    if pedal_before_on {
        // The authored amplitude route does not consume CC64. Moving its down edge
        // before On must leave attack/held PCM equal to the retained late-Off reference.
        let index = inputs
            .iter()
            .position(|input| {
                matches!(
                    input.kind,
                    script::InputKind::Controller {
                        controller: 64,
                        value: 127,
                        ..
                    }
                )
            })
            .unwrap();
        let mut down = inputs.remove(index);
        down.frame = 0;
        inputs.insert(0, down);
    }
    let expected = std::fs::read(format!("{ROOT}/{case}.stereo-f32")).unwrap();
    assert_eq!(expected.len(), 96_000 * 2 * 4);
    for start in (0..96_000u64).step_by(256) {
        let block: Vec<_> = inputs
            .iter()
            .copied()
            .filter(|v| v.frame >= start && v.frame < start + 256)
            .collect();
        let prepared = session.process(&block, start + 256).unwrap();
        let actual = renderer
            .render_with_sustain_routes(
                &prepared.commands,
                None,
                &prepared.sustain_layers,
                &prepared.host_commands,
                256,
            )
            .unwrap();
        for (offset, frame) in actual.iter().enumerate() {
            for (channel, value) in frame.iter().enumerate() {
                let index = ((start as usize + offset) * 2 + channel) * 4;
                let native = f32::from_le_bytes(expected[index..index + 4].try_into().unwrap());
                assert!(
                    value.is_finite() && (*value - native).abs() <= 2e-7,
                    "{case} frame {} channel {channel}: Rust={value}, retained-native={native}",
                    start + offset as u64
                );
            }
        }
    }
    assert_eq!(renderer.active_voices(), 0, "{case}: complete retirement");
}

#[test]
fn terminal_sustain_route_rejects_invalid_scope_before_pcm_or_clock_changes() {
    let xml = std::fs::read_to_string(format!("{ROOT}/pedal-channel0.xml")).unwrap();
    let program = super::super::program::parse_program(&xml).unwrap();
    let samples = [440, 880]
        .into_iter()
        .map(|hz| {
            let name = format!("tone-{hz}.wav");
            let bytes = std::fs::read(format!("{ROOT}/{name}")).unwrap();
            (
                name,
                Arc::new(super::super::sample::decode(&bytes).unwrap()),
            )
        })
        .collect();
    let mut renderer = Renderer::new(&program, samples, 48_000).unwrap();
    let cc = |controller| script::Command {
        frame: 0,
        action: script::Action::Controller {
            channel: 0,
            controller,
            value: 127,
        },
    };
    for (command, routes) in [
        (cc(64), vec![]),
        (cc(64), vec![Some(program.root)]),
        (cc(63), vec![Some(program.layers[0])]),
    ] {
        assert!(
            renderer
                .render_with_sustain_routes(&[command], None, &routes, &[], 256)
                .is_err()
        );
        assert_eq!(renderer.current_frame(), 0);
        assert_eq!(renderer.active_voices(), 0);
        assert!(!renderer.native_sustain);
        assert!(renderer.layer_sustain.iter().all(|down| !down));
    }
}

#[test]
fn terminal_sustain_scopes_preserve_init_wait_post_order_and_future_drains() {
    let program = super::super::program::parse_program(r#"<Program><EventProcessors><ScriptProcessor><script>
        function onInit()controlChange(64,11);postEvent({type=Event.ControlChange,controller=64,value=22},10)end
        function onController(e)postEvent(e);wait(1);e.value=0;postEvent(e)end
        </script></ScriptProcessor></EventProcessors><Layers><Layer/><Layer/></Layers></Program>"#).unwrap();
    let mut session =
        script::Session::new_program_chain(&program, BTreeMap::new(), None, 48_000).unwrap();
    let check = |out: &script::Processed, expected: &[(u64, u8)]| {
        assert_eq!(out.commands.len(), expected.len() * 2);
        assert_eq!(out.sustain_layers.len(), out.commands.len());
        assert!(out.command_roots.is_empty());
        for (pair, &(frame, value)) in expected.iter().enumerate() {
            for layer in 0..2 {
                let index = pair * 2 + layer;
                assert_eq!(out.commands[index].frame, frame);
                assert!(matches!(out.commands[index].action,
                    script::Action::Controller {controller:64,value:v,..}
                    | script::Action::ControllerAll {controller:64,value:v} if v==value));
                assert_eq!(out.sustain_layers[index], Some(program.layers[layer]));
            }
        }
    };
    check(&session.drain().unwrap(), &[(0, 11)]);
    session
        .input(script::Input {
            frame: 1,
            kind: script::InputKind::Controller {
                channel: 0,
                controller: 64,
                value: 127,
            },
        })
        .unwrap();
    check(&session.drain().unwrap(), &[(1, 127)]);
    session.advance(49).unwrap();
    check(&session.drain().unwrap(), &[(49, 0)]);
    session.advance(479).unwrap();
    check(&session.drain().unwrap(), &[]);
    session.advance(480).unwrap();
    check(&session.drain().unwrap(), &[(480, 22)]);
}

#[test]
fn terminal_sustain_scope_and_root_stay_aligned_when_choke_cancels_waited_posts() {
    let program = super::super::program::parse_program(
        r#"<Program><EventProcessors><ScriptProcessor><script>
        function onNote(e)postEvent(e);wait(1);controlChange(64,e.note)end
        </script></ScriptProcessor></EventProcessors><Layers><Layer/><Layer/></Layers></Program>"#,
    )
    .unwrap();
    let mut session = script::Session::new_hosted_program_chain_with_state(
        &program,
        BTreeMap::new(),
        None,
        48_000,
        1,
        1,
        None,
    )
    .unwrap();
    let root = |token| script::HostRoot {
        epoch: 1,
        generation: 1,
        token,
    };
    for (token, note) in [(1, 60), (2, 72)] {
        session
            .host_note_on(
                root(token),
                script::Input {
                    frame: 0,
                    kind: script::InputKind::NoteOn {
                        channel: 0,
                        note,
                        velocity: 100,
                    },
                },
            )
            .unwrap();
    }
    session.host_note_choke(root(1), 24).unwrap();
    session.advance(48).unwrap();
    let out = session.drain().unwrap();
    assert_eq!(out.commands.len(), 7);
    assert_eq!(out.command_roots.len(), out.commands.len());
    assert_eq!(out.sustain_layers.len(), out.commands.len());
    for (index, command) in out.commands.iter().enumerate() {
        match command.action {
            script::Action::ControllerAll {
                controller: 64,
                value: 72,
            } => {
                assert_eq!(command.frame, 48);
                assert_eq!(out.command_roots[index], Some(root(2)));
                assert_eq!(out.sustain_layers[index], Some(program.layers[index - 5]));
            }
            script::Action::Start(_) | script::Action::ChokeRoot => {
                assert_eq!(out.sustain_layers[index], None)
            }
            _ => panic!("canceled owner emitted a control or altered command order"),
        }
    }
    assert!(matches!(out.commands[4].action, script::Action::ChokeRoot));
}
