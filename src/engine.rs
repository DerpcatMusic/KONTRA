use crate::{audio::{self,Sample}, import::{Instrument,Zone,Group}};
use anyhow::{Context,Result,ensure};
use std::collections::HashMap;

pub const MAX_VOICES: usize = 256;
pub const MEMORY_LIMIT: usize = 1024 * 1024 * 1024;

pub struct Bank { pub zones: Vec<Zone>, pub groups: Vec<Group>, pub samples: Vec<Sample>, pub sample_ids: Vec<usize>, pub bytes: usize }
impl Bank {
    pub fn load(instrument: &Instrument, group: usize) -> Result<Self> {
        ensure!(group < instrument.groups.len(),"Invalid group {group}");
        // ponytail: one selected group is RAM resident (1 GiB ceiling); add DFD for larger groups.
        let zones:Vec<_>=instrument.zones.iter().filter(|z|z.group==group).cloned().collect();
        ensure!(!zones.is_empty(),"Selected group has no zones");
        let mut decoder=audio::Decoder::default();
        let mut samples=Vec::new(); let mut ids=HashMap::new(); let mut sample_ids=Vec::new(); let mut bytes=0;
        for z in &zones {
            ensure!(z.available,"Sample is missing or its archive member is damaged: {}",z.sample.display());
            let id=if let Some(&id)=ids.get(&z.sample) {id} else {
                let sample=decoder.decode(&z.sample,(MEMORY_LIMIT-bytes)/8).with_context(||format!("Decoding {}",z.sample.display()))?;
                bytes+=sample.frames.len()*8;
                let id=samples.len();ids.insert(z.sample.clone(),id);samples.push(sample);id
            };
            let frames=samples[id].frames.len();
            ensure!(z.start < frames && (frames as i64 + z.end as i64 > z.start as i64),"Invalid sample bounds for {}",z.sample.display());
            if let Some(l)=&z.loop_range { ensure!(l.start>=z.start && l.start<l.end && l.end<=(frames as i64 + z.end as i64) as usize && l.crossfade<=l.end-l.start,"Invalid loop for {}",z.sample.display()); }
            sample_ids.push(id);
        }
        Ok(Self {zones,groups:instrument.groups.clone(),samples,sample_ids,bytes})
    }
}

#[derive(Clone,Copy)]
struct Voice { zone: usize, channel: usize, note: u8, position: f64, step: f64, velocity: f32, env: f32, release_step: f32, releasing: bool, held: bool, age: u64, filter: [f32;2] }

pub struct Engine {
    pub bank: Option<Box<Bank>>,
    voices: Vec<Voice>,
    pub rate: f64,
    sustain: [bool;16],
    bend: [f64;16],
    expression: [f32;16],
    last_velocity: [[u8;128];16],
    clock: u64,
    pub attack: f32,
    pub release: f32,
    pub cutoff: f32,
}
impl Default for Engine {
    fn default()->Self {Self {bank:None,voices:Vec::with_capacity(MAX_VOICES),rate:48000.0,sustain:[false;16],bend:[1.0;16],expression:[1.0;16],last_velocity:[[0;128];16],clock:0,attack:0.002,release:0.15,cutoff:20000.0}}
}
impl Engine {
    pub fn reset(&mut self,rate:f64) {self.voices.clear();self.rate=rate;self.sustain=[false;16];self.bend=[1.0;16];self.expression=[1.0;16];self.last_velocity=[[0;128];16];}
    pub fn active_voices(&self)->usize {self.voices.len()}
    pub fn note_on(&mut self,channel:u8,note:u8,velocity:u8) {
        if channel>=16 || note>=128 {return;}
        if velocity==0 {self.note_off(channel,note);return;}
        self.last_velocity[channel as usize][note as usize]=velocity;
        self.start(channel,note,velocity,false);
    }
    fn start(&mut self,channel:u8,note:u8,velocity:u8,release:bool) {
        let Some(bank)=&self.bank else {return;};
        self.clock=self.clock.wrapping_add(1);
        for (index,z) in bank.zones.iter().enumerate() {
            let g=&bank.groups[z.group];
            if g.muted || g.release_trigger!=release || (g.channel>=0 && g.channel!=channel as i16) || note<z.low_key || note>z.high_key || velocity<z.low_velocity || velocity>z.high_velocity {continue;}
            let sample=&bank.samples[bank.sample_ids[index]];
            let step=sample.rate as f64/self.rate*z.tune*g.tune*if g.key_tracking {2f64.powf((note as f64-z.root as f64)/12.0)}else{1.0};
            let end=(sample.frames.len() as i64 + z.end as i64) as usize;
            let voice=Voice {zone:index,channel:channel as usize,note,position:if g.reverse {(end-1) as f64}else{z.start as f64},step,velocity:velocity as f32/127.0,env:0.0,release_step:0.0,releasing:false,held:true,age:self.clock,filter:[0.0;2]};
            if self.voices.len()==MAX_VOICES {
                let oldest=self.voices.iter().enumerate().min_by_key(|(_,v)|v.age).map(|(i,_)|i).unwrap();self.voices[oldest]=voice;
            }else{self.voices.push(voice);}
        }
    }
    fn release_voice(v:&mut Voice,rate:f64,release:f32) {v.releasing=true;v.release_step=v.env/(release.max(0.001)*rate as f32);}
    pub fn note_off(&mut self,channel:u8,note:u8) {
        if channel>=16 || note>=128 {return;}
        let c=channel as usize;
        let velocity=self.last_velocity[c][note as usize];
        self.last_velocity[c][note as usize]=0;
        for v in &mut self.voices {
            if v.channel==c && v.note==note && v.held {v.held=false;if !self.sustain[c] {Self::release_voice(v,self.rate,self.release);}}
        }
        if velocity>0 && !self.sustain[c] {self.start(channel,note,velocity,true);}
    }
    pub fn cc(&mut self,channel:u8,cc:u8,value:u8) {
        if channel>=16 {return;}
        let c=channel as usize;
        match cc {
            64=> {let on=value>=64;if self.sustain[c] && !on {for v in &mut self.voices {if v.channel==c && !v.held && !v.releasing {Self::release_voice(v,self.rate,self.release);}}}self.sustain[c]=on;},
            11=>self.expression[c]=value as f32/127.0,
            120=>self.voices.retain(|v|v.channel!=c),
            123=> {for n in 0..128 {self.note_off(channel,n);}},
            121=> {self.cc(channel,64,0);self.bend[c]=1.0;self.expression[c]=1.0;},
            _=>(),
        }
    }
    pub fn pitch_bend(&mut self,channel:u8,value:u16) {if channel<16 {self.bend[channel as usize]=2f64.powf((value.min(16383) as f64-8192.0)/8192.0*2.0/12.0);}}
    pub fn frame(&mut self)->[f32;2] {
        let Some(bank)=&self.bank else {return [0.0;2];};
        let mut out=[0.0;2];
        let attack_step=1.0/(self.attack.max(0.0001)*self.rate as f32);
        let filter=1.0-(-std::f32::consts::TAU*self.cutoff.min(self.rate as f32*0.45)/self.rate as f32).exp();
        for v in &mut self.voices {
            let z=&bank.zones[v.zone];let g=&bank.groups[z.group];let s=&bank.samples[bank.sample_ids[v.zone]];
            let end=(s.frames.len() as i64 + z.end as i64) as usize;
            if v.position<z.start as f64 || v.position>=end as f64 {v.env=-1.0;continue;}
            v.env=if v.releasing {v.env-v.release_step}else{(v.env+attack_step).min(1.0)};
            if v.env<=0.0 {continue;}
            let looping=z.loop_range.as_ref().filter(|l|!g.reverse && (!l.until_release || !v.releasing));
            let i=v.position as usize;let frac=(v.position-i as f64) as f32;
            let next=if let Some(l)=looping {if i+1>=l.end {l.start}else{(i+1).min(end-1)}}else{(i+1).min(end-1)};
            let pan=(z.pan+g.pan).clamp(-1.0,1.0);
            let gain=z.gain*g.gain*v.env*v.velocity*self.expression[v.channel];
            for ch in 0..2 {
                let mut x=s.frames[i][ch]+(s.frames[next][ch]-s.frames[i][ch])*frac;
                if let Some(l)=looping.filter(|l|l.crossfade>0 && v.position >= (l.end-l.crossfade) as f64 && v.position < l.end as f64) {
                    let t=((v.position-(l.end-l.crossfade) as f64)/l.crossfade as f64) as f32;
                    let pos=l.start as f64+v.position-(l.end-l.crossfade) as f64;let j=pos as usize;let f=(pos-j as f64) as f32;
                    let y=s.frames[j][ch]+(s.frames[(j+1).min(l.end-1)][ch]-s.frames[j][ch])*f;
                    x=x*(1.0-t)+y*t;
                }
                v.filter[ch]+=filter*(x-v.filter[ch]);
                let x=if self.cutoff>=20000.0 {x}else{v.filter[ch]};
                out[ch]+=x*gain*if ch==0 {1.0-pan.max(0.0)}else{1.0+pan.min(0.0)};
            }
            v.position+=v.step*self.bend[v.channel]*if g.reverse {-1.0}else{1.0};
            if let Some(l)=looping {if v.position>=l.end as f64 {v.position=l.start as f64+(v.position-l.end as f64)%(l.end-l.start) as f64;}}
        }
        self.voices.retain(|v|v.env>0.0);
        out
    }
}

pub const RACK_SLOTS: usize = 16;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartControls { pub port:u8, pub output:u8, pub channel: i16, pub gain: f32, pub pan: f32, pub mute: bool, pub solo: bool }
impl Default for PartControls { fn default()->Self {Self {port:0,output:0,channel:-1,gain:1.,pan:0.,mute:false,solo:false}} }
/// Fixed engines keep voice allocation and ownership changes outside rendering.
pub struct Rack { pub parts: [Engine;RACK_SLOTS], pub controls: [PartControls;RACK_SLOTS] }
impl Default for Rack {fn default()->Self {Self {parts:std::array::from_fn(|_|Engine::default()),controls:[PartControls::default();RACK_SLOTS]}}}
impl Rack {
    pub fn reset(&mut self,rate:f64) {for e in &mut self.parts {e.reset(rate);}}
    pub fn note_on(&mut self,channel:u8,note:u8,velocity:u8) {self.note_on_port(0,channel,note,velocity);}
    pub fn note_on_port(&mut self,port:u8,channel:u8,note:u8,velocity:u8) {for (e,c) in self.parts.iter_mut().zip(self.controls) {if c.port==port && (c.channel<0 || c.channel==channel as i16) {e.note_on(channel,note,velocity);}}}
    pub fn set_controls(&mut self,controls:[PartControls;RACK_SLOTS]) {for (n,c) in controls.iter().enumerate() {if (c.port,c.channel)!=(self.controls[n].port,self.controls[n].channel) {self.parts[n].reset(self.parts[n].rate);}}self.controls=controls;}
    pub fn note_off_port(&mut self,port:u8,channel:u8,note:u8) {for (e,c) in self.parts.iter_mut().zip(self.controls) {if c.port==port {e.note_off(channel,note);}}}
    pub fn cc_port(&mut self,port:u8,channel:u8,cc:u8,value:u8) {for (e,c) in self.parts.iter_mut().zip(self.controls) {if c.port==port {e.cc(channel,cc,value);}}}
    pub fn pitch_bend_port(&mut self,port:u8,channel:u8,value:u16) {for (e,c) in self.parts.iter_mut().zip(self.controls) {if c.port==port {e.pitch_bend(channel,value);}}}
    // Releases always reach every engine: changing routing while a key is held must not stick it.
    pub fn note_off(&mut self,channel:u8,note:u8) {for e in &mut self.parts {e.note_off(channel,note);}}
    pub fn cc(&mut self,channel:u8,cc:u8,value:u8) {for e in &mut self.parts {e.cc(channel,cc,value);}}
    pub fn pitch_bend(&mut self,channel:u8,value:u16) {for e in &mut self.parts {e.pitch_bend(channel,value);}}
    pub fn frame_outputs(&mut self)->[[f32;2];8] {
        let solo=self.controls.iter().any(|c|c.solo);let mut out=[[0.;2];8];
        for (e,c) in self.parts.iter_mut().zip(self.controls) {let x=e.frame();if c.mute || (solo && !c.solo){continue;}let bus=&mut out[c.output.min(7) as usize];bus[0]+=x[0]*c.gain*(1.-c.pan.max(0.));bus[1]+=x[1]*c.gain*(1.+c.pan.min(0.));}out
    }
    pub fn frame(&mut self)->[f32;2] {let mut out=[0.;2];for bus in self.frame_outputs(){out[0]+=bus[0];out[1]+=bus[1];}out}
}
