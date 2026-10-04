//! Loader-owned native panel snapshots. Neither Lua nor artwork decoding runs
//! in the editor or audio callback.

use crate::{artwork::Picture, uvi::{host::UiSnapshot, program::NodeId,
    ui_assets::UiAssets, worker::{Stamp, Status, Worker}}};
use std::{collections::{BTreeMap, HashMap}, sync::Arc};

/// Conservative processor-local declarations; native global write order is
/// unknown. Conflicts stay unknown until the retained roots agree again.
#[derive(Default)]
pub(crate) struct KeyColours {
    pub colours: BTreeMap<u8, String>,
    pub conflicts: usize,
}

impl KeyColours {
    fn merge(snapshots: &[UiSnapshot]) -> Self {
        let mut keys: [Option<&str>; 128] = [None; 128];
        let mut conflicts = [false; 128];
        for map in snapshots.iter().filter_map(|s| s.root.key_colours.as_ref()) {
            for (&note, colour) in map {
                let index = usize::from(note);
                if index >= 128 { continue; }
                if keys[index].is_some_and(|old| old != colour) { conflicts[index] = true; }
                else { keys[index] = Some(colour); }
            }
        }
        Self {
            colours: keys.into_iter().enumerate().filter_map(|(note, colour)|
                (!conflicts[note]).then_some(colour).flatten().map(|colour| (note as u8, colour.to_owned()))).collect(),
            conflicts: conflicts.into_iter().filter(|&conflict| conflict).count(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Published {
    pub stamp: Stamp,
    pub snapshots: Arc<Vec<UiSnapshot>>,
    pub key_colours: Arc<KeyColours>,
    pub pictures: Arc<HashMap<String, Arc<Picture>>>,
    pub fonts: Arc<HashMap<String, moose::mui::mui::prelude::Font>>,
}

pub(super) struct Mailbox {
    activation: Stamp,
    processors: Vec<NodeId>,
    discovered: bool,
    initialized: bool,
    next: usize,
    pending: Option<(u64, NodeId)>,
    snapshots: Arc<Vec<UiSnapshot>>,
    key_colours: Arc<KeyColours>,
    pictures: Arc<HashMap<String, Arc<Picture>>>,
    fonts: Arc<HashMap<String, moose::mui::mui::prelude::Font>>,
    published_frame: u64,
    assets: Option<UiAssets>,
}

impl Mailbox {
    pub fn new(activation: Stamp, assets: Option<UiAssets>) -> Self {
        Self { activation, processors: Vec::new(), discovered: false, initialized: false, next: 0, pending: None,
            snapshots: Arc::default(), key_colours: Arc::default(), pictures: Arc::default(), fonts: Arc::default(),
            published_frame: activation.frame, assets }
    }

    /// Reuse immutable owned data on an unchanged reply, while advancing its
    /// acknowledgement frame: the editor needs that frame to settle an edit
    /// whose callback rejected or restored the same visible value.
    fn accept(&mut self, stamp: Stamp, snapshot: UiSnapshot) -> Option<Arc<Published>> {
        let position = self.snapshots.iter().position(|s| s.processor == snapshot.processor);
        let changed = position.is_none_or(|index| self.snapshots[index] != snapshot);
        if changed {
            let snapshots = Arc::make_mut(&mut self.snapshots);
            if let Some(index) = position { snapshots[index] = snapshot; }
            else {
                snapshots.push(snapshot);
                snapshots.sort_by_key(|s| self.processors.iter().position(|p| *p == s.processor));
            }
            self.key_colours = Arc::new(KeyColours::merge(&self.snapshots));
            self.pictures = self.assets.as_mut().map(|a| a.refresh(&self.snapshots)).unwrap_or_default();
            self.fonts = self.assets.as_ref().map(UiAssets::fonts).unwrap_or_default();
        }
        if !changed && stamp.frame <= self.published_frame { return None; }
        self.published_frame = stamp.frame;
        Some(Arc::new(Published { stamp, snapshots: self.snapshots.clone(), key_colours: self.key_colours.clone(),
            pictures: self.pictures.clone(), fonts: self.fonts.clone() }))
    }

    /// Called by the serialized loader. A reply from a superseded activation
    /// never becomes editor state. One processor is requested at a time so
    /// coalescing cannot permanently starve an earlier panel.
    pub fn poll(&mut self, worker: &Worker) -> Option<Arc<Published>> {
        if !self.initialized
            && let Some((stamp, snapshots)) = worker.initialized_ui()
            && (stamp.epoch, stamp.generation) == (self.activation.epoch, self.activation.generation)
        {
            self.initialized = true;
            self.snapshots = snapshots.clone();
            self.key_colours = Arc::new(KeyColours::merge(&self.snapshots));
            self.pictures = self.assets.as_mut().map(|a| a.refresh(&self.snapshots)).unwrap_or_default();
            if matches!(worker.status(), Status::Failed | Status::Stopped) { return None; }
            self.fonts = self.assets.as_ref().map(UiAssets::fonts).unwrap_or_default();
            self.published_frame = stamp.frame;
            return Some(Arc::new(Published { stamp, snapshots, key_colours: self.key_colours.clone(), pictures: self.pictures.clone(), fonts: self.fonts.clone() }));
        }
        if worker.status() != Status::Ready { return None; }
        if !self.discovered {
            self.processors = worker.ui_processors();
            self.discovered = true;
            if self.processors.is_empty() {
                return Some(Arc::new(Published { stamp: self.activation,
                    snapshots: Arc::default(), key_colours: Arc::default(), pictures: Arc::default(), fonts: Arc::default() }));
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
                published = self.accept(reply.stamp, snapshot);
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

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(processor: NodeId, width: f64) -> UiSnapshot {
        UiSnapshot { processor, root: crate::uvi::host::UiRoot {
            width, height: 480., performance_view: true, background: None, background_colour: None,
            key_colours: None,
        }, widgets: Vec::new(), paint_order: Vec::new() }
    }

    #[test]
    fn unchanged_replies_share_data_but_keep_fresh_edit_acknowledgements() {
        let activation = Stamp { epoch: 7, generation: 11, frame: 0 };
        let mut mailbox = Mailbox::new(activation, None);
        mailbox.processors = vec![3];
        let first = mailbox.accept(activation, snapshot(3, 720.)).unwrap();
        let next = Stamp { frame: 256, ..activation };
        let acknowledged = mailbox.accept(next, snapshot(3, 720.)).unwrap();
        assert_eq!(acknowledged.stamp, next, "A reverted/unchanged edit must still settle");
        assert!(Arc::ptr_eq(&first.snapshots, &acknowledged.snapshots));
        assert!(Arc::ptr_eq(&first.key_colours, &acknowledged.key_colours));
        assert!(Arc::ptr_eq(&first.pictures, &acknowledged.pictures));
        assert!(Arc::ptr_eq(&first.fonts, &acknowledged.fonts));
        assert!(mailbox.accept(next, snapshot(3, 720.)).is_none());
    }

    #[test]
    fn changed_replies_replace_owned_data_without_mutating_the_old_reader() {
        let activation = Stamp { epoch: 7, generation: 11, frame: 0 };
        let mut mailbox = Mailbox::new(activation, None);
        mailbox.processors = vec![3, 4];
        let first = mailbox.accept(activation, snapshot(3, 720.)).unwrap();
        let changed = mailbox.accept(activation, snapshot(3, 360.)).unwrap();
        assert!(!Arc::ptr_eq(&first.snapshots, &changed.snapshots));
        assert_eq!(first.snapshots[0].root.width, 720.);
        assert_eq!(changed.snapshots[0].root.width, 360.);
        assert!(mailbox.accept(activation, snapshot(4, 720.)).is_some());
        assert_eq!(mailbox.snapshots.iter().map(|s| s.processor).collect::<Vec<_>>(), [3, 4]);
    }

    fn keys(processor: NodeId, declarations: &[(u8, &str)]) -> UiSnapshot {
        let mut snapshot = snapshot(processor, 720.);
        snapshot.root.key_colours = Some(declarations.iter().map(|&(note, colour)| (note, colour.into())).collect());
        snapshot
    }

    #[test]
    fn keyboard_merge_keeps_equal_declarations_and_makes_all_conflicts_unknown() {
        let a = keys(3, &[(0, "red"), (58, "#00FFFFFF"), (59, "#00FFFFFF"), (60, "#00FFFFFF")]);
        let b = keys(4, &[(0, "blue"), (58, "#00FFFFFF"), (59, "#00000000"), (60, "yellow")]);
        let first = KeyColours::merge(&[a.clone(), b.clone()]);
        let reversed = KeyColours::merge(&[b, a]);
        assert_eq!(first.colours, BTreeMap::from([(58, "#00FFFFFF".into())]));
        assert_eq!(first.conflicts, 3);
        assert_eq!(first.colours, reversed.colours);
        assert_eq!(first.conflicts, reversed.conflicts);
    }

    #[test]
    fn keyboard_refresh_rebuilds_current_roots_and_reset_removes_old_declarations() {
        let activation = Stamp { epoch: 7, generation: 11, frame: 0 };
        let mut mailbox = Mailbox::new(activation, None);
        mailbox.processors = vec![3, 4];
        let old = mailbox.accept(activation, keys(3, &[(58, "#00FFFFFF"), (0, "red")])).unwrap();
        let conflict = mailbox.accept(activation, keys(4, &[(58, "#00000000")])).unwrap();
        assert_eq!(conflict.key_colours.conflicts, 1);
        assert!(!conflict.key_colours.colours.contains_key(&58));
        let reset = mailbox.accept(activation, keys(4, &[])).unwrap();
        assert_eq!(reset.key_colours.conflicts, 0);
        assert_eq!(reset.key_colours.colours[&58], "#00FFFFFF");
        let cleared = mailbox.accept(activation, keys(3, &[])).unwrap();
        assert!(cleared.key_colours.colours.is_empty());
        assert_eq!(old.key_colours.colours[&0], "red", "an owned old publication stays immutable");
        assert_eq!(old.key_colours.colours[&58], "#00FFFFFF");
        let full = keys(3, &(0..128).map(|note| (note, "blue")).collect::<Vec<_>>());
        assert_eq!(KeyColours::merge(&[full]).colours.len(), 128);
    }

    #[test]
    fn initialized_mailbox_rejects_wrong_epoch_and_generation() {
        let (config, _) = crate::uvi::worker::tests::authored_bank_with_script(
            "function onInit()knob=Knob('initialized',0.5,0,1)end");
        let path = config.bank.clone();
        let mut worker = Worker::start(config, 7, 11).unwrap();
        worker.wait_ready(std::time::Duration::from_secs(5)).unwrap();
        for activation in [Stamp { epoch: 6, generation: 11, frame: 0 },
                           Stamp { epoch: 7, generation: 10, frame: 0 }] {
            let mut mailbox = Mailbox::new(activation, None);
            assert!(mailbox.poll(&worker).is_none());
        }
        let mut mailbox = Mailbox::new(worker.activation_stamp(), None);
        let published = mailbox.poll(&worker).unwrap();
        assert_eq!(published.snapshots.len(), 1);
        assert_eq!(published.snapshots[0].widgets.len(), 1);
        worker.stop();
        assert!(mailbox.poll(&worker).is_none());
        std::fs::remove_file(path).unwrap();
    }
}
