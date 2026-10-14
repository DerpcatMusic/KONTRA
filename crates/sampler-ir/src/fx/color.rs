//! `color` family: the shared names of the drive, character and utility DSP.
//!
//! One superset, [`Config`], configures every native unit of the family: a
//! gainer, sample hold and decimator, bit quantizer, waveshaper curve, damping
//! filter, noise and stereo modeller. Stages run in the field order below and
//! an absent stage costs nothing, so each native effect is a preset of this
//! struct (Kontakt Distortion = a curve plus damping; Lo-Fi = hold, quantizer
//! and noise; Gainer = the gainer alone; ...). The kernels live in
//! `sampler-core::dsp::color`; formats only fill the fields in.
//!
//! Nothing here approximates: each curve, hold and quantizer variant names the
//! one native law it reproduces. A native effect whose law is not proved is
//! reported unsupported by its translator, never mapped onto the nearest curve.
use crate::{Gain, Rectifier, SlotAddress};

/// Smoothed gain with a dry contribution: `x * (dry + g)`, `g` slewing to `gain`
/// (Kontakt Gainer, DSP_SYSTEM_INVENTORY "Gainer").
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gainer {
    pub gain: Gain,
    pub dry: f64,
}

/// Sample-and-hold.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hold {
    /// Kontakt Lo-Fi (0x140aad4e4): normalized Frequency `r`; active below
    /// 0.9999899864, period `max(1, Fs / max(50, Fs*r))`, capture when
    /// `phase >= period` (cadence `period + 1`).
    Native { frequency: f32 },
    /// UVI BitCrusher kernel 0x1412d1770: capture when the frame index of the
    /// call reaches the phase, which then advances by `Fs / hertz`. The phase
    /// restarts every processing call, as in the native kernel.
    Rate { hertz: f64 },
}

/// Fractional decimator with a linear ramp (UVI Formant Crusher sub-kernel
/// 0x1412d43c0): `blend` 1 holds, 0 follows the ramp.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decimator {
    pub period: f64,
    pub blend: f64,
}

/// Bit-depth reduction.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quantizer {
    /// Kontakt Lo-Fi (0x140aad679): normalized Bits `b`, `Q = 2^max(1, 16b)`,
    /// truncation toward zero; inactive at `b >= 0.9999899864`.
    Native { bits: f32 },
    /// UVI BitCrusher: scale `2^bits`, `floor` (and `+1` for negative values),
    /// rescale.
    Floor { bits: f64 },
}

/// The memoryless waveshaper.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Identity,
    /// Kontakt Classic Saturator / Shaper algorithm 0, signed Shape -1..1.
    Classic { shape: f32 },
    /// Kontakt Saturator Enhanced (algorithm 1): peak-linked table shaper, signed Shape.
    Enhanced { shape: f32 },
    /// Kontakt Saturator Drums (algorithm 2): table-driven clipper, signed Shape.
    Drums { shape: f32 },
    /// Kontakt Tube Distortion, normalized Drive 0..1.
    Tube { drive: f32 },
    /// Kontakt Transistor Distortion, normalized Drive 0..1.
    Transistor { drive: f32 },
    /// UVI WaveShaper internal rectifier modes 6 (full) and 7 (half).
    Rectifier(Rectifier),
    /// UVI BitCrusher drive nonlinearity, normalized Drive 0..1.
    Destructor { drive: f64 },
    /// Kontakt Skreamer's oversampled table shaper with its own filters:
    /// `[tone, drive, bass, bright, mix]`, normalized.
    Skreamer([f32; 5]),
    /// A UVI drive engine (gain, curve, wet/dry, output gain).
    Drive(Drive),
}

/// The UVI drive engines' curves.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    /// GarageFuzz: `tanh`.
    Garage,
    /// SmoothOverdrive: `s / (1 + |s|)`.
    Smooth,
    /// StraightFuzz: a stereo-linked square at `10^(-2.4 Trash)`.
    Straight,
}

/// UVI GarageFuzz / SmoothOverdrive / StraightFuzz controls as stored (dB, 0..10, 0..1, %).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    pub kind: DriveKind,
    pub input_gain: f32,
    pub overdrive: f32,
    pub trash: f32,
    pub mix: f32,
    pub output_gain: f32,
    /// 2^k oversampling (0..=4).
    pub oversampling: u8,
}

/// Kontakt Lo-Fi noise (0x140aad8a2): shared LCG, one-pole colored.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noise {
    /// Stored noise amount; the gain is `0.075 * level`.
    pub level: f32,
    /// Normalized tone: `alpha = 1 - 0.9 * color`.
    pub color: f32,
}

/// Kontakt Stereo Modeller: width 0..1 (.5 identity), balance -1..1, optional
/// pseudo stereo (right channel delayed by up to 10 ms).
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stereo {
    pub width: f64,
    pub pan: f64,
    pub pseudo: bool,
}

/// One color unit. See the module docs for the stage order.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    /// The script-addressable slot (`set_engine_par`) whose Shape, Drive,
    /// Damping, Bits, Frequency, Noise level and Noise color writes drive this
    /// unit's knobs. Absent: the knobs are constants.
    pub address: Option<SlotAddress>,
    pub gainer: Option<Gainer>,
    pub hold: Option<Hold>,
    pub decimator: Option<Decimator>,
    pub quantizer: Option<Quantizer>,
    pub curve: Curve,
    /// Kontakt Distortion damping: normalized 0..1 knob of the embedded
    /// low-pass, followed by the fixed DC blocker.
    pub damping: Option<f32>,
    pub noise: Option<Noise>,
    /// UVI BitCrusher Cutoff, -1..1: negative low-pass, positive high-pass,
    /// zero off (see `sampler-core::dsp::color::tone`).
    pub tone: Option<f32>,
    /// Wet share of the stages above (UVI Mix); 1 is fully wet.
    pub mix: f64,
    pub stereo: Option<Stereo>,
}

impl Config {
    /// No stage: the signal passes unchanged. Struct-update from this.
    pub const IDENTITY: Self = Self {
        address: None,
        gainer: None,
        hold: None,
        decimator: None,
        quantizer: None,
        curve: Curve::Identity,
        damping: None,
        noise: None,
        tone: None,
        mix: 1.0,
        stereo: None,
    };

    /// Kontakt Gainer.
    pub fn gainer(gain: Gain, dry: f64) -> Self {
        Self { gainer: Some(Gainer { gain, dry }), ..Self::IDENTITY }
    }

    /// Kontakt Stereo Modeller.
    pub fn stereo(width: f64, pan: f64, pseudo: bool) -> Self {
        Self { stereo: Some(Stereo { width, pan, pseudo }), ..Self::IDENTITY }
    }

    /// Kontakt Lo-Fi from its four saved controls.
    pub fn lofi(bits: f32, frequency: f32, noise: f32, color: f32) -> Self {
        Self {
            hold: Some(Hold::Native { frequency }),
            quantizer: Some(Quantizer::Native { bits }),
            noise: Some(Noise { level: noise, color }),
            ..Self::IDENTITY
        }
    }

    /// Kontakt Tube or Transistor Distortion.
    pub fn distortion(transistor: bool, drive: f32, damping: f32) -> Self {
        Self {
            curve: if transistor { Curve::Transistor { drive } } else { Curve::Tube { drive } },
            damping: Some(damping),
            ..Self::IDENTITY
        }
    }

    /// Kontakt Classic Saturator.
    pub fn classic(shape: f32) -> Self {
        Self { curve: Curve::Classic { shape }, ..Self::IDENTITY }
    }

    /// Kontakt Saturator Enhanced.
    pub fn enhanced(shape: f32) -> Self {
        Self { curve: Curve::Enhanced { shape }, ..Self::IDENTITY }
    }

    /// Kontakt Saturator Drums.
    pub fn drums(shape: f32) -> Self {
        Self { curve: Curve::Drums { shape }, ..Self::IDENTITY }
    }

    /// UVI BitCrusher (kernel 0x1412d1770): hold at `hertz`, `2^bits` floor
    /// quantizer, rational drive, tilt filter, wet `mix`.
    pub fn bit_crusher(hertz: f64, bits: f64, drive: f64, cutoff: f32, mix: f64) -> Self {
        Self {
            hold: Some(Hold::Rate { hertz }),
            quantizer: Some(Quantizer::Floor { bits }),
            curve: Curve::Destructor { drive },
            tone: (cutoff != 0.0).then_some(cutoff),
            mix,
            ..Self::IDENTITY
        }
    }

    /// Kontakt Skreamer.
    pub fn skreamer(knobs: [f32; 5]) -> Self {
        Self { curve: Curve::Skreamer(knobs), ..Self::IDENTITY }
    }

    /// A memoryless rectifier.
    pub fn rectifier(mode: Rectifier) -> Self {
        Self { curve: Curve::Rectifier(mode), ..Self::IDENTITY }
    }

    /// The same unit driven by a script slot.
    pub fn at(self, address: SlotAddress) -> Self {
        Self { address: Some(address), ..self }
    }

    /// The script-addressable knobs, in the order the lowering gives them
    /// engine-parameter lanes: (`ENGINE_PAR_*` name, role, saved value, law).
    /// Empty without an `address`.
    pub fn lanes(&self) -> Vec<Lane> {
        use crate::ProcessorParameter as P;
        let mut lanes = Vec::new();
        if self.address.is_none() {
            return lanes;
        }
        let mut lane = |name, role, value: f64, law| lanes.push(Lane { name, role, value, law });
        if let Curve::Classic { shape } | Curve::Enhanced { shape } | Curve::Drums { shape } = self.curve {
            lane("ENGINE_PAR_SHAPE", P::Shape, f64::from(shape), LaneLaw::Signed);
        }
        if let Curve::Tube { drive } | Curve::Transistor { drive } = self.curve {
            lane("ENGINE_PAR_DRIVE", P::Drive, f64::from(drive), LaneLaw::Unit);
        }
        if let Some(damping) = self.damping {
            lane("ENGINE_PAR_DAMPING", P::Damping, f64::from(damping), LaneLaw::Unit);
        }
        if let Some(Quantizer::Native { bits }) = self.quantizer {
            lane("ENGINE_PAR_BITS", P::LoFiBits, f64::from(bits), LaneLaw::Unit);
        }
        if let Some(Hold::Native { frequency }) = self.hold {
            lane("ENGINE_PAR_FREQUENCY", P::LoFiFrequency, f64::from(frequency), LaneLaw::Unit);
        }
        if let Some(Noise { level, color }) = self.noise {
            // Measured (Kontakt 8.13.1 probe): the stored noise amount is the
            // cube of the written value.
            lane("ENGINE_PAR_NOISELEVEL", P::LoFiNoise, f64::from(level), LaneLaw::Cubic);
            lane("ENGINE_PAR_NOISECOLOR", P::LoFiColor, f64::from(color), LaneLaw::Unit);
        }
        lanes
    }

    /// Short name for insert lists.
    pub fn label(&self) -> &'static str {
        match self {
            Self { quantizer: Some(_), .. } | Self { hold: Some(_), .. } | Self { noise: Some(_), .. } => "LoFi",
            Self { curve: Curve::Classic { .. } | Curve::Enhanced { .. } | Curve::Drums { .. }, .. } => "Saturation",
            Self { curve: Curve::Tube { .. } | Curve::Transistor { .. }, .. } => "Distortion",
            Self { curve: Curve::Rectifier(_), .. } => "Rectify",
            Self { curve: Curve::Destructor { .. }, .. } => "Bit crusher",
            Self { curve: Curve::Skreamer(_), .. } => "Skreamer",
            Self { curve: Curve::Drive(_), .. } => "Drive",
            Self { decimator: Some(_), .. } => "Decimator",
            Self { gainer: Some(_), .. } => "Gainer",
            Self { stereo: Some(_), .. } => "Stereo Modeller",
            _ => "Color",
        }
    }
}

/// One script-addressable knob of a [`Config`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lane {
    pub name: &'static str,
    pub role: crate::ProcessorParameter,
    /// The saved (DSP-domain) value, which is also the lane's default.
    pub value: f64,
    pub law: LaneLaw,
}

/// How the engine-parameter integer maps to the knob's DSP value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneLaw {
    /// 0..1 linear.
    Unit,
    /// -1..1 linear.
    Signed,
    /// `(v / 1e6)^3`.
    Cubic,
}
