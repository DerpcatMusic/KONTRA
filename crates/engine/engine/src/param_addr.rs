//! Format-neutral, real-time safe live parameter addressing via
//! [`kontra_ir::Scope`] and [`kontra_ir::ParamAddr`].

use kontra_ir::{EffectKind, ParamAddr, Scope};

use crate::Engine;

impl Engine {
    /// Writes a live parameter addressed by `(scope, addr)` with zero heap
    /// allocations. Returns `true` if the target existed and was updated.
    pub fn set_param(&mut self, scope: Scope, addr: ParamAddr, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        match (scope, addr) {
            (Scope::Instrument, ParamAddr::Volume) => {
                self.instrument.gain = value.max(0.0);
                true
            }
            (Scope::Instrument, ParamAddr::Pan) => {
                self.instrument.pan = value.clamp(-1.0, 1.0);
                true
            }
            (Scope::Instrument, ParamAddr::Pitch) => {
                self.instrument.tune = f64::from(value);
                true
            }
            (Scope::Bus(b), ParamAddr::Volume) => {
                let Some(bus) = self.buses.get_mut(usize::from(b)) else {
                    return false;
                };
                bus.gain = value.max(0.0);
                true
            }
            (Scope::Bus(b), ParamAddr::Pan) => {
                let Some(bus) = self.buses.get_mut(usize::from(b)) else {
                    return false;
                };
                bus.pan = value.clamp(-1.0, 1.0);
                true
            }
            (Scope::Bus(b), ParamAddr::BusSend { send }) => {
                let Some(s) = self
                    .buses
                    .get_mut(usize::from(b))
                    .and_then(|bus| bus.sends.get_mut(usize::from(send)))
                else {
                    return false;
                };
                s.level = value.max(0.0);
                true
            }
            (Scope::Group(g), ParamAddr::Volume) => {
                let Some(group) = self.groups.get_mut(usize::from(g)) else {
                    return false;
                };
                group.level.gain = value.max(0.0);
                true
            }
            (Scope::Group(g), ParamAddr::Pan) => {
                let Some(group) = self.groups.get_mut(usize::from(g)) else {
                    return false;
                };
                group.level.pan = value.clamp(-1.0, 1.0);
                true
            }
            (Scope::Group(g), ParamAddr::Pitch) => {
                let Some(group) = self.groups.get_mut(usize::from(g)) else {
                    return false;
                };
                group.level.tune = f64::from(value);
                true
            }
            (Scope::Group(g), ParamAddr::FilterCutoff { insert, .. }) => {
                let gi = usize::from(g);
                let Some(Some(effect)) = self
                    .groups
                    .get_mut(gi)
                    .and_then(|gr| gr.inserts.get_mut(usize::from(insert)))
                else {
                    return false;
                };
                let EffectKind::Filter(ref mut f) = effect.kind else {
                    return false;
                };
                f.hz = value.clamp(20.0, 22_000.0);
                self.group_written(gi);
                true
            }
            (Scope::Group(g), ParamAddr::FilterResonance { insert }) => {
                let gi = usize::from(g);
                let Some(Some(effect)) = self
                    .groups
                    .get_mut(gi)
                    .and_then(|gr| gr.inserts.get_mut(usize::from(insert)))
                else {
                    return false;
                };
                let EffectKind::Filter(ref mut f) = effect.kind else {
                    return false;
                };
                let k = value.clamp(0.02, 2.0);
                f.damping = [k; 3];
                self.group_written(gi);
                true
            }
            (Scope::Group(g), ParamAddr::EqBandGain { insert, band }) => {
                let gi = usize::from(g);
                let Some(Some(effect)) = self
                    .groups
                    .get_mut(gi)
                    .and_then(|gr| gr.inserts.get_mut(usize::from(insert)))
                else {
                    return false;
                };
                let EffectKind::Eq(ref mut bands) = effect.kind else {
                    return false;
                };
                let Some(b) = bands.get_mut(usize::from(band)) else {
                    return false;
                };
                b.gain_db = value.clamp(-24.0, 24.0);
                self.group_written(gi);
                true
            }
            _ => false,
        }
    }

    /// Reads the current value of a live parameter addressed by `(scope, addr)`.
    #[must_use]
    pub fn get_param(&self, scope: Scope, addr: ParamAddr) -> Option<f32> {
        match (scope, addr) {
            (Scope::Instrument, ParamAddr::Volume) => Some(self.instrument.gain),
            (Scope::Instrument, ParamAddr::Pan) => Some(self.instrument.pan),
            (Scope::Instrument, ParamAddr::Pitch) => Some(self.instrument.tune as f32),
            (Scope::Bus(b), ParamAddr::Volume) => {
                self.buses.get(usize::from(b)).map(|bus| bus.gain)
            }
            (Scope::Bus(b), ParamAddr::Pan) => self.buses.get(usize::from(b)).map(|bus| bus.pan),
            (Scope::Bus(b), ParamAddr::BusSend { send }) => self
                .buses
                .get(usize::from(b))
                .and_then(|bus| bus.sends.get(usize::from(send)))
                .map(|s| s.level),
            (Scope::Group(g), ParamAddr::Volume) => {
                self.groups.get(usize::from(g)).map(|gr| gr.level.gain)
            }
            (Scope::Group(g), ParamAddr::Pan) => {
                self.groups.get(usize::from(g)).map(|gr| gr.level.pan)
            }
            (Scope::Group(g), ParamAddr::Pitch) => self
                .groups
                .get(usize::from(g))
                .map(|gr| gr.level.tune as f32),
            (Scope::Group(g), ParamAddr::FilterCutoff { insert, .. }) => {
                let eff = self
                    .groups
                    .get(usize::from(g))?
                    .inserts
                    .get(usize::from(insert))?
                    .as_ref()?;
                match eff.kind {
                    EffectKind::Filter(f) => Some(f.hz),
                    _ => None,
                }
            }
            (Scope::Group(g), ParamAddr::FilterResonance { insert }) => {
                let eff = self
                    .groups
                    .get(usize::from(g))?
                    .inserts
                    .get(usize::from(insert))?
                    .as_ref()?;
                match eff.kind {
                    EffectKind::Filter(f) => Some(f.damping[0]),
                    _ => None,
                }
            }
            (Scope::Group(g), ParamAddr::EqBandGain { insert, band }) => {
                let eff = self
                    .groups
                    .get(usize::from(g))?
                    .inserts
                    .get(usize::from(insert))?
                    .as_ref()?;
                match eff.kind {
                    EffectKind::Eq(bands) => bands.get(usize::from(band)).map(|b| b.gain_db),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}
