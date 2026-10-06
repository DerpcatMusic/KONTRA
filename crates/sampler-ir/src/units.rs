//! Quantities keep the unit their source authored. Lowering converts once, so a
//! translator never guesses a native unit and a report can show the original.

/// Amplitude.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gain {
    Decibels(f64),
    /// Linear amplitude factor; 1.0 is unity.
    Linear(f64),
}

impl Gain {
    pub const UNITY: Self = Self::Linear(1.0);

    pub fn linear(self) -> f64 {
        match self {
            Self::Decibels(db) => 10f64.powf(db / 20.0),
            Self::Linear(factor) => factor,
        }
    }
}

impl Default for Gain {
    fn default() -> Self {
        Self::UNITY
    }
}

/// Pitch offset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pitch {
    Cents(f64),
    Semitones(f64),
    /// Frequency ratio; 1.0 is unchanged.
    Ratio(f64),
}

impl Pitch {
    pub const NONE: Self = Self::Cents(0.0);

    pub fn semitones(self) -> f64 {
        match self {
            Self::Cents(cents) => cents / 100.0,
            Self::Semitones(semitones) => semitones,
            Self::Ratio(ratio) => 12.0 * ratio.log2(),
        }
    }
}

impl Default for Pitch {
    fn default() -> Self {
        Self::NONE
    }
}

/// Duration independent of any sample rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Time {
    Seconds(f64),
    Milliseconds(f64),
}

impl Time {
    pub const ZERO: Self = Self::Seconds(0.0);

    pub fn seconds(self) -> f64 {
        match self {
            Self::Seconds(seconds) => seconds,
            Self::Milliseconds(ms) => ms / 1000.0,
        }
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::ZERO
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Frequency {
    Hertz(f64),
    /// Musical rate; one beat is a quarter note at the host tempo.
    Beats(f64),
}

/// Filter emphasis at the cutoff.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resonance {
    /// Peak gain above the flat response, as SFZ `resonance` authors it.
    Decibels(f64),
    Q(f64),
    /// Vendor 0..1 knob position whose law the source profile owns.
    Normalized(f64),
}

/// Stereo placement, -1.0 is left and 1.0 is right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pan {
    pub position: f64,
    pub law: PanLaw,
}

impl Pan {
    pub const CENTER: Self = Self {
        position: 0.0,
        law: PanLaw::Balance,
    };
}

impl Default for Pan {
    fn default() -> Self {
        Self::CENTER
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanLaw {
    /// Attenuates the far channel only; center is unity on both.
    #[default]
    Balance,
    /// Sine/cosine law, -3 dB per channel at center.
    EqualPower,
}

/// Sample frames of the referenced asset at its own rate.
pub type SourceFrames = u64;
