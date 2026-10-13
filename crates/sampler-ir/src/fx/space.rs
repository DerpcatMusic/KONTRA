//! `space` family: the shared names for every reverb and convolution effect.
//!
//! One [`Config`] describes an algorithmic network, a partitioned convolution,
//! or both summed, with the dry/wet mix. Kontakt's legacy Reverb, Galois,
//! Plate, Raum and Convolution and UVI's reverb and IR effects all fill this
//! in; the DSP lives in `sampler_core::dsp::space`.
//!
//! Fields are physical values in seconds, hertz, dB and linear gain. A format
//! converts its own controls to these and reports the conversion as generic
//! when the native law is not proved.

use crate::ImpulseRef;

/// Which delay-network layout the algorithmic part runs.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Topology {
    /// Eight modulated delay lines coupled by a Hadamard matrix, with
    /// per-line damping, input allpass diffusion and an optional three-band
    /// decay. The general-purpose hall/room.
    #[default]
    Fdn,
    /// Eight parallel damped feedback combs into four serial allpasses: the
    /// structure the legacy Kontakt Reverb (0x15) proves and UVI's older
    /// reverbs share.
    CombAllpass,
    /// A figure-eight tank of two modulated allpass / delay / damping loops
    /// fed by an input diffuser, crossed over to two output taps.
    Plate,
    /// Only the input allpass diffuser (no feedback tank): a smearing effect.
    Diffuser,
    /// The exact Kontakt legacy Reverb (0x15) network, driven by
    /// [`Network::freeverb`] and `predelay_seconds`/`input_cutoff_hz`.
    Freeverb,
    /// The exact Kontakt Reverb (Galois, 0x59) core, driven by
    /// [`Network::galois`]: 31 modulated delay lines coupled by an orthogonal
    /// m-sequence matrix, Room or Hall tables, allpass diffusion, a 200 Hz
    /// low shelf and a high cut. It does its own predelay, width and mix;
    /// the other `Network` fields only describe it (UI, tail length).
    Galois,
}

/// The ten normalized controls (`0..=1`) of the Kontakt Reverb core setter
/// `FUN_141aea370`, in its index order: time, size, damping, modulation,
/// diffusion, predelay, high cut, low shelf, internal dry/wet, stereo.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Galois {
    /// Hall (`true`, saved type 1) or Room core: the dispatcher's control 10
    /// (`FUN_1409ab2c0`, `>= 0.5` runs the core built with flag 1, whose
    /// lines are the longer ones).
    pub hall: bool,
    pub controls: [f32; 10],
}

impl Galois {
    pub const TIME: usize = 0;
    pub const SIZE: usize = 1;
    pub const DAMPING: usize = 2;
    pub const MODULATION: usize = 3;
    pub const DIFFUSION: usize = 4;
    pub const PREDELAY: usize = 5;
    pub const HIGH_CUT: usize = 6;
    pub const LOW_SHELF: usize = 7;
    pub const MIX: usize = 8;
    pub const STEREO: usize = 9;
}

/// Direct coefficients of the Kontakt legacy Reverb network (`FUN_140aa3d10`):
/// eight damped combs into four allpasses behind a predelay, with the native
/// Freeverb delay lengths.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Freeverb {
    /// Comb feedback gain.
    pub feedback: f64,
    /// Comb damping coefficient `d`: `store = out*d + (1-d)*store`.
    pub damp: f64,
    /// Allpass gain `g`.
    pub allpass: f64,
    /// Input gain ahead of the network.
    pub input_gain: f64,
    /// Channel-mix weight `w` in `0..=1` (`1` keeps the channels apart): the
    /// input becomes `b*mean + (1-b)*x` with `b = 2*(1-w)`.
    pub width: f64,
}

/// Decay ratios of the low and high bands against the mid band.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecayBands {
    /// Decay time below `low_hz` as a multiple of `Network::decay_seconds`.
    pub low: f64,
    /// Decay time above `high_hz` as a multiple of `Network::decay_seconds`.
    pub high: f64,
    pub low_hz: f64,
    pub high_hz: f64,
}

/// The algorithmic part of a [`Config`]. Stereo in, wet stereo out.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Network {
    pub topology: Topology,
    /// Seconds for the tail to fall 60 dB (mid band).
    pub decay_seconds: f64,
    /// Scale of the delay lengths: room size. `0.05..=1.5`.
    pub size: f64,
    pub predelay_seconds: f64,
    /// Feedback lowpass, Hz.
    pub damping_hz: f64,
    /// Different decay in the low and high bands; `None` is one band.
    pub bands: Option<DecayBands>,
    /// Peak delay-time modulation, seconds.
    pub modulation_seconds: f64,
    /// Rate of that modulation, Hz.
    pub modulation_hz: f64,
    /// Allpass coefficient of the input diffuser (and of the comb layout's
    /// serial allpasses), `0..=0.75`.
    pub diffusion: f64,
    /// Input lowpass, Hz.
    pub input_cutoff_hz: f64,
    /// Linear level of the early-reflection taps added to the wet signal;
    /// zero is off.
    pub early: f64,
    /// Wet change below about 250 Hz in dB; zero or negative.
    pub low_shelf_db: f64,
    /// Wet stereo width: 0 mono, 1 as generated.
    pub width: f64,
    /// A gated reverb: the wet signal closes this long after the input falls
    /// silent.
    pub gate_seconds: Option<f64>,
    /// Feedback of the predelay line (an echo before the tank), `0..=0.95`.
    pub echo_feedback: f64,
    /// Coefficients for [`Topology::Freeverb`]; ignored by other topologies.
    pub freeverb: Option<Freeverb>,
    /// Controls for [`Topology::Galois`]; ignored by other topologies.
    pub galois: Option<Galois>,
}

impl Network {
    /// A mid-sized hall with every extra off.
    pub const DEFAULT: Network = Network {
        topology: Topology::Fdn,
        decay_seconds: 2.0,
        size: 1.0,
        predelay_seconds: 0.0,
        damping_hz: 6_000.0,
        bands: None,
        modulation_seconds: 0.000_75,
        modulation_hz: 0.7,
        diffusion: 0.375,
        input_cutoff_hz: 20_000.0,
        early: 0.0,
        low_shelf_db: 0.0,
        width: 1.0,
        gate_seconds: None,
        echo_feedback: 0.0,
        freeverb: None,
        galois: None,
    };
}

/// One reverb or convolution unit over a stereo signal (bus or master scope).
///
/// `output = dry * input + wet * (network(input) + input * impulse)`.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub network: Option<Network>,
    /// The impulse response (already shaped by the importing profile) the
    /// input is convolved with; adds no latency.
    pub impulse: Option<ImpulseRef>,
    /// Linear gain of the unprocessed input.
    pub dry: f64,
    /// Linear gain of the network and convolution sum.
    pub wet: f64,
}

impl Config {
    /// A network alone, wet only (the dry path is the bus's other send).
    pub const fn network(network: Network) -> Self {
        Self { network: Some(network), impulse: None, dry: 0.0, wet: 1.0 }
    }

    /// A convolution alone, mixed `dry * x + wet * (x * impulse)`.
    pub const fn convolution(impulse: ImpulseRef, dry: f64, wet: f64) -> Self {
        Self { network: None, impulse: Some(impulse), dry, wet }
    }

    /// Whether any field is NaN or infinite.
    pub fn finite(&self) -> bool {
        let net = self.network.is_none_or(|n| {
            let band = n.bands.is_none_or(|b| [b.low, b.high, b.low_hz, b.high_hz].iter().all(|v| v.is_finite()));
            band && n.gate_seconds.is_none_or(f64::is_finite)
                && n.freeverb.is_none_or(|f| [f.feedback, f.damp, f.allpass, f.input_gain, f.width].iter().all(|v| v.is_finite()))
                && n.galois.is_none_or(|g| g.controls.iter().all(|v| v.is_finite()))
                && [n.decay_seconds, n.size, n.predelay_seconds, n.damping_hz, n.modulation_seconds, n.modulation_hz, n.diffusion, n.input_cutoff_hz, n.early, n.low_shelf_db, n.width, n.echo_feedback]
                    .iter()
                    .all(|v| v.is_finite())
        });
        net && self.dry.is_finite() && self.wet.is_finite()
    }
}
