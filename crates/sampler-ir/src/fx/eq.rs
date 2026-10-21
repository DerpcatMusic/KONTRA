//! `eq` family: the shared configuration names (owned by the eq agent).
//!
//! One superset covers every equaliser a Kontakt or UVI library can use: Kontakt legacy EQ kinds
//! 22..=24 and the Solid G-EQ, UVI Parametric/2-/3-/8-Band/Digital EQ, scripted band inserts.
//! A [`Config`] is a serial cascade of up to [`MAX_BANDS`] [`Band`]s plus an output gain. Formats
//! only fill it in; the coefficient laws live in `sampler-core` (`dsp/eq_family`).

/// Bands one [`Config`] carries. A wider EQ (UVI DigitalEq has 16) is several configs in series.
pub const MAX_BANDS: usize = 8;

/// The response a band draws.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A flat gain, `gain_db` minus the reference, no filtering (scripted gain stages).
    Level,
    Peak,
    LowShelf,
    HighShelf,
    /// 6 dB/octave shelves; `q` has no effect.
    FirstOrderLowShelf,
    FirstOrderHighShelf,
    /// Low and high pass of `poles` poles (1..=4): `poles == 2` honours `q`, other orders are
    /// Butterworth sections.
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

/// The Solid G-EQ section a band belongs to; selects its parameter spline tables.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolidSection {
    LowFrequency,
    LowMid,
    HighMid,
    HighFrequency,
    HighPass,
    LowPass,
}

/// How a band's `frequency`, `q` and `gain_db` are read and turned into coefficients.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Law {
    /// Generic RBJ biquad: Hertz, Q, decibels. Not any native Kontakt law.
    Rbj,
    /// Kontakt legacy EQ (Filter 0x18 kinds 22..=24, coefficient generator 0x1409591c0). Peak
    /// only. `frequency` and `q` are the native normalized frequency and width knobs (0..=1);
    /// `gain_db` is the displayed gain (-18..=18). Laws, 32-frame control cadence and the
    /// five-value recurrence are the native ones.
    KontaktLegacy,
    /// Solid G-EQ: `frequency`, `q` and `gain_db` are the saved normalized 0..=1 knobs of the
    /// section; the native cubic-spline tables turn them into Hz, bandwidth and dB (proved). The
    /// biquad that follows is generic RBJ (topology unproved). `q` is ignored for HP/LP sections.
    Solid(SolidSection),
    /// RBJ cookbook biquads by the bilinear transform: Hertz, Q (`alpha = sin(w0) / 2q`), decibels
    /// (`A = 10^(dB/40)`). UVI TwoBandEq/ThreeBandEq/ParametricEQ coefficient routines (proved).
    Cookbook,
    /// UVI DigitalEq band design (0x1412840f0): `poles` is the order (1, 2, 3, 4, 6, 8, 12, 16 for
    /// slopes 6..96 dB/oct) of a prototype whose poles `q` spreads (0.707 is Butterworth); band
    /// pass, notch and peak take `bandwidth` octaves; `gain_db` is the band gain. Proved under
    /// emulation.
    UviDigital,
}

#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub kind: Kind,
    pub law: Law,
    /// A disabled band passes the signal untouched.
    pub enabled: bool,
    pub frequency: f32,
    pub q: f32,
    pub gain_db: f32,
    /// Filter order for LowPass/HighPass: 1..=4 poles (12 dB/oct = 2). Other kinds ignore it.
    pub poles: u8,
    /// SSL-style proportional Q: the effective Q is `q * (1 + proportional_q * |gain_db| / 12)`.
    /// Zero keeps Q constant. Generic option, not a proved native law.
    pub proportional_q: f32,
    /// Octaves: the band-pass/notch/peak width under [`Law::UviDigital`]; other laws ignore it.
    pub bandwidth: f32,
}

impl Band {
    /// A 12 dB/octave, constant-Q band under the generic RBJ law.
    pub const fn new(kind: Kind, frequency_hz: f32, q: f32, gain_db: f32) -> Self {
        Self {
            kind,
            law: Law::Rbj,
            enabled: true,
            frequency: frequency_hz,
            q,
            gain_db,
            poles: 2,
            proportional_q: 0.,
            bandwidth: 1.,
        }
    }
    // Preserve the established parameter bits; replacing Q with a mathematical
    // constant changes the default when a caller later enables this band.
    #[allow(clippy::approx_constant)]
    pub const OFF: Self = Self::new(Kind::Peak, 1000., 0.7071, 0.);
}

/// A serial cascade of up to [`MAX_BANDS`] bands and an output gain. Plain data, `Copy`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub bands: [Band; MAX_BANDS],
    pub count: u8,
    /// Applied after the last band; 0 adds no stage.
    pub output_gain_db: f32,
    /// The script slot whose `set_engine_par` writes drive [`Config::lanes`].
    #[cfg_attr(feature = "cache", serde(default))]
    pub address: Option<crate::SlotAddress>,
    /// Which band of the slot's module this one-band config is: FREQ1 is 0 for the
    /// legacy law, the saved band index (LF 0, LMF 1, HMF 2, HF 3, HP 4, LP 5) for
    /// Solid G-EQ.
    #[cfg_attr(feature = "cache", serde(default))]
    pub band: u8,
}

impl Config {
    /// `None` when `bands` is empty or longer than [`MAX_BANDS`].
    pub fn new(bands: &[Band]) -> Option<Self> {
        if bands.is_empty() || bands.len() > MAX_BANDS {
            return None;
        }
        let mut all = [Band::OFF; MAX_BANDS];
        all[..bands.len()].copy_from_slice(bands);
        Some(Self { bands: all, count: bands.len() as u8, output_gain_db: 0., address: None, band: 0 })
    }
    pub fn band(band: Band) -> Self {
        Self::new(&[band]).expect("one band fits")
    }
    /// The live knobs of a script-addressed one-band EQ: FREQn, BWn, GAINn for
    /// the legacy law (kontakt-bindings filters_eq.tsv; the band holds the native
    /// frequency and bandwidth knobs, `(octaves - 1/3) 0.375`), the `SEQ_*`
    /// identity knobs for a Solid G-EQ band.
    pub fn lanes(&self) -> Vec<super::Lane> {
        use super::{Lane, Law as L};
        use crate::ProcessorParameter as P;
        let [b] = self.bands() else { return Vec::new() };
        if self.address.is_none() {
            return Vec::new();
        }
        if let Law::Solid(section) = b.law {
            return solid_lanes(b, section);
        }
        if b.law != Law::KontaktLegacy {
            return Vec::new();
        }
        const FREQ: [&str; 3] = ["ENGINE_PAR_FREQ1", "ENGINE_PAR_FREQ2", "ENGINE_PAR_FREQ3"];
        const BW: [&str; 3] = ["ENGINE_PAR_BW1", "ENGINE_PAR_BW2", "ENGINE_PAR_BW3"];
        const GAIN: [&str; 3] = ["ENGINE_PAR_GAIN1", "ENGINE_PAR_GAIN2", "ENGINE_PAR_GAIN3"];
        let Some(n) = FREQ.get(usize::from(self.band)).map(|_| usize::from(self.band)) else { return Vec::new() };
        let octaves = f64::from(b.q) / 0.375 + 1. / 3.;
        vec![
            Lane::new(FREQ[n], P::Frequency, f64::from(b.frequency), L::LegacyEqFrequency),
            Lane::new(BW[n], P::Quality, octaves, L::Linear { a: 1. / 3., b: 8. / 3. }).scaled([0., 1.]),
            Lane::new(GAIN[n], P::GainDecibels, f64::from(b.gain_db), L::Linear { a: -18., b: 36. }),
        ]
    }
    pub fn bands(&self) -> &[Band] {
        &self.bands[..usize::from(self.count).min(MAX_BANDS)]
    }
}

/// The live knobs of one script-addressed Solid G-EQ band. Every continuous knob
/// is the identity law over 0..=1 (`filters_eq.tsv`), so the lane value is the
/// stored knob itself; the bell and pass enables are switches and have no lane.
fn solid_lanes(b: &Band, section: SolidSection) -> Vec<super::Lane> {
    use super::{Lane, Law as L};
    use crate::ProcessorParameter as P;
    let (gain, frequency, q): (Option<&str>, &str, Option<&str>) = match section {
        SolidSection::LowFrequency => {
            (Some("ENGINE_PAR_SEQ_LF_GAIN"), "ENGINE_PAR_SEQ_LF_FREQ", None)
        }
        SolidSection::LowMid => (
            Some("ENGINE_PAR_SEQ_LMF_GAIN"),
            "ENGINE_PAR_SEQ_LMF_FREQ",
            Some("ENGINE_PAR_SEQ_LMF_Q"),
        ),
        SolidSection::HighMid => (
            Some("ENGINE_PAR_SEQ_HMF_GAIN"),
            "ENGINE_PAR_SEQ_HMF_FREQ",
            Some("ENGINE_PAR_SEQ_HMF_Q"),
        ),
        SolidSection::HighFrequency => {
            (Some("ENGINE_PAR_SEQ_HF_GAIN"), "ENGINE_PAR_SEQ_HF_FREQ", None)
        }
        SolidSection::HighPass => (None, "ENGINE_PAR_SEQ_HP_FREQ", None),
        SolidSection::LowPass => (None, "ENGINE_PAR_SEQ_LP_FREQ", None),
    };
    let mut lanes = Vec::with_capacity(3);
    if let Some(name) = gain {
        lanes.push(Lane::new(name, P::GainDecibels, f64::from(b.gain_db), L::Identity));
    }
    lanes.push(Lane::new(frequency, P::Frequency, f64::from(b.frequency), L::Identity));
    if let Some(name) = q {
        lanes.push(Lane::new(name, P::Quality, f64::from(b.q), L::Identity));
    }
    lanes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fx::Law as Lane;
    use crate::ProcessorParameter as P;

    fn solid(section: SolidSection, kind: Kind, gain: f32, q: f32) -> Config {
        let band = Band {
            law: Law::Solid(section),
            frequency: 0.25,
            q,
            gain_db: gain,
            ..Band::new(kind, 0., 0., 0.)
        };
        let mut config = Config::new(&[band]).expect("one band");
        config.address = Some(crate::SlotAddress { group: 3, slot: 2, generic: -1 });
        config
    }

    fn lanes(config: Config) -> Vec<(&'static str, P, f64, Lane)> {
        config.lanes().into_iter().map(|l| (l.name, l.role, l.value, l.law)).collect()
    }

    /// `filters_eq.tsv`: every continuous Solid knob is the identity law over
    /// 0..=1 and the lane value is the saved knob itself.
    #[test]
    fn solid_bands_have_the_identity_script_lanes_of_their_section() {
        assert_eq!(
            lanes(solid(SolidSection::LowFrequency, Kind::LowShelf, 0.75, 0.)),
            [
                ("ENGINE_PAR_SEQ_LF_GAIN", P::GainDecibels, 0.75, Lane::Identity),
                ("ENGINE_PAR_SEQ_LF_FREQ", P::Frequency, 0.25, Lane::Identity),
            ]
        );
        assert_eq!(
            lanes(solid(SolidSection::LowMid, Kind::Peak, 0.5, 0.625)),
            [
                ("ENGINE_PAR_SEQ_LMF_GAIN", P::GainDecibels, 0.5, Lane::Identity),
                ("ENGINE_PAR_SEQ_LMF_FREQ", P::Frequency, 0.25, Lane::Identity),
                ("ENGINE_PAR_SEQ_LMF_Q", P::Quality, 0.625, Lane::Identity),
            ]
        );
        assert_eq!(
            lanes(solid(SolidSection::HighMid, Kind::Peak, 0.5, 0.5)),
            [
                ("ENGINE_PAR_SEQ_HMF_GAIN", P::GainDecibels, 0.5, Lane::Identity),
                ("ENGINE_PAR_SEQ_HMF_FREQ", P::Frequency, 0.25, Lane::Identity),
                ("ENGINE_PAR_SEQ_HMF_Q", P::Quality, 0.5, Lane::Identity),
            ]
        );
        // The pass filters have one knob each.
        assert_eq!(
            lanes(solid(SolidSection::HighPass, Kind::HighPass, 0., 0.)),
            [("ENGINE_PAR_SEQ_HP_FREQ", P::Frequency, 0.25, Lane::Identity)]
        );
        assert_eq!(
            lanes(solid(SolidSection::LowPass, Kind::LowPass, 0., 0.)),
            [("ENGINE_PAR_SEQ_LP_FREQ", P::Frequency, 0.25, Lane::Identity)]
        );
        // A module no script addresses keeps its knobs constant.
        let mut unaddressed = solid(SolidSection::HighFrequency, Kind::HighShelf, 0.5, 0.);
        unaddressed.address = None;
        assert!(unaddressed.lanes().is_empty());
    }
}
