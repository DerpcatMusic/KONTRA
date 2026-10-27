//! The control store: what a script reads back with `get_control_par`.
//!
//! A KSP control *is* its variable: `$CONTROL_PAR_VALUE` reads and writes the
//! variable's cell, so `$sw := 1` and `set_control_par(id, VALUE, 1)` are the
//! same write. While a callback runs the VM is lent to the interpreter and its
//! cells reach the host only through the call, so value access takes them as
//! an argument. Other parameters (hide, picture state, range) are kept per
//! control as the script sets them.

use crate::Engine;

impl Engine {
    /// Index of the control with `id`, and its value cell if `par` is the value.
    fn control(&self, id: i32, par: i32) -> Option<(usize, Option<usize>)> {
        let controls = &self.plan.script.as_ref()?.controls;
        let n = usize::try_from(id.checked_sub(controls.first)?).ok()?;
        let cell = controls.cells.get(n)?;
        let value = (par == controls.value_par).then_some(*cell).flatten();
        Some((n, value.map(|c| c as usize)))
    }

    pub(crate) fn control_par(&self, cells: &[i64], id: i32, par: i32) -> i32 {
        match self.control(id, par) {
            Some((_, Some(cell))) => cells.get(cell).map_or(0, |&v| v as i32),
            Some((n, None)) => self
                .control_pars
                .get(n)
                .and_then(|pars| pars.iter().find(|(p, _)| *p == par))
                .map_or(0, |&(_, v)| v),
            None => 0,
        }
    }

    // ponytail: a parameter first set after init allocates its slot on the
    // audio thread; preallocate per control if a script does that per note.
    pub(crate) fn set_control_par(&mut self, cells: &mut [i64], id: i32, par: i32, value: i32) {
        match self.control(id, par) {
            Some((_, Some(cell))) => {
                if let Some(c) = cells.get_mut(cell) {
                    *c = i64::from(value);
                }
            }
            Some((n, None)) => {
                let Some(pars) = self.control_pars.get_mut(n) else {
                    return;
                };
                match pars.iter_mut().find(|(p, _)| *p == par) {
                    Some(slot) => slot.1 = value,
                    None => pars.push((par, value)),
                }
            }
            None => {}
        }
    }
}
