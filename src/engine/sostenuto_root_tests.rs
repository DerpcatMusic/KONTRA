use crate::{
    audio::Sample,
    engine::{Bank, Engine, Expression, HostExpression, HostNote, HostPattern},
    import::{Group, Zone},
    midi::Velocity,
};

const SCRIPT: &str = "on note\nif ($EVENT_NOTE=48)\nplay_note(60,127,0,-1)\nend if\nend on\non release\nignore_event($EVENT_ID)\nwait(1000)\nnote_off($EVENT_ID)\nend on";

fn engine(release: bool) -> Engine {
    let groups = (0..4).map(|_| Group { release_trigger: release, ..Group::default() }).collect();
    let zones = [(48,-1.),(60,1.),(49,1.),(50,1.)].into_iter().enumerate().map(|(group,(key,pan))| Zone {
        group, root: key, low_key: key, high_key: key, pan,
        ..Zone::default()
    }).collect();
    let bank = Bank::from_samples(groups,zones,vec![(Default::default(),Sample {
        rate: 48000, frames: (0..1024).map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / 64.;
            [(0.03*phase.sin()+0.008*(2.*phase).sin()) as f32;2]
        }).collect(),
    })]).unwrap();
    let mut e = Engine::default(); e.reset(48000.); e.attack=0.;
    e.set_home_channel(7); e.set_bank(Some(Box::new(bank)));
    if release {
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[
            SCRIPT,
        ],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none)); e.set_script(Some(Box::new(rt)));
    }
    e
}
fn pattern(n:HostNote)->HostPattern {
    HostPattern { port:i32::from(n.port),channel:i32::from(n.channel),key:i32::from(n.key),id:n.id,clap:n.clap }
}
fn same(a:&[[f32;2]],b:&[[f32;2]]) {
    assert_eq!(a.len(),b.len());
    assert!(a.iter().zip(b).all(|(a,b)|(0..2).all(|ch|(a[ch]-b[ch]).abs()<2e-6)),
        "exact physical sostenuto, generated-child lifetime, or release start changed independent scheduled PCM");
}
fn timeline(partition:&[usize], queued_anonymous:bool, reset_at:Option<usize>)->(Vec<[f32;2]>,[u64;3]) {
    let reset_ignored=reset_at.is_some();
    let immediate=reset_at==Some(512);
    let old_end=if immediate {1536} else if reset_ignored {1584} else {2048};
    let new_end=if immediate {1920} else {1968};
    // Ordinary sample voices are authored independently at the required musical
    // timestamps. None of these references receives a pedal or script release.
    let (mut actual,mut old_pcm,mut new_pcm,mut other_pcm)=(engine(true),engine(false),engine(false),engine(false));
    let mut outside_pcm=engine(false);
    let old=HostNote {port:1,channel:4,key:60,id:720,clap:true};
    let new=HostNote {id:721,..old};
    let other=HostNote {port:1,channel:5,key:64,id:722,clap:true};
    let outside=HostNote {port:1,channel:5,key:65,id:723,clap:true};
    if queued_anonymous { actual.configure_mpe(Some((0,4,48))); }
    if reset_ignored {
        let note_script=if immediate {SCRIPT.replace("wait(1000)\n","")} else {SCRIPT.to_owned()};
        let source=format!("{note_script}\non controller\nif ($CC_NUM=66)\nif (%CC[66]=0)\nmessage(\"ignored pedal-up\")\nignore_controller\nend if\nend if\nend on");
        let (rt,errors)=crate::ksp::Runtime::with_scripts(&[&source],&mut crate::ksp::LogEngine::default(),0,Vec::new());
        assert!(errors.iter().all(Option::is_none));actual.set_script(Some(Box::new(rt)));
    }
    let pedal_channel=if queued_anonymous {0} else {4};
    let (mut p,mut q)=(Vec::with_capacity(4096),Vec::with_capacity(4096));
    let (mut frame,mut chunk)=(0,0);
    let mut ends=[0;3];
    assert_eq!(crate::plugin::tests::allocations(|| {
        for stop in [0,256,512,536,560,768,896,912,944,1024,1536,1584,1968,2048,4096] {
            while frame<stop {
                let len=(stop-frame).min(partition[chunk%partition.len()]);
                let (mut a,mut ar,mut b,mut br,mut c,mut cr,mut d,mut dr,mut x,mut xr)=
                    ([0.;128],[0.;128],[0.;128],[0.;128],[0.;128],[0.;128],[0.;128],[0.;128],[0.;128],[0.;128]);
                actual.render_checked(&mut a[..len],&mut ar[..len]).unwrap();
                old_pcm.render_checked(&mut b[..len],&mut br[..len]).unwrap();
                new_pcm.render_checked(&mut c[..len],&mut cr[..len]).unwrap();
                other_pcm.render_checked(&mut d[..len],&mut dr[..len]).unwrap();
                outside_pcm.render_checked(&mut x[..len],&mut xr[..len]).unwrap();
                if frame<if immediate {512} else if reset_ignored {560} else {944} {
                    assert!(a[..len].iter().all(|v|v.abs()<1e-8),"captured old root sounded before pedal-up");
                }
                for i in 0..len { p.push([a[i],ar[i]]);q.push([b[i]+c[i]+d[i]+x[i],br[i]+cr[i]+dr[i]+xr[i]]); }
                frame+=len;chunk+=1;
            }
            same(&p,&q);
            match stop {
                0=>{
                    assert!(actual.host_note_on_velocity(old,7,48,Velocity::FULL,0.));
                    if queued_anonymous {
                        assert!(actual.note_on_input_velocity_pitch(crate::engine::InputAddress {port:1,channel:4,key:64},7,49,Velocity::FULL,None));
                    } else { assert!(actual.host_note_on_velocity(other,7,49,Velocity::FULL,0.)); }
                    if immediate {assert!(actual.host_note_on_velocity(outside,7,50,Velocity::FULL,0.));}
                },
                256 if !queued_anonymous=>actual.cc_from(7,4,66,127),
                512=>{
                    assert_eq!(actual.active_voices(),0,"all input and child attack families must be absent");
                    if queued_anonymous {
                        // No intervening render: accepted manager CC66 must
                        // capture both physical4 roots before these key-ups.
                        actual.cc_from(7,pedal_channel,66,127);
                        assert!(actual.player.sostenuto_channels() & (1 << 4) == 0,"pedal must still be queued at key-up");
                        assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==old)).is_some_and(|o|o.sostenuto),
                            "accepted down edge did not capture the canonical exact owner");
                    } else {
                        let old_owner=actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==old)).unwrap();
                        assert!(old_owner.sostenuto,"physical owner did not capture the accepted down edge");
                    }
                    actual.host_note_off_velocity(pattern(old),Velocity::from_midi2(32768));
                    if queued_anonymous {actual.note_off_from_velocity(7,4,49,Velocity::from_midi2(32768));}
                    else {actual.host_note_off_velocity(pattern(other),Velocity::from_midi2(32768));}
                    if queued_anonymous {
                        // This root arrives after the accepted down edge, but
                        // before the queued pedal reaches Player's held rows.
                        assert!(actual.host_note_on_velocity(new,7,48,Velocity::FULL,0.));
                        assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==new)).is_some_and(|o|!o.sostenuto));
                    }
                    // A generated following child has its own unlatched
                    // no-attack release at physical key-up, without root latch.
                    old_pcm.note_on(7,60,127);
                    if immediate {
                        // Both root releases are already queued, then reset
                        // arrives at the same frame before any DSP consumes them.
                        actual.cc_from(7,5,66,127);
                        actual.host_note_off_velocity(pattern(outside),Velocity::from_midi2(32768));
                        let mut captured=0;
                        actual.commands.for_each_release_mut(|command, release| {
                            if !command.applied && release.input_root && release.input_sostenuto
                                && command.input_channel==Some(4) {captured+=1;}
                        });
                        assert_eq!(captured,2,"fixture must reset both already-queued captured physical roots");
                        actual.cc_from(7,pedal_channel,121,93);
                        assert_eq!(actual.script().unwrap().last_message(),"ignored pedal-up");
                        assert!(actual.player.sostenuto_channels() & (1 << 4) == 0);
                        actual.commands.for_each_release_mut(|command, release| {
                            assert!(command.applied || !release.input_root || command.input_channel!=Some(4) || !release.input_sostenuto,
                                "mandatory reset left a queued root capture behind");
                            if !command.applied && release.input_root && command.input_channel==Some(5) {
                                assert!(release.input_sostenuto,"manager reset erased an unrelated physical root's queued capture");
                            }
                        });
                        old_pcm.note_on(7,48,127);other_pcm.note_on(7,49,127);
                    }
                },
                536 if reset_at==Some(536)=>{
                    actual.cc_from(7,pedal_channel,121,93);
                    assert_eq!(actual.script().unwrap().last_message(),"ignored pedal-up",
                        "fixture must enter the ignored synthetic CC66-up callback");
                    assert!(actual.player.sostenuto_channels() & (1 << 4) == 0,"CC121 must physically release the pedal despite ignore_controller");
                    assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==old)).is_some_and(|o|!o.sostenuto));
                },
                560=>{
                    if !queued_anonymous || reset_at==Some(536) {other_pcm.note_on(7,49,127);}
                    if reset_at==Some(536) {old_pcm.note_on(7,48,127);}
                },
                768=>{
                    if !queued_anonymous {assert!(actual.host_note_on_velocity(new,7,48,Velocity::FULL,0.));}
                    assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==new)).is_some_and(|o|!o.sostenuto),
                        "queued pedal re-captured a canonical owner admitted after the edge");
                    // A repeated accepted down while the new root is held is
                    // not a new physical edge and cannot capture that root.
                    if queued_anonymous && !reset_ignored {actual.cc_from(7,pedal_channel,66,127);}
                },
                896=>{
                    actual.host_note_off_velocity(pattern(new),Velocity::from_midi2(32768));
                    new_pcm.note_on(7,60,127);
                    if immediate {new_pcm.note_on(7,48,127);}
                },
                912=>{
                    actual.host_expression(pattern(old),HostExpression::Gain(0.5));
                    for key in [48,60] {old_pcm.set_expression_on(7,key,Expression {gain:0.5,..Expression::default()});}
                },
                944=>{
                    if !immediate {new_pcm.note_on(7,48,127);}
                    assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==new)).is_some_and(|o|!o.sostenuto));
                },
                1024 if !reset_ignored=>{
                    assert!(actual.player.pending_releases.iter().any(|r|r.host_note.and_then(|r|actual.player.host_notes.get(r)).is_some_and(|o|o.note==old)),
                        "old exact release did not remain deferred until its physical pedal-up");
                    actual.cc_from(7,pedal_channel,66,0); old_pcm.note_on(7,48,127);
                    if queued_anonymous {other_pcm.note_on(7,49,127);}
                },
                1024 if immediate=>{
                    assert!(actual.host_note_pending(outside));
                    assert!(actual.player.host_notes.active.iter().find_map(|r|actual.player.host_notes.get(*r).filter(|o|o.note==outside)).is_some_and(|o|o.sostenuto),
                        "earlier manager reset cleared an unrelated exact owner's capture");
                    actual.cc_from(7,5,121,93);outside_pcm.note_on(7,50,127);
                },
                _=>{},
            }
            actual.mark_host_notes();
            if stop<(old_end as usize) {assert!(actual.host_note_pending(old));}
        }
        assert!(p[2048..].iter().all(|s|s.iter().all(|v|v.abs()<1e-8)));
        // Required absolute ends are final release start + immutable1024-frame
        // source EOF, rather than another pedal-driven engine's terminal time.
        for (i,(n,end)) in [(old,old_end),(new,new_end),(other,1584)].into_iter().enumerate() {
            if queued_anonymous && n==other {ends[i]=old_end;continue;}
            assert!(!actual.host_note_pending(n));
            assert_eq!(actual.host_note_end_at(n),Some(end)); ends[i]=end;
            actual.retire_host_note(n);
        }
        if immediate {
            assert!(!actual.host_note_pending(outside));
            assert_eq!(actual.host_note_end_at(outside),Some(2048));actual.retire_host_note(outside);
        }
        assert_eq!(actual.active_voices(),0);
        assert_eq!(actual.dropped_commands(),0);
        assert!(actual.script().unwrap().diagnostics().is_empty());
    }),0);
    (p,ends)
}

#[test]
fn exact_release_only_roots_keep_physical_sostenuto_without_latching_generated_children() {
    let (fixed,ends)=timeline(&[128],false,None);
    let (fragmented,fragmented_ends)=timeline(&[1,7,31,127,13,64],false,None);
    same(&fixed,&fragmented);assert_eq!(ends,fragmented_ends);
}

#[test]
fn queued_manager_sostenuto_captures_exact_and_anonymous_roots_at_acceptance() {
    let (fixed,ends)=timeline(&[128],true,None);
    let (fragmented,fragmented_ends)=timeline(&[1,7,31,127,13,64],true,None);
    same(&fixed,&fragmented);assert_eq!(ends,fragmented_ends);
}

#[test]
fn physical_reset_releases_root_capture_even_when_synthetic_pedal_up_is_ignored() {
    let (fixed,ends)=timeline(&[128],true,Some(536));
    let (fragmented,fragmented_ends)=timeline(&[1,7,31,127,13,64],true,Some(536));
    same(&fixed,&fragmented);assert_eq!(ends,fragmented_ends);
}

#[test]
fn physical_reset_clears_already_queued_root_capture_before_same_frame_render() {
    let (fixed,ends)=timeline(&[128],true,Some(512));
    let (fragmented,fragmented_ends)=timeline(&[1,7,31,127,13,64],true,Some(512));
    same(&fixed,&fragmented);assert_eq!(ends,fragmented_ends);
}
