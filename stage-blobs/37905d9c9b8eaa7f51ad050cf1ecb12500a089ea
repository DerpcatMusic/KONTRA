//! Reduced factory-script mono policy. This does not admit a native group flag,
//! Tone/Time source Legato, shared AHDSR Retrigger0, or the whole factory preset.
use super::*;
use crate::{audio::Sample, import::{Group, Zone}};

const FRAMES: usize = 2304;

fn engine(shared: bool, source: Option<&str>) -> Engine {
    // A changing source exposes both pitch and cursor restarts in complete PCM.
    let frames = (0..16_384).map(|n| {
        let x = 0.15 + 0.05 * (n as f32 * 0.017).sin() + n as f32 / 200_000.;
        [x; 2]
    }).collect();
    let bank = Bank::from_samples(vec![Group::default()],
        vec![Zone { root: 60, start_mod: Some(4096), ..Zone::default() }],
        vec![(Default::default(), Sample { rate: 48_000, frames })]).unwrap()
        .with_prepared_settings(|rows| {
            rows[0].envelope = Some(Ahdsr { release: 0.002, ..Ahdsr::UNITY });
        }).unwrap();
    let mut e = Engine::default(); e.reset(48_000.); e.set_lanes(shared);
    e.set_home_channel(7); e.set_bank(Some(Box::new(bank)));
    if let Some(source) = source {
        let (rt, errors) = Runtime::with_scripts(&[source],
            &mut crate::ksp::LogEngine::default(), 1, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        e.set_script(Some(Box::new(rt)));
    }
    e
}

fn script(mode: usize, priority: usize) -> String {
    // Authored reduction of the *algorithm*, not a copied factory source. The
    // fixture fixes humanization/unison/pedals off and uses an explicit1ms
    // stop fade. Native factory defaults and optional amplitude fades differ.
    format!(r#"on init
declare %held[128]
declare $mono
declare $current := -1
declare $started
declare $next
declare $best
declare $i
declare $offset
end on
on note
if ($MIDI_CHANNEL # 7)
change_pan($EVENT_ID,1000,0)
exit
end if
%held[$EVENT_NOTE] := $EVENT_ID
ignore_event($EVENT_ID)
$next := $EVENT_NOTE
if ({priority}=2)
$i := 0
while ($i<$EVENT_NOTE)
if (%held[$i]>0)
$next := $i
$i := 128
end if
inc($i)
end while
end if
if ({priority}=3)
$i := $EVENT_NOTE
while ($i<128)
if (%held[$i]>0)
$next := $i
end if
inc($i)
end while
end if
if ($next # $EVENT_NOTE)
exit
end if
if ($mono=0)
$started := $ENGINE_UPTIME
end if
if ({mode}=2 and $mono # 0)
change_tune($mono,($next-$current)*100000,1)
else
if ($mono # 0)
fade_out($mono,1000,1)
end if
$offset := 0
if ({mode}=3)
$offset := ($ENGINE_UPTIME-$started)*1000
end if
$mono := play_note($next,127,$offset,0)
change_pan($mono,-1000,0)
end if
$current := $next
end on
on release
if ($MIDI_CHANNEL # 7)
exit
end if
ignore_event($EVENT_ID)
if (%held[$EVENT_NOTE]=$EVENT_ID)
%held[$EVENT_NOTE] := 0
end if
if ($EVENT_NOTE # $current)
exit
end if
$next := -1
$best := 0
$i := 0
while ($i<128)
if (%held[$i]>0)
select ({priority})
case 0
if ($next=-1 or abs($i-$EVENT_NOTE)<abs($next-$EVENT_NOTE))
$next := $i
end if
case 1
if (%held[$i]>$best)
$best := %held[$i]
$next := $i
end if
case 2
if ($next=-1)
$next := $i
end if
case 3
$next := $i
end select
end if
inc($i)
end while
if ($next=-1)
fade_out($mono,1000,1)
$mono := 0
$current := -1
exit
end if
if ({mode}=2)
change_tune($mono,($next-$current)*100000,1)
else
fade_out($mono,1000,1)
$offset := 0
if ({mode}=3)
$offset := ($ENGINE_UPTIME-$started)*1000
end if
$mono := play_note($next,127,$offset,0)
change_pan($mono,-1000,0)
end if
$current := $next
end on"#)
}

fn owner(index: usize) -> HostNote {
    let delivered = [60,72,48,84,66][index];
    HostNote { port: 0, channel: [1,2,1,2,5][index],
        key: delivered-12, id: 950+index as i32, clap: true }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern { port: i32::from(n.port), channel: i32::from(n.channel),
        key: i32::from(n.key), id: n.id, clap: n.clap }
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>;2], from: usize, to: usize, parts: &[usize]) {
    let (mut at,mut part) = (from,0);
    while at<to {
        let n = parts[part%parts.len()].min(to-at);
        let [left,right] = pcm;
        e.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
        at += n; part += 1;
    }
    e.render(&mut [],&mut []);
}
fn same(a: &[Vec<f32>;2], b: &[Vec<f32>;2]) {
    assert!(a.iter().flatten().zip(b.iter().flatten()).all(|(a,b)|
        a.is_finite() && b.is_finite() && (a-b).abs()<2e-6),
        "complete mono/legato PCM disagrees with explicit pitch/cursor schedule");
}

fn record(mode: usize, priority: usize, shared: bool, parts: &[usize]) -> ([Vec<f32>;2],[u64;5]) {
    let source = script(mode,priority);
    let (mut actual,mut reference) = (engine(shared,Some(&source)),engine(false,None));
    let (mut pcm,mut expected) = (std::array::from_fn(|_|vec![0.;FRAMES]),
        std::array::from_fn(|_|vec![0.;FRAMES]));
    // Explicit exterior musical decisions: there is no held-key scan or
    // priority implementation in this reference. None means no audible action.
    let selections = match priority {
        0 => [Some(60),Some(72),Some(48),Some(84),Some(72),None,Some(60)],
        1 => [Some(60),Some(72),Some(48),Some(84),Some(48),Some(72),Some(60)],
        2 => [Some(60),None,Some(48),None,None,Some(60),None],
        _ => [Some(60),Some(72),None,Some(84),Some(72),None,Some(60)],
    };
    let mut mono: Option<(EventId,u8)> = None;
    let mut sibling = None;
    let mut owner_gain = [1.;5];
    let mut ends = [0;5];
    assert_eq!(crate::plugin::tests::allocations(|| {
        let mut from = 0;
        for (index,to) in [0,240,480,720,960,1200,1440,1680,FRAMES].into_iter().enumerate() {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut reference,&mut expected,from,to,&[128]); from = to;
            if index<4 {
                let n = owner(index);
                assert!(actual.host_note_on(n,7,n.key+12,127,0.));
                assert!(actual.input_key_down_from(n.channel,7,n.key+12));
            } else if index<8 {
                actual.host_note_off(pattern(owner(7-index)));
            }
            if index==0 {
                let n = owner(4);
                assert!(actual.host_note_on(n,9,n.key+12,127,0.));
                sibling = reference.start_event(&NoteEvent { pan:1.,
                    ..NoteEvent::new(9,66,127) });
            }
            if index==2 { actual.set_home_channel(9); }
            if index==4 {
                // The ignored B root is physically held but never takes over
                // the existing Legato event's exact expression ownership.
                actual.host_expression(pattern(owner(1)),HostExpression::Gain(0.));
                owner_gain[1] = 0.;
            }
            if index==5 {
                actual.host_expression(pattern(owner(0)),HostExpression::Gain(0.5));
                owner_gain[0] = 0.5;
                if mode==2 { reference.change_event(mono.unwrap().0,EventChange::Volume(0.5)); }
            }
            if index<7 && let Some(note) = selections[index] {
                if mode==2 && let Some((id,base)) = mono {
                    reference.change_event(id,EventChange::Tune(f64::from(i16::from(note)-i16::from(base))));
                } else {
                    if let Some((id,_)) = mono { reference.fade_event(id,0.001,0.,true); }
                    let offset_us = if mode==3 { (to/48*1000) as u64 } else {0};
                    // Retriggers inherit the callback's physical root, even
                    // when the chosen pitch belongs to another held key.
                    let origin = if index<4 {index} else {7-index};
                    let id = reference.start_event(&NoteEvent { pan:-1.,offset_us,volume:owner_gain[origin],
                        input_channel:Some(owner(origin).channel),
                        ..NoteEvent::new(7,note,127) }).unwrap();
                    mono = Some((id,note));
                }
            }
            if index==7 {
                reference.fade_event(mono.unwrap().0,0.001,0.,true);
                actual.host_note_off(pattern(owner(4)));
                reference.release_event(sibling.unwrap());
            }
            actual.mark_host_notes();
        }
        for (index,end) in ends.iter_mut().enumerate() {
            let n = owner(index);
            *end = actual.host_note_end_at(n).expect("exact physical owner completed");
            assert!(!actual.host_note_pending(n));
        }
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
        // Legato keeps the first sounding event's physical/exact origin. New
        // ignored roots never acquire that voice merely because pitch changes.
        if mode==2 {
            assert_eq!(ends[0],1728);
            assert!(ends[1]<=1680 && ends[2]<=1680 && ends[3]<=1680,
                "ignored roots acquired the retained Legato sample voice");
        }
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][128..240].iter().any(|v|v.abs()>0.1));
    assert!(pcm[1][128..1680].iter().any(|v|v.abs()>0.1));
    assert!(pcm.iter().all(|row|row[1920..].iter().all(|v|v.abs()<1e-8)));
    (pcm,ends)
}

#[test]
fn reduced_factory_mono_modes_priorities_and_original_legato_ownership_reach_full_pcm_without_heap() {
    for mode in 1..=3 { for priority in 0..4 {
        let (fixed,ends) = record(mode,priority,false,&[128]);
        for shared in [false,true] {
            let (fragmented,other) = record(mode,priority,shared,&[1,7,31,127,13,64]);
            same(&fixed,&fragmented); assert_eq!(ends,other);
        }
    }}
}

fn early_original_release(shared: bool, parts: &[usize]) -> ([Vec<f32>;2],[u64;3]) {
    let source = script(2,1);
    let (mut actual,mut reference) = (engine(shared,Some(&source)),engine(false,None));
    let (mut pcm,mut expected) = (std::array::from_fn(|_|vec![0.;FRAMES]),
        std::array::from_fn(|_|vec![0.;FRAMES]));
    let mut mono = None;
    let mut ends = [0;3];
    assert_eq!(crate::plugin::tests::allocations(|| {
        let mut from = 0;
        for to in [0,240,480,600,720,960,1200,FRAMES] {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut reference,&mut expected,from,to,&[128]); from = to;
            match to {
                0 | 240 | 480 => {
                    let index = to/240;
                    let n = owner(index);
                    assert!(actual.host_note_on(n,7,n.key+12,127,0.));
                    if index==0 {
                        mono = reference.start_event(&NoteEvent { pan:-1.,
                            ..NoteEvent::new(7,60,127) });
                    } else {
                        reference.change_event(mono.unwrap(),EventChange::Tune(if index==1 {12.} else {-12.}));
                    }
                }
                600 => {
                    actual.host_note_off(pattern(owner(0)));
                    assert!(!actual.input_key_down_from(owner(0).channel,7,60));
                    actual.mark_host_notes();
                    assert!(actual.host_note_pending(owner(0)),
                        "physical key-up retired the phrase's retained generated event");
                    assert!(actual.player.voices.iter().any(|v| v.input_channel==Some(1)
                        && v.host_note.and_then(|r| actual.player.host_notes.get(r))
                            .is_some_and(|o|o.note==owner(0))));
                }
                720 => {
                    // The already-released original host still owns its sample;
                    // expression on a newer ignored physical key cannot replace it.
                    actual.host_expression(pattern(owner(0)),HostExpression::Gain(0.5));
                    actual.host_expression(pattern(owner(1)),HostExpression::Gain(0.));
                    reference.change_event(mono.unwrap(),EventChange::Volume(0.5));
                    actual.set_home_channel(9);
                }
                960 => {
                    actual.host_note_off(pattern(owner(2)));
                    reference.change_event(mono.unwrap(),EventChange::Tune(12.));
                }
                1200 => {
                    // Independent source cursor:240*1 +240*2 +480*0.5 +240*2.
                    assert!((actual.player.voices[0].pos-1440.).abs()<1e-6);
                    actual.host_note_off(pattern(owner(1)));
                    reference.fade_event(mono.unwrap(),0.001,0.,true);
                }
                _ => {}
            }
            actual.mark_host_notes();
        }
        for (index,end) in ends.iter_mut().enumerate() {
            *end = actual.host_note_end_at(owner(index)).expect("completed original/ignored root");
            assert!(!actual.host_note_pending(owner(index)));
        }
        assert_eq!(ends[0],1248);
        assert_eq!(actual.active_voices(),0); assert_eq!(actual.dropped_commands(),0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][720..1200].iter().any(|v|v.abs()>0.05));
    assert!(pcm.iter().all(|row|row[1440..].iter().all(|v|v.abs()<1e-8)));
    (pcm,ends)
}

#[test]
fn reduced_factory_legato_survives_original_physical_key_up_with_exact_owner_expression_in_complete_pcm() {
    let (fixed,ends) = early_original_release(false,&[128]);
    for shared in [false,true] {
        let (fragmented,other) = early_original_release(shared,&[1,7,31,127,13,64]);
        same(&fixed,&fragmented); assert_eq!(ends,other);
    }
}

#[test]
fn ignored_release_keeps_sounding_owner_until_sample_end_without_heap() {
    let source = "on init\nend on\non release\nignore_event($EVENT_ID)\nend on";
    let frames = 16_384 + 64;
    for parts in [&[128][..], &[1, 7, 31, 127, 13, 64][..]] {
        let (mut actual, mut reference) = (engine(false, Some(source)), engine(false, None));
        let (mut pcm, mut expected) = (
            std::array::from_fn(|_| vec![0.; frames]),
            std::array::from_fn(|_| vec![0.; frames]),
        );
        let n = owner(0);
        assert_eq!(crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut reference] {
                assert!(e.host_note_on(n, 7, n.key + 12, 127, 0.));
            }
            render(&mut actual, &mut pcm, 0, 128, parts);
            render(&mut reference, &mut expected, 0, 128, &[128]);
            actual.host_note_off(pattern(n));
            // The independent oracle stays held: ignoring release must keep
            // the original source clock and envelope, not start a new note.
            render(&mut actual, &mut pcm, 128, FRAMES, parts);
            render(&mut reference, &mut expected, 128, FRAMES, &[128]);
            actual.mark_host_notes();
            assert!(!actual.input_key_down_from(n.channel, 7, n.key + 12));
            assert!(actual.host_note_pending(n));
            assert_eq!(actual.host_note_end_at(n), None);
            assert_eq!(actual.active_voices(), 1);

            render(&mut actual, &mut pcm, FRAMES, frames, parts);
            render(&mut reference, &mut expected, FRAMES, frames, &[128]);
            actual.mark_host_notes();
            assert_eq!(actual.host_note_end_at(n), Some(16_384));
            assert!(!actual.host_note_pending(n));
            assert_eq!(actual.active_voices(), 0);
            assert!(actual.script().unwrap().diagnostics().is_empty());
        }), 0);
        same(&pcm, &expected);
        assert!(pcm[0][128..FRAMES].iter().any(|v| v.abs() > 0.1));
        assert!(pcm.iter().all(|row| row[16_384..].iter().all(|v| v.abs() < 1e-8)));
    }
}

fn finite_alias_engines(source: &str) -> (Engine, Engine) {
    use crate::import::Instrument;
    use ni_file::kontakt::objects::ProgramDataPrivateParams;
    const SOURCE: usize = 64;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("finite-release-alias.wav");
    let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
        channels: 2, sample_rate: 48_000, bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }).unwrap();
    for frame in 0..SOURCE {
        for value in [0.2 + frame as f32 / 1000., -0.3 + frame as f32 / 2000.] {
            wav.write_sample(value).unwrap();
        }
    }
    wav.finalize().unwrap();
    let instrument = Instrument {
        groups: vec![Group::default()],
        zones: vec![Zone { sample: path, root: 60, ..Zone::default() }],
        native_program_private: Some(ProgramDataPrivateParams {
            version: 0x80,
            data: [0u32.to_le_bytes().as_slice(), &[1, 1],
                0i32.to_le_bytes().as_slice(), &[0xa5, 0, 0xff]].concat(),
        }),
        scripts: vec![source.into()], ..Default::default()
    };
    let make_engine = |scripted| {
        let bank = Bank::load(&instrument).unwrap().with_prepared_settings(|rows| {
            // The forwarded-release control also reaches the finite source
            // end; its independent oracle applies the same envelope release.
            rows[0].envelope = Some(Ahdsr { release: 1., ..Ahdsr::UNITY });
        }).unwrap();
        assert_eq!(bank.initial_state.as_ref().unwrap().controller_policy,
            super::instrument::ControllerPolicy::Native);
        let mut e = Engine::default();
        e.reset(48_000.); e.attack = 0.; e.set_home_channel(7);
        e.set_bank(Some(Box::new(bank)));
        if scripted {
            let (runtime, errors) = super::script::load_scripts(&instrument, Vec::new(), 48_000.);
            assert!(errors.is_empty(), "{errors:?}");
            e.set_script(runtime);
        }
        e
    };
    (make_engine(true), make_engine(false))
}

fn finite_release_alias_cycles(ignore_release: bool) {
    const SOURCE: usize = 64;
    const CYCLE: usize = 96;
    const CYCLES: usize = crate::ksp::EVENT_CAPACITY + 1;
    let source = format!("on init\ndeclare $id\nend on\non note\n$id := $EVENT_ID\nend on\non release\n{}\nend on\non controller\nmessage(event_status($id))\nend on",
        if ignore_release { "ignore_event($EVENT_ID)" } else { "" });
    for parts in [&[128][..], &[1, 7, 31, 13][..]] {
        let (mut actual, mut reference) = finite_alias_engines(&source);
        let (mut pcm, mut expected) = (
            std::array::from_fn(|_| vec![0.; CYCLE * CYCLES]),
            std::array::from_fn(|_| vec![0.; CYCLE * CYCLES]),
        );
        let mut ignored_eof_active = 0;
        assert_eq!(crate::plugin::tests::allocations(|| {
            for cycle in 0..CYCLES {
                let at = cycle * CYCLE;
                let n = HostNote { id: 20_000 + cycle as i32, ..owner(0) };
                for e in [&mut actual, &mut reference] {
                    assert!(e.host_note_on(n, 7, 60, 127, 0.),
                        "host admission failed at cycle {cycle}");
                }
                render(&mut actual, &mut pcm, at, at + 8, parts);
                render(&mut reference, &mut expected, at, at + 8, &[128]);
                assert_eq!(actual.active_voices(), 1,
                    "finite alias admission failed at cycle {cycle}; ignored EOF statuses previously active: {ignored_eof_active}");
                actual.cc(7, 20, 1);
                assert_eq!(actual.script().unwrap().last_message(), "1");
                actual.host_note_off(pattern(n));
                if !ignore_release { reference.host_note_off(pattern(n)); }
                assert!(!actual.input_key_down_from(n.channel, 7, 60));
                actual.cc(7, 20, 1);
                assert_eq!(actual.script().unwrap().last_message(), "1");
                render(&mut actual, &mut pcm, at + 8, at + SOURCE, parts);
                render(&mut reference, &mut expected, at + 8, at + SOURCE, &[128]);
                if ignore_release { reference.host_note_off(pattern(n)); }
                render(&mut actual, &mut pcm, at + SOURCE, at + CYCLE, parts);
                render(&mut reference, &mut expected, at + SOURCE, at + CYCLE, &[128]);
                for e in [&mut actual, &mut reference] {
                    e.mark_host_notes();
                    assert_eq!(e.active_voices(), 0, "voice survived finite EOF at cycle {cycle}");
                    assert_eq!(e.host_note_end_at(n), Some((at + SOURCE) as u64));
                    assert!(!e.host_note_pending(n));
                    e.retire_host_note(n);
                    assert_eq!(e.host_note_end_at(n), None);
                }
                actual.cc(7, 20, 1);
                let status = actual.script().unwrap().last_message();
                if ignore_release {
                    // Record the UntilNoteOff status without deciding native
                    // post-EOF addressability before the pool-reuse probe.
                    assert!(matches!(status, "0" | "1"));
                    ignored_eof_active += usize::from(status == "1");
                } else {
                    assert_eq!(status, "0");
                }
                assert_eq!(actual.script().unwrap().now(), (at + CYCLE) as u64);
                assert!(actual.script().unwrap().faults().next().is_none(),
                    "KSP fault at cycle {cycle}");
                for (a, b) in pcm.iter().zip(&expected) {
                    assert!(a[at..at + CYCLE].iter().zip(&b[at..at + CYCLE]).all(|(a,b)|
                        a.is_finite() && b.is_finite() && (a-b).abs()<2e-6),
                        "complete finite PCM disagrees at cycle {cycle}");
                    assert!(a[at + SOURCE..at + CYCLE].iter().all(|v| v.abs()<1e-8));
                }
                assert!(pcm[0][at..at + SOURCE].iter().any(|v| v.abs()>0.1));
            }
        }), 0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
        same(&pcm, &expected);
    }
}

#[test]
fn finite_ignored_release_aliases_reuse_event_pool_after_exact_owner_retirement_without_heap() {
    finite_release_alias_cycles(true);
}

#[test]
fn finite_forwarded_release_aliases_reuse_event_pool_after_exact_owner_retirement_without_heap() {
    finite_release_alias_cycles(false);
}

#[test]
fn finite_alias_pressure_preserves_held_and_waiting_ignored_release_owners_without_heap() {
    const CYCLE: usize = 96;
    const CYCLES: usize = crate::ksp::EVENT_CAPACITY + 1;
    const RESUME: usize = 480_008;
    let source = "on init\ndeclare %ids[128]\ndeclare $done\nend on\non note\n%ids[$EVENT_VELOCITY] := $EVENT_ID\nend on\non release\nignore_event($EVENT_ID)\nif ($EVENT_VELOCITY=126)\nwait(10000000)\n$done := 1\nend if\nend on\non controller\nif ($CC_NUM=20)\nmessage(event_status(%ids[%CC[20]]))\nelse\nmessage($done)\nend if\nend on";
    for parts in [&[128][..], &[1, 7, 31, 13][..]] {
        let (mut actual, mut reference) = finite_alias_engines(source);
        let (mut pcm, mut expected) = (
            std::array::from_fn(|_| vec![0.; RESUME + CYCLE]),
            std::array::from_fn(|_| vec![0.; RESUME + CYCLE]),
        );
        let held = HostNote { id: 10_000, ..owner(0) };
        let waiting = HostNote { key: 49, id: 10_001, ..owner(1) };
        assert_eq!(crate::plugin::tests::allocations(|| {
            for e in [&mut actual, &mut reference] {
                assert!(e.host_note_on(held, 7, 60, 127, 0.));
                assert!(e.host_note_on(waiting, 7, 60, 126, 0.));
            }
            render(&mut actual, &mut pcm, 0, 8, parts);
            render(&mut reference, &mut expected, 0, 8, &[128]);
            actual.host_note_off(pattern(waiting));
            render(&mut actual, &mut pcm, 8, 64, parts);
            render(&mut reference, &mut expected, 8, 64, &[128]);
            reference.host_note_off(pattern(waiting));
            render(&mut actual, &mut pcm, 64, CYCLE, parts);
            render(&mut reference, &mut expected, 64, CYCLE, &[128]);
            for cycle in 0..CYCLES {
                let at = (cycle + 1) * CYCLE;
                let n = HostNote { key: 50, id: 30_000 + cycle as i32, ..owner(0) };
                for e in [&mut actual, &mut reference] {
                    assert!(e.host_note_on(n, 7, 60, 125, 0.));
                }
                render(&mut actual, &mut pcm, at, at + 8, parts);
                render(&mut reference, &mut expected, at, at + 8, &[128]);
                assert_eq!(actual.active_voices(), 1, "pool pressure at cycle {cycle}");
                actual.host_note_off(pattern(n));
                render(&mut actual, &mut pcm, at + 8, at + 64, parts);
                render(&mut reference, &mut expected, at + 8, at + 64, &[128]);
                reference.host_note_off(pattern(n));
                render(&mut actual, &mut pcm, at + 64, at + CYCLE, parts);
                render(&mut reference, &mut expected, at + 64, at + CYCLE, &[128]);
                for e in [&mut actual, &mut reference] {
                    e.mark_host_notes();
                    assert_eq!(e.active_voices(), 0);
                    assert_eq!(e.host_note_end_at(n), Some((at + 64) as u64));
                    assert!(!e.host_note_pending(n));
                    e.retire_host_note(n);
                    assert_eq!(e.host_note_end_at(n), None);
                }
                assert!(actual.host_note_pending(held));
                assert!(actual.host_note_pending(waiting));
                assert_eq!(actual.host_note_end_at(held), None);
                assert_eq!(actual.host_note_end_at(waiting), None);
                for velocity in [127, 126] {
                    actual.cc(7, 20, velocity);
                    assert_eq!(actual.script().unwrap().last_message(), "1",
                        "retained event ID was recycled at cycle {cycle}");
                }
                assert!(actual.script().unwrap().faults().next().is_none());
            }
            let at = (CYCLES + 1) * CYCLE;
            assert!(actual.input_key_down_from(held.channel, 7, 60));
            assert!(!actual.input_key_down_from(waiting.channel, 7, 60));
            actual.cc(7, 21, 1);
            assert_eq!(actual.script().unwrap().last_message(), "0");
            for e in [&mut actual, &mut reference] { e.host_note_off(pattern(held)); }
            render(&mut actual, &mut pcm, at, RESUME - 1, parts);
            render(&mut reference, &mut expected, at, RESUME - 1, &[128]);
            actual.cc(7, 21, 1);
            assert_eq!(actual.script().unwrap().last_message(), "0");
            actual.mark_host_notes();
            assert_eq!(actual.host_note_end_at(held), Some(at as u64));
            assert!(actual.host_note_pending(waiting));
            render(&mut actual, &mut pcm, RESUME - 1, RESUME + CYCLE, parts);
            render(&mut reference, &mut expected, RESUME - 1, RESUME + CYCLE, &[128]);
            actual.cc(7, 21, 1);
            assert_eq!(actual.script().unwrap().last_message(), "1");
            actual.mark_host_notes();
            assert_eq!(actual.host_note_end_at(waiting), Some(RESUME as u64));
            assert!(!actual.host_note_pending(waiting));
            assert_eq!(actual.script().unwrap().now(), (RESUME + CYCLE) as u64);
        }), 0);
        same(&pcm, &expected);
        assert!(pcm.iter().all(|row| row[(CYCLES + 1) * CYCLE..].iter().all(|v| v.abs()<1e-8)));
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }
}
