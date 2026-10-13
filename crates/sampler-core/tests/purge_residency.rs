use sampler_core::*;
mod support;

fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn region(sample: usize) -> Region {
    Region {
        sample,
        key_low: 60,
        key_high: 60,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback {
            loop_range: Some(Loop {
                start: 0,
                end: 128,
                mode: LoopMode::Continuous,
                shape: LoopShape::Wrap,
                passes: None,
            }),
            ..Default::default()
        },
    }
}
fn program(mode: i64) -> Program {
    Program::new(vec![
        Instruction::SetLocal { local: 0, value: 0 },
        Instruction::SetLocal {
            local: 1,
            value: mode,
        },
        Instruction::Op(Op::Purge {
            group: 0,
            local: 1,
            write: true,
        }),
        Instruction::End,
    ])
    .unwrap()
    .with_script_instance(ScriptInstanceId(0))
}
fn runtime(pcm: Pcm, shared: bool) -> Runtime {
    let regions = if shared {
        vec![region(0), region(0)]
    } else {
        vec![region(0)]
    };
    let members = if shared {
        vec![Some(0), Some(1)]
    } else {
        vec![Some(0)]
    };
    let plan = Prepared::new(48000, vec![pcm], regions, 2)
        .unwrap()
        .with_groups(if shared { 2 } else { 1 }, members)
        .unwrap()
        .with_script_instances(vec![vec![]])
        .unwrap()
        .with_programs(vec![program(0), program(1)], None)
        .unwrap();
    let limits = Limits {
        notes: 8,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 8,
        voices: 32,
        decisions: 8,
        commands: 16,
        behaviors: 8,
        behavior_fuel: 1024,
        behavior_cells: 32,
        note_cells: 0,
    };
    Runtime::new(plan, limits).unwrap()
}
#[test]
fn purge_retires_voices_and_refuses_new_group_starts_without_script_gain_work() {
    let pcm = Pcm::new(48000, vec![[0.25; 2]; 128].into()).unwrap();
    let mut rt = runtime(pcm, false);
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 1);
    let plan = rt.active_plan();
    support::without_heap(|| {
        rt.start_plan_behavior(plan, 0).unwrap();
    });
    assert_eq!(
        rt.voice_count(),
        0,
        "purge must retire source/FX work, not attenuate it"
    );
    let note = rt.note_on(input(2), 60, 1.).unwrap();
    rt.forward_attack(note).unwrap();
    assert_eq!(rt.voice_count(), 0);
}
