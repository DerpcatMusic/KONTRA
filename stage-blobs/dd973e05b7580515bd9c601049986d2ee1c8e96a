//! The editor's accessibility tree. Linux only: accesskit_unix speaks AT-SPI
//! over D-Bus and needs no window handle. Windows and macOS are not wired;
//! the crate README says why.

#[cfg(target_os = "linux")]
pub(crate) use linux::A11y;

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};

    use mui::{SemanticAction, Ui};
    use mui_access::accesskit::{
        Action, ActionData, ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler,
        TreeUpdate,
    };
    use mui_access::{Publisher, surface_of};

    pub(crate) struct A11y {
        adapter: accesskit_unix::Adapter,
        actions: Receiver<ActionRequest>,
        /// A reader asked for the tree: the next update must be whole.
        asked: Arc<AtomicBool>,
        publisher: Publisher,
    }

    /// Answers from the next tick, which holds the scene: the adapter
    /// thread cannot take the model lock.
    struct Asked(Arc<AtomicBool>);
    impl ActivationHandler for Asked {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.0.store(true, Ordering::Release);
            None
        }
    }
    struct Actions(Sender<ActionRequest>);
    impl ActionHandler for Actions {
        fn do_action(&mut self, request: ActionRequest) {
            // The editor closed: nobody to act for.
            let _ = self.0.send(request);
        }
    }
    struct Gone;
    impl DeactivationHandler for Gone {
        fn deactivate_accessibility(&mut self) {}
    }

    impl A11y {
        pub(crate) fn new() -> Self {
            let (send, actions) = channel();
            let asked = Arc::new(AtomicBool::new(false));
            let adapter =
                accesskit_unix::Adapter::new(Asked(Arc::clone(&asked)), Actions(send), Gone);
            Self {
                adapter,
                actions,
                asked,
                publisher: Publisher::default(),
            }
        }

        /// A reader is waiting for a tree: tick even if nothing moved.
        pub(crate) fn wants_tree(&self) -> bool {
            self.asked.load(Ordering::Acquire)
        }

        pub(crate) fn focus(&mut self, focused: bool) {
            self.adapter.update_window_focus_state(focused);
        }

        /// Hand the reader's requests to the `Ui`; whether any landed.
        pub(crate) fn apply(&mut self, ui: &mut Ui) -> bool {
            let mut landed = false;
            while let Ok(r) = self.actions.try_recv() {
                if let Some(action) = ui.scene().and_then(|scene| {
                    let key = surface_of(scene, r.target_node)?.key.to_string();
                    semantic(key, &r)
                }) {
                    landed |= ui.request_action(action);
                }
            }
            landed
        }

        /// This frame's tree, if a reader listens.
        // ponytail: bounds are window-relative; AT-SPI wants the window's
        // screen position (`set_root_window_bounds`), which baseview does not
        // report. Readers still get names, roles, values and focus.
        pub(crate) fn publish(&mut self, ui: &Ui) {
            let Some(scene) = ui.scene() else { return };
            if self.asked.swap(false, Ordering::AcqRel) {
                self.publisher.reset();
            }
            let (focus, scale) = (ui.focus_key(), ui.scale().unwrap_or(1.0));
            let publisher = &mut self.publisher;
            self.adapter
                .update_if_active(|| publisher.update(scene, focus, scale));
        }
    }

    fn semantic(key: String, r: &ActionRequest) -> Option<SemanticAction> {
        Some(match (r.action, &r.data) {
            (Action::Focus, _) => SemanticAction::focus(key),
            (Action::Click, _) => SemanticAction::activate(key),
            (Action::Increment, _) => SemanticAction::increment(key),
            (Action::Decrement, _) => SemanticAction::decrement(key),
            (Action::SetValue, Some(ActionData::NumericValue(v))) => {
                SemanticAction::set_value(key, *v)
            }
            (Action::SetTextSelection, Some(ActionData::SetTextSelection(s))) => {
                SemanticAction::set_selection(
                    key,
                    s.anchor.character_index,
                    s.focus.character_index,
                )
            }
            _ => return None,
        })
    }
}

/// No adapter off Linux.
#[cfg(not(target_os = "linux"))]
pub(crate) struct A11y;

#[cfg(not(target_os = "linux"))]
impl A11y {
    pub(crate) fn new() -> Self {
        Self
    }
    pub(crate) fn wants_tree(&self) -> bool {
        false
    }
    pub(crate) fn focus(&mut self, _: bool) {}
    pub(crate) fn apply(&mut self, _: &mut mui::Ui) -> bool {
        false
    }
    pub(crate) fn publish(&mut self, _: &mui::Ui) {}
}
