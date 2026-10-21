//! How a control answers the wheel, the reset gesture and the keyboard.
//!
//! Every continuous control reads the same gestures through [`nudge`]; every
//! discrete one through [`step_choice`]. The value is always the control's own
//! normalised value, so a step follows the parameter's law (log, pivoted,
//! skewed) without the control knowing which one it is. A change lands in the
//! caller's `value`, and the bind turns it into a host gesture.

use mui::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::RangeInclusive;

/// Of the range, per wheel notch or arrow key press, for a continuous control.
pub const STEP: f64 = 0.01;

/// Of a step, under Shift or Ctrl: the fine adjustment.
pub const FINE: f64 = 0.1;

/// Of the range, per Page Up or Page Down.
const PAGE: f64 = 0.1;

thread_local! {
    /// Wheel notches a discrete control has not yet spent. A trackpad sends
    /// many small deltas; a choice moves when a whole notch has built up.
    static NOTCHES: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

/// The wheel's notches this frame, positive up. The host reports a notch as
/// one text line in scene units, and a trackpad as pixels, so a line's size
/// turns both into notches.
fn notches(ui: &Ui, id: &str) -> f64 {
    -ui.get(id).wheel.y / ui.theme().text
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
    if r.double_clicked || (r.pressed && r.press_mods.ctrl) {
        *value = rest;
    } else {
        let mut turn = -r.wheel.y / ui.theme().text * STEP * span * fine(r.mods);
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
    let mut turn = NOTCHES.with(|n| {
        let mut n = n.borrow_mut();
        let spent = n.entry(id.to_string()).or_insert(0.0);
        *spent += notches(ui, id);
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

    #[test]
    fn a_fine_step_is_a_tenth_of_a_coarse_one() {
        assert!((fine(Mods { shift: true, ..Mods::default() }) - FINE).abs() < f64::EPSILON);
        assert!((fine(Mods { ctrl: true, ..Mods::default() }) - FINE).abs() < f64::EPSILON);
        assert!((fine(Mods::default()) - 1.0).abs() < f64::EPSILON);
    }
}
