//! Lua on its own thread, for hosts whose audio thread must never wait on a
//! script. The audio side sends note events stamped with the audio clock and
//! applies the commands that come back at the next block, so a script reacts
//! within a block or two. Offline renders use [`ScriptHost`] inline instead:
//! a thread cannot be deterministic against a clock that runs faster than real
//! time. Scan builds may explicitly opt into a seeded owner-thread barrier.

use super::{HostInput, Script};
mod diagnostics;
pub use diagnostics::{DiagnosticAcknowledgement, DiagnosticPhase, DiagnosticPoll, DiagnosticProgress,
    DiagnosticRequest, DiagnosticRequestError};
#[cfg(feature = "scan")]
use crate::script::diagnostics::{OwnerAudit, OwnerAuditSnapshot, OwnerCounter, OwnerPhase, OwnerStage, ScanProgress};
use crate::script::{
    Command, Config, FaultCategory, FaultCounts, Files, Finding, ScriptHost, UiState,
};
use sampler_ui_ir::{ControlId, Interface};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{Builder, JoinHandle},
    time::{Duration,Instant},
};

/// Events and commands in flight between the two sides.
const QUEUE: usize = 1024;
/// While a script waits for time, how often its thread looks at the clock.
const POLL: Duration = Duration::from_millis(1);

/// Diagnostic markers share command ordering, but never become audio commands.
enum Outgoing {
    Command(Command),
    Diagnostic(u64),
    Reset{sequence:u64,accepted:bool},
}

#[derive(Clone, Copy)]
enum Message {
    External(sampler_core::ExternalTurn),
    Reset{runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,sequence:u64},
    Control {
        generation: u64,
        id: ControlId,
        value: f64,
    },
    On {
        id: u64,
        key: u8,
        velocity: u8,
        channel: u8,
        at_ms: f64,
    },
    Off {
        id: u64,
        key: u8,
        velocity: u8,
        channel: u8,
        at_ms: f64,
    },
    In {
        input: HostInput,
        at_ms: f64,
    },
}

/// What loading the scripts found, for the host's report and interface.
pub struct Loaded {
    pub insert_overrides: Vec<(usize, String, String)>,
    /// Initialization writes folded into the program (`ScriptHost::folded_writes`).
    pub folded_writes: Vec<(usize, String, String)>,
    pub findings: Vec<Finding>,
    /// Preparation snapshot. Unavailable canonical fields are explicit; this
    /// copy is never a live-value ACK. Use ui snapshots/value after readback.
    pub interface: sampler_ui_ir::Interface,
    pub ui: Arc<UiBridge>,
}

/// The audio side of a script thread.
pub struct ScriptThread {
    #[cfg(feature = "scan")]
    scan: Arc<std::sync::Mutex<crate::script::ScanFaults>>,
    events: rtrb::Producer<Message>,
    ui: Arc<UiBridge>,
    commands: rtrb::Consumer<Outgoing>,
    reset_retirement:rtrb::Producer<Command>,
    reset_sequence:u64,
    pending_reset:Option<u64>,
    reset_retained:Option<Command>,
    /// The audio clock in milliseconds, as `f64` bits.
    clock: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    handles_notes: bool,
    authority:Option<crate::script::CoreAuthorityPort>,
    external_in_flight:Option<sampler_core::ExternalInputId>,
    external_terminal:Option<(sampler_core::ExternalInputId,u32)>,
    prepared_roots:Box<[crate::script::PreparedRootTask]>,
    root_requests:Box<[sampler_core::PreparedGlobalSourceTask]>,
    root_bindings:Option<Box<[Option<sampler_core::BehaviorId>]>>,
    root_setup:rtrb::Producer<RootSetup>,
    root_acks:rtrb::Consumer<RootSetupAck>,
    root_state:RootSetupState,
    time_ms: f64,
    #[cfg(feature = "scan")]
    audit: bool,
    #[cfg(feature = "scan")]
    audit_due: Option<f64>,
    #[cfg(feature = "scan")]
    audit_pending: Vec<Command>,
}

struct RootSetup {runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,bindings:Box<[Option<sampler_core::BehaviorId>]>}
struct RootSetupAck {setup:RootSetup,result:Result<(),sampler_core::Error>}
#[derive(Clone,Copy,Debug)]
enum RootSetupState {Idle,Pending{runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,frame:u64},Ready{runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,frame:u64},Failed}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum PreparedRootProgress {Pending,Ready{tasks:usize}}

enum UiRequest {
    Diagnostic(diagnostics::Pending),
    SampleStatus(mpsc::SyncSender<crate::script::SampleLoadStatus>),
    BindAuthority(crate::script::OwnerAuthorityPort,mpsc::SyncSender<Result<(),String>>),
    BindSourceProperties(crate::script::OwnerSourcePropertyPort,mpsc::SyncSender<Result<(),String>>),
    RefreshSourcePropertyPresentation,
    PreparedRoots(mpsc::SyncSender<Result<Vec<crate::script::PreparedRootTask>,String>>),
    BindPreparedRoots {runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,bindings:Vec<crate::script::PreparedRootBinding>,reply:mpsc::SyncSender<Result<(),String>>},
    SampleControl(sampler_core::SampleControl, mpsc::SyncSender<Result<(),String>>),
    SampleStream(sampler_kontakt::DynamicSources, mpsc::SyncSender<Result<(),String>>),
    Edit { generation:u64, id:ControlId, value:f64, drawn:bool },
    Activate {generation:u64,id:ControlId,activation:sampler_ui_ir::UiActivation},
    Save(mpsc::SyncSender<Result<UiState, String>>),
    #[cfg(feature = "scan")]
    Audit(f64, mpsc::SyncSender<(Vec<Command>, Option<f64>)>),
}

#[cfg(feature = "scan")]
type AuditReply = (
    mpsc::SyncSender<(Vec<Command>, Option<f64>)>,
    Vec<Command>,
    Option<f64>,
    f64,
);

fn process_events(host: &mut ScriptHost, incoming: &mut rtrb::Consumer<Message>, ui: &UiBridge) {
    while let Ok(message) = incoming.pop() {
        #[cfg(feature = "scan")]
        host.owner_count(OwnerCounter::Events, 1);
        if host.requires_external_authority()&&matches!(&message,Message::On{..}|Message::Off{..}|Message::In{..}){
            host.external_fault("legacy event delivery to staged canonical owner refused");continue;
        }
        match message {
            Message::Reset{runtime,plan,sequence}=>{
                match host.reset_execution_barrier(runtime,plan){
                    Ok(())=>ui.reset_ready.store(sequence,Ordering::Release),
                    Err(_)=>ui.reset_refused.store(sequence,Ordering::Release),
                }
                // A reset marker is a real FIFO boundary; never collapse two
                // resets or run later events before its command-order ACK.
                break;
            },
            Message::External(turn)=>{if let Err(error)=host.deliver_external_turn(turn){host.external_fault(&error);}},
            Message::Control { generation, id, value } => {
                ui.apply_control(host, generation, id, value, false, DiagnosticPhase::Events);
            }
            Message::On {
                id,
                key,
                velocity,
                channel,
                at_ms,
            } => {
                host.set_time(at_ms);
                host.note_on(id, key, velocity, channel);
            }
            Message::Off { id, key, velocity, channel, at_ms } => {
                host.set_time(at_ms);
                host.note_off(id, key, velocity, channel);
            }
            Message::In { input, at_ms } => {
                host.set_time(at_ms);
                Script::input(host, input);
            }
        }
        if let Some(progress) = ui.diagnostics.get() {
            progress.executed.fetch_add(1, Ordering::Release);
        }
    }
}

fn process_ui_requests(
    host: &mut ScriptHost,
    requests: &mpsc::Receiver<UiRequest>,
    _incoming: &mut rtrb::Consumer<Message>,
    ui: &UiBridge,
    diagnostic: &mut Option<diagnostics::Pending>,
    allow_diagnostic: bool,
    #[cfg(feature = "scan")] audit_reply: &mut Option<AuditReply>,
) {
    while let Ok(request) = requests.try_recv() {
        #[cfg(feature = "scan")]
        host.owner_count(OwnerCounter::UiRequests, 1);
        match request {
            UiRequest::Diagnostic(mut request) => {
                if request.abandoned() { continue; }
                // During command backpressure, defer until a fresh turn has
                // advanced and published the work generated by these events.
                if allow_diagnostic {
                    ui.diagnostic_phase(DiagnosticPhase::Events);
                    process_events(host, _incoming, ui);
                }
                request.eligible = allow_diagnostic;
                *diagnostic = Some(request);
                break;
            }
            UiRequest::BindSourceProperties(owner,reply)=>{
                let result=host.bind_source_property_owner(owner);let bound=result.is_ok();
                // Metadata ACK must precede readback: the caller can now retain
                // and service its Core endpoint. No bind/readback deadlock.
                let _=reply.send(result);
                if bound {ui.publish_source_property_presentation(host);}
            }
            UiRequest::RefreshSourcePropertyPresentation=>{ui.publish_source_property_presentation(host);}
            UiRequest::BindAuthority(owner,reply)=>{let _=reply.send(host.bind_owner_authority(owner));}
            UiRequest::PreparedRoots(reply)=>{let _=reply.send(host.prepared_root_tasks());}
            UiRequest::BindPreparedRoots{runtime,plan,bindings,reply}=>{let _=reply.send(host.bind_prepared_roots(runtime,plan,bindings));}
            UiRequest::SampleStatus(reply)=>{let _=reply.send(ScriptHost::sample_load_status(host));}
            UiRequest::SampleControl(control,reply)=>{let _=reply.send(host.bind_sample_control(control));}
            UiRequest::SampleStream(sources,reply)=>{let _=reply.send(host.bind_sample_stream(sources));}
            UiRequest::Edit{generation,id,value,drawn} => {
                ui.apply_control(host,generation,id,value,drawn,DiagnosticPhase::UiRequests);
            }
            UiRequest::Activate{generation,id,activation}=>{
                ui.apply_activation(host,generation,id,activation,DiagnosticPhase::UiRequests);
            }
            UiRequest::Save(reply) => {
                let _ = reply.send(host.save_ui_state());
            }
            #[cfg(feature = "scan")]
            UiRequest::Audit(ms, reply) => {
                // Events were published before this request; drain again to close the queue race.
                host.owner_phase(OwnerPhase::Events);
                let audit = host.owner_audit();
                {
                    let _span = audit.as_ref().map(|a|a.span(OwnerStage::AuditBarrierEvents));
                    process_events(host, _incoming, ui);
                }
                host.owner_phase(OwnerPhase::Advance);
                {
                    let _span = audit.as_ref().map(|a|a.span(OwnerStage::AuditBarrierAdvance));
                    host.advance(ms);
                }
                *audit_reply = Some((reply, host.take_commands(), host.next_due(), ms));
                // Complete this barrier after publication, before later UI requests.
                break;
            }
        }
    }
}

/// A whole already-generated safe batch may reach audio before UI conversion.
/// Never wait here: a full/partly free queue must not prevent UI publication.
fn send_ready_commands(
    host: &mut ScriptHost,
    outgoing: &mut rtrb::Producer<Outgoing>,
    ui: &UiBridge,
    backlog: &mut Vec<Command>,
) {
    if !backlog.is_empty() {
        return;
    }
    let batch = {
        #[cfg(feature = "scan")]
        let _span = host.owner_span(OwnerStage::ReadyCommandsCollect);
        host.take_ready_command_batch(outgoing.slots().min(QUEUE))
    };
    if batch.is_empty() {
        return;
    }
    if let Some(progress) = ui.diagnostics.get() {
        progress.collected.fetch_add(batch.len() as u64, Ordering::Release);
    }
    #[cfg(feature = "scan")]
    {
        host.owner_count(OwnerCounter::CommandsCollected, batch.len());
        host.owner_count(OwnerCounter::ReadyCommandsCollected, batch.len());
        if let Some(audit) = host.owner_audit() {
            audit.max(OwnerCounter::BacklogHighWater, batch.len());
        }
    }
    ui.diagnostic_phase(DiagnosticPhase::Commands);
    #[cfg(feature = "scan")]
    let _span = host.owner_span(OwnerStage::ReadyCommandsPush);
    let count = batch.len();
    match outgoing.write_chunk_uninit(count) {
        Ok(chunk) => {
            chunk.fill_from_iter(batch.into_iter().map(Outgoing::Command));
            if let Some(progress) = ui.diagnostics.get() {
                progress.pushed.fetch_add(count as u64, Ordering::Release);
            }
            #[cfg(feature = "scan")]
            {
                host.owner_count(OwnerCounter::CommandsPushed, count);
                host.owner_count(OwnerCounter::ReadyCommandsPushed, count);
            }
        }
        Err(_) => {
            // Preserve every command in FIFO order if admission ever changes.
            backlog.extend(batch);
        }
    }
}

impl ScriptThread {
    /// Load the scripts of program `xml` on a new thread; returns once they
    /// have loaded (their `onInit` has run).
    pub fn spawn(
        xml: String,
        files: impl Files + Send + 'static,
        config: Config,
    ) -> Result<(Self, Loaded), String> {
        Self::spawn_with_ui_state(xml, files, config, None)
    }

    pub fn spawn_with_ui_state(
        xml: String,
        files: impl Files + Send + 'static,
        config: Config,
        state: Option<UiState>,
    ) -> Result<(Self, Loaded), String> {
        Self::spawn_shared_with_ui_state(xml.into(), files, config, state)
    }

    pub fn spawn_shared_with_ui_state(
        xml: Arc<str>,
        files: impl Files + Send + 'static,
        config: Config,
        state: Option<UiState>,
    ) -> Result<(Self, Loaded), String> {
        Self::spawn_owner(xml, files, config, state,
            #[cfg(feature = "scan")]
            OwnerAudit::from_env()?,
        )
    }

    fn spawn_owner(
        xml: Arc<str>, files: impl Files + Send + 'static, config: Config, state: Option<UiState>,
        #[cfg(feature = "scan")] owner_audit: Option<Arc<OwnerAudit>>,
    ) -> Result<(Self, Loaded), String> {
        Self::spawn_owner_budget(xml,files,config,state,UiRegistryBudget::default(),UiSchemaWorkBudget::default(),false,
            #[cfg(feature="scan")] owner_audit,
        )
    }
    /// Explicit product registry admission profile; all cells are prepared here.
    pub fn spawn_with_ui_registry_budget(
        xml:String, files:impl Files+Send+'static, config:Config, state:Option<UiState>, budget:UiRegistryBudget,
    )->Result<(Self,Loaded),String>{
        Self::spawn_owner_budget(xml.into(),files,config,state,budget,UiSchemaWorkBudget::default(),false,
            #[cfg(feature="scan")] OwnerAudit::from_env()?,
        )
    }
    /// Explicit independent schema-inspection and metadata payload profiles.
    pub fn spawn_with_ui_registry_budgets(
        xml:String,files:impl Files+Send+'static,config:Config,state:Option<UiState>,
        budget:UiRegistryBudget,work_budget:UiSchemaWorkBudget,
    )->Result<(Self,Loaded),String>{
        Self::spawn_owner_budget(xml.into(),files,config,state,budget,work_budget,false,
            #[cfg(feature="scan")] OwnerAudit::from_env()?,
        )
    }
    /// Staged owner: main/restore/onLoad run offaudio, while Init and every
    /// waited segment need real Core tasks at the actual installation frame.
    pub fn spawn_staged_with_ui_registry_budgets(
        xml:Arc<str>,files:impl Files+Send+'static,config:Config,state:Option<UiState>,
        budget:UiRegistryBudget,work_budget:UiSchemaWorkBudget,
    )->Result<(Self,Loaded),String>{
        Self::spawn_owner_budget(xml,files,config,state,budget,work_budget,true,
            #[cfg(feature="scan")] OwnerAudit::from_env()?,
        )
    }
    pub fn spawn_staged_with_ui_state(
        xml:Arc<str>,files:impl Files+Send+'static,config:Config,state:Option<UiState>,
    )->Result<(Self,Loaded),String>{
        Self::spawn_staged_with_ui_registry_budgets(xml,files,config,state,UiRegistryBudget::default(),UiSchemaWorkBudget::default())
    }
    fn spawn_owner_budget(
        xml:Arc<str>,files:impl Files+Send+'static,config:Config,state:Option<UiState>,budget:UiRegistryBudget,work_budget:UiSchemaWorkBudget,staged:bool,
        #[cfg(feature="scan")] owner_audit:Option<Arc<OwnerAudit>>,
    )->Result<(Self,Loaded),String>{
        #[cfg(feature = "scan")]
        let audit = config.audit_seed.is_some();
        for layout in [
            std::alloc::Layout::array::<Message>(QUEUE),
            std::alloc::Layout::array::<Outgoing>(QUEUE),
            std::alloc::Layout::array::<Command>(QUEUE),
            std::alloc::Layout::array::<RootSetup>(1),
            std::alloc::Layout::array::<RootSetupAck>(1),
        ]{layout.map_err(|_|"script owner queue payload layout overflow")?;}
        let (ui_send, ui_receive) = mpsc::sync_channel(256);
        let (events, mut incoming) = rtrb::RingBuffer::new(QUEUE);
        let (mut outgoing, commands) = rtrb::RingBuffer::new(QUEUE);
        let (reset_retirement,mut owner_retirement)=rtrb::RingBuffer::new(QUEUE);
        let (root_setup,mut owner_roots)=rtrb::RingBuffer::<RootSetup>::new(1);
        let (mut owner_root_acks,root_acks)=rtrb::RingBuffer::<RootSetupAck>::new(1);
        let clock = Arc::new(AtomicU64::new(0f64.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, loaded) = mpsc::channel();
        #[cfg(feature = "scan")]
        let scan = Arc::new(std::sync::Mutex::new(crate::script::ScanFaults::default()));
        let thread = Builder::new()
            .name("uvi-script".into())
            .spawn({
                let (clock, stop) = (clock.clone(), stop.clone());
                #[cfg(feature = "scan")]
                let scan = scan.clone();
                move || {
                    #[cfg(feature = "scan")]
                    let load_span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Loading));
                    let mut host =
                        match if staged {ScriptHost::new_staged_with_ui_state(&xml,files,config,state.as_ref())}
                        else {ScriptHost::new_with_ui_state(&xml, files, config, state.as_ref())} {
                            Ok(host) => host,
                            Err(e) => {
                                #[cfg(feature = "scan")]
                                crate::script::scan_failed_load(&e);
                                return drop(ready.send(Err(e)));
                            }
                        };
                    #[cfg(feature = "scan")]
                    {
                        host.install_owner_audit(owner_audit.clone());
                        drop(load_span);
                    }
                    let handles = host.handles_notes();
                    let ui = match UiBridge::new(&host,ui_send,std::thread::current(),stop.clone(),budget,work_budget,
                        #[cfg(feature="scan")] audit,
                    ) { Ok(ui)=>Arc::new(ui),Err(error)=>return drop(ready.send(Err(error))) };
                    #[cfg(feature = "scan")]
                    {
                        *scan.lock().unwrap() = host.scan_faults();
                    }
                    let prepared_roots=if staged {
                        match host.prepared_root_tasks(){
                            Ok(roots)=>{
                                let valid=roots.len()<=4096
                                    &&std::alloc::Layout::array::<crate::script::PreparedRootTask>(roots.len()).is_ok()
                                    &&std::alloc::Layout::array::<sampler_core::PreparedGlobalSourceTask>(roots.len()).is_ok()
                                    &&std::alloc::Layout::array::<Option<sampler_core::BehaviorId>>(roots.len()).is_ok();
                                if !valid{return drop(ready.send(Err("prepared root payload layout/capacity".into())));}
                                roots.into_boxed_slice()
                            },Err(error)=>return drop(ready.send(Err(error)))
                        }
                    }else{Box::new([])};
                    let report = Loaded {
                        insert_overrides: host.insert_overrides(),
                        folded_writes: host.folded_writes(),
                        findings: host.findings(),
                        interface: ui.interface().as_ref().clone(),
                        ui: ui.clone(),
                    };
                    let _ = ready.send(Ok((handles, report, prepared_roots)));
                    drop(ready);
                    let mut backlog: Vec<Command> = Vec::new();
                    let mut revision = host.ui_revision();
                    let mut finding_revision = host.finding_revision();
                    let mut diagnostic: Option<diagnostics::Pending> = None;
                    while !stop.load(Ordering::Acquire) {
                        if owner_root_acks.slots()!=0&&let Ok(setup)=owner_roots.pop(){
                            let result=(||{
                                let mut bindings=Vec::new();bindings.try_reserve_exact(setup.bindings.len()).map_err(|_|sampler_core::Error::Capacity)?;
                                for (row,behavior) in setup.bindings.iter().enumerate(){bindings.push(crate::script::PreparedRootBinding{row,behavior:behavior.ok_or(sampler_core::Error::InvalidInput)?});}
                                host.bind_prepared_roots(setup.runtime,setup.plan,bindings).map_err(|_|sampler_core::Error::InvalidInput)
                            })();
                            // Sole producer after reservation; payload retirement
                            // stays on the owner if the invariant is violated.
                            if owner_root_acks.push(RootSetupAck{setup,result}).is_err(){host.external_fault("root setup reply reservation violated");break;}
                        }


                        while let Ok(command)=owner_retirement.pop(){drop(command);}
                        if let Some(request) = &mut diagnostic {
                            request.eligible = true;
                            if request.abandoned() { diagnostic = None; }
                        }
                        ui.diagnostic_phase(DiagnosticPhase::Events);
                        #[cfg(feature = "scan")]
                        host.owner_count(OwnerCounter::Turns, 1);
                        #[cfg(feature = "scan")]
                        let mut audit_reply = None;
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::Events);
                        {
                            #[cfg(feature = "scan")]
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Events));
                            process_events(&mut host, &mut incoming, &ui);
                        }
                        let ready=ui.reset_ready.swap(0,Ordering::AcqRel);
                        let refused=ui.reset_refused.swap(0,Ordering::AcqRel);
                        if ready!=0||refused!=0{
                            // Old legacy payloads retire on their real Lua owner.
                            if ready!=0{backlog.clear();}
                            let mut marker=Outgoing::Reset{sequence:ready.max(refused),accepted:ready!=0};
                            loop{
                                match outgoing.push(marker){Ok(())=>break,Err(rtrb::PushError::Full(back))=>marker=back}
                                while let Ok(command)=owner_retirement.pop(){drop(command);}
                                if stop.load(Ordering::Acquire){return;}
                                std::thread::sleep(POLL);
                            }
                            continue;
                        }
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::UiRequests);
                        ui.diagnostic_phase(DiagnosticPhase::UiRequests);
                        {
                        #[cfg(feature = "scan")]
                        let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::UiRequests));
                        process_ui_requests(
                            &mut host,
                            &ui_receive,
                            &mut incoming,
                            &ui,
                            &mut diagnostic,
                            true,
                            #[cfg(feature = "scan")]
                            &mut audit_reply,
                        );
                        }
                        if ui.reset_ready.load(Ordering::Acquire)!=0||ui.reset_refused.load(Ordering::Acquire)!=0{continue;}
                        ui.diagnostic_phase(DiagnosticPhase::Advance);
                        #[cfg(feature = "scan")]
                        if !audit {
                            host.owner_phase(OwnerPhase::Advance);
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Advance));
                            host.advance(f64::from_bits(clock.load(Ordering::Acquire)));
                        }
                        #[cfg(not(feature = "scan"))]
                        host.advance(f64::from_bits(clock.load(Ordering::Acquire)));
                        #[cfg(feature = "scan")]
                        if !audit {
                            send_ready_commands(&mut host, &mut outgoing, &ui, &mut backlog);
                        }
                        #[cfg(not(feature = "scan"))]
                        send_ready_commands(&mut host, &mut outgoing, &ui, &mut backlog);
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::Publish);
                        let current = host.ui_revision();
                        if current != revision {
                            ui.diagnostic_phase(DiagnosticPhase::UiExport);
                            #[cfg(feature = "scan")]
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::UiPublish));
                            ui.publish(&host);
                            revision = current;
                        }
                        if host.finding_revision() != finding_revision {
                            ui.diagnostic_phase(DiagnosticPhase::Findings);
                            #[cfg(feature = "scan")]
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::FindingsPublish));
                            ui.publish_findings(&host);
                            finding_revision = host.finding_revision();
                        }
                        #[cfg(feature = "scan")]
                        {
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::ScanSnapshot));
                            *scan.lock().unwrap() = host.scan_faults();
                        }
                        #[cfg(feature = "scan")]
                        if let Some((reply, commands, due, ms)) = audit_reply.take() {
                            if let Some(progress) = ui.scan_progress() {
                                progress.complete(ms);
                            }
                            let _ = reply.send((commands, due));
                        }
                        #[cfg(feature = "scan")]
                        if audit {
                            host.owner_phase(OwnerPhase::Parked);
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Park));
                            std::thread::park();
                            continue;
                        }
                        {
                            #[cfg(feature = "scan")]
                            let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::CommandsCollect));
                            let collected = host.take_commands();
                            if let Some(progress) = ui.diagnostics.get() {
                                progress.collected.fetch_add(collected.len() as u64, Ordering::Release);
                            }
                            #[cfg(feature = "scan")]
                            host.owner_count(OwnerCounter::CommandsCollected, collected.len());
                            backlog.extend(collected);
                            #[cfg(feature = "scan")]
                            if let Some(audit) = &owner_audit { audit.max(OwnerCounter::BacklogHighWater, backlog.len()); }
                        }
                        {
                        ui.diagnostic_phase(DiagnosticPhase::Commands);
                        #[cfg(feature = "scan")]
                        let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::CommandsPush));
                        for command in backlog.drain(..) {
                            // A full queue only delays: the audio side drains it every block.
                            let mut command = Outgoing::Command(command);
                            loop {
                                match outgoing.push(command) {
                                    Ok(()) => {
                                        if let Some(progress) = ui.diagnostics.get() {
                                            progress.pushed.fetch_add(1, Ordering::Release);
                                        }
                                        #[cfg(feature = "scan")]
                                        host.owner_count(OwnerCounter::CommandsPushed, 1);
                                        break;
                                    },
                                    Err(rtrb::PushError::Full(back)) => {
                                        #[cfg(feature = "scan")]
                                        let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Backpressure));
                                        #[cfg(feature = "scan")]
                                        host.owner_count(OwnerCounter::QueueFullRetries, 1);
                                        command = back;
                                        if stop.load(Ordering::Acquire) {
                                            return;
                                        }
                                        {
                                        #[cfg(feature = "scan")]
                                        let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::BackpressureUiRequests));
                                        process_ui_requests(
                                            &mut host,
                                            &ui_receive,
                                            &mut incoming,
                                            &ui,
                                            &mut diagnostic,
                                            false,
                                            #[cfg(feature = "scan")]
                                            &mut audit_reply,
                                        );
                                        }
                                        std::thread::sleep(POLL);
                                    }
                                }
                            }
                        }
                        }
                        if diagnostic.as_ref().is_some_and(|request| request.eligible) {
                            let request = diagnostic.take().unwrap();
                            // Only a consumed ordered marker proves that the
                            // core-side drain passed all preceding commands.
                            let mut marker = Outgoing::Diagnostic(request.id());
                            while !request.abandoned() && !stop.load(Ordering::Acquire) {
                                match outgoing.push(marker) {
                                    Ok(()) => { request.complete(&host, &ui); break; }
                                    Err(rtrb::PushError::Full(back)) => {
                                        marker = back;
                                        std::thread::sleep(POLL);
                                    }
                                }
                            }
                        }
                        ui.diagnostic_phase(DiagnosticPhase::Parked);
                        // A request consumed during command backpressure needs
                        // a fresh normal turn even when no Lua timer is due.
                        if diagnostic.is_some() { continue; }
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::Parked);
                        #[cfg(feature = "scan")]
                        let _span = owner_audit.as_ref().map(|a|a.span(OwnerStage::Park));
                        if host.next_due().is_some() {
                            std::thread::park_timeout(POLL);
                        } else if let Some(timer)=host.sample_collection_timer() {
                            std::thread::park_timeout(timer);
                        } else {
                            std::thread::park();
                        }
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        let (handles_notes, report, prepared_roots) = loaded
            .recv()
            .map_err(|_| "the script thread stopped while loading".to_string())??;
        Ok((
            Self {
                #[cfg(feature = "scan")]
                scan,
                events,
                ui: report.ui.clone(),
                commands,reset_retirement,reset_sequence:0,pending_reset:None,reset_retained:None,
                clock,
                stop,
                thread: Some(thread),
                handles_notes,
                root_requests:prepared_roots.iter().map(|root|sampler_core::PreparedGlobalSourceTask{processor:root.processor,family:root.family,origin:root.origin}).collect::<Vec<_>>().into_boxed_slice(),
                root_bindings:Some(vec![None;prepared_roots.len()].into_boxed_slice()),prepared_roots,
                root_setup,root_acks,root_state:RootSetupState::Idle,
                authority:None,external_in_flight:None,external_terminal:None,
                time_ms: 0.0,
                #[cfg(feature = "scan")]
                audit,
                #[cfg(feature = "scan")]
                audit_due: None,
                #[cfg(feature = "scan")]
                audit_pending: Vec::new(),
            },
            report,
        ))
    }

    /// Offaudio before Part publication: bind both endpoints to the actual
    /// Runtime/Plan table and wait for the genuine Lua owner shape ACK.
    pub fn bind_external_authority(&mut self,runtime:&sampler_core::Runtime,plan:sampler_core::PlanId,timeout:Duration)->Result<(),String>{
        if self.authority.is_some(){return Err("external authority already bound".into());}
        let (core,owner)=crate::script::authority_pair(runtime,plan,timeout).map_err(|error|format!("external authority preparation: {error:?}"))?;
        let (send,reply)=mpsc::sync_channel(1);
        self.ui.edits.try_send(UiRequest::BindAuthority(owner,send)).map_err(|_|"external owner binding unavailable/capacity".to_string())?;
        self.ui.owner.unpark();reply.recv_timeout(timeout).map_err(|_|"external owner binding deadline".to_string())??;
        self.authority=Some(core);Ok(())
    }
    /// Active candidate setup is nonblocking. All ring/binding storage was
    /// prepared by construction. Queue success is never the Lua-owner ACK.
    pub fn start_prepared_root_adoption(&mut self,runtime:&mut sampler_core::Runtime,plan:sampler_core::PlanId,performance:sampler_core::PerformanceId)->Result<PreparedRootProgress,sampler_core::Error>{
        use sampler_core::Error;
        if !self.external_authority_bound(runtime.id(),plan){return Err(Error::Canceled);}
        if self.pending_reset.is_some()||self.reset_retained.is_some(){return Err(Error::Busy);}
        match self.root_state{
            RootSetupState::Idle=>{},
            RootSetupState::Pending{..}=>return self.poll_prepared_root_adoption(runtime,plan),
            RootSetupState::Ready{runtime:actual,plan:actual_plan,frame}=>return if actual==runtime.id()&&actual_plan==plan&&frame==runtime.now(){Ok(PreparedRootProgress::Ready{tasks:self.prepared_roots.len()})}else{Err(Error::StaleHandle)},
            RootSetupState::Failed=>return Err(Error::Canceled),
        }
        if self.root_setup.slots()==0{return Err(Error::Capacity);}
        let frame=runtime.now();let bindings=self.root_bindings.as_mut().ok_or(Error::InvalidInput)?;
        if bindings.len()!=self.prepared_roots.len()||bindings.iter().any(Option::is_some){return Err(Error::InvalidInput);}
        for (index,root) in self.prepared_roots.iter().enumerate(){
            if root.row!=index||runtime.executable_source_binding(plan,root.processor,sampler_core::SourceEventFamily::Task(root.family))?.is_none(){return Err(Error::InvalidInput);}
        }
        // One real aggregate Core transaction validates the complete request
        // roster and writes ALL output handles, or changes none. In particular
        // repeated processor/family rows are distinct genuine Lua coroutines.
        let admitted=runtime.start_prepared_global_source_tasks(plan,performance,&self.root_requests,bindings)?;
        if admitted!=self.prepared_roots.len()||bindings.iter().any(Option::is_none){
            let _=runtime.abort_external_execution();self.cancel_external_authority();self.root_state=RootSetupState::Failed;return Err(Error::InvalidInput);
        }
        let setup=RootSetup{runtime:runtime.id(),plan,bindings:self.root_bindings.take().ok_or(Error::InvalidInput)?};
        if let Err(rtrb::PushError::Full(setup))=self.root_setup.push(setup){
            self.root_bindings=Some(setup.bindings);let _=runtime.abort_external_execution();self.cancel_external_authority();self.root_state=RootSetupState::Failed;return Err(Error::Capacity);
        }
        self.root_state=RootSetupState::Pending{runtime:runtime.id(),plan,frame};self.wake();Ok(PreparedRootProgress::Pending)
    }
    pub fn poll_prepared_root_adoption(&mut self,runtime:&mut sampler_core::Runtime,plan:sampler_core::PlanId)->Result<PreparedRootProgress,sampler_core::Error>{
        use sampler_core::Error;
        let (actual,actual_plan,frame)=match self.root_state{
            RootSetupState::Pending{runtime,plan,frame}=>(runtime,plan,frame),
            RootSetupState::Ready{runtime:actual,plan:actual_plan,frame}=>return if actual==runtime.id()&&actual_plan==plan&&frame==runtime.now(){Ok(PreparedRootProgress::Ready{tasks:self.prepared_roots.len()})}else{Err(Error::StaleHandle)},
            _=>return Err(Error::InvalidInput),
        };
        if actual!=runtime.id()||actual_plan!=plan||frame!=runtime.now()||!self.external_authority_bound(actual,plan){return Err(Error::StaleHandle);}
        let Ok(ack)=self.root_acks.pop()else{return Ok(PreparedRootProgress::Pending);};
        let valid=ack.setup.runtime==actual&&ack.setup.plan==plan&&ack.setup.bindings.len()==self.prepared_roots.len();
        self.root_bindings=Some(ack.setup.bindings);
        if !valid||ack.result.is_err(){let _=runtime.abort_external_execution();self.cancel_external_authority();self.root_state=RootSetupState::Failed;return Err(if valid{ack.result.unwrap_err()}else{Error::StaleHandle});}
        self.root_state=RootSetupState::Ready{runtime:actual,plan,frame};Ok(PreparedRootProgress::Ready{tasks:self.prepared_roots.len()})
    }

    /// Actual copied requests from the existing Lua owner. The Core actor must
    /// reserve a result slot and validate actual turn/plan before mutation.
    /// A copied real Core turn is queued once. A true result is transport
    /// admission only; only the later Complete/Cancel Core reply is acceptance.
    pub fn external_turn_slots(&self)->usize{if self.external_cancelled(){0}else{self.events.slots()}}
    pub fn deliver_external_turn(&mut self,turn:sampler_core::ExternalTurn)->Result<(),sampler_core::Error>{
        if self.authority.as_ref().is_none_or(|port|port.plan()!=turn.plan||port.cancelled()){return Err(sampler_core::Error::Canceled);}
        if self.external_in_flight.is_some(){return Err(sampler_core::Error::Busy);}
        self.events.push(Message::External(turn)).map_err(|_|sampler_core::Error::Capacity)?;
        self.external_in_flight=Some(turn.input);self.wake();Ok(())
    }
    /// Endpoint identity only. Every command must separately query Core's
    /// actual Running turn; this predicate does not authorize a mutation.
    pub fn external_authority_bound(&self,runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId)->bool{
        self.authority.as_ref().is_some_and(|port|port.runtime()==runtime&&port.plan()==plan&&!port.cancelled())
    }
    pub fn cancel_external_authority(&self){if let Some(port)=&self.authority{port.cancel();}}
    pub fn external_reply_slots(&self)->usize{self.authority.as_ref().map_or(0,|port|port.reply_slots())}
    pub fn pop_external_request(&mut self)->Option<crate::script::AuthorityRequest>{
        let request=self.authority.as_mut()?.pop()?;
        if matches!(&request.operation,crate::script::AuthorityOperation::Complete{..}|crate::script::AuthorityOperation::Cancel){self.external_terminal=Some((request.turn.input,request.ordinal));}
        Some(request)
    }
    pub fn reply_external_request(&mut self,reply:crate::script::AuthorityReply)->Result<(),crate::script::AuthorityReply>{
        let terminal=self.external_terminal==Some((reply.input,reply.ordinal))&&matches!(&reply.result,Ok(crate::script::AuthorityValue::Completed)|Err(sampler_core::Error::Canceled|sampler_core::Error::StaleHandle));
        let input=reply.input;
        match self.authority.as_mut(){Some(port)=>{port.reply(reply)?;if terminal&&self.external_in_flight==Some(input){self.external_in_flight=None;self.external_terminal=None;}Ok(())},None=>Err(reply)}
    }
    pub fn external_cancelled(&self)->bool{self.authority.as_ref().is_some_and(|port|port.cancelled())}

    /// The sole Core worker must first quiesce the previous segment through its
    /// actual terminal reply (or irreversibly abort the endpoint). Reset refuses
    /// while any delivery is queued/executing; it cannot roll back Lua globals.
    /// Only after this pure availability check may the worker cancel Core and
    /// enqueue the ordered owner marker. An aborted endpoint never rearms here.
    /// The sole Core worker cancels real continuations before this marker.
    /// Capacity is pure; one marker at a time prevents coalesced reset ACKs.
    pub fn reset_execution_available(&self)->bool{
        self.pending_reset.is_none()&&self.reset_retained.is_none()&&self.external_in_flight.is_none()
            &&!matches!(self.root_state,RootSetupState::Pending{..})
            &&self.events.slots()!=0&&!self.external_cancelled()
    }
    pub fn reset_execution(&mut self,runtime:sampler_core::RuntimeId,plan:sampler_core::PlanId,sequence:u64)->Result<(),sampler_core::Error>{
        use sampler_core::Error;
        #[cfg(feature="scan")]
        if self.audit{return Err(Error::InvalidInput);}
        if !self.external_authority_bound(runtime,plan){return Err(Error::StaleHandle);}
        if sequence==0||sequence<=self.reset_sequence{return Err(Error::InvalidInput);}
        if !self.reset_execution_available(){return Err(Error::Capacity);}
        self.events.push(Message::Reset{runtime,plan,sequence}).map_err(|_|Error::Capacity)?;
        self.reset_sequence=sequence;self.pending_reset=Some(sequence);self.wake();Ok(())
    }
    /// A queued marker is not ACK. Old owned Command payloads transfer back to
    /// the Lua owner; the Core worker never drops them on this path.
    pub fn reset_execution_ack(&mut self,sequence:u64)->Result<bool,sampler_core::Error>{
        use sampler_core::Error;
        if self.pending_reset!=Some(sequence){return Err(Error::StaleHandle);}
        if self.reset_retained.is_some(){return Err(Error::Canceled);}
        if self.ui.reset_failed.load(Ordering::Acquire)==sequence{self.pending_reset=None;return Err(Error::Canceled);}
        if self.ui.reset_ack.load(Ordering::Acquire)==sequence{self.pending_reset=None;return Ok(true);}
        for _ in 0..QUEUE{
            if self.reset_retirement.slots()==0{self.wake();return Ok(false);}
            let Ok(row)=self.commands.pop()else{self.wake();return Ok(false);};
            match row{
                Outgoing::Command(command)=>{
                    // Sole producer, reserved above; retain payload on this
                    // worker if this queue invariant is violated.
                    if let Err(rtrb::PushError::Full(command))=self.reset_retirement.push(command){
                        self.reset_retained=Some(command);self.cancel_external_authority();return Err(Error::Capacity);
                    }
                    if let Some(progress)=self.ui.diagnostics.get(){progress.drained.fetch_add(1,Ordering::Release);}
                },
                Outgoing::Diagnostic(id)=>{if let Some(progress)=self.ui.diagnostics.get(){progress.marker_drained.store(id,Ordering::Release);}},
                Outgoing::Reset{sequence:actual,accepted}=>{
                    if actual!=sequence{self.cancel_external_authority();return Err(Error::StaleHandle);}
                    self.pending_reset=None;
                    if accepted{self.ui.reset_ack.store(actual,Ordering::Release);return Ok(true);}
                    self.ui.reset_failed.store(actual,Ordering::Release);return Err(Error::Canceled);
                },
            }
        }
        self.wake();Ok(false)
    }

    pub fn ui(&self) -> &Arc<UiBridge> {
        &self.ui
    }

    pub fn set_control(&mut self, id: ControlId, value: f64) -> bool {
        if !value.is_finite() { return false; }
        let Some((generation,_))=self.ui.read_control(id)else{return false;};
        let ok = self.events.push(Message::Control { generation, id, value }).is_ok();
        self.count_enqueue(ok);
        if ok {
            self.wake();
        }
        ok
    }
    #[cfg(feature = "scan")]
    pub fn scan_faults(&self) -> crate::script::ScanFaults {
        self.scan.lock().unwrap().clone()
    }

    #[cfg(feature = "scan")]
    fn synchronize_audit(&mut self, out: &mut Vec<Command>) {
        let (reply, received) = mpsc::sync_channel(1);
        if let Some(progress) = self.ui.scan_progress() {
            progress.request(self.time_ms);
        }
        self.ui
            .edits
            .send(UiRequest::Audit(self.time_ms, reply))
            .expect("audit script owner stopped");
        self.wake();
        let (commands, due) = received.recv().expect("audit script owner stopped");
        out.extend(commands);
        self.audit_due = due;
    }

    fn enqueue(&mut self, message: Message) {
        #[cfg(feature = "scan")]
        {
            let mut message = message;
            loop {
                match self.events.push(message) {
                    Ok(()) => {
                        self.count_enqueue(true);
                        self.wake();
                        return;
                    }
                    Err(rtrb::PushError::Full(back)) => {
                        message = back;
                        if self.audit {
                            let mut commands = std::mem::take(&mut self.audit_pending);
                            self.synchronize_audit(&mut commands);
                            self.audit_pending = commands;
                            continue;
                        }
                        self.wake();
                        self.count_enqueue(false);
                        return;
                    }
                }
            }
        }
        #[cfg(not(feature = "scan"))]
        {
            let accepted = self.events.push(message).is_ok();
            self.count_enqueue(accepted);
            self.wake();
        }
    }

    fn count_enqueue(&self, accepted: bool) {
        if let Some(progress) = self.ui.diagnostics.get() {
            let counter = if accepted { &progress.enqueued } else { &progress.rejected };
            counter.fetch_add(1, Ordering::Release);
        }
    }

    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Script for ScriptThread {
    fn bind_source_property_owner(&mut self,owner:crate::script::OwnerSourcePropertyPort)->Result<(),String>{
        let(send,reply)=mpsc::sync_channel(1);
        self.ui.edits.try_send(UiRequest::BindSourceProperties(owner,send)).map_err(|_|"property owner binding capacity".to_owned())?;
        self.ui.owner.unpark();reply.recv_timeout(Duration::from_secs(2)).map_err(|_|"property owner binding timeout".to_owned())?
    }
    fn sample_load_status(&mut self)->Result<crate::script::SampleLoadStatus,String> {
        let (send,reply)=mpsc::sync_channel(1);
        self.ui.edits.try_send(UiRequest::SampleStatus(send)).map_err(|_|"sample status owner unavailable/capacity".to_string())?;
        self.ui.owner.unpark();
        reply.recv_timeout(Duration::from_secs(2)).map_err(|_|"sample status owner reply timeout".to_string())
    }
    fn bind_sample_control(&mut self, control: sampler_core::SampleControl) -> Result<(), String> {
        let (send,reply)=mpsc::sync_channel(1);
        self.ui.edits.try_send(UiRequest::SampleControl(control,send)).map_err(|_|"sample owner unavailable/capacity".to_string())?;
        self.ui.owner.unpark();reply.recv_timeout(Duration::from_secs(2)).map_err(|_|"sample owner binding timeout".to_string())?
    }
    fn bind_sample_stream(&mut self, sources: sampler_kontakt::DynamicSources) -> Result<(), String> {
        let (send,reply)=mpsc::sync_channel(1);
        self.ui.edits.try_send(UiRequest::SampleStream(sources,send)).map_err(|_|"sample owner unavailable/capacity".to_string())?;
        self.ui.owner.unpark();reply.recv_timeout(Duration::from_secs(2)).map_err(|_|"sample registry binding timeout".to_string())?
    }
    fn handles_notes(&self) -> bool {
        self.handles_notes
    }

    fn set_time(&mut self, ms: f64) {
        self.time_ms = ms;
    }

    fn note_on(&mut self, id: u64, key: u8, velocity: u8) {
        self.note_on_channel(id, key, velocity, 0);
    }
    fn note_off(&mut self, id: u64, key: u8) {
        self.note_off_channel(id, key, 64, 0);
    }
    fn note_off_velocity(&mut self, id: u64, key: u8, velocity: u8) {
        self.note_off_channel(id, key, velocity, 0);
    }
    fn note_on_channel(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let at_ms = self.time_ms;
        self.enqueue(Message::On { id, key, velocity, channel, at_ms });
    }
    fn note_off_channel(&mut self, id: u64, key: u8, velocity: u8, channel: u8) {
        let at_ms = self.time_ms;
        self.enqueue(Message::Off { id, key, velocity, channel, at_ms });
    }

    fn input(&mut self, input: HostInput) {
        let at_ms = self.time_ms;
        self.enqueue(Message::In { input, at_ms });
    }

    /// The thread advances itself from the clock.
    fn advance(&mut self, _ms: f64) {
        #[cfg(feature = "scan")]
        if self.audit {
            self.time_ms = _ms;
        }
    }

    fn next_due(&mut self) -> Option<f64> {
        #[cfg(feature = "scan")]
        if self.audit {
            return self.audit_due;
        }
        None
    }

    fn drain(&mut self, out: &mut Vec<Command>) {
        #[cfg(feature = "scan")]
        if self.audit {
            out.append(&mut self.audit_pending);
            self.synchronize_audit(out);
            return;
        }
        let mut drained = 0;
        while let Ok(command) = self.commands.pop() {
            match command {
                Outgoing::Command(command) => { out.push(command); drained += 1; }
                Outgoing::Reset{sequence,accepted}=>{
                    if accepted{self.ui.reset_ack.store(sequence,Ordering::Release);}else{self.ui.reset_failed.store(sequence,Ordering::Release);}
                },
                Outgoing::Diagnostic(id) => {
                    if let Some(progress) = self.ui.diagnostics.get() {
                        progress.marker_drained.store(id, Ordering::Release);
                    }
                }
            }
        }
        if let Some(progress) = self.ui.diagnostics.get() {
            progress.drained.fetch_add(drained, Ordering::Release);
        }
    }

    fn tick(&mut self, now_ms: f64) {
        #[cfg(feature = "scan")]
        if self.audit {
            self.time_ms = now_ms;
        }
        self.clock.store(now_ms.to_bits(), Ordering::Release);
    }
}

impl Drop for ScriptThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// Product admission profile for registry metadata, not a native widget limit.
/// UI faces/assets, VM memory, Core/Driver/source storage and allocator overhead
/// are outside this payload budget. Every new host prepares its own cells.
#[derive(Clone, Copy, Debug)]
pub struct UiRegistryBudget {
    pub max_controls: usize,
    pub max_retained_snapshots: usize,
    pub max_payload_bytes: usize,
}
impl Default for UiRegistryBudget {
    fn default() -> Self { Self { max_controls: 65_536, max_retained_snapshots: 64, max_payload_bytes: 32 * 1024 * 1024 } }
}
/// Actual retained registry metadata only; no allocator/RSS certificate.
#[derive(Clone, Copy, Debug, Default)]
pub struct UiRegistryStorage {
    pub prepared_cells: usize,
    pub initial_controls: usize,
    pub initial_requested_payload_bytes: usize,
    pub initial_retained_payload_bytes: usize,
    pub current_controls: usize,
    pub retired_snapshots: usize,
    pub fixed_payload_bytes: usize,
    pub current_payload_bytes: usize,
    pub retired_payload_bytes: usize,
    pub incoming_payload_bytes: usize,
    pub transient_payload_bytes: usize,
    pub requested_payload_bytes: usize,
    pub retained_payload_bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct UiControlSchema {
    id: ControlId,
    kind: u8,
    domain: [u64; 5],
    menu: Vec<(i32, bool)>,
    parent: Option<usize>,
    page: usize,
    rect: [u64; 4],
    editable: bool,
    activate_on_press:bool,
    menu_cycle:bool,
}
struct UiAtomicCell { key: AtomicU64, value: AtomicU64 }
struct UiPublished { face: Arc<Interface>, schema: Arc<Vec<UiControlSchema>>, schema_bytes:usize, generation: u64 }
struct UiPublication { current: Arc<UiPublished>, retired: Vec<std::sync::Weak<UiPublished>> }
// Existing internal causal tests borrow `.face.lock()` directly. Preserve that
// borrow over the SAME publication mutex, not a second presentation mirror.
struct UiFace { state:Mutex<UiPublication> }
#[cfg(test)]
struct UiFaceGuard<'a>(std::sync::MutexGuard<'a,UiPublication>);
#[cfg(test)]
impl std::ops::Deref for UiFaceGuard<'_> {
    type Target=Arc<Interface>;
    fn deref(&self)->&Self::Target{&self.0.current.face}
}
#[cfg(test)]
impl UiFace {
    fn lock(&self)->std::sync::LockResult<UiFaceGuard<'_>> {
        match self.state.lock(){Ok(guard)=>Ok(UiFaceGuard(guard)),Err(error)=>Err(std::sync::PoisonError::new(UiFaceGuard(error.into_inner())))}
    }
}
/// Receipt for the exact displayed schema and its genuine owner bridge.
/// An edit is queued, not a Core/DSP Applied acknowledgement.
#[derive(Clone)]
pub struct UiSnapshot { published: Arc<UiPublished>, owner: std::sync::Weak<UiBridge> }
impl UiSnapshot {
    pub fn interface(&self) -> &Arc<Interface> { &self.published.face }
    pub fn generation(&self) -> u64 { self.published.generation }
    pub fn same_schema(&self, other: &Self) -> bool {
        std::sync::Weak::ptr_eq(&self.owner, &other.owner) && self.generation() == other.generation()
    }
    /// Actual UI owner identity only, not current schema/execution liveness.
    pub fn same_owner(&self,other:&Self)->bool{std::sync::Weak::ptr_eq(&self.owner,&other.owner)}
    pub fn edit(&self, id: ControlId, value: f64) -> bool {
        self.owner.upgrade().is_some_and(|owner| owner.edit_generation(self.generation(), id, value, true))
    }
    pub fn activate(&self,id:ControlId,activation:sampler_ui_ir::UiActivation)->bool{
        self.owner.upgrade().is_some_and(|owner|owner.activate_snapshot(self,id,activation))
    }
}
fn registry_key(id: ControlId) -> Option<u64> {
    ((id.0 >> 64) == 0x5556490000000000 && ((id.0 >> 32) & 0xffff_ffff) != 0).then_some(id.0 as u64)
}
/// Explicit product work profile for registry projection/accounting only.
/// Existing Lua/Interface export and native rendering budgets remain separate.
#[derive(Clone,Copy,Debug)]
pub struct UiSchemaWorkBudget { pub max_inspections:usize }
impl Default for UiSchemaWorkBudget {fn default()->Self{Self{max_inspections:8_388_608}}}
/// Charged work credits, not measured CPU time or a whole UI work certificate.
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub struct UiSchemaWorkReport {pub limit:usize,pub requested:usize,pub admitted:usize,pub exhausted:bool}
struct UiSchemaMeter(UiSchemaWorkReport);
impl UiSchemaMeter {
    fn new(budget:UiSchemaWorkBudget)->Self{Self(UiSchemaWorkReport{limit:budget.max_inspections,..Default::default()})}
    fn charge(&mut self,amount:usize)->Result<(),String>{
        let Some(requested)=self.0.requested.checked_add(amount)else{self.0.requested=usize::MAX;self.0.exhausted=true;return Err("UVI registry schema inspection work overflow".into());};
        self.0.requested=requested;
        if requested>self.0.limit{self.0.exhausted=true;return Err("UVI registry schema inspection work budget exceeded".into());}
        self.0.admitted=requested;Ok(())
    }
}
struct UiRegistryProjection {
    schema:Vec<UiControlSchema>,values:Vec<(ControlId,f64)>,schema_bytes:usize,logical_menu_bytes:usize,comparison_work:usize,
}
fn registry_projection(face:&Interface,budget:UiRegistryBudget,work:&mut UiSchemaMeter)->Result<UiRegistryProjection,String>{
    use sampler_ui_ir::{Binding,Kind};
    let mut count=0usize;let mut menu_count=0usize;
    for widget in &face.widgets {
        work.charge(1)?; // Before reading this widget's declaration.
        let controls=match &widget.kind {
            Kind::Table{cells,columns,..}=>{
                if cells.len()!=*columns as usize||widget.components.len()!=cells.len(){return Err("incoherent UVI table registry".into());}cells.len()
            }
            Kind::Xy{..}=>0,
            _=>usize::from(matches!(widget.binding,Binding::Control(_))),
        };
        count=count.checked_add(controls).ok_or("UVI registry control overflow")?;
        if let Kind::Menu{items}=&widget.kind{menu_count=menu_count.checked_add(items.len()).ok_or("UVI registry menu overflow")?;}
    }
    if count>budget.max_controls{return Err("UVI registry concurrent-control budget exceeded".into());}
    // Reserve each component/menu-entry inspection before allocating output.
    // Credits may remain unused if a later ancestor check refuses; they are
    // never refunded into another attempt or described as executed CPU work.
    let entries=count.checked_add(menu_count).ok_or("UVI registry entry work overflow")?;
    work.charge(entries)?;
    let rows=count.checked_mul(std::mem::size_of::<UiControlSchema>()).ok_or("UVI registry payload overflow")?;
    let menus=menu_count.checked_mul(std::mem::size_of::<(i32,bool)>()).ok_or("UVI registry payload overflow")?;
    let value_bytes=count.checked_mul(std::mem::size_of::<(ControlId,f64)>()).ok_or("UVI registry value payload overflow")?;
    if rows.checked_add(menus).and_then(|n|n.checked_add(value_bytes)).ok_or("UVI registry payload overflow")?>budget.max_payload_bytes{return Err("UVI registry incoming payload budget exceeded".into());}
    let mut out=Vec::with_capacity(count);let mut values=Vec::with_capacity(count);
    let mut schema_bytes=out.capacity().checked_mul(std::mem::size_of::<UiControlSchema>()).and_then(|n|n.checked_add(std::mem::size_of::<Vec<UiControlSchema>>())).ok_or("UVI registry schema payload overflow")?;
    for (index,widget) in face.widgets.iter().enumerate(){
        work.charge(1)?;
        let (kind,range)=match &widget.kind{
            Kind::Knob{range,..}=>(1,Some(*range)),Kind::Slider{range,..}=>(2,Some(*range)),
            Kind::ValueEdit{range,..}=>(3,Some(*range)),Kind::Table{range,..}=>(4,Some(*range)),
            Kind::Button{momentary}=>(if *momentary{5}else{6},None),Kind::Switch=>(7,None),Kind::Menu{..}=>(8,None),_=>(9,None),
        };
        let domain=range.map_or([0;5],|r|[r.min.to_bits(),r.max.to_bits(),r.default.to_bits(),r.step.map_or(0,f64::to_bits),u64::from(r.step.is_some())]);
        // Same visibility/enabled and viewport-offset page coordinates as the
        // old helpers, but ONE charged walk; never call the unmetered helpers.
        let mut editable=widget.intercepts_mouse;let mut rect=widget.rect;
        let mut at=Some(sampler_ui_ir::WidgetRef(index));
        for step in 0..=face.widgets.len(){
            let Some(here)=at else{break;};
            work.charge(1)?; // Before following/reading this ancestor edge.
            let Some(owner)=face.widgets.get(here.0)else{return Err("invalid UVI registry parent".into());};
            editable&=owner.enabled&&!owner.hidden;
            if here.0!=index{
                rect.x+=owner.rect.x-f64::from(owner.viewport.map_or(0,|v|v[0]));
                rect.y+=owner.rect.y-f64::from(owner.viewport.map_or(0,|v|v[1]));
            }
            at=owner.parent;
            if step==face.widgets.len()&&at.is_some(){return Err("cyclic UVI registry parent".into());}
        }
        let components=match &widget.kind{Kind::Table{columns,..}=>*columns as usize,Kind::Xy{..}=>0,_=>usize::from(matches!(widget.binding,Binding::Control(_)))};
        for component in 0..components{
            let component=if kind==4{component+1}else{0};
            let id=crate::script::control_id(index+1,component);
            let value=if let Kind::Table{cells,..}=&widget.kind{
                if widget.components.get(component-1)!=Some(&id){return Err("UVI table control identity mismatch".into());}
                cells[component-1]
            }else{
                if widget.binding!=Binding::Control(id){return Err("UVI control identity mismatch".into());}widget.initial_value
            };
            if registry_key(id).is_none()||!value.is_finite(){return Err("incoherent UVI control/value registry".into());}
            let menu:Vec<_>=if let Kind::Menu{items}=&widget.kind{items.iter().map(|item|(item.value,item.visible)).collect()}else{Vec::new()};
            schema_bytes=schema_bytes.checked_add(menu.capacity().checked_mul(std::mem::size_of::<(i32,bool)>()).ok_or("UVI menu payload overflow")?).ok_or("UVI schema payload overflow")?;
            out.push(UiControlSchema{id,kind,domain,menu,parent:widget.parent.map(|p|p.0),page:widget.page.0,rect:[rect.x.to_bits(),rect.y.to_bits(),rect.width.to_bits(),rect.height.to_bits()],editable,activate_on_press:widget.activate_on_press,menu_cycle:widget.menu_cycle});
            values.push((id,value));
        }
    }
    // Canonical widget/component construction is already ascending. No hidden
    // sort/comparator work; explicitly bound the final identity-order check.
    work.charge(out.len().saturating_sub(1))?;
    if out.windows(2).any(|rows|rows[0].id>=rows[1].id){return Err("unordered/duplicate UVI registry control".into());}
    Ok(UiRegistryProjection{schema:out,values,schema_bytes,logical_menu_bytes:menus,comparison_work:entries.checked_add(1).ok_or("UVI comparison work overflow")?})
}

/// UI-thread edits and presentation snapshots. The audio side reads immutable
/// cell identities plus atomics; Lua and the interface mutex stay on the worker.
pub struct UiBridge {
    diagnostics: OnceLock<Arc<diagnostics::Progress>>,
    #[cfg(feature = "scan")]
    seeded: bool,
    #[cfg(feature = "scan")]
    owner_audit: Option<Arc<OwnerAudit>>,
    #[cfg(feature = "scan")]
    progress: Option<Arc<ScanProgress>>,
    edits: mpsc::SyncSender<UiRequest>,
    owner: std::thread::Thread,
    budget: UiRegistryBudget,
    schema_work_budget:UiSchemaWorkBudget,
    initial_schema_work:UiSchemaWorkReport,
    schema_work:Mutex<UiSchemaWorkReport>,
    initial_controls:usize,
    initial_requested_payload_bytes:usize,
    initial_retained_payload_bytes:usize,
    cells: Box<[UiAtomicCell]>,
    cell_count: AtomicUsize,
    schema_generation: AtomicU64,
    registry_valid: AtomicBool,
    registry_error: Mutex<Option<String>>,
    face: UiFace,
    stop: Arc<AtomicBool>,
    revision: AtomicU64,
    findings: Mutex<Vec<Finding>>,
    faults: Mutex<FaultCounts>,
    runtime_faults: AtomicU64,
    runtime_budgets: AtomicU64,
    reset_ready:AtomicU64,reset_refused:AtomicU64,reset_ack:AtomicU64,reset_failed:AtomicU64,
}
impl UiBridge {
    fn new(
        host:&ScriptHost, edits:mpsc::SyncSender<UiRequest>, owner:std::thread::Thread,
        stop:Arc<AtomicBool>, budget:UiRegistryBudget,work_budget:UiSchemaWorkBudget,
        #[cfg(feature="scan")] seeded:bool,
    )->Result<Self,String>{
        #[cfg(feature="scan")]
        let _span=host.owner_span(OwnerStage::InitialSnapshot);
        let _=host.save_ui_state();
        let face=host.interface();
        let mut work=UiSchemaMeter::new(work_budget);
        let projection=registry_projection(&face,budget,&mut work).map_err(|error|format!("{error}; requested={}, admitted={}, limit={}",work.0.requested,work.0.admitted,work.0.limit))?;
        let schema=Arc::new(projection.schema);let values=projection.values;
        let schema_bytes=projection.schema_bytes;
        let fixed=Self::fixed_registry_bytes(budget)?;
        let retained=fixed.checked_add(schema_bytes)
            .and_then(|n|n.checked_add(std::mem::size_of::<UiPublished>())).ok_or("UVI registry initial payload overflow")?;
        let required=retained.checked_add(values.capacity().checked_mul(std::mem::size_of::<(ControlId,f64)>()).ok_or("UVI registry value payload overflow")?).ok_or("UVI registry initial payload overflow")?;
        let logical_schema=schema.len().checked_mul(std::mem::size_of::<UiControlSchema>()).and_then(|n|n.checked_add(std::mem::size_of::<Vec<UiControlSchema>>())).and_then(|n|n.checked_add(projection.logical_menu_bytes)).ok_or("UVI registry requested schema overflow")?;
        let requested=fixed.checked_add(logical_schema).and_then(|n|n.checked_add(std::mem::size_of::<UiPublished>())).and_then(|n|n.checked_add(values.len().checked_mul(std::mem::size_of::<(ControlId,f64)>())?)).ok_or("UVI registry requested payload overflow")?;
        if required>budget.max_payload_bytes{return Err("UVI registry initial payload budget exceeded".into());}
        let cells:Box<[_]>=(0..budget.max_controls).map(|_|UiAtomicCell{key:AtomicU64::new(0),value:AtomicU64::new(0)}).collect();
        for (cell,(id,value)) in cells.iter().zip(&values){cell.key.store(registry_key(*id).ok_or("invalid initial UVI key")?,Ordering::Relaxed);cell.value.store(value.to_bits(),Ordering::Relaxed);}
        Ok(Self{
            diagnostics:OnceLock::new(),
            #[cfg(feature="scan")] seeded,
            #[cfg(feature="scan")] owner_audit:host.owner_audit(),
            #[cfg(feature="scan")] progress:host.scan_progress(),
            edits,owner,budget,schema_work_budget:work_budget,initial_schema_work:work.0,schema_work:Mutex::new(work.0),initial_controls:values.len(),initial_requested_payload_bytes:requested,initial_retained_payload_bytes:retained,cells,cell_count:AtomicUsize::new(values.len()),schema_generation:AtomicU64::new(2),
            registry_valid:AtomicBool::new(true),registry_error:Mutex::new(None),stop,
            face:UiFace{state:Mutex::new(UiPublication{current:Arc::new(UiPublished{face:Arc::new(face),schema,schema_bytes,generation:2}),retired:Vec::with_capacity(budget.max_retained_snapshots)})},
            revision:AtomicU64::new(1),findings:Mutex::new(host.findings()),faults:Mutex::new(host.fault_counts()),
            runtime_faults:AtomicU64::new(0),runtime_budgets:AtomicU64::new(0),
            reset_ready:AtomicU64::new(0),reset_refused:AtomicU64::new(0),reset_ack:AtomicU64::new(0),reset_failed:AtomicU64::new(0),
        })
    }
    fn publish_findings(&self, host: &ScriptHost) {
        *self.findings.lock().unwrap() = host.findings();
        let faults = host.fault_counts();
        self.runtime_faults.store(
            faults
                .runtime
                .values()
                .copied()
                .fold(0u64, u64::saturating_add),
            Ordering::Release,
        );
        self.runtime_budgets.store(
            faults
                .runtime
                .get(&FaultCategory::Budget)
                .copied()
                .unwrap_or(0),
            Ordering::Release,
        );
        *self.faults.lock().unwrap() = faults;
        self.revision.fetch_add(1, Ordering::Release);
    }
    pub fn findings(&self) -> Vec<Finding> {
        self.findings.lock().unwrap().clone()
    }
    /// Numeric progress only; this does not acquire a UI or Lua owner lock.
    #[cfg(feature = "scan")]
    pub fn scan_progress(&self) -> Option<Arc<ScanProgress>> {
        self.progress.clone()
    }
    /// Fixed numeric aggregates; never waits for a Lua/UI-owner request.
    #[cfg(feature = "scan")]
    pub fn owner_audit(&self) -> Option<OwnerAuditSnapshot> {
        self.owner_audit.as_ref().map(|audit|audit.snapshot())
    }
    pub fn fault_counts(&self) -> FaultCounts {
        self.faults.lock().unwrap().clone()
    }
    /// Lock-free counters for the audio host's cumulative runtime report.
    pub fn runtime_faults(&self) -> (u64, u64) {
        (
            self.runtime_faults.load(Ordering::Acquire),
            self.runtime_budgets.load(Ordering::Acquire),
        )
    }
    fn fixed_registry_bytes(budget: UiRegistryBudget) -> Result<usize,String> {
        if budget.max_controls == 0 || budget.max_payload_bytes == 0 { return Err("empty UVI registry budget".into()); }
        budget.max_controls.checked_mul(std::mem::size_of::<UiAtomicCell>())
            .and_then(|n|n.checked_add(std::mem::size_of::<Self>()))
            .and_then(|n|n.checked_add(budget.max_retained_snapshots.checked_mul(std::mem::size_of::<std::sync::Weak<UiPublished>>())?))
            .ok_or_else(||"UVI registry fixed payload overflow".into())
    }
    fn registry_storage_for(&self, state: &UiPublication, incoming: Option<(&Arc<Vec<UiControlSchema>>,usize)>, retiring: bool,work:&mut UiSchemaMeter) -> Result<UiRegistryStorage,String> {
        work.charge(state.retired.len().checked_add(1).ok_or("UVI registry inventory work overflow")?)?;
        let mut distinct: Vec<Arc<Vec<UiControlSchema>>> = Vec::with_capacity(state.retired.len()+2);
        distinct.push(state.current.schema.clone());
        let mut retired_payload = 0usize;
        let mut retired_count = 0usize;
        // Temporary strong references keep each inspected allocation alive for
        // this off-audio accounting pass. The roster itself contains Weak only.
        let retained: Vec<_> = state.retired.iter().filter_map(std::sync::Weak::upgrade).collect();
        for old in &retained {
            work.charge(distinct.len().checked_add(1).ok_or("UVI registry inventory work overflow")?)?;
            retired_count += 1;
            retired_payload=retired_payload.checked_add(std::mem::size_of::<UiPublished>()).ok_or("UVI registry payload overflow")?;
            if !distinct.iter().any(|schema|Arc::ptr_eq(schema,&old.schema)) {
                retired_payload=retired_payload.checked_add(old.schema_bytes).ok_or("UVI registry payload overflow")?;
                distinct.push(old.schema.clone());
            }
        }
        let current=state.current.schema_bytes.checked_add(std::mem::size_of::<UiPublished>()).ok_or("UVI registry payload overflow")?;
        let mut incoming_bytes=0;
        if let Some((schema,schema_bytes))=incoming {
            work.charge(distinct.len().checked_add(1).ok_or("UVI registry inventory work overflow")?)?;
            incoming_bytes=std::mem::size_of::<UiPublished>();
            if !distinct.iter().any(|old|Arc::ptr_eq(old,schema)) {
                incoming_bytes=incoming_bytes.checked_add(schema_bytes).ok_or("UVI registry payload overflow")?;
            }
        }
        if retiring { retired_count=retired_count.checked_add(1).ok_or("UVI registry count overflow")?; }
        let fixed=self.cells.len().checked_mul(std::mem::size_of::<UiAtomicCell>())
            .and_then(|n|n.checked_add(std::mem::size_of::<Self>()))
            .and_then(|n|n.checked_add(state.retired.capacity().checked_mul(std::mem::size_of::<std::sync::Weak<UiPublished>>())?)).ok_or("UVI registry fixed payload overflow")?;
        let inspection=distinct.capacity().checked_mul(std::mem::size_of::<Arc<Vec<UiControlSchema>>>())
            .and_then(|n|n.checked_add(retained.capacity().checked_mul(std::mem::size_of::<Arc<UiPublished>>())?)).ok_or("UVI registry inspection overflow")?;
        let retained_payload=fixed.checked_add(current).and_then(|n|n.checked_add(retired_payload)).ok_or("UVI registry payload overflow")?;
        let requested=retained_payload.checked_add(incoming_bytes).and_then(|n|n.checked_add(inspection)).ok_or("UVI registry payload overflow")?;
        Ok(UiRegistryStorage{prepared_cells:self.cells.len(),initial_controls:self.initial_controls,initial_requested_payload_bytes:self.initial_requested_payload_bytes,initial_retained_payload_bytes:self.initial_retained_payload_bytes,current_controls:state.current.schema.len(),retired_snapshots:retired_count,fixed_payload_bytes:fixed,current_payload_bytes:current,retired_payload_bytes:retired_payload,incoming_payload_bytes:incoming_bytes,transient_payload_bytes:inspection,requested_payload_bytes:requested,retained_payload_bytes:retained_payload})
    }
    /// Off-audio metadata inventory, including actual Vec capacities. Interface,
    /// image/font payload and allocator overhead are intentionally not priced.
    pub fn registry_storage(&self) -> Result<UiRegistryStorage,String> {
        let mut work=UiSchemaMeter::new(self.schema_work_budget);
        self.registry_storage_for(&self.face.state.lock().unwrap(),None,false,&mut work)
    }
    /// Last schema attempt's charged credits; failure never changes live cells.
    pub fn schema_work(&self)->UiSchemaWorkReport{*self.schema_work.lock().unwrap()}
    pub fn initial_schema_work(&self)->UiSchemaWorkReport{self.initial_schema_work}
    pub fn snapshot(self: &Arc<Self>) -> UiSnapshot {
        UiSnapshot{published:self.face.state.lock().unwrap().current.clone(),owner:Arc::downgrade(self)}
    }
    pub fn owns_snapshot(&self, snapshot:&UiSnapshot) -> bool {
        std::ptr::eq(self,std::sync::Weak::as_ptr(&snapshot.owner))
    }
    pub fn edit_snapshot(&self, snapshot:&UiSnapshot, id:ControlId, value:f64) -> bool {
        self.owns_snapshot(snapshot) && self.edit_generation(snapshot.generation(),id,value,true)
    }
    pub fn activate_snapshot(&self,snapshot:&UiSnapshot,id:ControlId,activation:sampler_ui_ir::UiActivation)->bool{
        if !self.owns_snapshot(snapshot)||!activation.valid()||(id.0&0xffff_ffff)!=0||self.read_control(id).is_none_or(|(generation,_)|generation!=snapshot.generation()){return false;}
        if self.edits.try_send(UiRequest::Activate{generation:snapshot.generation(),id,activation}).is_err(){return false;}
        self.owner.unpark();true
    }
    fn registry_failure(&self,host:&ScriptHost,message:String) {
        // A work-admission refusal must not poison the last admitted atomic
        // cells/presentation. Its queued edits still repeat fresh inspection
        // and refuse BEFORE Lua; readback remains the last admitted snapshot.
        if !self.schema_work().exhausted {self.registry_valid.store(false,Ordering::SeqCst);}
        let mut last=self.registry_error.lock().unwrap();
        if last.as_ref()!=Some(&message) {
            host.ui_registry_failure(&message);
            *last=Some(message);
            self.revision.fetch_add(1,Ordering::Release);
        }
    }
    fn try_publish(&self,host:&ScriptHost)->Result<(),String> {
        let mut work=UiSchemaMeter::new(self.schema_work_budget);
        let result=self.try_publish_metered(host,&mut work);
        *self.schema_work.lock().unwrap()=work.0;
        result
    }
    fn try_publish_metered(&self,host:&ScriptHost,work:&mut UiSchemaMeter)->Result<(),String> {
        let next={
            #[cfg(feature="scan")]
            let _span=host.owner_span(OwnerStage::InterfaceExport);
            host.interface()
        };
        #[cfg(feature="scan")]
        if let Some(audit)=&self.owner_audit {
            work.charge(next.widgets.len())?;
            audit.add(OwnerCounter::SnapshotWidgets,next.widgets.len());
            audit.add(OwnerCounter::SnapshotBindings,next.widgets.iter().filter(|widget|!matches!(widget.binding,sampler_ui_ir::Binding::None)).count());
        }
        let projection=registry_projection(&next,self.budget,work)?;
        let schema=projection.schema;let values=projection.values;
        let schema_bytes=projection.schema_bytes;
        let mut state={
            #[cfg(feature="scan")] let _span=host.owner_span(OwnerStage::FaceLock);
            self.face.state.lock().unwrap()
        };
        work.charge(state.retired.len())?;
        state.retired.retain(|old|old.strong_count()!=0);
        work.charge(projection.comparison_work)?;
        let (changed_schema,changed_face)={
            #[cfg(feature="scan")]
            let _span=host.owner_span(OwnerStage::FaceCompare);
            (*state.current.schema!=schema,*state.current.face!=next)
        };
        let generation=if changed_schema {state.current.generation.checked_add(2).ok_or("UVI registry generation exhausted")?}else{state.current.generation};
        let duplicate_schema_bytes=if changed_schema{0}else{schema_bytes};
        let scratch=values.capacity().checked_mul(std::mem::size_of::<(ControlId,f64)>())
            .and_then(|n|n.checked_add(duplicate_schema_bytes)).ok_or("UVI registry scratch overflow")?;
        let schema=if changed_schema {Arc::new(schema)}else{state.current.schema.clone()};
        // Both the new roster and reflection scratch coexist with retained
        // snapshots, even when no shape changed. They cannot borrow old credit.
        let retiring=Arc::strong_count(&state.current)>1;
        let storage=self.registry_storage_for(&state,Some((&schema,if changed_schema{schema_bytes}else{state.current.schema_bytes})),retiring && (changed_face||changed_schema),work)?;
        let peak=storage.requested_payload_bytes.checked_add(scratch).ok_or("UVI registry peak overflow")?;
        if storage.retired_snapshots>self.budget.max_retained_snapshots || peak>self.budget.max_payload_bytes {return Err("UVI registry retained/incoming payload budget exceeded".into());}
        if changed_face || changed_schema {
            // Preflight the coexistence peak BEFORE exposing a new face, cell
            // roster or epoch. Old snapshots may retain their schema payload.
            let old_count=self.cell_count.load(Ordering::SeqCst);
            work.charge(if changed_schema{old_count.max(values.len())}else{values.len()})?;
            let next=Arc::new(UiPublished{face:Arc::new(next),schema,schema_bytes:if changed_schema{schema_bytes}else{state.current.schema_bytes},generation});
            if changed_schema {
                self.schema_generation.store(generation-1,Ordering::SeqCst);
                for (index,(id,value)) in values.iter().enumerate() {
                    self.cells[index].key.store(id.0 as u64,Ordering::SeqCst);
                    self.cells[index].value.store(value.to_bits(),Ordering::SeqCst);
                }
                // Removed slots are immediately reusable; no reader keeps a
                // cell reference. Old requests still carry their OLD epoch.
                for cell in &self.cells[values.len()..old_count.max(values.len())] { cell.key.store(0,Ordering::SeqCst); }
                self.cell_count.store(values.len(),Ordering::SeqCst);
            } else {
                for (cell,(_,value)) in self.cells.iter().zip(&values) {cell.value.store(value.to_bits(),Ordering::Release);}
            }
            let old=std::mem::replace(&mut state.current,next);
            if retiring {state.retired.push(Arc::downgrade(&old));}
            drop(old);
            self.schema_generation.store(generation,Ordering::SeqCst);
            self.revision.fetch_add(1,Ordering::Release);
            #[cfg(feature="scan")] host.owner_count(OwnerCounter::FaceChanges,1);
        } else {
            work.charge(values.len())?;
            for (cell,(_,value)) in self.cells.iter().zip(&values) {cell.value.store(value.to_bits(),Ordering::Release);}
        }
        self.registry_valid.store(true,Ordering::SeqCst);
        *self.registry_error.lock().unwrap()=None;
        Ok(())
    }
    fn publish_source_property_presentation(&self,host:&ScriptHost) {
        // No fallback on a captured/product owner. Presence only selects this
        // request family; actual getters validate/query their bound Core plan.
        if host.has_standalone_property_presentation_endpoint(){self.publish(host);}
    }
    /// Queue-only non-RT presentation request, not a value/ready ACK. Normal
    /// Driver::wake services actual Core queries; then value()/snapshot() expose
    /// the newly admitted registry. No changed/onController callback is invoked.
    pub fn request_source_property_refresh(&self)->bool {
        if self.stop.load(Ordering::Acquire){return false;}
        if self.edits.try_send(UiRequest::RefreshSourcePropertyPresentation).is_err(){return false;}
        self.owner.unpark();true
    }
    fn publish(&self,host:&ScriptHost) {
        if let Err(error)=self.try_publish(host) {self.registry_failure(host,error);}
    }
    fn apply_control(&self,host:&mut ScriptHost,generation:u64,id:ControlId,value:f64,drawn:bool,caller_phase:DiagnosticPhase) {
        // A preceding callback can change Lua without the normal end-of-turn
        // export. Refresh its ACTUAL current declaration before this action.
        self.diagnostic_phase(DiagnosticPhase::UiExport);
        let inspected=self.try_publish(host);
        self.diagnostic_phase(caller_phase);
        if let Err(error)=inspected {self.registry_failure(host,error);return;}
        let allowed={
            let state=self.face.state.lock().unwrap();
            generation==state.current.generation && state.current.schema.binary_search_by_key(&id,|row|row.id).ok().is_some_and(|at|!drawn||state.current.schema[at].editable)
        };
        if allowed && !self.stop.load(Ordering::Acquire) {let _=host.set_control(id,value);}
    }
    fn apply_activation(&self,host:&mut ScriptHost,generation:u64,id:ControlId,activation:sampler_ui_ir::UiActivation,caller_phase:DiagnosticPhase){
        self.diagnostic_phase(DiagnosticPhase::UiExport);
        let inspected=self.try_publish(host);
        self.diagnostic_phase(caller_phase);
        if let Err(error)=inspected{self.registry_failure(host,error);return;}
        let allowed={
            let state=self.face.state.lock().unwrap();
            generation==state.current.generation&&state.current.schema.binary_search_by_key(&id,|row|row.id).ok().is_some_and(|at|{
                let row=&state.current.schema[at];
                let button=matches!(row.kind,5|6)||(row.kind==8&&row.menu_cycle);
                let phase=match activation.origin {
                    sampler_ui_ir::UiActivationOrigin::PointerDown=>row.activate_on_press,
                    sampler_ui_ir::UiActivationOrigin::PointerUp=>!row.activate_on_press,
                    sampler_ui_ir::UiActivationOrigin::Keyboard|sampler_ui_ir::UiActivationOrigin::Accessibility=>true,
                };
                row.editable&&button&&phase&&activation.valid()
            })
        };
        if allowed&&!self.stop.load(Ordering::Acquire){let _=host.activate_control(id,activation);}
    }
    fn read_control(&self,id:ControlId)->Option<(u64,f64)> {
        if self.stop.load(Ordering::Acquire)||!self.registry_valid.load(Ordering::SeqCst) {return None;}
        let key=registry_key(id)?;
        let generation=self.schema_generation.load(Ordering::SeqCst);
        if generation&1!=0 {return None;}
        let mut lo=0;let mut hi=self.cell_count.load(Ordering::SeqCst);
        if hi>self.cells.len(){return None;}
        let mut found=None;
        // One bounded binary-search probe. No retries, locks, heap or Lua.
        while lo<hi {
            let at=lo+(hi-lo)/2;
            match self.cells[at].key.load(Ordering::SeqCst).cmp(&key) {
                std::cmp::Ordering::Less=>lo=at+1,
                std::cmp::Ordering::Greater=>hi=at,
                std::cmp::Ordering::Equal=>{found=Some(f64::from_bits(self.cells[at].value.load(Ordering::SeqCst)));break;}
            }
        }
        if self.schema_generation.load(Ordering::SeqCst)!=generation||!self.registry_valid.load(Ordering::SeqCst)||self.stop.load(Ordering::Acquire){return None;}
        Some((generation,found?))
    }
    fn edit_generation(&self,generation:u64,id:ControlId,value:f64,drawn:bool)->bool {
        if !value.is_finite()||self.read_control(id).is_none_or(|(current,_)|current!=generation){return false;}
        if self.edits.try_send(UiRequest::Edit{generation,id,value,drawn}).is_err(){return false;}
        self.owner.unpark();true
    }
    pub fn edit(&self,id:ControlId,value:f64)->bool {
        if !value.is_finite(){return false;}
        let Some((generation,_))=self.read_control(id)else{return false;};
        // One probe, no retry. A race after enqueue is checked again by the
        // owner against its current actual schema before executing Lua.
        if self.edits.try_send(UiRequest::Edit{generation,id,value,drawn:false}).is_err(){return false;}
        self.owner.unpark();true
    }
    pub fn value(&self,id:ControlId)->Option<f64>{self.read_control(id).map(|(_,value)|value)}
    pub fn values(&self)->Vec<(ControlId,f64)> {
        let state=self.face.state.lock().unwrap();let generation=state.current.generation;
        let values:Vec<_>=state.current.schema.iter().filter_map(|row|self.read_control(row.id).filter(|(at,_)|*at==generation).map(|(_,value)|(row.id,value))).collect();
        if values.len()!=state.current.schema.len(){return Vec::new();}values
    }
    pub fn interface(&self)->Arc<Interface>{self.face.state.lock().unwrap().current.face.clone()}
    /// Host/background thread only: execute authored onSave on the Lua owner.
    pub fn state(&self) -> Result<UiState, String> {
        let (reply, state) = mpsc::sync_channel(1);
        self.owner.unpark();
        self.edits
            .send(UiRequest::Save(reply))
            .map_err(|_| "UVI save worker stopped".to_string())?;
        self.owner.unpark();
        state
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "UVI save worker did not reply".to_string())?
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}

#[cfg(all(test, feature = "scan"))]
mod owner_audit_tests;
#[cfg(test)]
mod diagnostic_tests;
#[cfg(test)]
mod ready_batch_tests;

#[cfg(test)]
mod registry_work_tests {
    use super::*;
    fn budget()->UiRegistryBudget{UiRegistryBudget{max_controls:256,max_retained_snapshots:2,max_payload_bytes:128*1024}}
    fn face()->Interface{Interface::default()}
    #[test]
    fn malformed_parent_cycle_stops_within_work_bound_and_never_returns_a_registry(){
        use sampler_ui_ir::{Kind,PageRef,Rect,Widget,WidgetRef};
        let mut face=face();
        let mut a=Widget::new("A",PageRef(0),Rect::default(),Kind::Panel);a.parent=Some(WidgetRef(1));
        let mut b=Widget::new("B",PageRef(0),Rect::default(),Kind::Panel);b.parent=Some(WidgetRef(0));face.widgets=vec![a,b];
        let mut work=UiSchemaMeter::new(UiSchemaWorkBudget{max_inspections:64});
        let error=registry_projection(&face,budget(),&mut work).err().expect("no registry for a cycle");
        assert!(error.contains("cyclic"),"{error}");assert!(work.0.admitted<=64);
        let mut work=UiSchemaMeter::new(UiSchemaWorkBudget{max_inspections:3});
        assert!(registry_projection(&face,budget(),&mut work).is_err());assert!(work.0.exhausted);assert_eq!(work.0.admitted,3);
    }
    #[test]
    fn all_table_components_are_work_preflighted_before_projection_allocation(){
        use sampler_ui_ir::{Binding,Kind,PageRef,Range,Rect,Widget};
        let mut face=face();let mut table=Widget::new("Steps",PageRef(0),Rect::default(),Kind::Table{columns:128,range:Range{min:0.,max:1.,default:0.25,step:None},bipolar:false,cells:vec![0.25;128],steps_shown:None});
        table.binding=Binding::Control(crate::script::control_id(1,1));table.components=(1..=128).map(|component|crate::script::control_id(1,component)).collect();face.widgets.push(table);
        let mut work=UiSchemaMeter::new(UiSchemaWorkBudget{max_inspections:128});
        let error=registry_projection(&face,budget(),&mut work).err().expect("component population exceeds admitted inspection credit");
        assert!(error.contains("work budget"),"{error}");assert_eq!((work.0.requested,work.0.admitted),(129,1));
    }
    #[test]
    fn charged_ancestor_projection_preserves_viewport_offset_and_effective_disabled_state(){
        use sampler_ui_ir::{Binding,Kind,PageRef,Range,Rect,Widget,WidgetRef};
        let mut face=face();let mut parent=Widget::new("Parent",PageRef(0),Rect::new(40,60,200,100),Kind::Panel);parent.viewport=Some([7,11]);parent.enabled=false;
        let mut child=Widget::new("Child",PageRef(0),Rect::new(13,17,80,30),Kind::Knob{range:Range{min:0.,max:1.,default:0.25,step:None},display:Default::default()});child.parent=Some(WidgetRef(0));child.binding=Binding::Control(crate::script::control_id(2,0));child.initial_value=0.25;face.widgets=vec![parent,child];
        let mut work=UiSchemaMeter::new(UiSchemaWorkBudget{max_inspections:64});let projection=registry_projection(&face,budget(),&mut work).unwrap();
        assert_eq!(projection.schema[0].rect,[46f64.to_bits(),66f64.to_bits(),80f64.to_bits(),30f64.to_bits()]);assert!(!projection.schema[0].editable);assert_eq!(projection.values,[(crate::script::control_id(2,0),0.25)]);
    }
}


#[cfg(test)]
mod prepared_root_handoff_tests {
    use super::*;
    use sampler_core::{Instruction,Program,WaitLifetime,SourceTaskFamily as Family,
        PreparedSourceTaskBinding as Binding};

    fn prepared(xml:&str)->sampler_core::Runtime {
        let translated=crate::translate(xml,std::path::Path::new(".")).unwrap();
        let processor=translated.instrument.source_indices.owners.iter()
            .flat_map(|owner|&owner.event_processors).find(|row|row.kind=="ScriptProcessor").unwrap().id;
        assert!(translated.instrument.assets.is_empty(),"authored task-only graph has no sample assets");
        // Behavior binding is supplied by the real external programs below;
        // decoded resources are the separate, empty PCM argument to lower.
        let prepared=sampler_core::lower::lower(&translated.instrument,48000,vec![],|_,plan|Ok(plan)).unwrap()
            .with_programs(vec![Program::new(vec![Instruction::AwaitExternal{processor},Instruction::End]).unwrap()
                .with_wait_lifetime(WaitLifetime::Callback)],None).unwrap()
            .with_source_tasks(vec![Binding{processor,family:Family::Run,program:0},
                Binding{processor,family:Family::Spawned,program:0}]).unwrap();
        let limits=sampler_core::Limits::for_plan(&prepared,8,8);
        sampler_core::Runtime::new(prepared,limits).unwrap()
    }
    fn ready(owner:&mut ScriptThread,rt:&mut sampler_core::Runtime,plan:sampler_core::PlanId)->usize {
        let deadline=Instant::now()+Duration::from_secs(3);
        loop {
            match owner.poll_prepared_root_adoption(rt,plan).unwrap() {
                PreparedRootProgress::Ready{tasks}=>return tasks,
                PreparedRootProgress::Pending=>{},
            }
            assert!(Instant::now()<deadline,"actual Lua root binding ACK deadline");
            std::thread::yield_now();
        }
    }
    fn execute_one(owner:&mut ScriptThread,rt:&mut sampler_core::Runtime) {
        use crate::script::{AuthorityOperation as Op,AuthorityValue as Value,AuthorityReply};
        let turn=rt.pending_external_turn().unwrap().expect("real prepared continuation");
        let turn=rt.begin_external_turn(turn.input).unwrap();
        owner.deliver_external_turn(turn).unwrap();
        let deadline=Instant::now()+Duration::from_secs(3);
        loop {
            if let Some(request)=owner.pop_external_request() {
                assert_eq!(request.turn.input,turn.input);
                assert!(!matches!(request.operation,Op::Cancel),"actual Lua root failed; a successful Cancel ACK is not a passing callback");
                let finishing=matches!(request.operation,Op::Complete{..}|Op::Cancel);
                let mut retired_command=None;
                let result=match request.operation {
                    Op::Validate=>rt.running_external_turn(turn.input).map(|_|Value::Validated),
                    Op::QueryConstant{node}=>rt.running_external_turn(turn.input).and_then(|_|rt.source_constant_property(turn.plan,node)).map(Value::Constant),
                    Op::Complete{next_frame}=>rt.complete_external_segment(turn.input,next_frame).map(|_|Value::Completed),
                    Op::Cancel=>rt.cancel_external_input(turn.input).map(|_|Value::Completed),
                    Op::Post(output)=>rt.post_external_output(turn.input,output).map(Value::Posted),
                    Op::Command(command)=>{retired_command=Some(command);Err(sampler_core::Error::InvalidInput)},
                };
                assert!(result.is_ok(),"real root task operation refused: {result:?}");
                owner.reply_external_request(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command}).unwrap();
                if finishing {return;}
            }
            assert!(!owner.external_cancelled(),"actual owner cancelled root task");
            assert!(Instant::now()<deadline,"actual task operation deadline");
            std::thread::yield_now();
        }
    }
    #[test]
    fn active_root_setup_is_pending_until_owner_ack_and_repeated_poll_does_not_duplicate_tasks() {
        let xml:Arc<str>=Arc::from(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            prefix=0;suffix=0;spawned=0;
            run(function() prefix=prefix+1;wait(1);assert(prefix==1);suffix=suffix+1;assert(suffix==1) end);
            spawn(function() spawned=spawned+1;assert(spawned==1) end)
        ]]></script></ScriptProcessor></EventProcessors></Program>"#);
        let mut rt=prepared(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let (mut owner,_)=ScriptThread::spawn_staged_with_ui_state(Arc::clone(&xml),(),Config::default(),None).unwrap();
        owner.bind_external_authority(&rt,plan,Duration::from_secs(2)).unwrap();
        assert_eq!(owner.start_prepared_root_adoption(&mut rt,plan,perf),Ok(PreparedRootProgress::Pending),
            "enqueued binding transfer never impersonates an owner ACK");
        assert_eq!(rt.now(),0);
        assert_eq!(ready(&mut owner,&mut rt,plan),2);
        for _ in 0..16 {assert_eq!(owner.start_prepared_root_adoption(&mut rt,plan,perf),Ok(PreparedRootProgress::Ready{tasks:2}));}
        execute_one(&mut owner,&mut rt); // adopt the existing residual wait, no replay
        execute_one(&mut owner,&mut rt); // independent deferred spawn, once
        assert!(rt.pending_external_turn().unwrap().is_none());
        rt.render(&mut [[0.;2];48]).unwrap();rt.service_external_boundary().unwrap();
        execute_one(&mut owner,&mut rt); // same genuine run coroutine suffix
        assert!(rt.pending_external_turn().unwrap().is_none());
        assert_eq!(rt.now(),48);assert!(!rt.external_execution_aborted());
    }
    #[test]
    fn root_setup_ack_cannot_bind_another_runtime_or_survive_authority_cancellation() {
        let xml:Arc<str>=Arc::from("<Program><EventProcessors><ScriptProcessor><script>spawn(function() end)</script></ScriptProcessor></EventProcessors></Program>");
        let mut actual=prepared(&xml);let plan=actual.active_plan();let perf=actual.performance(0).unwrap();
        let mut other=prepared(&xml);let other_plan=other.active_plan();
        let (mut owner,_)=ScriptThread::spawn_staged_with_ui_state(Arc::clone(&xml),(),Config::default(),None).unwrap();
        owner.bind_external_authority(&actual,plan,Duration::from_secs(2)).unwrap();
        assert_eq!(owner.start_prepared_root_adoption(&mut actual,plan,perf),Ok(PreparedRootProgress::Pending));
        assert_eq!(owner.poll_prepared_root_adoption(&mut other,other_plan),Err(sampler_core::Error::StaleHandle));
        assert_eq!(other.now(),0);assert!(other.pending_external_turn().unwrap().is_none());
        assert_eq!(ready(&mut owner,&mut actual,plan),1);
        owner.cancel_external_authority();
        assert!(owner.start_prepared_root_adoption(&mut actual,plan,perf).is_err());
        assert_eq!(actual.now(),0);assert_eq!(other.now(),0);
    }
    #[test]
    fn restored_widget_active_root_batch_refuses_late_invalid_entry_without_partial_adoption(){
        let xml:Arc<str>=Arc::from(r#"<Program><EventProcessors><ScriptProcessor><script><![CDATA[
            prefix=0;suffix=0;spawned=0;k=Knob('Root',0,0,1);
            k.changed=function(self)
                run(function() prefix=prefix+1;wait(1);assert(prefix==1);suffix=suffix+1;assert(suffix==1) end);
                spawn(function() spawned=spawned+1;assert(spawned==1) end);
                spawn(function() spawned=spawned+1;assert(spawned==2) end)
            end
        ]]></script></ScriptProcessor></EventProcessors></Program>"#);
        let state=UiState{widgets:vec![(1,"Root".into(),crate::script::SavedValue::Number(1.))],..Default::default()};
        let mut rt=prepared(&xml);let plan=rt.active_plan();let perf=rt.performance(0).unwrap();
        let (mut owner,_)=ScriptThread::spawn_staged_with_ui_state(Arc::clone(&xml),(),Config::default(),Some(state)).unwrap();
        assert_eq!(owner.prepared_roots.len(),3);
        assert!(owner.prepared_roots.iter().all(|root|root.origin==sampler_core::PreparedGlobalTaskOrigin::RestoredWidgetEvent));
        owner.bind_external_authority(&rt,plan,Duration::from_secs(2)).unwrap();
        let actual=owner.root_requests[2].processor;
        owner.root_requests[2].processor=sampler_ir::SourceEventProcessorId::UviNode(u32::MAX);
        assert!(owner.start_prepared_root_adoption(&mut rt,plan,perf).is_err());
        assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.now(),0);
        assert!(owner.root_bindings.as_ref().unwrap().iter().all(Option::is_none));
        assert!(!rt.external_execution_aborted());assert!(!owner.external_cancelled());
        owner.root_requests[2].processor=actual;
        assert_eq!(owner.start_prepared_root_adoption(&mut rt,plan,perf),Ok(PreparedRootProgress::Pending));
        assert_eq!(ready(&mut owner,&mut rt,plan),3);
        let turn=rt.pending_external_turn().unwrap().unwrap();
        assert!(matches!(turn.event,sampler_core::ExternalEvent::Task(task)if task.source.is_none()
            &&task.creator==sampler_core::SourceTaskCreator::NonMusicalEvent(sampler_core::NonMusicalTaskOrigin::RestoredWidget)));
        execute_one(&mut owner,&mut rt);execute_one(&mut owner,&mut rt);execute_one(&mut owner,&mut rt);
        assert!(rt.pending_external_turn().unwrap().is_none());
        rt.render(&mut [[0.;2];48]).unwrap();rt.service_external_boundary().unwrap();execute_one(&mut owner,&mut rt);
        assert!(rt.pending_external_turn().unwrap().is_none());assert_eq!(rt.now(),48);assert!(!owner.external_cancelled());
    }

    #[test]
    fn ordered_reset_requires_real_owner_ack_and_cannot_coalesce_or_replay_sequences(){
        let xml:Arc<str>=Arc::from("<Program><EventProcessors><ScriptProcessor><script>function onInit() end</script></ScriptProcessor></EventProcessors></Program>");
        let mut rt=prepared(&xml);let plan=rt.active_plan();
        let (mut owner,_)=ScriptThread::spawn_staged_with_ui_state(Arc::clone(&xml),(),Config::default(),None).unwrap();
        owner.bind_external_authority(&rt,plan,Duration::from_secs(2)).unwrap();
        // Cancel actual Core work at H before queueing the real owner barrier;
        // no fake physical key-up or advancement is involved.
        rt.panic();assert_eq!(rt.now(),0);
        owner.reset_execution(rt.id(),plan,1).unwrap();
        assert!(!owner.reset_execution_available());
        assert_eq!(owner.reset_execution(rt.id(),plan,2),Err(sampler_core::Error::Capacity));
        assert_eq!(owner.reset_execution(rt.id(),plan,1),Err(sampler_core::Error::InvalidInput));
        let deadline=Instant::now()+Duration::from_secs(3);
        while !owner.reset_execution_ack(1).unwrap(){assert!(Instant::now()<deadline);std::thread::yield_now();}
        assert_eq!(owner.reset_execution_ack(1),Err(sampler_core::Error::StaleHandle),"one exact marker has one owner ACK");
        assert_eq!(owner.reset_execution(rt.id(),plan,1),Err(sampler_core::Error::InvalidInput));
        assert!(owner.reset_execution_available());assert_eq!(rt.now(),0);
        owner.reset_execution(rt.id(),plan,2).unwrap();
        while !owner.reset_execution_ack(2).unwrap(){assert!(Instant::now()<deadline);std::thread::yield_now();}
        assert!(!owner.external_cancelled());assert_eq!(rt.now(),0);
    }

}


#[cfg(test)]
mod reset_quiescence42g_tests {
    use super::*;
    fn pair()->(sampler_core::Runtime,ScriptThread){
        use sampler_core::{Prepared,Program,Instruction,WaitLifetime,Limits};
        let xml:Arc<str>=Arc::from("<Program><EventProcessors><ScriptProcessor><script>calls=0;function onController(e) calls=calls+1 end</script></ScriptProcessor></EventProcessors></Program>");
        let translated=crate::translate(&xml,std::path::Path::new(".")).unwrap();let processor=translated.instrument.source_indices.owners.iter().flat_map(|owner|&owner.event_processors).find(|row|row.kind=="ScriptProcessor").unwrap().id;
        let prepared=sampler_core::lower::lower(&translated.instrument,48000,vec![],|_,plan:Prepared|Ok(plan)).unwrap().with_programs(vec![Program::new(vec![Instruction::AwaitExternal{processor},Instruction::End]).unwrap().with_wait_lifetime(WaitLifetime::Callback)],None).unwrap().with_controller_programs(vec![0]).unwrap();
        let limits=Limits::for_plan(&prepared,8,8);let runtime=sampler_core::Runtime::new(prepared,limits).unwrap();let(mut thread,_)=ScriptThread::spawn_staged_with_ui_state(xml,(),Default::default(),None).unwrap();thread.bind_external_authority(&runtime,runtime.active_plan(),Duration::from_secs(2)).unwrap();(runtime,thread)
    }
    #[test]
    fn reset_refuses_queued_or_running_segment_until_exact_terminal_reply_then_marker_ack(){
        use crate::script::{AuthorityOperation as Op,AuthorityValue as Value,AuthorityReply};
        let(mut rt,mut thread)=pair();let plan=rt.active_plan();let perf=rt.performance(0).unwrap();rt.dispatch_controller(perf,sampler_core::ChannelAddress{protocol:sampler_core::Protocol::Midi1,port:0,group:0,channel:0},1,1,0).unwrap();rt.service_external_boundary().unwrap();let pending=rt.pending_external_turn().unwrap().unwrap();let turn=rt.begin_external_turn(pending.input).unwrap();thread.deliver_external_turn(turn).unwrap();
        assert!(!thread.reset_execution_available());assert_eq!(thread.reset_execution(rt.id(),plan,1),Err(sampler_core::Error::Capacity));assert!(rt.running_external_turn(turn.input).is_ok());
        let deadline=std::time::Instant::now()+Duration::from_secs(2);
        while !thread.reset_execution_available(){
            if let Some(request)=thread.pop_external_request(){let result=match request.operation{
                Op::Validate=>rt.running_external_turn(turn.input).map(|_|Value::Validated),
                Op::Post(output)=>rt.post_external_output(turn.input,output).map(Value::Posted),
                Op::Complete{next_frame}=>rt.complete_external_segment(turn.input,next_frame).map(|_|Value::Completed),
                other=>panic!("pure callback has no other command: {other:?}"),
            };thread.reply_external_request(AuthorityReply{input:turn.input,ordinal:request.ordinal,result,retired_command:None}).unwrap();}
            assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_micros(50));
        }
        // The actual segment completed. Core cancellation of old waits (none in
        // this case) precedes the real ordered marker; queueing is not its ACK.
        thread.reset_execution(rt.id(),plan,1).unwrap();assert!(!thread.reset_execution_available());
        while !thread.reset_execution_ack(1).unwrap(){assert!(std::time::Instant::now()<deadline);std::thread::sleep(Duration::from_micros(50));}
        assert!(thread.reset_execution_available());thread.cancel_external_authority();assert!(!thread.reset_execution_available());assert!(thread.reset_execution(rt.id(),plan,2).is_err());
    }
}
