//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in KSP's 0..=1_000_000
//! encoding; the laws are Kontakt's (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::Engine;

/// Attack and hold run 0..=15 s, decay and release 0..=25 s, exponentially
/// with a 2 ms shift: `exp(ln 2 + (ln(top + 2) − ln 2) × x) − 2` ms.
fn stage_ms(top_ms: f64, x: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    ((low + (high - low) * x).exp() - 2.0).max(0.0)
}

impl Engine {
    /// Applies `set_engine_par(par, value, group, slot)`. Returns whether the
    /// engine answers this parameter.
    pub(crate) fn set_engine_par(&mut self, par: i32, value: i32, group: i32, slot: i32) -> bool {
        let Some(pars) = self.plan.script.as_ref().map(|s| s.engine_pars) else {
            return false;
        };
        let x = f64::from(value.clamp(0, 1_000_000)) / 1e6;
        let frames = |ms: f64| (ms * f64::from(self.rate) / 1000.0).round() as u32;
        let Some(g) = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get_mut(g))
        else {
            return par == pars.intensity
                || [
                    pars.attack,
                    pars.hold,
                    pars.decay,
                    pars.sustain,
                    pars.release,
                ]
                .contains(&par);
        };
        if par == pars.intensity {
            if let Some(ir::Mod {
                kind: ir::ModKind::Volume { intensity, .. },
                ..
            }) = g.mod_mut(slot)
            {
                *intensity = x as f32;
                self.refresh_all(None);
            }
            return true;
        }
        let envelope = g
            .mod_mut(slot)
            .is_some_and(|m| m.kind == ir::ModKind::Envelope);
        let e = &mut g.envelope;
        let stage = match par {
            p if p == pars.attack => &mut e.attack,
            p if p == pars.hold => &mut e.hold,
            p if p == pars.decay => &mut e.decay,
            p if p == pars.release => &mut e.release,
            p if p == pars.sustain => {
                if envelope {
                    e.sustain = x as f32;
                }
                return true;
            }
            _ => return false,
        };
        if envelope {
            let top = if par == pars.attack || par == pars.hold {
                15_000.0
            } else {
                25_000.0
            };
            *stage = frames(stage_ms(top, x));
        }
        true
    }

    /// `set_controller`: the script moves a controller as if it had arrived.
    pub(crate) fn set_controller(&mut self, controller: i32, value: i32) {
        let (Ok(cc), Ok(value)) = (u8::try_from(controller), u8::try_from(value.clamp(0, 127)))
        else {
            return;
        };
        if let Some(v) = self.cc_values.get_mut(usize::from(cc)) {
            *v = value;
        }
        if cc < 128 {
            for channel in 0..16 {
                self.behaviour.set_cc(channel, cc, value);
            }
            self.refresh_all(Some(cc));
        }
    }

    /// Re-reads gain (and pitch, unless `cc` moved) of every sounding note.
    fn refresh_all(&mut self, cc: Option<u8>) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.select(&mut hits, |_| true);
        for &note in &hits {
            self.refresh_note(note, cc);
        }
        self.hits = hits;
    }
}

#[cfg(test)]
mod tests {
    use super::stage_ms;

    #[test]
    fn stage_times_follow_kontakts_shifted_exponential() {
        assert!(stage_ms(15_000.0, 0.0).abs() < 1e-9);
        assert!((stage_ms(15_000.0, 1.0) - 15_000.0).abs() < 1e-6);
        assert!((stage_ms(25_000.0, 0.5) - ((2.0f64 * 25_002.0).sqrt() - 2.0)).abs() < 1e-6);
    }
}
