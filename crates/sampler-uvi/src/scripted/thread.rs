//! Lua on its own thread, for hosts whose audio thread must never wait on a
//! script. The audio side sends note events stamped with the audio clock and
//! applies the commands that come back at the next block, so a script reacts
//! within a block or two. Offline renders use [`ScriptHost`] inline instead:
//! a thread cannot be deterministic against a clock that runs faster than real
//! time. Scan builds may explicitly opt into a seeded owner-thread barrier.

use super::{HostInput, Script};
#[cfg(feature = "scan")]
use crate::script::diagnostics::{OwnerPhase, ScanProgress};
use crate::script::{Command, Config, Files, Finding, FaultCounts, FaultCategory, ScriptHost, UiState};
use sampler_ui_ir::{ControlId, Interface};
use std::{
    sync::{
        Arc, Mutex,
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
    commands: rtrb::Consumer<Command>,
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
    Edit(ControlId, f64),
    Save(mpsc::SyncSender<Result<UiState, String>>),
    #[cfg(feature = "scan")]
    Audit(f64, mpsc::SyncSender<(Vec<Command>, Option<f64>)>),
}

#[cfg(feature = "scan")]
type AuditReply = (mpsc::SyncSender<(Vec<Command>, Option<f64>)>, Vec<Command>, Option<f64>, f64);

fn process_events(host: &mut ScriptHost, incoming: &mut rtrb::Consumer<Message>) {
    while let Ok(message) = incoming.pop() {
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
            Message::Off { id, key, at_ms } => {
                host.set_time(at_ms);
                host.note_off(id, key, 64, 0);
            }
            Message::In { input, at_ms } => {
                host.set_time(at_ms);
                Script::input(host, input);
            }
        }
    }
}

fn process_ui_requests(host: &mut ScriptHost, requests: &mpsc::Receiver<UiRequest>, _incoming: &mut rtrb::Consumer<Message>, #[cfg(feature = "scan")] audit_reply: &mut Option<AuditReply>) {
    while let Ok(request) = requests.try_recv() {
        match request {
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
                process_events(host, _incoming);
                host.owner_phase(OwnerPhase::Advance);
                host.advance(ms);
                *audit_reply = Some((reply, host.take_commands(), host.next_due(), ms));
                // Complete this barrier after publication, before later UI requests.
                break;
            }
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
                    let mut host = match ScriptHost::new_with_ui_state(&xml, files, config, state.as_ref()) {
                        Ok(host) => host,
                        Err(e) => {
                            #[cfg(feature = "scan")]
                            crate::script::scan_failed_load(&e);
                            return drop(ready.send(Err(e)));
                        }
                    };
                    let handles = host.handles_notes();
                    let ui = Arc::new(UiBridge::new(&host, ui_send, std::thread::current()));
                    #[cfg(feature = "scan")]
                    { *scan.lock().unwrap() = host.scan_faults(); }
                    let report = Loaded {
                        insert_overrides: host.insert_overrides(),
                        findings: host.findings(),
                        interface: ui.interface().as_ref().clone(),
                        ui: ui.clone(),
                    };
                    let _ = ready.send(Ok((handles, report)));
                    drop(ready);
                    let mut backlog: Vec<Command> = Vec::new();
                    let mut revision = host.ui_revision();
                    let mut finding_revision = host.finding_revision();
                    while !stop.load(Ordering::Acquire) {
                        #[cfg(feature = "scan")]
                        let mut audit_reply = None;
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::Events);
                        process_events(&mut host, &mut incoming);
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::UiRequests);
                        process_ui_requests(&mut host, &ui_receive, &mut incoming, #[cfg(feature = "scan")] &mut audit_reply);
                        #[cfg(feature = "scan")]
                        if !audit { host.advance(f64::from_bits(clock.load(Ordering::Acquire))); }
                        #[cfg(not(feature = "scan"))]
                        host.advance(f64::from_bits(clock.load(Ordering::Acquire)));
                        #[cfg(feature = "scan")]
                        host.owner_phase(OwnerPhase::Publish);
                        let current = host.ui_revision();
                        if current != revision {
                            ui.publish(&host);
                            revision = current;
                        }
                        if host.finding_revision() != finding_revision {
                            ui.publish_findings(&host);
                            finding_revision = host.finding_revision();
                        }
                        #[cfg(feature = "scan")]
                        { *scan.lock().unwrap() = host.scan_faults(); }
                        #[cfg(feature = "scan")]
                        if let Some((reply, commands, due, ms)) = audit_reply.take() {
                            if let Some(progress) = ui.scan_progress() { progress.complete(ms); }
                            let _ = reply.send((commands, due));
                        }
                        #[cfg(feature = "scan")]
                        if audit {
                            host.owner_phase(OwnerPhase::Parked);
                            std::thread::park();
                            continue;
                        }
                        backlog.extend(host.take_commands());
                        for command in backlog.drain(..) {
                            // A full queue only delays: the audio side drains it every block.
                            let mut command = command;
                            loop {
                                match outgoing.push(command) {
                                    Ok(()) => break,
                                    Err(rtrb::PushError::Full(back)) => {
                                        command = back;
                                        if stop.load(Ordering::Acquire) {
                                            return;
                                        }
                                        process_ui_requests(&mut host, &ui_receive, &mut incoming, #[cfg(feature = "scan")] &mut audit_reply);
                                        std::thread::sleep(POLL);
                                    }
                                }
                            }
                        }
                        if host.next_due().is_some() {
                            std::thread::park_timeout(POLL);
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
        if ok {
            self.wake();
        }
        ok
    }
    #[cfg(feature = "scan")]
    pub fn scan_faults(&self) -> crate::script::ScanFaults { self.scan.lock().unwrap().clone() }

    #[cfg(feature = "scan")]
    fn synchronize_audit(&mut self, out: &mut Vec<Command>) {
        let (reply, received) = mpsc::sync_channel(1);
        if let Some(progress) = self.ui.scan_progress() { progress.request(self.time_ms); }
        self.ui.edits.send(UiRequest::Audit(self.time_ms, reply)).expect("audit script owner stopped");
        self.wake();
        let (commands, due) = received.recv().expect("audit script owner stopped");
        out.extend(commands);
        self.audit_due = due;
    }

    fn enqueue(&mut self, mut message: Message) {
        loop {
            match self.events.push(message) {
                Ok(()) => { self.wake(); return; }
                Err(rtrb::PushError::Full(back)) => {
                    message = back;
                    #[cfg(feature = "scan")]
                    if self.audit {
                        let mut commands = std::mem::take(&mut self.audit_pending);
                        self.synchronize_audit(&mut commands);
                        self.audit_pending = commands;
                        continue;
                    }
                    self.wake();
                    return;
                }
            }
        }
    }

    fn wake(&self) {
        if let Some(thread) = &self.thread {
            thread.thread().unpark();
        }
    }
}

impl Script for ScriptThread {
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
        self.enqueue(Message::Off { id, key, at_ms });
    }

    fn input(&mut self, input: HostInput) {
        let at_ms = self.time_ms;
        self.enqueue(Message::In { input, at_ms });
    }

    /// The thread advances itself from the clock.
    fn advance(&mut self, _ms: f64) {
        #[cfg(feature = "scan")]
        if self.audit { self.time_ms = _ms; }
    }

    fn next_due(&mut self) -> Option<f64> {
        #[cfg(feature = "scan")]
        if self.audit { return self.audit_due; }
        None
    }

    fn drain(&mut self, out: &mut Vec<Command>) {
        #[cfg(feature = "scan")]
        if self.audit {
            out.append(&mut self.audit_pending);
            self.synchronize_audit(out);
            return;
        }
        while let Ok(command) = self.commands.pop() {
            out.push(command);
        }
    }

    fn tick(&mut self, now_ms: f64) {
        #[cfg(feature = "scan")]
        if self.audit { self.time_ms = now_ms; }
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
    ) -> Self {
        let _ = host.save_ui_state();
        let face = host.interface();
        let mut values = ScriptHost::control_values_from(&face);
        values.sort_by_key(|(id, _)| *id);
        Self {
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
        self.runtime_faults.store(faults.runtime.values().copied().fold(0u64,u64::saturating_add), Ordering::Release);
        self.runtime_budgets.store(faults.runtime.get(&FaultCategory::Budget).copied().unwrap_or(0), Ordering::Release);
        *self.faults.lock().unwrap() = faults;
        self.revision.fetch_add(1, Ordering::Release);
    }
    pub fn findings(&self) -> Vec<Finding> { self.findings.lock().unwrap().clone() }
    /// Numeric progress only; this does not acquire a UI or Lua owner lock.
    #[cfg(feature = "scan")]
    pub fn scan_progress(&self) -> Option<Arc<ScanProgress>> { self.progress.clone() }
    pub fn fault_counts(&self) -> FaultCounts { self.faults.lock().unwrap().clone() }
    /// Lock-free counters for the audio host's cumulative runtime report.
    pub fn runtime_faults(&self) -> (u64,u64) {
        (self.runtime_faults.load(Ordering::Acquire), self.runtime_budgets.load(Ordering::Acquire))
    }
    fn publish(&self, host: &ScriptHost) {
        let next = host.interface();
        for (id, v) in ScriptHost::control_values_from(&next) {
            if let Ok(i) = self.values.binary_search_by_key(&id, |(id, _)| *id) {
                self.values[i].1.store(v.to_bits(), Ordering::Release);
            }
        }
        let mut face = self.face.lock().unwrap();
        if **face != next {
            *face = Arc::new(next);
            self.revision.fetch_add(1, Ordering::Release);
        }
        drop(face);
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
        self.edits.send(UiRequest::Save(reply))
            .map_err(|_| "UVI save worker stopped".to_string())?;
        self.owner.unpark();
        state.recv_timeout(Duration::from_secs(2))
            .map_err(|_| "UVI save worker did not reply".to_string())?
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}
