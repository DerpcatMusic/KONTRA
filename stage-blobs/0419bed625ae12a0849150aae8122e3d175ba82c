//! Runtime IDs retain real audio aliases, rather than pretending metadata is a voice.
use super::{Bank, Engine, EventChange, HostNote, HostPattern, NoteEvent, envelope::Ahdsr};
use crate::{audio::Sample, import::{Group, Zone}, ksp::{LogEngine, Runtime}};
use std::path::PathBuf;

fn engine(sources: &[(u8, usize, bool)], release: f32, private_release: bool, script: Option<&str>) -> Box<Engine> {
    let mut groups = vec![Group::default()];
    let mut zones = Vec::new(); let mut samples = Vec::new();
    let frames = |len, silent| (0..len).map(|i| if silent {[0.;2]} else {
        let phase = std::f32::consts::TAU * (i % 71) as f32 / 71.;
        [0.11 + 0.07*phase.sin(), -0.09 + 0.03*phase.cos()]
    }).collect();
    for &(key, len, silent) in sources {
        let path = PathBuf::from(format!("lifetime-{key}"));
        zones.push(Zone {root:key, low_key:key, high_key:key, sample:path.clone(), ..Default::default()});
        samples.push((path, Sample {rate:48_000, frames:frames(len,silent)}));
    }
    if private_release {
        groups.push(Group {release_trigger:true, ..Default::default()});
        let path = PathBuf::from("independent private release");
        zones.push(Zone {group:1,root:60,low_key:60,high_key:60,sample:path.clone(),..Default::default()});
        samples.push((path,Sample {rate:48_000,frames:frames(512,false)}));
    }
    let bank = Bank::from_samples(groups,zones,samples).unwrap().with_prepared_settings(|rows| {
        rows[0].envelope = Some(Ahdsr { release, ..Ahdsr::UNITY });
        if private_release {rows[1].envelope=Some(Ahdsr::UNITY);}
    }).unwrap();
    let mut e = Box::new(Engine::default()); e.reset(48_000.); e.attack = 0.; e.set_lanes(false);
    e.set_bank(Some(Box::new(bank)));
    if let Some(script) = script {
        let (rt, errors) = Runtime::with_scripts(&[script], &mut LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}"); e.set_script(Some(Box::new(rt)));
    }
    e
}
fn owner(key: u8, id: i32) -> HostNote { HostNote {port:0, channel:4, key, id, clap:true} }
fn off(e: &mut Engine, n: HostNote) {
    e.host_note_off(HostPattern {port:i32::from(n.port), channel:i32::from(n.channel), key:i32::from(n.key), id:n.id, clap:n.clap});
}
fn render(e: &mut Engine, pcm: &mut [Vec<f32>;2], from: usize, to: usize, parts: &[usize]) {
    let (mut at, mut part) = (from, 0);
    while at < to {
        let n = parts[part % parts.len()].min(to-at); let [l,r] = pcm;
        e.render_checked(&mut l[at..at+n], &mut r[at..at+n]).unwrap();
        e.render_checked(&mut [], &mut []).unwrap(); at += n; part += 1;
    }
    e.mark_host_notes();
}
fn same(a: &[Vec<f32>;2], b: &[Vec<f32>;2]) {
    for ch in 0..2 { for (frame,(a,b)) in a[ch].iter().zip(&b[ch]).enumerate() {
        assert!((a-b).abs()<4e-6, "channel{ch} frame{frame}: {a} vs {b}");
    }}
}
fn clean(e: &Engine) {
    assert_eq!(e.active_voices(),0); assert_eq!(e.underruns(),0); assert_eq!(e.dropped_commands(),0);
    assert_eq!(e.unsupported_native_controls(),0); assert!(e.script().unwrap().diagnostics().is_empty());
    assert!(e.script().unwrap().faults().next().is_none());
}

fn released_tail(parts: &[usize]) -> [Vec<f32>;2] {
    const FRAMES: usize = 2048;
    let script = r#"on init
declare $old
declare $new
declare $waiting
declare %ids[8]
end on
on note
set_event_par($EVENT_ID,$EVENT_PAR_0,73)
set_event_par_arr($EVENT_ID,$EVENT_PAR_CUSTOM,79,7)
set_event_mark($EVENT_ID,1)
if ($EVENT_NOTE=60)
$old := $EVENT_ID
end if
if ($EVENT_NOTE=61)
$new := $EVENT_ID
end if
if ($EVENT_NOTE=62)
$waiting := $EVENT_ID
wait(10000)
end if
end on
on release
if ($EVENT_NOTE=60)
ignore_event($EVENT_ID)
message("release waiting")
wait(1000)
note_off($EVENT_ID)
end if
end on
on controller
if ($CC_NUM=1)
message(event_status($old)*10+event_status($waiting))
if (event_status($old)=1 and get_event_par($old,$EVENT_PAR_NOTE)=60 and get_event_par($old,$EVENT_PAR_0)=73 and get_event_par_arr($old,$EVENT_PAR_CUSTOM,7)=79 and get_event_par_arr($old,$EVENT_PAR_ALLOW_GROUP,0)=1 and get_event_mark($old,1)=1)
change_vol($old,-6021,0)
change_pan($old,-500,0)
end if
end if
if ($CC_NUM=2)
message(event_status($old)*100+event_status($new)*10+event_status($waiting))
note_off($old)
change_vol($old,-12000,0)
end if
if ($CC_NUM=3)
get_event_ids(%ids)
message((%ids[0]=$new)*10+(%ids[1]=0))
end if
if ($CC_NUM=4)
message(event_status($new))
end if
if ($CC_NUM=5)
message($PLAYED_VOICES_INST)
end if
if ($CC_NUM=6)
set_event_mark($old,2)
delete_event_mark($old,1)
message(get_event_par($old,$EVENT_PAR_NOTE)=0 and get_event_par($old,$EVENT_PAR_0)=0 and get_event_par($old,$EVENT_PAR_VOLUME)=0 and get_event_par($old,$EVENT_PAR_ZONE_ID)=0 and get_event_par($old,$EVENT_PAR_PLAY_POS)=0 and get_event_par_arr($old,$EVENT_PAR_CUSTOM,7)=0 and get_event_par_arr($old,$EVENT_PAR_ALLOW_GROUP,0)=0 and get_event_mark($old,1)=0 and get_event_mark($old,2)=0)
end if
if ($CC_NUM=7)
message(get_event_par($waiting,$EVENT_PAR_NOTE)=62 and get_event_par_arr($waiting,$EVENT_PAR_CUSTOM,7)=79 and get_event_mark($waiting,1)=1)
end if
end on"#;
    let sources = [(60,1024,false), (61,1024,false), (62,64,false)];
    let (mut actual, mut reference) = (engine(&sources,0.05,true,Some(script)), engine(&sources,0.05,true,None));
    let mut pcm = std::array::from_fn(|_|vec![0.;FRAMES]);
    let mut expected = std::array::from_fn(|_|vec![0.;FRAMES]);
    let (old, new, waiting) = (owner(60,410), owner(61,411), owner(62,412));
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(actual.host_note_on(old,7,60,127,0.));
        assert!(actual.host_note_on(waiting,7,62,127,0.));
        actual.cc_from(7,4,5,127);
        assert_eq!(actual.script().unwrap().last_message(),"0","queued Start ownership is not an admitted sample voice");
        let dry_old = reference.start_event(&NoteEvent::new(7,60,127)).unwrap();
        reference.start_event(&NoteEvent::new(7,62,127)).unwrap();
        let mut from = 0;
        for to in [128,176,256,384,512,1100,1450,1536,1537,FRAMES] {
            render(&mut actual,&mut pcm,from,to,parts);
            render(&mut reference,&mut expected,from,to,&[128]); from=to;
            match to {
                128 => {off(&mut actual,old);off(&mut actual,waiting);
                    assert_eq!(actual.script().unwrap().last_message(),"release waiting");},
                176 => reference.release_event(dry_old),
                256 => {actual.cc_from(7,4,5,127);
                    assert_eq!(actual.script().unwrap().last_message(),"2","parent tail plus private release child; EOF callback owns no sample voice");
                    actual.cc_from(7,4,7,127);
                    assert_eq!(actual.script().unwrap().last_message(),"1","EOF callback retains direct metadata membership");
                    actual.cc_from(7,4,1,127);
                    assert_eq!(actual.script().unwrap().last_message(),"11",
                        "a released audio tail and an EOF-only waiting callback both own their aliases");
                    reference.change_event(dry_old,EventChange::Volume(10f32.powf(-6021./20_000.)));
                    reference.change_event(dry_old,EventChange::Pan(-0.5));},
                384 => {assert!(actual.host_note_on(new,7,61,127,0.));
                    reference.start_event(&NoteEvent::new(7,61,127)).unwrap();},
                512 => {actual.cc_from(7,4,5,127);
                    assert_eq!(actual.script().unwrap().last_message(),"3","two attacks and the private release child are actual sample voices");
                    actual.cc_from(7,4,7,127);
                    assert_eq!(actual.script().unwrap().last_message(),"0","finished callback has no direct getter membership");
                    actual.cc_from(7,4,1,127);
                    assert_eq!(actual.script().unwrap().last_message(),"10","the expired callback no longer owns its silent alias");},
                1100 | 1450 => {actual.cc_from(7,4,5,127);
                    assert_eq!(actual.script().unwrap().last_message(),if to==1100 {"1"} else {"0"},
                        "held metadata and released terminal rows do not inflate actual voice count");
                    actual.cc_from(7,4,6,127);
                    assert_eq!(actual.script().unwrap().last_message(),"1","all direct getters/marks reject retained terminal metadata");
                    actual.cc_from(7,4,2,127);
                    assert_eq!(actual.script().unwrap().last_message(),"10",
                        "terminal old source is inactive; ordinary new root stays held after its own attack EOF");},
                1536 => {actual.cc_from(7,4,3,127);
                    assert_eq!(actual.script().unwrap().last_message(),"11","get_event_ids excludes retained terminal aliases"); off(&mut actual,new);},
                1537 => {actual.cc_from(7,4,4,127);
                    assert_eq!(actual.script().unwrap().last_message(),"0","consumed terminal Off removes logical held ownership");},
                _ => {},
            }
        }
        clean(&actual);
        assert_eq!(actual.host_note_end_at(old),Some(1024));
        assert_eq!(actual.host_note_end_at(waiting),Some(480));
        assert_eq!(actual.host_note_end_at(new),Some(1536));
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][300].abs()>0.01 && pcm[0][900].abs()>0.01,"post-Off edits must affect a genuinely audible tail");
    assert!(pcm.iter().all(|ch|ch[1408..].iter().all(|x|*x==0.)));
    pcm
}

fn source_pool(parts: &[usize]) -> [Vec<f32>;2] {
    const LONG: usize = 32768;
    const FRAMES: usize = LONG+1024;
    let script = r#"on init
declare $first
declare $last
declare %ids[8]
end on
on note
ignore_event($EVENT_ID)
$last := play_note($EVENT_NOTE,127,0,0)
set_event_par($last,$EVENT_PAR_0,73)
set_event_par_arr($last,$EVENT_PAR_CUSTOM,79,7)
set_event_mark($last,1)
if ($EVENT_NOTE=60)
$first := $last
end if
end on
on controller
if ($CC_NUM=1)
message(event_status($first))
if (event_status($first)=1)
change_vol($first,-6021,0)
end if
end if
if ($CC_NUM=2)
message(event_status($first))
note_off($first)
change_vol($first,-12000,0)
end if
if ($CC_NUM=3)
get_event_ids(%ids)
message(event_status($first)*100+event_status($last)*10+(%ids[3]=0))
if (event_status($first)=0 and get_event_par($first,$EVENT_PAR_NOTE)=0 and get_event_par($first,$EVENT_PAR_0)=0 and get_event_par_arr($first,$EVENT_PAR_CUSTOM,7)=0 and get_event_mark($first,1)=0 and $PLAYED_VOICES_INST=0)
play_note(63,127,0,0)
end if
end if
if ($CC_NUM=4)
message($PLAYED_VOICES_INST)
end if
end on"#;
    let sources = [(60,LONG,false),(61,4,true),(63,512,false)];
    let (mut actual, mut reference) = (engine(&sources,f32::INFINITY,false,Some(script)), engine(&sources,f32::INFINITY,false,None));
    let mut pcm = std::array::from_fn(|_|vec![0.;FRAMES]);
    let mut expected = std::array::from_fn(|_|vec![0.;FRAMES]);
    let root = owner(60,993);
    assert_eq!(crate::plugin::tests::allocations(|| {
        assert!(actual.host_note_on(root,7,60,127,0.)); off(&mut actual,root);
        let first = reference.start_event(&NoteEvent::new(7,60,127)).unwrap();
        // Genuine silent four-frame sources retire in DSP. The retained long
        // Sample alias remains audible while admissions exceed the entire pool.
        for i in 0..5000 {
            let from = i*4; actual.note_on(7,61,127); actual.note_off(7,61);
            render(&mut actual,&mut pcm,from,from+4,parts);
            render(&mut reference,&mut expected,from,from+4,&[128]);
        }
        actual.cc_from(7,4,4,127);
        assert_eq!(actual.script().unwrap().last_message(),"1","one long sample remains among thousands of retired metadata aliases");
        actual.cc_from(7,4,1,127);
        assert_eq!(actual.script().unwrap().last_message(),"1","active Sample generation survives pressure");
        reference.change_event(first,EventChange::Volume(10f32.powf(-6021./20_000.)));
        render(&mut actual,&mut pcm,20_000,LONG,parts);
        render(&mut reference,&mut expected,20_000,LONG,&[128]);
        actual.cc_from(7,4,4,127);
        assert_eq!(actual.script().unwrap().last_message(),"0","source EOF removes the actual sample voice before alias reclamation");
        actual.cc_from(7,4,2,127);
        assert_eq!(actual.script().unwrap().last_message(),"0","complete finite Sample source has no logical held owner");
        // Multiple same-frame Starts occupy the first recyclable row before
        // the next allocation. This forces reuse of the original long alias's
        // row, instead of repeatedly recycling only one short-source row.
        for _ in 0..3 {actual.note_on(7,61,127);actual.note_off(7,61);}
        actual.cc_from(7,4,3,127);
        assert_eq!(actual.script().unwrap().last_message(),"11",
            "old generation stays inactive, pending new Start is active, and stale rows are absent from get_event_ids");
        reference.start_event(&NoteEvent::new(7,63,127)).unwrap();
        render(&mut actual,&mut pcm,LONG,FRAMES,parts);
        render(&mut reference,&mut expected,LONG,FRAMES,&[128]);
        clean(&actual); assert_eq!(actual.host_note_end_at(root),Some(LONG as u64));
    }),0);
    same(&pcm,&expected);
    assert!(pcm[0][20_100]>0.01,"the pressure-protected alias must actually sound after its edit");
    assert!(pcm[0][LONG+100]>0.01,"status zero creates the independently scheduled replacement source");
    assert!(pcm.iter().all(|ch|ch[LONG+512..].iter().all(|x|*x==0.)));
    pcm
}

#[test]
fn runtime_event_queue_keeps_actual_tails_callbacks_held_roots_and_generation_safe_sample_reuse_without_heap() {
    same(&released_tail(&[128]),&released_tail(&[1,7,31,127]));
    same(&source_pool(&[128]),&source_pool(&[1,7,31,127]));
}
