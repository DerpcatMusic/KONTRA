//! Deadline requests are separate from spring animation. The host does not need
//! to redraw 60/160 times per second just to blink a caret or show a tooltip.
use crate::Ui;
use mui_scene::A11y;
use std::time::Duration;
const HALF_CARET_PERIOD: f64 = 0.625;
impl Ui {
    /// Delay until the next timer-dependent visual change. Input/model changes
    /// still invalidate immediately, and `Frame::animating` requests an animation
    /// frame. Hosts should use the earlier of their own deadline and this one.
    pub fn repaint_after(&self) -> Option<Duration> {
        if !self.time.is_finite() || self.time < 0. {
            return None;
        }
        let tip = self.hover.as_ref().and_then(|(key, elapsed)| {
            if !elapsed.is_finite() || *elapsed >= crate::ui::TIP_DELAY {
                return None;
            }
            self.scene.as_ref()?.surface(key)?.tip.as_ref()?;
            Some((crate::ui::TIP_DELAY - elapsed).max(0.001))
        });
        let caret = self
            .focus
            .as_deref()
            .and_then(|key| self.scene.as_ref()?.surface(key))
            .filter(|s| {
                !s.disabled
                    && matches!(
                        s.semantics.as_ref().map(|s| &s.role),
                        Some(A11y::TextInput { .. })
                    )
            })
            .map(|_| (HALF_CARET_PERIOD - self.time.rem_euclid(HALF_CARET_PERIOD)).max(0.001));
        match (tip, caret) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
        .map(Duration::from_secs_f64)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use mui_input::Input;
    use mui_scene::prelude::*;
    #[test]
    fn idle_has_no_deadline() {
        let mut u = Ui::default();
        u.frame(block(20., 20.), None, Input::default(), 0.)
            .unwrap();
        assert!(u.repaint_after().is_none());
    }
    #[test]
    fn focused_input_gets_a_deadline_not_continuous_animation() {
        let mut u = Ui::default();
        let tree = || {
            block(100., 30.)
                .id("edit")
                .focusable()
                .a11y(A11y::TextInput {
                    value: "".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
        };
        u.frame(tree(), None, Input::default(), 0.).unwrap();
        u.focus("edit");
        let f = u.frame(tree(), None, Input::default(), 0.);
        let f = f.unwrap();
        assert!(!f.animating);
        assert_eq!(f.repaint_after, Some(Duration::from_millis(625)));
    }
    #[test]
    fn real_elapsed_time_crosses_blink_boundary() {
        let mut u = Ui::default();
        let tree = || {
            block(100., 30.)
                .id("edit")
                .focusable()
                .a11y(A11y::TextInput {
                    value: "".into(),
                    selection: (0, 0),
                    carets: Vec::new(),
                })
        };
        u.frame(tree(), None, Input::default(), 0.).unwrap();
        u.focus("edit");
        u.frame(tree(), None, Input::default(), 0.7).unwrap();
        let d = u.repaint_after().unwrap().as_secs_f64();
        assert!((d - 0.55).abs() < 1e-9);
    }
}
