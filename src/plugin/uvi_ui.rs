//! Loader-owned native panel snapshots. Neither Lua nor artwork decoding runs
//! in the editor or audio callback.

use crate::{artwork::Picture, uvi::{host::UiSnapshot, program::NodeId,
    ui_assets::UiAssets, worker::{Stamp, Status, Worker}}};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone)]
pub(crate) struct Published {
    pub stamp: Stamp,
    pub snapshots: Arc<Vec<UiSnapshot>>,
    pub pictures: Arc<HashMap<String, Arc<Picture>>>,
}

pub(super) struct Mailbox {
    activation: Stamp,
    processors: Vec<NodeId>,
    discovered: bool,
    next: usize,
    pending: Option<(u64, NodeId)>,
    snapshots: Vec<UiSnapshot>,
    assets: Option<UiAssets>,
}

impl Mailbox {
    pub fn new(activation: Stamp, assets: Option<UiAssets>) -> Self {
        Self { activation, processors: Vec::new(), discovered: false, next: 0, pending: None,
            snapshots: Vec::new(), assets }
    }

    /// Called by the serialized loader. A reply from a superseded activation
    /// never becomes editor state. One processor is requested at a time so
    /// coalescing cannot permanently starve an earlier panel.
    pub fn poll(&mut self, worker: &Worker) -> Option<Arc<Published>> {
        if worker.status() != Status::Ready { return None; }
        if !self.discovered {
            self.processors = worker.ui_processors();
            self.discovered = true;
            if self.processors.is_empty() {
                return Some(Arc::new(Published { stamp: self.activation,
                    snapshots: Arc::default(), pictures: Arc::default() }));
            }
        }
        let mut published = None;
        if let Some(reply) = worker.poll_ui_snapshot() {
            let expected = self.pending.take();
            if expected == Some((reply.request, reply.processor))
                && reply.stamp.epoch == self.activation.epoch
                && reply.stamp.generation == self.activation.generation
                && let Ok(snapshot) = reply.snapshot
            {
                if let Some(previous) = self.snapshots.iter_mut().find(|s| s.processor == snapshot.processor) {
                    *previous = snapshot;
                } else {
                    self.snapshots.push(snapshot);
                    self.snapshots.sort_by_key(|s| self.processors.iter().position(|p| *p == s.processor));
                }
                let pictures = self.assets.as_mut().map(|a| a.refresh(&self.snapshots)).unwrap_or_default();
                published = Some(Arc::new(Published { stamp: reply.stamp,
                    snapshots: Arc::new(self.snapshots.clone()), pictures }));
            }
        }
        if self.pending.is_none() && !self.processors.is_empty() {
            let processor = self.processors[self.next % self.processors.len()];
            self.next = (self.next + 1) % self.processors.len();
            if let Ok(request) = worker.request_ui_snapshot(processor) {
                self.pending = Some((request, processor));
            }
        }
        published
    }
}
