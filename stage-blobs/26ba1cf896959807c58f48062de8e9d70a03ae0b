//! Independent pitch PCM for RPN0's retained coarse field, not floor(total range).
use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostExpression, HostNote, HostPattern, envelope::Ahdsr},
    import::{Group, Zone},
    ksp::{LogEngine, Runtime},
    midi::{Bend, Control, NativeParameter, NativeParameterData},
};
use std::path::PathBuf;

fn engine(scripted: bool) -> Box<Engine> {
    let path = PathBuf::from("rpn compound fields");
    let frames = (0..16_384).map(|i| {
        let phase = std::f32::consts::TAU * (i % 97) as f32 / 97.;
        [0.11 + 0.05 * phase.sin(), 0.08 + 0.03 * phase.cos()]
    }).collect();
    let bank = Bank::from_samples(vec![Group::default()], vec![Zone {
        root:60, low_key:60, high_key:60, sample:path.clone(), ..Default::default()
    }], vec![(path, Sample { rate:48_000, frames })]).unwrap()
        .with_prepared_settings(|groups| groups[0].envelope = Some(Ahdsr { release:0.003, ..Ahdsr::UNITY })).unwrap();
    let mut e = Box::new(Engine::default());
    e.reset(48_000.); e.attack=0.; e.set_lanes(false); e.set_bank(Some(Box::new(bank)));
    if scripted {
        let source = "on init\nend on\non note\nwait(1000)\nend on\non rpn\nmessage($RPN_ADDRESS & \":\" & $RPN_VALUE)\nend on";
        let (rt, errors) = Runtime::with_scripts(&[source], &mut LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}"); e.set_script(Some(Box::new(rt)));
    }
    e
}
fn note(channel: u8) -> HostNote { HostNote {port:3, channel, key:60, id:3001, clap:true} }
fn pattern(n: HostNote) -> HostPattern {
    HostPattern {port:i32::from(n.port), channel:i32::from(n.channel), key:i32::from(n.key), id:n.id, clap:n.clap}
}
fn cc(e: &mut Engine, channel: u8, controller: u8, value: u8) { e.cc_from(7,channel,controller,value); }
fn select(e: &mut Engine, channel: u8) { cc(e,channel,101,0); cc(e,channel,100,0); }
fn native(e: &mut Engine, channel: u8, coarse: u8, cents: u8) {
    assert!(e.native_parameter_from(7,channel,NativeParameter {
        registered:true, address:0,
        data:NativeParameterData::Absolute((u32::from(coarse)<<25) | (u32::from(cents)<<18) | 0x12345),
    }));
}
fn bend(e: &mut Engine, channel: u8, value: f64) {
    e.control_from(7,channel,Control::Bend(Bend::new(value).unwrap()));
}
fn tune(e: &mut Engine, n: HostNote, value: f32) { e.host_expression(pattern(n),HostExpression::Tune(value)); }
fn start(e: &mut Engine, n: HostNote, pan: f32) {
    assert!(e.host_note_on(n,7,60,127,0.)); e.host_expression(pattern(n),HostExpression::Pan(pan));
}
fn same(a: &[Vec<f32>;2], b: &[Vec<f32>;2]) {
    for channel in 0..2 { for (at,(&a,&b)) in a[channel].iter().zip(&b[channel]).enumerate() {
        assert!((a-b).abs()<4e-6,"channel{channel} frame{at}: {a} != {b}");
    }}
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>;2], from: usize, to: usize, partition: &[usize]) {
    let (mut at,mut part)=(from,0);
    while at<to {
        let n=partition[part%partition.len()].min(to-at); let [left,right]=pcm;
        e.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
        e.render_checked(&mut [],&mut []).unwrap(); at+=n;part+=1;
    }
    e.mark_host_notes();
}
fn timeline(scripted: bool, partition: &[usize]) -> [Vec<f32>;2] {
    const FRAMES: usize = 7168;
    let (mut actual,mut oracle)=(engine(scripted),engine(false));
    let (mut pcm,mut expected)=(std::array::from_fn(|_|vec![0.;FRAMES]),std::array::from_fn(|_|vec![0.;FRAMES]));
    let (left,right)=(note(4),note(7));
    assert_eq!(crate::plugin::tests::allocations(|| {
        // The oracle has no negotiated range or channel bend. Its literal exact
        // tuning is the independent semitone sum times the received bend.
        select(&mut actual,4);cc(&mut actual,4,6,10);cc(&mut actual,4,38,127);
        select(&mut actual,7);cc(&mut actual,7,6,6);cc(&mut actual,7,38,25);
        bend(&mut actual,4,0.5);bend(&mut actual,7,-0.25);
        for (n,pan) in [(left,-1.),(right,1.)] {start(&mut actual,n,pan);start(&mut oracle,n,pan);}
        tune(&mut oracle,left,5.635);tune(&mut oracle,right,-1.5625);
        let mut from=0;
        for to in [512,1024,1536,2048,2560,3072,3584,4096,4352,4545,4800,5312,5824,6017,FRAMES] {
            render(&mut actual,&mut pcm,from,to,partition);render(&mut oracle,&mut expected,from,to,&[128]);from=to;
            match to {
                512 => {
                    cc(&mut actual,4,121,0);select(&mut actual,4);cc(&mut actual,4,38,25);bend(&mut actual,4,0.5);
                    tune(&mut oracle,left,5.125); // retained coarse10 + .25, never floor(11.27)
                },
                1024 => {
                    cc(&mut actual,4,6,2);native(&mut actual,4,12,127);cc(&mut actual,4,38,50);
                    tune(&mut oracle,left,6.25); // native coarse12 supersedes selected coarse2
                    if scripted {assert_eq!(actual.script().unwrap().last_message(),"0:1586");}
                },
                1536 => {
                    cc(&mut actual,4,101,127);cc(&mut actual,4,100,127);native(&mut actual,4,10,127);
                    cc(&mut actual,4,38,25);tune(&mut oracle,left,5.635); // native must not re-arm null selection
                    if scripted {assert_eq!(actual.script().unwrap().last_message(),"0:1407");}
                },
                2048 => {select(&mut actual,4);cc(&mut actual,4,38,25);tune(&mut oracle,left,5.125);},
                2560 => {cc(&mut actual,4,96,127);cc(&mut actual,4,38,50);tune(&mut oracle,left,5.25);},
                3072 => {cc(&mut actual,4,6,3);cc(&mut actual,4,38,127);tune(&mut oracle,left,2.135);},
                3584 => {cc(&mut actual,4,121,0);select(&mut actual,4);cc(&mut actual,4,38,5);bend(&mut actual,4,0.5);
                    tune(&mut oracle,left,1.525);},
                4096 => {actual.host_note_off(pattern(left));oracle.host_note_off(pattern(left));},
                4352 => {actual.host_note_off(pattern(right));oracle.host_note_off(pattern(right));},
                4545 => {
                    // Authored .003s release:144 samples at -60dB, crossing the
                    // ordinary -80dB cutoff after193 samples. Both exact tuples
                    // retain their independent completion timestamps.
                    for e in [&mut actual,&mut oracle] {
                        assert_eq!(e.host_note_end_at(left),Some(4289));assert_eq!(e.host_note_end_at(right),Some(4545));
                        e.retire_host_note(left);e.retire_host_note(right);
                    }
                },
                4800 => {
                    // Same exact host tuple is admitted with a fresh generation.
                    start(&mut actual,left,-1.);start(&mut oracle,left,-1.);tune(&mut oracle,left,1.525);
                },
                5312 => {native(&mut actual,4,8,127);cc(&mut actual,4,38,25);tune(&mut oracle,left,4.125);},
                5824 => {actual.host_note_off(pattern(left));oracle.host_note_off(pattern(left));},
                6017 => {assert_eq!(actual.host_note_end_at(left),Some(6017));},
                _ => {},
            }
        }
        assert_eq!(actual.active_voices(),0);assert_eq!(actual.dropped_commands(),0);assert_eq!(actual.underruns(),0);
        assert_eq!(actual.unsupported_native_controls(),0);
        if let Some(rt)=actual.script() {assert!(rt.diagnostics().is_empty());assert!(rt.faults().next().is_none());}
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][700].abs()>0.01 && pcm[1][1100].abs()>0.01,"both independently routed inputs must sound");
    assert!(pcm.iter().all(|channel|channel[6017..].iter().all(|&sample|sample==0.)));
    pcm
}
#[test]
fn rpn0_fine_edits_keep_compound_coarse_fields_after_reset_and_native_midi1_replacement_in_complete_pcm() {
    for scripted in [false,true] {same(&timeline(scripted,&[128]),&timeline(scripted,&[1,7,31,127]));}
}

fn relative_fields_timeline(mpe: bool, partition: &[usize]) -> [Vec<f32>;2] {
    const FRAMES: usize = 4096;
    let (mut actual,mut oracle)=(engine(false),engine(false));
    let (mut pcm,mut expected)=(std::array::from_fn(|_|vec![0.;FRAMES]),std::array::from_fn(|_|vec![0.;FRAMES]));
    let (a,b)=if mpe {(1,2)}else{(4,7)};
    let (left,right)=(note(a),note(b));
    if mpe {actual.configure_mpe(Some((0,2,48)));}
    assert_eq!(crate::plugin::tests::allocations(|| {
        select(&mut actual,a);select(&mut actual,b);
        cc(&mut actual,a,6,10);cc(&mut actual,a,38,127);
        if !mpe {cc(&mut actual,b,6,6);cc(&mut actual,b,38,25);}
        bend(&mut actual,a,0.5);bend(&mut actual,b,-0.25);
        for (n,pan) in [(left,-1.),(right,1.)] {start(&mut actual,n,pan);start(&mut oracle,n,pan);}
        tune(&mut oracle,left,5.635);tune(&mut oracle,right,if mpe {-2.8175}else{-1.5625});
        let mut from=0;
        for to in [256,512,768,1024,1280,1536,1792,2048,2177,2241,2560,2816,3072,3328,3521,FRAMES] {
            render(&mut actual,&mut pcm,from,to,partition);render(&mut oracle,&mut expected,from,to,&[128]);from=to;
            match to {
                256 => {
                    cc(&mut actual,a,97,127); // coarse10/cents127 becomes10/126.
                    tune(&mut oracle,left,5.63);if mpe {tune(&mut oracle,right,-2.815);}
                },
                512 => {
                    cc(&mut actual,a,38,25); // fine-only keeps coarse10, not11.
                    tune(&mut oracle,left,5.125);if mpe {tune(&mut oracle,right,-2.5625);}
                },
                768 => {
                    cc(&mut actual,a,6,12);cc(&mut actual,a,38,99);
                    tune(&mut oracle,left,6.495);if mpe {tune(&mut oracle,right,-3.2475);}
                },
                1024 => {
                    cc(&mut actual,a,96,0); // legal cents100 does not rewrite coarse12.
                    tune(&mut oracle,left,6.5);if mpe {tune(&mut oracle,right,-3.25);}
                },
                1280 => {
                    cc(&mut actual,b,38,25); // other MPE member's selected baseline is12.
                    if mpe {tune(&mut oracle,left,6.125);tune(&mut oracle,right,-3.0625);}
                },
                1536 => {
                    cc(&mut actual,a,121,0);select(&mut actual,a);cc(&mut actual,a,38,25);bend(&mut actual,a,0.5);
                    tune(&mut oracle,left,6.125);
                },
                1792 => {
                    cc(&mut actual,a,6,10);cc(&mut actual,a,38,127);cc(&mut actual,a,97,1);
                    cc(&mut actual,a,121,0);select(&mut actual,a);cc(&mut actual,a,38,25);bend(&mut actual,a,0.5);
                    tune(&mut oracle,left,5.125);if mpe {tune(&mut oracle,right,-2.5625);}
                },
                2048 => {actual.host_note_off(pattern(left));oracle.host_note_off(pattern(left));},
                2177 => {
                    cc(&mut actual,b,6,2);cc(&mut actual,b,38,100);cc(&mut actual,b,96,127);cc(&mut actual,b,38,25);
                    // The released MPE member keeps its captured5.125 semitones.
                    tune(&mut oracle,right,-0.5625);
                },
                2241 => {
                    for e in [&mut actual,&mut oracle] {assert_eq!(e.host_note_end_at(left),Some(2241));e.retire_host_note(left);}
                },
                2560 => {
                    start(&mut actual,left,-1.);start(&mut oracle,left,-1.);
                    tune(&mut oracle,left,if mpe {1.125}else{5.125});
                },
                2816 => {
                    cc(&mut actual,a,38,127);cc(&mut actual,a,97,0);
                    tune(&mut oracle,left,if mpe {1.63}else{5.63});if mpe {tune(&mut oracle,right,-0.815);}
                },
                3072 => {
                    cc(&mut actual,a,38,25);
                    tune(&mut oracle,left,if mpe {1.125}else{5.125});if mpe {tune(&mut oracle,right,-0.5625);}
                },
                3328 => {for n in [left,right] {actual.host_note_off(pattern(n));oracle.host_note_off(pattern(n));}},
                3521 => {for e in [&mut actual,&mut oracle] {for n in [left,right] {assert_eq!(e.host_note_end_at(n),Some(3521));}}},
                _ => {},
            }
        }
        assert_eq!(actual.active_voices(),0);assert_eq!(actual.underruns(),0);
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][600].abs()>0.01 && pcm[1][1100].abs()>0.01);
    assert!(pcm.iter().all(|channel|channel[3521..].iter().all(|&x|x==0.)));
    pcm
}

#[test]
fn relative_rpn0_supported_cents_keep_coarse_fields_in_ordinary_and_mpe_complete_pcm_without_heap() {
    for mpe in [false,true] {same(&relative_fields_timeline(mpe,&[128]),&relative_fields_timeline(mpe,&[1,7,31,127]));}
}
