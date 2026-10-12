//! The callback and DSP must read the same accepted physical tuning owner.
use crate::{
    audio::Sample,
    engine::{envelope::Ahdsr, Bank, Engine, EventChange, EventId, HostExpression, HostNote, HostPattern, NoteEvent},
    import::{Group, Zone},
    ksp::{LogEngine, Runtime},
    midi::{NativeParameter, NativeParameterData},
};
use std::path::PathBuf;

fn engine(scripted: bool) -> Box<Engine> {
    let path = PathBuf::from("mixed tuning callback");
    let sample = Sample { rate: 48_000, frames: (0..16_384).map(|i| {
        let phase = std::f32::consts::TAU * (i % 97) as f32 / 97.;
        [0.11 + 0.05 * phase.sin(), 0.08 + 0.03 * phase.cos()]
    }).collect() };
    let bank = Bank::from_samples(vec![Group::default()], vec![Zone {
        root: 60, low_key: 60, high_key: 61, sample: path.clone(), ..Default::default()
    }], vec![(path, sample)]).unwrap().with_prepared_settings(|groups| {
        groups[0].envelope = Some(Ahdsr { release: 0.003, ..Ahdsr::UNITY });
    }).unwrap();
    let mut e = Box::new(Engine::default());
    e.reset(48_000.); e.attack = 0.; e.set_lanes(false); e.set_bank(Some(Box::new(bank)));
    if scripted {
        let source = "on init\ndeclare %id[128]\nend on\non note\n%id[$EVENT_NOTE] := $EVENT_ID\nend on\non rpn\nmessage($RPN_ADDRESS & \":\" & $RPN_VALUE)\nif ($RPN_ADDRESS=1 and $RPN_VALUE=10265)\nwait(1000)\nchange_vol(%id[60],-12000,0)\nend if\nif ($RPN_ADDRESS=1 and $RPN_VALUE=10113)\nwait(1000)\nchange_vol(%id[60],-6000,0)\nend if\nif ($RPN_ADDRESS=2 and $RPN_VALUE=8577)\nwait(1000)\nchange_vol(%id[60],-18000,0)\nend if\nif ($RPN_ADDRESS=2 and $RPN_VALUE=8706)\nwait(1000)\nchange_vol(%id[60],-9000,0)\nend if\nend on";
        let (rt, errors) = Runtime::with_scripts(&[source], &mut LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        e.set_script(Some(Box::new(rt)));
    }
    e
}
fn host(channel: u8, key: u8) -> HostNote { HostNote { port: 3, channel, key, id: 3401, clap: true } }
fn pattern(n: HostNote) -> HostPattern { HostPattern {
    port: i32::from(n.port), channel: i32::from(n.channel), key: i32::from(n.key), id: n.id, clap: n.clap,
} }
fn cc(e: &mut Engine, input: u8, n: u8, value: u8) { e.cc_from(7, input, n, value); }
fn select(e: &mut Engine, input: u8, address: u8) { cc(e, input, 101, 0); cc(e, input, 100, address); }
fn native(e: &mut Engine, input: u8, address: u16, data: NativeParameterData) {
    assert!(e.native_parameter_from(7, input, NativeParameter { registered: true, address, data }));
}
fn message(e: &Engine, expected: &str) { assert_eq!(e.script().unwrap().last_message(), expected); }
fn start(e: &mut Engine, n: HostNote, pan: f32) {
    assert!(e.host_note_on(n, 7, n.key, 127, 0.));
    e.host_expression(pattern(n), HostExpression::Pan(pan));
}
fn reference(e: &mut Engine, key: u8, pan: f32, tune: f64) -> EventId {
    let mut note = NoteEvent::new(7, key, 127); note.pan = pan; note.tune = tune;
    e.start_event(&note).unwrap()
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>; 2], from: usize, to: usize, partition: &[usize]) {
    let (mut at, mut block) = (from, 0);
    while at < to {
        let n = partition[block % partition.len()].min(to - at);
        let [left, right] = pcm;
        e.render_checked(&mut left[at..at+n], &mut right[at..at+n]).unwrap();
        e.render_checked(&mut [], &mut []).unwrap(); at += n; block += 1;
    }
    e.mark_host_notes();
}
fn same(a: &[Vec<f32>; 2], b: &[Vec<f32>; 2]) {
    for channel in 0..2 { for (at, (&a, &b)) in a[channel].iter().zip(&b[channel]).enumerate() {
        assert!((a-b).abs() < 4e-6, "channel{channel} frame{at}: {a} != {b}");
    } }
}
fn timeline(partition: &[usize]) -> [Vec<f32>; 2] {
    use NativeParameterData::{Absolute, Relative};
    const FRAMES: usize = 5120;
    let (mut actual, mut expected) = (engine(true), engine(false));
    let (mut pcm, mut oracle) = (std::array::from_fn(|_| vec![0.; FRAMES]), std::array::from_fn(|_| vec![0.; FRAMES]));
    let (left, right) = (host(4, 60), host(7, 61));
    assert_eq!(crate::plugin::tests::allocations(|| {
        for input in [4, 7] { select(&mut actual, input, 1); cc(&mut actual, input, 6, 64); }
        start(&mut actual, left, -1.); start(&mut actual, right, 1.);
        let mut l = reference(&mut expected, 60, -1., 0.);
        let r = reference(&mut expected, 61, 1., 0.);
        let mut from = 0;
        for to in [512, 560, 768, 1024, 1072, 1536, 1584, 2048, 2096, 2560, 2688, 2816, 3072, 3200, 3584, 3777, 3936, 4096, 4352, 4400, 4608, 4801, FRAMES] {
            render(&mut actual, &mut pcm, from, to, partition);
            render(&mut expected, &mut oracle, from, to, &[128]); from = to;
            match to {
                512 => {
                    native(&mut actual, 4, 1, Absolute(0xa0012345)); cc(&mut actual, 4, 38, 25);
                    message(&actual, "1:10265");
                    expected.change_event(l, EventChange::Tune(0.2530517578125));
                },
                560 | 4400 => expected.change_event(l, EventChange::Volume(0.25118864)),
                768 => {
                    native(&mut actual, 7, 1, Absolute(60 << 25)); cc(&mut actual, 7, 38, 8);
                    message(&actual, "1:7688"); expected.change_event(r, EventChange::Tune(-0.0615234375));
                },
                1024 => {
                    native(&mut actual, 4, 1, Relative(-(153 << 18))); cc(&mut actual, 4, 38, 1);
                    message(&actual, "1:10113"); expected.change_event(l, EventChange::Tune(0.2344970703125));
                },
                1072 => expected.change_event(l, EventChange::Volume(0.5011872)),
                1536 => {
                    select(&mut actual, 4, 2); cc(&mut actual, 4, 6, 64);
                    native(&mut actual, 4, 2, Absolute((67 << 25) | 0x12345)); cc(&mut actual, 4, 38, 1);
                    message(&actual, "2:8577"); expected.change_event(l, EventChange::Tune(3.2344970703125));
                },
                1584 => expected.change_event(l, EventChange::Volume(0.12589254)),
                2048 => {
                    native(&mut actual, 4, 2, Relative(1 << 25)); cc(&mut actual, 4, 38, 2);
                    message(&actual, "2:8706"); expected.change_event(l, EventChange::Tune(4.2344970703125));
                },
                2096 => expected.change_event(l, EventChange::Volume(0.3548134)),
                2560 => {
                    cc(&mut actual, 4, 101, 127); cc(&mut actual, 4, 100, 127);
                    native(&mut actual, 4, 2, Absolute(66 << 25)); cc(&mut actual, 4, 38, 2);
                    message(&actual, "2:8448"); expected.change_event(l, EventChange::Tune(2.2344970703125));
                },
                2688 => {
                    select(&mut actual, 4, 1); native(&mut actual, 4, 2, Absolute(69 << 25));
                    cc(&mut actual, 4, 38, 3); // Other RPN cannot seed this unknown decoder baseline.
                    message(&actual, "2:8832"); expected.change_event(l, EventChange::Tune(5.2347412109375));
                },
                2816 => {
                    cc(&mut actual, 4, 99, 0); cc(&mut actual, 4, 98, 9); cc(&mut actual, 4, 6, 3);
                    native(&mut actual, 4, 1, Absolute(0xa4012345)); cc(&mut actual, 4, 38, 7);
                    message(&actual, "1:10496");
                    expected.change_event(l, EventChange::Tune(5.281284722033888));
                },
                3072 => {
                    select(&mut actual, 4, 1); cc(&mut actual, 4, 38, 9);
                    message(&actual, "1:10496"); expected.change_event(l, EventChange::Tune(5.2823486328125));
                },
                3200 => {
                    cc(&mut actual, 4, 6, 64); cc(&mut actual, 4, 38, 5);
                    message(&actual, "1:8197"); expected.change_event(l, EventChange::Tune(5.0006103515625));
                },
                3584 => { actual.host_note_off(pattern(left)); actual.host_note_off(pattern(right)); expected.release_event(l); expected.release_event(r); },
                3777 => {
                    // Authored .003s48k release crosses the ordinary -80dB cutoff after193 frames.
                    for n in [left, right] { assert_eq!(actual.host_note_end_at(n), Some(3777)); actual.retire_host_note(n); }
                    assert!(!expected.event_active(l) && !expected.event_active(r));
                },
                3936 => { actual.host_note_off(pattern(left)); actual.host_expression(pattern(left), HostExpression::Gain(0.)); },
                4096 => { start(&mut actual, left, -1.); l = reference(&mut expected, 60, -1., 5.0006103515625); },
                4352 => {
                    native(&mut actual, 4, 1, Absolute(80 << 25)); cc(&mut actual, 4, 38, 25);
                    message(&actual, "1:10265"); expected.change_event(l, EventChange::Tune(5.2530517578125));
                },
                4608 => { actual.host_note_off(pattern(left)); expected.release_event(l); },
                4801 => { assert_eq!(actual.host_note_end_at(left), Some(4801)); assert!(!expected.event_active(l)); },
                _ => {},
            }
        }
        assert_eq!(actual.active_voices(), 0); assert_eq!(actual.dropped_commands(), 0);
        assert_eq!(actual.underruns(), 0); assert_eq!(actual.unsupported_native_controls(), 0);
        let rt = actual.script().unwrap(); assert!(rt.diagnostics().is_empty()); assert!(rt.faults().next().is_none());
    }), 0);
    same(&pcm, &oracle);
    assert!(pcm[0][600].abs() > 0.002 && pcm[1][1100].abs() > 0.01);
    assert!(pcm.iter().all(|channel| channel[4801..].iter().all(|&v| v == 0.)));
    pcm
}

#[test]
fn native_tuning_replacements_and_relative_edits_reseed_only_selected_midi1_callbacks_in_complete_pcm() {
    same(&timeline(&[128]), &timeline(&[1, 7, 31, 127]));
}
