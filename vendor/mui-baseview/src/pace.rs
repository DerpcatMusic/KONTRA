//! KONTAKTO patch: frame pacing a host cannot turn into a spin, and the
//! counts that show it in the session log.
//!
//! A GPU present that reaches nothing (occluded, timed out, outdated) leaves
//! the frame unpainted. Retried every refresh, that is a full scene snapshot
//! and surface acquire per vsync for as long as the host hides the surface.
//! One immediate retry covers a resize's outdated surface; after that it waits
//! [`HOLD`] unless a new scene arrives.
//!
//! A screen reader that asks for a tree and switches off before the next tick
//! leaves upstream's `asked` set forever, and `wants_tree` redraws every
//! frame. A request no `prepare` could answer stops redrawing until one does.

use std::time::{Duration, Instant};

/// How long an unpainted frame waits after repeated skips.
pub(crate) const HOLD: Duration = Duration::from_millis(100);
/// How often the counts reach the session log.
pub(crate) const REPORT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub(crate) struct Pace {
    skips: u32,
    held_until: Option<Instant>,
    /// A tree request `prepare` could not answer: accessibility is off.
    a11y_unanswered: bool,
    counts: Counts,
    reported_at: Instant,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Counts {
    pub presented: u32,
    pub skipped: u32,
    pub a11y_redraws: u32,
    pub requested: u32,
}

impl Pace {
    pub(crate) fn new(now: Instant) -> Self {
        Self {
            skips: 0,
            held_until: None,
            a11y_unanswered: false,
            counts: Counts::default(),
            reported_at: now,
        }
    }

    /// Paint now? A new scene always paints; a skipped one waits its hold.
    pub(crate) fn paints(&self, fresh: bool, now: Instant) -> bool {
        fresh || self.held_until.is_none_or(|until| now >= until)
    }

    pub(crate) fn presented(&mut self) {
        self.counts.presented = self.counts.presented.saturating_add(1);
        self.skips = 0;
        self.held_until = None;
    }

    /// On screen already: not a retry, nothing counted.
    pub(crate) fn current(&mut self) {
        self.skips = 0;
        self.held_until = None;
    }

    pub(crate) fn skipped(&mut self, now: Instant) {
        self.counts.skipped = self.counts.skipped.saturating_add(1);
        self.skips = self.skips.saturating_add(1);
        self.held_until = (self.skips > 1).then(|| now + HOLD);
    }

    /// The plugin asked for a frame (a rebuilt scene).
    pub(crate) fn requested(&mut self) {
        self.counts.requested = self.counts.requested.saturating_add(1);
    }

    /// Redraw for a screen reader's tree request?
    pub(crate) fn a11y_redraws(&mut self, wants_tree: bool) -> bool {
        let redraw = wants_tree && !self.a11y_unanswered;
        if redraw {
            self.counts.a11y_redraws = self.counts.a11y_redraws.saturating_add(1);
        }
        redraw
    }

    /// After `prepare`: an update answered the request; none while it is
    /// still asked means accessibility switched off.
    pub(crate) fn a11y_prepared(&mut self, wants_tree: bool, answered: bool) {
        self.a11y_unanswered = wants_tree && !answered;
    }

    /// The last [`REPORT`]'s counts, once per report, then zeroed.
    pub(crate) fn report(&mut self, now: Instant) -> Option<Counts> {
        if now.duration_since(self.reported_at) < REPORT {
            return None;
        }
        self.reported_at = now;
        let counts = std::mem::take(&mut self.counts);
        (counts != Counts::default()).then_some(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hidden_surface_retries_at_a_capped_rate_not_every_refresh() {
        let t0 = Instant::now();
        let vsync = Duration::from_millis(16);
        let mut pace = Pace::new(t0);
        // One second of a surface that never presents (occluded).
        let mut attempts = 0;
        for i in 0..60 {
            let now = t0 + vsync * i;
            if pace.paints(false, now) {
                attempts += 1;
                pace.skipped(now);
            }
        }
        // The outdated-resize retry, then at most one per HOLD.
        assert!(
            attempts <= 2 + 1000 / HOLD.as_millis(),
            "{attempts} attempts"
        );
        // A new scene paints at once; a present ends the hold.
        let later = t0 + vsync * 61;
        assert!(pace.paints(true, later));
        pace.presented();
        assert!(pace.paints(false, later));
    }

    #[test]
    fn an_outdated_surface_retries_on_the_next_frame() {
        let t0 = Instant::now();
        let mut pace = Pace::new(t0);
        pace.skipped(t0);
        assert!(pace.paints(false, t0 + Duration::from_millis(16)));
    }

    #[test]
    fn a_tree_request_left_by_a_reader_that_switched_off_stops_redrawing() {
        let mut pace = Pace::new(Instant::now());
        assert!(pace.a11y_redraws(true));
        // prepare answered nothing: accessibility went inactive, `asked` stuck.
        pace.a11y_prepared(true, false);
        assert!(!pace.a11y_redraws(true));
        // Switched back on: prepare answers, redraws resume when asked again.
        pace.a11y_prepared(true, true);
        assert!(pace.a11y_redraws(true));
    }

    #[test]
    fn counts_reach_the_log_once_per_report_and_only_when_nonzero() {
        let t0 = Instant::now();
        let mut pace = Pace::new(t0);
        assert_eq!(pace.report(t0 + REPORT), None);
        pace.presented();
        pace.skipped(t0);
        pace.requested();
        assert_eq!(pace.report(t0 + REPORT / 2), None);
        let counts = pace.report(t0 + REPORT * 2);
        assert_eq!(
            counts,
            Some(Counts {
                presented: 1,
                skipped: 1,
                a11y_redraws: 0,
                requested: 1
            })
        );
        assert_eq!(pace.report(t0 + REPORT * 4), None);
    }
}
