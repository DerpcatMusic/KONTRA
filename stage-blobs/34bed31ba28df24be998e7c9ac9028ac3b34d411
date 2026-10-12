//! Notes a script plays start when its callback waits or ends, so it can pick
//! their groups right after `play_note` (Audio Imperia's legato releases do).
use sampler_core::{Envelope, Input, Limits, Pcm, Playback, Prepared, Protocol, Region, Runtime};

fn region(sample: usize, key: u8) -> Region {
    Region {
        sample,
        key_low: key,
        key_high: key,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}

/// Peak of one block after playing key 60 into a script that plays key 61,
/// whose group 0 sample is 1.0 and group 1 sample is 0.25.
fn peak(body: &str) -> f32 {
    let environment = sampler_ksp::Environment {
        groups: vec!["loud".into(), "quiet".into()],
        ..Default::default()
    };
    let script = sampler_ksp::compile_with(
        &format!("on init declare $id end on\non note\n ignore_event($EVENT_ID)\n{body}\nend on"),
        48000,
        sampler_ksp::Limits {
            source_bytes: 65536,
            instructions: 4096,
            variables: 16,
            array_cells: 16,
        },
        &[],
        &environment,
    )
    .unwrap();
    let pcm = vec![
        Pcm::new(48000, Box::from([[1.0; 2]; 4800])).unwrap(),
        Pcm::new(48000, Box::from([[0.25; 2]; 4800])).unwrap(),
    ];
    let prepared = Prepared::new(48000, pcm, vec![region(0, 61), region(1, 61)], 128)
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .unwrap();
    let note_cells = script.note_cells() * 8;
    let plan = script.bind(prepared).unwrap();
    let behavior_cells = plan.behavior_local_count() * 8;
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 8,
            voices: 8,
            expressions: 8,
            decisions: 0,
            commands: 16,
            behaviors: 8,
            behavior_fuel: 4096,
            behavior_cells,
            note_cells,
        },
    )
    .unwrap();
    let input = Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(60),
    };
    rt.trigger(input, 60, 1.).unwrap();
    let mut block = [[0.; 2]; 64];
    rt.render(&mut block).unwrap();
    block.iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()))
}

#[test]
fn allow_group_after_play_note_picks_the_started_groups() {
    // Without edits both groups sound.
    let all = peak("$id := play_note(61, 100, 0, 0)");
    assert!(all > 0.9, "{all}");
    // Only the quiet group: the edit lands before the note starts.
    let quiet = peak(
        "$id := play_note(61, 100, 0, 0)
         set_event_par_arr($id, $EVENT_PAR_ALLOW_GROUP, 0, $ALL_GROUPS)
         set_event_par_arr($id, $EVENT_PAR_ALLOW_GROUP, 1, 1)",
    );
    // 0.25 of the 1.25 sum.
    assert!((quiet / all - 0.2).abs() < 0.02, "{quiet} of {all}");
    // Editing after a wait is too late: the note already started.
    let late = peak(
        "$id := play_note(61, 100, 0, 0)
         wait(100)
         set_event_par_arr($id, $EVENT_PAR_ALLOW_GROUP, 0, $ALL_GROUPS)",
    );
    assert!((late - all).abs() < 0.02, "{late} vs {all}");
}
