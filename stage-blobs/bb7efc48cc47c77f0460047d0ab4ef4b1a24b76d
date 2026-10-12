use crate::platform::X11Connection;
use std::cell::{Cell, RefCell};
use std::num::NonZeroU32;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::{ConnectionExt, MapState, QueryTreeReply};
use x11rb::protocol::ErrorKind;
use x11rb::x11_utils::X11Error;

pub enum AncestorVisibilityState {
    Floating {
        own_window_viewable: Cell<bool>,
        own_window_id: NonZeroU32,
    },
    Parented {
        ancestry: AncestryList,
        root_id: Cell<Option<NonZeroU32>>,
        own_window_viewable: Cell<bool>,
    },
}

pub struct AncestryList {
    inner: RefCell<Vec<Ancestor>>,
}

impl AncestryList {
    pub fn new(own_window: NonZeroU32) -> Self {
        Self { inner: RefCell::new(vec![Ancestor { id: own_window, mapped: false.into() }]) }
    }

    pub fn pop_id(&self) -> Option<NonZeroU32> {
        self.inner.borrow_mut().pop().map(|a| a.id)
    }

    pub fn last_id(&self) -> Option<NonZeroU32> {
        self.inner.borrow().last().map(|a| a.id)
    }

    pub fn push(&self, ancestor: Ancestor) {
        self.inner.borrow_mut().push(ancestor);
    }

    pub fn parent_id(&self) -> Option<NonZeroU32> {
        self.inner.borrow().get(1).map(|a| a.id)
    }
    pub fn own_window_id(&self) -> Option<NonZeroU32> {
        self.inner.borrow().first().map(|a| a.id)
    }

    pub fn remove_window(&self, id: NonZeroU32) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(index) = inner.iter().position(|a| a.id == id) else {
            return false;
        };

        inner.truncate(index.saturating_add(1));

        true
    }

    pub fn remove_after_window(&self, id: NonZeroU32) -> bool {
        let mut inner = self.inner.borrow_mut();
        let Some(index) = inner.iter().position(|a| a.id == id) else {
            return false;
        };

        // Retain the reparented window and remove its old ancestry. Keeping
        // the old parent makes an unmapped former parent hide a viewable child.
        inner.truncate(index.saturating_add(1));

        true
    }

    pub fn check_all_mapped(&self) -> bool {
        self.inner.borrow().iter().all(|a| a.mapped.get())
    }

    pub fn set_mapped(&self, window: NonZeroU32, mapped: bool) -> bool {
        let inner = self.inner.borrow();
        let Some(ancestor) = inner.iter().find(|a| a.id == window) else {
            return false;
        };

        ancestor.mapped.set(mapped);
        true
    }
}

#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Ancestor {
    id: NonZeroU32,
    mapped: Cell<bool>,
}

impl AncestorVisibilityState {
    pub fn discover(
        connection: &X11Connection, own_window_id: NonZeroU32, parented: bool,
    ) -> Result<Self, ReplyError> {
        if !parented {
            return Ok(Self::Floating { own_window_viewable: Cell::new(false), own_window_id });
        }

        let this = Self::Parented {
            ancestry: AncestryList::new(own_window_id),
            own_window_viewable: Cell::new(false),
            root_id: Cell::new(NonZeroU32::new(connection.default_screen().root)),
        };

        this.try_regenerate_from_last_window(connection)?;

        Ok(this)
    }

    pub fn own_window_is_viewable(&self) -> bool {
        match self {
            Self::Parented { own_window_viewable, .. } => own_window_viewable.get(),
            Self::Floating { own_window_viewable, .. } => own_window_viewable.get(),
        }
    }

    pub fn parent_id(&self) -> Option<NonZeroU32> {
        match self {
            Self::Parented { ancestry, .. } => ancestry.parent_id(),
            _ => None,
        }
    }

    /// Returns `true` if this operation made our own window visible.
    pub fn window_mapped(&self, window_id: NonZeroU32) -> bool {
        match self {
            Self::Floating { own_window_id, own_window_viewable } => {
                if *own_window_id != window_id {
                    return false;
                }

                if own_window_viewable.get() {
                    return false;
                }

                own_window_viewable.set(true);
                true
            }
            Self::Parented { own_window_viewable, ancestry, .. } => {
                if !ancestry.set_mapped(window_id, true) {
                    return false;
                }

                if own_window_viewable.get() {
                    return false;
                }

                let all_mapped = ancestry.check_all_mapped();
                if all_mapped {
                    own_window_viewable.set(true);
                }

                all_mapped
            }
        }
    }

    pub fn window_unmapped(&self, window_id: NonZeroU32) {
        match self {
            Self::Floating { own_window_id, own_window_viewable } => {
                if *own_window_id != window_id {
                    return;
                }

                own_window_viewable.set(false);
            }
            Self::Parented { own_window_viewable, ancestry, .. } => {
                if !ancestry.set_mapped(window_id, false) {
                    return;
                }

                own_window_viewable.set(false);
            }
        }
    }

    pub fn window_destroyed(&self, window_id: NonZeroU32, connection: &X11Connection) {
        let Self::Parented { ancestry, .. } = &self else {
            return;
        };

        if !ancestry.remove_window(window_id) {
            return;
        }

        self.regenerate_from_last_window(connection);
    }

    pub fn window_reparented(
        &self, window_id: NonZeroU32, new_parent: Option<NonZeroU32>, connection: &X11Connection,
    ) {
        let Self::Parented { ancestry, root_id, .. } = &self else {
            return;
        };

        if !ancestry.remove_after_window(window_id) {
            return;
        }

        if let Some(new_parent) = new_parent {
            if Some(new_parent) == root_id.get() {
                // Root is not an ancestry entry, but mapping state and cached
                // visibility still need refreshing after this reparent.
                self.regenerate_from_last_window(connection);
                return;
            }

            ancestry.push(Ancestor { id: new_parent, mapped: Cell::new(false) });

            self.regenerate_from_last_window(connection);
        }
    }

    pub fn regenerate_from_last_window(&self, connection: &X11Connection) {
        if let Err(e) = self.try_regenerate_from_last_window(connection) {
            crate::warn!("Failed to generate window ancestry list: {}", e)
        }
    }

    fn try_regenerate_from_last_window(
        &self, connection: &X11Connection,
    ) -> Result<(), ReplyError> {
        let Self::Parented { ancestry, own_window_viewable, root_id } = &self else {
            return Ok(());
        };

        let Some(mut current_window) = ancestry.pop_id() else { return Ok(()) };

        let mut shitlist = Vec::new();
        let mut rechecked_children = Vec::new();

        loop {
            // Subscribe before taking the snapshot: a host can map an ancestor
            // while its own parent has not yet been visited. StructureNotify
            // observes that ancestor directly, closing the discovery race.
            // The child already has its input mask; never replace it here.
            let mut subscription_failed = false;
            if ancestry.own_window_id().is_some_and(|id| id != current_window) {
                if let Err(e) =
                    connection.register_tree_structure_events_for_window(current_window)?.check()
                {
                    crate::warn!(
                        "Could not register structure events for window {}: {}",
                        current_window,
                        e
                    );
                    subscription_failed = true;
                }
            }
            let Some((mut mapped, tree)) = fetch_window_info(connection, current_window)? else {
                // We got a BadWindow while trying to get a window's info, it must have been destroyed.
                // Try to go back a layer and fetch the window's state and parent again

                crate::warn!("Failed to get info for window {}: XBadWindow", current_window);

                let Some(previous_parent) = ancestry.pop_id() else {
                    // No previous parent, this was the first window. Stop everything and return an empty state
                    break;
                };

                if shitlist.contains(&previous_parent) {
                    crate::warn!(
                        "Failed to get info for window {} in the past already. Stopping.",
                        previous_parent
                    );
                    break;
                }

                if shitlist.len() > 10 {
                    crate::warn!(
                        "Too many failures while trying to build X ancestry tree. Stopping."
                    );
                    break;
                }

                shitlist.push(previous_parent);

                current_window = previous_parent;
                continue;
            };

            #[cfg(test)]
            tests::after_snapshot(current_window);

            if subscription_failed {
                // Preserve the existing fallback when events cannot be selected.
                mapped = true;
            }

            if tree.parent == current_window.get() {
                // Weird, but that might also mean we're at the end of the tree (or the window has no parent yet)
                break;
            }

            // Sanity check if the current parent is actually registered to have the child in its children list
            if let Some(child_id) = ancestry.last_id() {
                if !tree.children.contains(&child_id.get()) {
                    // The child has been orphaned, it must have been reparented between our server queries.
                    // Go back a step and check again.

                    if rechecked_children.contains(&child_id) {
                        crate::warn!(
                            "Children of parent {} does not contain {}: {:?}",
                            current_window,
                            child_id,
                            &tree.children
                        );
                    } else {
                        rechecked_children.push(child_id);
                    }

                    let Some(_) = ancestry.pop_id() else { unreachable!() };
                    current_window = child_id;
                    continue;
                }
            }


            // All checks succeeded, now register the current window info and fetch info from the parent
            ancestry.push(Ancestor { id: current_window, mapped: mapped.into() });

            if tree.parent == tree.root {
                // No need to get info for the root, we assume it's always there. We can just stop here.
                break;
            }

            // If parent == 0, assume there's no parent and just break
            if let Some(parent) = NonZeroU32::new(tree.parent) {
                current_window = parent;
            } else {
                break;
            }

            if let Some(root) = NonZeroU32::new(tree.root) {
                if Some(root) != root_id.get() {
                    root_id.set(Some(root))
                }
            }
        }

        own_window_viewable.set(ancestry.check_all_mapped());

        Ok(())
    }
}

/// Returns Ok(None) on BadWindow.
fn fetch_window_info(
    connection: &X11Connection, window: NonZeroU32,
) -> Result<Option<(bool, QueryTreeReply)>, ReplyError> {
    let attrs_cookie = connection.conn.get_window_attributes(window.get())?;
    let tree_cookie = connection.conn.query_tree(window.get())?;

    let mapped = match attrs_cookie.reply() {
        Ok(attr) => attr.map_state != MapState::UNMAPPED,
        Err(ReplyError::X11Error(X11Error { error_kind: ErrorKind::Window, .. })) => {
            tree_cookie.discard_reply_and_errors();
            return Ok(None);
        }
        Err(e) => {
            tree_cookie.discard_reply_and_errors();
            return Err(e);
        }
    };

    let tree = match tree_cookie.reply() {
        Ok(tree) => tree,
        Err(ReplyError::X11Error(X11Error { error_kind: ErrorKind::Window, .. })) => {
            return Ok(None)
        }
        Err(e) => return Err(e),
    };

    Ok(Some((mapped, tree)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{CreateWindowAux, EventMask, WindowClass};
    use x11rb::protocol::Event;

    type SnapshotHook = (NonZeroU32, Box<dyn FnOnce()>);
    thread_local! {
        static AFTER_SNAPSHOT: RefCell<Option<SnapshotHook>> = const { RefCell::new(None) };
    }

    pub(super) fn after_snapshot(window: NonZeroU32) {
        let hook = AFTER_SNAPSHOT.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.as_ref().is_some_and(|(target, _)| *target == window) {
                slot.take().map(|(_, hook)| hook)
            } else {
                None
            }
        });
        if let Some(hook) = hook {
            hook();
        }
    }

    struct ResetHook;
    impl Drop for ResetHook {
        fn drop(&mut self) {
            AFTER_SNAPSHOT.with(|slot| *slot.borrow_mut() = None);
        }
    }

    fn create_window(host: &X11Connection, parent: NonZeroU32) -> NonZeroU32 {
        let screen = host.default_screen();
        let id = NonZeroU32::new(host.conn.generate_id().unwrap()).unwrap();
        host.conn.create_window(
            screen.root_depth, id.get(), parent.get(), 0, 0, 64, 64, 0,
            WindowClass::INPUT_OUTPUT, screen.root_visual,
            &CreateWindowAux::new().background_pixel(screen.black_pixel),
        ).unwrap().check().unwrap();
        id
    }

    #[test]
    #[ignore = "requires an isolated X11 server; see external REAPER ancestor-race recipe"]
    fn map_between_ancestor_snapshot_and_parent_subscription_is_not_lost() {
        let host = Rc::new(X11Connection::new().unwrap());
        let root = NonZeroU32::new(host.default_screen().root).unwrap();
        let top = create_window(&host, root);
        let parent = create_window(&host, top);
        let child = create_window(&host, parent);
        host.conn.map_window(top.get()).unwrap().check().unwrap();
        assert_eq!(host.conn.get_window_attributes(top.get()).unwrap().reply().unwrap().map_state,
            MapState::VIEWABLE);

        let observer = X11Connection::new().unwrap();
        observer.register_tree_structure_events().unwrap().check().unwrap();
        // These are already selected by XcbWindow::new for a real editor.
        let own_mask = EventMask::STRUCTURE_NOTIFY | EventMask::EXPOSURE | EventMask::KEY_PRESS;
        observer.conn.change_window_attributes(child.get(),
            &x11rb::protocol::xproto::ChangeWindowAttributesAux::new().event_mask(own_mask)
        ).unwrap().check().unwrap();

        let _reset = ResetHook;
        let fired = Rc::new(Cell::new(false));
        let fired_at_snapshot = Rc::clone(&fired);
        let host_at_snapshot = Rc::clone(&host);
        AFTER_SNAPSHOT.with(|slot| *slot.borrow_mut() = Some((parent, Box::new(move || {
            // Actual production discovery has read parent=UNMAPPED. Its own
            // parent has not been subscribed yet. A second client now maps it.
            host_at_snapshot.conn.map_window(parent.get()).unwrap().check().unwrap();
            assert_eq!(host_at_snapshot.conn.get_window_attributes(parent.get())
                .unwrap().reply().unwrap().map_state, MapState::VIEWABLE);
            fired_at_snapshot.set(true);
        }))));
        let visibility = AncestorVisibilityState::discover(&observer, child, true).unwrap();
        assert!(fired.get(), "actual discovery must cross the targeted snapshot seam");

        // This is the later show operation, after discover has returned.
        host.conn.map_window(child.get()).unwrap().check().unwrap();
        let server_state = host.conn.get_window_attributes(child.get()).unwrap().reply().unwrap().map_state;
        assert_eq!(server_state, MapState::VIEWABLE, "host has made the real child viewable");
        // A reply on the observing connection also drains preceding server events.
        observer.conn.get_input_focus().unwrap().reply().unwrap();
        let mut mapped_events = 0;
        while let Some(event) = observer.conn.poll_for_event().unwrap() {
            match event {
                Event::MapNotify(e) => {
                    mapped_events += 1;
                    if let Some(id) = NonZeroU32::new(e.window) {
                        visibility.window_mapped(id);
                    }
                }
                Event::UnmapNotify(e) => {
                    if let Some(id) = NonZeroU32::new(e.window) {
                        visibility.window_unmapped(id);
                    }
                }
                _ => {}
            }
        }
        let preserved_mask = observer.conn.get_window_attributes(child.get()).unwrap().reply().unwrap().your_event_mask;
        assert_eq!(preserved_mask, own_mask, "ancestor subscription must preserve the child's input mask");
        eprintln!("X11_ANCESTOR_RACE server={server_state:?} cached={} map_events={mapped_events} seam_fired={}",
            visibility.own_window_is_viewable(), fired.get());
        assert!(visibility.own_window_is_viewable(),
            "a server-viewable child must not remain suppressed by a missed ancestor map");
    }
}
