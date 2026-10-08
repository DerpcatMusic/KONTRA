//! Lua on its own thread, for hosts whose audio thread must never wait on a
//! script. The audio side sends note events stamped with the audio clock and
//! applies the commands that come back at the next block, so a script reacts
//! within a block or two. Offline renders use [`ScriptHost`] inline instead:
//! a thread cannot be deterministic against a clock that runs faster than real
//! time.

use super::{HostInput, Script};
use crate::script::{Command, Config, Files, Finding, ScriptHost, UiState};
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
                    let mut host =
                        match ScriptHost::new_with_ui_state(&xml, files, config, state.as_ref()) {
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
                        findings: host.findings(),
                        interface: host.interface(),
                        ui: ui.clone(),
                    };
                    let _ = ready.send(Ok((handles, report)));
                    drop(ready);
                    let mut backlog: Vec<Command> = Vec::new();
                    let mut revision = host.ui_revision();
                    while !stop.load(Ordering::Acquire) {
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
                                    Script::input(&mut host, input);
                                }
                            }
                        }
                        while let Ok((id, value)) = ui_receive.try_recv() {
                            let _ = host.set_control(id, value);
                        }
                        host.advance(f64::from_bits(clock.load(Ordering::Acquire)));
                        let current = host.ui_revision();
                        if current != revision {
                            ui.publish(&host);
                            revision = current;
                        }
                        #[cfg(feature = "scan")]
                        { *scan.lock().unwrap() = host.scan_faults(); }
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
        let _ = self.events.push(Message::On {
            id,
            key,
            velocity,
            at_ms,
        });
        self.wake();
    }

    fn note_off(&mut self, id: u64, key: u8) {
        let at_ms = self.time_ms;
        let _ = self.events.push(Message::Off { id, key, at_ms });
        self.wake();
    }

    fn input(&mut self, input: HostInput) {
        let at_ms = self.time_ms;
        let _ = self.events.push(Message::In { input, at_ms });
        self.wake();
    }

    /// The thread advances itself from the clock.
    fn advance(&mut self, _ms: f64) {}

    fn next_due(&mut self) -> Option<f64> {
        None
    }

    fn drain(&mut self, out: &mut Vec<Command>) {
        while let Ok(command) = self.commands.pop() {
            out.push(command);
        }
    }

    fn tick(&mut self, now_ms: f64) {
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
    edits: mpsc::SyncSender<(ControlId, f64)>,
    owner: std::thread::Thread,
    values: Vec<(ControlId, AtomicU64)>,
    face: Mutex<Arc<Interface>>,
    state: Mutex<Result<UiState, String>>,
    revision: AtomicU64,
}
impl UiBridge {
    fn new(
        host: &ScriptHost,
        edits: mpsc::SyncSender<(ControlId, f64)>,
        owner: std::thread::Thread,
    ) -> Self {
        let mut values = host.control_values();
        values.sort_by_key(|(id, _)| *id);
        Self {
            edits,
            owner,
            values: values
                .into_iter()
                .map(|(id, v)| (id, AtomicU64::new(v.to_bits())))
                .collect(),
            face: Mutex::new(Arc::new(host.interface())),
            state: Mutex::new(host.save_ui_state()),
            revision: AtomicU64::new(1),
        }
    }
    fn publish(&self, host: &ScriptHost) {
        *self.state.lock().unwrap() = host.save_ui_state();
        for (id, v) in host.control_values() {
            if let Ok(i) = self.values.binary_search_by_key(&id, |(id, _)| *id) {
                self.values[i].1.store(v.to_bits(), Ordering::Release);
            }
        }
        let next = host.interface();
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
        if self.edits.try_send((id, value)).is_err() {
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
    pub fn state(&self) -> Result<UiState, String> {
        self.state.lock().unwrap().clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
}
