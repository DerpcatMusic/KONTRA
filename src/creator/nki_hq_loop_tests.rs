use super::*;

include!("nki_hq_loop_native_pcm.rs");

#[test]
fn saved_native_high_ultra_loop_pcm_matches_prepared_native_producer_in_ram_and_stream() {
    use crate::{engine::{Ahdsr, Bank, Engine, NoteEvent, Streaming, MEMORY_LIMIT},
        import::{Group, NativeLoop, Zone}};
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Samples")).unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    let wave = |path:&std::path::Path,rate,frames:&[[f32;2]]| {
        let mut writer=hound::WavWriter::create(path,hound::WavSpec { channels:2,sample_rate:rate,
            bits_per_sample:32,sample_format:hound::SampleFormat::Float }).unwrap();
        for frame in frames { for value in frame { writer.write_sample(*value).unwrap(); } }
        writer.finalize().unwrap();
    };
    // Prepared native source0/DFD3 captures retain the selected Normal/High/Ultra
    // points and loop domains; exercise both saved identities through real loading.
    for source_mode in [0,3] {
      for (case_index,case) in CASES.iter().chain(NORMAL_CASES).enumerate() {
        let domain = &case.domain;
        let source_name = format!("Samples/native-{source_mode}-{case_index}.wav");
        let source:Vec<_> = (0..domain.frames).map(|i| [(((i*73)%257) as f32-128.)/1024.;2]).collect();
        wave(&dir.path().join(&source_name),domain.rate,&source);
        let rows:Vec<_> = case.rows.iter().map(|&(slot,mode,start,length,count)| NativeLoop {
            slot,mode,start,length,count,alternating:false,tuning:1.,crossfade:0 }).collect();
        let groups=[Group { source_mode:Some(source_mode),interp_quality:case.quality,..Default::default() }];
        let zones=[Zone { sample:source_name.clone().into(),native_loops:rows.clone(),
            start:domain.start,end:domain.end as i32-domain.frames as i32,
            start_mod:(case.offset_us!=0).then_some((domain.end-domain.start-1) as u32),..Default::default() }];
        let samples=[SampleFile { relative:source_name.into(),rate:domain.rate,channels:2,bits:32,
            frames:domain.frames as u64,modified:0 }];
        let program=Program { name:"Native loop source",author:"Test",groups:&groups,zones:&zones,
            samples:&samples,script:None,native_private:None,default_key_switch:None };
        let path=dir.path().join(format!("native-{source_mode}-{case_index}.nki"));
        write(&path,&program).unwrap();
        let cache=cache_root.path().join(format!("cache-{source_mode}-{case_index}"));
        std::fs::create_dir_all(&cache).unwrap();
        let imported=crate::import::read(&path).unwrap();
        assert_eq!(imported.groups[0].source_mode,Some(source_mode));
        assert_eq!(imported.groups[0].interp_quality,case.quality);
        assert_eq!(imported.zones[0].native_loops,rows);
        assert_eq!(imported.zones[0].start,domain.start);
        assert_eq!(imported.zones[0].end,domain.end as i32-domain.frames as i32);
        assert_eq!(imported.zones[0].start_mod,zones[0].start_mod,
            "the authored public offset must retain its saved admission range");
        crate::cache::store(&cache,&path,0,&imported);
        let restored=crate::cache::load(&cache,&path,0).unwrap();
        assert_eq!(restored.zones[0].native_loops,rows);
        assert_eq!(restored.groups[0].source_mode,Some(source_mode));
        let continuous=case.name=="continuous";
        let end=if continuous { 320 } else { (case.sounding+64).max(384) };
        // Literal native convolution output supplies the source. Its captured
        // last physical phase and point count independently determine the
        // existing one-millisecond ordinary EOF amplifier law.
        let reference_frames=(end+1024).max(4096);
        let mut reference=vec![[0.;2];reference_frames];
        let step=f64::from(domain.rate)/f64::from(domain.host_rate);
        let ramp=0.001*domain.host_rate;
        for (i,&bits) in case.native.iter().enumerate() {
            let fade=if continuous { 1. } else {
                let remaining=((case.sounding-1) as f64-i as f64+(domain.end as f64-case.tail_phase)/step) as f32;
                (remaining/ramp).clamp(0.,1.)
            };
            reference[i]=[f32::from_bits(bits)*fade;2];
        }
        // Public nonzero offsets receive the independently authored one-ms
        // integer-frame onset ramp; the captured reference starts at zero.
        if case.offset_us != 0 {
            let mut gain=0f32;
            for (i,frame) in reference.iter_mut().take(ramp as usize).enumerate() {
                gain=if i+1==ramp as usize {1.} else {gain+1./(ramp as u32).max(1) as f32};
                for channel in frame { *channel*=gain; }
            }
        }
        let reference_name=format!("Samples/reference-{source_mode}-{case_index}.wav");
        wave(&dir.path().join(&reference_name),domain.host_rate as u32,&reference);
        let reference_groups=[Group::default()];
        let reference_zones=[Zone { sample:reference_name.clone().into(),..Default::default() }];
        let reference_samples=[SampleFile { relative:reference_name,rate:domain.host_rate as u32,channels:2,bits:32,
            frames:reference_frames as u64,modified:0 }];
        let reference_program=Program { name:"Captured native",author:"Test",groups:&reference_groups,
            zones:&reference_zones,samples:&reference_samples,script:None,native_private:None,default_key_switch:None };
        let reference_path=dir.path().join(format!("reference-{case_index}.nki"));
        write(&reference_path,&reference_program).unwrap();
        let reference=Bank::load_counting(&crate::import::read(&reference_path).unwrap(),MEMORY_LIMIT,
            Streaming::RamOnly,&[],&std::sync::atomic::AtomicU32::new(0)).unwrap();
        let ram=Bank::load_counting(&restored,MEMORY_LIMIT,Streaming::RamOnly,&[],
            &std::sync::atomic::AtomicU32::new(0)).unwrap();
        let streamed=Bank::load_bare(&restored).unwrap();
        assert_eq!(streamed.streamed_samples(),1);
        let banks=[ram,streamed,Bank::load_bare(&restored).unwrap(),Bank::load(&restored).unwrap(),reference];
        let mut engines=banks.into_iter().enumerate().map(|(index,bank)| {
            let bank=bank.with_prepared_settings(|settings| {
                settings[0].envelope=Some(Ahdsr::UNITY);
            }).unwrap();
            let mut engine=Engine::default();engine.reset(domain.host_rate as f64);engine.attack=0.;engine.blocking_streams=true;
            engine.set_bank(Some(Box::new(bank)));
            for _ in 0..4 {
                let mut note=NoteEvent::new(0,60,127);
                if index<4 { note.offset_us=case.offset_us; }
                engine.start_event(&note).unwrap();
            }
            engine
        }).collect::<Vec<_>>();
        let mut pcm:Vec<[Vec<f32>;2]>=(0..5).map(|_|[vec![0.;end],vec![0.;end]]).collect();
        assert_eq!(crate::plugin::tests::allocations(|| {
            for (index,engine) in engines.iter_mut().enumerate() {
                let mut at=0;let mut block=0;
                while at<end {
                    if case.release==Some(at) { engine.note_off(0,60); }
                    let mut n=if index%2==0 { 128 } else { [1,7,31,127][block%4] }.min(end-at);
                    for boundary in [case.release.unwrap_or(end),case.sounding.saturating_sub(1),case.sounding] {
                        if at<boundary { n=n.min(boundary-at); }
                    }
                    let [left,right]=&mut pcm[index];
                    engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
                    at+=n;block+=1;
                    if index<4 && !continuous && matches!(at,x if x==case.sounding-1 || x==case.sounding) {
                        assert_eq!(engine.active_voices(),if at<case.sounding { 4 } else { 0 },
                            "native {} mode{source_mode} quality{} engine{index} endpoint{at}",case.name,case.quality);
                    }
                }
                assert_eq!(engine.underruns(),0);
                if index<4 { assert_eq!(engine.active_voices(),if continuous { 4 } else { 0 }); }
            }
        }),0);
        assert!(pcm[4].iter().flatten().any(|v|v.abs()>0.001),"native {} mode{source_mode} quality{} reference excitation",case.name,case.quality);
        for index in 0..4 {
            let error=pcm[index].iter().flatten().zip(pcm[4].iter().flatten())
                .map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error<2e-6,"native {} mode{source_mode} quality{} engine{index}: {error}",case.name,case.quality);
        }
      }
    }
}

#[test]
fn native_normal_until_release_requires_prepared_domain_before_pcm_and_preserves_sibling() {
    use crate::{engine::{Ahdsr,Bank,Engine,NoteEvent,Streaming,StreamErrorKind,MEMORY_LIMIT},
        import::{Group,NativeLoop,SamplingProfile,Zone}};
    let dir=tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Samples")).unwrap();
    let cache_root=tempfile::tempdir().unwrap();
    let wave=|name:&str,frames:usize,sibling:bool| {
        let mut writer=hound::WavWriter::create(dir.path().join(name),hound::WavSpec {
            channels:2,sample_rate:48000,bits_per_sample:32,sample_format:hound::SampleFormat::Float }).unwrap();
        for i in 0..frames {
            let value=if sibling { ((i%31) as f32-15.)/64. } else { 0.25 };
            writer.write_sample(value).unwrap();writer.write_sample(-value*0.5).unwrap();
        }
        writer.finalize().unwrap();
    };
    wave("Samples/sibling.wav",4096,true);
    for source_mode in [0,3] {
      // Invalid native asset size; normalized loop10..14 crossing crop12;
      // valid prepared native loop; explicit Compatibility tiny loop.
      for (case,(frames,start,length,end,profile,reject)) in [
          (3,0,3,0,SamplingProfile::Kontakt,true),
          (100,10,2,-88,SamplingProfile::Kontakt,true),
          (4,0,4,0,SamplingProfile::Kontakt,false),
          (3,0,3,0,SamplingProfile::Compatibility,false),
      ].into_iter().enumerate() {
        let name=format!("Samples/guard-{source_mode}-{case}.wav");wave(&name,frames,false);
        let rows=vec![NativeLoop { slot:0,mode:2,start,length,count:0,alternating:false,tuning:1.,crossfade:0 }];
        let groups=[Group { source_mode:Some(source_mode),interp_quality:0,..Default::default() }];
        let zones=[Zone { sample:name.clone().into(),low_key:60,high_key:60,native_loops:rows.clone(),end,..Default::default() },
            Zone { sample:"Samples/sibling.wav".into(),low_key:64,high_key:64,root:64,..Default::default() }];
        let samples=[SampleFile { relative:name,rate:48000,channels:2,bits:32,frames:frames as u64,modified:0 },
            SampleFile { relative:"Samples/sibling.wav".into(),rate:48000,channels:2,bits:32,frames:4096,modified:0 }];
        let program=Program { name:"Native domain admission",author:"Test",groups:&groups,zones:&zones,
            samples:&samples,script:None,native_private:None,default_key_switch:None };
        let path=dir.path().join(format!("guard-{source_mode}-{case}.nki"));write(&path,&program).unwrap();
        let cache=cache_root.path().join(format!("guard-{source_mode}-{case}"));std::fs::create_dir_all(&cache).unwrap();
        let imported=crate::import::read(&path).unwrap();
        assert_eq!(imported.groups[0].source_mode,Some(source_mode));assert_eq!(imported.zones[0].native_loops,rows);
        crate::cache::store(&cache,&path,0,&imported);
        let mut restored=crate::cache::load(&cache,&path,0).unwrap();restored.sampling_profile=profile;
        for streamed in [false,true] {
            let bank=|| {
                let bank=if streamed { Bank::load_bare(&restored).unwrap() } else {
                    Bank::load_counting(&restored,MEMORY_LIMIT,Streaming::RamOnly,&[],
                        &std::sync::atomic::AtomicU32::new(0)).unwrap() };
                bank.with_prepared_settings(|settings| settings[0].envelope=Some(Ahdsr::UNITY)).unwrap()
            };
            let engine=|| {
                let mut engine=Engine::default();engine.reset(48000.);engine.attack=0.;engine.blocking_streams=true;
                engine.set_bank(Some(Box::new(bank())));engine
            };
            let mut actual=engine();
            if reject {
                let mut reference=engine();
                actual.start_event(&NoteEvent::new(0,64,127)).unwrap();reference.start_event(&NoteEvent::new(0,64,127)).unwrap();
                let mut a=[[0.;128];2];let mut b=[[0.;128];2];
                let render=|e:&mut Engine,out:&mut [[f32;128];2]| {
                    let [left,right]=out;e.render_checked(left,right)
                };
                render(&mut actual,&mut a).unwrap();render(&mut reference,&mut b).unwrap();assert_eq!(a,b);
                actual.start_event(&NoteEvent::new(0,60,127)).unwrap();
                assert_eq!(crate::plugin::tests::allocations(|| {
                    let error=render(&mut actual,&mut a).unwrap_err();assert_eq!(error.kind,StreamErrorKind::UnsupportedWindow);
                    render(&mut reference,&mut b).unwrap();assert_eq!(a,b,"failed source changed sibling PCM");
                    assert_eq!(actual.active_voices(),1);assert_eq!(reference.active_voices(),1);
                    render(&mut actual,&mut a).unwrap();render(&mut reference,&mut b).unwrap();assert_eq!(a,b);
                }),0);
            } else {
                actual.start_event(&NoteEvent::new(0,60,127)).unwrap();
                let mut left=[0.;128];let mut right=[0.;128];
                assert_eq!(crate::plugin::tests::allocations(|| { actual.render_checked(&mut left,&mut right).unwrap(); }),0);
                assert!(left.iter().any(|v|v.abs()>0.001));assert_eq!(actual.active_voices(),1);
            }
            assert_eq!(actual.underruns(),0);
        }
      }
    }
}


#[test]
fn saved_sampler_nonreflecting_alternating_count_one_keeps_later_row_pcm_in_ram_and_stream() {
    use crate::{engine::{Ahdsr,Bank,Engine,Expression,NoteEvent,Streaming,MEMORY_LIMIT},
        import::{Group,NativeLoop,Zone}};
    let dir=tempfile::tempdir().unwrap();std::fs::create_dir_all(dir.path().join("Samples")).unwrap();
    let cache_root=tempfile::tempdir().unwrap();
    let mut wav=hound::WavWriter::create(dir.path().join("Samples/source.wav"),hound::WavSpec {
        channels:2,sample_rate:780000,bits_per_sample:32,sample_format:hound::SampleFormat::Float }).unwrap();
    for i in 0..4096 { let value=(((i*73)%257) as f32-128.)/1024.;wav.write_sample(value).unwrap();wav.write_sample(-value*0.5).unwrap(); }
    wav.finalize().unwrap();
    for quality in [0,1,2] {
      for (scenario,start,release,retune) in [(0,0,None,false),(1,5,None,false),(2,5,Some(1),false),(3,0,Some(15),true)] {
        let groups=[Group { source_mode:Some(0),interp_quality:quality,..Default::default() }];
        let mut imported=Vec::new();
        for alternate in [false,true] {
            let rows=vec![NativeLoop { slot:0,mode:2,start:4,length:8,count:1,alternating:alternate,tuning:1.,crossfade:0 },
                NativeLoop { slot:2,mode:1,start:16,length:32,count:2,alternating:false,tuning:1.,crossfade:0 }];
            let zones=[Zone { sample:"Samples/source.wav".into(),start,native_loops:rows.clone(),..Default::default() }];
            let samples=[SampleFile { relative:"Samples/source.wav".into(),rate:780000,channels:2,bits:32,frames:4096,modified:0 }];
            let program=Program { name:"Nonreflecting count one",author:"Test",groups:&groups,zones:&zones,
                samples:&samples,script:None,native_private:None,default_key_switch:None };
            let path=dir.path().join(format!("single-{quality}-{scenario}-{alternate}.nki"));write(&path,&program).unwrap();
            let decoded=crate::import::read(&path).unwrap();assert_eq!(decoded.zones[0].native_loops,rows);
            let cache=cache_root.path().join(format!("single-{quality}-{scenario}-{alternate}"));std::fs::create_dir_all(&cache).unwrap();
            crate::cache::store(&cache,&path,0,&decoded);let restored=crate::cache::load(&cache,&path,0).unwrap();
            assert_eq!(restored.zones[0].native_loops,rows);assert_eq!(restored.groups[0].source_mode,Some(0));
            imported.push(restored);
        }
        // The reference clears only the alternating marker. Its forward
        // counted path and later row already have independent native PCM proof.
        let mut engines=Vec::new();
        for (side,instrument) in imported.iter().enumerate() {
          for streamed in [false,true] {
            let bank=if streamed { Bank::load_bare(instrument).unwrap() } else {
                Bank::load_counting(instrument,MEMORY_LIMIT,Streaming::RamOnly,&[],&std::sync::atomic::AtomicU32::new(0)).unwrap() };
            assert!(!bank.issues.iter().any(|w|w.contains("alternating loop")),"side{side} quality{quality}: {:?}",bank.issues);
            if streamed { assert_eq!(bank.streamed_samples(),1); }
            let bank=bank.with_prepared_settings(|settings|settings[0].envelope=Some(Ahdsr::UNITY)).unwrap();
            let mut engine=Engine::default();engine.reset(48000.);engine.attack=0.;engine.blocking_streams=true;
            engine.set_bank(Some(Box::new(bank)));engine.start_event(&NoteEvent::new(0,60,127)).unwrap();engines.push(engine);
          }
        }
        let end=1024;let terminal=((4096+32-start)*4+64)/65;
        let mut pcm:Vec<[Vec<f32>;2]>=(0..4).map(|_|[vec![0.;end],vec![0.;end]]).collect();
        assert_eq!(crate::plugin::tests::allocations(|| {
          for (index,engine) in engines.iter_mut().enumerate() {
            let mut at=0;let mut block=0;
            while at<end {
                if release==Some(at) { engine.note_off(0,60); }
                if retune && matches!(at,8|24|40) {
                    let tune=if at==8 {0.5} else if at==24 {2.} else {1.};
                    engine.set_expression_on(0,60,Expression { tune,..Default::default() });
                }
                let mut n=if index%2==0 {128} else {[1,7,31,127][block%4]}.min(end-at);
                for boundary in [release.unwrap_or(end),8,24,40,terminal-1,terminal] {
                    if at<boundary {n=n.min(boundary-at);}
                }
                let [left,right]=&mut pcm[index];engine.render_checked(&mut left[at..at+n],&mut right[at..at+n]).unwrap();
                at+=n;block+=1;
                if !retune && (at==terminal-1 || at==terminal) {
                    assert_eq!(engine.active_voices(),if at<terminal {1} else {0},"quality{quality} scenario{scenario} engine{index} end{at}");
                }
            }
            assert_eq!(engine.active_voices(),0);assert_eq!(engine.underruns(),0);
          }
        }),0);
        assert!(pcm[0].iter().flatten().any(|v|v.abs()>0.001));
        for index in 1..4 {
            let error=pcm[index].iter().flatten().zip(pcm[0].iter().flatten()).map(|(a,b)|(a-b).abs()).fold(0.,f32::max);
            assert!(error<2e-6,"nonreflecting quality{quality} scenario{scenario} engine{index}: {error}");
        }
      }
    }
}
