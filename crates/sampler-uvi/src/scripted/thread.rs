//! Lua on its own thread, for hosts whose audio thread must never wait on a
//! script. The audio side sends note events stamped with the audio clock and
//! applies the commands that come back at the next block, so a script reacts
//! within a block or two. Offline renders use [`ScriptHost`] inline instead:
//! a thread cannot be deterministic against a clock that runs faster than real
//! time.

use super::Script;
use crate::script::{Command, Config, Files, Finding, ScriptHost};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc,
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
}

/// What loading the scripts found, for the host's report and interface.
pub struct Loaded {
    pub findings: Vec<Finding>,
    pub interface: sampler_ui_ir::Interface,
}

/// The audio side of a script thread.
pub struct ScriptThread {
    events: rtrb::Producer<Message>,
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
        let (events, mut incoming) = rtrb::RingBuffer::new(QUEUE);
        let (mut outgoing, commands) = rtrb::RingBuffer::new(QUEUE);
        let clock = Arc::new(AtomicU64::new(0f64.to_bits()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, loaded) = mpsc::channel();
        let thread = Builder::new()
            .name("uvi-script".into())
            .spawn({
                let (clock, stop) = (clock.clone(), stop.clone());
                move || {
                    let mut host = match ScriptHost::new(&xml, files, config) {
                        Ok(host) => host,
                        Err(e) => return drop(ready.send(Err(e))),
                    };
                    let handles = host.handles_notes();
                    let report = Loaded {
                        findings: host.findings(),
                        interface: host.interface(),
                    };
                    let _ = ready.send(Ok((handles, report)));
                    drop(ready);
                    let mut backlog: Vec<Command> = Vec::new();
                    while !stop.load(Ordering::Acquire) {
                        while let Ok(message) = incoming.pop() {
                            match message {
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
                            }
                        }
                        host.advance(f64::from_bits(clock.load(Ordering::Acquire)));
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
                events,
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
