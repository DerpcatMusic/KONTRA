//! PRIVATE, unregistered, unexecuted public-API causal witness.
//! Own clear XML + generated PCM only. No installed library access.
//! Root may register as sampler-uvi example AFTER its finite-file grant.
use sampler_core::{Frame, Input, Limits, Protocol, Runtime};
use sampler_core::trace::{TraceReader, TraceRecord};
use sampler_uvi::script::{Command, Config, ScriptHost};
use sampler_uvi::scripted::{Driver, HostInput, Script};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc};

const RATE: u32 = 48_000;
const PHASE: usize = 24_000;
const WINDOW: usize = 12_000;
const SCOPES: [&str; 5] = ["program", "layer", "keygroup", "player", "constant_value"];

// Observation wrapper forwards the unchanged public ScriptHost, recording only
// typed commands and numeric error counts. It does not synthesize commands.
struct ObservedHost { host: ScriptHost, events: Rc<RefCell<Vec<Value>>> }
impl Script for ObservedHost {
    fn handles_notes(&self) -> bool { self.host.handles_notes() }
    fn set_time(&mut self, ms: f64) { self.host.set_time(ms) }
    fn note_on(&mut self, id: u64, key: u8, velocity: u8) { self.host.note_on(id,key,velocity,0) }
    fn note_off(&mut self, id: u64, key: u8) { self.host.note_off(id,key,64,0) }
    fn advance(&mut self, ms: f64) { self.host.advance(ms) }
    fn next_due(&mut self) -> Option<f64> { self.host.next_due() }
    fn input(&mut self, input: HostInput) { Script::input(&mut self.host,input); }
    fn drain(&mut self, out: &mut Vec<Command>) {
        for c in self.host.take_commands() {
            let row = match &c {
                Command::Parameter { scope,param,value,authored } => json!({"kind":"parameter","scope":format!("{scope:?}"),"parameter":format!("{param:?}"),"value":value,"previous":authored}),
                Command::Modulation { id,value,voice,glide_ms,.. } => json!({"kind":"modulation","id":id,"value":value,"voice":voice,"glide_ms":glide_ms}),
                Command::EngineParameter { address,value } => json!({"kind":"processor_control","address":address,"value":value}),
                Command::Play(p) => json!({"kind":"play","id":p.id,"key":p.key,"velocity":p.velocity,"duration_ms":p.duration_ms,"layers":p.layers.0,"oscillator":p.osc,"volume":p.vol}),
                _ => json!({"kind":"other"}),
            };
            let mut events=self.events.borrow_mut();
            if events.len()<256 { events.push(row); }
            drop(events);
            out.push(c);
        }
        let findings=self.host.findings().len();
        if findings>0 {let mut events=self.events.borrow_mut();if events.len()<256 {events.push(json!({"kind":"finding_count","count":findings}));}}
        let f=self.host.fault_counts();
        if !f.init.is_empty() || !f.runtime.is_empty() {
            let mut events=self.events.borrow_mut();
            if events.len()<256 {events.push(json!({"kind":"fault_counts","init":f.init,"runtime":f.runtime}));}
        }
    }
}

fn wav(frequency: f64) -> Vec<u8> {
    // Four/equivalent integer cycles in a 400-frame authored wrap loop.
    let frames=400usize;
    let mut out=b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
    out.extend(RATE.to_le_bytes()); out.extend((RATE*2).to_le_bytes());
    out.extend(2u16.to_le_bytes()); out.extend(16u16.to_le_bytes());
    out.extend(b"data"); out.extend((frames as u32*2).to_le_bytes());
    for i in 0..frames {
        let sample=((i as f64*frequency*std::f64::consts::TAU/RATE as f64).sin()*8192.).round() as i16;
        out.extend(sample.to_le_bytes());
    }
    let size=(out.len()-8) as u32; out[4..8].copy_from_slice(&size.to_le_bytes()); out
}

fn xml(initial: [f64;5], live_constant: bool, unit_reference: bool) -> String {
    let gate=if live_constant {"if e.controller==24 then Program.modulations['Gate']:setParameter('Value',v) end"} else {""};
    format!(r#"<UVI4><Program Name="Public gain witness" Gain="{p}">
<ControlSignalSources><ConstantModulation Name="Gate" Value="{g}"/></ControlSignalSources>
<EventProcessors><ScriptProcessor><script><![CDATA[
 local targets={{Program,Program.layers[1],Program.layers[1].keygroups[1],Program.layers[1].keygroups[1].oscillators[1]}}
 function onNote(e) playNote(e.note,e.velocity,-1) end
 function onController(e)
   local v=e.value==64 and 0.5 or e.value/127
   if e.controller>=20 and e.controller<=23 then targets[e.controller-19]:setParameter('Gain',v) end
   {gate}
 end
]]></script></ScriptProcessor></EventProcessors>
<Layers><Layer Name="Primary" Gain="{l}"><Keygroups><Keygroup Name="Primary" LowKey="60" HighKey="60" Gain="{k}">
<ControlSignalSources><DAHDSR Name="Amp" AttackTime="0" DecayTime="0" SustainLevel="1" ReleaseTime="0.01" VelocityAmount="0"/></ControlSignalSources>
<Connections><SignalConnection Source="$Keygroup/Amp" Destination="Gain" Ratio="1"/></Connections>
<Inserts><Gain Volume="1"/></Inserts><Oscillators><SamplePlayer Name="Primary" SamplePath="primary.wav" BaseNote="60" Gain="{o}">
<Connections><SignalConnection Source="$Program/Gate" Destination="Gain" Ratio="1"/></Connections>
<PlaybackOptions Start="0" Stop="400"><Loop Start="0" End="400" Type="0"/></PlaybackOptions>
</SamplePlayer></Oscillators></Keygroup></Keygroups></Layer>
<Layer Name="Sibling" Gain="{sg}"><Keygroups><Keygroup LowKey="72" HighKey="72" Gain="{sg}">
<ControlSignalSources><DAHDSR Name="Amp" AttackTime="0" DecayTime="0" SustainLevel="1" ReleaseTime="0.01" VelocityAmount="0"/></ControlSignalSources>
<Connections><SignalConnection Source="$Keygroup/Amp" Destination="Gain" Ratio="1"/></Connections>
<Inserts><Gain Volume="1"/></Inserts><Oscillators><SamplePlayer SamplePath="sibling.wav" BaseNote="72" Gain="{sg}">
<PlaybackOptions Start="0" Stop="400"><Loop Start="0" End="400" Type="0"/></PlaybackOptions>
</SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers></Program></UVI4>"#,
        p=initial[0],l=initial[1],k=initial[2],o=initial[3],g=initial[4],sg=if unit_reference {1.} else {0.5})
}

#[derive(Default)]
struct Energy { frames: usize, sum: [f64;2], squares: [f64;2], peak: [f64;2], nonfinite: usize }
impl Energy {
    fn add(&mut self, frames: &[Frame]) {
        for f in frames {self.frames+=1; for c in 0..2 {let x=f[c] as f64; if !x.is_finite(){self.nonfinite+=1;continue;} self.sum[c]+=x;self.squares[c]+=x*x;self.peak[c]=self.peak[c].max(x.abs());}}
    }
    fn rms(&self)->[f64;2] {self.squares.map(|s|(s/self.frames.max(1) as f64).sqrt())}
    fn value(&self)->Value {
        let n=self.frames.max(1) as f64;let dc=self.sum.map(|s|s/n);
        let ac=std::array::from_fn::<_,2,_>(|c|(self.squares[c]/n-dc[c]*dc[c]).max(0.).sqrt());
        json!({"frames":self.frames,"nonfinite":self.nonfinite,"peak":self.peak,"rms":self.rms(),"dc":dc,"ac_rms":ac,"rms_dbfs":self.rms().map(|v|if v>0.{Some(20.*v.log10())}else{None})})
    }
}
#[derive(Default)]
struct TraceEnergy { frames: usize, source_squares: [f64;2], out_squares: [f64;2], first: Option<TraceRecord>, last: Option<TraceRecord>, wraps: usize }
impl TraceEnergy {
    fn add(&mut self,r:TraceRecord) {
        self.frames+=r.frames;for c in 0..2 {self.source_squares[c]+=r.identity.source_metrics.rms[c].powi(2)*r.frames as f64;self.out_squares[c]+=r.output.rms[c].powi(2)*r.frames as f64;}
        if self.last.is_some_and(|old|r.identity.source_frame<old.identity.source_frame){self.wraps+=1;}
        if self.first.is_none(){self.first=Some(r);}self.last=Some(r);
    }
    fn value(&self)->Value {let n=self.frames.max(1)as f64;json!({"frames":self.frames,"source_rms":self.source_squares.map(|v|(v/n).sqrt()),"output_rms":self.out_squares.map(|v|(v/n).sqrt()),"observed_cursor_decreases":self.wraps,"first":self.first,"last":self.last})}
}
fn periodicity(frames:&[Frame])->Value {
    if frames.is_empty(){return Value::Null;}
    let n=frames.len();let mean=std::array::from_fn::<_,2,_>(|c|frames.iter().map(|f|f[c]as f64).sum::<f64>()/n as f64);
    let energy=std::array::from_fn::<_,2,_>(|c|frames.iter().map(|f|(f[c]as f64-mean[c]).powi(2)).sum::<f64>());
    let c=usize::from(energy[1]>energy[0]);if energy[c]==0.{return json!({"channel":c,"peaks":[],"silent":true});}
    let data:Vec<_>=frames.iter().map(|f|f[c]as f64-mean[c]).collect();
    let lo=(RATE/3000)as usize;let hi=((RATE/60)as usize).min(n/4);
    let corr:Vec<_>=(lo..=hi).map(|lag|{let(mut ab,mut aa,mut bb)=(0.,0.,0.);for i in 0..n-lag{let(a,b)=(data[i],data[i+lag]);ab+=a*b;aa+=a*a;bb+=b*b;}if aa*bb>0.{ab/(aa*bb).sqrt()}else{0.}}).collect();
    let mut peaks:Vec<_>=(1..corr.len().saturating_sub(1)).filter(|&i|corr[i]>corr[i-1]&&corr[i]>=corr[i+1]).map(|i|(i+lo,corr[i])).collect();
    peaks.sort_by(|a,b|b.1.total_cmp(&a.1));peaks.truncate(8);peaks.sort_by_key(|p|p.0);
    json!({"channel":c,"lag_search":[lo,hi],"peaks":peaks.iter().map(|(lag,score)|json!({"lag":lag,"hz":RATE as f64/ *lag as f64,"score":score})).collect::<Vec<_>>()})
}

fn phase(rt:&mut Runtime,driver:&mut Driver<ObservedHost>,reader:&TraceReader)->Value {
    let mut full=Energy::default();let mut late=Energy::default();let mut waveform=Vec::with_capacity(WINDOW);
    let mut stages=BTreeMap::<(u32,usize,bool),TraceEnergy>::new();
    let mut done=0;
    while done<PHASE {
        let boundary=if done<PHASE-WINDOW {PHASE-WINDOW}else{PHASE};
        let due=driver.wake(rt).unwrap();let count=64.min(boundary-done).min(due.unwrap_or(64).max(1));
        let mut out=[[0.;2];64];rt.render(&mut out[..count]).unwrap();rt.flush_ended(|_|true);
        full.add(&out[..count]);if done>=PHASE-WINDOW {late.add(&out[..count]);waveform.extend_from_slice(&out[..count]);}
        for row in reader.drain() {
            if done>=PHASE-WINDOW && (row.contribution || reader.graph.nodes[row.node].kind=="amplifier" || reader.graph.nodes[row.node].kind=="sample_source") {
                stages.entry((row.identity.zone,row.node,row.contribution)).or_default().add(row);
            }
        }
        // Onset rows are numeric and drained to keep independent bounded queues healthy.
        let _=reader.onset_reader().drain().unwrap();
        done+=count;
    }
    json!({"full":full.value(),"late":late.value(),"periodicity":periodicity(&waveform),"voices":rt.voice_count(),"notes":rt.note_count(),"trace_dropped":reader.dropped(),"onset_trace_dropped":reader.onset_reader().dropped(),"trace":stages.iter().map(|((zone,node,contribution),m)|json!({"zone":zone,"node":node,"kind":reader.graph.nodes[*node].kind,"contribution":contribution,"metrics":m.value()})).collect::<Vec<_>>()})
}
fn write(driver:&mut Driver<ObservedHost>,rt:&Runtime,scope:usize,value:f64) {
    let midi=if value==0.5{64}else{(value*127.).round()as u8};
    driver.input(rt,HostInput::Controller{cc:20+scope as u8,value:midi,channel:0});
}
fn trigger(rt:&mut Runtime,driver:&mut Driver<ObservedHost>,key:u8) {
    let note=rt.note_on(Input{protocol:Protocol::Native,port:0,group:0,channel:0,key,external_id:None},key,96./127.).unwrap();
    driver.note_on(rt,note,key,96./127.).unwrap();
}
fn run_case(dir:&Path,name:&str,initial:[f64;5],live_constant:bool,unit_reference:bool,tapped:bool,edits:&[(usize,f64)])->Value {
    let dir=dir.join(name);std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("primary.wav"),wav(480.)).unwrap();std::fs::write(dir.join("sibling.wav"),wav(720.)).unwrap();
    let path=dir.join("Public.uvip");std::fs::write(&path,xml(initial,live_constant,unit_reference)).unwrap();
    let mut translated=sampler_uvi::translate_path(&path).unwrap();
    // The same public IR transformation performed by V2 nest: each group's
    // output gets a bus, tap_group moves its authored gain onto that bus.
    // This exercises the real lower/runtime fader path. It is not the async
    // V2 loader, initialized folding, a native host, or Clarinet evidence.
    if tapped {
        for index in 0..translated.instrument.groups.len() {
            let ir=&mut translated.instrument;
            let output=ir.groups[index].output;
            let bus=sampler_ir::BusRef(ir.buses.len());
            ir.buses.push(sampler_ir::Bus { name:format!("Public tap {index}"),
                chain:None,sends:Vec::new(),output,gain:sampler_ir::Gain::UNITY });
            ir.groups[index].output=sampler_ir::Output::Bus(bus);
            ir.tap_group(index,bus);
        }
    }
    let promoted=translated.instrument.uvi_script_sources.as_ref().unwrap().xml.clone();
    let host=ScriptHost::new(&promoted,(),Config::default()).unwrap();
    assert!(host.fault_counts().init.is_empty());
    let metadata=json!({"groups":translated.instrument.groups.iter().map(|g|json!({"gain":g.gain.linear(),"muted":g.muted})).collect::<Vec<_>>(),"zones":translated.instrument.zones.iter().map(|z|json!({"group":z.group.map(|g|g.0),"gain":z.gain.linear(),"low":z.keys.low,"high":z.keys.high,"routes":z.routes.len()})).collect::<Vec<_>>()});
    let groups=translated.groups.clone();let loaded=sampler_uvi::assemble_translated(translated,RATE).unwrap();
    let (plan,reader)=loaded.plan.with_signal_trace_reader(8192).unwrap();let limits=Limits::for_plan(&plan,16,16);
    let mut rt=Runtime::new(plan,limits).unwrap();let events=Rc::new(RefCell::new(Vec::new()));
    let mut driver=Driver::new(ObservedHost{host,events:events.clone()},groups,RATE);
    driver.wake(&mut rt).unwrap();trigger(&mut rt,&mut driver,60);
    let mut expected=initial;let mut rows=Vec::new();
    rows.push(json!({"edit":null,"expected_linear_product":expected.iter().product::<f64>(),"window":phase(&mut rt,&mut driver,&reader)}));
    for &(scope,value)in edits {
        write(&mut driver,&rt,scope,value);expected[scope]=value;
        rows.push(json!({"edit":{"scope":SCOPES[scope],"value":value},"expected_linear_product":expected.iter().product::<f64>(),"window":phase(&mut rt,&mut driver,&reader)}));
    }
    // After releasing primary, use a fresh held sibling to detect leaked local
    // scope changes. Program writes correctly affect both layers.
    driver.note_off(&mut rt,60).unwrap();let primary_release=phase(&mut rt,&mut driver,&reader);
    trigger(&mut rt,&mut driver,72);let sibling=phase(&mut rt,&mut driver,&reader);
    driver.note_off(&mut rt,72).unwrap();let release=phase(&mut rt,&mut driver,&reader);
    json!({"case":name,"fixture_scope":if tapped {"nested_group_taps"} else {"flat_adapter"},"initial":initial,"constant_live":live_constant,"metadata":metadata,"phases":rows,"sibling_expected_linear_product":expected[0]*if unit_reference {1.} else {0.125},"unit_gain_reference":unit_reference,"sibling":sibling,"primary_release":primary_release,"sibling_release":release,"typed_commands":*events.borrow(),"audit_truncated":events.borrow().len()>=256,"driver_unmodeled":driver.unmodeled(),"runtime_stats": {"underruns":rt.stats().stream_underruns,"nonfinite":rt.stats().nonfinite_frames,"voice_drops":rt.stats().voice_drops,"refused_starts":rt.stats().refused_starts,"voice_pool_refusals":rt.stats().voice_pool_refusals,"stream_capacity_refusals":rt.stats().stream_capacity_refusals,"stream_disconnected_refusals":rt.stats().stream_disconnected_refusals},"scope":"synthetic public production APIs; not Clarinet cause evidence"})
}
fn main() {
    let dir=std::env::args_os().nth(1).expect("explicit new private DEV artifact directory required");let dir=Path::new(&dir);
    assert!(dir.is_absolute()&&dir.starts_with("/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts"));std::fs::create_dir(dir).unwrap();
    let tapped=match std::env::args_os().nth(2) {
        None=>false,
        Some(flag) if flag=="--tapped"=>true,
        Some(_)=>panic!("only optional --tapped is supported"),
    };
    let mut rows=Vec::new();
    rows.push(run_case(dir,"unit_gain_reference",[1.;5],false,true,tapped,&[]));
    for focus in 0..4 {
        let mut initial=[0.5;5];initial[4]=1.;initial[focus]=0.;
        let edits=[(focus,1.),(focus,1.),(focus,0.5),(focus,0.),(focus,0.),(focus,1.)];
        rows.push(run_case(dir,SCOPES[focus],initial,false,false,tapped,&edits));
    }
    rows.push(run_case(dir,"nonzero_composition",[0.5,0.5,0.5,0.5,1.],false,false,tapped,&[(0,1.),(1,1.),(2,1.),(3,1.),(3,0.),(1,0.),(3,1.),(1,0.5),(3,0.5)]));
    rows.push(run_case(dir,"constant_live",[0.5,0.5,0.5,0.5,0.],true,false,tapped,&[(4,1.),(4,0.5),(4,0.),(4,1.)]));
    // A static authored constant gate must remain closed while another scope
    // opens: changing unrelated Gain is not permission to erase that source.
    rows.push(run_case(dir,"constant_static_barrier",[0.5,0.5,0.5,0.5,0.],false,false,tapped,&[(1,1.),(2,1.),(3,1.)]));
    println!("{}",json!({"schema":2,"fixture_scope":if tapped {"nested_group_taps"} else {"flat_adapter"},"case_execution_is_observational":true,"requires_external_contract_validator":true,"reference_scope":"Fresh owner with primary and sibling authored gains all unity; shared velocity/pan/envelope pipeline observed independently","cases":rows,"library_access_executed":false,"production_parameters_invented":false,"raw_protected_content_written":false}));
}
