use super::*;
use crate::{
    audio::Sample,
    engine::{Bank, HostNote, HostPattern},
    import::{Group, ModAssignment, ModSource, ModTarget, Zone as SampleZone},
    midi::Velocity,
};

const FRAMES: usize = 4096;

fn engine(scripted: bool) -> Engine {
    let group = Group {
        mods: vec![ModAssignment {
            cc_default: None,
            name: "note brightness volume".into(),
            source: ModSource::MidiCc(74),
            target: ModTarget::Volume,
            intensity: 0.5,
            invert: false,
            lag_ms: 0,
            shaper: None,
        }],
        ..Default::default()
    };
    let frames = (0..16384).map(|frame| {
        let phase = frame as f32 * std::f32::consts::TAU / 64.;
        [0.1 + 0.08 * phase.sin(), -0.04 + 0.06 * (phase * 0.5).cos()]
    }).collect();
    let bank = Bank::from_samples(vec![group], vec![SampleZone::default()],
        vec![(Default::default(), Sample { rate: 48000, frames })]).unwrap();
    let mut engine = Engine::default();
    engine.reset(48000.);
    engine.attack = 0.;
    engine.release = 0.002;
    engine.set_bank(Some(Box::new(bank)));
    if scripted {
        let source = "on note\nif (($EVENT_NOTE=64) or ($EVENT_NOTE=68))\nwait(5000)\nend if\nend on\non release\nend on";
        let (runtime, errors) = crate::ksp::Runtime::with_scripts(&[source],
            &mut crate::ksp::LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        engine.set_script(Some(Box::new(runtime)));
    }
    engine
}

fn router() -> Router {
    let mut router = Router::default();
    router.route.mode = Mode::Channel;
    router
}

fn host(channel: u8, key: u8, id: i32) -> HostNote {
    HostNote { port: 0, channel, key, id, clap: true }
}

fn pattern(note: HostNote) -> HostPattern {
    HostPattern { port: i32::from(note.port), channel: i32::from(note.channel),
        key: i32::from(note.key), id: note.id, clap: note.clap }
}

fn expression(router: &mut Router, engine: &mut Engine, home: u8,
    input: u8, key: u8, gain: f32, tune: f32, pan: f32, brightness: u8)
{
    for event in [In::NoteGain(input, key, gain), In::NoteTune(input, key, tune),
        In::NotePan(input, key, pan), In::NoteBrightness(input, key,
            crate::midi::ControlValue::from_midi1(brightness))]
    {
        feed(router, engine, event, home);
    }
}

fn render(engine: &mut Engine, pcm: &mut [[f32; FRAMES]; 2],
    start: usize, end: usize, sizes: &[usize])
{
    let (mut at, mut fragment) = (start, 0);
    while at < end {
        let frames = sizes[fragment % sizes.len()].min(end - at);
        let [left, right] = &mut *pcm;
        engine.render_checked(&mut left[at..at + frames], &mut right[at..at + frames]).unwrap();
        engine.mark_host_notes();
        at += frames;
        fragment += 1;
    }
}

fn performance(scripted: bool, sizes: &[usize]) -> ([[f32; FRAMES]; 2], [u64; 3]) {
    let (mut actual, mut reference) = (engine(scripted), engine(scripted));
    let (mut a_route, mut r_route) = (router(), router());
    let (mut pcm, mut oracle) = ([[0.; FRAMES]; 2], [[0.; FRAMES]; 2]);
    let survivor = host(7, 62, 701);
    let reassigned = host(1, 67, 702);
    let canceled_wait = host(2, 68, 703);
    // The oracle delivers exactly the same physical performance to logical11,
    // outside the negotiated physical mask. Logical channel numbers must not
    // change this instrument's PCM or exact-owner lifetimes.
    assert_eq!(crate::plugin::tests::allocations(|| {
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            feed(route, engine, In::NoteOn(6, 60, 127), home);
            expression(route, engine, home, 6, 60, 0.4, 3., -0.75, 64);
            feed(route, engine, In::HostOn(survivor, Velocity::from_midi1(127), 0.), home);
            expression(route, engine, home, 7, 62, 0.7, -2., 0.5, 100);
            feed(route, engine, In::HostOn(reassigned, Velocity::from_midi1(100), 0.), home);
            // The direct Engine API retains explicitly authored destination
            // expression until changed. It does not perform Router's On reset.
            engine.set_expression_on(home, 66, Expression { gain: 0.3, tune: 2., pan: -0.4,
                note_cc74: Some(crate::midi::ControlValue::from_midi1(96)), ..Default::default() });
        }
        render(&mut actual, &mut pcm, 0, 192, sizes);
        render(&mut reference, &mut oracle, 0, 192, sizes);
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            // A physical8 root survives while its callback waits; the source
            // starts after reconfiguration and must read its retained route.
            feed(route, engine, In::NoteOn(8, 64, 127), home);
            expression(route, engine, home, 8, 64, 0.55, 7., -0.25, 80);
            feed(route, engine, In::HostOn(canceled_wait, Velocity::from_midi1(127), 0.), home);
            for (cc, value) in [(101, 0), (100, 6), (6, 4)] {
                feed(route, engine, In::Cc(0, cc, value), home);
            }
            assert_eq!(engine.mpe_zones().member(4).unwrap().manager, 0);
            assert!(engine.mpe_zones().zone(6).is_none());
            assert!(!engine.host_key_held(1, 67), "changed physical owner must be canceled");
            assert!(!engine.host_key_held(2, 68), "changed waiting owner must be canceled");
            assert!(engine.host_key_held(7, 62));
            assert!(engine.key_down(home, 60) && engine.key_down(home, 64));
            // Retarget just gain/pan after the reset. A wrongly cleared Router
            // row would also erase the existing tune and note brightness.
            feed(route, engine, In::NoteGain(6, 60, 0.625), home);
            feed(route, engine, In::NotePan(7, 62, 0.75), home);
        }
        render(&mut actual, &mut pcm, 192, 768, sizes);
        render(&mut reference, &mut oracle, 192, 768, sizes);
        assert!(!actual.host_note_pending(reassigned));
        assert!(!reference.host_note_pending(reassigned));
        assert!(!actual.host_note_pending(canceled_wait));
        assert!(!reference.host_note_pending(canceled_wait));
        assert_eq!(actual.host_note_end_at(reassigned), reference.host_note_end_at(reassigned));

        // Only the actual destination has stale, authored pre-note expression.
        // An ordinary new note must replace it with the same default baseline
        // used by the untouched oracle destination.
        expression(&mut a_route, &mut actual, 4, 9, 65, 0.2, -9., -1., 127);
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            feed(route, engine, In::NoteOn(9, 65, 127), home);
            assert_eq!(route.expression[home as usize][65], Expression::default());
            assert!(engine.note_on_input_velocity_pitch(crate::engine::InputAddress {
                port: 0, channel: 10, key: 66 }, home, 66, Velocity::from_midi1(127), None));
        }
        render(&mut actual, &mut pcm, 768, 1024, sizes);
        render(&mut reference, &mut oracle, 768, 1024, sizes);
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            feed(route, engine, In::NoteGain(6, 60, 0.), home);
        }
        render(&mut actual, &mut pcm, 1024, 1536, sizes);
        render(&mut reference, &mut oracle, 1024, 1536, sizes);
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            feed(route, engine, In::NoteGain(6, 60, 0.625), home);
        }
        render(&mut actual, &mut pcm, 1536, 2048, sizes);
        render(&mut reference, &mut oracle, 1536, 2048, sizes);
        for (engine, route, home) in [(&mut actual, &mut a_route, 4),
            (&mut reference, &mut r_route, 11)]
        {
            for event in [In::NoteOff(6, 60), In::HostOff(pattern(survivor)),
                In::NoteOff(8, 64), In::NoteOff(9, 65), In::NoteOff(10, 66)]
            {
                feed(route, engine, event, home);
            }
        }
        render(&mut actual, &mut pcm, 2048, FRAMES, sizes);
        render(&mut reference, &mut oracle, 2048, FRAMES, sizes);
        assert_eq!(actual.active_voices(), 0);
        assert_eq!(reference.active_voices(), 0);
        assert_eq!(actual.dropped_commands(), 0);
        assert_eq!(reference.dropped_commands(), 0);
    }), 0);
    assert!(oracle.iter().flatten().any(|value| value.abs() > 0.02));
    let error = pcm.iter().flatten().zip(oracle.iter().flatten())
        .map(|(a, b)| (a - b).abs()).fold(0_f32, f32::max);
    assert!(error < 2e-6, "scripted={scripted}: physical MPE reset changed unrelated logical PCM by {error}");
    assert!(pcm.iter().all(|row| row[3072..].iter().all(|sample| *sample == 0.)));
    let ends = [survivor, reassigned, canceled_wait].map(|note| {
        let end = actual.host_note_end_at(note).expect("exact owner must finish");
        assert_eq!(Some(end), reference.host_note_end_at(note));
        end
    });
    (pcm, ends)
}

#[test]
fn physical_mpe_role_changes_preserve_unrelated_delivered_expression_and_complete_pcm() {
    for scripted in [false, true] {
        let (fixed, fixed_ends) = performance(scripted, &[128]);
        let (fragmented, fragmented_ends) = performance(scripted, &[1, 7, 31, 127]);
        let error = fixed.iter().flatten().zip(fragmented.iter().flatten())
            .map(|(a, b)| (a - b).abs()).fold(0_f32, f32::max);
        assert!(error < 2e-5, "scripted={scripted}: callback partition changed complete PCM by {error}");
        assert_eq!(fixed_ends, fragmented_ends);
    }
}
