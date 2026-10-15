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
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    thread::{Builder, JoinHandle},
    time::Duration,
};

/// Events and commands in flight between the two sides.
const QUEUE: usize = 1024;
/// While a script waits for time, how often its thread looks at the clock.
const POLL: Duration = Duration::from_millis(1);

/// Diagnostic markers share command ordering, but never become audio commands.
enum Outgoing {
    Command(Command),
    Diagnostic(u64),
}

#[derive(Clone, Copy)]
enum Message {
    Control {
        id: ControlId,
        value: f64,
    },
    On {
        id: u64,
        key: u8,
        velocity: u8,
        at_ms: f64,
    },
    Off {
        id: u64,
        key: u8,
        velocity: u8,
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
    /// The audio clock in milliseconds, as `f64` bits.
    clock: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    handles_notes: bool,
    time_ms: f64,
    #[cfg(feature = "scan")]
    audit: bool,
    #[cfg(feature = "scan")]
    audit_due: Option<f64>,
    #[cfg(feature = "scan")]
    audit_pending: Vec<Command>,
}

enum UiRequest {
    Diagnostic(diagnostics::Pending),
    SampleStatus(mpsc::SyncSender<crate::script::SampleLoadStatus>),
    SampleControl(sampler_core::SampleControl, mpsc::SyncSender<Result<(),String>>),
    SampleStream(sampler_kontakt::DynamicSources, mpsc::SyncSender<Result<(),String>>),
    Edit(ControlId, f64),
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
        match message {
            Message::Control { id, value } => {
                let _ = host.set_control(id, value);
            }
            Message::On {
                id,
                key,
                velocity,
                at_ms,
            } => {
                host.set_time(at_ms);
                host.note_on(id, key, velocity, 0);
            }
            Message::Off { id, key, velocity, at_ms } => {
                host.set_time(at_ms);
                host.note_off(id, key, velocity, 0);
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
            UiRequest::SampleStatus(reply)=>{let _=reply.send(ScriptHost::sample_load_status(host));}
            UiRequest::SampleControl(control,reply)=>{let _=reply.send(host.bind_sample_control(control));}
            UiRequest::SampleStream(sources,reply)=>{let _=reply.send(host.bind_sample_stream(sources));}
            UiRequest::Edit(id, value) => {
                let _ = host.set_control(id, value);
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
    let mut batch = batch.into_iter();
    while let Some(command) = batch.next() {
        match outgoing.push(Outgoing::Command(command)) {
            Ok(()) => {
                if let Some(progress) = ui.diagnostics.get() {
                    progress.pushed.fetch_add(1, Ordering::Release);
                }
                #[cfg(feature = "scan")]
                {
                    host.owner_count(OwnerCounter::CommandsPushed, 1);
                    host.owner_count(OwnerCounter::ReadyCommandsPushed, 1);
                }
            }
            Err(rtrb::PushError::Full(Outgoing::Command(command))) => {
                // SPSC free space cannot shrink while this producer pushes
                // the admitted batch. Retain FIFO even if that API changes.
                backlog.push(command);
                backlog.extend(batch);
                break;
            }
            Err(rtrb::PushError::Full(Outgoing::Diagnostic(_))) => unreachable!(),
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
        #[cfg(feature = "scan")]
        let audit = config.audit_seed.is_some();
        let (ui_send, ui_receive) = mpsc::sync_channel(256);
        let (events, mut incoming) = rtrb::RingBuffer::new(QUEUE);
        let (mut outgoing, commands) = rtrb::RingBuffer::new(QUEUE);
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
                        match ScriptHost::new_with_ui_state(&xml, files, config, state.as_ref()) {
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
                    let ui = Arc::new(UiBridge::new(&host, ui_send, std::thread::current(),
                        #[cfg(feature = "scan")] audit,
                    ));
                    #[cfg(feature = "scan")]
                    {
                        *scan.lock().unwrap() = host.scan_faults();
                    }
                    let report = Loaded {
                        insert_overrides: host.insert_overrides(),
                        folded_writes: host.folded_writes(),
                        findings: host.findings(),
                        interface: ui.interface().as_ref().clone(),
                        ui: ui.clone(),
                    };
                    let _ = ready.send(Ok((handles, report)));
                    drop(ready);
                    let mut backlog: Vec<Command> = Vec::new();
                    let mut revision = host.ui_revision();
                    let mut finding_revision = host.finding_revision();
                    let mut diagnostic: Option<diagnostics::Pending> = None;
                    while !stop.load(Ordering::Acquire) {
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
        let (handles_notes, report) = loaded
            .recv()
            .map_err(|_| "the script thread stopped while loading".to_string())??;
        Ok((
            Self {
                #[cfg(feature = "scan")]
                scan,
                events,
                ui: report.ui.clone(),
                commands,
                clock,
                stop,
                thread: Some(thread),
                handles_notes,
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

    pub fn ui(&self) -> &Arc<UiBridge> {
        &self.ui
    }

    pub fn set_control(&mut self, id: ControlId, value: f64) -> bool {
        if !value.is_finite() || self.ui.value(id).is_none() {
            return false;
        }
        let ok = self.events.push(Message::Control { id, value }).is_ok();
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
        let at_ms = self.time_ms;
        self.enqueue(Message::On {
            id,
            key,
            velocity,
            at_ms,
        });
    }

    fn note_off(&mut self, id: u64, key: u8) {
        let at_ms = self.time_ms;
        self.enqueue(Message::Off { id, key, velocity: 64, at_ms });
    }

    fn note_off_velocity(&mut self, id: u64, key: u8, velocity: u8) {
        let at_ms = self.time_ms;
        self.enqueue(Message::Off { id, key, velocity, at_ms });
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
    values: Vec<(ControlId, AtomicU64)>,
    face: Mutex<Arc<Interface>>,
    revision: AtomicU64,
    findings: Mutex<Vec<Finding>>,
    faults: Mutex<FaultCounts>,
    runtime_faults: AtomicU64,
    runtime_budgets: AtomicU64,
}
impl UiBridge {
    fn new(
        host: &ScriptHost,
        edits: mpsc::SyncSender<UiRequest>,
        owner: std::thread::Thread,
        #[cfg(feature = "scan")] seeded: bool,
    ) -> Self {
        #[cfg(feature = "scan")]
        let _span = host.owner_span(OwnerStage::InitialSnapshot);
        let _ = host.save_ui_state();
        let face = host.interface();
        let mut values = ScriptHost::control_values_from(&face);
        values.sort_by_key(|(id, _)| *id);
        Self {
            diagnostics: OnceLock::new(),
            #[cfg(feature = "scan")]
            seeded,
            #[cfg(feature = "scan")]
            owner_audit: host.owner_audit(),
            #[cfg(feature = "scan")]
            progress: host.scan_progress(),
            edits,
            owner,
            values: values
                .into_iter()
                .map(|(id, v)| (id, AtomicU64::new(v.to_bits())))
                .collect(),
            face: Mutex::new(Arc::new(face)),
            revision: AtomicU64::new(1),
            findings: Mutex::new(host.findings()),
            faults: Mutex::new(host.fault_counts()),
            runtime_faults: AtomicU64::new(0),
            runtime_budgets: AtomicU64::new(0),
        }
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
    fn publish(&self, host: &ScriptHost) {
        let next = {
            #[cfg(feature = "scan")]
            let _span = host.owner_span(OwnerStage::InterfaceExport);
            host.interface()
        };
        #[cfg(feature = "scan")]
        {
            if let Some(audit) = &self.owner_audit {
                audit.add(OwnerCounter::SnapshotWidgets, next.widgets.len());
                audit.add(OwnerCounter::SnapshotBindings, next.widgets.iter().filter(|widget|
                    !matches!(widget.binding, sampler_ui_ir::Binding::None)).count());
            }
        }
        {
        #[cfg(feature = "scan")]
        let _span = host.owner_span(OwnerStage::ValueMirror);
        for (id, v) in ScriptHost::control_values_from(&next) {
            if let Ok(i) = self.values.binary_search_by_key(&id, |(id, _)| *id) {
                self.values[i].1.store(v.to_bits(), Ordering::Release);
            }
        }
        }
        let mut face = {
            #[cfg(feature = "scan")]
            let _span = host.owner_span(OwnerStage::FaceLock);
            self.face.lock().unwrap()
        };
        let changed = {
            #[cfg(feature = "scan")]
            let _span = host.owner_span(OwnerStage::FaceCompare);
            **face != next
        };
        {
        #[cfg(feature = "scan")]
        let _span = host.owner_span(OwnerStage::FaceReplace);
        if changed {
            *face = Arc::new(next);
            self.revision.fetch_add(1, Ordering::Release);
            #[cfg(feature = "scan")]
            host.owner_count(OwnerCounter::FaceChanges, 1);
            drop(face);
        } else {
            drop(face);
            drop(next);
        }
        }
    }
    pub fn edit(&self, id: ControlId, value: f64) -> bool {
        if !value.is_finite() || self.value(id).is_none() {
            return false;
        }
        if self.edits.try_send(UiRequest::Edit(id, value)).is_err() {
            return false;
        }
        self.owner.unpark();
        true
    }
    pub fn value(&self, id: ControlId) -> Option<f64> {
        let i = self.values.binary_search_by_key(&id, |(id, _)| *id).ok()?;
        Some(f64::from_bits(self.values[i].1.load(Ordering::Acquire)))
    }
    pub fn values(&self) -> Vec<(ControlId, f64)> {
        self.values
            .iter()
            .map(|(id, v)| (*id, f64::from_bits(v.load(Ordering::Acquire))))
            .collect()
    }
    pub fn interface(&self) -> Arc<Interface> {
        self.face.lock().unwrap().clone()
    }
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
