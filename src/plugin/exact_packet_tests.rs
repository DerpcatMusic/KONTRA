//! Exact-only packets must reach the same real receiver as typed companions.
use super::*;
use moose::core::{RawMidi1, RawUmp};

fn decode(body: ExactEventBody) -> Option<ExactInput> {
    let mut events=EventList::with_capacity(1);
    events.try_push_exact(ExactEvent::new(7,body)).unwrap();
    let LosslessEventRef::Exact(event)=events.lossless_iter().next().unwrap() else {panic!("exact packet lost")};
    exact_host_input(event)
}

#[test]
fn exact_only_raw_packets_preserve_their_original_port_group_bits_and_precision() {
    for words in [[0x15f80000,0], [0x45a53c00,0x80000001], [0x45653c00,0x7fffffff], [0x45450709,0xffffffff]] {
        let count=if words[0]>>28==4 {2}else{1};
        let packet=RawUmp::new([words[0],words[1],0,0],count).unwrap();
        assert!(matches!(decode(ExactEventBody::Ump {port:2,packet}),Some(ExactInput::Routed(CoreEvent::Ump(actual),2)) if actual==words),"exact-only packet vanished or changed");
    }
    let message=RawMidi1::new([0xf2,0x12,0x34],3).unwrap();
    assert!(matches!(decode(ExactEventBody::Midi1 {port:3,message}),Some(ExactInput::Routed(CoreEvent::Ump([0x10f21234,0]),3))));
}

#[test]
fn exact_packet_refuses_unrepresentable_ports_and_lengths_without_truncation() {
    for (port,words,count) in [(256,[0x40903c00,0x80000000,0,0],2),(0,[0x40903c00,0,0,0],1),(0,[0x50903c00,0,0,0],4)] {
        let packet=RawUmp::new(words,count).unwrap();
        assert!(matches!(decode(ExactEventBody::Ump {port,packet}),Some(ExactInput::Unsupported)));
    }
    let message=RawMidi1::new([0x90,60,0],2).unwrap();
    assert!(matches!(decode(ExactEventBody::Midi1 {port:0,message}),Some(ExactInput::Unsupported)));
}

#[test]
fn exact_packet_and_semantic_companion_trigger_one_actual_callback_at_the_original_sample() {
    use sampler_core::{Envelope,Limits,Pcm,Playback,Prepared,Region,Runtime};
    let plan=Prepared::new(48000,vec![Pcm::new(48000,vec![[0.25;2];4096].into_boxed_slice()).unwrap()],vec![Region {
        sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,
        envelope:Envelope::default(),playback:Playback::default(),
    }],128).unwrap();
    let program=sampler_ksp::compile("on init declare ui_knob $calls(0,100,1) end on on note inc($calls) end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let plan=program.bind(plan).unwrap();let mut limits=Limits::for_plan(&plan,16,16);limits.channels=48;
    let part=CorePart::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("exact packet")).unwrap();
    let p=SamplerParams::new();let mut s=Dsp::default();s.core=V2Core::with_parts(1,48000.);s.core.install(0,Some(Box::new(part)));
    let words=[0x40903c00,0x80000000,0,0];let mut events=EventList::with_capacity(4);
    let token=events.try_push_exact_token(ExactEvent::new(7,ExactEventBody::Ump {port:0,packet:RawUmp::new(words,2).unwrap()})).unwrap();
    let body=moose::core::ump::decode_ump_channel_voice_2(words).unwrap();events.try_push_exact_companion(token,Event::on_port(7,0,body)).unwrap();
    let mut output=EventList::with_capacity(4);let transport=TransportInfo::default();let mut cx=ProcessContext::new(&transport,48000.,32,&mut output);
    let mut l=[0.;32];let mut r=[0.;32];let mut channels=[l.as_mut_slice(),r.as_mut_slice()];let mut audio=AudioBuffer::from_slices_checked(&[],&mut channels,32);
    Sampler::process(&mut s,&p,&mut audio,&events,&mut cx);
    let id=sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0,"$calls").0);
    assert_eq!(s.core.control_value(0,id),Some(1.));assert_eq!(s.core.voices().active,1);assert_eq!(s.unsupported,0);
    assert!(l[..7].iter().all(|v|*v==0.));assert!(l[7..].iter().any(|v|*v!=0.));
    assert!(p.shared.heard[60].load(Ordering::Relaxed)>0,"semantic companion still updates physical UI state");
}

#[test]
fn fixed_three_byte_clap_storage_does_not_invalidate_short_semantic_midi_messages() {
    for (bytes,expected) in [([0xc0,12,255],0x20c00c00),([0xd1,95,255],0x20d15f00),([0xf8,255,255],0x10f80000),([0xf1,12,255],0x10f10c00)] {
        let message=RawMidi1::new(bytes,3).unwrap();
        assert!(matches!(decode(ExactEventBody::Midi1{port:1,message}),Some(ExactInput::Routed(CoreEvent::Ump([word,0]),1)) if word==expected),"CLAP fixed storage is not semantic message length");
    }
}

#[test]
fn fixed_clap_storage_short_messages_reach_the_real_receiver_without_double_callbacks() {
    use sampler_core::{Envelope,Limits,Pcm,Playback,Prepared,Region,Runtime};
    let plan=Prepared::new(48000,vec![Pcm::new(48000,vec![[0.25;2];4096].into_boxed_slice()).unwrap()],vec![Region {
        sample:0,key_low:0,key_high:127,root_key:None,velocity_low:0.,velocity_high:1.,gain:1.,
        envelope:Envelope::default(),playback:Playback::default(),
    }],128).unwrap();
    let program=sampler_ksp::compile("on init declare ui_knob $calls(0,100,1) declare ui_knob $value(0,127,1) end on on controller if ($CC_NUM=129) inc($calls) $value:=%CC[129] end if end on",48000,sampler_ksp::Limits::LIBRARY,&[]).unwrap();
    let plan=program.bind(plan).unwrap();let mut limits=Limits::for_plan(&plan,16,16);limits.channels=48;
    let part=CorePart::new(Runtime::new(plan,limits).unwrap(),MixTree::instrument("short exact packet")).unwrap();
    let p=SamplerParams::new();let mut s=Dsp::default();s.core=V2Core::with_parts(1,48000.);s.core.install(0,Some(Box::new(part)));
    let mut events=EventList::with_capacity(8);
    for (at,bytes,companion) in [(3,[0xd0,95,255],Some(EventBody::ChannelPressure{group:0,channel:0,pressure:95})),
        (9,[0xc0,12,255],Some(EventBody::ProgramChange{group:0,channel:0,program:12})),(17,[0xf8,255,255],None)] {
        let token=events.try_push_exact_token(ExactEvent::new(at,ExactEventBody::Midi1{port:0,message:RawMidi1::new(bytes,3).unwrap()})).unwrap();
        if let Some(body)=companion {events.try_push_exact_companion(token,Event::on_port(at,0,body)).unwrap();}
    }
    let mut output=EventList::with_capacity(8);let transport=TransportInfo::default();let mut cx=ProcessContext::new(&transport,48000.,32,&mut output);
    let mut l=[0.;32];let mut r=[0.;32];let mut channels=[l.as_mut_slice(),r.as_mut_slice()];let mut audio=AudioBuffer::from_slices_checked(&[],&mut channels,32);
    assert_eq!(super::tests::allocations(||{Sampler::process(&mut s,&p,&mut audio,&events,&mut cx);}),0);
    let id=|name|sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0,name).0);
    assert_eq!(s.core.control_value(0,id("$calls")),Some(1.));assert_eq!(s.core.control_value(0,id("$value")),Some(95.));
    assert_eq!(s.unsupported,0,"none of these fixed-storage packets may be rejected at the plugin boundary");
    // This fixture has no program selector or external MIDI-clock authority.
    // They reach one truthful core refusal each; neither packet is swallowed
    // nor replayed through its optional typed companion.
    assert_eq!(s.core.problems(0).ignored_input,2);assert_eq!(s.core.problems(0).capacity_drops,0);assert_eq!(s.core.problems(0).fault_program,0);
    assert_eq!(s.core.clock(0),32);
}
