//! Complete PCM proof of the existing release Note Mono receiver policy.
//! Native field identity is proved; physical scope, mask timing and fades are
//! local policies, not measurements of Kontakt's corresponding execution.

use super::*;
use crate::{audio::Sample, import::{Group, Loop, Zone}};

const FRAMES: usize = 2816;

fn note(id: i32) -> HostNote {
    HostNote { port: 0, channel: if id == 2 { 2 } else { 1 },
        key: if id == 4 { 61 } else { 60 }, id, clap: true }
}

fn pattern(id: i32) -> HostPattern {
    let n = note(id);
    HostPattern { port: i32::from(n.port), channel: i32::from(n.channel),
        key: i32::from(n.key), id, clap: true }
}

fn engine(mono: bool, shared: bool, source: Option<&str>) -> Engine {
    // Carry the switch through the actual native public group decoder and
    // cache path, rather than granting admission from an unrelated local flag.
    use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};
    let mut public = 0u32.to_le_bytes().to_vec(); // Empty UTF16 name.
    for value in [1.0f32, -1., 1.] { public.extend(value.to_le_bytes()); }
    public.extend([0, 0, 1, u8::from(mono)]);
    public.extend(0i32.to_le_bytes()); public.extend((-1i16).to_le_bytes());
    public.extend((-1i32).to_le_bytes()); public.extend(6i32.to_le_bytes());
    public.extend([0, 0]); public.extend(0i32.to_le_bytes());
    let mut criteria = Vec::new();
    ni_file::kontakt::objects::StartCriteriaList::default().write(&mut criteria).unwrap();
    let raw = RawGroup(StructuredObject { version: 0x95, private_data: Vec::new(),
        public_data: public, children: vec![Chunk { id: 0x38, data: criteria }] });
    let decoded = raw.params().unwrap();
    assert!(decoded.release_trigger);
    assert_eq!(decoded.release_trigger_note_monophonic, mono);
    let imported = Group { release_trigger: decoded.release_trigger,
        release_trigger_note_monophonic: decoded.release_trigger_note_monophonic,
        key_tracking: false, pan: decoded.pan, ..Group::default() };
    let json = serde_json::to_value(&imported).unwrap();
    let mut legacy = json.clone();
    legacy.as_object_mut().unwrap().remove("release_trigger_note_monophonic");
    assert!(!serde_json::from_value::<Group>(legacy).unwrap().release_trigger_note_monophonic);
    let imported: Group = serde_json::from_value(json).unwrap();
    let groups = vec![Group { muted: true, ..Group::default() }, imported,
        Group { release_trigger: true, key_tracking: false, gain: 0.5, pan: 1., ..Group::default() }];
    let zones = (0..3).map(|group| Zone { group, loop_range: Some(Loop {
        start: 0, end: 64, crossfade: 0, alternating: false, until_release: false }),
        ..Zone::default() }).collect();
    let bank = Bank::from_samples(groups, zones, vec![(Default::default(),
        Sample { rate: 48_000, frames: vec![[0.25; 2]; 4096] })]).unwrap()
        .with_prepared_settings(|rows| {
            for row in rows { row.envelope = Some(Ahdsr::UNITY); }
        }).unwrap();
    let mut e = Engine::default(); e.reset(48_000.); e.set_lanes(shared);
    e.set_bank(Some(Box::new(bank)));
    if let Some(source) = source {
        let (runtime, errors) = Runtime::with_scripts(&[source],
            &mut crate::ksp::LogEngine::default(), 3, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        e.set_script(Some(Box::new(runtime)));
    }
    e
}

fn on(e: &mut Engine, id: i32) {
    let n = note(id);
    assert!(e.host_note_on(n, if id == 3 { 8 } else { 7 }, n.key, 127, 0.));
}

fn off(e: &mut Engine, id: i32) { e.host_note_off(pattern(id)); }

fn reference_cut(e: &mut Engine, ids: &[i32]) {
    // Explicit fixture roots and the left release group identify the oracle;
    // no delivered-key/channel predicate from Player::start is reproduced.
    let mut found = 0;
    for voice in &mut e.player.voices {
        if voice.group == 1 && voice.release_trigger && voice.host_note
            .and_then(|r| e.player.host_notes.get(r)).is_some_and(|o| ids.contains(&o.note.id)) {
            voice.fade.start(0., 48, true);
            found += 1;
        }
    }
    assert_eq!(found, ids.len());
}

fn compare(actual: &[f32], reference: &[f32]) {
    assert!(actual.iter().zip(reference).all(|(a, b)| a.is_finite() && (a-b).abs() < 2e-6),
        "complete release Note Mono PCM disagrees with explicit owner fades");
}

fn record(shared: bool, parts: &[usize], manual_duration: Option<i32>) -> ([Vec<f32>; 2], [u64; 8]) {
    let script = manual_duration.map(|duration| format!(
        "on init\nSET_CONDITION(NO_SYS_SCRIPT_RLS_TRIG)\nend on\non note\ndisallow_group($ALL_GROUPS)\nallow_group(0)\nend on\non release\ndisallow_group($ALL_GROUPS)\nallow_group(1)\nallow_group(2)\nplay_note($EVENT_NOTE,$EVENT_VELOCITY,0,{duration})\nend on"));
    let (mut actual, mut reference) = (engine(true, shared, script.as_deref()),
        engine(false, false, script.as_deref()));
    let (mut left, mut right, mut oracle_left, mut oracle_right) = (
        vec![0.; FRAMES], vec![0.; FRAMES], vec![0.; FRAMES], vec![0.; FRAMES]);
    let mut ends = [0; 8];
    let (mut frame, mut chunk) = (0, 0);
    let manual = manual_duration.is_some();
    assert_eq!(crate::plugin::tests::allocations(|| {
        let boundaries = if manual { &[0,96,256,384,768,FRAMES][..] }
            else { &[0,96,256,384,512,640,768,896,1024,1152,1280,1408,1536,
                1664,1792,1920,2048,2176,2304,2432,FRAMES][..] };
        for &stop in boundaries {
            let before = frame;
            while frame < stop {
                let n = parts[chunk % parts.len()].min(stop-frame);
                actual.render(&mut left[frame..frame+n], &mut right[frame..frame+n]);
                frame += n; chunk += 1;
            }
            let mut at = before;
            while at < stop {
                let n = MAX_BLOCK.min(stop-at);
                reference.render(&mut oracle_left[at..at+n], &mut oracle_right[at..at+n]); at += n;
            }
            for e in [&mut actual, &mut reference] {
                e.render(&mut [], &mut []);
                if manual {
                    match stop {
                        0 => on(e,1), 96 => off(e,1),
                        256 => on(e,5), 384 => off(e,5),
                        768 => e.all_sound_off_from(7,u16::MAX), _ => {}
                    }
                } else {
                    match stop {
                        0 => on(e,1), 96 => off(e,1), 256 => on(e,2), 384 => off(e,2),
                        512 => on(e,3), 640 => off(e,3), 768 => on(e,4), 896 => off(e,4),
                        1024 => on(e,5), 1152 => off(e,5),
                        1280 => e.cc_from(7,1,64,127), 1408 => on(e,6), 1536 => off(e,6),
                        1664 => on(e,7), 1792 => off(e,7), 1920 => e.cc_from(7,1,64,0),
                        2048 => { e.set_group_allowed(1,false); e.set_group_allowed(2,false); on(e,8); },
                        2176 => { e.set_group_allowed(2,true); off(e,8); },
                        2304 => off(e,8), 2432 => e.all_sound_off_from(7,u16::MAX), _ => {}
                    }
                }
            }
            match stop {
                256 if manual => reference_cut(&mut reference,&[1]),
                1024 if !manual => reference_cut(&mut reference,&[1]),
                1408 if !manual => reference_cut(&mut reference,&[5]),
                2048 if !manual => reference_cut(&mut reference,&[6,7]), _ => {}
            }
        }
        actual.mark_host_notes(); reference.mark_host_notes();
        for id in 1..=8 {
            if manual && id != 1 && id != 5 { continue; }
            let n = note(id);
            assert!(!actual.host_note_pending(n));
            ends[(id-1) as usize] = actual.host_note_end_at(n).expect("completed exact root");
            assert_eq!(Some(ends[(id-1) as usize]),reference.host_note_end_at(n));
            assert_eq!(ends[(id-1) as usize], if manual { 1008 } else { 2672 });
        }
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        if let Some(rt) = actual.script() { assert!(rt.diagnostics().is_empty()); }
    }),0);
    compare(&left,&oracle_left); compare(&right,&oracle_right);
    // Independent constant-source oracle for EVERY output sample. Each tuple
    // names an expected source onset and its explicit mono-tail stop; none
    // derives event eligibility by examining production voice state.
    let final_stop = if manual { 768 } else { 2432 };
    let tails = if manual { &[(96,Some(256)),(384,None)][..] } else {
        &[(96,Some(1024)),(384,None),(640,None),(896,None),
            (1152,Some(1408)),(1920,Some(2048)),(1920,Some(2048)),(2176,None)][..]
    };
    let fade = |frame:usize,start:usize,length:usize| {
        if frame < start { 1. } else { (1.-(frame-start+1) as f32/length as f32).max(0.) }
    };
    for frame in 0..FRAMES {
        let mut expected = [0.;2];
        for &(start, cut) in tails {
            if frame < start { continue; }
            // The final masked release admits only the right sibling group.
            if manual || start != 2176 {
                expected[0] += 0.25*cut.map_or(1.,|at|fade(frame,at,48));
            }
            expected[1] += 0.125;
        }
        for (channel, pcm) in [&left,&right].into_iter().enumerate() {
            expected[channel] *= fade(frame,final_stop,240);
            assert!((pcm[frame]-expected[channel]).abs()<2e-6,
                "frame{frame} channel{channel}: constant-source oracle {} != {}",pcm[frame],expected[channel]);
        }
    }
    let silent = if manual { 1008 } else { 2672 };
    assert!(left[silent..].iter().chain(&right[silent..]).all(|v| *v==0.));
    ([left,right],ends)
}

#[test]
fn saved_release_note_mono_preserves_scope_masks_pedals_and_complete_pcm_without_heap() {
    for duration in [None,Some(-1),Some(0),Some(5000)] {
        let (baseline,ends) = record(false,&[128],duration);
        for shared in [false,true] {
            let (pcm,fragmented_ends) = record(shared,&[1,7,31,127,13,64],duration);
            compare(&pcm[0],&baseline[0]); compare(&pcm[1],&baseline[1]);
            assert_eq!(ends,fragmented_ends);
        }
    }
}
