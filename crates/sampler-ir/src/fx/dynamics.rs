//! `dynamics` family: the shared names every dynamics effect is configured through.
//!
//! One superset, [`Config`]. A format fills it in; the DSP (sampler-core
//! `dsp/dynamics/`) is written once. `law` picks the gain computer and its
//! ballistics; the remaining fields are the stages around it (stereo link,
//! make-up, wet/dry). Laws split in two kinds:
//!
//! * [`Law::Generic`]: the configurable textbook kernel (detector, knee,
//!   direction, topology, look-ahead, side-chain high-pass). Every format may
//!   use it, but it is not a native law: an importer that reaches for it for a
//!   native effect must report that effect as generic.
//! * `Law::Kontakt*`: Kontakt 8.13.1 cores reproduced from the binary, driven
//!   by the effect's saved knobs ([`Native`]).
//!
//! Attack and release are [`Time`] so a live control binds in seconds whatever
//! the law; native kernels convert to the milliseconds their cores take.

use crate::{Gain, Time};

/// Where the detector listens.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    /// Detector on the input.
    FeedForward,
    /// Detector on the gain-applied output (one sample late).
    Feedback,
}

/// Level detector of the generic law.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Detector {
    /// `|x|`.
    Peak,
    /// Square root of the one-pole mean of `x^2`, over this window.
    Rms { window: Time },
}

/// What the gain computer does around the threshold.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Level above the threshold is reduced by `1 - 1/ratio` (a limiter is a
    /// very large ratio).
    Downward,
    /// Level below the threshold is raised by `1 - 1/ratio`.
    Upward,
    /// Level below the threshold is lowered by `ratio - 1` per dB (a gate is a
    /// large ratio).
    Expand,
}

/// How the channels share a detector.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// Each channel is detected alone and gets its own gain.
    Off,
    /// One detector on the signed arithmetic channel mean (Kontakt Classic and
    /// Enhanced: equal opposite-polarity channels read zero).
    Mean,
    /// One detector on the larger channel magnitude.
    Max,
}

/// The configurable textbook kernel. Units: dB, ratio:1 (at least 1), seconds
/// through [`Time`], Hz.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Generic {
    pub threshold_db: f64,
    pub ratio: f64,
    /// Width of the quadratic knee centred on the threshold; 0 is a hard knee.
    pub knee_db: f64,
    pub direction: Direction,
    pub detector: Detector,
    pub topology: Topology,
    /// Delay of the audio behind the detector, so the gain leads the signal.
    pub lookahead: Time,
    /// High-pass of the detector's side-chain; 0 is off.
    pub sidechain_highpass_hz: f64,
}

/// The saved knobs of a Kontakt compressor core.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Native {
    /// Saved dB; the detector threshold is `2^(dB/6)`.
    pub threshold_db: f64,
    /// Saved normalized ratio, 0..=1.
    pub ratio: f64,
}

/// The gain computer plus its ballistics.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Law {
    Generic(Generic),
    /// Kontakt Compressor mode 0, core `0x140ab2250`: a threshold trigger
    /// whose hold, attack and release shape a fixed gain `thr^(1-ratio)`.
    KontaktClassic(Native),
    /// Kontakt Compressor mode 1, core `0x140ab2c80`.
    KontaktEnhanced(Native),
    /// Kontakt Compressor mode 2, core `0x140ab35d0`: 32-frame look-ahead,
    /// dB-domain soft knee. Always detects on the channel maximum.
    KontaktPro(Native),
}

/// A dynamics processor.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub law: Law,
    pub attack: Time,
    pub release: Time,
    pub link: Link,
    /// Gain after the law. Native cores have no make-up of their own; the
    /// importer states what it applies.
    pub makeup: Gain,
    /// Wet share, 0..=1 (1 is fully processed).
    pub mix: f64,
}

impl Config {
    /// A configuration with the generic law and no extras.
    pub fn generic(threshold_db: f64, ratio: f64, attack: Time, release: Time) -> Self {
        Self {
            law: Law::Generic(Generic {
                threshold_db,
                ratio,
                knee_db: 0.,
                direction: Direction::Downward,
                detector: Detector::Peak,
                topology: Topology::FeedForward,
                lookahead: Time::Seconds(0.),
                sidechain_highpass_hz: 0.,
            }),
            attack,
            release,
            link: Link::Mean,
            makeup: Gain::UNITY,
            mix: 1.,
        }
    }

    /// Threshold in the law's own unit (dB; Kontakt saved dB).
    pub fn threshold(&self) -> f64 {
        match self.law {
            Law::Generic(g) => g.threshold_db,
            Law::KontaktClassic(n) | Law::KontaktEnhanced(n) | Law::KontaktPro(n) => n.threshold_db,
        }
    }

    /// Ratio in the law's own unit (ratio:1; Kontakt saved 0..=1).
    pub fn ratio(&self) -> f64 {
        match self.law {
            Law::Generic(g) => g.ratio,
            Law::KontaktClassic(n) | Law::KontaktEnhanced(n) | Law::KontaktPro(n) => n.ratio,
        }
    }

    /// Whether this configuration reproduces a recovered Kontakt core.
    pub fn is_native(&self) -> bool {
        !matches!(self.law, Law::Generic(_))
    }
}
