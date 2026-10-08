//! Compare visual ownership to actual voices; synthetic PCM stays in memory.
use super::*;
use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};
use crate::sound::event::{HostPattern, HostNote};

fn fixture(script: Option<&str>) -> (SamplerParams, Dsp) {
    let pcm = Pcm::new(48000, vec![[0.1; 2]; 48000].into_boxed_slice()).unwrap();
    let region = Region { sample: 0, key_low: 48, key_high: 84, root_key: Some(60),
        velocity_low: 0., velocity_high: 1., gain: 1., envelope: Envelope::new(0,0,0,1.,128).unwrap(), playback: Playback::default() };
    let prepared = Prepared::new(48000, vec![pcm], vec![region], 128).unwrap();
    let prepared = if let Some(source) = script { sampler_ksp::compile(source,48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap().bind(prepared).unwrap() } else { prepared };
    let limits = Limits::for_plan(&prepared,16,16);
    let runtime = Runtime::new(prepared, limits).unwrap();
    let part = CorePart::new(runtime, MixTree::instrument("pressed test")).unwrap();
    let params = SamplerParams::new();
    let mut dsp = Dsp::default(); dsp.core = V2Core::with_parts(1,48000.); dsp.core.install(0,Some(Box::new(part)));
    (params,dsp)
}
fn exact(dsp: &mut Dsp, p: &SamplerParams, event: CoreEvent) {
    let mut out = EventList::with_capacity(32); let transport=TransportInfo::default();
    let mut cx=ProcessContext::new(&transport,48000.,64,&mut out);
    feed_exact_input(dsp,p,event,0,0,&mut cx);
}
fn drain(dsp: &mut Dsp, p: &SamplerParams) {
    for _ in 0..8 { process(dsp,p,false); }
}
fn note(key:u8)->HostNote { HostNote {port:0,channel:0,key,id:i32::from(key),clap:true} }
fn on(key:u8)->CoreEvent { CoreEvent::NoteOn {note:note(key),velocity:0.8,tune:0.} }
fn off(key:u8)->CoreEvent { CoreEvent::NoteOff(HostPattern {port:0,channel:0,key:i32::from(key),id:i32::from(key),clap:true}) }

fn process(dsp: &mut Dsp, p: &SamplerParams, playing: bool) {
    let mut out = EventList::with_capacity(32);
    let transport = TransportInfo { playing, ..Default::default() };
    let mut cx = ProcessContext::new(&transport,48000.,64,&mut out);
    let (mut left,mut right)=([0.;64],[0.;64]);
    let mut channels=[left.as_mut_slice(),right.as_mut_slice()];
    let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,64);
    Sampler::process(dsp,p,&mut buffer,&EventList::with_capacity(0),&mut cx);
}

#[test]
fn host_stop_clears_real_and_visual_notes() {
    let (p,mut dsp)=fixture(None);
    process(&mut dsp,&p,true);
    exact(&mut dsp,&p,on(60)); process(&mut dsp,&p,true);
    assert_eq!(dsp.core.voices().active,1);
    process(&mut dsp,&p,false);
    assert_eq!(dsp.core.voices().active,1,"transport stop preserves the release tail");
    assert!(p.shared.heard.iter().all(|v|v.load(Ordering::Relaxed)==0));
    drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0,"the release tail must finish");
}

#[test]
fn reset_hard_cuts_held_notes_and_release_tails() {
    let (p,mut dsp)=fixture(None);
    exact(&mut dsp,&p,on(60)); process(&mut dsp,&p,true);
    exact(&mut dsp,&p,off(60));
    Sampler::reset(&mut dsp,&p,&AudioConfig::new(48000.,64));
    assert_eq!(dsp.core.voices().active,0);
    assert_eq!(p.shared.voices.load(Ordering::Relaxed),0,"hard reset must publish zero voices without another audio block");
    assert_eq!(p.shared.audible.load(Ordering::Relaxed),0);
    assert!(p.shared.heard.iter().chain(&p.shared.played).all(|v|v.load(Ordering::Relaxed)==0));
}

#[test]
fn host_stop_runs_release_callback_and_clears_sustain() {
    let (p,mut dsp)=fixture(Some("on init declare ui_knob $releases(0,100,1) end on on release inc($releases) end on"));
    process(&mut dsp,&p,true);
    exact(&mut dsp,&p,on(60));
    dsp.core.play(0,CoreEvent::midi1(0xb0,64,127)); process(&mut dsp,&p,true);
    process(&mut dsp,&p,false);
    let id=sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0,"$releases").0);
    assert_eq!(dsp.core.control_value(0,id),Some(1.),"transport stop uses the normal release callback");
    drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0,"pedal must not retain the stopped note");
}

#[test]
fn exact_note_off_clears_visual_after_real_voice_ends() {
    let (p,mut dsp)=fixture(None);
    exact(&mut dsp,&p,on(60)); dsp.core.render(64);
    assert!(p.shared.heard[60].load(Ordering::Relaxed)>0);
    exact(&mut dsp,&p,off(60)); drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0,"this is a visual failure, not a hanging voice");
    assert!(!dsp.core.key_held(0,60));
    assert_eq!(p.shared.heard[60].load(Ordering::Relaxed),0,"completed release must unlight the key");
}

#[test]
fn sustain_off_clears_visual_after_real_voice_ends() {
    let (p,mut dsp)=fixture(None);
    exact(&mut dsp,&p,on(60)); dsp.core.play(0,CoreEvent::midi1(0xb0,64,127)); dsp.core.render(64);
    exact(&mut dsp,&p,off(60)); drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,1,"pedal intentionally holds this voice");
    dsp.core.play(0,CoreEvent::midi1(0xb0,64,0)); drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0);
    assert_eq!(p.shared.heard[60].load(Ordering::Relaxed),0,"pedal release must not leave a stale key");
}

#[test]
fn all_notes_off_clears_visual_after_real_voice_ends() {
    let (p,mut dsp)=fixture(None);
    exact(&mut dsp,&p,on(60)); dsp.core.render(64);
    dsp.core.play(0,CoreEvent::midi1(0xb0,123,0)); drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0);
    assert_eq!(p.shared.heard[60].load(Ordering::Relaxed),0);
}

#[test]
fn script_generated_note_release_clears_both_visual_keys() {
    let (p,mut dsp)=fixture(Some("on init declare $child end on on note $child := play_note(64,100,0,-1) end on on release note_off($child) end on"));
    exact(&mut dsp,&p,on(60)); dsp.core.render(64);
    assert!(dsp.core.voices().active>0);
    process(&mut dsp,&p,false);
    assert!(p.shared.heard[64].load(Ordering::Relaxed)>0,"generated gate must light its actual key");
    exact(&mut dsp,&p,off(60)); drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0);
    assert!(p.shared.heard.iter().all(|v|v.load(Ordering::Relaxed)==0));
}

#[cfg(feature="shots")]
fn pointer_release(drag: bool) {
    use moose::mui::mui::prelude::*;
    let (p,mut dsp)=fixture(None);
    let p=Arc::new(p);
    let mut h=crate::ui::tests::Harness::new(&p,1000.,800.);
    let path=if drag {vec![60,64]} else {vec![60]};
    for note in path {
        let r=h.ui.scene().unwrap().surface(&format!("key-{note}")).unwrap().frame;
        let pos=Point::new(r.x+r.size.width/2.,r.y+r.size.height*0.85);
        for _ in 0..3 {
            h.tick(Input {pointer:PointerInput {pos:Some(pos),buttons:Buttons::PRIMARY,..Default::default()},..Default::default()});
            process(&mut dsp,&p,false);
        }
        assert!(dsp.core.voices().active>0);
        assert!(p.shared.heard[note].load(Ordering::Relaxed)>0);
    }
    for _ in 0..3 {
        h.tick(Input {pointer:PointerInput {pos:Some(Point::new(-20.,-20.)),..Default::default()},..Default::default()});
        process(&mut dsp,&p,false);
    }
    drain(&mut dsp,&p);
    assert_eq!(dsp.core.voices().active,0);
    assert!(p.shared.heard.iter().chain(&p.shared.played).all(|v|v.load(Ordering::Relaxed)==0));
}

#[cfg(feature="shots")]
#[test]
fn keyboard_mouse_up_outside_releases_real_voice_and_visual() { pointer_release(false); }

#[cfg(feature="shots")]
#[test]
fn keyboard_drag_across_keys_releases_real_voices_and_visuals() { pointer_release(true); }

#[cfg(feature="clap")]
#[test]
fn clap_reset_and_deactivate_clear_pressed_state_through_real_vtable() {
    let result=moose_clap::lifecycle_reset_smoke::<Plugin>(|p| {
        p.shared.press_key(EVERY_PART,60,100);
        p.shared.heard[60].store(100,Ordering::Relaxed);
    },|p|p.shared.played.iter().chain(&p.shared.heard).all(|v|v.load(Ordering::Relaxed)==0));
    assert!(result[0],"CLAP reset must clear engine and keyboard ownership");
    assert!(result[1],"CLAP deactivate must invoke the same hard reset");
}

#[test]
fn explicit_panic_hard_cuts_held_notes_and_tails() {
    let (p,mut dsp)=fixture(None);
    exact(&mut dsp,&p,on(60));process(&mut dsp,&p,true);
    p.shared.panic.store(true,Ordering::Release);
    process(&mut dsp,&p,true);
    assert_eq!(dsp.core.voices().active,0);
    assert!(p.shared.heard.iter().chain(&p.shared.played).all(|v|v.load(Ordering::Relaxed)==0));
}
