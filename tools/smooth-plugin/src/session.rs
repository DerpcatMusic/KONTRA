//! What the plug-in remembers between edits: the undo history and the two
//! parameter sets.
//!
//! A snapshot is every parameter's plain value. The history watches the
//! live values: a change that arrives while a pointer is held is one edit
//! however many frames it lasts, and is committed when the pointer lets go.
//! Restoring a snapshot (undo, redo, switching sets) is never itself an edit.

/// Every parameter's id and plain value.
pub type Snapshot = Vec<(u32, f64)>;

/// The most edits the history keeps.
const DEPTH: usize = 200;

/// The undo and redo stacks, and the A/B sets.
#[derive(Default)]
pub struct Session {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The values as of the last committed edit.
    base: Snapshot,
    /// Set while an edit is open: the values it began from.
    open: Option<Snapshot>,
    /// The two parameter sets; the live one is whichever `active` names, and
    /// its slot holds the values it had when it was last left.
    sets: [Option<Snapshot>; 2],
    active: usize,
}

impl Session {
    /// Look at the live values. Call once per editor frame with whether a
    /// pointer button is down.
    pub fn observe(&mut self, now: &Snapshot, held: bool) {
        if self.base.is_empty() {
            self.base = now.clone();
            return;
        }
        if *now != self.base && self.open.is_none() {
            self.open = Some(self.base.clone());
        }
        if !held && let Some(from) = self.open.take() {
            if *now != from {
                self.undo.push(from);
                if self.undo.len() > DEPTH {
                    self.undo.remove(0);
                }
                self.redo.clear();
            }
            self.base = now.clone();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Step back: the values to restore.
    pub fn undo(&mut self, now: &Snapshot) -> Option<Snapshot> {
        Self::step(&mut self.undo, &mut self.redo, &mut self.base, &mut self.open, now)
    }

    /// Step forward: the values to restore.
    pub fn redo(&mut self, now: &Snapshot) -> Option<Snapshot> {
        Self::step(&mut self.redo, &mut self.undo, &mut self.base, &mut self.open, now)
    }

    /// Pops `from`, parks `now` on `to`, and makes the popped values the base.
    fn step(
        from: &mut Vec<Snapshot>,
        to: &mut Vec<Snapshot>,
        base: &mut Snapshot,
        open: &mut Option<Snapshot>,
        now: &Snapshot,
    ) -> Option<Snapshot> {
        let target = from.pop()?;
        to.push(now.clone());
        *base = target.clone();
        *open = None;
        Some(target)
    }

    /// Which set is live: 0 is a, 1 is b.
    pub fn active(&self) -> usize {
        self.active
    }

    /// Swap to the other set, returning the values to restore. The first time
    /// the other set is visited it is a copy of this one.
    pub fn switch(&mut self, now: &Snapshot) -> Snapshot {
        self.sets[self.active] = Some(now.clone());
        self.active = 1 - self.active;
        let target = self.sets[self.active].clone().unwrap_or_else(|| now.clone());
        self.base = target.clone();
        self.open = None;
        target
    }

    /// Replace the other set with a copy of this one.
    pub fn copy_to_other(&mut self, now: &Snapshot) {
        self.sets[1 - self.active] = Some(now.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(v: f64) -> Snapshot {
        vec![(1, v), (2, 0.5)]
    }

    #[test]
    fn a_held_drag_is_one_edit_and_undo_restores_where_it_began() {
        let mut s = Session::default();
        s.observe(&at(0.1), false);
        for v in [0.2, 0.3, 0.4] {
            s.observe(&at(v), true);
        }
        assert!(!s.can_undo(), "the edit is still open");
        s.observe(&at(0.4), false);
        assert!(s.can_undo());
        assert_eq!(s.undo(&at(0.4)), Some(at(0.1)));
        assert!(!s.can_undo() && s.can_redo());
        assert_eq!(s.redo(&at(0.1)), Some(at(0.4)));
    }

    #[test]
    fn restoring_is_not_an_edit_and_a_new_edit_clears_redo() {
        let mut s = Session::default();
        s.observe(&at(0.1), false);
        s.observe(&at(0.2), false);
        let back = s.undo(&at(0.2)).unwrap();
        s.observe(&back, false);
        assert!(!s.can_undo(), "restoring must not push history");
        s.observe(&at(0.9), false);
        assert!(s.can_undo() && !s.can_redo());
    }

    #[test]
    fn the_two_sets_keep_their_own_values() {
        let mut s = Session::default();
        let a = at(0.2);
        let b = s.switch(&a);
        assert_eq!(b, a, "b starts as a copy of a");
        let back = s.switch(&at(0.8));
        assert_eq!(back, a);
        assert_eq!(s.switch(&back), at(0.8));
    }
}
