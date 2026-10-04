//! Independent source-start oracle for the pinned native key-table law.
use crate::{
    audio::Sample,
    engine::{envelope::Ahdsr, instrument::ControllerPolicy, Bank, Engine, GroupMask,
        HostNote, HostPattern, NoteEvent},
    import::{Group, ModAssignment, ModSource, ModTarget, Zone},
};

const RATE: f32 = 48_000.;
const FRAMES: usize = 6146;
const SAMPLE: usize = 2048;
const RANGE: u32 = 511;
// Hand-replayed native wrapper sizes:1025 ->513+512;
// 3073 ->769+769+769+766; 2048 ->1024+1024.
const SPANS: [(usize, usize); 8] = [
    (0,513),(513,1025),(1025,1794),(1794,2563),
    (2563,3332),(3332,4098),(4098,5122),(5122,6146),
];
const RELEASES: [(usize, usize); 5] = [
    (37,257), (1100,1900), (2000,2800), (3000,3424), (3548,4200),
];

fn raw(reset: usize, admission: usize) -> f32 {
    let origin = SPANS.iter().find(|&&(a,b)| a<=reset && reset<b).unwrap().0;
    let mut age = ((reset-origin) as f32 * -1000.) / RATE;
    for &(a,b) in &SPANS {
        if b>reset && b<=admission {
            let delta = (((b-a) as f64 * 1000.) * (1. / f64::from(RATE))) as f32;
            age += delta;
        }
    }
    (100.-age).max(0.) / 100.
}

fn owner(index: usize) -> HostNote {
    HostNote { port:0, channel:[2,5,4,6,9][index], key:if index==3 {61} else {60},
        id:900+index as i32, clap:true }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern { port:i32::from(n.port), channel:i32::from(n.channel),
        key:i32::from(n.key), id:n.id, clap:n.clap }
}
fn engine(counter: bool, scalar: f32, scripted: bool) -> Box<Engine> {
    let route = ModAssignment { cc_default:None, name:"Native RTC start".into(),
        source:if counter {ModSource::ReleaseTriggerCounter} else {ModSource::Constant},
        target:ModTarget::SampleStart, intensity:0.5*scalar, invert:false, lag_ms:0, shaper:None };
    let mut bank=Bank::from_samples(vec![Group { release_trigger:counter,
        release_counter_ms:100, key_tracking:false, mods:vec![route], ..Group::default() }],
        [60,61].map(|key| Zone { root:key, low_key:key, high_key:key,
            start_mod:Some(RANGE), ..Zone::default() }).to_vec(),
        vec![(Default::default(),Sample { rate:48_000, frames:(0..SAMPLE).map(|i| {
            let phase=std::f64::consts::TAU*i as f64/71.;
            [(phase.sin()*0.03+0.011*(phase*2.).sin()) as f32,
                (phase.cos()*0.021+0.009*(phase*3.).sin()) as f32]
        }).collect() })]).unwrap().with_prepared_settings(|rows| {
            rows[0].envelope=Some(Ahdsr::UNITY);
        }).unwrap();
    if counter { bank.initial_state.as_mut().unwrap().controller_policy=ControllerPolicy::Native; }
    let mut e=Box::new(Engine::default()); e.reset(f64::from(RATE)); e.attack=0.;
    e.set_home_channel(7); e.set_lanes(false); e.set_bank(Some(Box::new(bank)));
    if scripted {
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[r#"on note
if ($EVENT_NOTE=48)
ignore_event($EVENT_ID)
wait(1000)
play_note(60,127,0,-1)
end if
end on
on release
if ($EVENT_NOTE=61)
ignore_event($EVENT_ID)
wait(3000)
note_off($EVENT_ID)
end if
end on"#],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
    }
    e
}

fn timeline(fragments: &[usize]) -> Vec<[f32;2]> {
    let mut actual=engine(true,1.,true);
    let factors=RELEASES.map(|(reset,admission)| raw(reset,admission));
    let mut reference:Vec<_>=factors.iter().map(|&x| engine(false,x.min(1.),false)).collect();
    let mut pcm=Vec::with_capacity(FRAMES);
    let mut oracle=Vec::with_capacity(FRAMES);
    let (mut l,mut r,mut other,mut other_r)=([0.;128],[0.;128],[0.;128],[0.;128]);
    let (mut frame,mut fragment)=(0,0);
    assert_eq!(crate::plugin::tests::allocations(|| {
        actual.begin_audio_block(1025,1,true);
        for stop in [37,257,600,1025,1100,1900,2000,2050,2100,2800,
            3000,3280,3400,3500,3548,4098,4200,FRAMES] {
            while frame<stop {
                let n=fragments[fragment%fragments.len()].min(stop-frame);
                actual.render(&mut l[..n],&mut r[..n]);
                let mut sum=[[0.;2];128];
                for e in &mut reference {
                    e.render(&mut other[..n],&mut other_r[..n]);
                    for i in 0..n { sum[i][0]+=other[i]; sum[i][1]+=other_r[i]; }
                }
                for i in 0..n { pcm.push([l[i],r[i]]); oracle.push(sum[i]); }
                frame+=n; fragment+=1;
            }
            if stop==1025 { actual.begin_audio_block(3073,1,true); }
            if stop==4098 { actual.begin_audio_block(2048,1,true); }
            match stop {
                37=>assert!(actual.host_note_on(owner(0),7,60,127,0.)),
                257=>{
                    assert_eq!(actual.player.release_counter.factor(60,100).to_bits(),factors[0].to_bits());
                    assert!(factors[0]>1.,"negative wrapper age must preserve the raw scalar above unity");
                    actual.host_note_off(pattern(owner(0)));
                },
                600=>assert!(actual.host_note_on(owner(1),7,60,127,0.)),
                1100=>{
                    // A no-zone final ordinary admission resets the same
                    // program key without adopting or releasing either root.
                    let none=GroupMask::none();
                    assert!(actual.start_event(&NoteEvent { groups:Some(&none),
                        ..NoteEvent::new(12,60,127) }).is_some());
                },
                1900=>{
                    assert_eq!(actual.player.release_counter.factor(60,100).to_bits(),factors[1].to_bits());
                    actual.host_note_off(pattern(owner(1)));
                },
                2000=>assert!(actual.host_note_on(owner(2),7,60,127,0.)),
                2050=>actual.cc(4,64,127),
                2100=>actual.host_note_off(pattern(owner(2))),
                2800=>{
                    assert_eq!(actual.player.pending_releases.len(),1);
                    assert_eq!(actual.player.release_counter.factor(60,100).to_bits(),factors[2].to_bits());
                    actual.cc(4,64,0);
                },
                3000=>assert!(actual.host_note_on(owner(3),7,61,127,0.)),
                3280=>actual.host_note_off(pattern(owner(3))),
                3400=>assert!(actual.host_note_on(owner(4),7,60,127,0.)),
                3500=>actual.note_on_from(7,8,48,127),
                4200=>{
                    // The ignored48 input only seeds60 when its waited child
                    // reaches final ordinary admission at3548.
                    assert_eq!(actual.player.release_counter.factor(60,100).to_bits(),factors[4].to_bits());
                    actual.host_note_off(pattern(owner(4)));
                },
                _=>{},
            }
            for (index,&(_,admission)) in RELEASES.iter().enumerate() {
                if stop==admission {
                    reference[index].note_on(7,owner(index).key,127);
                }
            }
            // The waited release timer is internal to the current host
            // scope. Its independent reference starts at its authored deadline.
            if stop==3400 {
                // Finish to3424 explicitly so the reference onset can be
                // scheduled without peeking at a runtime/voice cursor.
                while frame<3424 {
                    let n=fragments[fragment%fragments.len()].min(3424-frame);
                    actual.render(&mut l[..n],&mut r[..n]);
                    let mut sum=[[0.;2];128];
                    for e in &mut reference {
                        e.render(&mut other[..n],&mut other_r[..n]);
                        for i in 0..n {sum[i][0]+=other[i];sum[i][1]+=other_r[i];}
                    }
                    for i in 0..n {pcm.push([l[i],r[i]]);oracle.push(sum[i]);}
                    frame+=n;fragment+=1;
                }
                reference[3].note_on(7,61,127);
            }
        }
    }),0);
    let error=pcm.iter().zip(&oracle).flat_map(|(a,b)|[(a[0]-b[0]).abs(),(a[1]-b[1]).abs()]).fold(0_f32,f32::max);
    assert!(error<2e-6,"shared native RTC source-start differs from constant authored PCM: {error}");
    assert!(oracle.iter().any(|s|s[0].abs()>0.01));
    assert!(pcm[..257].iter().all(|s|*s==[0.;2]));
    assert!(pcm[6100..].iter().all(|s|*s==[0.;2]),"finite releases must finish without stranded audio");
    actual.mark_host_notes();
    for (index,&(_,admission)) in RELEASES.iter().enumerate() {
        let offset=(0.5*factors[index].min(1.)*RANGE as f32) as usize;
        let end=(admission+SAMPLE-offset) as u64;
        assert_eq!(actual.host_note_end_at(owner(index)),Some(end),"root{index} exact source EOF");
        assert_eq!(reference[index].active_voices(),0,"finite reference{index} source EOF");
    }
    assert_eq!(actual.active_voices(),0);
    assert!(actual.player.pending_releases.is_empty());
    assert!(actual.script.as_deref().unwrap().diagnostics().is_empty());
    assert!(actual.take_stream_error().is_none());
    assert_eq!(actual.dropped_commands(),0);
    pcm
}

#[test]
fn native_key_rtc_owns_host_spans_final_admission_waits_pedals_and_complete_pcm_without_heap() {
    let fixed=timeline(&[128]);
    let fragmented=timeline(&[1,7,31,127]);
    assert_eq!(fixed.len(),FRAMES);
    assert!(fixed.iter().zip(&fragmented).all(|(a,b)|
        (a[0]-b[0]).abs()<2e-6 && (a[1]-b[1]).abs()<2e-6));
}

#[test]
fn native_rtc_empty_abandoned_zero_rate_and_bypass_scopes_retain_only_elapsed_float_age() {
    let mut empty=Box::new(Engine::default());
    empty.reset(f64::from(RATE));
    empty.begin_audio_block(1025,1,true);
    empty.player.release_counter.reset(0,60,f64::from(RATE));
    assert_eq!(crate::plugin::tests::allocations(|| {
        empty.render(&mut [0.;384],&mut [0.;384]);
        empty.render(&mut [],&mut []);
        assert_eq!(empty.player.release_counter.factor(60,100),1.);
        empty.render(&mut [0.;258],&mut [0.;258]);
        //513 completed frames plus129 genuinely rendered frames close at the
        //old rate. The abandoned remaining383 frames never enter this cell.
        empty.reset(96_000.);
        empty.begin_audio_block(0,1,true);
        empty.render(&mut [],&mut []);
    }),0);
    let d=|n:usize| ((n as f64*1000.)*(1./f64::from(RATE))) as f32;
    let age=d(513)+d(129);
    assert_eq!(empty.player.release_counter.factor(60,100).to_bits(),((100.-age)/100.).to_bits());
    assert!(empty.player.release_counter.next_boundary().is_none());

    let mut bypass=engine(true,1.,false);
    let (rt,errors)=crate::ksp::Runtime::with_scripts(&[r#"on init
SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)
end on
on controller
if ($CC_NUM=1)
reset_rls_trig_counter(60)
end if
end on"#],&mut crate::ksp::LogEngine::default(),0,Vec::new());
    assert!(errors.iter().all(Option::is_none)); bypass.set_script(Some(Box::new(rt)));
    bypass.begin_audio_block(1025,1,true);
    assert_eq!(crate::plugin::tests::allocations(|| {
        bypass.render(&mut [0.;600],&mut [0.;600]);
        let prior=bypass.player.release_counter.factor(60,100).to_bits();
        bypass.note_on(3,60,127); bypass.render(&mut [0.;1],&mut [0.;1]);
        assert_eq!(bypass.player.release_counter.factor(60,100).to_bits(),prior,
            "system release bypass must suppress ordinary reset, not table advancement");
        bypass.cc(9,1,127); bypass.render(&mut [0.;1],&mut [0.;1]);
    }),0);
    // Explicit reset at601 uses offset88 into the second native chunk and
    // ignores the controller's channel9. No table advancement occurred yet.
    let explicit=(100.-(88_f32*-1000.)/RATE)/100.;
    assert_eq!(bypass.player.release_counter.factor(60,100).to_bits(),explicit.to_bits());
    assert_eq!(bypass.player.release_counter.factor(60,0),1.);
    assert!(bypass.script.as_deref().unwrap().diagnostics().is_empty());

    // Native float additions intentionally follow caller host partitions.
    // The per-partition expectation is independent of production Counter;
    // we do not demand equality across different native processing scopes.
    let mut bits=[0;2];
    for (mode,chunk) in [127_usize,1024].into_iter().enumerate() {
        let mut e=Box::new(Engine::default()); e.reset(44_100.);
        e.player.release_counter.reset(0,60,44_100.);
        let (mut frame,mut age)=(0,0_f32);
        assert_eq!(crate::plugin::tests::allocations(|| {
            while frame<100_000 {
                let n=chunk.min(100_000-frame);
                e.begin_audio_block(n,1,true); e.render(&mut [0.;1024][..n],&mut [0.;1024][..n]);
                age+=((n as f64*1000.)*(1./44_100_f64)) as f32;
                frame+=n;
            }
        }),0);
        let expected=(10_000.-age)/10_000.;
        bits[mode]=e.player.release_counter.factor(60,10_000).to_bits();
        assert_eq!(bits[mode],expected.to_bits());
    }
    assert_ne!(bits[0],bits[1],"native f32 host partition drift must remain observable");
}
