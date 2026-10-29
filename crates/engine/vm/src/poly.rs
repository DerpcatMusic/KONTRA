//! KSP polyphonic variables: one set of cells per note event, shared by
//! its note and release callbacks.

/// Note events whose polyphonic cells are kept at once.
// ponytail: 512 events, the least recently started one's cells are reused;
// Kontakt keeps 8192. Raise it if held notes lose release state.
pub(crate) const POLY_EVENTS: usize = 512;

/// One note event's polyphonic cells. Its note callback and its release
/// callback share them (KSP: "a polyphonic variable retains its value in
/// the on release callback of the corresponding note").
pub(crate) struct Poly {
    /// The low half of the context the callbacks started with: the event.
    pub(crate) event: u32,
    pub(crate) cells: Box<[i64]>,
    /// Busy callbacks using these cells.
    pub(crate) users: u16,
    /// When a callback last took these cells; the oldest free ones go first.
    pub(crate) used: u64,
}

/// The polyphonic cells for a callback of `event`: the ones its event's
/// other callbacks use or left, else the oldest free ones, zeroed.
pub(crate) fn take_poly(polys: &mut [Poly], event: u32, starts: &mut u64) -> usize {
    *starts += 1;
    let found = polys.iter().position(|p| p.used != 0 && p.event == event);
    let at = found.or_else(|| {
        let free = polys.iter().enumerate().filter(|(_, p)| p.users == 0);
        free.min_by_key(|(_, p)| p.used).map(|(i, _)| i)
    });
    let Some(p) = at.and_then(|i| polys.get_mut(i)) else {
        return usize::MAX;
    };
    if found.is_none() {
        p.event = event;
        p.cells.fill(0);
    }
    p.users = p.users.saturating_add(1);
    p.used = *starts;
    at.unwrap_or(usize::MAX)
}

pub(crate) fn free_poly(polys: &mut [Poly], at: usize) {
    if let Some(p) = polys.get_mut(at) {
        p.users = p.users.saturating_sub(1);
    }
}
