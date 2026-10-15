use sampler_core::{Limits, Runtime, SampleTargetId};
use sampler_uvi::script::{Config, ScriptHost};
use std::{path::PathBuf, time::{Duration, Instant}};

#[test]
fn synthetic_part_writes_cannot_alias_wrapped_or_unwrapped_program_xml() {
    for wrapped in [false,true] {
        let program=r#"<Program Streaming="1"><EventProcessors><ScriptProcessor><script><![CDATA[
            function onInit()
                Program.parent:setParameter('Streaming',false)
                Program.parent:setParameter('Gain',0.25)
                partStreaming=Program.parent:getParameter('Streaming')
                programStreaming=Program:getParameter('Streaming')
                programGain=Program:getParameter('Gain')
            end
        ]]></script></ScriptProcessor></EventProcessors></Program>"#;
        let text=if wrapped {format!("<UVI4>{program}</UVI4>")} else {program.into()};
        let host=ScriptHost::new(&text,(),Config::default()).unwrap();
        assert_eq!(host.global_text("partStreaming"),"true");
        assert_eq!(host.global_text("programStreaming"),"true");
        assert_eq!(host.global_text("programGain"),"1");
        assert!(host.folded_writes().is_empty(),"synthetic Part has no original XML identity");
        assert!(host.insert_overrides().is_empty());
        for field in ["Part.Streaming","Part.Gain"] {
            assert!(host.findings().iter().any(|f|f.feature==format!("lua setParameter {field}")));
        }
    }
}

#[test]
fn changed_live_residency_flags_are_explicitly_unavailable_and_keep_committed_readback() {
    let xml=r#"<Program Streaming="1"><EventProcessors><ScriptProcessor><script><![CDATA[
        function onNote(e)
            local osc=Program.layers[1].keygroups[1].oscillators[1]
            Program:setParameter('Streaming',false)
            osc:setParameter('AllowStreaming',false)
            Program.parent:setParameter('Streaming',false)
            programStreaming=Program:getParameter('Streaming')
            oscillatorStreaming=osc:getParameter('AllowStreaming')
        end
    ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer AllowStreaming="1"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#;
    let mut host=ScriptHost::new(xml,(),Config::default()).unwrap();
    host.take_commands();
    host.note_on(1,60,100,0);
    assert_eq!(host.global_text("programStreaming"),"true");
    assert_eq!(host.global_text("oscillatorStreaming"),"true");
    assert!(host.take_commands().is_empty());
    for field in ["Program.Streaming","SamplePlayer.AllowStreaming","Part.Streaming"] {
        assert!(host.findings().iter().any(|f|f.feature==format!("lua setParameter {field}")));
    }
}

struct Scratch(PathBuf);
impl Drop for Scratch {fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}

#[test]
fn final_on_init_policy_controls_real_dynamic_sources_without_losing_cancelled_tasks() {
    let dir=Scratch(std::env::temp_dir().join(format!("uvi-init-residency-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let frames=40000usize;
    let mut bytes=b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
    bytes.extend(48000u32.to_le_bytes());bytes.extend(96000u32.to_le_bytes());bytes.extend(2u16.to_le_bytes());bytes.extend(16u16.to_le_bytes());bytes.extend(b"data");bytes.extend((frames as u32*2).to_le_bytes());
    for i in 0..frames {bytes.extend((((i*37)%8000)as i16-4000).to_le_bytes());}
    let extent=(bytes.len()-8)as u32;bytes[4..8].copy_from_slice(&extent.to_le_bytes());
    std::fs::write(dir.0.join("tone.wav"),bytes).unwrap();
    for (program,oscillator,write,resident) in [
        (1,1,"Program:setParameter('Streaming',false)",true),
        (0,1,"Program:setParameter('Streaming',true)",false),
        (1,1,"osc:setParameter('AllowStreaming',false)",true),
        (1,0,"osc:setParameter('AllowStreaming',true)",false),
        (0,0,"Program:setParameter('Streaming',true)",true),
    ] {
        let xml=format!(r#"<UVI4><Program Streaming="{program}"><EventProcessors><ScriptProcessor><script><![CDATA[
            Ready=OnOffButton('Ready',false)
            function onInit()
                local osc=Program.layers[1].keygroups[1].oscillators[1]
                first=loadSample(osc,'tone.wav',function(t) assert(rawequal(first,t) and t.finished and not t.success) end)
                first:cancel()
                {write}
                final=loadSample(osc,'tone.wav',function(t) assert(rawequal(final,t) and t.finished and t.success); Ready:setValue(true) end)
            end
        ]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup LowKey="60" HighKey="60"><Oscillators><SamplePlayer AllowStreaming="{oscillator}" BaseNote="60" NoteTracking="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#);
        let path=dir.0.join("Initial.uvip");std::fs::write(&path,xml).unwrap();
        let mut translated=sampler_uvi::translate_path(&path).unwrap();
        let attached=translated.attach_script(48000,Default::default()).unwrap().unwrap();
        assert!(!translated.instrument.unsupported.iter().any(|u|u.feature=="lua setParameter Program.Streaming"||u.feature=="lua setParameter SamplePlayer.AllowStreaming"),"recognized initialization policy is consumed");
        let target=SampleTargetId(translated.instrument.source_indices.sample_targets[0].node);
        let streamed=sampler_uvi::assemble_translated_streamed(translated,48000,&sampler_kontakt::StreamPolicy{voices:4,head_budget:1<<20,..Default::default()}).unwrap();
        let limits=Limits::for_plan(&streamed.loaded.plan,4,4);
        let (mut runtime,control)=Runtime::new(streamed.loaded.plan,limits).unwrap().with_stream_cache(streamed.cache).with_sample_updates(4).unwrap();
        let mut driver=attached.driver;
        driver.bind_sample_stream(streamed.streamer.dynamic_sources()).unwrap();driver.bind_sample_control(control).unwrap();
        let until=Instant::now()+Duration::from_secs(3);
        loop {
            runtime.poll_sample_update().unwrap();
            let status=driver.sample_load_status().unwrap();
            if status.pending==0 && status.successful==1 && status.cancelled==1 && driver.ui().values().iter().any(|(_,v)|*v==1.) {break;}
            assert!(Instant::now()<until,"{status:?}; {:?}",driver.ui().fault_counts());
            std::thread::sleep(Duration::from_millis(1));
        }
        let pcm=runtime.sample_target_asset(target).unwrap();
        assert_eq!((pcm.sample_rate(),pcm.frame_count()),(48000,frames));
        assert_eq!(pcm.resident_frames().is_some(),resident,"{write}");
        let mut out=[[0.;2];256];pcm.copy_resident_range(0,&mut out).unwrap();
        for (i,f) in out.iter().enumerate(){let expected=(((i*37)%8000)as i16-4000)as f32/32768.;assert_eq!(f.map(f32::to_bits),[expected;2].map(f32::to_bits));}
        assert!(driver.ui().fault_counts().runtime.is_empty());
    }
}
