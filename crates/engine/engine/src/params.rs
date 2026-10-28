//! What a script changes in the engine while it plays: modulation intensities,
//! envelope stages and controller values. Values arrive in KSP's 0..=1_000_000
//! encoding; the laws are Kontakt's (sampler-core `EngineParameterLaw`).

use kontra_ir as ir;

use crate::{Engine, Level};

/// Effect gains are cubic, unity at this value, so 1_000_000 is +24 dB (v2 sampler-core,
/// fitted to slots whose saved gains follow their scripts' init writes).
const UNITY_EFFECT_GAIN: f64 = 396_851.0;

/// Volume is cubic in the setting, 0 dB at this value, +12 dB at 1_000_000.
const UNITY_VOLUME: f64 = 629_960.524_947_436_1;

/// Attack and hold run 0..=15 s, decay and release 0..=25 s, exponentially
/// with a 2 ms shift: `exp(ln 2 + (ln(top + 2) − ln 2) × x) − 2` ms.
fn stage_ms(top_ms: f64, x: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    ((low + (high - low) * x).exp() - 2.0).max(0.0)
}

/// The inverse of [`stage_ms`]: the 0..=1 setting of a stage `ms` long.
/// The attack curve knob (raw / 500000 − 1: −1 concave ..= 1 convex) as a
/// segment shape: the law the Kontakt translator applies to stored curves
/// (format/kontakt/src/laws.rs, docs/re/engine/k-mod-envelopes.md).
fn attack_shape(x: f64) -> f32 {
    let c = (x * 2.0 - 1.0).clamp(-1.0, 1.0);
    let b = f64::from(((1.0 - c.abs()) * 500_000f64.ln() - 20_000f64.ln()).exp() as f32);
    let k = if c > 0.0 {
        (b / (1.0 + b)).ln()
    } else {
        ((1.0 + b) / b).ln()
    };
    k as f32
}

/// Inverse of [`attack_shape`], as a 0..=1 knob.
fn attack_x(shape: f32) -> f64 {
    let k = f64::from(shape);
    if k == 0.0 {
        return 0.5;
    }
    let b = 1.0 / k.abs().exp_m1();
    let c = (1.0 - (b.ln() + 20_000f64.ln()) / 500_000f64.ln()).clamp(0.0, 1.0);
    0.5 * (1.0 - k.signum() * c)
}

fn stage_x(top_ms: f64, ms: f64) -> f64 {
    let (low, high) = (2f64.ln(), (top_ms + 2.0).ln());
    (((ms + 2.0).ln() - low) / (high - low)).clamp(0.0, 1.0)
}

/// Which of a [`Level`]'s settings a parameter is.
#[derive(Clone, Copy)]
enum LevelPar {
    Volume,
    Pan,
    Tune,
}

impl LevelPar {
    fn set(self, level: &mut Level, raw: f64) {
        match self {
            Self::Volume => level.gain = (raw / UNITY_VOLUME).powi(3) as f32,
            Self::Pan => level.pan = ((raw - 500_000.0) / 500_000.0) as f32,
            // ±36 semitones.
            Self::Tune => level.tune = (raw - 500_000.0) * 7.2e-5,
        }
    }

    fn get(self, level: &Level) -> f64 {
        match self {
            Self::Volume => UNITY_VOLUME * f64::from(level.gain.max(0.0)).cbrt(),
            Self::Pan => f64::from(level.pan) * 500_000.0 + 500_000.0,
            Self::Tune => level.tune / 7.2e-5 + 500_000.0,
        }
    }
}

/// What an effect parameter sets in a slot.
#[derive(Clone, Copy)]
enum EffectPar {
    Bypass,
    Output,
    Gain,
    Width,
    Pan,
}

impl EffectPar {
    /// Sets it from the raw value; false when the slot has no such control.
    fn set(self, e: &mut ir::Effect, raw: f64) -> bool {
        let gain = (raw / UNITY_EFFECT_GAIN).powi(3) as f32;
        match (self, &mut e.kind) {
            (Self::Bypass, _) => e.bypass = raw != 0.0,
            (Self::Output, _) => e.output = gain,
            (Self::Gain, ir::EffectKind::Gain(g)) => *g = gain,
            // ponytail: width and pan linear in the setting; no measurement yet.
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => *width = (raw / 1e6) as f32,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => *pan = (raw / 5e5 - 1.0) as f32,
            _ => return false,
        }
        true
    }

    fn get(self, e: &ir::Effect) -> Option<f64> {
        let gain = |g: f32| UNITY_EFFECT_GAIN * f64::from(g.max(0.0)).cbrt();
        Some(match (self, e.kind) {
            (Self::Bypass, _) => f64::from(u8::from(e.bypass)),
            (Self::Output, _) => gain(e.output),
            (Self::Gain, ir::EffectKind::Gain(g)) => gain(g),
            (Self::Width, ir::EffectKind::Stereo { width, .. }) => f64::from(width) * 1e6,
            (Self::Pan, ir::EffectKind::Stereo { pan, .. }) => (f64::from(pan) + 1.0) * 5e5,
            _ => return None,
        })
    }
}

impl Engine {
    fn effect_par(&self, par: i32) -> Option<EffectPar> {
        let pars = self.plan.script.as_ref()?.engine_pars;
        match par {
            p if p == pars.effect_bypass => Some(EffectPar::Bypass),
            p if p == pars.effect_output => Some(EffectPar::Output),
            p if p == pars.gainer_gain => Some(EffectPar::Gain),
            p if p == pars.stereo_width => Some(EffectPar::Width),
            p if p == pars.stereo_pan => Some(EffectPar::Pan),
            _ => None,
        }
    }

    /// A group's insert slot (generic -1), the instrument insert's (group
    /// -1, generic `$NI_INSERT_BUS` = 1) or a bus's. Send and main racks are
    /// not carried yet.
    fn effect_mut(&mut self, group: i32, slot: i32, generic: i32) -> Option<&mut ir::Effect> {
        let rack = match (usize::try_from(group), generic) {
            (Ok(g), -1) => &mut self.groups.get_mut(g)?.inserts[..],
            // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0 (one rack per slot), `$NI_MAIN_BUS` 2.
            (Err(_), 1) => &mut self.rack.slots[..],
            (Err(_), 0) => {
                let send = self.sends.get_mut(usize::try_from(slot).ok()?)?;
                return send.slots.first_mut()?.as_mut();
            }
            (Err(_), 2) => &mut self.main.slots[..],
            // `$NI_BUS_OFFSET + n`: bus n is plan bus n + 1 (0 is the main bus).
            (Err(_), b) if b >= 1000 => {
                let bus = usize::try_from(b - 999).ok()?;
                &mut self.buses.get_mut(bus)?.rack.slots[..]
            }
            _ => return None,
        };
        rack.get_mut(usize::try_from(slot).ok()?)?.as_mut()
    }

    /// `set_engine_par` of an effect control; false when not carried.
    fn set_effect(&mut self, par: i32, value: i32, group: i32, slot: i32, generic: i32) -> bool {
        let Some(which) = self.effect_par(par) else {
            return false;
        };
        let raw = f64::from(value.clamp(0, 1_000_000));
        let set = self
            .effect_mut(group, slot, generic)
            .is_some_and(|e| which.set(e, raw));
        if set {
            self.refresh_all(None);
        }
        set
    }

    fn level_par(&self, par: i32) -> Option<LevelPar> {
        let pars = self.plan.script.as_ref()?.engine_pars;
        match par {
            p if p == pars.volume => Some(LevelPar::Volume),
            p if p == pars.pan => Some(LevelPar::Pan),
            p if p == pars.tune => Some(LevelPar::Tune),
            _ => None,
        }
    }

    /// A group's level, or the instrument's (group -1, generic -1). Buses
    /// (generic `$NI_BUS_OFFSET + n`) are not answered yet.
    fn level_mut(&mut self, group: i32, generic: i32) -> Option<&mut Level> {
        match usize::try_from(group) {
            Ok(g) => self.groups.get_mut(g).map(|g| &mut g.level),
            Err(_) if generic == -1 => Some(&mut self.instrument),
            Err(_) => None,
        }
    }

    /// `set_engine_par` of volume, pan or tune; false when not one of them.
    fn set_level(&mut self, par: i32, value: i32, group: i32, generic: i32) -> bool {
        let Some(which) = self.level_par(par) else {
            return false;
        };
        let Some(level) = self.level_mut(group, generic) else {
            return false;
        };
        which.set(level, f64::from(value.clamp(0, 1_000_000)));
        self.refresh_all(None);
        true
    }

    /// Answers `get_engine_par(par, group, slot)` for what
    /// [`Engine::set_engine_par`] sets, in the same encoding, so a script that
    /// saves and restores a parameter gets back what it had. `None`: not answered.
    pub(crate) fn get_engine_par(
        &mut self,
        par: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> Option<i32> {
        if let Some(which) = self.effect_par(par) {
            let e = *self.effect_mut(group, slot, generic)?;
            return Some(which.get(&e)?.round().clamp(0.0, 1e6) as i32);
        }
        if let Some(which) = self.level_par(par) {
            let level = *self.level_mut(group, generic)?;
            return Some(which.get(&level).round().clamp(0.0, 1e6) as i32);
        }
        let pars = self.plan.script.as_ref()?.engine_pars;
        let g = usize::try_from(group)
            .ok()
            .and_then(|g| self.groups.get(g))?;
        let ms = |frames: u32| f64::from(frames) * 1000.0 / f64::from(self.rate);
        let e = &g.envelope;
        let x = match par {
            p if p == pars.intensity => {
                match g.mods.iter().find(|m| i32::from(m.slot) == slot)?.kind {
                    ir::ModKind::Volume { intensity, .. } => f64::from(intensity),
                    _ => return None,
                }
            }
            p if p == pars.attack => stage_x(15_000.0, ms(e.attack)),
            p if p == pars.hold => stage_x(15_000.0, ms(e.hold)),
            p if p == pars.decay => stage_x(25_000.0, ms(e.decay)),
            p if p == pars.release => stage_x(25_000.0, ms(e.release)),
            p if p == pars.sustain => f64::from(e.sustain),
            p if p == pars.attack_curve => attack_x(e.attack_shape),
            _ => return None,
        };
        Some((x * 1e6).round() as i32)
    }

    /// Applies `set_engine_par(par, value, group, slot)`. Returns whether the
    /// engine answers this parameter.
    pub(crate) fn set_engine_par(
        &mut self,
        par: i32,
        value: i32,
        group: i32,
        slot: i32,
        generic: i32,
    ) -> bool {
        if self.set_level(par, value, group, generic)
            || self.set_effect(par, value, group, slot, generic)
        {
            return true;
        }
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
                    pars.attack_curve,
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
            p if p == pars.attack_curve => {
                if envelope {
                    e.attack_shape = attack_shape(x);
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
    use super::{attack_shape, attack_x, stage_ms, stage_x};

    #[test]
    fn attack_curve_round_trips() {
        for x in [0.1, 0.3, 0.5, 0.7, 0.95] {
            assert!((attack_x(attack_shape(x)) - x).abs() < 1e-4, "{x}");
        }
    }

    #[test]
    fn stage_times_follow_kontakts_shifted_exponential() {
        assert!(stage_ms(15_000.0, 0.0).abs() < 1e-9);
        assert!((stage_ms(15_000.0, 1.0) - 15_000.0).abs() < 1e-6);
        assert!((stage_ms(25_000.0, 0.5) - ((2.0f64 * 25_002.0).sqrt() - 2.0)).abs() < 1e-6);
        for x in [0.0, 0.25, 0.8, 1.0] {
            assert!((stage_x(25_000.0, stage_ms(25_000.0, x)) - x).abs() < 1e-9);
        }
    }
}
