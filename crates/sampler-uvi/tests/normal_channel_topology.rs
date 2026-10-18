//! Authored ordinary-reader N-port causals. The caller supplies a concrete
//! output matrix; no file-channel count is re-labelled a native layout enum.
//! Original multichannel_matrix.rs cases/oracles are untouched.
use sampler_core::{ControlId, ControlValue, ControlWrite, Input, Limits, NoteId, Protocol, Runtime, SlotKind};
use sampler_ir as ir;
use std::{path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}, time::Duration};
const RATE:u32=48_000;
const FRAMES:usize=2048;
static NEXT:AtomicU64=AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new()->Self{let base=PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch");let path=base.join(format!("normal-channel-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));std::fs::create_dir_all(&path).unwrap();Self(path)}
    fn program<const N:usize>(&self,name:&str,players:&[(&str,Vec<[i16;N]>,bool)],gain:f64,matrix:Option<[[i16;6];6]>,filter_bypass:bool)->PathBuf {
        let mut oscillator=String::new();for (file,samples,bypass) in players {
            std::fs::write(self.0.join(format!("{file}.wav")),wav(samples)).unwrap();
            oscillator+=&format!(r#"<SamplePlayer SamplePath="{file}.wav" BaseNote="60" Gain="1" Bypass="{}"><PlaybackOptions Start="0" Stop="{FRAMES}"><Loop Start="0" End="{FRAMES}" Type="0"/></PlaybackOptions></SamplePlayer>"#,u8::from(*bypass));
        }
        let mut inserts=String::new();if let Some(matrix)=matrix {
            inserts+="<GainMatrix Name=\"matrix\" Bypass=\"0\"";for (output,row) in matrix.iter().enumerate(){for (input,value) in row.iter().enumerate(){inserts+=&format!(" Gain_{}_{}=\"{}\"",input+1,output+1,f64::from(*value)/4.);}}inserts+="/>";
            inserts+="<WaveShaper Mode=\"6\" InputGain=\"0\" OutputGain=\"0\"/>";
        }
        inserts+=&format!(r#"<OnePole Name="filter" Freq="1800" Mode="0" Bypass="{}"/>"#,u8::from(filter_bypass));
        let xml=format!(r#"<Program Gain="1"><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() end]]></script></ScriptProcessor></EventProcessors><Layers><Layer Gain="1"><Keygroups><Keygroup Gain="{gain}" FXPostGain="0" LowKey="60" HighKey="60"><Inserts>{inserts}</Inserts><Oscillators>{oscillator}</Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);
        let path=self.0.join(format!("{name}.uvip"));std::fs::write(&path,xml).unwrap();path
    }
}
impl Drop for Fixture{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
fn wav<const N:usize>(samples:&[[i16;N]])->Vec<u8>{let bytes=u32::try_from(samples.len()*N*2).unwrap();let mut out=b"RIFF".to_vec();out.extend((36+bytes).to_le_bytes());out.extend(b"WAVEfmt ");out.extend(16u32.to_le_bytes());out.extend(1u16.to_le_bytes());out.extend(u16::try_from(N).unwrap().to_le_bytes());out.extend(RATE.to_le_bytes());out.extend((RATE*u32::try_from(N*2).unwrap()).to_le_bytes());out.extend(u16::try_from(N*2).unwrap().to_le_bytes());out.extend(16u16.to_le_bytes());out.extend(b"data");out.extend(bytes.to_le_bytes());for frame in samples{for sample in frame{out.extend(sample.to_le_bytes());}}out}
fn matrix()->[[i16;6];6]{let mut matrix=[[0;6];6];matrix[0][2]=2;matrix[0][4]=1;matrix[1][3]=-1;matrix[1][5]=2;matrix}
fn sources()->(Vec<[i16;6]>,Vec<[i16;6]>,Vec<[i16;6]>,Vec<[i16;6]>){let a:Vec<_>=(0..FRAMES).map(|i|std::array::from_fn(|c|if (i/(8+c*4))%2==0 {256*(c as i16+1)}else{-256*(c as i16+1)})).collect();let b:Vec<_>=a.iter().enumerate().map(|(i,a)|std::array::from_fn(|c|-a[c]+if(i/(11+c*3))%2==0{128}else{-128})).collect();let c:Vec<_>=(0..FRAMES).map(|i|std::array::from_fn(|c|if(i/(7+c))%2==0{8192}else{-8192})).collect();let sum=a.iter().zip(&b).map(|(a,b)|std::array::from_fn(|c|a[c]+b[c])).collect();(a,b,c,sum)}
fn projection()->ir::ChannelMatrix{ir::ChannelMatrix{input:ir::SignalLayout{channels:6},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,0.,0.5,0.,0.,1.,0.,0.,0.,0.5],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32}}
// Independent signed-integer matrix, rectifier and caller projection. The WAV
// encodes exact authored quarter coefficients, not output from production DSP.
fn reference(samples:&[[i16;6]],matrix:[[i16;6];6],matrix_bypassed:bool)->Vec<[i16;2]>{samples.iter().map(|frame|{let intermediate:[i32;6]=std::array::from_fn(|output|{let n=if matrix_bypassed{i32::from(frame[output])*4}else{frame.iter().zip(matrix[output]).map(|(v,g)|i32::from(*v)*i32::from(g)).sum()};assert_eq!(n%4,0);(n/4).abs()});std::array::from_fn(|channel|{let n=intermediate[channel]*2+intermediate[4+channel];assert_eq!(n%2,0);i16::try_from(n/2).unwrap()})}).collect()}
fn input()->Input{Input{protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None}}
struct Session{rt:Runtime,ir:ir::Instrument,note:NoteId,filter:u32,matrix:Option<u32>,_keep:Option<(sampler_kontakt::Streamer,Vec<sampler_core::Pcm>)>,horizon:Option<u32>}
impl Session {
    fn new(path:&Path,stream:bool,port:bool)->Self{
        let mut translated=sampler_uvi::translate_path(path).unwrap();
        let xml=std::fs::read_to_string(path).unwrap();let document=roxmltree::Document::parse(&xml).unwrap();let filter=u32::try_from(document.descendants().find(|n|n.has_tag_name("OnePole")).unwrap().id().get_usize()).unwrap();let matrix=translated.instrument.source_indices.processing_matrices.first().map(|m|m.node);
        if port {translated.instrument.source_indices.processing_ports=translated.instrument.source_indices.processing.iter().map(|recipe|ir::SourceProcessingPort{owner:recipe.owner,conversion:projection()}).collect();}
        let (loaded,cache,keep,horizon)=if stream{let value=sampler_uvi::assemble_translated_streamed(translated,RATE,&Default::default()).unwrap();let horizon=u32::try_from(value.report.head_frames.max(sampler_core::PAGE_FRAMES)+127).unwrap();(value.loaded,Some(value.cache),Some((value.streamer,value.assets)),Some(horizon))}else{(sampler_uvi::assemble_translated(translated,RATE).unwrap(),None,None,None)};
        let limits=Limits::for_plan(&loaded.plan,8,16);let mut rt=Runtime::new(loaded.plan,limits).unwrap();if let Some(cache)=cache{rt=rt.with_stream_cache(cache);rt.set_stream_horizon(horizon.unwrap()).unwrap();rt.set_offline(true);}
        let note=rt.trigger(input(),60,0.75).unwrap();Self{rt,ir:loaded.instrument,note,filter,matrix,_keep:keep,horizon}
    }
    fn render(&mut self,n:usize)->Vec<[f32;2]>{let mut output=vec![[0.;2];n];let mut at=0;let mut step=0;let sizes=[31,127,64,7];while at<n{let end=(at+sizes[step%4]).min(n);if let Some(horizon)=self.horizon{let now=self.rt.now();self.rt.wait_streaming(horizon,Duration::from_secs(2)).unwrap();assert_eq!(self.rt.now(),now);}self.rt.render(&mut output[at..end]).unwrap();at=end;step+=1;}self.health();output}
    fn health(&self){let stats=self.rt.stats();assert_eq!(stats.stream_underruns,0);assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.voice_pool_refusals,0);assert_eq!(stats.refused_starts,0);assert_eq!(stats.voice_drops,0);}
    fn write(&mut self,id:ControlId,value:f64){let plan=self.rt.active_plan();self.rt.edit_controls(plan,None,&[ControlWrite{id,value:ControlValue::Real(value)}]).unwrap();}
    fn bypass(&mut self,node:u32,value:bool){self.write(sampler_core::slot_control(SlotKind::Bypass,-1,-1,i32::try_from(node).unwrap()),f64::from(value));}
    fn close(mut self){self.rt.release(self.note).unwrap();self.render(8192);self.rt.flush_ended(|_|true);assert_eq!(self.rt.voice_count(),0);assert_eq!(self.rt.processing_owner_count(),0);self.health();}
}
fn compare(actual:&[[f32;2]],expected:&[[f32;2]]){let energy:f64=expected.iter().flatten().map(|x|f64::from(*x).powi(2)).sum();assert!(energy>1e-6,"independent reference must sound");let error:f64=actual.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)|(f64::from(*a)-f64::from(*b)).powi(2)).sum();let worst=actual.iter().flatten().zip(expected.iter().flatten()).map(|(a,b)|(a-b).abs()).fold(0f32,f32::max);assert!(error/energy<1e-8&&worst<4e-6,"N-port mismatch normalized={} worst={worst}",error/energy);}
fn paths(f:&Fixture,filter_bypass:bool)->(PathBuf,PathBuf){let(a,b,c,sum)=sources();let target=f.program("wide",&[("a",a,false),("b",b,false),("c",c,true)],1.,Some(matrix()),filter_bypass);let reference=f.program("ordinary-reference",&[("reference",reference(&sum,matrix(),false),false)],1.,None,filter_bypass);(target,reference)}
fn static_case(stream:bool){let f=Fixture::new();let(target,reference)=paths(&f,false);let mut actual=Session::new(&target,stream,true);let mut expected=Session::new(&reference,stream,false);let a=actual.render(8192);let b=expected.render(8192);compare(&a[4096..],&b[4096..]);assert_eq!(actual.rt.voice_count(),2);assert_eq!(actual.rt.processing_owner_count(),1);assert_eq!(actual.ir.zones.len(),3);assert_eq!(actual.ir.assets.len(),3);let matrix=&actual.ir.source_indices.processing_matrices[0];assert_eq!(matrix.matrix.cells.len(),144);let bound=matrix.compiled.as_ref().unwrap();assert_eq!(bound.layout.channels,6);assert_eq!(bound.cells.len(),36);assert!(bound.signal_node.is_some());assert_eq!(matrix.control_definitions.len(),144);let keys:Vec<_>=matrix.control_definitions.iter().map(|cell|&cell.control.key).collect();assert_eq!(actual.ir.controls.iter().filter(|control|keys.contains(&&control.key)).count(),36,"inactive source cells must not allocate mutable runtime controls");actual.close();expected.close();}
#[test]fn normal_six_channel_shared_nonlinear_full_matches_independent_stereo_pcm(){static_case(false);}
#[test]fn normal_six_channel_shared_nonlinear_streamed_matches_independent_stereo_pcm(){static_case(true);}
#[test]fn initially_zero_upper_matrix_output_remains_live_on_same_held_sources(){let f=Fixture::new();let(target,reference_a)=paths(&f,true);let(_,_,_,sum)=sources();let mut changed=matrix();changed[4][2]=1;let reference_b=f.program("changed-reference",&[("changed",reference(&sum,changed,false),false)],1.,None,true);let mut actual=Session::new(&target,false,true);let mut a=Session::new(&reference_a,false,false);let mut b=Session::new(&reference_b,false,false);let first=actual.render(4123);compare(&first,&a.render(4123));b.render(4123);let mut ids=Vec::new();actual.rt.visit_processing_owners(|owner|ids.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));let bound=actual.ir.source_indices.processing_matrices[0].compiled.as_ref().unwrap();let control=bound.cells.iter().find(|cell|cell.input==2&&cell.output==4).unwrap().control;let id=sampler_core::lower::ir_control_id(&actual.ir.controls[control.0].key);actual.write(id,0.25);let after=actual.render(4123);let expected=b.render(4123);compare(&after,&expected);assert!(after.iter().flatten().zip(a.render(4123).iter().flatten()).any(|(a,b)|(a-b).abs()>1e-4));let mut current=Vec::new();actual.rt.visit_processing_owners(|owner|current.push((owner.id,owner.note,owner.plan,owner.template,owner.sources)));assert_eq!(ids,current);actual.close();a.close();b.close();}
#[test]fn matrix_bypass_uses_full_dry_port_without_retrigger_or_source_pruning(){let f=Fixture::new();let(target,_)=paths(&f,true);let(_,_,_,sum)=sources();let reference=f.program("matrix-dry-reference",&[("matrix-dry",reference(&sum,matrix(),true),false)],1.,None,true);let mut actual=Session::new(&target,false,true);let mut expected=Session::new(&reference,false,false);actual.render(4096);expected.render(4096);let note=actual.note;actual.bypass(actual.matrix.unwrap(),true);let a=actual.render(4096);let b=expected.render(4096);compare(&a[1024..],&b[1024..]);assert_eq!(actual.note,note);assert_eq!(actual.rt.voice_count(),2);actual.close();expected.close();}
#[test]fn retained_onepole_bypass_rests_and_resumes_shared_history(){let f=Fixture::new();let(target,reference)=paths(&f,false);let mut actual=Session::new(&target,false,true);let mut expected=Session::new(&reference,false,false);for bypass in [None,Some(true),Some(false),Some(true),Some(false)]{if let Some(bypass)=bypass{actual.bypass(actual.filter,bypass);expected.bypass(expected.filter,bypass);}let a=actual.render(4096);let b=expected.render(4096);compare(&a,&b);}assert_eq!(actual.rt.voice_count(),2);actual.close();expected.close();}
#[test]fn absent_wide_caller_port_refuses_without_first_two_channel_fallback(){let f=Fixture::new();let(target,_)=paths(&f,true);let mut translated=sampler_uvi::translate_path(&target).unwrap();translated.instrument.source_indices.uvi_native_processing.clear();let error=sampler_uvi::assemble_translated(translated,RATE).err().unwrap().to_string();assert!(error.contains("explicit caller output matrix"),"{error}");}
#[test]fn mismatched_caller_port_refuses_before_any_runtime_publication(){let f=Fixture::new();let(target,_)=paths(&f,true);let mut translated=sampler_uvi::translate_path(&target).unwrap();let owner=translated.instrument.source_indices.processing[0].owner;let mut conversion=projection();conversion.input.channels=5;conversion.cells.truncate(10);translated.instrument.source_indices.processing_ports.push(ir::SourceProcessingPort{owner,conversion});let error=sampler_uvi::assemble_translated(translated,RATE).err().unwrap().to_string();assert!(error.contains("actual source working layout"),"{error}");}


// Normal reader/global-owner tests use ordinary authored stereo files. Core
// callback allocation probes remain in Core's established permitted harness.
struct GlobalSession {rt:Runtime,ir:ir::Instrument,note:NoteId,layer:ir::SourceOwnerId,program:ir::SourceOwnerId,_keep:Option<(sampler_kontakt::Streamer,Vec<sampler_core::Pcm>)>,horizon:Option<u32>}
impl GlobalSession {
    fn new(path:&Path,stream:bool)->Self {
        let translated=sampler_uvi::translate_path(path).unwrap();
        let (loaded,cache,keep,horizon)=if stream{let value=sampler_uvi::assemble_translated_streamed(translated,RATE,&Default::default()).unwrap();let horizon=u32::try_from(value.report.head_frames.max(sampler_core::PAGE_FRAMES)+127).unwrap();(value.loaded,Some(value.cache),Some((value.streamer,value.assets)),Some(horizon))}else{(sampler_uvi::assemble_translated(translated,RATE).unwrap(),None,None,None)};
        let layer=loaded.instrument.source_indices.owners.iter().find(|row|row.kind==ir::SourceOwnerKind::Layer).unwrap().id;let program=loaded.instrument.source_indices.owners.iter().find(|row|row.kind==ir::SourceOwnerKind::Instrument).unwrap().id;
        let limits=Limits::for_plan(&loaded.plan,8,16);let mut rt=Runtime::new(loaded.plan,limits).unwrap();if let Some(cache)=cache{rt=rt.with_stream_cache(cache);rt.set_stream_horizon(horizon.unwrap()).unwrap();rt.set_offline(true);}
        let note=rt.trigger(input(),60,0.75).unwrap();Self{rt,ir:loaded.instrument,note,layer,program,_keep:keep,horizon}
    }
    fn render(&mut self,frames:usize)->Vec<[f32;2]>{let mut out=vec![[0.;2];frames];for block in out.chunks_mut(64){if let Some(horizon)=self.horizon{let now=self.rt.now();self.rt.wait_streaming(horizon,Duration::from_secs(2)).unwrap();assert_eq!(now,self.rt.now());}self.rt.render(block).unwrap();}let stats=self.rt.stats();assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.stream_underruns,0);out}
    fn owner_write(&mut self,owner:ir::SourceOwnerId,field:sampler_core::SourceOwnerControl,value:ControlValue){let plan=self.rt.active_plan();let id=self.rt.source_owner_control(plan,owner,field).unwrap();self.rt.edit_controls(plan,None,&[ControlWrite{id,value}]).unwrap();}
}
fn global_program(f:&Fixture,name:&str,program:&str,layer:&str,kg_fx:&str,layer_fx:&str,routers:&str,aux:&str,samples:&[[i16;2]])->PathBuf {
    std::fs::write(f.0.join(format!("{name}.wav")),wav(samples)).unwrap();let xml=format!(r#"<Program {program}><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() end]]></script></ScriptProcessor></EventProcessors>{aux}<Layers><Layer {layer}>{routers}<Inserts>{layer_fx}</Inserts><Keygroups><Keygroup LowKey="60" HighKey="60" Gain="1" FXPostGain="0"><Inserts>{kg_fx}</Inserts><Oscillators><SamplePlayer SamplePath="{name}.wav" BaseNote="60" Gain="1"><PlaybackOptions Start="0" Stop="2048"><Loop Start="0" End="2048" Type="0"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);let path=f.0.join(format!("{name}.uvip"));std::fs::write(&path,xml).unwrap();path
}
#[test]
fn normal_layer_pan_is_after_same_kg_matrix_and_not_prefolded_into_sources(){
    let f=Fixture::new();let source=vec![[4096,-8192];FRAMES];let expected=vec![[4096,4096];FRAMES];
    let kg=r#"<GainMatrix Gain_1_1="0" Gain_2_1="1" Gain_1_2="1" Gain_2_2="0"/><WaveShaper Mode="6" InputGain="0" OutputGain="0"/>"#;
    let target=global_program(&f,"scope-pan-target",r#"Gain="2""#,r#"Gain="0.5" Pan="0.5""#,kg,"","","",&source);
    let reference=global_program(&f,"scope-pan-reference",r#"Gain="1""#,r#"Gain="1" Pan="0""#,"","","","",&expected);
    let mut actual=GlobalSession::new(&target,false);let mut reference=GlobalSession::new(&reference,false);compare(&actual.render(4096),&reference.render(4096));assert_eq!(actual.rt.voice_count(),1);
}
fn zero_global_gain_case(program:bool,stream:bool){
    let f=Fixture::new();let samples=vec![[4096,-8192];FRAMES];let(pa,la)=if program{(r#"Gain="0""#,r#"Gain="1""#)}else{(r#"Gain="1""#,r#"Gain="0""#)};
    let target=global_program(&f,"zero-global-target",pa,la,"","","","",&samples);let reference=global_program(&f,"zero-global-reference",r#"Gain="1""#,r#"Gain="1""#,"","","","",&samples);
    let mut actual=GlobalSession::new(&target,stream);let mut reference=GlobalSession::new(&reference,stream);assert!(actual.render(257).iter().flatten().all(|v|*v==0.));reference.render(257);let note=actual.note;let mut before=Vec::new();actual.rt.visit_stream_demand(1,|d|{before.push((d.voice,d.key.asset,d.source_frame));true}).unwrap();
    let owner=if program{actual.program}else{actual.layer};actual.owner_write(owner,sampler_core::SourceOwnerControl::Gain,ControlValue::Real(1.));let mut after=Vec::new();actual.rt.visit_stream_demand(1,|d|{after.push((d.voice,d.key.asset,d.source_frame));true}).unwrap();assert_eq!(before,after);compare(&actual.render(257),&reference.render(257));assert_eq!(note,actual.note);assert_eq!(actual.rt.voice_count(),1);
}
#[test]fn normal_program_zero_gain_opens_same_held_source_without_ratio_floor(){zero_global_gain_case(true,false);}
#[test]fn normal_layer_zero_gain_opens_same_streamed_source_without_ratio_floor(){zero_global_gain_case(false,true);}
fn output_mute_global_case(stream:bool){
    let f=Fixture::new();let samples=vec![[4096,-8192];FRAMES];let filter=r#"<OnePole Freq="200" Mode="0" Bypass="0"/>"#;
    let target=global_program(&f,"retained-muted-layer",r#"Gain="1""#,r#"Gain="1" Mute="1" MidiMute="0""#,"",filter,"","",&samples);let reference=global_program(&f,"unmuted-history-reference",r#"Gain="1""#,r#"Gain="1" Mute="0" MidiMute="0""#,"",filter,"","",&samples);
    let mut actual=GlobalSession::new(&target,stream);let mut reference=GlobalSession::new(&reference,stream);assert_eq!(actual.ir.assets.len(),1);assert_eq!(actual.ir.zones.len(),1);assert!(actual.ir.processing_buses.iter().filter(|row|row.owner==Some(actual.layer)).count()>=2);
    assert!(actual.render(257).iter().flatten().all(|v|*v==0.));assert!(reference.render(257).iter().flatten().any(|v|v.abs()>0.01));let note=actual.note;assert_eq!(actual.rt.voice_count(),1);actual.owner_write(actual.layer,sampler_core::SourceOwnerControl::OutputMute,ControlValue::Toggle(false));compare(&actual.render(257),&reference.render(257));assert_eq!(note,actual.note);assert_eq!(actual.rt.voice_count(),1);
    actual.owner_write(actual.layer,sampler_core::SourceOwnerControl::OutputMute,ControlValue::Toggle(true));actual.rt.release(actual.note).unwrap();reference.rt.release(reference.note).unwrap();assert!(actual.render(32).iter().flatten().all(|v|*v==0.));reference.render(32);actual.owner_write(actual.layer,sampler_core::SourceOwnerControl::OutputMute,ControlValue::Toggle(false));let a=actual.render(257);let b=reference.render(257);compare(&a,&b);assert_eq!(actual.rt.voice_count(),0);
}
#[test]fn saved_muted_layer_keeps_held_source_and_one_global_history(){output_mute_global_case(false);}
#[test]fn saved_muted_layer_keeps_streamed_source_and_one_global_history(){output_mute_global_case(true);}
#[test]
fn normal_pre_fader_send_uses_shared_fx_outlet_and_post_send_uses_gain_outlet(){
    let f=Fixture::new();let samples=vec![[4096,-8192];FRAMES];let routers=r#"<BusRouters><BusRouter Name="pre" Destination="../../Aux0" Gain="0.5" PreFader="1" Bypass="0"/><BusRouter Name="post" Destination="../../Aux0" Gain="0.25" PreFader="0" Bypass="0"/></BusRouters>"#;let aux=r#"<Auxs><AuxEffect Name="Aux0" Gain="1" Bypass="0"/></Auxs>"#;
    let target=global_program(&f,"global-real-sends",r#"Gain="1""#,r#"Gain="0""#,"","",routers,aux,&samples);let mut actual=GlobalSession::new(&target,false);let a=actual.render(257);for frame in a{assert!((frame[0]-0.0625).abs()<3e-7);assert!((frame[1]+0.125).abs()<3e-7);}actual.owner_write(actual.layer,sampler_core::SourceOwnerControl::Gain,ControlValue::Real(1.));let b=actual.render(257);for frame in b{assert!((frame[0]-0.21875).abs()<3e-7);assert!((frame[1]+0.4375).abs()<3e-7);}assert_eq!(actual.rt.voice_count(),1);assert!(actual.ir.groups.iter().all(|group|group.sends.is_empty()));
}
#[test]
fn normal_output_mute_and_midimute_retain_distinct_actual_toggle_controls(){
    let f=Fixture::new();let target=global_program(&f,"distinct-mute-fields",r#"Gain="1""#,r#"Gain="1" Mute="1" MidiMute="0""#,"","","","",&vec![[4096;2];FRAMES]);let actual=GlobalSession::new(&target,false);let plan=actual.rt.active_plan();let output=actual.rt.source_owner_control(plan,actual.layer,sampler_core::SourceOwnerControl::OutputMute).unwrap();let reception=actual.rt.source_owner_control(plan,actual.layer,sampler_core::SourceOwnerControl::MidiMute).unwrap();assert_ne!(output,reception);assert_eq!(actual.rt.control_value(plan,output),Ok(ControlValue::Toggle(true)));assert_eq!(actual.rt.control_value(plan,reception),Ok(ControlValue::Toggle(false)));let row=actual.rt.source_owners(plan).unwrap().rows().iter().find(|row|row.id==actual.layer).unwrap();assert_eq!(row.mute.as_ref().unwrap().control,Some(reception));
}


#[test]
fn normal_distinct_layer_output_homes_do_not_alias_sibling_or_program_gain(){
    let f=Fixture::new();std::fs::write(f.0.join("actual-a.wav"),wav(&vec![[4096,8192];FRAMES])).unwrap();std::fs::write(f.0.join("actual-b.wav"),wav(&vec![[2048,4096];FRAMES])).unwrap();
    let layer=|key:u8,file:&str|format!(r#"<Layer Gain="1"><Keygroups><Keygroup LowKey="{key}" HighKey="{key}" Gain="1"><Oscillators><SamplePlayer SamplePath="{file}" BaseNote="{key}" Gain="1"><PlaybackOptions Start="0" Stop="2048"><Loop Start="0" End="2048" Type="0"/></PlaybackOptions></SamplePlayer></Oscillators></Keygroup></Keygroups></Layer>"#);
    let xml=format!(r#"<Program Gain="1"><Layers>{}{}</Layers></Program>"#,layer(60,"actual-a.wav"),layer(61,"actual-b.wav"));let path=f.0.join("separate-real-layer-homes.uvip");std::fs::write(&path,xml).unwrap();let mut actual=GlobalSession::new(&path,false);let layers:Vec<_>=actual.ir.source_indices.owners.iter().filter(|row|row.kind==ir::SourceOwnerKind::Layer).map(|row|row.id).collect();assert_eq!(layers.len(),2);
    let other=actual.rt.trigger(Input{key:61,..input()},61,0.75).unwrap();let plan=actual.rt.active_plan();let field=sampler_core::SourceOwnerControl::Gain;let a=actual.rt.source_owner_control(plan,layers[0],field).unwrap();let b=actual.rt.source_owner_control(plan,layers[1],field).unwrap();let program=actual.rt.source_owner_control(plan,actual.program,field).unwrap();assert_ne!(a,b);assert_ne!(a,program);assert_ne!(b,program);
    let check=|samples:&[[f32;2]],expected:[f32;2]|{for frame in samples{for c in 0..2{assert!((frame[c]-expected[c]).abs()<4e-7);}}};check(&actual.render(257),[0.1875,0.375]);
    actual.owner_write(layers[0],field,ControlValue::Real(0.));check(&actual.render(257),[0.0625,0.125]);assert_eq!(actual.rt.control_value(plan,b),Ok(ControlValue::Real(1.)));assert_eq!(actual.rt.control_value(plan,program),Ok(ControlValue::Real(1.)));assert_eq!(actual.rt.voice_count(),2);
    actual.owner_write(layers[1],field,ControlValue::Real(0.5));check(&actual.render(257),[0.03125,0.0625]);actual.owner_write(actual.program,field,ControlValue::Real(2.));check(&actual.render(257),[0.0625,0.125]);assert_eq!(actual.rt.control_value(plan,a),Ok(ControlValue::Real(0.)));assert_eq!(actual.rt.control_value(plan,b),Ok(ControlValue::Real(0.5)));assert_eq!(actual.rt.voice_count(),2);
    actual.rt.release(actual.note).unwrap();actual.rt.release(other).unwrap();actual.render(8192);assert_eq!(actual.rt.voice_count(),0);assert_eq!(actual.rt.processing_owner_count(),0);
}

fn scope_route_fixture(f:&Fixture,stream:bool) {
    let(target,reference)=paths(f,false);
    let connections=r#"<Connections><SignalConnection Source="@VoiceParam Velocity" Destination="Gain" Ratio="1"/><SignalConnection Source="@VoiceParam Velocity" Destination="Pan" Ratio="0.5"/></Connections>"#;
    let xml=std::fs::read_to_string(&target).unwrap().replace("<Inserts>",&format!("{connections}<Inserts>"));std::fs::write(&target,xml).unwrap();
    let xml=std::fs::read_to_string(&reference).unwrap().replace("Keygroup Gain=\"1\"","Keygroup Gain=\"0.75\" Pan=\"0.375\"");std::fs::write(&reference,xml).unwrap();
    let mut actual=Session::new(&target,stream,true);let mut expected=Session::new(&reference,stream,false);
    let recipe=&actual.ir.source_indices.processing[0];assert_eq!(recipe.routes.len(),2);assert_eq!(recipe.parameter_roles.len(),2);assert!(recipe.parameter_roles.iter().all(|role|matches!(role.compiled,Some(ir::ScopeControlSite::SignalNode(_)))));
    for zone in &actual.ir.zones{assert!(!zone.routes.iter().any(|route|recipe.routes.contains(route)),"shared routes must not be copied into player state");}
    let owner=recipe.owner;
    compare(&actual.render(8192),&expected.render(8192));assert_eq!(actual.rt.processing_owner_count(),1);assert_eq!(actual.rt.voice_count(),2);
    let pan=actual.rt.source_owner_control(actual.rt.active_plan(),owner,sampler_core::SourceOwnerControl::Pan).unwrap();
    actual.write(pan,-0.75);let pan=expected.rt.source_owner_control(expected.rt.active_plan(),owner_from_first_recipe(&expected.ir),sampler_core::SourceOwnerControl::Pan).unwrap();expected.write(pan,-0.375);
    compare(&actual.render(8192),&expected.render(8192));actual.close();expected.close();
}
fn owner_from_first_recipe(instrument:&ir::Instrument)->ir::SourceOwnerId{instrument.source_indices.processing[0].owner}
#[test]fn normal_wide_shared_gain_pan_program_full_matches_independent_static_route_values(){let f=Fixture::new();scope_route_fixture(&f,false);}
#[test]fn normal_wide_shared_gain_pan_program_streamed_matches_independent_static_route_values(){let f=Fixture::new();scope_route_fixture(&f,true);}
#[test]
fn normal_owner_lfo_is_partition_invariant_and_not_duplicated_per_source() {
    let f=Fixture::new();let(target,_)=paths(&f,true);
    let source=r#"<ControlSignalSources><LFO Name="owner-lfo" Freq="17.3" Depth="0.8" Bipolar="1" Retrigger="1" WaveFormType="0" Phase="0.2"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/owner-lfo" Destination="Pan" Ratio="0.5"/></Connections>"#;
    let xml=std::fs::read_to_string(&target).unwrap().replace("<Inserts>",&format!("{source}<Inserts>"));std::fs::write(&target,xml).unwrap();
    let mut fragmented=Session::new(&target,false,true);let mut whole=Session::new(&target,false,true);
    assert_eq!(fragmented.ir.source_indices.processing[0].routes.len(),1);assert!(fragmented.ir.zones.iter().all(|zone|zone.routes.is_empty()));
    let a=fragmented.render(8191);let mut b=vec![[0.;2];8191];whole.rt.render(&mut b).unwrap();whole.health();compare(&a,&b);
    assert_eq!(fragmented.rt.processing_owner_count(),1);assert_eq!(fragmented.rt.voice_count(),2);fragmented.close();whole.close();
}

// Reference: ordinary normal-reader source assets and shared nonlinear graph,
// but the Pitch clock is explicitly the established local Core source path.
// It neither reads provider endpoints nor calls the new evaluator.
fn ordinary_pitch_reference(path:&Path,stream:bool)->Session {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget};
    let mut translated=sampler_uvi::translate_path(path).unwrap();
    translated.instrument.source_indices.processing_ports=translated.instrument.source_indices.processing.iter().map(|recipe|ir::SourceProcessingPort{owner:recipe.owner,conversion:projection()}).collect();
    let xml=std::fs::read_to_string(path).unwrap();let document=roxmltree::Document::parse(&xml).unwrap();
    let filter=u32::try_from(document.descendants().find(|n|n.has_tag_name("OnePole")).unwrap().id().get_usize()).unwrap();
    let matrix=translated.instrument.source_indices.processing_matrices.first().map(|m|m.node);
    let (loaded,cache,keep,horizon)=if stream{let value=sampler_uvi::assemble_translated_streamed(translated,RATE,&Default::default()).unwrap();let horizon=u32::try_from(value.report.head_frames.max(sampler_core::PAGE_FRAMES)+127).unwrap();(value.loaded,Some(value.cache),Some((value.streamer,value.assets)),Some(horizon))}else{(sampler_uvi::assemble_translated(translated,RATE).unwrap(),None,None,None)};
    let mut route=ModRoute::new(0,ModTarget::Pitch,0.2);route.shape=Some(0);
    let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Native(ir::NativeWave::Uvi(ir::UviWave::Sinus)),rate:LfoRate::Hertz(17.3),phase:0.,delay:0,fade:0,retrigger:true,shared:false})],routes:vec![route],shapes:vec![vec![(0.,0.1),(1.,0.9)]],..Default::default()};
    let count=loaded.instrument.zones.len();let plan=loaded.plan.with_voice_modulation(vec![program],vec![Some(0);count],vec![0;count]).unwrap();
    let limits=Limits::for_plan(&plan,8,16);let mut rt=Runtime::new(plan,limits).unwrap();if let Some(cache)=cache{rt=rt.with_stream_cache(cache);rt.set_stream_horizon(horizon.unwrap()).unwrap();rt.set_offline(true);}
    let note=rt.trigger(input(),60,0.75).unwrap();Session{rt,ir:loaded.instrument,note,filter,matrix,_keep:keep,horizon}
}
fn normal_provider_cross_use_case(stream:bool){
    let f=Fixture::new();let(target,_)=paths(&f,true);
    // EXACT original refused input, retained as real positive playback.
    let source=r#"<ControlSignalSources><LFO Name="shared-source" Freq="17.3" Depth="0.8" Bipolar="1" Retrigger="1" WaveFormType="0"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/shared-source" Destination="Pan" Ratio="0.5"/></Connections>"#;
    let physical=r#"<Connections><SignalConnection Source="$Keygroup/shared-source" Destination="Pitch" Ratio="0.2"/></Connections>"#;
    let xml=std::fs::read_to_string(&target).unwrap().replace("<Inserts>",&format!("{source}<Inserts>"))
        .replace("<PlaybackOptions",&format!("{physical}<PlaybackOptions"));std::fs::write(&target,&xml).unwrap();
    let reference_path=f.0.join("independent-local-source.uvip");std::fs::write(&reference_path,xml.replace(physical,"")).unwrap();
    let mut actual=Session::new(&target,stream,true);let mut whole=Session::new(&target,stream,true);let mut expected=ordinary_pitch_reference(&reference_path,stream);
    let recipe=&actual.ir.source_indices.processing[0];assert_eq!(recipe.routes.len(),1);
    let source=actual.ir.routes[recipe.routes[0].0].source;assert!(matches!(actual.ir.modulators[source.0].source,ir::ModulationSource::Lfo(_)));
    assert!(actual.ir.zones.iter().all(|zone|zone.routes.iter().all(|route|actual.ir.routes[route.0].target==ir::Target::Pitch&&actual.ir.routes[route.0].source==source)));
    let a=actual.render(8191);let b=expected.render(8191);compare(&a,&b);
    let mut c=vec![[0.;2];8191];if stream{c=whole.render(8191);}else{whole.rt.render(&mut c).unwrap();whole.health();}compare(&a,&c);
    assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(2,1));let mut owners=Vec::new();actual.rt.visit_processing_owners(|owner|owners.push((owner.id,owner.note,owner.plan)));assert_eq!(owners.len(),1);
    actual.close();whole.close();expected.close();
}
#[test]
fn normal_player_cross_use_of_shared_stateful_source_requires_real_provider(){normal_provider_cross_use_case(false);}
#[test]
fn normal_streamed_player_cross_use_of_shared_stateful_source_uses_the_same_real_provider(){normal_provider_cross_use_case(true);}


// Normal decode/materialization with an explicit typed Core rate overlay. The
// current native XML live-Freq parser law is NOT silently expanded by this test.
fn normal35_session(path:&Path,edit:impl FnOnce(&mut ir::Instrument),ordinary_rate_reference:bool,initial_cc:u32)->Session {
    use sampler_core::{Lfo,LfoRate,LfoShape,ModProgram,ModRoute,ModSource,ModTarget};
    let mut translated=sampler_uvi::translate_path(path).unwrap();edit(&mut translated.instrument);
    translated.instrument.source_indices.processing_ports=translated.instrument.source_indices.processing.iter().map(|recipe|ir::SourceProcessingPort{owner:recipe.owner,conversion:projection()}).collect();
    let xml=std::fs::read_to_string(path).unwrap();let document=roxmltree::Document::parse(&xml).unwrap();
    let filter=u32::try_from(document.descendants().find(|n|n.has_tag_name("OnePole")).unwrap().id().get_usize()).unwrap();let matrix=translated.instrument.source_indices.processing_matrices.first().map(|m|m.node);
    let loaded=sampler_uvi::assemble_translated(translated,RATE).unwrap();let count=loaded.instrument.zones.len();
    let plan=if ordinary_rate_reference{
        let rate=ModRoute::new(1,ModTarget::SourceRate(0),0.05);let mut pitch=ModRoute::new(0,ModTarget::Pitch,0.2);pitch.shape=Some(0);
        let program=ModProgram{sources:vec![ModSource::Lfo(Lfo{shape:LfoShape::Native(ir::NativeWave::Uvi(ir::UviWave::Sinus)),rate:LfoRate::Hertz(1.),phase:0.,delay:0,fade:0,retrigger:true,shared:false}),ModSource::Controller(1)],routes:vec![rate,pitch],shapes:vec![vec![(0.,0.1),(1.,0.9)]],
            controls:vec![(0,[sampler_core::lower::ir_control_id("provider35-rate-depth"),sampler_core::lower::ir_control_id("provider35-rate-bypass")])],..Default::default()};
        loaded.plan.with_voice_modulation(vec![program],vec![Some(0);count],vec![0;count]).unwrap()
    }else{loaded.plan};
    let limits=Limits::for_plan(&plan,8,16);let mut rt=Runtime::new(plan,limits).unwrap();if initial_cc!=0{let performance=rt.performance(0).unwrap();rt.dispatch_controller(performance,sampler_core::ChannelAddress{protocol:Protocol::Native,port:0,group:0,channel:0},1,1,initial_cc).unwrap();}
    let note=rt.trigger(input(),60,0.75).unwrap();Session{rt,ir:loaded.instrument,note,filter,matrix,_keep:None,horizon:None}
}
fn normal35_rate_overlay(instrument:&mut ir::Instrument){
    let source=instrument.routes[instrument.source_indices.processing[0].routes[0].0].source;
    let cc=ir::ModulatorRef(instrument.modulators.len());instrument.modulators.push(ir::Modulator{scope:ir::Scope::Voice,source:ir::ModulationSource::Controller(1)});
    let route=ir::RouteRef(instrument.routes.len());instrument.routes.push(ir::Route{source:cc,target:ir::Target::SourceRate(source),depth:ir::Depth::Normalized(0.05),invert:false,shape:None,smoothing:ir::Time::ZERO,scale:None});
    let depth=ir::ControlRef(instrument.controls.len());instrument.controls.push(ir::Control{key:"provider35-rate-depth".into(),label:"typed source rate depth".into(),value:ir::ControlValue::Continuous{min:0.,max:0.25,default:0.05,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    let bypass=ir::ControlRef(instrument.controls.len());instrument.controls.push(ir::Control{key:"provider35-rate-bypass".into(),label:"typed source rate bypass".into(),value:ir::ControlValue::Continuous{min:0.,max:1.,default:0.,unit:ir::ControlUnit::None},automation:ir::Automation::None});
    instrument.source_indices.route_controls.push((route,depth,bypass));instrument.source_indices.processing[0].routes.push(route);
}
#[test]
fn normal_decoded_owner_first_forecast_uses_same_frame_cc_and_live_depth_without_pcm(){
    for live_depth in [false,true]{
        let f=Fixture::new();let(target,_)=paths(&f,true);
        let source=r#"<ControlSignalSources><LFO Name="shared-source" Freq="1" Depth="0.8" Bipolar="1" Retrigger="1" WaveFormType="0"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/shared-source" Destination="Pan" Ratio="0.5"/></Connections>"#;
        let physical=r#"<Connections><SignalConnection Source="$Keygroup/shared-source" Destination="Pitch" Ratio="0.2"/></Connections>"#;
        let xml=std::fs::read_to_string(&target).unwrap().replace("<Inserts>",&format!("{source}<Inserts>")).replace("<PlaybackOptions",&format!("{physical}<PlaybackOptions"));std::fs::write(&target,&xml).unwrap();
        let reference_path=f.0.join("normal35-rate-reference.uvip");std::fs::write(&reference_path,xml.replace(physical,"")).unwrap();let initial_cc=if live_depth{u32::MAX}else{0};
        let mut actual=normal35_session(&target,normal35_rate_overlay,false,initial_cc);let mut reference=normal35_session(&reference_path,normal35_rate_overlay,true,initial_cc);
        for session in [&mut actual,&mut reference]{
            if live_depth{let id=sampler_core::lower::ir_control_id("provider35-rate-depth");session.write(id,0.2);}
            else{let performance=session.rt.performance(0).unwrap();session.rt.dispatch_controller(performance,sampler_core::ChannelAddress{protocol:Protocol::Native,port:0,group:0,channel:0},1,1,u32::MAX).unwrap();}
            assert_eq!(session.rt.now(),0);session.rt.render(&mut []).unwrap();assert_eq!(session.rt.now(),0);
        }
        compare(&actual.render(1025),&reference.render(1025));assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(2,1));actual.close();reference.close();
    }
}
fn normal35_is_player_source(source:&ir::ModulationSource)->bool{
    matches!(source,ir::ModulationSource::Envelope(_))||matches!(source,ir::ModulationSource::Lfo(lfo)if lfo.shape==ir::LfoShape::Native(ir::NativeWave::Uvi(ir::UviWave::RandomHold)))
}
#[test]
fn normal_player_local_random_lfos_and_envelopes_keep_ordinary_state_seed_and_retrigger(){
    for mixed_kg in [false,true]{for reversed in [false,true]{
        let f=Fixture::new();let(target,_)=paths(&f,true);
        let local=r#"<ControlSignalSources><LFO Name="local-random" Freq="7" Depth="0.8" Bipolar="1" Retrigger="1" WaveFormType="6"/><DAHDSR Name="local-eg" AttackTime="0.002" DecayTime="0.003" SustainLevel="0.5" ReleaseTime="0.01" VelocityAmount="0"/></ControlSignalSources><Connections><SignalConnection Source="local-random" Destination="Pitch" Ratio="0.4"/><SignalConnection Source="local-eg" Destination="Pitch" Ratio="0.2"/></Connections>"#;
        let mut xml=std::fs::read_to_string(&target).unwrap().replace("<PlaybackOptions",&format!("{local}<PlaybackOptions"));
        if mixed_kg{
            let kg=r#"<ControlSignalSources><LFO Name="real-kg" Freq="17.3" Depth="0.8" Bipolar="1" Retrigger="1" WaveFormType="0"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/real-kg" Destination="Pan" Ratio="0.5"/></Connections>"#;
            xml=xml.replace("<Inserts>",&format!("{kg}<Inserts>")).replace(r#"<Connections><SignalConnection Source="local-random""#,r#"<Connections><SignalConnection Source="$Keygroup/real-kg" Destination="Pitch" Ratio="0.2"/><SignalConnection Source="local-random""#);
        }
        if reversed{
            let document=roxmltree::Document::parse(&xml).unwrap();let ranges:Vec<_>=document.descendants().filter(|node|node.has_tag_name("SamplePlayer")).take(2).map(|node|node.range()).collect();let a=&ranges[0];let b=&ranges[1];drop(document);
            xml=format!("{}{}{}{}{}",&xml[..a.start],&xml[b.clone()],&xml[a.end..b.start],&xml[a.clone()],&xml[b.end..]);
        }
        std::fs::write(&target,&xml).unwrap();
        let mut actual=normal35_session(&target,|_|{},false,0);
        // Reference bypasses ONLY the new automatic ownership classification;
        // all actual normal-reader assets/players/routes/nonlinear DSP remain.
        let mut reference=normal35_session(&target,|instrument|{
            let local:Vec<_>=instrument.modulators.iter().enumerate().filter(|(_,row)|normal35_is_player_source(&row.source)).map(|(index,_)|ir::ModulatorRef(index)).collect();
            for recipe in &mut instrument.source_indices.processing{recipe.sources.retain(|source|!local.contains(source));}
        },false,0);
        assert_eq!((actual.ir.zones.len(),actual.ir.assets.len()),(3,3),"saved-muted third player and its definitions remain retained");
        let locals:Vec<_>=actual.ir.modulators.iter().enumerate().filter(|(_,row)|normal35_is_player_source(&row.source)).map(|(index,_)|ir::ModulatorRef(index)).collect();assert_eq!(locals.len(),6,"distinct genuine local LFO+EG per retained player");
        for _ in 0..2{
            compare(&actual.render(4097),&reference.render(4097));assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(2,1));
            let roots=&actual.ir.source_indices.processing[0].sources;assert_eq!(roots.len(),usize::from(mixed_kg));assert!(roots.iter().all(|source|!locals.contains(source)),"player-local definitions must not get a KG provider/state row");
            actual.rt.release(actual.note).unwrap();reference.rt.release(reference.note).unwrap();actual.render(8192);reference.render(8192);actual.rt.flush_ended(|_|true);reference.rt.flush_ended(|_|true);assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(0,0));
            actual.note=actual.rt.trigger(input(),60,0.75).unwrap();reference.note=reference.rt.trigger(input(),60,0.75).unwrap();
        }
        actual.close();reference.close();
    }}
}


// Definition identity is independent of the listener number. These use only
// actual XML references, ordinary WAV loading and existing note-script APIs;
// they do not register synthetic native property lanes or enable shared Gain/Pan.
fn normal40_script_program(f:&Fixture,name:&str,with_routes:bool,reverse_definitions:bool)->PathBuf {
    let samples:Vec<[i16;2]>=(0..FRAMES).map(|frame|{
        let left=if(frame/13)%2==0{1536}else{-1536};
        let right=if(frame/19)%2==0{1024}else{-1024};[left,right]
    }).collect();
    let a=r#"<ScriptEventModulation Name="A" EventId="7" Bipolar="0"/>"#;
    let b=r#"<ScriptEventModulation Name="B" EventId="7" Bipolar="0"/>"#;
    let definitions=if reverse_definitions{format!("{b}{a}")}else{format!("{a}{b}")};
    let mut players=String::new();
    for (index,(source,ratios))in [("A",vec![1,3]),("B",vec![4]),("A",vec![4])].into_iter().enumerate(){
        let file=format!("{name}-{index}.wav");std::fs::write(f.0.join(&file),wav(&samples)).unwrap();
        let connections=if with_routes{format!("<Connections>{}</Connections>",ratios.into_iter().map(|ratio|
            format!(r#"<SignalConnection Source="$Program/{source}" Destination="Pitch" Ratio="{ratio}"/>"#)).collect::<String>())}else{String::new()};
        players+=&format!(r#"<SamplePlayer Name="P{index}" SamplePath="{file}" BaseNote="60" Gain="1">{connections}<PlaybackOptions Start="0" Stop="2048"><Loop Start="0" End="2048" Type="0"/></PlaybackOptions></SamplePlayer>"#);
    }
    let xml=format!(r#"<Program Gain="1"><ControlSignalSources>{definitions}</ControlSignalSources><Layers><Layer Gain="1"><Keygroups><Keygroup LowKey="60" HighKey="64" Gain="1"><Oscillators>{players}</Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);
    let path=f.0.join(format!("{name}.uvip"));std::fs::write(&path,xml).unwrap();path
}
fn normal40_refs(instrument:&ir::Instrument)->(ir::ModulatorRef,ir::ModulatorRef){
    assert_eq!(instrument.zones.len(),3);
    let first=&instrument.zones[0].routes;assert_eq!(first.len(),2);
    let a=instrument.routes[first[0].0].source;
    assert_eq!(instrument.routes[first[1].0].source,a);
    let b=instrument.routes[instrument.zones[1].routes[0].0].source;
    assert_ne!(a,b,"two actual definitions with listener7 must not collapse");
    assert_eq!(instrument.routes[instrument.zones[2].routes[0].0].source,a,"cross-player reference must reuse actual definition A");
    assert_eq!(instrument.modulators.iter().filter(|row|matches!(row.source,ir::ModulationSource::Script(7))).count(),2);
    assert_eq!(instrument.modulators[a.0].source,ir::ModulationSource::Script(7));
    assert_eq!(instrument.modulators[b.0].source,ir::ModulationSource::Script(7));
    (a,b)
}
#[test]
fn normal_script_event_definition_identity_survives_cache_and_zone_narrowing(){
    for reversed in [false,true]{
        let f=Fixture::new();let path=normal40_script_program(&f,"identity-cache",true,reversed);
        let original=sampler_uvi::translate_path(&path).unwrap().instrument;
        let (a,b)=normal40_refs(&original);
        let encoded=serde_json::to_vec(&original).unwrap();
        let mut cached:ir::Instrument=serde_json::from_slice(&encoded).unwrap();
        assert_eq!(original.controls.len(),cached.controls.len());
        for (before,after) in original.controls.iter().zip(&cached.controls) {
            assert_eq!(before.key,after.key);
            if let (ir::ControlValue::Continuous{min:a,max:b,default:c,..},ir::ControlValue::Continuous{min:x,max:y,default:z,..})=(&before.value,&after.value) {
                assert_eq!([a.to_bits(),b.to_bits(),c.to_bits()],[x.to_bits(),y.to_bits(),z.to_bits()],
                    "cached scalar domain must preserve exact admitted f64 bits for {}: original={:?}, cached={:?}",before.key,before.value,after.value);
            }
        }
        assert_eq!(normal40_refs(&cached),(a,b));cached.validate().unwrap();
        let first_asset=cached.zones[0].asset.unwrap();
        let retained=cached.retain_zones(|zone|zone.asset!=Some(first_asset));
        assert_eq!(retained.len(),2);assert_eq!(cached.zones.len(),2);
        assert_eq!(cached.routes[cached.zones[0].routes[0].0].source,b);
        assert_eq!(cached.routes[cached.zones[1].routes[0].0].source,a);
        assert_ne!(a,b);cached.validate().unwrap();
        assert_eq!(cached.modulators[a.0].source,ir::ModulationSource::Script(7));
        assert_eq!(cached.modulators[b.0].source,ir::ModulationSource::Script(7));
    }
}
#[test]
fn normal_same_listener_definitions_and_references_preserve_independent_note_pitch_pcm(){
    for reversed in [false,true]{
        let f=Fixture::new();let target=normal40_script_program(&f,"identity-target",true,reversed);
        let reference=normal40_script_program(&f,"identity-reference",false,reversed);
        let loaded=sampler_uvi::assemble_translated(sampler_uvi::translate_path(&target).unwrap(),RATE).unwrap();
        normal40_refs(&loaded.instrument);
        let reference=sampler_uvi::assemble_translated(sampler_uvi::translate_path(&reference).unwrap(),RATE).unwrap();
        assert!(reference.instrument.zones.iter().all(|zone|zone.routes.is_empty()));
        let limits=Limits::for_plan(&loaded.plan,8,16);let mut actual=Runtime::new(loaded.plan,limits).unwrap();
        let limits=Limits::for_plan(&reference.plan,8,16);let mut expected=Runtime::new(reference.plan,limits).unwrap();
        // Independent PCM oracle: each imported player adds4*listenerValue
        // semitones. The reference has NO ScriptEvent routes or modulator
        // interpreter. Its ordinary per-note Pitch layer independently ramps
        // from0 to1/2 on the same declared64-frame control interval; setting
        // a ScriptEvent target after trigger does not rewrite its onset point.
        let a=actual.trigger(input(),60,0.75).unwrap();actual.set_note_script_value(a,7,0.25).unwrap();
        let ea=expected.trigger(input(),60,0.75).unwrap();expected.set_note_param(ea,sampler_core::ModTarget::Pitch,1.,false).unwrap();
        let other=Input{channel:1,..input()};
        let b=actual.trigger(other,60,0.75).unwrap();actual.set_note_script_value(b,7,0.5).unwrap();
        let eb=expected.trigger(other,60,0.75).unwrap();expected.set_note_param(eb,sampler_core::ModTarget::Pitch,2.,false).unwrap();
        assert_eq!(actual.now(),0);assert_eq!(expected.now(),0);
        let mut out=vec![[0.;2];1025];let mut reference=vec![[0.;2];1025];
        let mut offset=0;for frames in [1,17,63,64,65,127,128,256,304]{let end=offset+frames;actual.render(&mut out[offset..end]).unwrap();expected.render(&mut reference[offset..end]).unwrap();offset=end;}
        assert_eq!(offset,out.len());compare(&out,&reference);
        assert_eq!(actual.note_script_value(a,7),Ok(0.25));assert_eq!(actual.note_script_value(b,7),Ok(0.5));
        for (rt,notes)in [(&mut actual,[a,b]),(&mut expected,[ea,eb])]{
            assert_eq!(rt.voice_count(),6);for note in notes{rt.release(note).unwrap();}
            let mut tail=[[0.;2];64];for _ in 0..128{rt.render(&mut tail).unwrap();}
            rt.flush_ended(|_|true);assert_eq!(rt.voice_count(),0);assert_eq!(rt.processing_owner_count(),0);
            let stats=rt.stats();assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.stream_underruns,0);assert_eq!(stats.refused_starts,0);assert_eq!(stats.voice_pool_refusals,0);assert_eq!(stats.voice_drops,0);
        }
    }
}

// Profile65: ordinary normal loaders, NO injected caller matrix/profile.
fn native65_identity()->[[i16;6];6]{std::array::from_fn(|row|std::array::from_fn(|column|if row==column{4}else{0}))}
fn native65_sources()->(Vec<[i16;6]>,Vec<[i16;6]>,Vec<[i16;6]>){
    let a=(0..FRAMES).map(|frame|[if frame<128{2048}else{0},if frame<128{-1024}else{0},-512,256,if frame%41<20{2048}else{-2048},if frame%37<19{1536}else{-1536}]).collect();
    let b=(0..FRAMES).map(|frame|[if frame<128{-512}else{0},if frame<128{256}else{0},256,-128,if frame%29<14{-1024}else{1024},if frame%31<16{512}else{-512}]).collect();
    (a,b,vec![[8192;6];FRAMES])
}
// Independent original-code f32 recurrence and numeric descriptor6->2 law.
// Its arrays contain raw authored PCM, not data emitted by production DSP.
fn native65_reference(a:&[[i16;6]],b:&[[i16;6]],frames:usize,filtered:bool)->Vec<[f32;2]>{
    let a_coefficient=(-std::f64::consts::TAU*1800./f64::from(RATE)).exp()as f32;
    let b_coefficient=1.0f32-a_coefficient;let scale=f32::from_bits(0x3f13cd3a);let mut history=[0f32;6];
    (0..frames).map(|frame|{
        let mut signal=[0f32;6];
        for channel in 0..6 {
            let input=(f32::from(a[frame%a.len()][channel])/32768.+f32::from(b[frame%b.len()][channel])/32768.).abs();
            signal[channel]=if filtered{history[channel]+=(input-history[channel])*b_coefficient;history[channel]}else{input};
        }
        [signal[0]*scale+signal[4]*scale,signal[1]*scale+signal[5]*scale]
    }).collect()
}
fn native65_automatic_case(stream:bool){
    let f=Fixture::new();let(a,b,c)=native65_sources();
    let target=f.program("native-auto",&[("auto-a",a.clone(),false),("auto-b",b.clone(),false),("auto-c",c,true)],1.,Some(native65_identity()),false);
    let mut actual=Session::new(&target,stream,false);let output=actual.render(8192);let expected=native65_reference(&a,&b,8192,true);
    compare(&output,&expected);
    assert!(output[4096..].iter().flatten().any(|sample|sample.abs()>0.005),"late body in original4/5 must survive after first-pair attack ends");
    assert_eq!(actual.ir.assets.len(),3);assert_eq!(actual.ir.zones.len(),3);assert_eq!(actual.rt.voice_count(),2);assert_eq!(actual.rt.processing_owner_count(),1);
    assert_eq!(actual.ir.source_indices.processing_matrices[0].compiled.as_ref().unwrap().layout.channels,6);
    actual.close();
}
#[test]fn native65_normal_full_automatic_retained_three_player_sustain_matches_original_law(){native65_automatic_case(false);}
#[test]fn native65_normal_streamed_automatic_retained_three_player_sustain_matches_original_law(){native65_automatic_case(true);}
#[test]fn native65_wide_base_pan_has_real_home_and_is_ignored_without_retrigger(){
    let f=Fixture::new();let(a,b,c)=native65_sources();let path=f.program("native-pan",&[("pan-a",a.clone(),false),("pan-b",b.clone(),false),("pan-c",c,true)],1.,Some(native65_identity()),true);
    let mut actual=Session::new(&path,false,false);
    let first=actual.render(257);compare(&first,&native65_reference(&a,&b,257,false));
    let mut owners=Vec::new();actual.rt.visit_processing_owners(|owner|owners.push((owner.id,owner.note,owner.plan,owner.sources)));assert_eq!(owners.len(),1);
    let owner=actual.ir.source_indices.processing[0].owner;let plan=actual.rt.active_plan();let id=actual.rt.source_owner_control(plan,owner,sampler_core::SourceOwnerControl::Pan).unwrap();
    actual.write(id,0.75);let after=actual.render(2048);let expected=native65_reference(&a,&b,2305,false);compare(&after,&expected[257..]);
    let mut current=Vec::new();actual.rt.visit_processing_owners(|owner|current.push((owner.id,owner.note,owner.plan,owner.sources)));assert_eq!(owners,current);actual.close();
}
#[test]fn native65_true_mono_and_stereo_members_convert_into_six_before_shared_matrix(){
    let f=Fixture::new();std::fs::write(f.0.join("mono.wav"),wav(&vec![[4096i16];FRAMES])).unwrap();
    std::fs::write(f.0.join("pair.wav"),wav(&vec![[1024i16,2048];FRAMES])).unwrap();
    std::fs::write(f.0.join("six.wav"),wav(&vec![[512i16,256,0,0,1024,512];FRAMES])).unwrap();
    let players=["mono.wav","pair.wav","six.wav"].map(|name|format!(r#"<SamplePlayer SamplePath="{name}" BaseNote="60"><PlaybackOptions Start="0" Stop="2048"><Loop Start="0" End="2048" Type="0"/></PlaybackOptions></SamplePlayer>"#)).join("");
    let xml=format!(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[function onInit() end]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup LowKey="60" HighKey="60"><Inserts><GainMatrix Gain_3_5="0.5"/><OnePole Freq="1800" Bypass="1"/></Inserts><Oscillators>{players}</Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#);
    let path=f.0.join("mixed-native.uvip");std::fs::write(&path,xml).unwrap();let mut actual=Session::new(&path,false,false);
    let scale=f32::from_bits(0x3f13cd3a);let expected=[(1536f32/32768.)*scale+(3072f32/32768.)*scale,(2304f32/32768.)*scale+(512f32/32768.)*scale];
    let output=actual.render(512);compare(&output,&vec![expected;512]);assert_eq!(actual.rt.voice_count(),3);assert_eq!(actual.rt.processing_owner_count(),1);actual.close();
}

// Supplement67: automatic native wide path + canonical per-note expression.
// Expected PCM comes only from raw authored source arrays/native65_reference.
fn native67_owner_rows(rt:&Runtime)->Vec<(sampler_core::ProcessingOwnerId,NoteId,sampler_core::PlanId,usize)>{
    let mut rows=Vec::new();rt.visit_processing_owners(|owner|rows.push((owner.id,owner.note,owner.plan,owner.sources)));rows
}
fn native67_control_rows(rt:&Runtime,seed:ControlId)->(u64,Vec<ControlWrite>){
    let plan=rt.active_plan();let mut rows=vec![ControlWrite{id:seed,value:ControlValue::Real(0.)};rt.control_definitions(plan).unwrap().len()];
    let (count,revision)=rt.capture_controls(plan,&mut rows).unwrap();assert_eq!(count,rows.len());(revision,rows)
}
#[test]
fn native67_automatic_wide_two_notes_keep_expression_pan_after_conversion_and_fresh_note_independent(){
    let f=Fixture::new();let(a,b,c)=native65_sources();
    let path=f.program("native-expression",&[("expression-a",a.clone(),false),("expression-b",b.clone(),false),("expression-c",c,true)],1.,Some(native65_identity()),false);
    let mut actual=Session::new(&path,false,false);let first=actual.note;
    let second_source=Input{external_id:Some(6701),..input()};let second=actual.rt.trigger(second_source,60,0.75).unwrap();assert_ne!(first,second);
    let first_expression=actual.rt.expression_id(first).unwrap();let second_expression=actual.rt.expression_id(second).unwrap();assert_ne!(first_expression,second_expression);
    let untouched=actual.rt.expression(second_expression).unwrap();assert_eq!(untouched,sampler_core::Expression::default());
    // All notes use the same key/source, but independent real owners and histories.
    let reference=native65_reference(&a,&b,1024,true);let prefix=actual.render(257);
    let expected:Vec<_>=reference[..257].iter().map(|frame|[frame[0]+frame[0],frame[1]+frame[1]]).collect();compare(&prefix,&expected);
    assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(4,2));let owners=native67_owner_rows(&actual.rt);assert_eq!(owners.len(),2);
    let owner=actual.ir.source_indices.processing[0].owner;let plan=actual.rt.active_plan();let pan=actual.rt.source_owner_control(plan,owner,sampler_core::SourceOwnerControl::Pan).unwrap();
    let clock=actual.rt.now();actual.write(pan,0.75);assert_eq!(actual.rt.now(),clock);
    let base_check=actual.render(33);let expected:Vec<_>=reference[257..290].iter().map(|frame|[frame[0]+frame[0],frame[1]+frame[1]]).collect();compare(&base_check,&expected);
    assert_eq!(actual.rt.control_base_value(plan,pan),Ok(ControlValue::Real(0.75)),"native wide base Pan is a real scalar, but this converter ignores it");
    let controls=native67_control_rows(&actual.rt,pan);
    // Endpoint oracle needs no panner implementation: +1 silences only left.
    let clock=actual.rt.now();actual.rt.set_expression(first_expression,sampler_core::Expression{pan:1.,..Default::default()}).unwrap();assert_eq!(actual.rt.now(),clock);
    assert_eq!(actual.rt.expression(second_expression),Ok(untouched));assert_eq!(native67_owner_rows(&actual.rt),owners);assert_eq!(native67_control_rows(&actual.rt,pan),controls);
    let right=actual.render(65);let expected:Vec<_>=reference[290..355].iter().map(|frame|[frame[0],frame[1]+frame[1]]).collect();compare(&right,&expected);
    // -1 instead silences only right, without changing the sibling or any bus.
    let clock=actual.rt.now();actual.rt.set_expression(first_expression,sampler_core::Expression{pan:-1.,..Default::default()}).unwrap();assert_eq!(actual.rt.now(),clock);
    let left=actual.render(97);let expected:Vec<_>=reference[355..452].iter().map(|frame|[frame[0]+frame[0],frame[1]]).collect();compare(&left,&expected);
    assert_eq!(actual.rt.expression(second_expression),Ok(untouched));assert_eq!(native67_owner_rows(&actual.rt),owners);assert_eq!(native67_control_rows(&actual.rt,pan),controls);
    // A newly born actual note must not inherit the first note's expression or
    // filter history. Its raw-PCM/filter age starts0 while the others reach452.
    let third_source=Input{external_id:Some(6702),..input()};let clock=actual.rt.now();let third=actual.rt.trigger(third_source,60,0.75).unwrap();assert_eq!(actual.rt.now(),clock);
    let third_expression=actual.rt.expression_id(third).unwrap();assert_ne!(third_expression,first_expression);assert_ne!(third_expression,second_expression);assert_eq!(actual.rt.expression(third_expression),Ok(untouched));
    let fresh=actual.render(65);let expected:Vec<_>=reference[452..517].iter().zip(&reference[..65]).map(|(held,new)|[(held[0]+held[0])+new[0],held[1]+new[1]]).collect();compare(&fresh,&expected);
    let old_rows:Vec<_>=native67_owner_rows(&actual.rt).into_iter().filter(|row|row.1==first||row.1==second).collect();assert_eq!(old_rows,owners);
    assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(6,3));assert_eq!(actual.rt.now(),517);assert_eq!(native67_control_rows(&actual.rt,pan),controls);
    assert_eq!(actual.rt.note(first),Ok((60,0.75,true)));assert_eq!(actual.rt.note(second),Ok((60,0.75,true)));assert_eq!(actual.rt.note(third),Ok((60,0.75,true)));
    for note in [first,second,third]{actual.rt.release(note).unwrap();}actual.render(8192);let mut endings=[0usize;3];actual.rt.flush_ended(|source|{if source==input(){endings[0]+=1;}else if source==second_source{endings[1]+=1;}else if source==third_source{endings[2]+=1;}else{panic!("unexpected source owner: {source:?}");}true});
    assert_eq!(endings,[1;3]);assert_eq!((actual.rt.voice_count(),actual.rt.processing_owner_count()),(0,0));actual.health();
}
