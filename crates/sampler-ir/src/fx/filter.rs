//! `filter` family: the shared names every format fills in.
//!
//! One superset `Config` (topology, response, poles, drive, key tracking, cutoff and
//! resonance laws) describes a Kontakt group/bus/insert filter or a UVI filter. `Config::
//! processors` turns it into the IR processors the engine runs, so the knob laws live in one
//! place and a format only chooses the configuration.
//!
//! A type whose law or kernel is not proved is not given a `Config`: its mapping returns a
//! [`Gap`] naming the precise reason, and a stand-in is reported through [`Fidelity`].

use crate::{
    Ar, Daft, Filter, FilterKind, Frequency, LadderLP4, Processor, Resonance, SlotAddress,
};

/// The passband a topology realises.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Response {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    /// Ladder "Peak": the native peak row of the Ladder family.
    Peak,
}

/// The structure that filters the signal.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    /// Cascaded zero-delay state-variable sections (`poles / 2`), one-pole for `poles == 1`.
    Svf,
    /// Native Ladder family (Kontakt saved types 30..=41): `Response` and `poles` pick the
    /// output row, `drive` is its signed Gain control.
    Ladder,
    /// Native adaptive-resonance filter (FilterDJ, saved types 100..=108). `poles` 2 or 4, or
    /// 0 for the combined 2/4 types.
    Ar,
    /// Native Daft low or high pass; `drive` is its Gain control.
    Daft,
    /// Native Formant I (Talk = `cutoff`, Sharp = `resonance`, Size = `extra`).
    Formant,
    /// The second table of the Formant core (rows 5..=9): Kontakt Formant II.
    FormantII,
    /// Kontakt's legacy BFilter classes (saved types 2..=4, 6..=12): first- and second-order
    /// direct-form sections whose native law the engine owns. `Response` and `poles` name the
    /// class (BR4 is `Notch`, 4); cutoff and resonance are the stored knobs. [`Variant`]
    /// names the members response and poles do not (Vowel, Pro-53, Legacy Ladder, Versatile).
    Cascade,
}

/// The member of a native family that `Topology`, `Response` and `poles` do not name.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Variant {
    /// Named by response and poles.
    Default,
    /// Kontakt Vowel A / Vowel B (saved types 14 / 15).
    VowelA,
    VowelB,
    /// Kontakt Pro-53 (saved type 16).
    Pro52,
    /// Kontakt Legacy Ladder (saved type 17).
    Ladder,
    /// Kontakt 3x2 Versatile (saved type 19) and its stored knobs.
    Versatile(Versatile),
    /// Kontakt SV LP1..Notch 6 (saved types 50..=62), named by response and poles.
    Sv,
    /// Kontakt SV Par. LP/HP, Par. BP/BP, Ser. LP/HP (saved 63..=65: `mode` 0..=2) and the
    /// stored bandwidth.
    MultiSv { mode: u8, bandwidth: f32 },
}

/// The 3x2 Versatile knobs beyond cutoff and resonance, as stored (0..=1).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Versatile {
    /// SHIFTB, SHIFTC.
    pub shift: [f32; 2],
    /// RESB, RESC.
    pub resonance: [f32; 2],
    /// FILTER_TYPEA/B/C: low -> band -> high morph.
    pub types: [f32; 3],
    /// BYPA/B/C: each filter's wet amount.
    pub wet: [f32; 3],
    /// FILTER_GAIN.
    pub gain: f32,
}

/// How the stored cutoff knob becomes a frequency.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutoffLaw {
    /// The value is hertz already.
    Hertz,
    /// Kontakt SV filters: `25 * 800^x` Hz.
    Sv,
    /// The native kernel owns the law and takes the 0..=1 knob as stored.
    Native,
}

/// How the stored resonance knob becomes a Q.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResonanceLaw {
    /// The value is Q already.
    Q,
    /// Kontakt SV filters: `1/Q = 1.987 (1 - r)^3.1 + 0.013`.
    Sv,
    /// The native kernel owns the law and takes the 0..=1 knob as stored.
    Native,
}

/// How far a configuration is proved against the native effect.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fidelity {
    /// Native kernel and laws checked against extracted vectors.
    Exact,
    /// Native laws measured or extracted, but the kernel, the cadence or the other family
    /// members are not checked; the reason is carried by the mapping.
    Partial,
    /// A clearly labelled stand-in: right response shape, laws not established.
    Generic,
}

/// Where a native kernel is bound to a Kontakt slot (live controls and modulation).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub address: SlotAddress,
    pub record_version: u16,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub topology: Topology,
    pub response: Response,
    /// Poles of the whole filter (SVF: 1, 2, 4 or 6; Ladder 1..=4; AR 2, 4 or 0 = 2/4).
    pub poles: u8,
    pub cutoff: f64,
    pub cutoff_law: CutoffLaw,
    pub resonance: f64,
    pub resonance_law: ResonanceLaw,
    /// Ladder and Daft Gain control, as stored.
    pub drive: f64,
    /// Octaves of cutoff per octave of key distance from middle C (UVI `KeyTracking`).
    pub key_tracking: f64,
    /// Topology-specific third knob (Formant Size).
    pub extra: f64,
    pub variant: Variant,
    pub binding: Option<Binding>,
}

/// A native filter that cannot be configured, and exactly why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gap {
    pub name: &'static str,
    pub reason: &'static str,
}

impl Config {
    /// A configuration with no drive, tracking or binding.
    pub const fn new(topology: Topology, response: Response, poles: u8) -> Self {
        Self {
            topology,
            response,
            poles,
            cutoff: 0.0,
            cutoff_law: CutoffLaw::Native,
            resonance: 0.0,
            resonance_law: ResonanceLaw::Native,
            drive: 0.0,
            key_tracking: 0.0,
            extra: 0.0,
            variant: Variant::Default,
            binding: None,
        }
    }

    /// Cutoff in hertz for the laws that produce one; `key` is the MIDI key for tracking.
    pub fn hertz(&self, key: Option<u8>) -> Option<f64> {
        let base = match self.cutoff_law {
            CutoffLaw::Hertz => self.cutoff,
            CutoffLaw::Sv => sv_hertz(self.cutoff as f32),
            CutoffLaw::Native => return None,
        };
        let tracked = key.map_or(0.0, |k| (f64::from(k) - 60.0) / 12.0) * self.key_tracking;
        Some(base * tracked.exp2())
    }

    /// Q for the laws that produce one.
    pub fn q(&self) -> Option<f64> {
        match self.resonance_law {
            ResonanceLaw::Q => Some(self.resonance),
            ResonanceLaw::Sv => Some(sv_q(self.resonance as f32)),
            ResonanceLaw::Native => None,
        }
    }

    /// The IR processors realising this configuration.
    pub fn processors(&self, key: Option<u8>) -> Result<Vec<Processor>, Gap> {
        let gap = |reason| Gap { name: "filter", reason };
        match self.topology {
            Topology::Svf => {
                let (Some(hz), Some(q)) = (self.hertz(key), self.q()) else {
                    return Err(gap("SVF needs a cutoff and a resonance law"));
                };
                let poles = self.poles;
                let kind = match (self.response, poles) {
                    (Response::LowPass, 1 | 2 | 4 | 6) => FilterKind::LowPass { poles },
                    (Response::HighPass, 1 | 2 | 4 | 6) => FilterKind::HighPass { poles },
                    (Response::BandPass, 2 | 4 | 6) => FilterKind::BandPass { poles },
                    (Response::Notch, 2 | 4 | 6) => FilterKind::Notch { poles },
                    _ => return Err(gap("pole count not realised by cascaded SVF sections")),
                };
                Ok(vec![Processor::Filter(Filter {
                    kind,
                    cutoff: Frequency::Hertz(hz),
                    resonance: Resonance::Q(q),
                })])
            }
            Topology::Ladder => {
                let taps = ladder_taps(self.response, self.poles)
                    .ok_or(gap("no Ladder output row for this response and pole count"))?;
                let binding = self.binding;
                Ok(vec![Processor::LadderLP4(LadderLP4 {
                    address: binding.map(|b| b.address),
                    gain: self.drive,
                    cutoff: self.cutoff,
                    resonance: self.resonance,
                    record_version: binding.map_or(0, |b| b.record_version),
                    taps,
                })])
            }
            Topology::Ar => {
                let group = match self.poles {
                    2 => 0,
                    4 => 1,
                    0 => 2,
                    _ => return Err(gap("AR filters are 2-pole, 4-pole or combined 2/4")),
                };
                let band = match self.response {
                    Response::LowPass => 0,
                    Response::BandPass => 1,
                    Response::HighPass => 2,
                    _ => return Err(gap("AR filters are low, band or high pass")),
                };
                Ok(vec![Processor::Ar(Ar {
                    // Saved types 100..=108 run pole group major: LP2 BP2 HP2 LP4 BP4 HP4 ...
                    mode: group * 3 + band,
                    cutoff: self.cutoff,
                    resonance: self.resonance,
                })])
            }
            Topology::Daft => Ok(vec![Processor::Daft(Daft {
                gain: self.drive,
                cutoff: self.cutoff,
                resonance: self.resonance,
                highpass: match self.response {
                    Response::LowPass => false,
                    Response::HighPass => true,
                    _ => return Err(gap("Daft is a low or high pass")),
                },
            })]),
            Topology::Formant => Ok(vec![Processor::Formant([
                self.cutoff as f32,
                self.resonance as f32,
                self.extra as f32,
            ])]),
            Topology::FormantII => Ok(vec![Processor::FilterBank(*self)]),
            Topology::Cascade => match (self.response, self.poles) {
                _ if self.variant != Variant::Default => Ok(vec![Processor::FilterBank(*self)]),
                (Response::LowPass, 1 | 2 | 4 | 6)
                | (Response::HighPass, 1 | 2 | 4)
                | (Response::BandPass, 2 | 4)
                | (Response::Notch, 4) => Ok(vec![Processor::FilterBank(*self)]),
                _ => Err(gap("no legacy class for this response and pole count")),
            },
        }
    }
}

/// Ladder output row (saved Ladder type minus 29): LP1..4, HP1..4, BP2, BP4, Peak, Notch.
pub fn ladder_taps(response: Response, poles: u8) -> Option<u8> {
    Some(match (response, poles) {
        (Response::LowPass, 1..=4) => poles,
        (Response::HighPass, 1..=4) => 4 + poles,
        (Response::BandPass, 2) => 9,
        (Response::BandPass, 4) => 10,
        (Response::Peak, _) => 11,
        (Response::Notch, _) => 12,
        _ => return None,
    })
}

/// Kontakt's SV filters: the knob is 25 Hz * 800^x (measured on SV LP2 against Kontakt 8).
pub fn sv_hertz(x: f32) -> f64 {
    25.0 * 800f64.powf(f64::from(x))
}

/// Kontakt's SV filters: resonance r gives `k = 1/Q = (2 - 0.013) (1 - r)^3.1 + 0.013`
/// (measured on SV LP2). The measured passband loss at high resonance (-6 dB at r = 1) is
/// not part of this law.
pub fn sv_q(r: f32) -> f64 {
    1.0 / ((2.0 - 0.013) * (1.0 - f64::from(r)).powf(3.1) + 0.013)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(response: Response, poles: u8, cutoff: f64, resonance: f64) -> Config {
        Config {
            cutoff,
            cutoff_law: CutoffLaw::Sv,
            resonance,
            resonance_law: ResonanceLaw::Sv,
            ..Config::new(Topology::Svf, response, poles)
        }
    }

    #[test]
    fn sv_laws_match_the_measured_lp2() {
        let c = sv(Response::LowPass, 2, 0.293, 0.0);
        assert!((c.hertz(None).unwrap() - 25.0 * 800f64.powf(0.293f32 as f64)).abs() < 1e-9);
        // Resonance 0 is Q = 0.5, the two cascaded one-poles of the measurement.
        assert!((c.q().unwrap() - 0.5).abs() < 1e-3);
        assert!(sv(Response::LowPass, 2, 0.5, 1.0).q().unwrap() > 50.0);
    }

    #[test]
    fn svf_poles_and_responses() {
        let one = sv(Response::HighPass, 1, 0.5, 0.0).processors(None).unwrap();
        assert!(matches!(
            one[..],
            [Processor::Filter(Filter { kind: FilterKind::HighPass { poles: 1 }, .. })]
        ));
        for (response, poles) in [
            (Response::BandPass, 4),
            (Response::Notch, 4),
            (Response::LowPass, 6),
            (Response::HighPass, 6),
        ] {
            assert_eq!(sv(response, poles, 0.5, 0.0).processors(None).unwrap().len(), 1);
        }
        assert!(sv(Response::BandPass, 1, 0.5, 0.0).processors(None).is_err());
        assert!(sv(Response::LowPass, 3, 0.5, 0.0).processors(None).is_err());
    }

    #[test]
    fn key_tracking_scales_the_cutoff_by_octaves() {
        let c = Config {
            cutoff: 1000.0,
            cutoff_law: CutoffLaw::Hertz,
            key_tracking: 1.0,
            ..sv(Response::LowPass, 2, 0.0, 0.0)
        };
        assert_eq!(c.hertz(Some(72)).unwrap(), 2000.0);
        assert_eq!(c.hertz(None).unwrap(), 1000.0);
        assert_eq!(Config { key_tracking: 0.5, ..c }.hertz(Some(72)).unwrap(), 1000.0 * 2f64.sqrt());
    }

    #[test]
    fn native_topologies_keep_their_saved_modes() {
        let ar = |poles, response| Config::new(Topology::Ar, response, poles).processors(None);
        for (poles, response, mode) in [
            (2, Response::LowPass, 0),
            (2, Response::HighPass, 2),
            (4, Response::LowPass, 3),
            (4, Response::BandPass, 4),
            (0, Response::LowPass, 6),
            (0, Response::HighPass, 8),
        ] {
            assert!(matches!(ar(poles, response).unwrap()[..], [Processor::Ar(a)] if a.mode == mode));
        }
        // Ladder rows are the saved type minus 29.
        for (response, poles, saved) in [
            (Response::LowPass, 1, 30),
            (Response::LowPass, 4, 33),
            (Response::HighPass, 1, 34),
            (Response::HighPass, 4, 37),
            (Response::BandPass, 2, 38),
            (Response::BandPass, 4, 39),
            (Response::Peak, 4, 40),
            (Response::Notch, 4, 41),
        ] {
            assert_eq!(ladder_taps(response, poles), Some(saved - 29));
        }
    }
}
