//! `get_engine_par_disp`: which [`Readout`] a parameter's knob shows.

use crate::Engine;
use kontra_ir as ir;
use kontra_plan::par_laws::display;
use kontra_script_ui::readout::{Readout, write};
use kontra_vm::Text;

impl Engine {
    /// `get_engine_par_disp(par, group, slot, generic)`, or with `ext` its `_ext`
    /// form, which shows its second argument. False when not carried.
    pub(crate) fn par_text(&mut self, ext: bool, n: impl Fn(usize) -> i32, out: &mut Text) -> bool {
        let par = self.script().and_then(|s| s.engine_pars.get(n(0)));
        let at = usize::from(ext) + 1;
        let raw = match ext {
            true => Some(n(1)),
            false => self.get_engine_par(n(0), n(at), n(at + 1), n(at + 2)),
        };
        *out = Text::EMPTY;
        // The reverb's time shows its law's seconds in ms (oracle 2026-10-29).
        if let Some((ir::ReverbPar::Seconds, s)) =
            par.zip(raw).and_then(|(p, r)| self.reverb_value(p, r))
        {
            return write(Readout::Padded(1000.0 * f64::from(s)), 0.0, out);
        }
        par.zip(raw).is_some_and(|(p, raw)| display(p, raw, out))
    }
}
