//! `get_engine_par_disp`: which [`Readout`] a parameter's knob shows.

use crate::Engine;
use kontra_ir as ir;
use kontra_plan::par_laws::text;
use kontra_script_ui::readout::{Readout, write};
use kontra_vm::Text;

impl Engine {
    /// `get_engine_par_disp(par, group, slot, generic)`, or with `ext` its `_ext`
    /// form, which shows its second argument. False for an unknown parameter.
    pub(crate) fn par_text(&mut self, ext: bool, n: impl Fn(usize) -> i32, out: &mut Text) -> bool {
        let par = self.script().and_then(|s| s.engine_pars.get(n(0)));
        *out = Text::EMPTY;
        // A known parameter on a missing target shows "" (Engine::absent).
        if !ext && self.absent(n(0), n(1), n(2), self.send_generic(n(0), n(1), n(3))) {
            return true;
        }
        let raw = match ext {
            true => Some(n(1)),
            false => self.get_engine_par(n(0), n(1), n(2), n(3)),
        };
        // A stepped LFO's rate shows its count, one decimal (ANALOG, Kontakt
        // 8.13.1: 0 .. 1 read 4609.0 .. 1.0); a free one its Hz, padded
        // (0, .5, 1 read " 0.01", "  1.5", "213.1").
        if let (Some(crate::EnginePar::Knob(k)), Some(raw)) = (par, raw) {
            let (group, slot) = if ext { (n(2), n(3)) } else { (n(1), n(2)) };
            let row = self.row(k, group, slot);
            let knob = self.script().and_then(|s| s.knobs.get(usize::from(row)));
            let x = raw.clamp(0, 1_000_000) as f32 / 1e6;
            match knob {
                Some((ir::KnobTarget::LfoSteps, law)) => {
                    let count = f64::from(law.eval(x).round().max(1.0));
                    return write(Readout::Signed(count), 0.0, out);
                }
                Some((ir::KnobTarget::LfoRate, law)) => {
                    return write(Readout::Padded(f64::from(law.eval(x))), 0.0, out);
                }
                // A time-stretch speed in %, one decimal (Kontakt 8.13.1, Time
                // Machine 2: 0, .1, .5, 1 read "0.0", "26.4", "100.0", "800.0").
                // ponytail: Time Machine Pro reads "1.0" at 0; its floor is not carried.
                Some((ir::KnobTarget::Speed, law)) => {
                    return write(Readout::Signed(100.0 * f64::from(law.eval(x))), 0.0, out);
                }
                _ => {}
            }
        }
        // The reverb's time shows its law's seconds in ms (oracle 2026-10-29).
        if let Some((ir::ReverbPar::Seconds, s)) =
            par.zip(raw).and_then(|(p, r)| self.reverb_value(p, r))
        {
            return write(Readout::Padded(1000.0 * f64::from(s)), 0.0, out);
        }
        self.script()
            .zip(raw)
            .is_some_and(|(s, raw)| text(&s.engine_pars, n(0), raw, out))
    }
}
