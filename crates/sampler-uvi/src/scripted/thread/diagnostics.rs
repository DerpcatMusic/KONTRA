//! Opt-in, seed-independent owner observations. No observer waits on Lua.
use super::*;
use std::sync::atomic::AtomicU8;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticPhase {
    Parked,
    Events,
    UiRequests,
    Advance,
    UiExport,
    Findings,
    Commands,
    Unknown,
}

/// Independent numeric observations, not a coherent owner completion snapshot.
/// Event counters start when monitoring is enabled; enable before the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticProgress {
    pub phase: DiagnosticPhase,
    pub events_enqueued: u64,
    pub events_rejected: u64,
    pub events_executed: u64,
    pub commands_collected: u64,
    pub commands_pushed: u64,
    pub commands_drained: u64,
    pub owner_acknowledged: u64,
    pub marker_drained: u64,
}

pub(super) struct Progress {
    phase: AtomicU8,
    pub(super) enqueued: AtomicU64,
    pub(super) rejected: AtomicU64,
    pub(super) executed: AtomicU64,
    pub(super) collected: AtomicU64,
    pub(super) pushed: AtomicU64,
    pub(super) drained: AtomicU64,
    acknowledged: AtomicU64,
    pub(super) marker_drained: AtomicU64,
    next: AtomicU64,
    occupied: AtomicBool,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            phase: AtomicU8::new(DiagnosticPhase::Unknown as u8),
            enqueued: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            executed: AtomicU64::new(0),
            collected: AtomicU64::new(0),
            pushed: AtomicU64::new(0),
            drained: AtomicU64::new(0),
            acknowledged: AtomicU64::new(0),
            marker_drained: AtomicU64::new(0),
            next: AtomicU64::new(1),
            occupied: AtomicBool::new(false),
        }
    }
}
impl Progress {
    pub(super) fn phase(&self, phase: DiagnosticPhase) {
        self.phase.store(phase as u8, Ordering::Release);
    }
    fn snapshot(&self) -> DiagnosticProgress {
        DiagnosticProgress {
            phase: match self.phase.load(Ordering::Acquire) {
                0 => DiagnosticPhase::Parked,
                1 => DiagnosticPhase::Events,
                2 => DiagnosticPhase::UiRequests,
                3 => DiagnosticPhase::Advance,
                4 => DiagnosticPhase::UiExport,
                5 => DiagnosticPhase::Findings,
                6 => DiagnosticPhase::Commands,
                _ => DiagnosticPhase::Unknown,
            },
            events_enqueued: self.enqueued.load(Ordering::Acquire),
            events_rejected: self.rejected.load(Ordering::Acquire),
            events_executed: self.executed.load(Ordering::Acquire),
            commands_collected: self.collected.load(Ordering::Acquire),
            commands_pushed: self.pushed.load(Ordering::Acquire),
            commands_drained: self.drained.load(Ordering::Acquire),
            owner_acknowledged: self.acknowledged.load(Ordering::Acquire),
            marker_drained: self.marker_drained.load(Ordering::Acquire),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticRequestError {
    Busy,
    QueueFull,
    OwnerStopped,
    SeededOwner,
    InvalidTimeout,
    InvalidGraphQuery,
}

/// Published after the owner drains earlier events, advances at the ordinary
/// audio clock, publishes UI/faults, and pushes its collected commands.
/// This is neither a future-coroutine barrier nor evidence of audible output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiagnosticAcknowledgement {
    pub request_id: u64,
    pub events_executed: u64,
    pub commands_collected: u64,
    pub commands_pushed: u64,
    pub ui_revision: u64,
    pub runtime_faults: u64,
    pub runtime_budgets: u64,
    pub next_due_ms: Option<f64>,
    pub samples: crate::script::SampleLoadStatus,
}
impl DiagnosticAcknowledgement {
    pub fn sample_counts_conserved(&self) -> bool {
        let s = self.samples;
        let pending = s
            .queued_metadata
            .checked_add(s.worker_active)
            .and_then(|n| n.checked_add(s.ready))
            .and_then(|n| n.checked_add(s.submitted));
        let total = s
            .successful
            .checked_add(s.failed)
            .and_then(|n| n.checked_add(s.cancelled))
            .and_then(|n| n.checked_add(s.pending));
        pending == Some(s.pending) && total == Some(s.requested)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DiagnosticPoll {
    Pending,
    Complete(DiagnosticAcknowledgement),
    TimedOut,
    Cancelled,
    OwnerStopped,
}

/// Host/background only. Poll is one try_recv, never a blocking receive.
/// Drop/cancel abandons this observation, not the owner's authored work.
#[derive(Debug)]
struct Reply {
    acknowledgement: DiagnosticAcknowledgement,
    graph: Option<Box<crate::script::LiveGraphReceipt>>,
}

pub struct DiagnosticRequest {
    id: u64,
    deadline: Instant,
    reply: mpsc::Receiver<Reply>,
    graph: Option<Box<crate::script::LiveGraphReceipt>>,
    cancelled: Arc<AtomicBool>,
    progress: Arc<Progress>,
    terminal: Option<DiagnosticPoll>,
}
impl DiagnosticRequest {
    pub fn id(&self) -> u64 {
        self.id
    }
    /// One-shot owner-model receipt; only present after a successful poll.
    pub fn take_live_graph(&mut self) -> Option<Box<crate::script::LiveGraphReceipt>> {
        if matches!(self.terminal, Some(DiagnosticPoll::Complete(_))) { self.graph.take() } else { None }
    }
    pub fn progress(&self) -> DiagnosticProgress {
        self.progress.snapshot()
    }
    pub fn commands_drained(&self) -> bool {
        self.progress.marker_drained.load(Ordering::Acquire) >= self.id
    }
    fn finish(&mut self, result: DiagnosticPoll) -> DiagnosticPoll {
        self.cancelled.store(true, Ordering::Release);
        self.progress.occupied.store(false, Ordering::Release);
        self.terminal = Some(result);
        result
    }
    pub fn poll(&mut self) -> DiagnosticPoll {
        if let Some(result) = self.terminal {
            return result;
        }
        // A caller that missed its deadline cannot turn a late reply into success.
        if Instant::now() >= self.deadline {
            return self.finish(DiagnosticPoll::TimedOut);
        }
        match self.reply.try_recv() {
            Ok(reply) => {
                self.graph = reply.graph;
                self.finish(DiagnosticPoll::Complete(reply.acknowledgement))
            },
            Err(mpsc::TryRecvError::Empty) => DiagnosticPoll::Pending,
            Err(mpsc::TryRecvError::Disconnected) => self.finish(DiagnosticPoll::OwnerStopped),
        }
    }
    pub fn cancel(&mut self) -> DiagnosticPoll {
        if let Some(result) = self.terminal {
            return result;
        }
        self.finish(DiagnosticPoll::Cancelled)
    }
}
impl Drop for DiagnosticRequest {
    fn drop(&mut self) {
        if self.terminal.is_none() {
            self.cancel();
        }
    }
}

pub(super) struct Pending {
    id: u64,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Reply>,
    query: Option<crate::script::LiveGraphQuery>,
    pub(super) eligible: bool,
}
impl Pending {
    pub(super) fn abandoned(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    pub(super) fn id(&self) -> u64 {
        self.id
    }
    pub(super) fn complete(self, host: &ScriptHost, ui: &UiBridge) {
        if self.abandoned() {
            return;
        }
        let p = ui.diagnostics.get().expect("diagnostic state installed");
        let (runtime_faults, runtime_budgets) = ui.runtime_faults();
        let ack = DiagnosticAcknowledgement {
            request_id: self.id,
            events_executed: p.executed.load(Ordering::Acquire),
            commands_collected: p.collected.load(Ordering::Acquire),
            commands_pushed: p.pushed.load(Ordering::Acquire),
            ui_revision: ui.revision(),
            runtime_faults,
            runtime_budgets,
            next_due_ms: host.next_due(),
            samples: host.sample_load_status(),
        };
        let graph = self.query.as_ref().map(|q| Box::new(host.observe_live_graph(q)));
        if self.abandoned() { return; }
        p.acknowledged.store(self.id, Ordering::Release);
        let _ = self.reply.try_send(Reply { acknowledgement: ack, graph });
    }
}

impl UiBridge {
    /// Off audio. Install fixed numeric state before sending the observed input.
    pub fn enable_diagnostics(&self) -> DiagnosticProgress {
        self.diagnostics
            .get_or_init(|| Arc::new(Progress::default()))
            .snapshot()
    }
    pub fn diagnostic_progress(&self) -> Option<DiagnosticProgress> {
        self.diagnostics.get().map(|p| p.snapshot())
    }
    pub(super) fn diagnostic_phase(&self, phase: DiagnosticPhase) {
        if let Some(p) = self.diagnostics.get() {
            p.phase(phase);
        }
    }
    /// One live request per owner; the existing service queue is also bounded.
    /// Seeded owners are refused because their separate blocking audit transport
    /// does not use the ordinary ordered command queue.
    pub fn request_diagnostics(
        &self,
        timeout: Duration,
    ) -> Result<DiagnosticRequest, DiagnosticRequestError> {
        self.request_diagnostics_inner(timeout, None)
    }
    /// Same ordinary owner/FIFO service, with a bounded optional model receipt.
    pub fn request_live_graph(
        &self, timeout: Duration, query: crate::script::LiveGraphQuery,
    ) -> Result<DiagnosticRequest, DiagnosticRequestError> {
        query.validate().map_err(|_| DiagnosticRequestError::InvalidGraphQuery)?;
        self.request_diagnostics_inner(timeout, Some(query))
    }
    fn request_diagnostics_inner(
        &self, timeout: Duration, query: Option<crate::script::LiveGraphQuery>,
    ) -> Result<DiagnosticRequest, DiagnosticRequestError> {
        #[cfg(feature = "scan")]
        if self.seeded {
            return Err(DiagnosticRequestError::SeededOwner);
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .filter(|_| !timeout.is_zero())
            .ok_or(DiagnosticRequestError::InvalidTimeout)?;
        self.enable_diagnostics();
        let progress = self.diagnostics.get().unwrap().clone();
        if progress
            .occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(DiagnosticRequestError::Busy);
        }
        let id = progress.next.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let (send, reply) = mpsc::sync_channel(1);
        let request = Pending {
            id,
            deadline,
            cancelled: cancelled.clone(),
            reply: send,
            eligible: false,
            query,
        };
        if let Err(error) = self.edits.try_send(UiRequest::Diagnostic(request)) {
            progress.occupied.store(false, Ordering::Release);
            return Err(match error {
                mpsc::TrySendError::Full(_) => DiagnosticRequestError::QueueFull,
                mpsc::TrySendError::Disconnected(_) => DiagnosticRequestError::OwnerStopped,
            });
        }
        self.owner.unpark();
        Ok(DiagnosticRequest {
            id,
            deadline,
            reply,
            cancelled,
            progress,
            terminal: None,
            graph: None,
        })
    }
}

#[cfg(test)]
mod live_graph_tests {
    use super::*;
    fn acknowledgement()->DiagnosticAcknowledgement {
        DiagnosticAcknowledgement {request_id:1,events_executed:0,commands_collected:0,commands_pushed:0,
            ui_revision:0,runtime_faults:0,runtime_budgets:0,next_due_ms:None,samples:Default::default()}
    }
    fn request()->(DiagnosticRequest,mpsc::SyncSender<Reply>) {
        let(send,reply)=mpsc::sync_channel(1);
        let progress=Arc::new(Progress::default());progress.occupied.store(true,Ordering::Release);
        (DiagnosticRequest{id:1,deadline:Instant::now()+Duration::from_secs(2),reply,
            graph:None,cancelled:Arc::new(AtomicBool::new(false)),progress,terminal:None},send)
    }
    #[test]
    fn copy_contract_and_one_shot_envelope() {
        fn is_copy<T:Copy>(){} is_copy::<DiagnosticAcknowledgement>();is_copy::<DiagnosticPoll>();
        let(mut r,send)=request();assert_eq!(r.poll(),DiagnosticPoll::Pending);
        assert!(r.take_live_graph().is_none());
        send.send(Reply{acknowledgement:acknowledgement(),graph:Some(Box::new(crate::script::LiveGraphReceipt{
            value:serde_json::json!({"status":"observed"})}))}).unwrap();
        assert!(matches!(r.poll(),DiagnosticPoll::Complete(_)));assert!(!r.commands_drained());
        r.progress.marker_drained.store(1,Ordering::Release);assert!(r.commands_drained());
        assert_eq!(r.take_live_graph().unwrap().value["status"],"observed");assert!(r.take_live_graph().is_none());
        assert!(matches!(r.poll(),DiagnosticPoll::Complete(_)));
    }
    #[test]
    fn cancellation_and_expired_reply_never_publish_graph() {
        let(mut r,send)=request();assert_eq!(r.cancel(),DiagnosticPoll::Cancelled);
        send.send(Reply{acknowledgement:acknowledgement(),graph:None}).unwrap();
        assert_eq!(r.poll(),DiagnosticPoll::Cancelled);assert!(r.take_live_graph().is_none());
        let(mut r,send)=request();r.deadline=Instant::now();
        send.send(Reply{acknowledgement:acknowledgement(),graph:None}).unwrap();
        assert_eq!(r.poll(),DiagnosticPoll::TimedOut);assert!(r.take_live_graph().is_none());
        assert!(!r.progress.occupied.load(Ordering::Acquire));
    }
}
