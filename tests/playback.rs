use kontakto::{audio::Sample,engine::{Bank,Engine},import::{Group,Zone,Loop,Resolver}};
use std::path::PathBuf;
fn engine()->Engine {
 let group=Group{name:"test".into(),gain:1.0,pan:0.0,tune:1.0,key_tracking:true,reverse:false,release_trigger:false,muted:false,channel:-1,..Default::default()};
 let zone=Zone{group:0,sample:PathBuf::new(),available:true,low_key:60,high_key:72,root:60,low_velocity:10,high_velocity:127,start:0,end:0,gain:1.0,pan:0.0,tune:1.0,loop_range:Some(Loop{start:0,end:100,until_release:false,crossfade:0}),..Default::default()};
 let mut e=Engine::default();e.attack=0.0001;e.release=0.001;e.bank=Some(Box::new(Bank{zones:vec![zone],groups:vec![group],samples:vec![Sample{rate:48000,frames:vec![[0.5,0.25];100]}],sample_ids:vec![0],bytes:800}));e
}
#[test]
fn mapped_polyphony_sustain_channels_and_release() {
 let mut e=engine();e.note_on(0,59,100);e.note_on(0,60,9);assert_eq!(e.frame(),[0.,0.]);
 e.note_on(0,60,127);for _ in 0..20 {e.frame();}assert_eq!(e.frame(),[0.5,0.25]);
 e.note_on(1,60,127);for _ in 0..20{e.frame();}assert_eq!(e.frame(),[1.,0.5]);
 e.cc(0,64,127);e.note_off(0,60);e.note_off(1,60);for _ in 0..1000{e.frame();}assert_eq!(e.frame(),[0.5,0.25]);
 e.cc(0,64,0);for _ in 0..1000{e.frame();}assert_eq!(e.frame(),[0.,0.]);assert_eq!(e.active_voices(),0);
 for _ in 0..1000 {e.note_on(0,60,127);}assert_eq!(e.active_voices(),256);e.cc(0,120,0);assert_eq!(e.active_voices(),0);
}
#[test]
fn root_pitch_octave_reverse_and_end_bounds() {
 let mut a=engine();a.bank.as_mut().unwrap().zones[0].loop_range=None;
 a.bank.as_mut().unwrap().samples[0].frames=(0..100).map(|i|[i as f32/100.;2]).collect();
 a.note_on(0,72,127);for _ in 0..10{a.frame();}assert!((a.frame()[0]-0.2).abs()<1e-6);
 for _ in 0..100{a.frame();}assert_eq!(a.active_voices(),0);
 a.bank.as_mut().unwrap().groups[0].reverse=true;a.note_on(0,60,127);for _ in 0..10{a.frame();}assert!((a.frame()[0]-0.89).abs()<1e-6);
}
#[test]
fn wav_decode_and_case_insensitive_resolution() {
 let dir=std::env::temp_dir().join(format!("kontakto-check-{}",std::process::id()));std::fs::create_dir_all(dir.join("Samples")).unwrap();
 let path=dir.join("Samples/piano.wav");
 let spec=hound::WavSpec{channels:2,sample_rate:44100,bits_per_sample:16,sample_format:hound::SampleFormat::Int};
 let mut w=hound::WavWriter::create(&path,spec).unwrap();for _ in 0..128{w.write_sample(16384i16).unwrap();w.write_sample(-8192i16).unwrap();}w.finalize().unwrap();
 let mut r=Resolver::new(&dir);let resolved=r.resolve(&dir,"samples/PIANO.WAV").unwrap().unwrap();assert_eq!(resolved,path);
 let sample=kontakto::audio::decode(&path,128).unwrap();assert_eq!(sample.rate,44100);assert_eq!(sample.frames.len(),128);assert_eq!(sample.frames[0],[0.5,-0.25]);
 assert!(kontakto::audio::decode(&path,127).is_err());std::fs::remove_dir_all(dir).unwrap();
}
#[test]
#[ignore="uses the owner's installed library; never redistributes samples"]
fn vista_harp_real_instrument() {
 let path=PathBuf::from(kontakto::import::LIBRARY_ROOT).join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki");
 let source=kontakto::import::source_inventory(&path).unwrap();assert!(!source["chunks"].as_object().unwrap().is_empty());assert!(source["chunks"].as_object().unwrap().values().all(|v|v.get("inspection_error").is_none()));
 let i=kontakto::import::read(&path).unwrap();assert_eq!(i.groups.len(),20);assert_eq!(i.zones.len(),2000);assert!(i.missing_samples.is_empty());assert!(!i.scripts.is_empty());
 let b=Bank::load(&i,0).unwrap();assert!(!b.samples.is_empty());let mut e=Engine::default();e.bank=Some(Box::new(b));e.note_on(0,60,100);
 let peak=(0..48000).map(|_|e.frame()[0].abs()).fold(0f32,f32::max);assert!(peak>0.001 && peak.is_finite());
}

#[test]
#[ignore = "uses the owner's installed library; never redistributes samples"]
fn vista_harp_modulation_is_decoded() {
    use kontakto::import::{ModSource, ModTarget};

    let path = PathBuf::from(kontakto::import::LIBRARY_ROOT)
        .join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki");
    let instrument = kontakto::import::read(&path).unwrap();
    assert!(!instrument.warnings.iter().any(|w| w.contains("modulation not imported")));
    for group in &instrument.groups {
        let env = group.volume_env.as_ref().expect("every group has a volume AHDSR");
        assert!(env.attack_ms >= 0.0 && (0.0..=1.0).contains(&env.sustain));
        assert_eq!(group.cc_volume().map(|(cc, _)| cc), Some(11));
        assert!(group.modulation(ModSource::KeyPosition, &ModTarget::Volume).is_some());
    }
}

#[test]
#[ignore = "requires the owner's local Una Corda library"]
fn encrypted_una_corda_uses_local_access_data() {
 let p=std::path::Path::new(kontakto::import::LIBRARY_ROOT).join("Una Corda Library/Instruments/Una Corda Cotton.nki");
 let i=kontakto::import::read(&p).unwrap();
 assert_eq!(i.missing_samples.len(),i.missing_samples.iter().collect::<std::collections::HashSet<_>>().len());
 assert_eq!(i.name,"Una Corda Cotton");assert!(i.zones.len()>4000);assert!(!i.scripts.is_empty());
 let group=i.first_playable_group().unwrap();
 let bank=kontakto::engine::Bank::load(&i,group).unwrap();assert!(!bank.zones.is_empty());
}

#[test]
fn rack_routes_layers_mutes_solos_and_releases_after_channel_changes() {
 use kontakto::engine::Rack;
 let mut rack=Rack::default();rack.parts[0]=engine();rack.parts[1]=engine();rack.controls[0].channel=0;rack.controls[1].channel=1;
 rack.note_on(0,60,127);for _ in 0..20 {rack.frame();}assert_eq!(rack.frame(),[0.5,0.25]);assert_eq!(rack.parts[1].active_voices(),0);
 rack.note_on(1,60,127);for _ in 0..20 {rack.frame();}assert_eq!(rack.frame(),[1.,0.5]);
 rack.controls[0].mute=true;assert_eq!(rack.frame(),[0.5,0.25]);rack.controls[0].mute=false;rack.controls[1].solo=true;assert_eq!(rack.frame(),[0.5,0.25]);
 rack.controls[1].pan=1.;rack.controls[1].gain=0.5;assert_eq!(rack.frame(),[0.,0.125]);
 rack.controls[0].channel=4;rack.note_off(0,60);rack.note_off(1,60);for _ in 0..1000 {rack.frame();}assert_eq!(rack.frame(),[0.,0.]);
 rack.controls[1].solo=false;rack.controls[0].channel=-1;rack.controls[1].channel=-1;rack.controls[1].gain=1.;rack.controls[1].pan=0.;rack.note_on(3,60,127);for _ in 0..20 {rack.frame();}assert_eq!(rack.frame(),[1.,0.5]);
}

#[test]
fn rack_midi_ports_audio_buses_and_route_changes_are_isolated() {
 use kontakto::engine::Rack;
 let mut rack=Rack::default();rack.parts[0]=engine();rack.parts[1]=engine();rack.controls[1].port=1;rack.controls[1].output=3;
 rack.note_on_port(1,0,60,127);for _ in 0..20 {rack.frame_outputs();}let out=rack.frame_outputs();assert_eq!(out[0],[0.,0.]);assert_eq!(out[3],[0.5,0.25]);
 rack.note_off_port(0,0,60);for _ in 0..100 {rack.frame_outputs();}assert_eq!(rack.frame_outputs()[3],[0.5,0.25]);
 let mut controls=rack.controls;controls[1].port=2;rack.set_controls(controls);assert_eq!(rack.frame_outputs(),[[0.,0.];8]);
 rack.note_on_port(1,0,60,127);assert_eq!(rack.parts[1].active_voices(),0);rack.note_on_port(2,0,60,127);assert_eq!(rack.parts[1].active_voices(),1);
 rack.note_off_port(2,0,60);for _ in 0..1000 {rack.frame_outputs();}assert_eq!(rack.parts[1].active_voices(),0);
}
