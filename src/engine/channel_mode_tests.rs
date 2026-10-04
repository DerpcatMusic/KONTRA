//! Receiver actions, not a claim that Kontakt implements keyboard Mono/Omni.
use super::*;
use crate::{audio::Sample, import::{Group, Loop, Zone}, midi::{Control, ControlValue}};

const FRAMES: usize = 2048;

fn owner(id: i32, input: u8, key: u8) -> HostNote {
    HostNote { port: 0, channel: input, key, id, clap: true }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern { port: i32::from(n.port), channel: i32::from(n.channel),
        key: i32::from(n.key), id: n.id, clap: n.clap }
}
fn engine(shared: bool, native_modes: bool, source: Option<&str>) -> Engine {
    let zones = [(60,-1.),(61,1.),(62,-1.)].map(|(key,pan)| Zone {
        root: key, low_key: key, high_key: key, pan, loop_range: Some(Loop {
            start: 0, end: 64, crossfade: 0, alternating: false, until_release: false,
        }), ..Zone::default()
    });
    let mut bank = Bank::from_samples(vec![Group { key_tracking: false, ..Group::default() }],
        zones.to_vec(), vec![(Default::default(), Sample {
            rate: 48_000, frames: vec![[0.25;2];4096],
        })]).unwrap().with_prepared_settings(|rows| {
            rows[0].envelope = Some(Ahdsr { release: 0.002, ..Ahdsr::UNITY });
        }).unwrap();
    bank.initial_state.as_mut().unwrap().controller_options.standard_note_modes = native_modes;
    let mut e = Engine::default(); e.reset(48_000.); e.set_lanes(shared);
    e.set_home_channel(7); e.set_bank(Some(Box::new(bank)));
    if let Some(source) = source {
        let (rt, errors) = Runtime::with_scripts(&[source],
            &mut crate::ksp::LogEngine::default(), 1, Vec::new());
        assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
    }
    e
}
fn received(e: &mut Engine, home: u8, input: u8, cc: u8, value: u8) {
    e.control_from_received(home,input,crate::midi::ReceivedControl::without_parameter(Control::Cc(cc,ControlValue::from_midi1(value))));
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>;2], from: usize, to: usize, parts: &[usize]) {
    let (mut at, mut part) = (from,0);
    while at < to {
        let count = parts[part % parts.len()].min(to-at);
        let [left,right] = pcm;
        e.render_checked(&mut left[at..at+count],&mut right[at..at+count]).unwrap();
        at += count; part += 1;
    }
    e.render(&mut [],&mut []);
}
fn same(a: &[Vec<f32>;2], b: &[Vec<f32>;2]) {
    assert!(a.iter().flatten().zip(b.iter().flatten()).all(|(a,b)|
        a.is_finite() && b.is_finite() && (a-b).abs()<2e-6),
        "complete receiver PCM disagrees with independently scheduled note/expression oracle");
}

fn mode_pcm(cc: u8, native: bool, exact: bool, scripted: bool,
    pedal: u8, shared: bool, parts: &[usize]) -> ([Vec<f32>;2],[u64;3]) {
    let source = scripted.then_some("on note\nwait(1000)\nend on\non release\nwait(1000)\nend on");
    let (mut actual,mut oracle) = (engine(shared,native,source),engine(false,native,source));
    let notes = [owner(910,1,60),owner(911,2,61),owner(912,5,62)];
    let homes = [7,9,7];
    let (mut pcm,mut expected) = (std::array::from_fn(|_|vec![0.;FRAMES]),
        std::array::from_fn(|_|vec![0.;FRAMES]));
    let mut ends = [0;3];
    let action = cc != 123 || native;
    assert_eq!(crate::plugin::tests::allocations(|| {
        actual.configure_mpe(Some((0,2,48))); oracle.configure_mpe(Some((0,2,48)));
        for e in [&mut actual,&mut oracle] {
            for (n,home) in notes.into_iter().zip(homes) {
                if exact { assert!(e.host_note_on(n,home,n.key,127,0.)); }
                else { e.note_on_from(home,n.channel,n.key,127); }
            }
        }
        let off = |e:&mut Engine,index:usize| {
            if exact { e.host_note_off(pattern(notes[index])); }
            else { e.note_off_from(homes[index],notes[index].channel,notes[index].key); }
        };
        let mut from = 0;
        for to in [96,192,384,512,768,FRAMES] {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut oracle,&mut expected,from,to,&[128]); from = to;
            match to {
                96 => for e in [&mut actual,&mut oracle] { received(e,7,0,pedal,127); },
                192 => {
                    received(&mut actual,7,0,cc,if cc==126 {2} else {0});
                    if action { off(&mut oracle,0); off(&mut oracle,1); }
                    assert_eq!(actual.player.library_receiver.cc[7][cc as usize],if cc==126 {2} else {0});
                    assert_eq!(actual.cc_state()[5][cc as usize],0);
                }
                384 => for e in [&mut actual,&mut oracle] { received(e,9,0,pedal,0); },
                512 => {
                    // Disabled saved CC123 leaves both members held. The four
                    // other mode numbers retain their existing release action.
                    if !action { off(&mut actual,0); off(&mut actual,1); off(&mut oracle,0); off(&mut oracle,1); }
                }
                768 => { off(&mut actual,2); off(&mut oracle,2); },
                _ => {}
            }
        }
        actual.mark_host_notes(); oracle.mark_host_notes();
        if exact {
            for (index,n) in notes.into_iter().enumerate() {
                ends[index] = actual.host_note_end_at(n).expect("released exact MIDI owner");
                assert_eq!(Some(ends[index]),oracle.host_note_end_at(n));
                assert!(!actual.host_note_pending(n));
            }
            assert!(ends[0]<ends[2] && ends[1]<ends[2],"unrelated physical input must survive manager release");
        }
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        if let Some(rt)=actual.script() { assert!(rt.diagnostics().is_empty()); }
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][128]>0.25 && pcm[1][128]>0.1);
    assert!(pcm[0][320]>0.25 && pcm[1][320]>0.1,"mode release must obey the selected pedal");
    assert!(pcm[0][700]>0.1 && pcm[1][700].abs()<1e-8,"only unrelated physical input remains audible");
    assert!(pcm.iter().all(|row|row[1024..].iter().all(|v|v.abs()<1e-8)));
    (pcm,ends)
}

#[test]
fn cc123_through127_preserve_saved_policy_physical_mpe_pedals_and_complete_pcm_without_heap() {
    for cc in 123..=127 { for native in [false,true] { for exact in [false,true] {
        for scripted in [false,true] { for pedal in [64,66] {
            let (fixed,ends)=mode_pcm(cc,native,exact,scripted,pedal,false,&[128]);
            for shared in [false,true] {
                let (fragmented,other)=mode_pcm(cc,native,exact,scripted,pedal,shared,&[1,7,31,127,13,64]);
                same(&fixed,&fragmented); assert_eq!(ends,other);
            }
        }}
    }}}
}

fn local_control_pcm(ignored: bool, shared: bool, parts: &[usize]) -> ([Vec<f32>;2],[u64;2]) {
    let source = format!("on init\ndeclare %notes[128]\nend on\non note\n%notes[$EVENT_NOTE] := $EVENT_ID\nend on\non controller\nif ($CC_NUM=122)\n{}if (%CC[122]=127 and $MIDI_CHANNEL=7)\nwait(1000)\nchange_vol(%notes[60],-6000,0)\nend if\nend if\nend on",if ignored {"ignore_controller\n"} else {""});
    let (mut actual,mut oracle) = (engine(shared,true,Some(&source)),engine(false,true,None));
    let notes = [owner(920,1,60),owner(921,2,61)];
    let (mut pcm,mut expected) = (std::array::from_fn(|_|vec![0.;FRAMES]),
        std::array::from_fn(|_|vec![0.;FRAMES]));
    let mut ends = [0;2];
    assert_eq!(crate::plugin::tests::allocations(|| {
        for e in [&mut actual,&mut oracle] {
            for n in notes { assert!(e.host_note_on(n,7,n.key,127,0.)); }
        }
        let mut from = 0;
        for to in [256,280,304,512,1024,FRAMES] {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut oracle,&mut expected,from,to,&[128]); from = to;
            match to {
                256 => {
                    received(&mut actual,7,1,122,127);
                    assert_eq!(actual.player.library_receiver.cc[7][122],127);
                    assert_eq!(actual.cc_state()[1][122],if ignored {0} else {127});
                    assert_eq!(actual.cc_state()[2][122],0);
                }
                280 => {
                    actual.set_home_channel(9);
                    received(&mut actual,9,2,122,0);
                    assert_eq!(actual.player.library_receiver.cc[7][122],127);
                    assert_eq!(actual.player.library_receiver.cc[9][122],0);
                }
                304 => oracle.host_expression(pattern(notes[0]),HostExpression::Gain(10f32.powf(-6./20.))),
                1024 => for e in [&mut actual,&mut oracle] { for n in notes { e.host_note_off(pattern(n)); } },
                _ => {}
            }
        }
        actual.mark_host_notes(); oracle.mark_host_notes();
        for (index,n) in notes.into_iter().enumerate() {
            ends[index]=actual.host_note_end_at(n).expect("local-control callback owner completed");
            assert_eq!(Some(ends[index]),oracle.host_note_end_at(n));
            assert!(!actual.host_note_pending(n));
        }
        assert_eq!(actual.cc_state()[1][122],if ignored {0} else {127});
        assert_eq!(actual.player.library_receiver.cc[7][122],127);
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }),0);
    same(&pcm,&expected);
    let gain = 10f32.powf(-6./20.);
    assert!(pcm[0][640..1024].iter().all(|v|(*v-0.25*gain).abs()<2e-6));
    assert!(pcm[1][640..1024].iter().all(|v|(*v-0.25).abs()<2e-6));
    assert!(pcm.iter().all(|row|row[1280..].iter().all(|v|v.abs()<1e-8)));
    (pcm,ends)
}

#[test]
fn cc122_controller_callback_wait_ignore_and_receiver_capture_reach_complete_pcm_without_heap() {
    for ignored in [false,true] {
        let (fixed,ends)=local_control_pcm(ignored,false,&[128]);
        for shared in [false,true] {
            let (fragmented,other)=local_control_pcm(ignored,shared,&[1,7,31,127,13,64]);
            same(&fixed,&fragmented); assert_eq!(ends,other);
        }
    }
}
