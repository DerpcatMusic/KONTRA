//! Same authored bytes are used for baseline RED and candidate GREEN.
//! No installed bank, external encoder, native host or protected source access.
use sampler_core::{Input,Limits,Protocol,Runtime};
use sampler_ir::{KeyTracking,Looping};
use std::{path::{Path,PathBuf},sync::atomic::{AtomicU64,Ordering}};
const FRAMES:usize=128;
static NEXT:AtomicU64=AtomicU64::new(0);
struct Fixture(PathBuf);
impl Drop for Fixture {fn drop(&mut self) {let _=std::fs::remove_dir_all(&self.0);}}
impl Fixture {
    fn new()->Self {
        let scratch=std::env::var_os("TMPDIR").map(PathBuf::from).unwrap_or_else(||PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch"));
        std::fs::create_dir_all(&scratch).unwrap();
        let path=scratch.join(format!("uvi-file-loop-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();Self(path)
    }
    fn program(&self,format:&str,bytes:&[u8],options:&str)->PathBuf {
        std::fs::write(self.0.join(format!("tone.{format}")),bytes).unwrap();
        let xml=format!(r#"<Program><Layers><Layer><Keygroups><Keygroup LowKey="60" HighKey="60">
<ControlSignalSources><DAHDSR Name="Amp" AttackTime="0" DecayTime="0" SustainLevel="1" ReleaseTime="0" VelocityAmount="0"/></ControlSignalSources>
<Connections><SignalConnection Source="$Keygroup/Amp" Destination="Gain" Ratio="1"/></Connections>
<Oscillators><SamplePlayer SamplePath="tone.{format}" BaseNote="60" FineTune="0">{options}</SamplePlayer></Oscillators>
</Keygroup></Keygroups></Layer></Layers></Program>"#);
        let path=self.0.join("Public.uvip");std::fs::write(&path,xml).unwrap();path
    }
}
fn signal()->Vec<i16> {(0..FRAMES).map(|i|((i as f64*std::f64::consts::TAU/16.).sin()*8192.).round()as i16).collect()}
fn chunk(id:&[u8;4],data:&[u8],big:bool)->Vec<u8> {
    let mut out=id.to_vec();out.extend(if big {(data.len()as u32).to_be_bytes()}else{(data.len()as u32).to_le_bytes()});out.extend(data);if data.len()%2!=0 {out.push(0);}out
}
fn smpl(start:u32,end:u32,vendor:bool)->Vec<u8> {
    let mut out=vec![0;60];out[12..16].copy_from_slice(&64u32.to_le_bytes()); // deliberately differs from oscillator60
    out[16..20].copy_from_slice(&0x40000000u32.to_le_bytes()); // file fine tune25 cents is ignored
    out[28..32].copy_from_slice(&1u32.to_le_bytes());out[44..48].copy_from_slice(&start.to_le_bytes());out[48..52].copy_from_slice(&end.to_le_bytes());
    if vendor {out[32..36].copy_from_slice(&4u32.to_le_bytes());out.extend([1,2,3,4]);}out
}
fn wav(start:u32,end:u32,vendor:bool)->Vec<u8> {
    let mut fmt=vec![];fmt.extend(1u16.to_le_bytes());fmt.extend(1u16.to_le_bytes());fmt.extend(48000u32.to_le_bytes());fmt.extend(96000u32.to_le_bytes());fmt.extend(2u16.to_le_bytes());fmt.extend(16u16.to_le_bytes());
    let data:Vec<_>=signal().into_iter().flat_map(i16::to_le_bytes).collect();
    let mut out=b"RIFF\0\0\0\0WAVE".to_vec();out.extend(chunk(b"fmt ",&fmt,false));out.extend(chunk(b"smpl",&smpl(start,end,vendor),false));out.extend(chunk(b"data",&data,false));
    let n=(out.len()-8)as u32;out[4..8].copy_from_slice(&n.to_le_bytes());out
}
fn aiff()->Vec<u8> {
    let mut comm=vec![];comm.extend(1u16.to_be_bytes());comm.extend((FRAMES as u32).to_be_bytes());comm.extend(16u16.to_be_bytes());comm.extend([0x40,0x0e,0xbb,0x80,0,0,0,0,0,0]);
    let mut inst=vec![0;20];inst[0]=64;inst[1]=25;inst[3]=127;inst[5]=127;inst[8..10].copy_from_slice(&1u16.to_be_bytes());inst[10..12].copy_from_slice(&1u16.to_be_bytes());inst[12..14].copy_from_slice(&2u16.to_be_bytes());
    let mut marks=2u16.to_be_bytes().to_vec();for (id,frame) in [(1u16,16u32),(2,96)] {marks.extend(id.to_be_bytes());marks.extend(frame.to_be_bytes());marks.extend([0,0]);}
    let mut sound=vec![0;8];sound.extend(signal().into_iter().flat_map(i16::to_be_bytes));
    let mut out=b"FORM\0\0\0\0AIFF".to_vec();for (id,data) in [(b"COMM",comm),(b"INST",inst),(b"MARK",marks),(b"SSND",sound)] {out.extend(chunk(id,&data,true));}
    let n=(out.len()-8)as u32;out[4..8].copy_from_slice(&n.to_be_bytes());out
}
fn crc8(bytes:&[u8])->u8 {let mut crc=0u8;for byte in bytes {crc^=*byte;for _ in 0..8 {crc=if crc&0x80!=0 {(crc<<1)^7}else{crc<<1};}}crc}
fn crc16(bytes:&[u8])->u16 {let mut crc=0u16;for byte in bytes {crc^=u16::from(*byte)<<8;for _ in 0..8 {crc=if crc&0x8000!=0 {(crc<<1)^0x8005}else{crc<<1};}}crc}
fn flac()->Vec<u8> {
    // One valid verbatim mono16 FLAC frame and ordinary foreign RIFF smpl.
    let mut info=vec![0;34];info[0..2].copy_from_slice(&(FRAMES as u16).to_be_bytes());info[2..4].copy_from_slice(&(FRAMES as u16).to_be_bytes());
    let packed=(48000u64<<44)|(15u64<<36)|FRAMES as u64;info[10..18].copy_from_slice(&packed.to_be_bytes());
    let mut out=b"fLaC".to_vec();out.extend([0,0,0,34]);out.extend(info);
    let mut foreign=b"riff".to_vec();foreign.extend(chunk(b"smpl",&smpl(16,95,false),false));let n=foreign.len();out.extend([0x82,(n>>16)as u8,(n>>8)as u8,n as u8]);out.extend(foreign);
    let mut frame=vec![0xff,0xf8,0x60,0x08,0,(FRAMES-1)as u8];frame.push(crc8(&frame));frame.push(2);frame.extend(signal().into_iter().flat_map(i16::to_be_bytes));frame.extend(crc16(&frame).to_be_bytes());out.extend(frame);out
}
fn render(path:&Path,streamed:bool,key:u8)->(sampler_ir::Instrument,usize,Vec<[f32;2]>,Option<u64>) {
    let (loaded,cache)=if streamed {let s=sampler_uvi::load_streamed(path,48000,&Default::default()).unwrap();(s.loaded,Some(s.cache))}else{(sampler_uvi::load(path,48000).unwrap(),None)};
    let ir=loaded.instrument;let limits=Limits::for_plan(&loaded.plan,8,8);let mut rt=Runtime::new(loaded.plan,limits).unwrap();if let Some(cache)=cache {rt=rt.with_stream_cache(cache);}
    let note=rt.trigger(Input {protocol:Protocol::Native,port:0,group:0,channel:0,key,external_id:None},key,1.).unwrap();
    let mut audio=vec![[0.;2];1024];for block in audio.chunks_mut(64) {rt.render(block).unwrap();}
    let voices=rt.voice_count();let mut frame=None;rt.visit_onset_voices(|v|{frame=v.cursor.frame;true});
    let stats=rt.stats();assert_eq!(stats.nonfinite_frames,0);assert_eq!(stats.stream_underruns,0);
    rt.release(note).unwrap();rt.render(&mut [[0.;2];128]).unwrap();rt.flush_ended(|_|true);assert_eq!(rt.voice_count(),0);
    (ir,voices,audio,frame)
}
fn loop_range(ir:&sampler_ir::Instrument)->(u64,u64) {
    match ir.zones[0].playback.looping {Looping::Continuous(r)=>(r.start,r.end),other=>panic!("missing inherited continuous loop: {other:?}")}
}
fn late_energy(audio:&[[f32;2]])->f64 {audio[768..].iter().map(|f|f64::from(f[0]).powi(2)).sum::<f64>()/256.}

fn inherited_case(format:&str,bytes:Vec<u8>,streamed:bool) {
    let f=Fixture::new();let path=f.program(format,&bytes,"");
    let (ir,voices,audio,frame)=render(&path,streamed,60);
    assert_eq!(loop_range(&ir),(16,96));assert_eq!(ir.zones[0].pitch,KeyTracking::Tracked {root:60});assert_eq!(ir.zones[0].tune.semitones(),0.);
    assert_eq!(voices,1,"held source must survive beyond its128-frame file");assert!((16..96).contains(&frame.unwrap()));assert!(late_energy(&audio)>0.01);
    if streamed {let (_,_,reference,_)=render(&path,false,60);assert_eq!(audio,reference,"resident/streamed bytes and cursor semantics must agree");}
}
#[test] fn absent_options_wav_full() {inherited_case("wav",wav(16,95,false),false);}
#[test] fn absent_options_wav_streamed() {inherited_case("wav",wav(16,95,false),true);}
#[test] fn absent_options_aiff_full() {inherited_case("aiff",aiff(),false);}
#[test] fn absent_options_aiff_streamed() {inherited_case("aiff",aiff(),true);}
#[test] fn absent_options_flac_full() {inherited_case("flac",flac(),false);}
#[test] fn absent_options_flac_streamed() {inherited_case("flac",flac(),true);}
#[test]
fn stored_empty_options_disable_file_loop_and_stored_loop_has_precedence() {
    for options in ["<PlaybackOptions/>","<PlaybackOptions Start='0' Stop='128'><Loop Start='32' End='64' Type='0'/></PlaybackOptions>"] {
        let f=Fixture::new();let path=f.program("wav",&wav(16,95,false),options);
        for streamed in [false,true] {
            let (ir,voices,audio,_)=render(&path,streamed,60);
            if options=="<PlaybackOptions/>" {assert!(matches!(ir.zones[0].playback.looping,Looping::None));assert_eq!(voices,0);assert_eq!(late_energy(&audio),0.);}
            else {assert_eq!(loop_range(&ir),(32,64));assert_eq!(voices,1);assert!(late_energy(&audio)>0.01);}
        }
    }
}
#[test]
fn shared_asset_keeps_player_specific_stored_options() {
    let f=Fixture::new();let path=f.program("wav",&wav(16,95,false),"");let xml=std::fs::read_to_string(&path).unwrap();
    let first=xml.find("<Keygroup LowKey").unwrap();let end=xml.find("</Keygroup>").unwrap()+"</Keygroup>".len();
    let second=xml[first..end].replace("LowKey=\"60\" HighKey=\"60\"","LowKey=\"61\" HighKey=\"61\"")
        .replace("</SamplePlayer>","<PlaybackOptions/></SamplePlayer>");
    std::fs::write(&path,xml.replacen("</Keygroups>",&format!("{second}</Keygroups>"),1)).unwrap();
    for streamed in [false,true] {
        let (ir,voices,audio,_)=render(&path,streamed,60);assert_eq!(ir.assets.len(),1);assert_eq!(loop_range(&ir),(16,96));assert_eq!(voices,1);assert!(late_energy(&audio)>0.01);
        let (_,voices,audio,_)=render(&path,streamed,61);assert_eq!(voices,0);assert_eq!(late_energy(&audio),0.);
    }
}
#[test]
fn explicit_options_do_not_depend_on_unsupported_file_vendor_metadata() {
    let f=Fixture::new();let path=f.program("wav",&wav(16,95,true),"<PlaybackOptions/>");
    for streamed in [false,true] {let (_,voices,audio,_)=render(&path,streamed,60);assert_eq!(voices,0);assert_eq!(late_energy(&audio),0.);}
}
#[test]
fn malformed_inherited_loop_is_reported_and_cancellation_never_publishes() {
    let f=Fixture::new();let path=f.program("wav",&wav(16,999,false),"");
    let loaded=sampler_uvi::load(&path,48000).unwrap();assert!(loaded.instrument.unsupported.iter().any(|u|u.feature=="unreadable initial sample" && u.value.contains("metadata")),"malformed metadata must not silently become unlooped playback");
    let loaded=sampler_uvi::load_streamed(&path,48000,&Default::default()).unwrap();assert!(loaded.loaded.instrument.unsupported.iter().any(|u|u.value.contains("metadata")));
    let path=f.program("wav",&wav(16,95,false),"");let t=sampler_uvi::translate_path(&path).unwrap();let count=AtomicU64::new(0);
    let result=sampler_uvi::assemble_translated_streamed_cancelable(t,48000,&Default::default(),&||count.fetch_add(1,Ordering::Relaxed)>1);
    let error=result.err().unwrap();
    let typed=error.downcast_ref::<sampler_kontakt::LoadError>().expect("cancellation keeps its existing typed public load error");
    assert!(matches!(typed,sampler_kontakt::LoadError::Canceled));assert_eq!(typed.stage(),None);assert!(count.load(Ordering::Relaxed)>1);
}


// Existing public owner/worker/commit API, not a new playback setter.
struct ReplacementFiles;
impl sampler_uvi::script::Files for ReplacementFiles {
    fn script(&self,_:&str)->Option<String> {None}
    fn sample_source(&self)->Option<sampler_uvi::script::SampleSource> {
        Some(std::sync::Arc::new(|request,_| {
            if request.path!="replacement.wav" {return Err("unexpected authored sample request".into());}
            struct Constant;
            impl sampler_kontakt::AssetSource for Constant {
                fn open(&self)->std::io::Result<sampler_kontakt::SampleReader> {
                    Ok(sampler_kontakt::SampleReader::custom(48000,128,|_,out| {out.fill([0.75;2]);Ok(())}))
                }
            }
            Ok(sampler_uvi::script::SampleResource {
                source:std::sync::Arc::new(Constant),reader:None,
                playback:sampler_core::Playback {end:Some(128),loop_range:Some(sampler_core::Loop {
                    start:32,end:64,mode:sampler_core::LoopMode::Continuous,
                    shape:sampler_core::LoopShape::Wrap,passes:None}),..Default::default()},
                base_note:64,fine_tune:25.,geometry_error:None,format_code:1,channels:1,bits:16,
                tempo:0.,timings:Default::default(),
            })
        }))
    }
}
fn add_init_script(path:&Path,script:&str)->String {
    let xml=std::fs::read_to_string(path).unwrap().replace("</Program>",&format!(
        "<EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors></Program>"));
    std::fs::write(path,&xml).unwrap();xml
}
#[test]
fn init_load_sample_commits_replacement_file_loop_or_stored_program_options_without_changing_root() {
    for (options,expected) in [("",(32,64)),("<PlaybackOptions Start='0' Stop='128'><Loop Start='48' End='80' Type='0'/></PlaybackOptions>",(48,80))] {
        let f=Fixture::new();let path=f.program("wav",&wav(16,95,false),options);
        let xml=add_init_script(&path,r#"
          local osc=Program.layers[1].keygroups[1].oscillators[1]
          function onInit()
            loadSample(osc,'replacement.wav',function(t)
              assert(t.finished and t.success)
              assert(osc:getParameter('BaseNote')==60)
              assert(osc.sampleInfo.basenote==64)
              completed=true
            end)
          end
        "#);
        // The normal initial assembly precedes transaction publication. Inherited
        // original [16,96) must never be reapplied over the committed replacement.
        let loaded=sampler_uvi::load(&path,48000).unwrap();
        if options.is_empty() {assert_eq!(loop_range(&loaded.instrument),(16,96));}
        let limits=Limits::for_plan(&loaded.plan,8,8);
        let (mut rt,control)=Runtime::new(loaded.plan,limits).unwrap().with_sample_updates(4).unwrap();
        let mut host=sampler_uvi::script::ScriptHost::new(&xml,ReplacementFiles,Default::default()).unwrap();
        assert!(host.fault_counts().init.is_empty(),"{:?}",host.findings());
        assert_ne!(host.global_text("completed"),"true");
        host.bind_sample_control(control).unwrap();
        let until=std::time::Instant::now()+std::time::Duration::from_secs(2);let mut ms=0.;
        while host.global_text("completed")!="true" {
            host.advance(ms);rt.poll_sample_update().unwrap();host.advance(ms);ms+=1.;
            assert!(std::time::Instant::now()<until,"{:?}; {:?}",host.fault_counts(),host.findings());
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(host.fault_counts().runtime.is_empty(),"{:?}",host.findings());
        let status=host.sample_load_status();assert_eq!((status.requested,status.successful,status.failed,status.pending),(1,1,0,0));
        let note=rt.trigger(Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:None},60,1.).unwrap();
        let mut out=[[0.;2];1024];for block in out.chunks_mut(64) {rt.render(block).unwrap();}
        assert_eq!(rt.voice_count(),1);assert!(out[768..].iter().all(|v|*v==[0.75;2]));
        let mut seen=0;rt.visit_onset_voices(|v| {seen+=1;assert!((expected.0..expected.1).contains(&v.cursor.frame.unwrap()));assert_eq!(v.cursor.step,1.);true});assert_eq!(seen,1);
        assert_eq!(rt.stats().nonfinite_frames,0);assert_eq!(rt.stats().stream_underruns,0);
        rt.release(note).unwrap();rt.render(&mut [[0.;2];128]).unwrap();rt.flush_ended(|_|true);assert_eq!(rt.voice_count(),0);
    }
}
#[test]
fn unsupported_init_set_playback_options_is_an_explicit_lua_fault_not_an_effective_write() {
    let f=Fixture::new();let path=f.program("wav",&wav(16,95,false),"");
    let xml=add_init_script(&path,r#"
      function onInit()
        local osc=Program.layers[1].keygroups[1].oscillators[1]
        assert(osc.setPlaybackOptions==nil)
        osc:setPlaybackOptions({start=32,stop=64})
        accepted=true
      end
    "#);
    let host=sampler_uvi::script::ScriptHost::new(&xml,ReplacementFiles,Default::default()).unwrap();
    assert_ne!(host.global_text("accepted"),"true");assert!(!host.fault_counts().init.is_empty());
    assert!(host.findings().iter().any(|f|f.feature=="lua error" && f.value.contains("setPlaybackOptions")),"{:?}",host.findings());
    let loaded=sampler_uvi::load(&path,48000).unwrap();assert_eq!(loop_range(&loaded.instrument),(16,96));
}

#[test]
fn aiff_stream_header_and_full_decode_match_the_authored_pcm_frame_count() {
    let f=Fixture::new();let path=f.program("aiff",&aiff(),"<PlaybackOptions/>");
    let full=sampler_uvi::decode_sample(&f.0.join("tone.aiff")).unwrap();
    assert_eq!(full.rate,48000);assert_eq!(full.frames.len(),FRAMES);
    let expected:Vec<_>=signal().into_iter().map(|s|[f32::from(s)/32768.;2]).collect();assert_eq!(full.frames,expected);
    let streamed=sampler_uvi::load_streamed(&path,48000,&Default::default()).unwrap();
    assert_eq!(streamed.assets.len(),1);assert_eq!(streamed.assets[0].frame_count(),FRAMES);
    assert_eq!(streamed.assets[0].sample_rate(),48000);
}


fn mixed_players(f:&Fixture,bytes:&[u8],options:&str,impulse:bool)->PathBuf {
    let path=f.program("wav",bytes,"");let mut xml=std::fs::read_to_string(&path).unwrap();
    let start=xml.find("<Keygroup LowKey").unwrap();let end=xml.find("</Keygroup>").unwrap()+"</Keygroup>".len();
    let second=xml[start..end].replace("LowKey=\"60\" HighKey=\"60\"","LowKey=\"61\" HighKey=\"61\"")
        .replace("</SamplePlayer>",&format!("{options}</SamplePlayer>"));
    xml=xml.replacen("</Keygroups>",&format!("{second}</Keygroups>"),1);
    if impulse {xml=xml.replace("<Program>","<Program><Inserts><Convolver Dry='0' Wet='1' NormalizePower='0' SamplePath='tone.wav'/></Inserts>");}
    std::fs::write(&path,xml).unwrap();path
}
#[test]
fn malformed_inheritance_preserves_explicit_players_sharing_valid_pcm_full_and_streamed() {
    for bytes in [wav(16,95,true),wav(16,999,false)] {
        for options in ["<PlaybackOptions/>","<PlaybackOptions Start='0' Stop='128'><Loop Start='32' End='64' Type='0'/></PlaybackOptions>"] {
            let f=Fixture::new();let path=mixed_players(&f,&bytes,options,false);
            for streamed in [false,true] {
                let (ir,voices,audio,_)=render(&path,streamed,60);
                let bad=ir.zones.iter().find(|z|z.keys.low==60).unwrap();assert!(bad.asset.is_none());
                assert_eq!(voices,0);assert!(audio.iter().all(|v|*v==[0.;2]));
                assert!(ir.unsupported.iter().any(|u|u.feature=="unreadable initial sample" && u.value.contains("metadata")),"{:?}",ir.unsupported);
                let (ir,voices,audio,_)=render(&path,streamed,61);
                assert!(ir.zones.iter().find(|z|z.keys.low==61).unwrap().asset.is_some());
                assert!(audio[..64].iter().any(|v|v[0].abs()>0.1),"explicit player's PCM must remain audible");
                if options=="<PlaybackOptions/>" {assert_eq!(voices,0);assert_eq!(late_energy(&audio),0.);}
                else {assert_eq!(voices,1);assert!(late_energy(&audio)>0.01);}
            }
        }
    }
}
#[test]
fn malformed_inheritance_does_not_replace_a_shared_decodable_impulse_with_placeholder() {
    let f=Fixture::new();let path=mixed_players(&f,&wav(16,95,true),"<PlaybackOptions/>",true);
    let expected:Vec<_>=signal().into_iter().map(|s|f32::from(s)/32768.).collect();
    for streamed in [false,true] {
        let ir=if streamed {sampler_uvi::load_streamed(&path,48000,&Default::default()).unwrap().loaded.instrument}
            else {sampler_uvi::load(&path,48000).unwrap().instrument};
        assert!(ir.zones.iter().find(|z|z.keys.low==60).unwrap().asset.is_none());
        assert!(ir.zones.iter().find(|z|z.keys.low==61).unwrap().asset.is_some());
        assert_eq!(ir.impulses.len(),1);assert_eq!(ir.impulses[0].rate,48000);
        assert_eq!(ir.impulses[0].left,expected);assert_eq!(ir.impulses[0].right,expected);
        assert!(!ir.unsupported.iter().any(|u|u.feature=="unreadable impulse response (passed through)"));
    }
}
