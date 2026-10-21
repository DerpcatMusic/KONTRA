//! How a control answers the wheel, the reset gesture and the keyboard.
//!
//! Every continuous control reads the same gestures through [`nudge`]; a switch
//! with several choices through [`step_choice`]; a toggle only flips on a
//! click or Enter, as it always did. The value is always the control's own
//! normalised value, so a step follows the parameter's law (log, pivoted,
//! skewed) without the control knowing which one it is. A change lands in the
//! caller's `value`, and the bind turns it into a host gesture.

use mui::host::{NOTCH_LINES, WHEEL_ROLL};
use mui::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Of the range, per wheel notch or arrow key press, for a continuous control.
pub const STEP: f64 = 0.01;

/// Of a step, under Shift or Ctrl: the fine adjustment.
pub const FINE: f64 = 0.1;

/// Of the range, per Page Up or Page Down.
const PAGE: f64 = 0.1;

/// How long a roll of the wheel stays one edit: see [`WHEEL_ROLL`].
fn roll() -> Duration {
    Duration::from_secs_f64(WHEEL_ROLL)
}

/// When the wheel last moved a control; `None` before it ever has.
static LAST_NOTCH: Mutex<Option<Instant>> = Mutex::new(None);

fn mark_notch(id: &str) {
    let now = Instant::now();
    if let Ok(mut at) = LAST_NOTCH.lock() {
        *at = Some(now);
    }
    ROLLS.with(|r| r.borrow_mut().insert(id.to_string(), now));
}

/// Whether the wheel is still turning control `id`: its edit stays open
/// until the roll stops (see `Bridge::hold`).
pub fn rolling(id: &str) -> bool {
    ROLLS.with(|r| r.borrow().get(id).is_some_and(|at| at.elapsed() < roll()))
}

/// Whether the wheel is still rolling: an edit the host should keep open, as
/// it does while the pointer is down.
pub fn wheel_rolling() -> bool {
    LAST_NOTCH
        .lock()
        .ok()
        .and_then(|at| *at)
        .is_some_and(|at| at.elapsed() < roll())
}

thread_local! {
    /// When each control's wheel last turned it: a roll is one edit per control.
    static ROLLS: RefCell<HashMap<String, Instant>> = RefCell::new(HashMap::new());
    /// Wheel notches a discrete control has not yet spent. A trackpad sends
    /// many small deltas; a choice moves when a whole notch has built up.
    static NOTCHES: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

/// The wheel's notches this frame, positive up. The host reports a notch as
/// `NOTCH_LINES` text rows in scene units, and a trackpad as pixels, so the
/// notch's size in scene units turns both into notches.
fn notches(ui: &Ui, wheel: Vec2) -> f64 {
    -wheel.y / (ui.theme().text * NOTCH_LINES)
}

/// Whether Shift or Ctrl asks for the fine step.
fn fine(mods: Mods) -> f64 {
    if mods.shift || mods.ctrl { FINE } else { 1.0 }
}

/// Wheel, reset and keyboard for a continuous control whose value runs over
/// `range`. The wheel moves it `STEP` of the range per notch, a tenth as far
/// under Shift or Ctrl. A double click, or Ctrl+click, returns it to `rest`.
/// The focused control also takes the arrows, Page Up and Page Down, Home and
/// End. Returns whether the value changed.
pub fn nudge(ui: &Ui, id: &str, value: &mut f64, range: RangeInclusive<f64>, rest: f64) -> bool {
    let (lo, hi) = (range.start().min(*range.end()), range.start().max(*range.end()));
    let span = hi - lo;
    let before = *value;
    let r = ui.get(id);
    if r.wheel.y != 0.0 {
        mark_notch(id);
    }
    if r.double_clicked || (r.pressed && r.press_mods.ctrl) {
        *value = rest;
    } else {
        let mut turn = notches(ui, r.wheel) * STEP * span * fine(r.mods);
        if ui.focus_visible(id) {
            for key in ui.keys(id) {
                let step = STEP * span * fine(key.mods);
                match key.key {
                    Key::Up | Key::Right => turn += step,
                    Key::Down | Key::Left => turn -= step,
                    Key::PageUp => turn += PAGE * span,
                    Key::PageDown => turn -= PAGE * span,
                    Key::Home => *value = lo,
                    Key::End => *value = hi,
                    _ => {}
                }
            }
        }
        *value = (*value + turn).clamp(lo, hi);
    }
    *value != before
}

/// Wheel and arrow keys for a discrete control of `len` choices: one choice
/// per notch, wrapping at neither end. Up and Right move forward. Returns
/// whether the choice changed.
pub fn step_choice(ui: &Ui, id: &str, choice: &mut usize, len: usize) -> bool {
    if len < 2 {
        return false;
    }
    let before = *choice;
    if ui.get(id).wheel.y != 0.0 {
        mark_notch(id);
    }
    let mut turn = NOTCHES.with(|n| {
        let mut n = n.borrow_mut();
        let spent = n.entry(id.to_string()).or_insert(0.0);
        *spent += notches(ui, ui.get(id).wheel);
        let whole = spent.trunc();
        *spent -= whole;
        whole
    });
    if ui.focus_visible(id) {
        for key in ui.keys(id) {
            match key.key {
                Key::Up | Key::Right => turn += 1.0,
                Key::Down | Key::Left => turn -= 1.0,
                Key::Home => *choice = 0,
                Key::End => *choice = len - 1,
                _ => {}
            }
        }
    }
    let next = (*choice as f64 + turn).clamp(0.0, (len - 1) as f64);
    *choice = next as usize;
    *choice != before
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame over a single focusable target at the origin, with the
    /// pointer inside it and whatever the test sends on top.
    fn frame(ui: &mut Ui, pointer: Option<Point>, down: bool, wheel: Vec2, keys: Vec<KeyPress>, mods: Mods) {
        let root = block(100., 100.).id("k").focusable();
        let buttons = Buttons::default().set(Button::Primary, down);
        let input = Input {
            pointer: PointerInput { pos: pointer, buttons, mods },
            wheel,
            keys,
            ..Input::default()
        };
        ui.frame(root, None, input, 0.016).unwrap();
    }

    fn inside() -> Option<Point> {
        Some(Point::new(50., 50.))
    }

    fn key(key: Key) -> KeyPress {
        KeyPress { key, mods: Mods::default() }
    }

    #[test]
    fn a_roll_of_the_wheel_is_held_open_until_it_stops() {
        let mut ui = Ui::default();
        let notch = ui.theme().text * NOTCH_LINES;
        frame(&mut ui, inside(), false, Vec2::new(0., -notch), vec![], Mods::default());
        let mut v = 0.5;
        nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5);
        assert!(wheel_rolling());
    }

    #[test]
    fn a_fine_step_is_a_tenth_of_a_coarse_one() {
        assert!((fine(Mods { shift: true, ..Mods::default() }) - FINE).abs() < f64::EPSILON);
        assert!((fine(Mods { ctrl: true, ..Mods::default() }) - FINE).abs() < f64::EPSILON);
        assert!((fine(Mods::default()) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_notch_up_raises_the_value_by_one_step_and_down_lowers_it() {
        let mut ui = Ui::default();
        let notch = ui.theme().text * NOTCH_LINES;
        frame(&mut ui, inside(), false, Vec2::new(0., -notch), vec![], Mods::default());
        let mut v = 0.5;
        assert!(nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5));
        assert!((v - (0.5 + STEP)).abs() < 1e-12, "up: {v}");

        let mut ui = Ui::default();
        frame(&mut ui, inside(), false, Vec2::new(0., notch), vec![], Mods::default());
        let mut v = 0.5;
        assert!(nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5));
        assert!((v - (0.5 - STEP)).abs() < 1e-12, "down: {v}");
    }

    #[test]
    fn shift_or_ctrl_makes_the_wheel_fine_and_the_value_stays_in_range() {
        let mut ui = Ui::default();
        let notch = ui.theme().text * NOTCH_LINES;
        let shift = Mods { shift: true, ..Mods::default() };
        frame(&mut ui, inside(), false, Vec2::new(0., -notch), vec![], shift);
        let mut v = 0.5;
        nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5);
        assert!((v - (0.5 + STEP * FINE)).abs() < 1e-12, "fine: {v}");

        let mut ui = Ui::default();
        frame(&mut ui, inside(), false, Vec2::new(0., -notch * 500.), vec![], Mods::default());
        let mut v = 0.99;
        nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5);
        assert_eq!(v, 1.0, "clamped at the top of the range");
    }

    #[test]
    fn a_double_click_returns_the_value_to_rest() {
        let mut ui = Ui::default();
        // A warm-up frame gives the pointer a resolved scene to hit.
        frame(&mut ui, inside(), false, Vec2::ZERO, vec![], Mods::default());
        frame(&mut ui, inside(), true, Vec2::ZERO, vec![], Mods::default());
        frame(&mut ui, inside(), false, Vec2::ZERO, vec![], Mods::default());
        frame(&mut ui, inside(), true, Vec2::ZERO, vec![], Mods::default());
        let mut v = 0.9;
        assert!(nudge(&ui, "k", &mut v, 0.0..=1.0, 0.25));
        assert_eq!(v, 0.25);
    }

    #[test]
    fn the_arrows_step_only_a_focused_control_and_home_and_end_reach_the_ends() {
        let mut ui = Ui::default();
        // A warm-up frame gives focus a scene to move over; Tab then takes it.
        frame(&mut ui, None, false, Vec2::ZERO, vec![], Mods::default());
        frame(&mut ui, None, false, Vec2::ZERO, vec![key(Key::Tab)], Mods::default());
        frame(&mut ui, None, false, Vec2::ZERO, vec![key(Key::Right)], Mods::default());
        let mut v = 0.5;
        nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5);
        assert!((v - (0.5 + STEP)).abs() < 1e-12, "right: {v}");

        frame(&mut ui, None, false, Vec2::ZERO, vec![key(Key::End)], Mods::default());
        let mut v = 0.5;
        nudge(&ui, "k", &mut v, 0.0..=1.0, 0.5);
        assert_eq!(v, 1.0, "end");
    }

    #[test]
    fn a_discrete_control_moves_one_choice_per_whole_notch() {
        let mut ui = Ui::default();
        let notch = ui.theme().text * NOTCH_LINES;
        // Three trackpad-sized deltas of 0.4 notch each: the third makes a notch.
        let mut choice = 0usize;
        let mut moved = Vec::new();
        for _ in 0..3 {
            frame(&mut ui, inside(), false, Vec2::new(0., -0.4 * notch), vec![], Mods::default());
            moved.push(step_choice(&ui, "k", &mut choice, 4));
        }
        assert_eq!(moved, [false, false, true]);
        assert_eq!(choice, 1);
    }
}
