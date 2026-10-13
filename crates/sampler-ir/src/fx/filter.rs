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
}

/// How the stored cutoff knob becomes a frequency.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutoffLaw {
    /// The value is hertz already.
    Hertz,
    /// Kontakt SV filters: `25 * 800^x` Hz.
    Sv,
    /// Kontakt legacy filters (pinned v1): `43.6 * 2^(8.96 x)` Hz.
    Legacy,
    /// Kontakt legacy BFilter1Pole/2Pole/4Pole/6Pole: a fixed Butterworth prototype moved by
    /// an allpass warp, as the hertz of its -3 dB point (see [`legacy_warp_hertz`]).
    /// `Response` and `poles` pick the class's warp constant.
    LegacyWarp,
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
    /// Kontakt legacy filters (pinned v1): `Q = 0.7071 * 28^r`.
    Legacy,
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
            binding: None,
        }
    }

    /// Cutoff in hertz for the laws that produce one; `key` is the MIDI key for tracking.
    pub fn hertz(&self, key: Option<u8>) -> Option<f64> {
        let base = match self.cutoff_law {
            CutoffLaw::Hertz => self.cutoff,
            CutoffLaw::Sv => sv_hertz(self.cutoff as f32),
            CutoffLaw::Legacy => legacy_hertz(self.cutoff as f32),
            CutoffLaw::LegacyWarp => legacy_warp_hertz(
                self.cutoff as f32,
                match (self.response, self.poles) {
                    (Response::HighPass, 2) => Warp::HighPass2,
                    (Response::HighPass, _) => Warp::HighPass,
                    _ => Warp::LowPass,
                },
            ),
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
            ResonanceLaw::Legacy => Some(legacy_q(self.resonance as f32)),
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

/// Octaves spanned by the legacy cutoff knob.
pub const LEGACY_CUTOFF_OCTAVES: f64 = 8.96;

/// Pinned v1 legacy cutoff law, in f32 as v1 evaluates it.
pub fn legacy_hertz(x: f32) -> f64 {
    f64::from(43.6f32 * (LEGACY_CUTOFF_OCTAVES as f32 * x.clamp(0., 1.)).exp2())
}

/// The allpass warp of a legacy filter class: its angle scale (the first f64 after pi/2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warp {
    /// LP1/LP2/LP4/LP6 (0.99, then 0.9987809049669; FUN_14095a5a0, 14095b2d0, 14095c9c0, 14095cf60).
    LowPass,
    /// HP1/HP4 (0.82, then 0.99713317384107; FUN_14095a200, 14095c410).
    HighPass,
    /// HP2 (0.92, then 0.99713317384107; FUN_14095ae80).
    HighPass2,
}

/// Kontakt legacy cutoff law, extracted from the BFilter1Pole/2Pole/4Pole/6Pole setCutoff
/// functions (Kontakt 8.13.1) and evaluated in the binary's own truncated series and f32
/// operation order.
///
/// Every class designs a Butterworth prototype at a fixed `tan(w/2) = 0.5463`
/// (FUN_14082a0b0, sections at `(2k+1) pi / 2N`) and moves it with one allpass warp, so at
/// zero resonance the -3 dB point is the same for every order of one class.
///
/// The native filter is a fixed prototype (`b0 = b1 = 0.26276`, pole `0.29341`, constants at
/// `FUN_1408de6e0`) warped by an allpass whose pole is `sin(.5-x)/sin(.5+x)` (LP) or
/// `-cos(x+.5)/cos(x-.5)` (HP). The result is a zero at Nyquist (LP) or DC (HP) and one real
/// pole `a`, normalised to unity at DC or Nyquist: exactly a bilinear one-pole with
/// `g = (1-a)/(1+a)`. That is returned as hertz, so the shared one-pole runs it without a
/// kernel of its own. The `44011.976 / rate` factor of the native law cancels in hertz, so
/// the result does not depend on the sample rate (checked at 44.1, 48 and 96 kHz).
pub fn legacy_warp_hertz(x: f32, warp: Warp) -> f64 {
    let high = warp != Warp::LowPass;
    const RATE: f64 = 48_000.0;
    let a1 = f32::from_bits(0x3e96_3993);
    let clamp = |v: f32| if v < 0.0 { 0.0 } else if v > 1.0 { 1.0 } else { v };
    let x = (f64::from(x.clamp(0., 1.)) * 6.15) as f32;
    // exp(x) to the eighth order, 1 for tiny x.
    let e = if x >= 1e-4 || x <= -1e-4 {
        let (x3, x2) = (x * x * x, x * x);
        let (x4, x5, x6, x7) = (x3 * x, x3 * x * x, x3 * x * x * x, x3 * x * x * x * x);
        x7 * x * 2.480_158_7e-5 + x7 * 1.984_127e-4 + x6 * 1.388_888_9e-3 + x4 * 4.166_666_8e-2
            + x5 * 8.333_334e-3 + x2 * 0.5 + x + x3 * 0.166_666_67 + 1.0
    } else {
        1.0
    };
    let v = clamp((e - 1.0) * 2.572_831_7e-3);
    let v = clamp((v + 2.000_000_1e-3) * (44_011.976_047_904_19 / RATE) as f32);
    let half_pi = |v: f32| (f64::from(v) * std::f64::consts::FRAC_PI_2) as f32;
    let (pole, sign) = if high {
        let w = (f64::from(half_pi(v)) * if warp == Warp::HighPass2 { 0.92 } else { 0.82 }) as f32;
        let w = (f64::from(w) * 0.997_133_173_841_07) as f32;
        let cos = |y: f32| {
            if y >= 1e-4 || y <= -1e-4 {
                let y4 = y * y * y * y;
                (y4 * y * y * 1.388_888_9e-3 - y4 * 4.166_666_8e-2) + (y * y * 0.5 - 1.0)
            } else {
                -1.0
            }
        };
        let cos_pos = |z: f32| {
            if z >= 1e-4 || z <= -1e-4 {
                let z4 = z * z * z * z;
                (z4 * 4.166_666_8e-2 - z4 * z * z * 1.388_888_9e-3) + (1.0 - z * z * 0.5)
            } else {
                1.0
            }
        };
        (f64::from(cos(w + 0.5) / cos_pos(w - 0.5)), -1.0)
    } else {
        let w = (f64::from(half_pi(v)) * 0.99) as f32;
        let w = (f64::from(w) * 0.998_780_904_966_9) as f32;
        let sin = |y: f32| {
            if y >= 1e-4 || y <= -1e-4 {
                let (y3, y5) = (y * y * y, y * y * y * y * y);
                (y5 * 8.333_334e-3 - y5 * y * y * 1.984_127e-4) + (y - y3 * 0.166_666_67)
            } else {
                0.0
            }
        };
        (f64::from(sin(0.5 - w) / sin(0.5 + w)), 1.0)
    };
    // The unity normalisation fixes the gain, so only the warped pole matters (the zero stays
    // at Nyquist for LP, at DC for HP because the two numerator taps are equal).
    let d = 1.0 / (f64::from(a1) * pole + 1.0);
    let pole = (f64::from((f64::from(a1) + pole) as f32) * d) as f32;
    // LP keeps the pole; HP mirrors it (z -> -z).
    let a = f64::from(pole) * sign;
    let g = (1.0 - a) / (1.0 + a);
    RATE / std::f64::consts::PI * g.atan()
}

/// Pinned v1 legacy resonance law.
pub fn legacy_q(r: f32) -> f64 {
    f64::from(std::f32::consts::FRAC_1_SQRT_2 * 28f32.powf(r.clamp(0., 1.)))
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
    fn legacy_warp_hertz_matches_the_native_coefficients() {
        // -3 dB points of the native recurrence evaluated from the extracted coefficient law
        // (truncated series, prototype 0.26276/0.29341), found by bisection outside this code.
        for (x, high, hz) in [
            (0.0, false, 43.5), (0.5, false, 1194.2), (1.0, false, 21802.9),
            (0.0, true, 36.0), (0.5, true, 987.5), (1.0, true, 18029.1),
        ] {
            let got = legacy_warp_hertz(x, if high { Warp::HighPass } else { Warp::LowPass });
            assert!((got / hz - 1.0).abs() < 2e-3, "{x} {high}: {got} vs {hz}");
        }
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
