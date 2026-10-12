//! The native source flag is finite source policy, not an immortal event pin.
use crate::{engine::{Engine, Expression, HostNote, HostPattern}, import::{Group, Zone},
    ksp::{LogEngine, Runtime}};
use super::primary_amp_tests::{create, saved};

const FRAMES: usize = 2048;
fn group() -> Group {
    let mut g = saved(false, [0, 0, 1, 0], 1);
    g.source_mode = Some(0);
    assert_eq!(g.native_primary_bypass_slot, Some(0));
    assert_eq!(g.modulators.len(), 1);
    g
}
fn script(engine: &mut Engine, mark: bool, wait: bool, bypass_release: bool) {
    script_length(engine, mark, wait, bypass_release, 0);
}
fn script_length(engine: &mut Engine, mark: bool, wait: bool, bypass_release: bool, duration: i32) {
    let source = format!("on init\ndeclare $child\n{}end on\non note\nignore_event($EVENT_ID)\n{}$child := play_note($EVENT_NOTE,$EVENT_VELOCITY,0,{duration})\n{}end on\non controller\nif ($CC_NUM=1)\n_will_never_terminate($child)\nend if\nend on",
        if bypass_release {"SET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)\n"} else {""},
        if wait {"wait(1000)\n"} else {""},
        if mark {"_will_never_terminate($child)\n"} else {""});
    let mut log = LogEngine::new(vec!["Primary".into(), "Release".into()], 48000.);
    let (rt, errors) = Runtime::with_scripts(&[&source], &mut log, 0, Vec::new());
    assert!(errors.iter().all(Option::is_none), "{errors:?}");
    engine.set_script(Some(Box::new(rt)));
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>;2], from: usize, to: usize, parts: &[usize]) {
    let mut at = from; let mut part = 0;
    while at < to {
        let n = parts[part % parts.len()].min(to-at);
        let [l,r] = pcm;
        e.render_checked(&mut l[at..at+n], &mut r[at..at+n]).unwrap();
        e.render_checked(&mut [], &mut []).unwrap(); at += n; part += 1;
    }
}
fn same(a: &[Vec<f32>;2], b: &[Vec<f32>;2]) {
    for ch in 0..2 { for (frame, (a,b)) in a[ch].iter().zip(&b[ch]).enumerate() {
        assert!((a-b).abs()<3e-6, "channel {ch}, frame {frame}: {a} vs {b}");
    }}
}
// Independent literal authored geometry: A/H/D/R = 10/3/6/30 source points.
// Uses no Native/Amplitude/phase methods. The final Decay recurrence value is
// the Release scale, without assuming it is bit-identical to authored sustain.
fn points() -> [f32;51] {
    let mut out = [0.;51];
    let base = (-20_000f64.ln()).exp() as f32;
    let start = 1f32 + base;
    let coefficient = f64::from(base/start).powf(1./10.) as f32;
    let mut state = start;
    for value in &mut out[..10] { *value = ((state-0.075)*-1. + (start-0.075)).max(0.); state *= coefficient; }
    out[10..13].fill(1.);
    let sustain = (0.3f32 + 0.075) - 0.075;
    let coefficient = (43f64/3.).powf(-1./6.) as f32;
    state = 1.075;
    for value in &mut out[13..19] { *value = ((state-0.075)*(1.-sustain)+sustain).max(0.); state *= coefficient; }
    let release_start = (state-0.075)*(1.-sustain)+sustain;
    let coefficient = (43f64/3.).powf(-1./30.) as f32;
    state = 1.075;
    for value in &mut out[19..49] { *value = ((state-0.075)*release_start).max(0.); state *= coefficient; }
    out
}
fn level(points: &[f32;51], frame: usize) -> f32 {
    let k = frame/32;
    if k >= points.len() { return 0.; }
    let previous = points[if k==0 {0} else {k-1}];
    previous + (points[k]-previous)*((frame&31) as f32*(1./32.))
}
fn timeline(parts: &[usize], wait: bool, mute: bool) -> ([Vec<f32>;2], u64) {
    let mut actual = create(&group(), false, 4096, false);
    script(&mut actual, true, wait, false);
    let mut dry = create(&Group::default(), false, 4096, false);
    dry.state.settings[0].envelope = Some(crate::engine::envelope::Ahdsr::UNITY);
    let mut pcm = std::array::from_fn(|_|vec![0.;FRAMES]);
    let mut plain = std::array::from_fn(|_|vec![0.;FRAMES]);
    let owner = HostNote {port:0,channel:4,key:60,id:993,clap:true};
    let onset = if wait {48} else {0};
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(actual.host_note_on(owner,0,60,127,0.));
        if onset==0 {dry.note_on(0,60,127);}
        let mut from = 0;
        for to in [48,64,96,173,401,1600,1648,1800,FRAMES] {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut dry,&mut plain,from,to,&[128]); from=to;
            match to {
                48 => {if onset==48 {dry.note_on(0,60,127);} actual.cc_from(0,4,64,127);},
                64 => actual.host_note_off(HostPattern {port:0,channel:4,key:60,id:993,clap:true}),
                173 | 401 if mute => for e in [&mut actual,&mut dry] {
                    e.set_expression_on(0,60,Expression {gain:if to==173 {0.} else {1.},..Default::default()});
                },
                1800 => actual.cc_from(0,4,64,0),
                _ => {},
            }
        }
        actual.mark_host_notes();
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        assert_eq!(actual.unsupported_native_controls(),0);
        assert!(!actual.host_note_pending(owner));
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }),0);
    let source = points();
    for ch in 0..2 { for frame in 0..FRAMES {
        let gain = if frame<onset {0.} else {level(&source,frame-onset)};
        let expected = plain[ch][frame]*gain;
        assert!((pcm[ch][frame]-expected).abs()<3e-6,"native complete source ch{ch} frame{frame}: {} vs {expected}",pcm[ch][frame]);
    }}
    if !mute {assert!(pcm[0][onset+432]>0.1,"authored envelope must reach audible Hold/Decay");}
    assert!(pcm[0][onset+1100]>0.001,"Release must retain finite audible tail");
    assert!(pcm.iter().all(|ch|ch[onset+1600..].iter().all(|x|*x==0.)));
    // The child owns 49 authored source points. Its two zero destination
    // endpoints coincide at point50/frame1600; physical key-up at64 is earlier
    // and has no retained callback. Sample duration0 does not follow key-up or
    // hold its finite amplitude source until the physical4 pedal-up at1800.
    let expected_end = (onset + 50*32) as u64;
    assert_eq!(actual.host_note_end_at(owner), Some(expected_end),
        "complete source endpoint is independent of host processing fragments");
    (pcm, expected_end)
}

#[test]
fn will_never_terminate_native_primary_has_finite_complete_pcm_and_exact_owner() {
    for wait in [false,true] { for mute in [false,true] {
        let (fixed,end)=timeline(&[128],wait,mute);
        let (irregular,other_end)=timeline(&[1,7,31,127,13,64],wait,mute);
        same(&fixed,&irregular); assert_eq!(end,other_end);
    }}
}

#[test]
fn will_never_terminate_rejects_unproven_sources_and_live_queued_edits_without_audio() {
    for case in 0..6 {
        let mut g=group();
        match case {
            0 => {let p=g.volume_env.as_mut().unwrap();p.attack_ms=0.;p.hold_ms=0.;p.decay_ms=0.;},
            1 => g.modulators.push(g.modulators[0].clone()),
            2 => g.volume_env.as_mut().unwrap().unknown_flag=1,
            _ => {},
        }
        let mut e=create(&g,false,4096,case==3);
        script(&mut e,true,false,false);
        let owner=HostNote {port:0,channel:4,key:60,id:994,clap:true};
        let mut pcm=std::array::from_fn(|_|vec![0.;FRAMES]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            if case==4 {e.state.settings[0].primary_bypass.as_mut().unwrap().1=true;}
            assert!(e.host_note_on(owner,0,60,127,0.));
            if case==5 {
                assert_eq!(e.active_voices(),0,"queued Start owns immutable flag before source creation");
                e.state.settings[0].primary_bypass.as_mut().unwrap().1=true;
            }
            e.host_note_off(HostPattern {port:0,channel:4,key:60,id:994,clap:true});
            render(&mut e,&mut pcm,0,FRAMES,&[1,7,31,127]); e.mark_host_notes();
            assert_eq!(e.active_voices(),0);assert_eq!(e.dropped_commands(),0);
            assert_eq!(e.unsupported_native_controls(),1);
            assert!(!e.host_note_pending(owner));
        }),0);
        assert!(pcm.iter().flatten().all(|x|*x==0.));
        if case!=5 {assert!(e.script().unwrap().diagnostics().iter().any(|s|s.contains("marked note admission failed")));}
    }
    // duration0 normally starts selected release groups even without attack
    // zones. This unproved roster must not escape marked capability rejection.
    for (bypass,duration) in [(false,0),(true,0),(true,-1)] { for mixed in [false,true] {
        let groups=if mixed {vec![group(),Group {release_trigger:true,..Group::default()}]}
            else {vec![Group {release_trigger:true,..Group::default()}]};
        let engine=|mark| {
            let zones=(0..groups.len()).map(|group|Zone {group,..Default::default()}).collect();
            let bank=crate::engine::Bank::from_samples(groups.clone(),zones,vec![(Default::default(),
                crate::audio::Sample {rate:48000,frames:vec![[0.25;2];4096]})]).unwrap();
            let mut e=Engine::default();e.reset(48000.);e.set_bank(Some(Box::new(bank)));
            script_length(&mut e,mark,false,bypass,duration);e
        };
        let (mut e,mut unmarked)=(engine(true),engine(false));
        let mut pcm=std::array::from_fn(|_|vec![0.;FRAMES]);
        let mut reference=std::array::from_fn(|_|vec![0.;FRAMES]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            e.note_on(0,60,127);unmarked.note_on(0,60,127);
            render(&mut e,&mut pcm,0,FRAMES,&[1,7,31,127]);
            render(&mut unmarked,&mut reference,0,FRAMES,&[128]);
            assert_eq!(e.active_voices(),0);assert_eq!(e.dropped_commands(),0);
            assert_eq!(e.unsupported_native_controls(),u64::from(mixed));
            assert!(unmarked.active_voices()>0,"unmarked immediate release fallback remains admitted");
        }),0);
        assert!(pcm.iter().flatten().all(|x|*x==0.));
        assert!(reference[0][256..].iter().any(|x|*x>0.01),"rejected marked roster was genuinely audible without the policy");
        assert!(e.script().unwrap().diagnostics().iter().any(|s|s.contains("marked note admission failed")));
    }}
}

#[test]
fn will_never_terminate_late_event_marker_does_not_rewrite_existing_source() {
    let mut actual=create(&group(),false,4096,false);
    let mut reference=create(&group(),false,4096,false);
    for e in [&mut actual,&mut reference] {script(e,false,false,false);}
    let mut pcm=std::array::from_fn(|_|vec![0.;5000]);
    let mut expected=std::array::from_fn(|_|vec![0.;5000]);
    assert_eq!(crate::plugin::tests::allocations(|| {
        for e in [&mut actual,&mut reference] {e.note_on(0,60,127);}
        render(&mut actual,&mut pcm,0,401,&[1,7,31,127]);
        render(&mut reference,&mut expected,0,401,&[128]);
        assert_eq!(actual.active_voices(),1);actual.cc(0,1,127);
        render(&mut actual,&mut pcm,401,5000,&[1,7,31,127]);
        render(&mut reference,&mut expected,401,5000,&[128]);
        assert_eq!(actual.active_voices(),0);assert_eq!(actual.unsupported_native_controls(),0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][1800]>0.05,"late marker must retain ordinary Sustain until sample EOF");
}
