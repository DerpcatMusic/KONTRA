//! Authored source-free witnesses through the real timestamp-split process loop.
use super::*;
use sampler_core::{Envelope, Limits, Pcm, Playback, Prepared, Region, Runtime};

fn fixture(source: Option<&str>) -> (SamplerParams, Dsp) {
    let pcm=Pcm::new(48000,vec![[0.25;2];48000].into_boxed_slice()).unwrap();
    let region=Region{sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,envelope:Envelope::new(0,0,0,1.,128).unwrap(),playback:Playback::default()};
    let plan=Prepared::new(48000,vec![pcm],vec![region],128).unwrap();
    let plan=if let Some(source)=source {sampler_ksp::compile(source,48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap().bind(plan).unwrap()} else {plan};
    let limits=Limits{channels:48,..Limits::for_plan(&plan,32,32)};
    let part=CorePart::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("transport event")).unwrap();
    let p=SamplerParams::new();p.volume.set_sample_rate(48000.);p.volume.snap_smoother();
    let mut dsp=Dsp::default();dsp.core=V2Core::with_parts(1,48000.);dsp.core.install(0,Some(Box::new(part)));(p,dsp)
}
fn run<const N:usize>(dsp:&mut Dsp,p:&SamplerParams,events:&EventList,transport:&TransportInfo)->([f32;N],[f32;N]) {
    let mut output=EventList::with_capacity(64);let mut cx=ProcessContext::new(transport,48000.,N,&mut output);
    let (mut left,mut right)=([0.;N],[0.;N]);let mut channels=[left.as_mut_slice(),right.as_mut_slice()];
    let mut audio=AudioBuffer::from_slices_checked(&[],&mut channels,N);
    assert_eq!(super::tests::allocations(||{Sampler::process(dsp,p,&mut audio,events,&mut cx);}),0,"transport publication/render/release allocated or freed on process thread");
    assert_eq!(dsp.core.problems(0).capacity_drops,0);assert_eq!(dsp.core.problems(0).fault_program,0);(left,right)
}
fn on(at:u32,key:u8)->Event {Event::new(at,EventBody::NoteOn{group:0,channel:0,note:key,velocity:100})}
fn value(dsp:&Dsp,name:&str)->i64 {let id=sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0,name).0);dsp.core.control_value(0,id).unwrap() as i64}

#[test]
fn timestamped_transport_updates_all_host_slots_in_order_and_keeps_the_render_clock() {
    let fields=[("q","$DURATION_QUARTER"),("e","$DURATION_EIGHTH"),("s","$DURATION_SIXTEENTH"),("qt","$DURATION_QUARTER_TRIPLET"),("et","$DURATION_EIGHTH_TRIPLET"),("st","$DURATION_SIXTEENTH_TRIPLET"),("bar","$DURATION_BAR"),("pos","$NI_SONG_POSITION"),("num","$SIGNATURE_NUM"),("den","$SIGNATURE_DENOM"),("run","$NI_TRANSPORT_RUNNING")];
    let mut script="on init\n".to_string();
    for row in 0..4 {for (name,_) in fields {script.push_str(&format!("declare ui_knob ${name}{row}(-1000000000,1000000000,1)\n"));}}
    script.push_str("end on\non note\n");
    for row in 0..4 {script.push_str(&format!("if ($EVENT_NOTE={})\n",60+row));for (name,system) in fields {script.push_str(&format!("${name}{row}:={system}\n"));}script.push_str("end if\n");}script.push_str("end on");
    let (p,mut dsp)=fixture(Some(&script));
    let start=TransportInfo{playing:true,tempo:120.,position_beats:10.,time_sig_num:4,time_sig_den:4,..Default::default()};
    let mut events=EventList::with_capacity(8);events.push(on(0,60));
    events.push(Event::new(128,EventBody::Transport(TransportInfo{tempo:240.,position_beats:-2.5,time_sig_num:7,time_sig_den:8,..start})));events.push(on(128,61));
    events.push(Event::new(640,EventBody::Transport(TransportInfo{tempo:60.,position_beats:f64::NAN,time_sig_num:3,time_sig_den:4,..start})));events.push(on(640,62));
    run::<1024>(&mut dsp,&p,&events,&start);
    let expected=[
        [500000,250000,125000,333333,166666,83333,2000000,9600,4,4,1],
        [250000,125000,62500,166666,83333,41666,875000,-2400,7,8,1],
        [1000000,500000,250000,666666,333333,166666,3000000,-2360,3,4,1],
    ];
    for (row,values) in expected.into_iter().enumerate(){for ((name,_),expected) in fields.into_iter().zip(values){assert_eq!(value(&dsp,&format!("${name}{row}")),expected,"host slot {name} at ordered row {row}");}}
    assert_eq!(dsp.core.clock(0),1024);
    // Missing timeline advances from the previous event's rendered clock and tempo.
    let mut next=EventList::with_capacity(1);next.push(on(0,63));
    run::<32>(&mut dsp,&p,&next,&TransportInfo{tempo:f64::NAN,position_beats:f64::NAN,time_sig_num:0,time_sig_den:0,..start});
    assert_eq!(value(&dsp,"$q3"),1000000);assert_eq!(value(&dsp,"$pos3"),-2352);assert_eq!(value(&dsp,"$num3"),3);assert_eq!(value(&dsp,"$den3"),4);
    assert_eq!(dsp.core.clock(0),1056);assert_eq!(dsp.unsupported,0);
}

#[test]
fn timestamped_stop_releases_callbacks_pedals_and_tails_then_start_resumes_at_its_offset() {
    let (p,mut dsp)=fixture(Some("on init declare ui_knob $releases(0,100,1) end on on release inc($releases) end on"));
    let playing=TransportInfo{playing:true,..Default::default()};let stopped=TransportInfo{playing:false,..playing};
    let mut events=EventList::with_capacity(4);events.push(on(0,60));events.push(Event::new(1,EventBody::ControlChange{group:0,channel:0,cc:64,value:127}));events.push(Event::new(16,EventBody::Transport(stopped)));
    dsp.audition[0]=(1000,60);let (left,_)=run::<32>(&mut dsp,&p,&events,&playing);
    assert!(left[..16].iter().any(|sample|*sample!=0.));assert!(left[17..].iter().any(|sample|*sample!=0.),"stop is a musical release, not a hard choke");
    assert_eq!(value(&dsp,"$releases"),1);assert_eq!(dsp.core.voices().active,1);assert_eq!(dsp.audition[0],(0,0));assert!(!dsp.playing);
    assert!(p.shared.heard.iter().all(|key|key.load(Ordering::Relaxed)==0));
    run::<192>(&mut dsp,&p,&EventList::with_capacity(0),&stopped);assert_eq!(dsp.core.voices().active,0,"stopped sustain cannot retain the release tail");
    let mut events=EventList::with_capacity(2);events.push(Event::new(8,EventBody::Transport(playing)));events.push(on(8,62));
    let (left,_)=run::<32>(&mut dsp,&p,&events,&stopped);assert!(left[..8].iter().all(|sample|*sample==0.));assert!(left[8..].iter().any(|sample|*sample!=0.));assert!(dsp.playing);assert_eq!(dsp.core.voices().active,1);
    // The next physical release must finish without another CC64-up.
    let mut off=EventList::with_capacity(1);off.push(Event::new(0,EventBody::NoteOff{group:0,channel:0,note:62,velocity:0}));
    run::<192>(&mut dsp,&p,&off,&playing);assert_eq!(value(&dsp,"$releases"),2);assert_eq!(dsp.core.voices().active,0);assert_eq!(dsp.core.clock(0),448);assert_eq!(dsp.unsupported,0);
}

#[test]
fn transport_only_splits_preserve_source_phase_master_ramp_and_host_automation_order() {
    let (actual_p,mut actual)=fixture(None);let (reference_p,mut reference)=fixture(None);let host=TransportInfo{playing:true,..Default::default()};
    let mut attack=EventList::with_capacity(1);attack.push(on(0,60));run::<32>(&mut actual,&actual_p,&attack,&host);run::<32>(&mut reference,&reference_p,&attack,&host);
    actual_p.volume.set_value(-12.);reference_p.volume.set_value(-12.);
    let mut actual_events=EventList::with_capacity(4);actual_events.push(Event::new(5,EventBody::Transport(TransportInfo{tempo:90.,position_beats:1.,..host})));actual_events.push(Event::new(17,EventBody::Transport(TransportInfo{tempo:180.,position_beats:2.,..host})));
    actual_events.push(Event::new(23,EventBody::ParamChange{id:actual_p.cutoff.id(),value:1.}));
    let mut reference_events=EventList::with_capacity(1);reference_events.push(Event::new(23,EventBody::ParamChange{id:reference_p.cutoff.id(),value:1.}));
    let a=run::<128>(&mut actual,&actual_p,&actual_events,&host);let b=run::<128>(&mut reference,&reference_p,&reference_events,&host);
    for (a,b) in a.0.into_iter().chain(a.1).zip(b.0.into_iter().chain(b.1)){assert!((a-b).abs()<2e-7,"transport split changed ordinary source/ramp PCM: {a} vs {b}");}
    let actual_current:f32=actual_p.volume.current();let reference_current:f32=reference_p.volume.current();assert_eq!(actual_current.to_bits(),reference_current.to_bits());
    assert_eq!(actual_p.cutoff.raw_target(),reference_p.cutoff.raw_target());assert_eq!(actual.core.clock(0),160);assert_eq!(reference.core.clock(0),160);assert_eq!(actual.unsupported,0);assert_eq!(reference.unsupported,0);
}

#[test]
fn timestamped_stop_preserves_the_existing_exact_native_release_owner() {
    let (p,mut dsp)=fixture(Some("on init declare ui_knob $releases(0,100,1) end on on release inc($releases) end on"));
    let host=TransportInfo{playing:true,..Default::default()};let mut events=EventList::with_capacity(4);
    let address=ExactNoteAddress::from_raw_signed(0,0,60,71);
    events.try_push_exact(ExactEvent::new(0,ExactEventBody::Note{kind:ExactNoteKind::On,address,velocity:0.8})).unwrap();
    events.push(Event::new(1,EventBody::ControlChange{group:0,channel:0,cc:64,value:127}));
    events.push(Event::new(16,EventBody::Transport(TransportInfo{playing:false,..host})));
    let (left,_)=run::<32>(&mut dsp,&p,&events,&host);
    assert!(left[..16].iter().any(|sample|*sample!=0.));assert!(left[17..].iter().any(|sample|*sample!=0.));assert_eq!(value(&dsp,"$releases"),1);
    run::<192>(&mut dsp,&p,&EventList::with_capacity(0),&TransportInfo{playing:false,..host});
    assert_eq!(dsp.core.voices().active,0);assert_eq!(value(&dsp,"$releases"),1);assert_eq!(dsp.unsupported,0);
}
