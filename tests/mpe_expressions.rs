//! Owned loose format fixtures -> ordinary translator/load -> MIDI MPE -> rich
//! authored VoiceChain -> core post-chain Tone. Not a native audio parity claim.
//! These public loader defaults use Tone. The product host may instead select
//! MpeDefaults::for_instrument (authored Cutoff/Controller); that policy is not
//! substituted here or claimed to be the same recipe.
use sampler_core::{Expression,Limits,NoteId,Runtime};
use sampler_midi::{Applied,Mpe,Packets,Zone};
use sampler_ir as ir;
use std::path::PathBuf;
#[path="../crates/sampler-kontakt/tests/support/chunks.rs"]
mod wire;
#[path="../crates/sampler-core/tests/support/mod.rs"]
mod support;

#[derive(Clone,Copy,Debug)]
enum Format { Kontakt,Uvi }
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        // The development checkout is on the dev drive; this is also portable
        // in CI and never asks the host to write under its system temp root.
        let base=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-artifacts");
        std::fs::create_dir_all(&base).unwrap();
        let dir=base.join(format!("mpe-rich-{}-{}",std::process::id(),NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("owned.wav"),wave()).unwrap();
        std::fs::write(dir.join("owned.nki"),kontakt(true)).unwrap();
        std::fs::write(dir.join("dry.nki"),kontakt(false)).unwrap();
        std::fs::write(dir.join("owned.uvip"),r#"<UVI4><Program Name='Owned rich MPE' Gain='1'><Layers><Layer Gain='1'><Keygroups>
        <Keygroup LowKey='60' HighKey='61' Gain='1'><Inserts><WaveShaper Mode='6'/><OnePole Freq='4000' Mode='0' KeyTracking='0'/></Inserts>
        <Oscillators><SamplePlayer SamplePath='owned.wav' BaseNote='60' Gain='1' NoteTracking='1'/></Oscillators>
        </Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#).unwrap();
        let rich=std::fs::read_to_string(dir.join("owned.uvip")).unwrap();
        let dry=rich.replace("<Inserts><WaveShaper Mode='6'/><OnePole Freq='4000' Mode='0' KeyTracking='0'/></Inserts>","");
        assert_ne!(rich,dry);
        std::fs::write(dir.join("dry.uvip"),dry).unwrap();
        Self(dir)
    }
    fn load_path(&self,format:Format,stem:&str)->sampler_kontakt::Loaded {
        assert_eq!(sampler_kontakt::Options::default().mpe.unwrap().timbre,sampler_core::lower::TimbreTarget::Tone);
        match format {
            Format::Kontakt=>sampler_kontakt::load(&self.0.join(format!("{stem}.nki")),&sampler_kontakt::Options{
                scripts:false,..Default::default()},|_|{}).unwrap(),
            Format::Uvi=>sampler_uvi::assemble_translated(sampler_uvi::translate_path(&self.0.join(format!("{stem}.uvip"))).unwrap(),48000).unwrap(),
        }
    }
    fn loaded(&self,format:Format)->sampler_kontakt::Loaded {
        let loaded=self.load_path(format,"owned");
        assert_eq!(loaded.instrument.zones.len(),1,"{format:?}");
        let chain=loaded.instrument.zones[0].chain.expect("authored voice chain must survive translation");
        let chain=&loaded.instrument.chains[chain.0];
        assert!(matches!(chain.scope,ir::Scope::Voice));
        let stages:Vec<_>=chain.pre_amplitude.iter().chain(&chain.post_amplitude).collect();
        assert!(stages.iter().any(|p|matches!(p,ir::Processor::Color(_))),"nonlinear stage dropped: {format:?}");
        assert!(stages.iter().any(|p|matches!(p,ir::Processor::Filter(_)|ir::Processor::FilterBank(_))),"authored filter dropped: {format:?}");
        assert!(!loaded.instrument.unsupported.iter().any(|u|u.feature=="effect decoding"||u.feature.contains("not modelled")||u.feature.contains("not modeled")),"{:?}",loaded.instrument.unsupported);
        loaded
    }
    fn runtime(&self,format:Format)->Runtime {
        let loaded=self.loaded(format);let limits=Limits::for_plan(&loaded.plan,16,32);Runtime::new(loaded.plan,limits).unwrap()
    }
}
impl Drop for Fixture { fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);} }
fn wave()->Vec<u8> {
    const FRAMES:usize=16384;let mut b=b"RIFF".to_vec();b.extend((36+FRAMES as u32*2).to_le_bytes());
    b.extend(b"WAVEfmt ");b.extend(16u32.to_le_bytes());b.extend(1u16.to_le_bytes());b.extend(1u16.to_le_bytes());
    b.extend(48000u32.to_le_bytes());b.extend(96000u32.to_le_bytes());b.extend(2u16.to_le_bytes());b.extend(16u16.to_le_bytes());
    b.extend(b"data");b.extend((FRAMES as u32*2).to_le_bytes());
    for i in 0..FRAMES { let value=((i*137)%24576) as i16-12288;b.extend(value.to_le_bytes()); } b
}
fn wide(s:&str)->Vec<u8> {let mut out=(s.encode_utf16().count() as u32).to_le_bytes().to_vec();out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));out}
fn effect(module:u16,version:u16,public:&[u8])->Vec<u8> {
    let mut state=20u32.to_le_bytes().to_vec();state.extend([0;6]);state.extend(1f32.to_le_bytes());state.extend(0f32.to_le_bytes());state.extend((-1i32).to_le_bytes());
    wire::chunk(0x25,&wire::object(0x50,&state,&[],&wire::chunk(module,&wire::object(version,&[],public,&[]))))
}
fn kontakt(rich:bool)->Vec<u8> {
    let mut group=wide("Owned nonlinear and filter");for v in [1f32,0.,1.] {group.extend(v.to_le_bytes());}
    group.extend([1,0,0,0]);group.extend(0i32.to_le_bytes());group.extend((-1i16).to_le_bytes());
    group.extend((-1i32).to_le_bytes());group.extend(2i32.to_le_bytes());group.extend([0,0]);group.extend(0i32.to_le_bytes());
    let mut private=Vec::new();for _ in 0..136 {private.extend(8u32.to_le_bytes());private.extend([0;8]);}private.extend([0;24]);
    private.extend([0,0x13,0]);private.extend(8u32.to_le_bytes());
    let mut saturation=0.75f32.to_le_bytes().to_vec();saturation.extend(1i32.to_le_bytes());
    let mut filter=52i32.to_le_bytes().repeat(2);filter.extend(0.4f32.to_le_bytes());filter.extend(0.2f32.to_le_bytes());
    for slot in 0..8 {private.push(u8::from(rich&&slot<2));if rich&&slot==0 {private.extend(effect(0x1d,0x80,&saturation));}if rich&&slot==1 {private.extend(effect(0x18,0x90,&filter));}}
    private.push(1);private.extend([0,6,1]);private.extend(0u32.to_le_bytes());private.extend([0;23]);private.extend([255;22]);
    let mut groups=1u32.to_le_bytes().to_vec();groups.extend(wire::object(0x95,&private,&group,&wire::chunk(0x38,&0u32.to_le_bytes())));
    let mut zone=vec![0;12];for v in [1i16,127,60,61,0,0,0,0,60] {zone.extend(v.to_le_bytes());}
    for v in [1f32,0.,1.] {zone.extend(v.to_le_bytes());}zone.extend([0,1,0,0,0,0]);
    for v in [0i32,0,48000] {zone.extend(v.to_le_bytes());}zone.push(1);
    for v in [16384i32,0,60] {zone.extend(v.to_le_bytes());}zone.extend(0f32.to_le_bytes());zone.push(0);zone.extend(0i32.to_le_bytes());
    let mut zones=1u32.to_le_bytes().to_vec();zones.extend(0u32.to_le_bytes());zones.extend(wire::object(0x9a,&[],&zone,&[]));
    let mut program=wide("Owned MPE source-free fixture");program.extend(0f64.to_le_bytes());program.push(0);
    for v in [1f32,0.,1.] {program.extend(v.to_le_bytes());}program.extend([1,127,0,127]);program.extend((-1i16).to_le_bytes());program.extend([0;16]);program.push(0);program.extend(0i32.to_le_bytes());
    for _ in 0..3 {program.extend(wide(""));}program.extend([0;6]);
    let mut children=wire::chunk(0x33,&groups);children.extend(wire::chunk(0x34,&zones));
    let mut payload=wire::chunk(0x28,&wire::object(0xae,&[],&program,&children));
    let mut table=vec![0;4];table.extend(1u32.to_le_bytes());table.extend(1i32.to_le_bytes());table.push(4);table.extend(wide("owned.wav"));table.extend([0;12]);payload.extend(wire::chunk(0x3d,&table));
    let packed:Vec<_>=payload.chunks(32).flat_map(|b|std::iter::once((b.len()-1) as u8).chain(b.iter().copied())).collect();
    let mut nks=vec![0;222];nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());nks[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());nks[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());nks.extend(packed);nks.extend(0xb00ee1aeu32.to_le_bytes());nks.extend([1,1,12,0]);nks
}
fn packet(status:u8,channel:u8,a:u8,b:u8)->u32 {0x2300_0000|(u32::from(status|channel)<<16)|(u32::from(a)<<8)|u32::from(b)}
fn apply(mpe:&mut Mpe,rt:&mut Runtime,word:u32)->Applied {mpe.apply(rt,Packets::new(&[word]).next().unwrap().unwrap()).unwrap()}
fn start(mpe:&mut Mpe,rt:&mut Runtime,channel:u8)->NoteId {let Applied::Started(n)=apply(mpe,rt,packet(0x90,channel,60,127)) else {panic!("no note admitted")};n}
fn expression(rt:&Runtime,note:NoteId)->Expression {rt.expression(rt.expression_id(note).unwrap()).unwrap()}
fn render(rt:&mut Runtime,frames:usize,fragment:usize)->Vec<[f32;2]> {let mut out=vec![[0.;2];frames];for b in out.chunks_mut(fragment){rt.render(b).unwrap();}assert_eq!((rt.nonfinite_frames(),rt.stream_underruns()),(0,0));out}
fn close(a:&[[f32;2]],b:&[[f32;2]]) {assert_eq!(a.len(),b.len());for (frame,(a,b)) in a.iter().zip(b).enumerate(){for c in 0..2 {assert!((a[c]-b[c]).abs()<3e-6,"frame{frame}/ch{c}:{} !={}",a[c],b[c]);}}}
fn sum(a:&[[f32;2]],b:&[[f32;2]])->Vec<[f32;2]> {a.iter().zip(b).map(|(a,b)|[a[0]+b[0],a[1]+b[1]]).collect()}

fn bend(channel:u8,value:u16)->u32 {packet(0xe0,channel,(value&127) as u8,(value>>7) as u8)}
fn owners(f:&Fixture,format:Format)->(Runtime,Mpe) {let rt=f.runtime(format);let mpe=Mpe::new(&rt,7,3,Zone::Lower,2,16).unwrap();(rt,mpe)}
fn gestures(m:&mut Mpe,r:&mut Runtime,ch:u8,timbre:u8,pressure:u8,pitch:u16) {
    apply(m,r,packet(0xb0,ch,74,timbre));apply(m,r,packet(0xd0,ch,pressure,0));apply(m,r,bend(ch,pitch));
}
fn active_audio(x:&[[f32;2]]) {assert!(x.iter().flatten().all(|v|v.is_finite()));assert!(x.iter().flatten().any(|v|v.abs()>1e-5),"silent fixture is not an audio oracle");}
fn independent_members(format:Format) {
    let f=Fixture::new();let (mut mixed,mut mm)=owners(&f,format);let (mut a,mut ma)=owners(&f,format);let (mut b,mut mb)=owners(&f,format);
    gestures(&mut mm,&mut mixed,1,110,20,8256);gestures(&mut ma,&mut a,1,110,20,8256);
    gestures(&mut mm,&mut mixed,2,80,70,8128);gestures(&mut mb,&mut b,2,80,70,8128);
    let na=start(&mut mm,&mut mixed,1);let nb=start(&mut mm,&mut mixed,2);
    let sa=start(&mut ma,&mut a,1);let sb=start(&mut mb,&mut b,2);
    assert_ne!(mixed.expression_id(na),mixed.expression_id(nb));assert_eq!(mixed.voice_count(),2);
    assert_ne!(expression(&mixed,na).pressure,expression(&mixed,nb).pressure);
    assert_ne!(expression(&mixed,na).timbre,expression(&mixed,nb).timbre);
    assert_ne!(expression(&mixed,na).pitch_semitones,expression(&mixed,nb).pitch_semitones);
    let baseline=render(&mut mixed,512,64);active_audio(&baseline);close(&baseline,&sum(&render(&mut a,512,64),&render(&mut b,512,64)));
    let sibling=expression(&mixed,nb);
    support::without_heap(|| {gestures(&mut mm,&mut mixed,1,3,110,8352);gestures(&mut ma,&mut a,1,3,110,8352);});
    assert_eq!(expression(&mixed,nb),sibling);assert_eq!(mixed.voice_count(),2);assert!(mixed.key_down(na).unwrap()&&mixed.key_down(nb).unwrap());
    let changed=render(&mut mixed,512,37);close(&changed,&sum(&render(&mut a,512,37),&render(&mut b,512,37)));
    assert_eq!(expression(&mixed,na),expression(&a,sa));assert_eq!(expression(&mixed,nb),expression(&b,sb));
    // End only member 1, then reuse its member channel while the original
    // member 2's physical voice/history remains uninterrupted.
    assert!(matches!(apply(&mut mm,&mut mixed,packet(0x80,1,60,0)),Applied::Released{note,..} if note==na));
    assert!(matches!(apply(&mut ma,&mut a,packet(0x80,1,60,0)),Applied::Released{note,..} if note==sa));
    assert!(!mixed.key_down(na).unwrap());let released=expression(&mixed,na);
    let new=start(&mut mm,&mut mixed,1);let new_solo=start(&mut ma,&mut a,1);assert_ne!(new,na);
    gestures(&mut mm,&mut mixed,1,127,0,8192);gestures(&mut ma,&mut a,1,127,0,8192);
    assert_eq!(expression(&mixed,na),released);assert_eq!(expression(&mixed,nb),sibling);assert_eq!(expression(&mixed,new),expression(&a,new_solo));
    close(&render(&mut mixed,512,64),&sum(&render(&mut a,512,64),&render(&mut b,512,64)));
    let mut block=[[0.;2];64];
    support::without_heap(|| {for _ in 0..4 {gestures(&mut mm,&mut mixed,1,30,50,8200);mixed.render(&mut block).unwrap();}mixed.panic();mixed.render(&mut block).unwrap();mixed.flush_ended(|_|true);});
    assert_eq!((mixed.note_count(),mixed.expression_count(),mixed.voice_count()),(0,0,0));
    assert!(block.iter().flatten().all(|v|*v==0.));assert_eq!((mixed.nonfinite_frames(),mixed.stream_underruns()),(0,0));
}

// Independent public-law reference: consumes the already rendered authored
// chain's stereo outlet. It calls no production Tone/coefficient helpers.
fn postchain_reference(open:&[[f32;2]])->Vec<[f32;2]> {
    let mut state=[[0f64;2];2];let mut out=Vec::with_capacity(open.len());
    for (index,input) in open.iter().enumerate() {
        // Change arrives on absolute frame 512, a 64-frame control boundary.
        // Existing midpoint law: 0 -> -60 in the first cell, then -60.
        let semitones=if index<64 {-30f64}else{-60f64};
        let cutoff=(0.45*48000.*(semitones/12.).exp2()).max(10.);
        let g=(std::f64::consts::PI*cutoff/48000.).tan();let k=std::f64::consts::SQRT_2;
        let a1=1./(1.+g*(g+k));let a2=g*a1;let a3=g*g*a1;
        let mut frame=[0.;2];for c in 0..2 {let [s1,s2]=state[c];let v3=f64::from(input[c])-s2;let v1=a1*s1+a2*v3;let v2=s2+a2*s1+a3*v3;state[c]=[2.*v1-s1,2.*v2-s2];frame[c]=v2 as f32;}
        out.push(frame);
    }out
}
fn postchain_oracle(format:Format) {
    let f=Fixture::new();let (mut actual,mut ma)=owners(&f,format);let (mut open,mut mo)=owners(&f,format);
    gestures(&mut ma,&mut actual,1,127,0,8192);gestures(&mut mo,&mut open,1,127,0,8192);
    let note=start(&mut ma,&mut actual,1);start(&mut mo,&mut open,1);
    let warm=render(&mut actual,512,64);active_audio(&warm);close(&warm,&render(&mut open,512,64));
    let before=expression(&actual,note);apply(&mut ma,&mut actual,packet(0xb0,1,74,0));
    assert_eq!(actual.voice_count(),1);assert!(actual.key_down(note).unwrap());assert_eq!(expression(&actual,note).pitch_semitones,before.pitch_semitones);
    let unfiltered=render(&mut open,512,64);let expected=postchain_reference(&unfiltered);let observed=render(&mut actual,512,64);
    active_audio(&observed);close(&observed,&expected);assert!(observed.iter().zip(&unfiltered).any(|(a,b)|(a[0]-b[0]).abs()>1e-4),"CC74 did not close post-chain Tone");
    // The authored chain must also affect actual PCM, beyond its retained IR.
    let dry_loaded=f.load_path(format,"dry");let limits=Limits::for_plan(&dry_loaded.plan,16,32);let mut dry=Runtime::new(dry_loaded.plan,limits).unwrap();
    let mut md=Mpe::new(&dry,7,3,Zone::Lower,2,16).unwrap();gestures(&mut md,&mut dry,1,127,0,8192);start(&mut md,&mut dry,1);
    let dry_pcm=render(&mut dry,512,64);active_audio(&dry_pcm);assert!(warm.iter().zip(&dry_pcm).any(|(a,b)|(a[0]-b[0]).abs()>1e-4),"retained nonlinear/filter chain had no audible effect");
}
#[test] fn kontakt_normal_rich_chain_mpe_members_are_independent(){independent_members(Format::Kontakt);}
#[test] fn uvi_normal_rich_chain_mpe_members_are_independent(){independent_members(Format::Uvi);}
#[test] fn kontakt_normal_rich_chain_tone_matches_independent_postchain_oracle(){postchain_oracle(Format::Kontakt);}
#[test] fn uvi_normal_rich_chain_tone_matches_independent_postchain_oracle(){postchain_oracle(Format::Uvi);}

fn each_held_axis_is_audible(format:Format) {
    let f=Fixture::new();let (mut unchanged,mut mu)=owners(&f,format);
    let (mut pressure,mut mp)=owners(&f,format);let (mut pitch,mut mb)=owners(&f,format);let (mut timbre,mut mt)=owners(&f,format);
    for (rt,mpe) in [(&mut unchanged,&mut mu),(&mut pressure,&mut mp),(&mut pitch,&mut mb),(&mut timbre,&mut mt)] {
        gestures(mpe,rt,1,127,0,8192);start(mpe,rt,1);active_audio(&render(rt,512,64));
    }
    // Each uses a fresh retained physical voice and changes one expression axis.
    // No new note, IR rewrite, authored filter edit or disabled MPE option.
    apply(&mut mp,&mut pressure,packet(0xd0,1,127,0));
    apply(&mut mb,&mut pitch,bend(1,8256));
    apply(&mut mt,&mut timbre,packet(0xb0,1,74,0));
    let neutral=render(&mut unchanged,512,64);
    for (axis,rt) in [("pressure",&mut pressure),("pitch",&mut pitch),("timbre",&mut timbre)] {
        assert_eq!(rt.voice_count(),1);let pcm=render(rt,512,64);active_audio(&pcm);
        assert!(pcm.iter().zip(&neutral).any(|(a,b)|(a[0]-b[0]).abs()>1e-4),"{format:?} held {axis} updated metadata but not audio");
    }
}
#[test] fn kontakt_each_normal_held_mpe_axis_changes_rich_chain_audio(){each_held_axis_is_audible(Format::Kontakt);}
#[test] fn uvi_each_normal_held_mpe_axis_changes_rich_chain_audio(){each_held_axis_is_audible(Format::Uvi);}
