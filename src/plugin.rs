use crate::{engine::{Bank,Engine,MAX_BLOCK,Rack,PartControls,RACK_SLOTS,load_scripts}, fx::FxProcessor, import::{self, Instrument}, ksp::{Persisted,Runtime}};
use crossbeam_queue::ArrayQueue;
#[path="artwork.rs"] mod artwork;
use moose::mui::mui::{scene::{Theme,Color,TypeScale,Fill,Fit},geometry::Path as DrawPath};
use moose::{prelude::*, mui::{MuiEditor, mui::prelude::*}};
use std::{path::{Path,PathBuf}, sync::{Mutex,RwLock,atomic::{AtomicU64,AtomicBool,Ordering}}, time::{Instant,Duration}};

#[derive(State, Clone, PartialEq)]
pub struct Part { pub path:String, pub group:u32, pub port:u8, pub output:u8, pub channel:i16, pub gain:f32, pub pan:f32, pub mute:bool, pub solo:bool, pub program:u32,
    /// Script persistent variables as JSON (`Vec<Persisted>`); empty uses the instrument's saved values.
    pub script_state:String }
impl Default for Part {fn default()->Self {Self {path:String::new(),program:0,group:u32::MAX,port:0,output:0,channel:-1,gain:0.,pan:0.,mute:false,solo:false,script_state:String::new()}}}
#[derive(State, Default, Clone, PartialEq)]
pub struct Selection { pub root:String, pub parts:Vec<Part>, pub order:Vec<u32>, pub midi_thru:bool, pub multi:String }

#[derive(Params)]
pub struct SamplerParams {
    #[param(name="Volume",range="linear(-60, 6)",default=-12.0,unit="dB",smooth="exp(5)")]
    pub volume:FloatParam,
    #[param(name="Attack",range="log(0.0001, 5)",default=0.002,unit="s")]
    pub attack:FloatParam,
    #[param(name="Release",range="log(0.001, 10)",default=0.15,unit="s")]
    pub release:FloatParam,
    #[param(name="Tone",range="log(20, 20000)",default=20000.0,unit="Hz")]
    pub cutoff:FloatParam,
    // Raw MIDI stays port/channel-specific; VST3 supplies its own controller proxies.
    #[persist="selection"]
    pub selection:RwLock<Selection>,
    #[skip]
    pub shared:Shared,
    #[meter]
    pub level:MeterSlot,
}
use SamplerParamsParamId as P;

pub struct Shared {
    ready:ArrayQueue<(usize,u64,Handoff)>, discard:ArrayQueue<Retired>,
    /// Persistence snapshots: the loader lends one per scripted slot, the audio thread fills it in place and returns it.
    snapshot_requests:ArrayQueue<(usize,Box<Vec<Persisted>>)>, snapshots:ArrayQueue<(usize,u64,Box<Vec<Persisted>>)>,
    /// Host sample rate (`f64` bits) that effect processors are built for.
    rate:AtomicU64,
    key_owners:[AtomicU64;128],keyboard:ArrayQueue<(usize,u8,bool)>, controls:ArrayQueue<[PartControls;RACK_SLOTS]>, generation:[AtomicU64;RACK_SLOTS],
    audition:AtomicBool, audition_note:AtomicU64, selected:AtomicU64, focus_request:AtomicU64, panic:AtomicBool, midi_thru:AtomicBool,
    multi_request:Mutex<Option<String>>,view:Mutex<View>,
}
#[derive(Default,Clone)]
struct PartView {program:u32,interface:Option<Arc<crate::ksp::Interface>>,interface_status:String,wallpaper:Option<Arc<moose::mui::mui::scene::Image>>,wallpaper_status:String,attempted:Option<(String,u32)>,instrument:Option<Arc<Instrument>>,status:String,active:String,bytes:usize,
    /// Rate of the effects handed to the audio thread; 0 when none were.
    fx_rate:f64,
    /// `Part::script_state` the audio thread's runtime matches (loaded from or last saved).
    script_state:String,
    /// Epoch of the runtime last handed to the audio thread.
    script_epoch:u64,
    /// Snapshot buffer for this slot's runtime while the loader holds it.
    snapshot:Option<Box<Vec<Persisted>>>}
#[derive(Clone)]
struct View {
    /// Last script epoch handed out; tags runtimes so stale persistence snapshots are ignored.
    script_epoch:u64,
    multi_status:String,artwork:std::collections::HashMap<String,Arc<moose::mui::mui::scene::Image>>,root:String,files:Arc<Vec<PathBuf>>,parts:[PartView;RACK_SLOTS],status:String}
impl Default for Shared {
    fn default()->Self {Self {ready:ArrayQueue::new(64),discard:ArrayQueue::new(64),snapshot_requests:ArrayQueue::new(2*RACK_SLOTS),snapshots:ArrayQueue::new(2*RACK_SLOTS),rate:AtomicU64::new(48000f64.to_bits()),key_owners:std::array::from_fn(|_|AtomicU64::new(128)),keyboard:ArrayQueue::new(256),controls:ArrayQueue::new(1),generation:std::array::from_fn(|_|AtomicU64::new(0)),audition:AtomicBool::new(false),audition_note:AtomicU64::new(128),selected:AtomicU64::new(0),focus_request:AtomicU64::new(128),panic:AtomicBool::new(false),midi_thru:AtomicBool::new(false),multi_request:Mutex::new(None),view:Mutex::new(View{script_epoch:0,multi_status:String::new(),artwork:Default::default(),root:String::new(),files:Arc::default(),parts:std::array::from_fn(|_|PartView::default()),status:"Choose a library and select a preset".into()})}}
}
fn rack_controls(selection:&Selection)->[PartControls;RACK_SLOTS] {std::array::from_fn(|n|selection.parts.get(n).map(|p|PartControls {port:p.port.min(3),output:p.output.min(7),channel:p.channel.clamp(-1,15),gain:if p.gain.is_finite(){db_to_linear(p.gain.clamp(-60.,6.))}else{1.},pan:if p.pan.is_finite(){p.pan.clamp(-1.,1.)}else{0.},mute:p.mute,solo:p.solo}).unwrap_or_default())}
/// Loader-built state for one rack slot, installed by the audio thread.
enum Handoff {
    /// A new instrument, or an empty slot, with its effects and initialized scripts.
    Part{bank:Option<Box<Bank>>,fx:FxProcessor,script:Option<Box<Runtime>>,epoch:u64},
    /// Effects rebuilt for a new host sample rate; the bank stays.
    Fx(FxProcessor),
    /// Scripts rebuilt from restored host state; the bank stays.
    Script{script:Option<Box<Runtime>>,epoch:u64},
}
/// What the audio thread replaced, freed on the loader thread.
#[expect(dead_code,reason="held only to be dropped off the audio thread")]
#[derive(Default)]
struct Retired {bank:Option<Box<Bank>>,fx:Option<FxProcessor>,script:Option<Box<Runtime>>}
/// Persistent script values to restore: the host's saved state, else the instrument's.
fn persisted(saved:&str,i:&Instrument)->Vec<Persisted> {serde_json::from_str(saved).unwrap_or_else(|_|i.script_state.clone())}
/// Initialize `i`'s scripts off the audio thread, with a snapshot buffer shaped for them.
fn scripts(i:&Instrument,saved:&str,rate:f64)->(Option<Box<Runtime>>,Option<Box<Vec<Persisted>>>,Vec<String>) {
    let (script,errors)=load_scripts(i,persisted(saved,i),rate);
    // Nothing persistent: no snapshots to trade with the audio thread.
    let snapshot=script.as_ref().map(|rt|rt.persistence()).filter(|s|s.iter().any(|p|!p.is_empty())).map(Box::new);
    (script,snapshot,errors)
}
/// Epoch for a runtime about to be handed off from `slot`; forgets the old runtime's snapshot.
fn next_epoch(view:&mut View,slot:usize,snapshot:Option<Box<Vec<Persisted>>>)->u64 {view.script_epoch+=1;let v=&mut view.parts[slot];v.script_epoch=view.script_epoch;v.snapshot=snapshot;view.script_epoch}
impl Shared {fn rate(&self)->f64 {f64::from_bits(self.rate.load(Ordering::Acquire))}}
pub struct Load;
impl BackgroundTask for Load {
    type Params=SamplerParams;
    const SERIALIZED:bool=true;
    fn run(self,params:&SamplerParams) {
        while params.shared.discard.pop().is_some() {}
        let requested={params.shared.multi_request.lock().unwrap().take()};
        if let Some(path)=requested {
            let before=params.selection.read().unwrap().clone();
            let result=import::read_multi(Path::new(&path)).and_then(|m|{anyhow::ensure!(m.parts.len()<=RACK_SLOTS,"Multi exceeds the 16-instrument rack limit");Ok(m)});
            if params.shared.multi_request.lock().unwrap().is_some(){return;}
            match result {Ok(m)=>{let mut current=params.selection.write().unwrap();if *current==before {
                current.parts=m.parts.iter().map(|p|Part{path:path.clone(),program:p.program,..Default::default()}).collect();current.order=(0..m.parts.len() as u32).collect();current.multi=path;
                params.shared.focus_request.store(0,Ordering::Release);params.shared.view.lock().unwrap().multi_status=format!("{} · {} instruments · original multi routing/scripts unavailable",m.name,m.parts.len());
            }else{params.shared.view.lock().unwrap().multi_status="Multi load canceled because the rack changed".into();}},Err(e)=>params.shared.view.lock().unwrap().multi_status=format!("Multi load failed: {e:#}")}
        }
        let selection=params.selection.read().unwrap().clone();params.shared.midi_thru.store(selection.midi_thru,Ordering::Release);
        let root=if selection.root.is_empty() {import::LIBRARY_ROOT}else{&selection.root};
        if params.shared.view.lock().unwrap().root!=root {
            let result=import::presets(Path::new(root));
            let artwork=result.as_ref().ok().map(|f|artwork::scan(Path::new(root),f)).unwrap_or_default();
            let mut view=params.shared.view.lock().unwrap();view.root=root.into();view.artwork=artwork;
            match result {Ok(files)=>{view.files=Arc::new(files);view.status=format!("{} presets",view.files.len());},Err(e)=>{view.files=Arc::default();view.status=format!("Scan failed: {e:#}");}}
        }
        {let current=params.selection.read().unwrap();let _=params.shared.controls.force_push(rack_controls(&current));params.shared.midi_thru.store(current.midi_thru,Ordering::Release);}
        for slot in 0..RACK_SLOTS {
            let part=selection.parts.get(slot).cloned().unwrap_or_default();let target=(part.path.clone(),part.program);
            // The host restored different script values for a loaded part: rebuild only its scripts.
            let restore={let view=params.shared.view.lock().unwrap();let v=&view.parts[slot];
                v.instrument.clone().filter(|i|v.attempted.as_ref()==Some(&target) && v.fx_rate!=0. && !i.scripts.is_empty() && part.script_state!=v.script_state)};
            if let Some(instrument)=restore {
                let (script,snapshot,_)=scripts(&instrument,&part.script_state,params.shared.rate());
                let mut view=params.shared.view.lock().unwrap();let epoch=next_epoch(&mut view,slot,snapshot);view.parts[slot].script_state=part.script_state.clone();
                let _=params.shared.ready.force_push((slot,params.shared.generation[slot].load(Ordering::Acquire),Handoff::Script{script,epoch}));
                continue;
            }
            let cached={let mut view=params.shared.view.lock().unwrap();let v=&mut view.parts[slot];
                if v.attempted.as_ref()==Some(&target) {continue;}
                v.attempted=Some(target.clone());v.status="Loading samples…".into();v.script_epoch=0;v.snapshot=None;
                v.instrument.as_ref().filter(|i|i.path==Path::new(&part.path) && v.program==part.program).cloned()};
            let generation=params.shared.generation[slot].fetch_add(1,Ordering::AcqRel)+1;
            if part.path.is_empty() {params.shared.view.lock().unwrap().parts[slot]=PartView{attempted:Some(target),..Default::default()};let _=params.shared.ready.force_push((slot,generation,Handoff::Part{bank:None,fx:FxProcessor::default(),script:None,epoch:0}));continue;}
            let result=(||->anyhow::Result<_>{
                let instrument=if let Some(i)=cached {i}else{Arc::new(import::read_program(Path::new(&part.path),part.program)?)};
                let (script,snapshot,_)=scripts(&instrument,&part.script_state,params.shared.rate());
                {
                    let needs_art={let view=params.shared.view.lock().unwrap();let v=&view.parts[slot];v.program!=part.program || v.instrument.as_ref().is_none_or(|i|i.path!=instrument.path)};
                    let parsed=needs_art.then(||script_interface(script.as_deref()));
                    let art=parsed.as_ref().map(|(interface,_)|artwork::performance(&instrument,interface.as_ref().map(|u|u.wallpaper.as_str())));
                    let mut view=params.shared.view.lock().unwrap();let v=&mut view.parts[slot];v.instrument=Some(instrument.clone());v.program=part.program;
                    if let Some((interface,status))=parsed {v.interface=interface;v.interface_status=status;}
                    if let Some(art)=art {match art {Ok(image)=>{v.wallpaper=image;v.wallpaper_status.clear();},Err(e)=>{v.wallpaper=None;v.wallpaper_status=e;}}}
                }
                if instrument.zones.is_empty(){return Ok((instrument,None,script,snapshot));}
                // Every group plays; the stored group only selects what the mapping inspector shows.
                if part.group==u32::MAX {
                    let group=instrument.first_playable_group().unwrap_or(0);
                    let mut current=params.selection.write().unwrap();
                    if let Some(c)=current.parts.get_mut(slot).filter(|c|c.path==part.path && c.program==part.program && c.group==u32::MAX) {c.group=group as u32;}
                }
                let bank=Box::new(Bank::load(&instrument)?);
                let resident:usize=params.shared.view.lock().unwrap().parts.iter().enumerate().filter(|(n,_)|*n!=slot).map(|(_,p)|p.bytes).sum();
                anyhow::ensure!(resident+bank.bytes<=2*crate::engine::MEMORY_LIMIT,"Rack exceeds 2 GiB RAM limit");
                Ok((instrument,Some(bank),script,snapshot))
            })();
            let current=params.selection.read().unwrap();if current.parts.get(slot).map(|p|(&p.path,p.program))!=Some((&target.0,target.1)) {continue;}drop(current);
            let mut view=params.shared.view.lock().unwrap();
            match result {Ok((instrument,bank,script,snapshot))=>{let epoch=if script.is_some(){next_epoch(&mut view,slot,snapshot)}else{0};let v=&mut view.parts[slot];v.script_state=part.script_state.clone();v.active=instrument.name.clone();v.bytes=bank.as_ref().map(|b|b.bytes).unwrap_or(0);v.status=bank.as_deref().map(bank_status).unwrap_or_else(||"Controller instrument · KSP playback unavailable".into());
                let rate=params.shared.rate();v.fx_rate=rate;let fx=instrument.fx.processor(rate as f32,MAX_BLOCK);let _=params.shared.ready.force_push((slot,generation,Handoff::Part{bank,fx,script,epoch}));},
                Err(e)=>{let v=&mut view.parts[slot];v.status=format!("Load failed: {e:#}");v.fx_rate=0.;}}
        }
        // The host changed sample rate since these effects were built: rebuild them here, off the audio thread.
        let rate=params.shared.rate();
        for slot in 0..RACK_SLOTS {
            let stale={let view=params.shared.view.lock().unwrap();let v=&view.parts[slot];v.instrument.clone().filter(|_|v.fx_rate!=0. && v.fx_rate!=rate)};
            let Some(instrument)=stale else {continue};
            let fx=instrument.fx.processor(rate as f32,MAX_BLOCK);
            params.shared.view.lock().unwrap().parts[slot].fx_rate=rate;
            let _=params.shared.ready.force_push((slot,params.shared.generation[slot].load(Ordering::Acquire),Handoff::Fx(fx)));
        }
        // Save script values the audio thread reported, then lend the buffers out again.
        while let Some((slot,epoch,snapshot))=params.shared.snapshots.pop() {
            let mut view=params.shared.view.lock().unwrap();let v=&mut view.parts[slot];
            if epoch==0 || epoch!=v.script_epoch {continue;}
            let json=serde_json::to_string(&*snapshot).unwrap_or_default();v.snapshot=Some(snapshot);
            if json==v.script_state {continue;}
            let path=v.instrument.as_ref().map(|i|i.path.clone());let program=v.program;v.script_state=json.clone();drop(view);
            let mut current=params.selection.write().unwrap();
            if let Some(p)=current.parts.get_mut(slot).filter(|p|path.as_deref()==Some(Path::new(&p.path)) && p.program==program) {p.script_state=json;}
        }
        for slot in 0..RACK_SLOTS {
            let mut view=params.shared.view.lock().unwrap();
            if let Some(snapshot)=view.parts[slot].snapshot.take() && let Err((_,snapshot))=params.shared.snapshot_requests.push((slot,snapshot)) {view.parts[slot].snapshot=Some(snapshot);}
        }
    }
}
#[derive(Default)]
pub struct Dsp {rack:Rack,until_poll:usize,audition_left:[usize;RACK_SLOTS],
    /// Epoch of each slot's installed runtime, returned with its persistence snapshots.
    script_epoch:[u64;RACK_SLOTS]}
pub struct Sampler;

/// Channel that makes the on-screen keyboard reach the part's first zone.
fn preview_channel(e: &Engine) -> u8 {
    e.bank().and_then(|b| b.zones().first().map(|z| b.groups()[z.group].channel.max(0) as u8)).unwrap_or(0)
}

/// Mid-range velocity of the first zone on `note`.
fn preview_velocity(e: &Engine, note: u8) -> u8 {
    let zone = e.bank().and_then(|b| b.zones().iter().find(|z| (z.low_key..=z.high_key).contains(&note)));
    zone.map_or(100, |z| ((u16::from(z.low_velocity) + u16::from(z.high_velocity)) / 2).max(1) as u8)
}

fn bank_status(bank: &Bank) -> String {
    let mut status = format!("{} samples · {} streamed · {:.0} MB", bank.sample_count(), bank.streamed_samples(), bank.bytes as f64 / 1048576.0);
    if bank.skipped_zones > 0 {
        status += &format!(" · {} zones skipped (missing or damaged)", bank.skipped_zones);
    }
    status
}
impl PluginLogic for Sampler {
    type Params=SamplerParams;type DspState=Dsp;
    fn bus_layouts()->Vec<BusLayout> {vec![BusLayout::new().with_output("Main",ChannelConfig::Stereo).with_output("Out 2",ChannelConfig::Stereo).with_output("Out 3",ChannelConfig::Stereo).with_output("Out 4",ChannelConfig::Stereo).with_output("Out 5",ChannelConfig::Stereo).with_output("Out 6",ChannelConfig::Stereo).with_output("Out 7",ChannelConfig::Stereo).with_output("Out 8",ChannelConfig::Stereo)]}
    fn reset(s:&mut Dsp,p:&SamplerParams,c:&AudioConfig){s.rack.reset(c.sample_rate);p.shared.rate.store(c.sample_rate.to_bits(),Ordering::Release);s.until_poll=0;s.audition_left.fill(0);}
    fn process(s: &mut Dsp, p: &SamplerParams, b: &mut AudioBuffer, events: &EventList, cx: &mut ProcessContext) -> ProcessStatus {
        let rate = s.rack.parts[0].rate();
        let frames = b.num_samples();
        if s.until_poll <= frames {
            if let Some(tasks) = cx.tasks::<Load>() {
                tasks.spawn_coalescing(Load);
            }
            s.until_poll = (rate * 0.1) as usize;
        } else {
            s.until_poll -= frames;
        }
        if let Some(controls) = p.shared.controls.pop() {
            s.rack.set_controls(controls);
        }
        // Retired banks, effects and scripts go back to the loader thread to be
        // freed; stop while it cannot take more.
        while !p.shared.discard.is_full() {
            let Some((slot, generation, handoff)) = p.shared.ready.pop() else { break };
            let engine = &mut s.rack.parts[slot];
            let current = generation == p.shared.generation[slot].load(Ordering::Acquire);
            let retired = match handoff {
                Handoff::Part { bank, fx, script, epoch } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    Retired { script: engine.set_script(script), bank: engine.set_bank(bank), fx: Some(engine.set_fx(fx)) }
                }
                Handoff::Fx(fx) if current => Retired { fx: Some(engine.set_fx(fx)), ..Retired::default() },
                Handoff::Script { script, epoch } if current => {
                    engine.reset(rate);
                    s.script_epoch[slot] = epoch;
                    Retired { script: engine.set_script(script), ..Retired::default() }
                }
                Handoff::Part { bank, fx, script, .. } => Retired { bank, fx: Some(fx), script },
                Handoff::Fx(fx) => Retired { fx: Some(fx), ..Retired::default() },
                Handoff::Script { script, .. } => Retired { script, ..Retired::default() },
            };
            let _ = p.shared.discard.push(retired);
        }
        // Refresh lent persistence snapshots in place; the loader saves them.
        while !p.shared.snapshots.is_full() {
            let Some((slot, mut snapshot)) = p.shared.snapshot_requests.pop() else { break };
            if let Some(rt) = s.rack.parts[slot].script() {
                rt.refresh_persistence(&mut snapshot);
            }
            let _ = p.shared.snapshots.push((slot, s.script_epoch[slot], snapshot));
        }
        for e in &mut s.rack.parts {
            e.attack = p.attack.value();
            e.release = p.release.value();
            e.cutoff = p.cutoff.value();
        }
        if p.shared.panic.swap(false, Ordering::AcqRel) {
            while p.shared.keyboard.pop().is_some() {}
            for owner in &p.shared.key_owners {
                owner.store(128, Ordering::Release);
            }
            for channel in 0..16 {
                s.rack.cc(channel, 120, 0);
                s.rack.cc(channel, 121, 0);
            }
            s.audition_left.fill(0);
        }
        while let Some((slot, note, down)) = p.shared.keyboard.pop() {
            let e = &mut s.rack.parts[slot.min(RACK_SLOTS - 1)];
            let channel = preview_channel(e);
            if down {
                let velocity = preview_velocity(e, note);
                e.note_on(channel, note, velocity);
            } else {
                e.note_off(channel, note);
            }
        }
        if p.shared.audition.swap(false, Ordering::AcqRel) {
            let slot = (p.shared.selected.load(Ordering::Relaxed) as usize).min(RACK_SLOTS - 1);
            let e = &mut s.rack.parts[slot];
            for channel in 0..16 {
                e.cc(channel, 120, 0);
            }
            let requested = p.shared.audition_note.swap(128, Ordering::Relaxed);
            let note = if requested < 128 { requested as u8 } else { e.bank().and_then(|b| b.zones().first()).map_or(60, |z| z.root) };
            let (channel, velocity) = (preview_channel(e), preview_velocity(e, note));
            e.note_on(channel, note, velocity);
            s.audition_left[slot] = (rate * 1.5) as usize;
        }

        let channels = b.num_output_channels();
        let thru = p.shared.midi_thru.load(Ordering::Relaxed);
        let mut peak = 0f32;
        let mut gains = [0f32; MAX_BLOCK];
        let (mut at, mut next) = (0, 0);
        loop {
            // Apply events due now; once the buffer is rendered, apply any stragglers.
            while let Some(e) = events.get(next).filter(|e| at >= frames || e.sample_offset as usize <= at) {
                if thru && matches!(e.body, EventBody::NoteOn { .. } | EventBody::NoteOff { .. } | EventBody::PitchBend { .. } | EventBody::ControlChange { .. }) {
                    let mut out = *e;
                    out.port = 0;
                    cx.output_events.push(out);
                }
                match e.body {
                    EventBody::NoteOn { channel, note, velocity, .. } => s.rack.note_on_port(e.port, channel, note, velocity),
                    EventBody::NoteOff { channel, note, .. } => s.rack.note_off_port(e.port, channel, note),
                    EventBody::PitchBend { channel, value, .. } => s.rack.pitch_bend_port(e.port, channel, value),
                    EventBody::ControlChange { channel, cc, value, .. } => s.rack.cc_port(e.port, channel, cc, value),
                    EventBody::ChannelPressure { channel, pressure, .. } => s.rack.channel_pressure_port(e.port, channel, pressure),
                    EventBody::Aftertouch { channel, note, pressure, .. } => s.rack.poly_pressure_port(e.port, channel, note, pressure),
                    _ => {}
                }
                next += 1;
            }
            if at >= frames {
                break;
            }
            let due = events.get(next).map_or(frames, |e| (e.sample_offset as usize).min(frames));
            let len = (due - at).min(MAX_BLOCK);
            for (e, left) in s.rack.parts.iter_mut().zip(&mut s.audition_left) {
                if *left > 0 {
                    *left = left.saturating_sub(len);
                    if *left == 0 {
                        for channel in 0..16 {
                            e.cc(channel, 123, 0);
                        }
                    }
                }
            }
            for gain in &mut gains[..len] {
                *gain = db_to_linear(p.volume.read());
            }
            let buses = s.rack.render(len);
            for channel in 0..channels {
                b.output(channel)[at..at + len].fill(0.0);
            }
            for (bus, x) in buses.iter().enumerate() {
                let route = cx.bus_routing.output(bus).map(|r| (r.channel_start(), r.channel_count()));
                let (start, count) = route.unwrap_or(if bus == 0 { (0, channels.min(2)) } else { (0, 0) });
                for channel in (0..count.min(2)).filter(|c| start + c < channels) {
                    let out = &mut b.output(start + channel)[at..at + len];
                    for (i, (o, gain)) in out.iter_mut().zip(&gains[..len]).enumerate() {
                        let value = if count == 1 { (x[0][i] + x[1][i]) * 0.5 } else { x[channel][i] };
                        *o = value * gain;
                        peak = peak.max(o.abs());
                    }
                }
            }
            at += len;
        }
        cx.set_meter(P::Level, peak.min(1.0));
        ProcessStatus::Normal
    }
    fn editor(params:Arc<SamplerParams>)->Box<dyn Editor> {editor(params)}
}

fn editor_theme() -> Ui {
    Ui::new(Theme {
        palette: Palette { neutral: Pigment::GREY, primary: Pigment::GREY, ..Palette::NEUTRAL },
        corners: Corners { field:1., box_:0., selector:1., ..Corners::DEFAULT },
        text:13., control:3., type_scale:TypeScale {title:24.,body:13.,caption:11.},
        ..Theme::DEFAULT
    }).font(Font::new(include_bytes!("../assets/NotoSans.ttf").as_slice()).expect("bundled Noto Sans"))
}
fn editor(params:Arc<SamplerParams>)->Box<dyn Editor> {
    let build=editor_ui(&params);let drop_params=params.clone();let cancel_params=params.clone();
    MuiEditor::new(params,editor_theme(),(1180,780),build).on_files(move |ui,at,paths,dropped|native_files(&drop_params,ui,at,paths,dropped)).on_cancel(move |_|release_keyboard(&cancel_params)).changed(||true).fixed_zoom().resizable((900,640)).into_editor()
}
fn action(ui:&mut Ui,id:impl Into<Id>,label:&str,selected:bool)->(bool,El) {
    let b=button(ui,id,label).size(S).variant(if selected {Variant::Soft}else{Variant::Ghost}).role(if selected {Role::Primary}else{Role::Ink});
    (b.changed,b.el.el().radius(1).text_size(12))
}
fn rule()->El {block(Len::Pct(100.),1).fill(Role::Dim.alpha(0.15)).shrink(0)}
fn note_name(note:u8)->String {format!("{}{}",["C","C#","D","D#","E","F","F#","G","G#","A","A#","B"][(note%12) as usize],note as i16/12-1)}
fn release_keyboard(p:&SamplerParams) {
    for (note,owner) in p.shared.key_owners.iter().enumerate() {let slot=owner.swap(128,Ordering::AcqRel);if slot<RACK_SLOTS as u64 && p.shared.keyboard.push((slot as usize,note as u8,false)).is_err(){p.shared.panic.store(true,Ordering::Release);}}
}
fn keybed(ui:&mut Ui,p:&SamplerParams,octave:i16)->El {
    let mut octaves=Vec::new();
    for o in octave..octave+4 {
        let mut make=|note:i16,black:bool| {
            let name=note_name(note as u8);let b=button(ui,format!("key-{note}"),if note%12==0 {&name}else{""});
            let r=ui.get(format!("key-{note}"));let slot=p.shared.selected.load(Ordering::Relaxed) as usize;
            if r.pressed {p.shared.key_owners[note as usize].store(slot as u64,Ordering::Release);if p.shared.keyboard.push((slot,note as u8,true)).is_err(){p.shared.panic.store(true,Ordering::Release);}}
            if r.released {let owner=p.shared.key_owners[note as usize].swap(128,Ordering::AcqRel);if owner<RACK_SLOTS as u64 && p.shared.keyboard.push((owner as usize,note as u8,false)).is_err(){p.shared.panic.store(true,Ordering::Release);}}
            if r.key_activated {p.shared.audition_note.store(note as u64,Ordering::Relaxed);p.shared.audition.store(true,Ordering::Release);}
            b.el.el().fill(Color::oklch(if black{0.16}else{0.94},0.,0.)).radius(0).pad(1).text_size(9).named(format!("Audition {name}")).tip(name).justify(Justify::Center).align(Align::End)
        };
        let whites=row([0,2,4,5,7,9,11].map(|n|make(o*12+n,false).flex(1).min_w(0).h(68))).gap(1);
        let mut blacks=Vec::new();for (gap,n) in [(1.4,1),(0.8,3),(2.8,6),(0.8,8),(0.8,10)] {blacks.push(spacer().flex(gap));blacks.push(make(o*12+n,true).flex(1.2).min_w(0).h(42));}blacks.push(spacer().flex(1.4));
        octaves.push(stack([whites,row(blacks).gap(0).h(42).anchor(Align::Start,Align::Start).w(Len::Pct(100.))]).flex(1).min_w(0).h(68));
    }
    row(octaves).gap(1).h(68)
}
fn mapping(instrument:Option<&Instrument>,group:u32)->El {
    let zones:Vec<_>=instrument.map(|i|i.zones.iter().filter(|z|z.group==group as usize).map(|z|(z.low_key,z.high_key,z.low_velocity,z.high_velocity,z.available)).collect()).unwrap_or_default();
    canvas(move |s| {
        let rectangle=|x:f64,y:f64,w:f64,h:f64|DrawPath::polyline([(x,y),(x+w,y),(x+w,y+h),(x,y+h)].map(|(x,y)|Point::new(x,y)),true);
        let mut draw=Vec::new();
        for n in (0..128).step_by(12) {draw.push(Draw::fill(rectangle(n as f64/128.*s.width,0.,1.,s.height),Role::Dim.alpha(0.13)));}
        for v in [32.,64.,96.] {draw.push(Draw::fill(rectangle(0.,s.height*(1.-v/128.),s.width,1.),Role::Dim.alpha(0.1)));}
        for &(lo,hi,lv,hv,available) in &zones {
            draw.push(Draw::fill(rectangle(lo as f64/128.*s.width,(127-hv) as f64/128.*s.height,(hi-lo+1) as f64/128.*s.width,((hv-lv+1) as f64/128.*s.height).max(1.)),if available {Role::Primary.alpha(0.32)}else{Role::Danger.alpha(0.2)}));
        }
        draw
    }).h(112).fill(Role::Field).radius(4).clip().named("Selected group key and velocity mapping")
}
fn queue_multi(p:&SamplerParams,path:String) {
    p.shared.view.lock().unwrap().multi_status="Loading multi…".into();
    *p.shared.multi_request.lock().unwrap()=Some(path);
}
fn native_files(p:&SamplerParams,ui:&Ui,at:Point,paths:&[PathBuf],dropped:bool)->bool {
    if paths.len()==1 && import::is_multi(&paths[0]) {if dropped{queue_multi(p,paths[0].to_string_lossy().into());}return true;}
    if paths.is_empty() || paths.iter().any(|p|!p.extension().is_some_and(|e|e.eq_ignore_ascii_case("nki"))) {return false;}
    let mut selection=p.selection.write().unwrap();
    let target=(0..selection.parts.len()).find(|n|ui.scene().and_then(|s|s.surface(&format!("part-{n}"))).is_some_and(|s|{let r=s.frame;at.x>=r.x && at.x<r.x+r.size.width && at.y>=r.y && at.y<r.y+r.size.height}));
    let free=RACK_SLOTS.saturating_sub(selection.parts.iter().filter(|p|!p.path.is_empty()).count());
    if paths.len()>free+usize::from(target.is_some()){return false;}
    if dropped {for (n,path) in paths.iter().enumerate(){let path=path.to_string_lossy().into_owned();if n==0 && let Some(slot)=target {selection.parts[slot].path=path;selection.parts[slot].program=0;selection.parts[slot].group=u32::MAX;p.shared.focus_request.store(slot as u64,Ordering::Relaxed);}else if let Some(slot)=add_part(&mut selection,Part{path,..Default::default()}){p.shared.focus_request.store(slot as u64,Ordering::Relaxed);}}}
    true
}
enum RackDrag { Instrument(String), Part(usize) }
fn add_part(selection:&mut Selection,part:Part)->Option<usize> {
    let slot=selection.parts.iter().position(|p|p.path.is_empty()).or_else(||(selection.parts.len()<RACK_SLOTS).then_some(selection.parts.len()))?;
    if slot==selection.parts.len(){selection.parts.push(part);}else{selection.parts[slot]=part;}
    selection.order.retain(|n|*n!=slot as u32);selection.order.push(slot as u32);Some(slot)
}
fn move_part(selection:&mut Selection,from:usize,before:usize) {
    if from==before{return;}selection.order.retain(|n|*n!=from as u32);
    let to=selection.order.iter().position(|n|*n==before as u32).unwrap_or(selection.order.len());selection.order.insert(to,from as u32);
}
fn number(ui:&mut Ui,id:impl Into<Id>,label:&str,value:&mut f64,range:std::ops::RangeInclusive<f64>,display:String)->El {
    let c=drag_value(ui,id,label,value,range).size(S);
    row![caption(label).fill(Role::Dim),c.el.value_text(display).el().min_w(36)].gap(4).align(Align::Center)
}
/// The performance view of initialized scripts (the last slot with one) and their issues.
fn script_interface(rt:Option<&Runtime>)->(Option<Arc<crate::ksp::Interface>>,String) {
    let Some(rt)=rt else {return (None,String::new())};
    let interface=(0..rt.slots()).map(|slot|rt.interface(slot)).filter(|ui|ui.performance).last().map(Arc::new);
    (interface,rt.diagnostics().join("\n"))
}
/// Authored KSP coordinates belong inside this bounded canvas; the editor shell still reflows.
fn script_preview(ui:&mut Ui,interface:&crate::ksp::Interface,image:Option<&Arc<moose::mui::mui::scene::Image>>)->El {
    let available=ui.scene().and_then(|s|s.surface("ksp-preview")).map(|s|s.frame.size.width).unwrap_or(600.);
    let scale=(available/interface.width as f64).min(1.).max(0.25);
    let width=interface.width as f64*scale;let height=interface.height as f64*scale;
    let mut layers=Vec::new();
    if let Some(image)=image {let image=image.clone();layers.push(canvas(move |_|vec![Draw::image(0.,-68.*scale,image.width as f64*scale,image.height as f64*scale,image.clone())]).w(width).h(height).at(0,0).clip().id("instrument-wallpaper"));}
    for (n,c) in interface.controls.iter().enumerate() {
        let int=|name:&str,default:i32|match c.properties.get(&format!("$CONTROL_PAR_{name}")){Some(crate::ksp::Value::Int(n))=>*n,_=>default};
        let text=|name:&str|match c.properties.get(&format!("$CONTROL_PAR_{name}")){Some(crate::ksp::Value::Text(s))=>s.as_str(),_=>""};
        if int("HIDE",0)==1 {continue;}
        let (x,y,w,h)=(int("POS_X",0),int("POS_Y",0),int("WIDTH",85),int("HEIGHT",18));
        if x<0 || y<0 || x>=interface.width || y>=interface.height || w<=0 || h<=0 {continue;}
        let label=text("TEXT");if label.is_empty() && matches!(c.kind.as_str(),"ui_button"|"ui_label"){continue;}
        let id=format!("ksp-preview-{n}");
        let control=match c.kind.as_str() {
            "ui_knob"=>col![canvas(move |s|{let r=(s.height.min(s.width)/2.-2.).max(1.);let path=DrawPath::polyline((0..32).map(|n|{let a=n as f64*std::f64::consts::TAU/32.;Point::new(s.width/2.+a.cos()*r,s.height/2.+a.sin()*r)}),true);vec![Draw::fill(path,Color::srgb(0.2,0.2,0.2))]}).h((h as f64*scale-14.).max(8.)).w(Len::Pct(100.)),body(label).text_size(10).fill(Color::srgb(0.1,0.1,0.1)).h(14)].gap(0),
            "ui_slider"=>canvas(|s|vec![Draw::fill(DrawPath::polyline([Point::new(1.,s.height/2.-1.),Point::new(s.width-1.,s.height/2.-1.),Point::new(s.width-1.,s.height/2.+1.),Point::new(1.,s.height/2.+1.)],true),Color::srgb(0.35,0.35,0.35))]),
            _=>body(if c.kind=="ui_menu"{"—"}else{label}).text_size(10).fill(if image.is_some(){Color::srgb(0.08,0.08,0.08)}else{Color::srgb(0.85,0.85,0.85)}).pad((2,0)).radius(1),
        };
        let control=if matches!(c.kind.as_str(),"ui_switch"|"ui_button"|"ui_menu"){row![control].fill(Color::srgb(0.65,0.65,0.65))}else{control};
        layers.push(control.w((if c.kind=="ui_knob"{w.max(52)}else{w}).min(interface.width-x) as f64*scale).h(h.min(interface.height-y) as f64*scale).at(x as f64*scale,y as f64*scale).clip().disabled().named(format!("Preview only: {} {label}",c.kind)).id(id));
    }
    stack(layers).w(Len::Pct(100.)).h(height).align(Align::Start).clip().shrink(0).id("ksp-preview")
}
fn editor_ui(params:&Arc<SamplerParams>)->impl FnMut(&mut Ui,&mut moose::mui::Bridge<SamplerParams>)->El + Send + 'static {
    let mut search=String::new();let mut library=String::new();let mut show_multis=false;let mut tab=0;let mut settings=false;let mut octave=3i16;let mut selected=0usize;let mut notice=String::new();
    let mut root=params.selection.read().unwrap().root.clone();if root.is_empty(){root=import::LIBRARY_ROOT.into();}
    let mut last_poll=Instant::now()-Duration::from_secs(1);
    move |ui,bridge| {
        if last_poll.elapsed()>Duration::from_millis(100) {if let Some(t)=bridge.context().and_then(|c|c.tasks::<Load>()) {t.spawn_coalescing(Load);}last_poll=Instant::now();}
        let p=bridge.params().clone();let view=p.shared.view.lock().unwrap().clone();let mut selection=p.selection.read().unwrap().clone();let before=selection.clone();
        let focus=p.shared.focus_request.swap(128,Ordering::Relaxed);if focus<RACK_SLOTS as u64 {selected=focus as usize;tab=4;notice.clear();}selection.parts.truncate(RACK_SLOTS);
        for part in &mut selection.parts {part.channel=part.channel.clamp(-1,15);part.port=part.port.min(3);part.output=part.output.min(7);part.gain=if part.gain.is_finite(){part.gain.clamp(-60.,6.)}else{0.};part.pan=if part.pan.is_finite(){part.pan.clamp(-1.,1.)}else{0.};}
        let mut seen=[false;RACK_SLOTS];selection.order.retain(|n|{let n=*n as usize;if n>=selection.parts.len() || selection.parts[n].path.is_empty() || seen[n]{false}else{seen[n]=true;true}});
        for (n,part) in selection.parts.iter().enumerate(){if !part.path.is_empty() && !seen[n]{selection.order.push(n as u32);}}
        selected=selected.min(selection.parts.len().saturating_sub(1));p.shared.selected.store(selected as u64,Ordering::Relaxed);
        let library_of=|path:&Path|path.strip_prefix(&view.root).ok().and_then(|r|r.components().next()).map(|c|c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        let mut libraries=std::collections::BTreeMap::<String,Vec<&PathBuf>>::new();for file in view.files.iter() {libraries.entry(library_of(file)).or_default().push(file);}
        if library.is_empty() {library=selection.parts.get(selected).map(|p|library_of(Path::new(&p.path))).filter(|s|!s.is_empty()).or_else(||libraries.keys().next().cloned()).unwrap_or_default();}
        let query=text_input(ui,"search",&mut search);let needle=search.to_lowercase();let mut library_rows=Vec::new();
        for (idx,(name,files)) in libraries.iter().enumerate() {
            if !needle.is_empty() && !files.iter().any(|f|f.to_string_lossy().to_lowercase().contains(&needle)) {continue;}
            let label=name.replace("Performance Samples ","").replace(" Library","");let id=format!("library-{idx}");
            if ui.get(id.as_str()).activated() {library=name.clone();}
            let mut tile=Vec::new();if let Some(image)=view.artwork.get(name) {tile.push(block(Len::Pct(100.),Len::Auto).aspect(image.width as f64/image.height as f64).fill(Fill::Image(image.clone(),Fit::Contain)).shrink(0));}
            tile.push(row![body(label).text_size(12).lines(2).flex(1).min_w(0),caption(files.len().to_string()).fill(Role::Dim)].align(Align::Center).gap(6).pad((8,5)));
            library_rows.push(col(tile).gap(0).fill(if library==*name {Role::Raised}else{Role::Surface}).radius(0).focusable().a11y(A11y::Button).named(format!("{name}, {} presets",files.len())).id(id).on(State::Hover,|s|s.fill(Role::Raised)).shrink(0));
            library_rows.push(rule());
        }
        let mut picker_tabs=Vec::new();for (multi,label,id) in [(false,"Instruments","picker-instruments"),(true,"Multis","picker-multis")] {let(hit,el)=action(ui,id,label,show_multis==multi);if hit{show_multis=multi;}picker_tabs.push(el);}
        let mut presets=Vec::new();let files:Vec<_>=view.files.iter().filter(|f|import::is_multi(f)==show_multis && library_of(f)==library && (needle.is_empty() || f.to_string_lossy().to_lowercase().contains(&needle))).collect();
        for (n,path) in files.iter().enumerate() {
            let label=path.file_stem().unwrap_or_default().to_string_lossy();let loaded=selection.parts.iter().any(|p|p.path==path.to_string_lossy());
            let id=format!("instrument-{n}");let (hit,el)=action(ui,id.as_str(),&label,loaded);if ui.get(id.as_str()).dragged {ui.start_drag(id.as_str(),RackDrag::Instrument(path.to_string_lossy().into()));}
            if hit {if import::is_multi(path){queue_multi(&p,path.to_string_lossy().into());notice.clear();}else if let Some(slot)=selection.parts.iter().position(|p|p.path==path.to_string_lossy() && p.program==0){selected=slot;tab=4;notice.clear();}else if let Some(slot)=add_part(&mut selection,Part{path:path.to_string_lossy().into(),..Default::default()}) {selected=slot;tab=4;notice.clear();}else{notice="Rack is full (16 instruments). Remove one to add another.".into();}}
            presets.push(el.min_w(0).w(Len::Pct(100.)).lines(2).min_h(28).shrink(0).tip(format!("{}: {}",if import::is_multi(path){"Load multi into rack"}else{"Open instrument"},path.display())));
        }
        if presets.is_empty() {presets.push(body(if show_multis{"No multis in this library"}else{"No matching instruments"}).fill(Role::Dim).lines(2).pad(8));}
        let browser=col![row![body("Libraries").text_weight(Weight::SEMIBOLD),spacer(),caption(view.files.len().to_string()).fill(Role::Dim)].align(Align::Center).pad((8,8)),row![caption("Search").fill(Role::Dim),query.el.radius(0).h(28).flex(1).min_w(0).named("Search presets in the selected library")].gap(6).pad((8,4)).shrink(0),col(library_rows).gap(0).flex(1).min_h(0).scroll().id("libraries-scroll"),rule(),row![row(picker_tabs).gap(0),spacer()].align(Align::Center).pad((8,6)),col(presets).gap(0).flex(1).min_h(0).scroll().id(format!("presets-{library}-{needle}"))].gap(0).w(Len::Pct(30.)).min_w(270).max_size(Size::new(360.,100000.)).shrink(0).fill(Role::Field).clip();
        let mut tabs=Vec::new();for (n,name) in [(0,"Rack"),(4,"Instrument"),(1,"Mapping"),(2,"Groups"),(3,"Info")] {let (hit,el)=action(ui,format!("tab-{n}"),name,tab==n);if hit {tab=n;}tabs.push(el);}
        let (settings_hit,settings_el)=action(ui,"settings","Folders",settings);if settings_hit {settings=!settings;}
        let (thru,thru_el)=action(ui,"midi-thru","MIDI thru",selection.midi_thru);if thru{selection.midi_thru=!selection.midi_thru;}
        let (panic,panic_el)=action(ui,"panic","Panic",false);if panic {p.shared.panic.store(true,Ordering::Release);}
        let toolbar=row![body("KONTAKTO").text_size(16).text_weight(Weight::SEMIBOLD),row(tabs).gap(0),spacer(),thru_el,panic_el,settings_el].gap(8).align(Align::Center).pad((12,6)).shrink(0).fill(Role::Raised);
        let mut content=Vec::new();
        if settings {let field=text_input(ui,"root",&mut root);let (scan,scan_el)=action(ui,"scan","Rescan",false);if scan {selection.root=root.clone();p.shared.view.lock().unwrap().root.clear();}content.push(row![field.el.flex(1).min_w(0).radius(0).named("Library folder"),scan_el].gap(6).pad(8).shrink(0));}
        let used:usize=view.parts.iter().map(|v|v.bytes).sum();
        content.push(row![caption(format!("{}  /  {} instruments",if selection.multi.is_empty(){"Untitled multi".into()}else{Path::new(&selection.multi).file_stem().unwrap_or_default().to_string_lossy()},selection.parts.iter().filter(|p|!p.path.is_empty()).count())).lines(2).flex(1).min_w(0).fill(Role::Dim),spacer(),caption(format!("{:.0} MB",used as f64/1048576.)).fill(Role::Dim)].align(Align::Center).pad((12,7)).shrink(0));
        let mut rack=Vec::new();let mut remove=None;let mut drop=None;let mut duplicate=None;let mut reorder=None;
        for (position,slot) in selection.order.clone().into_iter().map(|n|n as usize).enumerate() {
            let part=&mut selection.parts[slot];
            if part.path.is_empty(){continue;}
            let pv=&view.parts[slot];let name=pv.instrument.as_ref().filter(|i|i.path==Path::new(&part.path) && pv.program==part.program).map(|i|i.name.clone()).unwrap_or_else(||Path::new(&part.path).file_stem().unwrap_or_default().to_string_lossy().into_owned());
            let id=format!("part-{slot}");let (choose,name_el)=action(ui,id.as_str(),&name,false);if choose {selected=slot;tab=4;library=library_of(Path::new(&part.path));}if ui.get(id.as_str()).dragged {ui.start_drag(id.as_str(),RackDrag::Part(slot));}if let Some(payload)=ui.dropped_on::<RackDrag>(id.as_str()){drop=Some((slot,payload));}
            let (m,me)=action(ui,format!("mute-{slot}"),"M",part.mute);if m{part.mute=!part.mute;}
            let (solo,se)=action(ui,format!("solo-{slot}"),"S",part.solo);if solo{part.solo=!part.solo;}
            let (close,ce)=action(ui,format!("remove-{slot}"),"Remove",false);if close{remove=Some(slot);}
            let header=row![caption(format!("{:02}",position+1)).fill(Role::Dim),name_el.flex(1).min_w(0).lines(2),me.named(format!("Mute {name}")),se.named(format!("Solo {name}")),ce].gap(4).align(Align::Center).pad((8,3)).fill(if selected==slot{Role::Raised}else{Role::Surface});
            let mut rows=vec![header];
            if tab!=4 && (selected==slot || tab==0) {
                let mut gain=part.gain as f64;let g=number(ui,format!("gain-{slot}"),"Level",&mut gain,-60.0..=6.0,format!("{:.1} dB",part.gain));part.gain=gain as f32;
                let mut pan=part.pan as f64;let pe=number(ui,format!("pan-{slot}"),"Pan",&mut pan,-1.0..=1.0,if part.pan.abs()<0.01{"C".into()}else{format!("{:.0}{}",part.pan.abs()*100.,if part.pan<0.{"L"}else{"R"})});part.pan=pan as f32;
                let mut channel=(part.channel+1) as f64;let che=number(ui,format!("channel-{slot}"),"Ch",&mut channel,0.0..=16.0,if part.channel<0{"Omni".into()}else{format!("{}",part.channel+1)});part.channel=channel.round() as i16-1;
                let mut port=part.port as f64+1.;let port_el=number(ui,format!("port-{slot}"),"MIDI",&mut port,1.0..=4.0,format!("{}",part.port+1));part.port=port.round() as u8-1;
                let mut output=part.output as f64+1.;let output_el=number(ui,format!("output-{slot}"),"Out",&mut output,1.0..=8.0,format!("{}",part.output+1));part.output=output.round() as u8-1;
                rows.push(row![port_el,che,output_el,spacer(),g,pe].gap(8).align(Align::Center).pad((10,5)));
                if selected==slot {
                    let (dup,de)=action(ui,format!("duplicate-{slot}"),"Duplicate",false);if dup{duplicate=Some(part.clone());}
                    let (up,ue)=action(ui,format!("up-{slot}"),"Up",false);if up && position>0{reorder=Some((slot,selection.order[position-1] as usize));}
                    let (down,dne)=action(ui,format!("down-{slot}"),"Down",false);if down && position+1<selection.order.len(){reorder=Some((selection.order[position+1] as usize,slot));}
                    rows.push(row![de,ue,dne,spacer(),caption("Drag values · double-click to type").fill(Role::Dim)].gap(2).align(Align::Center).pad((8,0)));
                    let (audition,audition_el)=action(ui,format!("audition-{slot}"),"Audition",false);if audition {p.shared.audition.store(true,Ordering::Release);}
                    rows.push(row![spacer(),audition_el].pad((8,3)));

                }
                if pv.status.starts_with("Load failed") {rows.push(body(format!("{}{}",pv.status,if pv.active.is_empty(){String::new()}else{format!(". Still playing: {}",pv.active)})).text_size(12).fill(Role::Danger).lines(4).pad((10,5)));}
            }
            rack.push(col(rows).gap(0).fill(Role::Surface).shrink(0));rack.push(rule());
        }
        if let Some((slot,payload))=drop {match payload{RackDrag::Instrument(path)=>{if import::is_multi(Path::new(&path)){queue_multi(&p,path);}else{selection.parts[slot].path=path;selection.parts[slot].program=0;selection.parts[slot].group=u32::MAX;selected=slot;tab=4;}},RackDrag::Part(from)=>move_part(&mut selection,from,slot)}}
        if let Some((from,to))=reorder {move_part(&mut selection,from,to);}
        if let Some(part)=duplicate {if let Some(slot)=add_part(&mut selection,part){selected=slot;}else{notice="Rack is full (16 instruments).".into();}}
        if let Some(RackDrag::Instrument(path))=ui.dropped_on::<RackDrag>("rack-drop") {if import::is_multi(Path::new(&path)){queue_multi(&p,path);}else if let Some(slot)=add_part(&mut selection,Part{path,..Default::default()}){selected=slot;tab=4;notice.clear();}else{notice="Rack is full (16 instruments).".into();}}
        if let Some(slot)=remove {selection.parts[slot]=Part::default();selected=selection.parts.iter().position(|p|!p.path.is_empty()).unwrap_or(0);}
        if selection.parts.iter().all(|p|p.path.is_empty()) {rack.push(col![body("Your multi is empty").text_size(19),body(if show_multis{"Select a multi in the browser to load its instruments."}else{"Select an instrument in the browser to add it."}).fill(Role::Dim).lines(2)].gap(8).pad(24));}
        rack.push(body(if show_multis{"Drop a multi here to replace the rack"}else if ui.dragging::<RackDrag>().is_some(){"Drop a preset here"}else{"Drag an instrument here to add · drag a rack name to reorder"}).text_size(12).fill(Role::Dim).lines(2).pad(16).focusable().a11y(A11y::Button).named("Load preset drop target").id("rack-drop").on(State::Hover,|s|s.fill(Role::Raised)).shrink(0));
        if !view.multi_status.is_empty(){content.push(caption(view.multi_status.clone()).fill(Role::Dim).lines(2).pad((12,4)).shrink(0));}
        if !notice.is_empty(){content.push(body(notice.clone()).fill(Role::Warning).text_size(12).lines(2).pad(8).shrink(0));}
        if tab==0 {content.push(col(rack).gap(2).flex(1).min_h(0).scroll().id("rack-scroll"));}
        else {
            content.push(col(rack).gap(1).max_size(Size::new(100000.,if tab==4{84.}else{160.})).min_h(0).scroll().id("rack-summary").shrink(0));
            let pv=&view.parts[selected];let i=pv.instrument.as_deref();let group=selection.parts.get(selected).map(|p|p.group).unwrap_or(0);
            if tab==4 {
                let mut panel=Vec::new();
                if let Some(part)=selection.parts.get_mut(selected).filter(|p|!p.path.is_empty()) {
                    let family=library_of(Path::new(&part.path));
                    let multi=import::is_multi(Path::new(&part.path));
                    let candidates:Vec<_>=view.files.iter().filter(|path|import::is_multi(path)==multi && library_of(path)==family).collect();
                    let (previous,prev_el)=action(ui,"preset-prev","Previous",false);let (next,next_el)=action(ui,"preset-next","Next",false);
                    if let Some(index)=candidates.iter().position(|path|path.to_string_lossy()==part.path) {
                        let target=if previous{index.checked_sub(1)}else if next && index+1<candidates.len(){Some(index+1)}else{None};
            if let Some(target)=target {if multi{queue_multi(&p,candidates[target].to_string_lossy().into());}else{part.path=candidates[target].to_string_lossy().into();part.program=0;part.group=u32::MAX;}}
                    }
                    let index=candidates.iter().position(|path|path.to_string_lossy()==part.path);let prev_el=if index.is_some_and(|n|n>0){prev_el}else{prev_el.disabled()};let next_el=if index.is_some_and(|n|n+1<candidates.len()){next_el}else{next_el.disabled()};
                    panel.push(row![caption(family.replace("Performance Samples ","")).fill(Role::Dim).lines(2).flex(1).min_w(0),prev_el,next_el].gap(4).align(Align::Center).pad((12,4)));
                    let current=pv.instrument.as_deref().filter(|i|i.path==Path::new(&part.path) && pv.program==part.program);
                    panel.push(row![body(current.map(|i|i.name.as_str()).unwrap_or("Loading instrument…")).text_size(20).lines(2).flex(1).min_w(0),caption("INSTRUMENT").fill(Role::Dim)].align(Align::Center).gap(8).pad((12,10)));
                    if current.is_some_and(|i|!i.scripts.is_empty()) {panel.push(caption(if pv.interface.is_some(){"Scripts run · interface preview only"}else{"Manual performance · scripted interface unavailable"}).fill(Role::Dim).pad((12,4)).lines(2).tip(pv.interface_status.clone()));}
                    let mut gain=part.gain as f64;let gain_el=number(ui,"performance-gain","Level",&mut gain,-60.0..=6.0,format!("{:.1} dB",part.gain));part.gain=gain as f32;
                    let mut pan=part.pan as f64;let pan_el=number(ui,"performance-pan","Pan",&mut pan,-1.0..=1.0,format!("{:.0}",part.pan*100.));part.pan=pan as f32;
                    let (audition,play)=action(ui,"performance-play","Audition",false);if audition {p.shared.audition.store(true,Ordering::Release);}
                    panel.push(row![gain_el,pan_el,spacer(),play].gap(12).align(Align::Center).pad((12,8)));
                    if current.is_some() {if let Some(interface)=&pv.interface {panel.push(script_preview(ui,interface,pv.wallpaper.as_ref()));}else if let Some(image)=&pv.wallpaper {panel.push(block(Len::Pct(100.),180).fill(Fill::Image(image.clone(),Fit::Contain)).shrink(0).named("Original instrument wallpaper").id("instrument-wallpaper"));}}
                    panel.push(rule());
                    if !pv.wallpaper_status.is_empty() {panel.push(caption(pv.wallpaper_status.clone()).fill(Role::Dim).lines(3).pad(12));}
                    if pv.status.starts_with("Load failed") {panel.push(body(format!("{}{}",pv.status,if pv.active.is_empty(){String::new()}else{format!(". Still playing: {}",pv.active)})).text_size(12).fill(Role::Danger).lines(4).pad(12));}
                }else {panel.push(body("Choose a preset in the library to open its instrument view.").fill(Role::Dim).lines(3).pad(20));}
                content.push(col(panel).gap(0).flex(1).min_h(0).scroll().id("performance-scroll"));
            }
            else if tab==1 {content.push(mapping(i,group).h(Len::Auto).flex(1).min_h(0).radius(0));content.push(row![caption("C−1"),spacer(),caption("Key / velocity"),spacer(),caption("G9")].pad(8).shrink(0));}
            else if tab==2 {let mut rows=Vec::new();if let Some(i)=i {for (n,g) in i.groups.iter().enumerate() {let(hit,e)=action(ui,format!("group-{n}"),&g.name,group==n as u32);if hit {if let Some(p)=selection.parts.get_mut(selected){p.group=n as u32;}}rows.push(e.lines(2).shrink(0));rows.push(rule());}}content.push(col(rows).gap(0).flex(1).min_h(0).scroll().id("groups-scroll"));}
            else {let mut rows=vec![body("Scripts play the groups they choose; long samples stream from disk").text_weight(Weight::SEMIBOLD),body("Script interfaces are preview only; engine parameters and modulation from scripts are not applied yet.").fill(Role::Dim).lines(4)];if let Some(i)=i {rows.push(body(format!("{} groups · {} zones · {} missing references",i.groups.len(),i.zones.len(),i.missing_samples.len())).lines(3));rows.push(caption(i.path.display().to_string()).fill(Role::Dim).lines(4));for w in &i.warnings {rows.push(body(w.as_str()).text_size(12).fill(Role::Dim).lines(6).shrink(0));}}content.push(col(rows).gap(8).pad(12).flex(1).min_h(0).scroll().id("details-scroll"));}
        }
        let master_text=format!("{:.1} dB",p.volume.value());let master=bridge.bind(ui,P::Volume,|ui,id,v|{let mut db=*v*66.-60.;let control=drag_value(ui,id,"Master",&mut db,-60.0..=6.0).size(S).value_text(master_text);*v=(db+60.)/66.;control});
        let status=view.parts.get(selected).map(|v|v.status.as_str()).filter(|s|!s.is_empty()).unwrap_or(&view.status);
        content.push(row![caption(status).lines(2).flex(1).min_w(0),caption("Master").fill(Role::Dim),master.w(70)].gap(8).align(Align::Center).pad((10,6)).shrink(0).fill(Role::Surface));
        let workspace=col(content).gap(0).flex(1).min_w(0).min_h(0).fill(Role::Background);
        let (down,a)=action(ui,"octave-down","Lower",false);let (up,b)=action(ui,"octave-up","Higher",false);if down || up {release_keyboard(&p);}if down {octave=(octave-1).max(0);}if up {octave=(octave+1).min(6);}
        let keyboard=row![col![caption(format!("{} – {}",note_name((octave*12) as u8),note_name((octave*12+47) as u8))).fill(Role::Dim),row![a,b].gap(0)].gap(4).shrink(0),keybed(ui,&p,octave).flex(1).min_w(0)].gap(12).align(Align::Center).pad((12,8)).shrink(0).fill(Role::Raised);
        if selection!=before {let mut current=p.selection.write().unwrap();if *current==before{*current=selection;let _=p.shared.controls.force_push(rack_controls(&current));p.shared.midi_thru.store(current.midi_thru,Ordering::Release);}}p.shared.selected.store(selected as u64,Ordering::Relaxed);
        col![toolbar,rule(),row![workspace,block(1,Len::Pct(100.)).fill(Role::Dim.alpha(0.15)),browser].flex(1).min_h(0),rule(),keyboard,row![caption("4 MIDI inputs · 8 stereo outputs").fill(Role::Dim),spacer(),meter(ui,"output",bridge.meter(P::Level)).w(110).h(4)].align(Align::Center).pad((12,4)).shrink(0)].gap(0).full().fill(Role::Background).radius(0).clip()
    }
}

moose::plugin! { logic:Sampler, params:SamplerParams, tasks:[Load] }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_contract(){assert!(SamplerParams::new().param_infos().iter().all(|p|p.midi_map.is_none()),"global MIDI parameter bindings would collapse port/channel routing");moose_test::assert_valid_info::<Plugin>();moose_test::assert_has_editor::<Plugin>();moose_test::assert_state_round_trip::<Plugin>();}
    #[test]
    fn rack_state_round_trip() {
        use moose::core::custom_state::State;
        let state=Selection{root:"/libraries".into(),multi:String::new(),parts:vec![Part{path:"first.nkm".into(),program:2,channel:2,gain:-9.,pan:0.25,mute:true,..Default::default()},Part{path:"second.nki".into(),group:3,solo:true,..Default::default()}],order:vec![1,0],midi_thru:true};
        assert!(Selection::deserialize(&state.serialize()).unwrap()==state);
    }
    #[test]
    fn process_routes_bus_and_midi_thru() {
        use crate::{audio::Sample,import::{Group,Zone,Loop}};
        use moose::core::bus_routing::{BusRouting,BusActivation};
        let mut dsp=Dsp::default();let p=SamplerParams::new();p.shared.midi_thru.store(true,Ordering::Relaxed);
        let group=Group{name:"test".into(),..Group::default()};
        let zone=Zone{loop_range:Some(Loop{start:0,end:100,until_release:false,crossfade:0}),..Zone::default()};
        dsp.rack.parts[1].set_bank(Some(Box::new(Bank::from_samples(vec![group],vec![zone],vec![(PathBuf::new(),Sample{rate:48000,frames:vec![[0.5,0.25];100]})]).unwrap())));
        dsp.rack.controls[1].port=1;dsp.rack.controls[1].output=2;
        let mut events=EventList::with_capacity(4);events.push(Event::on_port(16,1,EventBody::NoteOn{group:0,channel:0,note:60,velocity:127}));
        let mut outputs=vec![vec![0f32;128];6];let mut refs:Vec<_>=outputs.iter_mut().map(|o|o.as_mut_slice()).collect();let mut buffer=AudioBuffer::from_slices_checked(&[],&mut refs,128);
        let transport=TransportInfo::default();let mut midi_out=EventList::with_capacity(4);let mut routes=BusRouting::new();for _ in 0..3 {routes.push_output(2,BusActivation::Active);}
        let mut cx=ProcessContext::new(&transport,48000.,128,&mut midi_out).with_bus_routing(routes);Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx);
        for ch in 0..4 {assert!(outputs[ch].iter().all(|x|*x==0.));}assert!(outputs[4][..16].iter().all(|x|*x==0.));assert!(outputs[4][127]>0.);assert!((outputs[4][127]-2.*outputs[5][127]).abs()<1e-6);
        assert_eq!(midi_out.len(),1);assert_eq!(midi_out.get(0).unwrap().port,0);assert_eq!(midi_out.get(0).unwrap().sample_offset,16);
    }
    #[test]
    #[ignore="requires the owner's local Vista library"]
    fn worker_loads_and_removes_real_rack_parts() {
        let p=SamplerParams::new();let path=Path::new(import::LIBRARY_ROOT).join("Performance Samples Vista/Instruments/Bonus/Vista - Harp.nki").to_string_lossy().into_owned();
        p.selection.write().unwrap().parts=vec![Part{path:path.clone(),group:0,..Default::default()},Part{path,group:1,output:1,..Default::default()}];
        p.shared.view.lock().unwrap().root=import::LIBRARY_ROOT.into();Load.run(&p);
        assert!(p.shared.view.lock().unwrap().parts[0].wallpaper.is_some(),"Vista named NKR wallpaper loads on the worker");
        {let view=p.shared.view.lock().unwrap();let interface=view.parts[0].interface.as_ref().expect("Vista KSP init");assert_eq!(interface.controls.len(),47);assert_eq!(interface.height,180);assert!(interface.controls.iter().any(|c|matches!(c.properties.get("$CONTROL_PAR_TEXT"),Some(crate::ksp::Value::Text(s)) if s=="Mic Mixer")));}
        let mut dsp=Dsp::default();let transport=TransportInfo::default();let mut events=EventList::with_capacity(4);events.push(Event::new(0,EventBody::NoteOn{group:0,channel:0,note:60,velocity:100}));let mut outgoing=EventList::with_capacity(4);
        let mut data=vec![vec![0f32;2048];4];let mut channels:Vec<_>=data.iter_mut().map(|v|v.as_mut_slice()).collect();let mut buffer=AudioBuffer::from_slices_checked(&[],&mut channels,2048);
        let mut routing=moose::core::bus_routing::BusRouting::new();for _ in 0..2{routing.push_output(2,moose::core::bus_routing::BusActivation::Active);}
        let mut cx=ProcessContext::new(&transport,48000.,2048,&mut outgoing).with_bus_routing(routing);Sampler::process(&mut dsp,&p,&mut buffer,&events,&mut cx);
        assert!(dsp.rack.parts[0].bank().is_some() && dsp.rack.parts[1].bank().is_some());
        for ch in [0,2] {assert!(buffer.output(ch).iter().all(|x|x.is_finite()));assert!(buffer.output(ch).iter().any(|x|x.abs()>0.00001));}
        p.selection.write().unwrap().parts[0]=Part::default();Load.run(&p);Sampler::process(&mut dsp,&p,&mut buffer,&EventList::with_capacity(0),&mut cx);
        assert!(dsp.rack.parts[0].bank().is_none());assert!(dsp.rack.parts[1].bank().is_some());assert!(dsp.rack.parts[1].active_voices()>0);
        // Scripts run on the audio thread; their persistent values reach the host state.
        assert!(dsp.rack.parts[1].script().is_some());
        Load.run(&p);Sampler::process(&mut dsp,&p,&mut buffer,&EventList::with_capacity(0),&mut cx);Load.run(&p);
        let saved=p.selection.read().unwrap().parts[1].script_state.clone();assert!(saved.starts_with("[{"),"the instrument's values are saved");
        // Restored host state rebuilds only the scripts: "[]" restores nothing, leaving declared defaults.
        let mut round_trip=|state:&str|{let epoch=dsp.script_epoch[1];p.selection.write().unwrap().parts[1].script_state=state.into();Load.run(&p);Sampler::process(&mut dsp,&p,&mut buffer,&EventList::with_capacity(0),&mut cx);
            assert!(dsp.script_epoch[1]>epoch);assert!(dsp.rack.parts[1].bank().is_some() && dsp.rack.parts[1].script().is_some());
            Load.run(&p);Sampler::process(&mut dsp,&p,&mut buffer,&EventList::with_capacity(0),&mut cx);Load.run(&p);p.selection.read().unwrap().parts[1].script_state.clone()};
        assert!(round_trip("[]")!=saved,"declared defaults differ from the saved values");
        assert!(round_trip(&saved)==saved,"saved values restore exactly");
        // A host rate change rebuilds the effects on the loader and keeps the bank.
        Sampler::reset(&mut dsp,&p,&AudioConfig::new(44100.,2048));Load.run(&p);assert_eq!(p.shared.view.lock().unwrap().parts[1].fx_rate,44100.);assert!(!p.shared.ready.is_empty());
        Sampler::process(&mut dsp,&p,&mut buffer,&EventList::with_capacity(0),&mut cx);assert!(p.shared.ready.is_empty());assert!(dsp.rack.parts[1].bank().is_some());
    }
    #[test]
    fn rack_interactions() {
        let p=Arc::new(SamplerParams::new());
        {let mut v=p.shared.view.lock().unwrap();v.root="/virtual".into();v.files=Arc::new(vec!["/virtual/Library/Piano.nki".into(),"/virtual/Library/Strings.nki".into(),"/virtual/Library/Ensemble.nkm".into()]);}
        let mut ui=editor_theme();let mut build=editor_ui(&p);let mut bridge=moose::mui::Bridge::new(p.clone());
        let mut tick=|ui:&mut Ui,input:Input|{let root=build(ui,&mut bridge);ui.frame(root,Some(Size::new(1180.,780.)),input,1./60.).unwrap();};
        for _ in 0..3 {tick(&mut ui,Input::default());}
        fn center(ui:&Ui,id:&str)->Point {let r=ui.scene().unwrap().surface(id).unwrap().frame;Point::new(r.x+r.size.width/2.,r.y+r.size.height/2.)}
        let pointer=|pos,down|Input{pointer:PointerInput{pos:Some(pos),buttons:if down{Buttons::PRIMARY}else{Buttons::default()},..Default::default()},..Default::default()};
        let from=center(&ui,"instrument-0");let to=center(&ui,"rack-drop");
        for (pos,down) in [(from,true),(Point::new(from.x-15.,from.y),true),(to,true),(to,false)] {tick(&mut ui,pointer(pos,down));}
        for _ in 0..2 {tick(&mut ui,Input::default());}assert_eq!(p.selection.read().unwrap().parts.len(),1);
        ui.focus("instrument-1");tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3 {tick(&mut ui,Input::default());}
        assert_eq!(p.selection.read().unwrap().parts.len(),2);
        for id in ["instrument-0","preset-next"] {ui.focus(id);tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3{tick(&mut ui,Input::default());}}
        assert_eq!(p.selection.read().unwrap().parts.len(),2);assert!(p.selection.read().unwrap().parts[0].path.ends_with("Strings.nki"));
        ui.focus("preset-prev");tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3{tick(&mut ui,Input::default());}assert!(p.selection.read().unwrap().parts[0].path.ends_with("Piano.nki"));
        for id in ["picker-multis","instrument-0"] {ui.focus(id);tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3{tick(&mut ui,Input::default());}}
        assert!(p.shared.multi_request.lock().unwrap().take().unwrap().ends_with("Ensemble.nkm"));assert_eq!(p.selection.read().unwrap().parts.len(),2);
        ui.focus("picker-instruments");tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3{tick(&mut ui,Input::default());}

        let from=center(&ui,"part-1");let to=center(&ui,"part-0");
        for (pos,down) in [(from,true),(Point::new(from.x-15.,from.y),true),(to,true),(to,false)] {tick(&mut ui,pointer(pos,down));}
        for _ in 0..2 {tick(&mut ui,Input::default());}assert_eq!(p.selection.read().unwrap().order,vec![1,0]);
        for id in ["part-0","mute-0"] {ui.focus(id);tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3 {tick(&mut ui,Input::default());}}
        assert!(p.selection.read().unwrap().parts[0].mute);
        assert!(ui.scene().unwrap().surface("performance-scroll").is_some(),"preset selection opens the instrument view");
        ui.focus("tab-0");tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3 {tick(&mut ui,Input::default());}
        for (id,text) in [("output-0","4"),("channel-0","2"),("port-0","2")] {
            ui.focus(id);tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..2 {tick(&mut ui,Input::default());}
            tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Char('a'),mods:Mods{ctrl:true,..Default::default()}}],..Default::default()});tick(&mut ui,Input{text:text.into(),..Default::default()});tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..3 {tick(&mut ui,Input::default());}
        }
        {let s=p.selection.read().unwrap();assert_eq!((s.parts[0].output,s.parts[0].channel,s.parts[0].port),(3,1,1));}
        let key=center(&ui,"key-60");tick(&mut ui,pointer(key,true));tick(&mut ui,pointer(key,false));tick(&mut ui,Input::default());assert_eq!(p.shared.keyboard.pop(),Some((0,60,true)));assert_eq!(p.shared.keyboard.pop(),Some((0,60,false)));p.shared.key_owners[61].store(1,Ordering::Relaxed);release_keyboard(&p);assert_eq!(p.shared.keyboard.pop(),Some((1,61,false)));release_keyboard(&p);assert!(p.shared.keyboard.pop().is_none());
        ui.focus("remove-0");tick(&mut ui,Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()});for _ in 0..2 {tick(&mut ui,Input::default());}let s=p.selection.read().unwrap();assert!(s.parts[0].path.is_empty());assert!(s.parts[1].path.ends_with("Strings.nki"));drop(s);
        let files=vec![PathBuf::from("/external/Native.nki")];let at=Point::new(10.,10.);
        assert!(native_files(&p,&ui,at,&files,false));assert!(p.selection.read().unwrap().parts[0].path.is_empty());assert!(native_files(&p,&ui,at,&files,true));assert!(p.selection.read().unwrap().parts[0].path.ends_with("Native.nki"));
        assert!(!native_files(&p,&ui,at,&[PathBuf::from("bad.wav")],true));
        assert!(!native_files(&p,&ui,at,&vec![PathBuf::from("full.nki");RACK_SLOTS],true));
        let at=center(&ui,"part-1");assert!(native_files(&p,&ui,at,&files,true));assert!(p.selection.read().unwrap().parts[1].path.ends_with("Native.nki"));
        let multi=[PathBuf::from("/external/Multi.nkm")];let before=p.selection.read().unwrap().clone();assert!(native_files(&p,&ui,at,&multi,false));assert!(p.shared.multi_request.lock().unwrap().is_none());assert!(native_files(&p,&ui,at,&multi,true));assert_eq!(p.shared.multi_request.lock().unwrap().take().unwrap(),"/external/Multi.nkm");assert!(*p.selection.read().unwrap()==before);

    }
    #[test]
    #[ignore="requires the owner's local Chorus multi"]
    fn real_multi_opens_embedded_instruments() {
        let p=SamplerParams::new();p.shared.view.lock().unwrap().root=import::LIBRARY_ROOT.into();
        queue_multi(&p,format!("{}/Audio Imperia CHORUS/Multis/10 Chorus - Ensemble - Traditional Syllables.nkm",import::LIBRARY_ROOT));Load.run(&p);
        let s=p.selection.read().unwrap();assert_eq!(s.parts.iter().map(|p|p.program).collect::<Vec<_>>(),vec![0,1,2]);assert!(s.parts.iter().all(|p|p.path==s.multi));
        let view=p.shared.view.lock().unwrap();assert!(view.parts[0].instrument.as_ref().unwrap().zones.is_empty());assert!(view.parts[0].instrument.as_ref().unwrap().missing_samples.is_empty());assert!(view.parts[1].instrument.as_ref().unwrap().name.contains("Women"));assert!(view.parts[2].instrument.as_ref().unwrap().name.contains("Men"));assert!(view.parts[0].status.starts_with("Controller instrument"));
    }
    #[test]
    fn failed_multi_keeps_the_rack() {
        let p=SamplerParams::new();p.shared.view.lock().unwrap().root=import::LIBRARY_ROOT.into();let before=p.selection.read().unwrap().clone();queue_multi(&p,"/missing.nkm".into());Load.run(&p);assert!(*p.selection.read().unwrap()==before);assert!(p.shared.view.lock().unwrap().multi_status.starts_with("Multi load failed"));
    }
    #[test]
    fn screenshot(){
        use moose::mui::mui::vello::{self, vello_cpu::{Pixmap,RenderContext,Resources}};
        let files=import::presets(Path::new(import::LIBRARY_ROOT)).unwrap_or_default();
        let instruments:Vec<_>=["Vista - Harp","Vista - 3 Cellos","Vista - 5 Violins"].iter().filter_map(|name|files.iter().find(|p|p.file_stem().is_some_and(|n|n==*name)).and_then(|p|import::read(p).ok()).map(Arc::new)).collect();
        std::fs::create_dir_all(".impeccable/review").unwrap();
        for (state,loaded,target) in [("empty",false,None),("loaded",true,None),("instrument",true,Some("tab-4")),("mapping",true,Some("tab-1")),("groups",true,Some("tab-2")),("multis",false,Some("picker-multis")),("details",true,Some("tab-3")),("settings",true,Some("settings")),("error",true,None)] {
          for (width,height) in [(1180,780),(900,640)] {
            let p=Arc::new(SamplerParams::new());
            {let mut view=p.shared.view.lock().unwrap();view.artwork=artwork::scan(Path::new(import::LIBRARY_ROOT),&files);view.files=Arc::new(files.clone());view.root=import::LIBRARY_ROOT.into();
             if loaded {for (slot,i) in instruments.iter().enumerate() {let group=i.first_playable_group().unwrap();p.selection.write().unwrap().parts.push(Part{path:i.path.to_string_lossy().into(),group:group as u32,channel:slot as i16,..Default::default()});view.parts[slot]=PartView{interface:script_interface(load_scripts(i,i.script_state.clone(),48000.).0.as_deref()).0,wallpaper:artwork::performance(i,None).unwrap_or(None),instrument:Some(i.clone()),active:i.name.clone(),status:if state=="error" && slot==0 {"Load failed: missing sample data in archive".into()}else{format!("{} groups · {} zones",i.groups.len(),i.zones.len())},..Default::default()};}}}
            let mut ui=editor_theme();let mut build=editor_ui(&p);let mut bridge=moose::mui::Bridge::new(p.clone());
            for _ in 0..3 {let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),Input::default(),1./60.).unwrap();}
            if state=="multis" {let lib=files.iter().filter_map(|p|p.strip_prefix(import::LIBRARY_ROOT).ok()?.components().next().map(|c|c.as_os_str().to_string_lossy().into_owned())).collect::<std::collections::BTreeSet<_>>().iter().position(|name|name=="Audio Imperia CHORUS").unwrap();ui.focus(format!("library-{lib}"));let input=Input{keys:vec![KeyPress{key:Key::Enter,mods:Mods::default()}],..Default::default()};let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),input,1./60.).unwrap();for _ in 0..3{let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),Input::default(),1./60.).unwrap();}}
            if let Some(target)=target {
                ui.focus(target);
                let input=Input {keys:vec![KeyPress {key:Key::Enter,mods:Mods::default()}],..Input::default()};
                let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),input,1./60.).unwrap();
                for _ in 0..3 {let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),Input::default(),1./60.).unwrap();}
            }
            let scene=ui.scene().unwrap();
            for id in ["tab-0","tab-1","tab-2","search","key-36","key-83"] {let r=scene.surface(id).unwrap().frame;assert!(r.x>=0. && r.y>=0. && r.x+r.size.width<=width as f64+1. && r.y+r.size.height<=height as f64+1.,"{id}: {r:?}");}
            if state=="instrument" {assert!(scene.surface("performance-group-0").is_none());assert!(scene.surface("preset-next").is_some());for id in ["performance-gain","performance-play"] {let r=scene.surface(id).unwrap().frame;assert!(r.y+r.size.height<height as f64-100.,"instrument control clipped: {id} {r:?}");}}
            if state=="groups" {assert!(scene.surface("groups-scroll").is_some());}
            if state=="details" {assert!(scene.surface("details-scroll").is_some());}
            if state=="settings" {assert!(scene.surface("root").is_some());}
            let mut ctx=RenderContext::new(width,height);let mut resources=Resources::default();
            vello::paint(&mut vello::Cpu {ctx:&mut ctx,resources:&mut resources,cache:&mut vello::Cache::default()},scene,vello::kurbo::Affine::IDENTITY).unwrap();
            ctx.flush();let mut pix=Pixmap::new(width,height);ctx.render(&mut pix,&mut resources);
            let rgba:Vec<u8>=pix.take_unpremultiplied().iter().flat_map(|p|[p.r,p.g,p.b,p.a]).collect();
            moose::core::screenshot::save_png(Path::new(&format!(".impeccable/review/{state}-{width}.png")),&rgba,width as u32,height as u32);
            ui.focus("key-60");
            let root=build(&mut ui,&mut bridge);ui.frame(root,Some(Size::new(width as f64,height as f64)),Input {keys:vec![KeyPress {key:Key::Enter,mods:Mods::default()}],..Input::default()},1./60.).unwrap();
            let _=build(&mut ui,&mut bridge);assert!(p.shared.audition.load(Ordering::Acquire));assert_eq!(p.shared.audition_note.load(Ordering::Relaxed),60);
          }
        }
    }
}
