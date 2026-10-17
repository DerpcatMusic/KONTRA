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
