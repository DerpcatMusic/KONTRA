use crate::{audio::Sample, engine::{Bank, Engine, Expression, HostNote, HostPattern},
    import::{Ahdsr, FlexEnvelope, FlexPoint, Group, Loop, Zone}, ksp::EnginePar};
use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};

pub(super) fn saved(flex: bool, flags: [u8;4], targets: usize) -> Group {
    fn object(id:u16,version:u16,private:&[u8],public:&[u8],children:&[u8])->Chunk {
        let mut data=vec![1]; data.extend(version.to_le_bytes());
        for part in [private,public,children] { data.extend((part.len() as u32).to_le_bytes()); data.extend(part); }
        Chunk { id,data }
    }
    fn name(out:&mut Vec<u8>,text:&str) { out.extend((text.len() as u32).to_le_bytes()); out.extend(text.as_bytes()); }
    let mut concrete=Vec::new();
    if flex {
        FlexEnvelope { points:vec![FlexPoint {time_ms:1.,level:1.,curve:0.5},
            FlexPoint {time_ms:1000.,level:0.8,curve:0.5},FlexPoint {time_ms:1000.,level:0.,curve:0.5}],
            sustain:1,unknown_index:0,unknown_tail:[vec![0,1],vec![0,0,128,191,0,0,0,0,0,0,128,63,0]].concat()
        }.write(&mut concrete).unwrap();
    } else {
        Ahdsr {attack_curve:1.,attack_ms:7.,hold_ms:2.,decay_ms:4.,sustain:0.3,release_ms:20.,
            unknown_flag:0,unknown_tail:[0,0,128,191,0,0,0,0,0,0,128,63,0].repeat(4)
        }.write(&mut concrete).unwrap();
    }
    let mut wrapper=Vec::new();
    object(7,0x90,&[],&u32::from(flex).to_le_bytes(),&concrete).write(&mut wrapper).unwrap();
    let mut slots=Vec::new();
    for _ in 0..targets {
        let mut private=1u32.to_le_bytes().to_vec(); name(&mut private,"volume");
        private.extend(1f32.to_le_bytes()); private.extend((-1i16).to_le_bytes()); private.push(0x10);
        private.extend(0u16.to_le_bytes()); name(&mut private,"Arbitrary label");
        private.extend([0,0]); private.extend(flags); private.extend(0u32.to_le_bytes());
        name(&mut private,"Primary"); private.extend(2u32.to_le_bytes());
        slots.push(1); object(0x0d,0x80,&private,&[],&wrapper).write(&mut slots).unwrap();
    }
    slots.resize(slots.len()+16-targets,0);
    let raw=RawGroup(StructuredObject {version:0x95,private_data:vec![],public_data:vec![],
        children:vec![object(0x3b,0x10,&[],&slots,&[])]});
    let decoded=crate::modulation::read_group(&raw).unwrap();
    Group {volume_env:decoded.volume_env,native_volume_env:decoded.native_volume_env,
        flex_env:decoded.flex_env,native_flex_oneshot:decoded.native_flex_oneshot,
        native_flex_loop:decoded.native_flex_loop,native_primary_bypass_slot:decoded.native_primary_bypass_slot,
        modulators:decoded.modulators,..Default::default()}
}
pub(super) fn create(group:&Group,shared:bool,frames:usize,looped:bool)->Engine {
    let bank=Bank::from_samples(vec![group.clone()],vec![Zone {loop_range:looped.then_some(Loop {
        start:0,end:frames,crossfade:0,alternating:false,until_release:false}),..Default::default()}],
        vec![(Default::default(),Sample {rate:48000,frames:(0..frames).map(|i|
            [0.20+(i%37) as f32/1024., -0.15-(i%53) as f32/2048.]).collect()})]).unwrap();
    let mut engine=Engine::default(); engine.reset(48000.); engine.player.shared=shared;
    engine.set_bank(Some(Box::new(bank))); engine
}

#[test]
fn saved_primary_bypass_pauses_source_and_has_independent_complete_pcm_and_engine_ends() {
    use crate::engine::{envelope::{Envelope,Ahdsr as PreparedAhdsr},params::{self,Address,id}};
    const FRAMES:usize=2048;
    let first=HostNote {port:0,channel:0,key:60,id:81,clap:true};
    let second=HostNote {id:82,..first};
    let pattern=|note:HostNote| HostPattern {port:0,channel:0,key:60,id:note.id,clap:true};
    for flex in [false,true] {
        let group=saved(flex,[0,0,1,0],1);
        assert_eq!(group.native_primary_bypass_slot,Some(0));
        assert!(group.modulators[0].targets.iter().all(|name|name!="Volume"),"admission uses raw addresses");
        let json=serde_json::to_value(&group).unwrap();
        let restored:Group=serde_json::from_value(json.clone()).unwrap();
        assert_eq!(restored.native_primary_bypass_slot,Some(0));
        assert_eq!(restored.modulators,group.modulators);
        assert_eq!(restored.volume_env,group.volume_env);
        assert_eq!(restored.flex_env,group.flex_env);
        let mut legacy=json; legacy.as_object_mut().unwrap().remove("native_primary_bypass_slot");
        assert_eq!(serde_json::from_value::<Group>(legacy).unwrap().native_primary_bypass_slot,None);
        for flags in [[0,1,1,0],[0,0,0,0],[0,0,1,1]] {
            assert_eq!(saved(flex,flags,1).native_primary_bypass_slot,None);
        }
        assert_eq!(saved(flex,[0,0,1,0],2).native_primary_bypass_slot,None,"second actual volume target is not inferred from its label");
        let par=EnginePar {id:id::INTMOD_BYPASS,group:0,slot:0,generic:-1};
        let address=Address::resolve(par,std::slice::from_ref(&restored)).unwrap();
        for shared in [false,true] { for mute in [false,true] {
            let mut engines=[create(&restored,shared,4096,false),create(&restored,shared,4096,false),
                create(&restored,shared,4096,false),create(&restored,shared,4096,false)];
            let mut dry=[create(&Group::default(),false,4096,false),create(&Group::default(),false,4096,false)];
            for engine in &mut dry { engine.state.settings[0].envelope=Some(PreparedAhdsr::UNITY); }
            let prepared=crate::engine::GroupSettings::from(&restored);
            let mut native:[_;2]=std::array::from_fn(|_|crate::engine::ahdsr::Native::new(&prepared.envelope.unwrap_or(PreparedAhdsr::UNITY),48000.));
            let mut flexible=[Envelope::flex();2];
            let mut levels=[[0.;FRAMES];2];
            let mut fading=[1f32;2];
            let step=-1f32/479.;
            // Reference owns source clocks and raw f32 fade arithmetic, with no
            // production PrimaryAmplitude state or bypass API in its engines.
            for frame in 0..FRAMES {
                for note in 0..2 {
                    let onset=if note==0 {0} else {317}; let off=if note==0 {713} else {853};
                    if frame<onset {continue;}
                    let bypass=(151..317).contains(&frame)||frame>=529;
                    if frame==off {
                        if flex {flexible[note].release(prepared.flex.as_ref());} else {native[note].release();}
                    }
                    let mut value=[1.];
                    if !bypass {
                        if flex {flexible[note].render(&mut value,prepared.flex.as_ref(),48000.);}
                        else {native[note].render(&mut value);}
                    }
                    if frame>=off {
                        if frame<off+480 {value[0]*=fading[note];fading[note]=(fading[note]+step).clamp(0.,1.);}
                        else {value[0]=0.;}
                    }
                    levels[note][frame]=value[0];
                }
            }
            let mut pcm=[[[0.;FRAMES];2];4]; let mut unmodulated=[[[0.;FRAMES];2];2];
            assert_eq!(crate::plugin::tests::allocations(|| {
                for engine in &mut engines {assert!(engine.host_note_on(first,0,60,127,0.));engine.cc(0,64,127);}
                dry[0].note_on(0,60,127);
                let mut start=0;
                for end in [103,151,317,350,528,529,713,853,1193,1194,1333,1334,FRAMES] {
                    for (index,engine) in engines.iter_mut().enumerate() {
                        let mut at=start;let mut part=0;
                        while at<end {
                            let n=if index==1 {[1,7,31,127,13,64][part%6].min(end-at)} else {(end-at).min(128)};
                            let [l,r]=&mut pcm[index];engine.render_checked(&mut l[at..at+n],&mut r[at..at+n]).unwrap();
                            engine.render_checked(&mut [],&mut []).unwrap();at+=n;part+=1;
                        }
                        if index<3 {
                            assert_eq!(engine.active_voices(),usize::from(end<1193)+usize::from(end>317&&end<1333),"exact Engine boundary{end}");
                        }
                    }
                    for (index,engine) in dry.iter_mut().enumerate() {
                        let [l,r]=&mut unmodulated[index];engine.render_checked(&mut l[start..end],&mut r[start..end]).unwrap();
                    }
                    if mute&&matches!(end,103|528) {
                        for engine in engines.iter_mut().chain(dry.iter_mut()) {engine.set_expression_on(0,60,
                            Expression {gain:if end==103 {0.} else {1.},..Default::default()});}
                    }
                    if matches!(end,151|317|529) {
                        for (index,engine) in engines.iter_mut().enumerate() {
                            if index==3&&end==529 {continue;}
                            assert!(params::write(&mut engine.state.settings,address,f32::from(end!=317)));
                            assert_eq!(params::read(&engine.state.settings,address),Some(f32::from(end!=317)));
                        }
                    }
                    if end==317 {
                        for engine in &mut engines {assert!(engine.host_note_on(second,0,60,127,0.));}
                        dry[1].note_on(0,60,127);
                    }
                    if end==350 {
                        for engine in &mut engines[..2] {engine.host_note_off(pattern(first));engine.host_note_off(pattern(first));}
                        engines[3].host_note_off(pattern(first));
                        assert!(engines[0].player.voices.iter().any(|voice|!voice.held&&!voice.released));
                    }
                    if end==713 {
                        engines[2].host_note_off(pattern(first));
                        for engine in &mut engines {engine.cc(0,64,0);}
                        assert!(engines[0].player.voices.iter().any(|voice|voice.released&&!voice.held));
                    }
                    if end==853 {for engine in &mut engines {engine.host_note_off(pattern(second));}}
                    start=end;
                }
                for engine in &mut engines[..3] {
                    engine.mark_host_notes();assert_eq!(engine.host_note_end_at(first),Some(1193));
                    assert_eq!(engine.host_note_end_at(second),Some(1333));
                }
            }),0);
            for index in 0..3 {for channel in 0..2 {for frame in 0..FRAMES {
                let expected=unmodulated[0][channel][frame]*levels[0][frame]
                    +unmodulated[1][channel][frame]*levels[1][frame];
                assert!((pcm[index][channel][frame]-expected).abs()<2e-6,
                    "complete independent PCM flex={flex} shared={shared} mute={mute} index={index} frame={frame}");
            }}}
            assert!(pcm[0].iter().flatten().zip(pcm[3].iter().flatten()).any(|(a,b)|(a-b).abs()>1e-5),"ordinary source-owned release differs from bypassed10ms stop");
            if mute {assert!(pcm.iter().all(|note|note.iter().all(|channel|channel[343..528].iter().all(|&value|value==0.))));}
        }}
        let mut eof=create(&restored,false,96,false);eof.note_on(0,60,127);
        assert!(params::write(&mut eof.state.settings,address,1.));
        eof.render_checked(&mut [0.;128],&mut [0.;128]).unwrap();assert_eq!(eof.active_voices(),0);
        let mut stopped=create(&restored,false,4096,false);stopped.note_on(0,60,127);
        assert!(params::write(&mut stopped.state.settings,address,1.));stopped.cc(0,120,0);
        stopped.render_checked(&mut [0.;256],&mut [0.;256]).unwrap();assert_eq!(stopped.active_voices(),0);
        let mut looped=create(&restored,false,4096,true);
        assert!(!params::write(&mut looped.state.settings,address,1.));assert_eq!(params::read(&looped.state.settings,address),None);
        let instrument=crate::import::Instrument {groups:vec![restored.clone()],zones:vec![Zone {loop_range:Some(Loop {
            start:0,end:4096,crossfade:0,alternating:false,until_release:false}),..Default::default()}],..Default::default()};
        use crate::ksp::KspEngine;
        let mut setup=crate::engine::ScriptSetup::new(&instrument,48000.);
        assert_eq!(setup.engine_par(par),None);assert!(!setup.set_engine_par(0,par,1));
        let bank=Bank::from_samples(vec![restored.clone()],vec![Zone::default()],vec![(Default::default(),Sample {
            rate:48000,frames:vec![[0.25;2];4096]})]).unwrap();
        assert!(bank.with_prepared_settings(|rows|rows[0].native_volume_env=!rows[0].native_volume_env).is_err());
        let mut remap=Bank::from_samples(vec![restored.clone(),Group::default()],vec![Zone::default(),
            Zone {group:1,loop_range:Some(Loop {start:0,end:4096,crossfade:0,alternating:false,until_release:false}),
                ..Default::default()}],vec![(Default::default(),Sample {rate:48000,frames:vec![[0.25;2];4096]})]).unwrap();
        let edit=crate::ksp::engine::ZoneEdit {zone:1,par:crate::ksp::engine::ZonePar::Group,value:0,slot:0,id:-1};
        let error=remap.prepare_zone_init(1,&[edit],&||false).unwrap_err();
        assert!(format!("{error:#}").contains("requires an unlooped sample zone"));
        assert_eq!(remap.zones()[1].group,1,"failed worker does not acquire amplitude source ownership");
    }
}
