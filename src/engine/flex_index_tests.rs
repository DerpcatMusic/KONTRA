use super::{Ahdsr, Envelope, Flex, FlexPoint};
use crate::{
    audio::Sample,
    engine::{Bank, Engine, Expression, HostNote, HostPattern},
    import::{FlexEnvelope, FlexPoint as ImportedPoint, Group, Zone},
};

fn point(index: usize) -> f32 {
    match index {
        256 => 0.25,
        257 => 0.75,
        258 => 0.5,
        259 => 0.,
        _ => (1 + index % 7) as f32 / 8.,
    }
}

fn create(group: &Group, mode: usize, shared: bool) -> Engine {
    let bank = Bank::from_samples(
        vec![group.clone()], vec![Zone::default()],
        vec![(Default::default(), Sample { rate: 48_000, frames: (0..4096)
            .map(|i| [0.2 + (i % 37) as f32 / 1024., -0.15 - (i % 53) as f32 / 2048.])
            .collect() })],
    ).unwrap().with_prepared_settings(|rows| {
        rows[0].envelope = Some(Ahdsr::UNITY);
        if let Some(flex) = &mut rows[0].flex {
            assert_eq!(flex.points.len(), 260);
            assert_eq!(flex.sustain, 257);
            flex.loop_start = (mode == 1).then_some(255);
            flex.one_shot = mode == 2;
        }
    }).unwrap();
    let mut engine = Engine::default();
    engine.reset(48_000.);
    engine.player.shared = shared;
    engine.set_bank(Some(Box::new(bank)));
    engine
}

// Independent two-frame linear segments; no production envelope state or
// narrowed index is used to produce the amplitude reference.
fn held(age: usize, mode: usize) -> f32 {
    if age < 516 || mode == 2 && age < 520 {
        let index = age / 2;
        let from = if index == 0 { 0. } else { point(index - 1) };
        from + (point(index) - from) * ((age % 2 + 1) as f32 / 2.)
    } else if mode == 1 {
        [0.5, 0.25, 0.5, 0.75][(age - 516) % 4]
    } else if mode == 2 {
        0.
    } else {
        0.75
    }
}

fn level(age: usize, off: usize, mode: usize) -> f32 {
    if age < off || mode == 2 {
        held(age, mode)
    } else {
        match age - off {
            0 => (held(off - 1, mode) + 0.5) / 2.,
            1 => 0.5,
            2 => 0.25,
            _ => 0.,
        }
    }
}

#[test]
fn compatibility_flex_owns_full_vector_indices_and_complete_pcm_without_heap() {
    const FRAMES: usize = 1024;
    // Compatibility authored vectors are independent of the native editor's
    // documented32-point limit. Preparation must retain their legal indices.
    let group = Group { flex_env: Some(FlexEnvelope {
        points: (0..260).map(|i| ImportedPoint {
            time_ms: 1000. * 2. / 48_000., level: point(i), curve: 0.5,
        }).collect(), sustain: 257, unknown_index: 255, unknown_tail: vec![],
    }), ..Default::default() };
    let cached: Group = serde_json::from_value(serde_json::to_value(&group).unwrap()).unwrap();
    assert_eq!(cached.flex_env, group.flex_env);
    let first = HostNote { port: 0, channel: 0, key: 60, id: 181, clap: true };
    let second = HostNote { channel: 1, id: 182, ..first };
    let pattern = |note: HostNote| HostPattern {
        port: i32::from(note.port), channel: i32::from(note.channel), key: i32::from(note.key), id: note.id, clap: note.clap,
    };
    for mode in 0..3 { for shared in [false, true] { for mute in [false, true] {
        let mut engines = [create(&cached, mode, shared), create(&cached, mode, shared)];
        let mut dry = [create(&Group::default(), 0, false), create(&Group::default(), 0, false)];
        let mut pcm = [[[0.; FRAMES]; 2]; 2];
        let mut reference = [[[0.; FRAMES]; 2]; 2];
        let audio_ends = if mode == 2 { [520, 837] } else { [717, 354] };
        let host_ends = if mode == 2 { [600, 837] } else { audio_ends };
        assert_eq!(crate::plugin::tests::allocations(|| {
            for engine in &mut engines {
                assert!(engine.host_note_on(first, 0, 60, 127, 0.));
                engine.cc(0, 64, 127);
            }
            dry[0].note_on(0, 60, 127);
            let mut start = 0;
            for end in [103, 317, 343, 350, 354, 355, 512, 516, 520, 521, 528, 600,
                713, 717, 718, 837, 838, FRAMES] {
                for (index, engine) in engines.iter_mut().enumerate() {
                    let (mut at, mut part) = (start, 0);
                    while at < end {
                        let n = if index == 1 { [1, 7, 31, 127, 13, 64][part % 6] }
                            else { 128 }.min(end - at);
                        let [l, r] = &mut pcm[index];
                        engine.render_checked(&mut l[at..at+n], &mut r[at..at+n]).unwrap();
                        engine.render_checked(&mut [], &mut []).unwrap();
                        at += n; part += 1;
                    }
                    assert_eq!(engine.active_voices(), usize::from(end < audio_ends[0])
                        + usize::from(end > 317 && end < audio_ends[1]), "mode={mode} boundary={end}");
                    if mode == 2 && (520..=600).contains(&end) {
                        engine.mark_host_notes();
                        assert!(engine.host_key_held(0, 60));
                        assert_eq!(engine.host_note_end_at(first), None,
                            "completed audio retains its physically held Host owner");
                    }
                }
                for (index, engine) in dry.iter_mut().enumerate() {
                    let [l, r] = &mut reference[index];
                    engine.render_checked(&mut l[start..end], &mut r[start..end]).unwrap();
                }
                if mute && matches!(end, 103 | 528) {
                    for engine in engines.iter_mut().chain(dry.iter_mut()) { for channel in 0..2 {
                        engine.set_expression_on(channel, 60, Expression {
                            gain: if end == 103 { 0. } else { 1. }, ..Default::default()
                        });
                    }}
                }
                if end == 317 {
                    for engine in &mut engines { assert!(engine.host_note_on(second, 1, 60, 127, 0.)); }
                    dry[1].note_on(1, 60, 127);
                }
                if end == 350 { for engine in &mut engines { engine.host_note_off(pattern(second)); } }
                if end == 600 { for engine in &mut engines {
                    engine.host_note_off(pattern(first)); engine.host_note_off(pattern(first));
                    assert!(!engine.host_key_held(0, 60));
                    if mode != 2 { assert!(engine.player.voices.iter().any(|v| !v.held && !v.released)); }
                    else {
                        engine.mark_host_notes();
                        assert_eq!(engine.host_note_end_at(first), Some(600),
                            "physical Off releases the held pin after audio completed at520");
                    }
                }}
                if end == 713 { for engine in &mut engines { engine.cc(0, 64, 0); } }
                start = end;
            }
            for engine in &mut engines {
                engine.mark_host_notes();
                assert_eq!(engine.host_note_end_at(first), Some(host_ends[0] as u64));
                assert_eq!(engine.host_note_end_at(second), Some(host_ends[1] as u64));
            }
        }), 0);
        for index in 0..2 { for channel in 0..2 { for frame in 0..FRAMES {
            let expected = reference[0][channel][frame] * level(frame, 713, mode)
                + if frame >= 317 { reference[1][channel][frame] * level(frame - 317, 33, mode) } else { 0. };
            assert!((pcm[index][channel][frame] - expected).abs() < 2e-6,
                "full vector PCM mode={mode} shared={shared} mute={mute} partition={index} frame={frame}");
        }}}
        if mute { assert!(pcm.iter().all(|voice| voice.iter().all(|channel|
            channel[343..520].iter().all(|&x| x == 0.)))); }
        assert!(pcm[0].iter().flatten().any(|x| x.abs() > 0.01), "oracle must be audible");
        assert!(pcm[0].iter().all(|channel| channel[838..].iter().all(|&x| x == 0.)));
    }}}
}

#[test]
fn malformed_flex_successors_finish_without_index_arithmetic_overflow() {
    let mut flex = Flex { points: vec![FlexPoint { seconds: 1. / 48_000., level: 0., curve: 0. }]
        .into_boxed_slice(), sustain: usize::MAX, one_shot: false, loop_start: None };
    let mut state = Envelope::flex();
    state.release(Some(&flex));
    assert_eq!(state.render_terminal(&mut [0.; 2], Some(&flex), 48_000.), Some(0));
    assert!(state.done());
    flex.sustain = 0;
    flex.loop_start = Some(usize::MAX);
    let mut state = Envelope::flex();
    assert_eq!(state.render_terminal(&mut [0.; 2], Some(&flex), 48_000.), Some(1));
    assert!(state.done());
    flex.points = Box::default();
    let mut state = Envelope::flex();
    assert_eq!(state.render_terminal(&mut [0.; 2], Some(&flex), 48_000.), Some(0));
    assert!(state.done());
    flex.one_shot = true;
    let mut state = Envelope::flex();
    state.release(Some(&flex));
    assert_eq!(state.render_terminal(&mut [0.; 2], Some(&flex), 48_000.), Some(0));
    assert!(state.done());
}
