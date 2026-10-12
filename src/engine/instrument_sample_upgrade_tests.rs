//! Header changes replace old source cursors; real fills preserve their geometry.

use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostNote, params::{Address, GroupPar}},
    import::{Ahdsr, Group, Zone},
};

const SWITCH: usize = 4101;
const ON: usize = 4105;
const END: usize = 9000;

fn bank(rate: u32, frames: usize) -> Box<Bank> {
    let group = Group {
        volume_env: Some(Ahdsr {
            attack_curve:0., attack_ms:0., hold_ms:0., decay_ms:0.,
            sustain:1., release_ms:5., unknown_flag:0, unknown_tail:Vec::new(),
        }),
        ..Default::default()
    };
    Box::new(Bank::from_samples(vec![group], vec![Zone {
        root:60, low_key:60, high_key:60, ..Default::default()
    }], vec![(Default::default(), Sample {
        rate, frames:(0..frames).map(|at|
            [0.125 + at as f32 / 16384., 0.375 - at as f32 / 32768.]).collect(),
    })]).unwrap())
}

fn performance(rate: u32, frames: usize, fragments: &[usize]) -> [[Vec<f32>; 2]; 2] {
    let old = HostNote { port:0, channel:4, key:60, id:901, clap:true };
    let new = HostNote { id:902, ..old };
    let (mut actual, mut expected) = (Engine::default(), Engine::default());
    for engine in [&mut actual, &mut expected] {
        engine.reset(48000.);
        engine.set_bank(Some(bank(48000,8192)));
    }
    let (mut incoming, mut explicit) = (Some(bank(rate,frames)), Some(bank(rate,frames)));
    assert!(!incoming.as_ref().unwrap().prepared_settings_edited,
        "header geometry must be the reason for replacement");
    let (mut retired, mut expected_retired) = (None, None);
    let mut pcm: [[Vec<f32>;2];2] = std::array::from_fn(|_| std::array::from_fn(|_| vec![0.;END]));
    assert_eq!(crate::plugin::tests::allocations(|| {
        for engine in [&mut actual, &mut expected] {
            assert!(engine.write(Address::Group(0,GroupPar::Volume),0.625));
            assert!(engine.host_note_on(old,4,60,127,0.));
        }
        let (mut at, mut fragment) = (0,0);
        for stop in [SWITCH,ON,END] {
            while at < stop {
                let n = fragments[fragment % fragments.len()].min(stop-at);
                for (engine, output) in [&mut actual, &mut expected].into_iter().zip(&mut pcm) {
                    let [left,right] = output;
                    engine.render_checked(&mut left[at..at+n], &mut right[at..at+n]).unwrap();
                }
                at += n; fragment += 1;
            }
            match stop {
                SWITCH => {
                    assert_eq!(actual.player.voices[0].pos, SWITCH as f64);
                    if frames == 2048 {
                        assert!(actual.player.voices[0].pos > frames as f64,
                            "the old cursor is already beyond the incoming EOF");
                    }
                    actual.cc_from(4,4,1,64); expected.cc_from(4,4,1,64);
                    assert_ne!(actual.player.midi.cc_received[4], [0;2]);
                    retired = actual.upgrade_bank(incoming.take().unwrap());
                    // The independent reference explicitly installs the exact
                    // requested source instead of invoking the upgrade guard.
                    expected_retired = expected.set_bank(explicit.take());
                    for engine in [&mut actual, &mut expected] {
                        engine.mark_host_notes();
                        assert_eq!(engine.active_voices(),0);
                        assert_eq!(engine.host_note_end_at(old),Some(SWITCH as u64));
                        assert_eq!(engine.state.base[0].gain,1.);
                        assert_eq!(engine.player.midi.cc_received, [[0;2];16]);
                        assert_eq!(engine.player.now,SWITCH as u64);
                    }
                },
                ON => for engine in [&mut actual, &mut expected] {
                    assert!(engine.host_note_on(new,4,60,127,0.));
                    assert_eq!(engine.player.voices[0].step, f64::from(rate)/48000.);
                },
                _ => {},
            }
        }
        let source_end = ON as u64 + frames as u64 * 48000 / u64::from(rate);
        for engine in [&mut actual, &mut expected] {
            engine.mark_host_notes();
            assert_eq!(engine.host_note_end_at(old),Some(SWITCH as u64));
            assert_eq!(engine.host_note_end_at(new),Some(source_end));
            assert_eq!(engine.active_voices(),0);
            assert_eq!(engine.underruns(),0);
        }
    }),0);
    let error = pcm[0].iter().flatten().zip(pcm[1].iter().flatten())
        .fold(0_f32, |error,(a,b)| error.max((a-b).abs()));
    assert!(error < 2e-5, "changed header retained an old cursor: error{error}");
    assert!(pcm[0].iter().all(|channel| channel[SWITCH..ON].iter().all(|value| *value == 0.)));
    assert!(pcm[0][0][ON] > 0.01, "the replacement must start a real attack");
    // Unity amplifier and binary-fraction source values expose the fresh
    // source clock independently of the Engine reference above.
    let source_at_64 = 64. * rate as f32 / 48000.;
    let ratio = (0.125 + source_at_64 / 16384.) / 0.125;
    assert!((pcm[0][0][ON+64] / pcm[0][0][ON] - ratio).abs() < 2e-6);
    let source_end = ON + frames * 48000 / rate as usize;
    assert!(pcm[0].iter().all(|channel| channel[source_end..].iter().all(|value| *value == 0.)));
    drop(retired); drop(expected_retired); // Retire old buffers away from audio.
    pcm
}

#[test]
fn changed_sample_headers_replace_old_cursors_and_match_complete_pcm_without_heap() {
    for (rate,frames) in [(48000,2048),(96000,8192)] {
        let fixed = performance(rate,frames,&[128]);
        let fragmented = performance(rate,frames,&[1,7,31,127]);
        let error = fixed.iter().flatten().flatten().zip(fragmented.iter().flatten().flatten())
            .fold(0_f32, |error,(a,b)| error.max((a-b).abs()));
        assert!(error < 2e-5, "fragments changed source geometry PCM by{error}");
    }
}
