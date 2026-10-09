//! A plugin's editor without a window: mui-cut's generic adapter hosts any
//! plugin's MUI editor this way, with no code in the plugin.
//!
//! The adapter calls [`claim`], then has the plugin open its editor through
//! its framework as a host would (moose's `Editor::open`, nice-plug's
//! `spawn`). The window crate's `open` hands its [`View`] to [`offer`]
//! instead of making a window, and the adapter [`take`]s it: the plugin's
//! own `Ui`, its view (build closure, parameter bridge) and its size, to
//! frame and capture off screen.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use mui_input::{Input, Key, Mods};
use mui_scene::prelude::{El, Point, Size};

use super::{Shared, View, lock};
use crate::Ui;

static CLAIMED: AtomicBool = AtomicBool::new(false);
static OFFERED: Mutex<Option<Headless>> = Mutex::new(None);

/// An editor that opened while [`claim`]ed: frame `view` into `ui` at
/// `size` logical points.
pub struct Headless {
    pub ui: Ui,
    pub view: Box<dyn View + Send>,
    pub size: Size,
}

/// From now on, editors this process opens are headless.
pub fn claim() {
    CLAIMED.store(true, Ordering::Release);
}

/// Seconds on the host's clock, as `f64` bits.
static TIME: AtomicU64 = AtomicU64::new(0);

/// The host's clock, which a headless editor animates by in place of the
/// wall's: frames stay the same however fast or slow they are made.
pub fn set_time(seconds: f64) {
    TIME.store(seconds.max(0.).to_bits(), Ordering::Release);
}

/// The time since the host's clock started, once [`claim`]ed (zero until
/// it first sets one); `None` in a windowed process, which keeps the wall's.
pub fn time() -> Option<Duration> {
    CLAIMED
        .load(Ordering::Acquire)
        .then(|| Duration::from_secs_f64(f64::from_bits(TIME.load(Ordering::Acquire))))
}

/// A window crate's `open`, first: `true` when the process [`claim`]ed
/// editors, and then the view is parked for [`take`] and no window opens.
pub fn offer<V: View + Send + 'static>(shared: &Arc<Mutex<Shared<V>>>, size: (u32, u32)) -> bool {
    if !CLAIMED.load(Ordering::Acquire) {
        return false;
    }
    let ui = std::mem::take(&mut lock(shared).ui);
    *OFFERED.lock().unwrap_or_else(PoisonError::into_inner) = Some(Headless {
        ui,
        view: Box::new(Parked(Arc::clone(shared))),
        size: Size::new(f64::from(size.0), f64::from(size.1)),
    });
    true
}

/// The editor the last headless `open` parked.
pub fn take() -> Option<Headless> {
    OFFERED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

/// A shared view, locked for each call.
struct Parked<V>(Arc<Mutex<Shared<V>>>);

impl<V: View> View for Parked<V> {
    fn build(&mut self, ui: &mut Ui, input: &Input) -> El {
        lock(&self.0).view.build(ui, input)
    }
    fn changed(&mut self) -> bool {
        lock(&self.0).view.changed()
    }
    fn request_resize(&mut self, width: u32, height: u32) -> bool {
        lock(&self.0).view.request_resize(width, height)
    }
    fn after_frame(&mut self, ui: &mut Ui) {
        lock(&self.0).view.after_frame(ui);
    }
    fn cancel(&mut self, ui: &Ui) {
        lock(&self.0).view.cancel(ui);
    }
    fn still(&self, ui: &Ui, from: Option<Point>, to: Option<Point>) -> bool {
        lock(&self.0).view.still(ui, from, to)
    }
    fn zoom(&self, window: Size) -> f64 {
        lock(&self.0).view.zoom(window)
    }
    fn claims_key(&self, key: &Key, mods: Mods) -> bool {
        lock(&self.0).view.claims_key(key, mods)
    }
    fn log(&mut self, line: &str) {
        lock(&self.0).view.log(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    struct Label(&'static str);
    impl View for Label {
        fn build(&mut self, _: &mut Ui, _: &Input) -> El {
            text(self.0).id("label")
        }
        fn changed(&mut self) -> bool {
            false
        }
        fn request_resize(&mut self, _: u32, _: u32) -> bool {
            false
        }
    }

    #[test]
    fn a_claimed_open_parks_the_view_instead_of_a_window() {
        let shared = Arc::new(Mutex::new(Shared {
            ui: Ui::default(),
            view: Label("hi"),
        }));
        // Unclaimed: the window crate makes its window.
        assert!(!offer(&shared, (300, 200)));
        assert!(take().is_none());
        claim();
        assert!(offer(&shared, (300, 200)));
        let mut h = take().expect("parked");
        assert_eq!(h.size, Size::new(300., 200.));
        let tree = h.view.build(&mut h.ui, &Input::default());
        h.ui.frame(tree, Some(h.size), Input::default(), 0.)
            .expect("frames");
        assert!(h.ui.scene().is_some_and(|s| s.surface("label").is_some()));
        assert!(take().is_none(), "taken once");
    }
}
