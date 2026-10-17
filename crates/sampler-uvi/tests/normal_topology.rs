//! Normal-loader topology witnesses. All files/PCM are authored; no bank or
//! native execution. The ordinary one-player PCM reference is independent of
//! the candidate multi-player owner implementation.
use sampler_core::{Input, Limits, NoteId, Protocol, Runtime};
use sampler_uvi::{script::{Command, Param, Scope}, scripted::{Driver, Script}};
use std::{path::{Path,PathBuf}, sync::atomic::{AtomicU64,Ordering}, time::Duration};
const RATE:u32=48000;
const FRAMES:usize=2048;
static NEXT:AtomicU64=AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new()->Self {
        let base=PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch");
        std::fs::create_dir_all(&base).unwrap();
        let p=base.join(format!("normal-kg-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir(&p).unwrap();Self(p)
    }
    fn program(&self,name:&str,players:&[(&str,Vec<[i16;2]>,bool)],gain:f64,pre:bool,inserts:&str)->PathBuf {
        let mut oscillators=String::new();
        for (file,pcm,bypass) in players {
            std::fs::write(self.0.join(format!("{file}.wav")),wav(pcm)).unwrap();
            oscillators+=&format!(r#"<SamplePlayer SamplePath="{file}.wav" BaseNote="60" Gain="1" Bypass="{}"><PlaybackOptions Start="0" Stop="{FRAMES}"><Loop Start="0" End="{FRAMES}" Type="0"/></PlaybackOptions></SamplePlayer>"#,u8::from(*bypass));
        }
        let xml=format!(r#"<Program Gain="1"><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() end]]></script></ScriptProcessor></EventProcessors><Layers><Layer Gain="1"><Keygroups><Keygroup Gain="{gain}" FXPostGain="{}" LowKey="60" HighKey="60"><Inserts>{inserts}</Inserts><Oscillators>{oscillators}</Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#,u8::from(pre));
        let path=self.0.join(format!("{name}.uvip"));std::fs::write(&path,xml).unwrap();path
    }
}
impl Drop for Fixture {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
fn wav(pcm:&[[i16;2]])->Vec<u8>{
    let size=u32::try_from(pcm.len()*4).unwrap();let mut v=b"RIFF".to_vec();v.extend((36+size).to_le_bytes());v.extend(b"WAVEfmt ");v.extend(16u32.to_le_bytes());v.extend(1u16.to_le_bytes());v.extend(2u16.to_le_bytes());v.extend(RATE.to_le_bytes());v.extend((RATE*4).to_le_bytes());v.extend(4u16.to_le_bytes());v.extend(16u16.to_le_bytes());v.extend(b"data");v.extend(size.to_le_bytes());for f in pcm{for s in f{v.extend(s.to_le_bytes());}}v
}
fn sources()->(Vec<[i16;2]>,Vec<[i16;2]>,Vec<[i16;2]>,Vec<[i16;2]>){
    let a:Vec<_>=(0..FRAMES).map(|i|[if i%32<16{8192}else{-8192},if i%48<24{6144}else{-6144}]).collect();
    let b:Vec<_>=(0..FRAMES).map(|i|[-a[i][0]+if i%16<8{1024}else{-1024},-a[i][1]+if i%24<12{1536}else{-1536}]).collect();
    let c:Vec<_>=(0..FRAMES).map(|i|[if i%20<10{12000}else{-12000},if i%28<14{-9000}else{9000}]).collect();
    let sum=a.iter().zip(&b).map(|(a,b)|[i16::try_from(i32::from(a[0])+i32::from(b[0])).unwrap(),i16::try_from(i32::from(a[1])+i32::from(b[1])).unwrap()]).collect();(a,b,c,sum)
}
fn input()->Input{Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None}}
struct Session{rt:Runtime,ir:sampler_ir::Instrument,note:NoteId,_keep:Option<(sampler_kontakt::Streamer,Vec<sampler_core::Pcm>)>,stream:bool}
impl Session {
    fn new(path:&Path,stream:bool)->Self{
        let (loaded,cache,keep,horizon)=if stream {
            let s=sampler_uvi::load_streamed(path,RATE,&Default::default()).unwrap();
            let h=u32::try_from(s.report.head_frames.max(sampler_core::PAGE_FRAMES)+127).unwrap();
            (s.loaded,Some(s.cache),Some((s.streamer,s.assets)),Some(h))
        }else{(sampler_uvi::load(path,RATE).unwrap(),None,None,None)};
        let limits=Limits::for_plan(&loaded.plan,8,16);let mut rt=Runtime::new(loaded.plan,limits).unwrap();
        if let Some(cache)=cache{rt=rt.with_stream_cache(cache);rt.set_stream_horizon(horizon.unwrap()).unwrap();rt.set_offline(true);}
        let note=rt.trigger(input(),60,0.75).unwrap();Self{rt,ir:loaded.instrument,note,_keep:keep,stream}
    }
    fn render(&mut self,n:usize)->Vec<[f32;2]>{
        let mut out=vec![[0.;2];n];let blocks=[31,127,64,7];let mut at=0;let mut k=0;
        while at<n{let end=(at+blocks[k%blocks.len()]).min(n);if self.stream{let now=self.rt.now();self.rt.wait_streaming(sampler_core::PAGE_FRAMES as u32+127,Duration::from_secs(2)).unwrap();assert_eq!(self.rt.now(),now);}self.rt.render(&mut out[at..end]).unwrap();at=end;k+=1;}self.health();out
    }
    fn health(&self){let s=self.rt.stats();assert_eq!(s.nonfinite_frames,0);assert_eq!(s.stream_underruns,0);assert_eq!(s.voice_pool_refusals,0);assert_eq!(s.voice_drops,0);assert_eq!(s.refused_starts,0);}
    fn close(mut self){self.rt.release(self.note).unwrap();self.render(4096);self.rt.flush_ended(|_|true);self.health();assert_eq!(self.rt.voice_count(),0);assert_eq!(self.rt.processing_owner_count(),0);}
}
fn compare(actual:&[[f32;2]],reference:&[[f32;2]]){
    assert_eq!(actual.len(),reference.len());let energy:f64=reference.iter().flatten().map(|v|f64::from(*v).powi(2)).sum();assert!(energy>1e-5,"independent ordinary reference must sound");
    let error:f64=actual.iter().flatten().zip(reference.iter().flatten()).map(|(a,b)|(f64::from(*a)-f64::from(*b)).powi(2)).sum();let worst=actual.iter().flatten().zip(reference.iter().flatten()).map(|(a,b)|(*a-*b).abs()).fold(0f32,f32::max);
    assert!(error/energy<1e-8&&worst<4e-6,"ordered-sum/shared-state PCM mismatch normalized={} worst={worst}",error/energy);
}
const NONLINEAR:&str=r#"<WaveShaper Mode="6" InputGain="0" OutputGain="0"/><OnePole Freq="1800" Mode="0" Bypass="0"/>"#;
fn ordinary_case(stream:bool){
    let f=Fixture::new();let(a,b,c,sum)=sources();let target=f.program("members",&[("a",a,false),("b",b,false),("c",c,true)],1.,false,NONLINEAR);let reference=f.program("reference",&[("sum",sum,false)],1.,false,NONLINEAR);
    let mut actual=Session::new(&target,stream);let mut expected=Session::new(&reference,stream);let a=actual.render(8192);let e=expected.render(8192);compare(&a[4096..],&e[4096..]);
    // Check topology only AFTER the independent PCM causal comparison.
    assert_eq!(actual.rt.processing_owner_count(),1);assert_eq!(actual.rt.voice_count(),2);
    let ir=&actual.ir;assert_eq!(ir.zones.len(),3);assert_eq!(ir.assets.len(),3);assert_eq!(ir.processing_scopes[0].members.len(),3);assert!(ir.zones.iter().all(|z|z.chain.is_none()));
    actual.close();expected.close();
}
#[test]fn normal_keygroup_sums_all_retained_players_before_nonlinear_full(){ordinary_case(false);}
#[test]fn normal_keygroup_sums_all_retained_players_before_nonlinear_streamed(){ordinary_case(true);}
#[derive(Default)]struct Commands(Vec<Command>);
impl Script for Commands{
    fn handles_notes(&self)->bool{false}fn set_time(&mut self,_:f64){}fn note_on(&mut self,_:u64,_:u8,_:u8){}fn note_off(&mut self,_:u64,_:u8){}fn advance(&mut self,_:f64){}fn next_due(&mut self)->Option<f64>{None}fn drain(&mut self,out:&mut Vec<Command>){out.extend(self.0.drain(..));}
}
#[test]fn normal_keygroup_zero_gain_opens_closes_reopens_same_sources_absolute(){
    let f=Fixture::new();let(a,b,c,sum)=sources();let target=f.program("zero",&[("a",a,false),("b",b,false),("c",c,true)],0.,false,NONLINEAR);let reference=f.program("open",&[("sum",sum,false)],1.,false,NONLINEAR);
    let translated=sampler_uvi::translate_path(&target).unwrap();let owner=translated.groups[0].keygroup;let mut actual=Session::new(&target,false);let mut expected=Session::new(&reference,false);
    assert!(actual.render(512).iter().flatten().all(|v|*v==0.));expected.render(512);
    let mut driver=Driver::new(Commands(vec![Command::Parameter{scope:Scope::Keygroup(owner),param:Param::Gain,value:1.,authored:0.}]),translated.groups,RATE);driver.wake(&mut actual.rt).unwrap();
    compare(&actual.render(2048),&expected.render(2048));
    let groups=sampler_uvi::translate_path(&target).unwrap().groups;
    let mut close=Driver::new(Commands(vec![Command::Parameter{scope:Scope::Keygroup(owner),param:Param::Gain,value:0.,authored:1.}]),groups.clone(),RATE);close.wake(&mut actual.rt).unwrap();
    assert!(actual.render(512).iter().flatten().all(|v|*v==0.));expected.render(512);
    let mut reopen=Driver::new(Commands(vec![Command::Parameter{scope:Scope::Keygroup(owner),param:Param::Gain,value:1.,authored:0.}]),groups,RATE);reopen.wake(&mut actual.rt).unwrap();
    compare(&actual.render(2048),&expected.render(2048));assert_eq!(actual.rt.voice_count(),2);assert_eq!(actual.rt.processing_owner_count(),1);actual.close();expected.close();
}


// Pan oracle uses cos² rather than the production (1+cos(pi*p))/2 form.
fn pan_reference(mut frames:Vec<[f32;2]>,position:f64)->Vec<[f32;2]> {
    let far=(std::f64::consts::FRAC_PI_2*position.abs()).cos().powi(2);
    for frame in &mut frames {if position>0. {frame[0]=(f64::from(frame[0])*far) as f32;}
        else if position<0. {frame[1]=(f64::from(frame[1])*far) as f32;}}
    frames
}
const PAN_COUPLED:&str=r#"<GainMatrix Gain_1_1="0" Gain_2_1="1" Gain_1_2="1" Gain_2_2="0"/><WaveShaper Mode="6" InputGain="0" OutputGain="0"/><OnePole Freq="1800" Mode="0" Bypass="0"/>"#;
fn pan_paths(f:&Fixture,position:f64)->(PathBuf,PathBuf) {
    let(a,b,c,sum)=sources();let target=f.program("pan-members",&[("pan-a",a,false),("pan-b",b,false),("pan-c",c,true)],1.,false,PAN_COUPLED);
    let xml=std::fs::read_to_string(&target).unwrap().replace("<Keygroup Gain=",&format!("<Keygroup Pan=\"{position}\" Gain="));std::fs::write(&target,xml).unwrap();
    let reference=f.program("pan-reference",&[("pan-sum",sum,false)],1.,false,PAN_COUPLED);(target,reference)
}
fn pan_boundary_case(stream:bool) {
    let f=Fixture::new();let(target,reference)=pan_paths(&f,0.25);let mut actual=Session::new(&target,stream);let mut expected=Session::new(&reference,stream);
    compare(&actual.render(8192)[4096..],&pan_reference(expected.render(8192),0.25)[4096..]);
    assert!(actual.ir.zones.iter().all(|zone|zone.pan.position==0.));assert_eq!(actual.rt.processing_owner_count(),1);assert_eq!(actual.rt.voice_count(),2);
    actual.close();expected.close();
}
#[test]fn normal_keygroup_pan_is_after_shared_coupled_nonlinear_full(){pan_boundary_case(false);}
#[test]fn normal_keygroup_pan_is_after_shared_coupled_nonlinear_streamed(){pan_boundary_case(true);}
#[test]fn normal_keygroup_pan_has_one_live_scalar_for_driver_and_public_engine() {
    let f=Fixture::new();let(target,reference)=pan_paths(&f,0.25);let translated=sampler_uvi::translate_path(&target).unwrap();let owner=translated.groups[0].keygroup;
    let mut actual=Session::new(&target,false);let mut expected=Session::new(&reference,false);
    compare(&actual.render(4123),&pan_reference(expected.render(4123),0.25));
    let mut identities=Vec::new();actual.rt.visit_processing_owners(|owner|identities.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));
    let address=sampler_core::EngineParameterAddress{parameter:sampler_core::engine_parameter_id("ENGINE_PAR_PAN").unwrap(),group:-1,slot:-1,generic:i32::try_from(owner).unwrap()};
    assert_eq!(actual.rt.engine_parameter(address).unwrap(),625000);
    actual.rt.set_engine_parameter(address,375000).unwrap();
    compare(&actual.render(2048),&pan_reference(expected.render(2048),-0.25));
    let mut driver=Driver::new(Commands(vec![Command::Parameter{scope:Scope::Keygroup(owner),param:Param::Pan,value:0.5,authored:-0.25}]),translated.groups,RATE);
    driver.wake(&mut actual.rt).unwrap();assert_eq!(actual.rt.engine_parameter(address).unwrap(),750000);
    compare(&actual.render(2048),&pan_reference(expected.render(2048),0.5));
    let mut after=Vec::new();actual.rt.visit_processing_owners(|owner|after.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));assert_eq!(identities,after);
    actual.close();expected.close();
}


fn constant_pan_path(f:&Fixture,name:&str,base:f64,offset:f64)->(PathBuf,PathBuf) {
    let(target,reference)=pan_paths(f,base);
    let xml=std::fs::read_to_string(&target).unwrap().replace("<Inserts>",&format!(r#"<ControlSignalSources><ConstantModulation Name="SharedPan" Value="{offset}" Bipolar="0"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/SharedPan" Destination="Pan" Ratio="1"/></Connections><Inserts>"#));
    let distinct=f.0.join(format!("{name}.uvip"));std::fs::write(&distinct,xml).unwrap();(distinct,reference)
}
#[test]
fn constant_keygroup_pan_offset_survives_public_absolute_writes() {
    let f=Fixture::new();let(target,reference)=constant_pan_path(&f,"offset",0.25,0.2);
    let translated=sampler_uvi::translate_path(&target).unwrap();let owner=translated.groups[0].keygroup;
    let mut actual=Session::new(&target,false);let mut expected=Session::new(&reference,false);
    compare(&actual.render(4123),&pan_reference(expected.render(4123),0.45));
    assert!(actual.ir.zones.iter().all(|zone|zone.pan.position==0.));
    let address=sampler_core::EngineParameterAddress{parameter:sampler_core::engine_parameter_id("ENGINE_PAR_PAN").unwrap(),group:-1,slot:-1,generic:i32::try_from(owner).unwrap()};
    // The public value remains the raw authored base, not base+constant.
    assert_eq!(actual.rt.engine_parameter(address).unwrap(),625000);
    actual.rt.set_engine_parameter(address,250000).unwrap();
    compare(&actual.render(2048),&pan_reference(expected.render(2048),-0.3));
    let mut driver=Driver::new(Commands(vec![Command::Parameter{scope:Scope::Keygroup(owner),param:Param::Pan,value:0.9,authored:-0.5}]),translated.groups,RATE);
    driver.wake(&mut actual.rt).unwrap();
    // Clamp once after .9 + .2, not the independent control or coefficient.
    compare(&actual.render(2048),&pan_reference(expected.render(2048),1.));
    actual.close();expected.close();
}
#[test]
fn unsupported_shared_pan_refuses_normal_translation_without_partial_plan() {
    let f=Fixture::new();let(a,b,c,_)=sources();let path=f.program("live-pan",&[("a",a,false),("b",b,false),("c",c,true)],1.,false,PAN_COUPLED);
    let original=std::fs::read_to_string(&path).unwrap();
    for connection in [
        r#"<SignalConnection Source="@MIDI CC 1" Destination="Pan" Ratio="1"/>"#,
        r#"<SignalConnection Source="@MIDI CC 1" Destination="Pan" Ratio="1" Bypass="1"/>"#,
        r#"<SignalConnection Source="@VoiceParam LinearKeyFollow" Destination="Pan" Ratio="1"/>"#,
    ] {
        let xml=original.replace("<Inserts>",&format!("<Connections>{connection}</Connections><Inserts>"));std::fs::write(&path,xml).unwrap();
        let error=sampler_uvi::translate_path(&path).err().expect("no partial shared-Pan plan");assert!(error.to_string().contains("live keygroup Pan connection"),"{error}");
    }
    for tag in ["Program","Layer"] {
        let old=format!("<{tag} Gain=\"1\">");let replacement=format!("{old}<Connections><SignalConnection Source=\"@MIDI CC 1\" Destination=\"Pan\" Ratio=\"1\"/></Connections>");
        let xml=original.replace(&old,&replacement);assert_ne!(xml,original);std::fs::write(&path,xml).unwrap();
        let error=sampler_uvi::translate_path(&path).err().expect("no per-player global-Pan fallback");assert!(error.to_string().contains("Program/Layer Pan connection"),"{error}");
    }
}
#[test]
fn player_local_pan_connection_retains_its_original_route_home() {
    let f=Fixture::new();let(a,_,_,_)=sources();let path=f.program("player-pan",&[("player",a,false)],1.,false,"");
    let xml=std::fs::read_to_string(&path).unwrap().replace("<PlaybackOptions",r#"<Connections><SignalConnection Source="@MIDI CC 1" Destination="Pan" Ratio="0.5"/></Connections><PlaybackOptions"#);std::fs::write(&path,xml).unwrap();
    let translated=sampler_uvi::translate_path(&path).unwrap();let zone=&translated.instrument.zones[0];assert_eq!(zone.routes.len(),1);let route=&translated.instrument.routes[zone.routes[0].0];assert_eq!(route.target,sampler_ir::Target::Pan);
}
