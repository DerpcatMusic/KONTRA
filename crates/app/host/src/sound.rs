//! A part's sound as the editor shows and edits it: each group's amplitude
//! envelope and its filter and EQ bands, in seconds, hertz and decibels. An
//! edit changes the copy here and gives the values the engine takes
//! ([`kontra_engine::Engine::set_group_sound`]); they apply from the next
//! note.

use kontra_engine::{Ahdsr, ir};

/// A value of a group's sound: an envelope stage (seconds; sustain 0..=1),
/// or band n's frequency, Q or gain (dB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Hz(usize),
    Q(usize),
    Gain(usize),
}

/// A filter or EQ band: where it sits (insert slot, band of an EQ), and
/// what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub slot: usize,
    pub band: Option<usize>,
    pub kind: BandKind,
    pub hz: f64,
    pub q: f64,
    pub gain_db: f64,
    /// A filter's poles; 0 for an EQ band.
    pub poles: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandKind {
    Low,
    High,
    Band,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
}

/// Every group's envelope and inserts, as the engine plays them.
#[derive(Clone, Debug, Default)]
pub struct Sounds {
    rate: u32,
    groups: Vec<(Ahdsr, Vec<Option<ir::Effect>>)>,
    /// Moves on every edit, for the editor to copy again.
    pub revision: u64,
}

impl Sounds {
    /// From each group's envelope and inserts.
    #[must_use]
    pub fn new(rate: u32, groups: Vec<(Ahdsr, Vec<Option<ir::Effect>>)>) -> Self {
        Self {
            rate,
            groups,
            revision: 0,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Group `g`'s envelope: attack, hold, decay, sustain, release.
    #[must_use]
    pub fn envelope(&self, g: usize) -> Option<[f64; 5]> {
        let (e, _) = self.groups.get(g)?;
        let s = |frames: u32| f64::from(frames) / f64::from(self.rate.max(1));
        Some([
            s(e.attack),
            s(e.hold),
            s(e.decay),
            f64::from(e.sustain),
            s(e.release),
        ])
    }

    /// Group `g`'s filters and EQ bands, in series.
    #[must_use]
    pub fn bands(&self, g: usize) -> Vec<Band> {
        let Some((_, inserts)) = self.groups.get(g) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (slot, e) in inserts.iter().enumerate() {
            match e.as_ref().map(|e| e.kind) {
                Some(ir::EffectKind::Filter(f)) => out.push(filter_band(slot, &f)),
                Some(ir::EffectKind::Eq(bells)) => {
                    out.extend((bells.iter().enumerate()).map(|(n, b)| Band {
                        slot,
                        band: Some(n),
                        kind: match b.shape {
                            ir::Shape::Peak => BandKind::Peak,
                            ir::Shape::LowShelf => BandKind::LowShelf,
                            ir::Shape::HighShelf => BandKind::HighShelf,
                        },
                        hz: f64::from(b.hz),
                        q: f64::from(b.q),
                        gain_db: f64::from(b.gain_db),
                        poles: 0,
                    }));
                }
                _ => {}
            }
        }
        out
    }

    /// Sets `param` of group `g` to `value`; the group's envelope and the
    /// insert that changed, for the engine. `None` when there is no such
    /// group or band.
    pub fn set(
        &mut self,
        g: usize,
        param: Param,
        value: f64,
    ) -> Option<(Ahdsr, Option<(usize, ir::Effect)>)> {
        let band = match param {
            Param::Hz(n) | Param::Q(n) | Param::Gain(n) => Some(*self.bands(g).get(n)?),
            _ => None,
        };
        let rate = f64::from(self.rate);
        let (e, inserts) = self.groups.get_mut(g)?;
        let frames = |s: f64| (s.max(0.) * rate).round() as u32;
        let mut changed = None;
        match (param, band) {
            (Param::Attack, _) => e.attack = frames(value),
            (Param::Hold, _) => e.hold = frames(value),
            (Param::Decay, _) => e.decay = frames(value),
            (Param::Sustain, _) => e.sustain = value.clamp(0., 1.) as f32,
            (Param::Release, _) => e.release = frames(value),
            (_, Some(b)) => {
                let effect = inserts.get_mut(b.slot)?.as_mut()?;
                set_band(&mut effect.kind, b.band, param, value as f32);
                changed = Some((b.slot, *effect));
            }
            _ => return None,
        }
        self.revision += 1;
        Some((*e, changed))
    }
}

fn filter_band(slot: usize, f: &ir::Filter) -> Band {
    let k = f
        .damping
        .get(usize::from(f.stages.max(1)) - 1)
        .copied()
        .unwrap_or(1.);
    Band {
        slot,
        band: None,
        kind: match f.pass {
            ir::Pass::Low => BandKind::Low,
            ir::Pass::High => BandKind::High,
            ir::Pass::Band => BandKind::Band,
            ir::Pass::Notch => BandKind::Notch,
        },
        hz: f64::from(f.hz),
        q: 1. / f64::from(k.max(1e-6)),
        gain_db: 20. * f64::from(f.gain.max(1e-9)).log10(),
        poles: (2 * f.stages).max(1),
    }
}

/// An EQ band's or filter's frequency, Q or gain. A filter's Q scales every
/// stage's damping alike, so a cascade keeps its shape.
fn set_band(kind: &mut ir::EffectKind, band: Option<usize>, param: Param, v: f32) {
    match (kind, band) {
        (ir::EffectKind::Eq(bells), Some(n)) => {
            let Some(b) = bells.get_mut(n) else { return };
            match param {
                Param::Hz(_) => b.hz = v.max(1.),
                Param::Q(_) => b.q = v.max(0.01),
                Param::Gain(_) => b.gain_db = v,
                _ => {}
            }
        }
        (ir::EffectKind::Filter(f), None) => match param {
            Param::Hz(_) => f.hz = v.max(1.),
            Param::Q(_) => {
                let last = usize::from(f.stages.max(1)) - 1;
                let now = f.damping.get(last).copied().unwrap_or(1.).max(1e-6);
                let scale = (1. / v.max(0.01)) / now;
                for k in &mut f.damping {
                    *k *= scale;
                }
            }
            Param::Gain(_) => f.gain = 10f32.powf(v / 20.),
            _ => {}
        },
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decay_and_a_band_edit_reach_the_values_the_engine_takes() {
        let eq = Some(ir::Effect {
            kind: ir::EffectKind::Eq([ir::Bell::FLAT; 4]),
            bypass: false,
            output: 1.,
            dry: 0.,
        });
        let mut s = Sounds::new(48_000, vec![(Ahdsr::GATE, vec![eq])]);
        let (e, insert) = s.set(0, Param::Decay, 0.5).unwrap();
        assert_eq!((e.decay, insert), (24_000, None));
        assert_eq!(s.envelope(0).map(|e| e[2]), Some(0.5));
        let (_, insert) = s.set(0, Param::Gain(2), -6.).unwrap();
        let Some((
            0,
            ir::Effect {
                kind: ir::EffectKind::Eq(b),
                ..
            },
        )) = insert
        else {
            panic!("{insert:?}");
        };
        assert_eq!(b[2].gain_db, -6.);
        assert_eq!(s.bands(0)[2].gain_db, -6.);
        assert_eq!(s.revision, 2);
        assert!(s.set(0, Param::Hz(9), 1.).is_none());
    }
}
