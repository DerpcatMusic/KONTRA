//! NKA-only control worker. Audio only captures/applies through ControlClient.
use sampler_core::{ArrayFileCompletion, PlanId};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

type Job = (PlanId, ArrayFileCompletion);

pub(super) struct Worker {
    input: Option<SyncSender<Job>>,
    output: Option<Receiver<Job>>,
    pending: VecDeque<Job>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    pub fn new() -> std::io::Result<Self> {
        let (input, incoming) = sync_channel::<Job>(sampler_core::ARRAY_FILE_JOBS);
        let (outgoing, output) = sync_channel::<Job>(sampler_core::ARRAY_FILE_JOBS);
        let thread = std::thread::Builder::new()
            .name("kontra-nka".into())
            .spawn(move || {
                while let Ok((plan, mut output)) = incoming.recv() {
                    let _ = output.perform();
                    if outgoing.send((plan, output)).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            input: Some(input),
            output: Some(output),
            pending: VecDeque::new(),
            thread: Some(thread),
        })
    }
    pub fn submit(&mut self, plan: PlanId, output: ArrayFileCompletion) {
        // Pending ownership is bounded by the prepared generation's admitted jobs.
        self.pending.push_back((plan, output));
    }
    pub fn poll(&mut self) -> Option<Job> {
        while let Some(job) = self.pending.pop_front() {
            match self.input.as_ref()?.try_send(job) {
                Ok(()) => {}
                Err(TrySendError::Full(job) | TrySendError::Disconnected(job)) => {
                    self.pending.push_front(job);
                    break;
                }
            }
        }
        self.output.as_ref()?.try_recv().ok()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Release the reply channel first: a full queue cannot deadlock shutdown.
        self.output.take();
        self.input.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
