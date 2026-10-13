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
        }
    }
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
}

impl Config {
    /// `None` when `bands` is empty or longer than [`MAX_BANDS`].
    pub fn new(bands: &[Band]) -> Option<Self> {
        if bands.is_empty() || bands.len() > MAX_BANDS {
            return None;
        }
        let mut all = [Band::OFF; MAX_BANDS];
        all[..bands.len()].copy_from_slice(bands);
        Some(Self { bands: all, count: bands.len() as u8, output_gain_db: 0. })
    }
    pub fn band(band: Band) -> Self {
        Self::new(&[band]).expect("one band fits")
    }
    pub fn bands(&self) -> &[Band] {
        &self.bands[..usize::from(self.count).min(MAX_BANDS)]
    }
}
