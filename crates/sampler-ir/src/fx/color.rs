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
/// 0x1412d43c0): period `1 / f32(h0 / rate + h1 / rate)` frames; `blend` 1 holds, 0 follows
/// the ramp.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decimator {
    pub hertz: [f64; 2],
    pub blend: f32,
}

/// An RBJ biquad in transposed direct form II, f32 (UVI 0x141646fa0; designs 0x14163fd30 LP,
/// 0x14163ff50 BP at 0 dB peak, 0x14163fe30 HP, in f64 at `hertz / rate`). `post` runs it
/// after the drive/quantize stages instead of before them.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    /// 0 low-pass, 1 band-pass, 2 high-pass.
    pub mode: u8,
    pub hertz: f64,
    pub q: f64,
    pub post: bool,
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
    /// Kontakt Dirt (two diode drive stages with routing, effektrig x2).
    Dirt(Dirt),
    /// Kontakt Bite (resampling log quantizer with its own filters, effektrig x2).
    Bite(Bite),
    /// Kontakt Tape Saturator (oversampled table shaper with its own filters).
    Tape(Tape),
    /// UVI Exciter (Tape sine / Tube envelope cubic, HIIR oversampled).
    Exciter(Exciter),
    /// UVI ToneStack (Yeh/Smith passive stack, 26 models).
    ToneStack(ToneStack),
    /// UVI DiodeClipper (Newton-solved diode pair, DC blocker, output gain).
    Diode(Diode),
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

/// UVI DiodeClipper controls as stored: Drive dB (0..30), Tone and HighPass Hz, Asymmetry
/// 0..1, OutputGain dB.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Diode {
    pub drive: f32,
    pub tone: f32,
    pub high_pass: f32,
    pub asymmetry: f32,
    pub output_gain: f32,
}

/// UVI Exciter controls as stored: Mode 0 Tape / 1 Tube, gains in dB, Amount 0..15, Mix %,
/// 2^k oversampling.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Exciter {
    pub mode: u8,
    pub input_gain: f32,
    pub amount: f32,
    pub mix: f32,
    pub output_gain: f32,
    pub oversampling: u8,
}

/// Kontakt Bite core values (0..1, enums as i/2) by core index: saturate, prefilter,
/// frequency, jitter, crunch, bits, dither, dcquant, expand, postfilter, hpf, mix.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bite {
    pub knobs: [f32; 12],
}

/// Kontakt Dirt core values (0..1, enums as i/2) by core index: drive A/B, blend A/B, tone
/// A/B, bias A/B, diode type A/B, comp A/B, routing, amount lo/hi, level.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dirt {
    pub knobs: [f32; 16],
}

/// Kontakt Tape Saturator controls (normalized 0..1): gain, warmth, HF roll-off, Quality.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tape {
    pub gain: f32,
    pub warmth: f32,
    pub rolloff: f32,
    pub quality: bool,
}

/// UVI Widener controls as stored: Delay ms (-15..15, negative delays the left), Width
/// -1..1, the side shelf's HighShelfFreqSide Hz and HighSelfGainSide dB.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widener {
    pub delay: f32,
    pub width: f32,
    pub shelf_hertz: f32,
    pub shelf_db: f32,
}

/// UVI Redux2's rate and bit reduction: Frequency Hz, Bits 1..32, Jitter 0..1, Dither 0..2,
/// QuantizeLaw 1 = mu-law, Type 0 = impulses (else held), Antialiasing, BandLimitFreq Hz.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Redux {
    pub frequency: f32,
    pub bits: f32,
    pub jitter: f32,
    pub dither: f32,
    pub mu_law: bool,
    pub impulses: bool,
    pub antialias: bool,
    pub band_limit: f32,
}

/// UVI ToneStack controls: Bass, Mid, Treble, Gain 0..1, Model 0..=25.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToneStack {
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub gain: f32,
    pub model: u8,
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
    /// The tone/filter section (before the hold unless `post`).
    pub filter: Option<Biquad>,
    pub hold: Option<Hold>,
    pub decimator: Option<Decimator>,
    /// UVI Redux2's clocked reduction (after the decimator).
    pub redux: Option<Redux>,
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
    /// UVI Widener (after the mix).
    pub widener: Option<Widener>,
}

impl Config {
    /// No stage: the signal passes unchanged. Struct-update from this.
    pub const IDENTITY: Self = Self {
        address: None,
        gainer: None,
        filter: None,
        hold: None,
        decimator: None,
        redux: None,
        quantizer: None,
        curve: Curve::Identity,
        damping: None,
        noise: None,
        tone: None,
        mix: 1.0,
        stereo: None,
        widener: None,
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

    /// UVI FormantCrusher (prepare 0x1412d31a0, update 0x1412d33c0, kernel 0x1412d4000): an
    /// RBJ low-pass at F1 (Q), then the decimator at the period `1 / (F1 + F2)`, wet `mix`.
    /// F1, F2 morph geometrically between vowels `a` and `b` (table 0x141e7a420), scaled by
    /// `2^formant`.
    pub fn formant_crusher(morph: f32, q: f32, formant: f32, mix: f32, bite: f32, a: u8, b: u8) -> Self {
        const VOWELS: [[f64; 2]; 11] = [
            [700.0, 1100.0], [500.0, 1350.0], [250.0, 1700.0], [570.0, 840.0], [300.0, 870.0], [440.0, 1020.0],
            [520.0, 1190.0], [660.0, 1720.0], [530.0, 1840.0], [390.0, 1990.0], [270.0, 2290.0],
        ];
        let fm = 2f64.powf(f64::from(formant));
        let (a, b) = (VOWELS[usize::from(a.min(10))], VOWELS[usize::from(b.min(10))]);
        let hz: [f64; 2] = std::array::from_fn(|i| (b[i] / a[i]).powf(f64::from(morph)) * a[i] * fm);
        Self {
            filter: Some(Biquad { mode: 0, hertz: hz[0], q: f64::from(q), post: false }),
            decimator: Some(Decimator { hertz: hz, blend: bite }),
            mix: f64::from(mix),
            ..Self::IDENTITY
        }
    }

    /// UVI Redux2 (FilterPosition 1 filters before, 2 after; FilterMode 0 LP, 1 BP, 2 HP;
    /// resonance `Q = 9.5 res + 0.5`), wet `mix`.
    pub fn redux(redux: Redux, filter: Option<(u8, f32, f32, bool)>, mix: f32) -> Self {
        Self {
            redux: Some(redux),
            filter: filter.map(|(mode, hertz, res, post)| Biquad {
                mode,
                hertz: f64::from(hertz),
                q: f64::from((res * 9.5) + 0.5),
                post,
            }),
            mix: f64::from(mix),
            ..Self::IDENTITY
        }
    }

    /// Kontakt Dirt.
    pub fn dirt(dirt: Dirt) -> Self {
        Self { curve: Curve::Dirt(dirt), ..Self::IDENTITY }
    }

    /// Kontakt Bite.
    pub fn bite(bite: Bite) -> Self {
        Self { curve: Curve::Bite(bite), ..Self::IDENTITY }
    }

    /// Kontakt Tape Saturator.
    pub fn tape(tape: Tape) -> Self {
        Self { curve: Curve::Tape(tape), ..Self::IDENTITY }
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
            Self { curve: Curve::Dirt(_), .. } => "Dirt",
            Self { curve: Curve::Bite(_), .. } => "Bite",
            Self { curve: Curve::Tape(_), .. } => "Tape",
            Self { curve: Curve::Exciter(_), .. } => "Exciter",
            Self { curve: Curve::ToneStack(_), .. } => "Tone stack",
            Self { curve: Curve::Diode(_), .. } => "Diode clipper",
            Self { decimator: Some(_), .. } => "Formant crusher",
            Self { redux: Some(_), .. } => "Redux",
            Self { gainer: Some(_), .. } => "Gainer",
            Self { stereo: Some(_), .. } => "Stereo Modeller",
            Self { widener: Some(_), .. } => "Widener",
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
