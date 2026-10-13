//! Universal `space` DSP (wave 4): every reverb and convolution effect is one
//! [`Space`], configured by [`sampler_ir::fx::space::Config`].
//!
//! A unit runs an algorithmic network (FDN, comb/allpass, plate or diffuser,
//! with predelay, early reflections, gate and tone), a zero-latency
//! partitioned convolution, or both summed, then mixes with the dry input.
//! Everything is allocated by [`Space::new`] on the control side; processing
//! allocates nothing.

mod comb;
mod common;
mod convolver;
mod fdn;
mod freeverb;
mod galois;
mod plate;
mod shape;
mod sparkverb;

pub use convolver::{Impulse, MAX_IMPULSE_FRAMES};
pub use convolver::ConvolutionUpload;
pub use shape::{neutral_impulse_shape, shape_impulse};

use crate::Error;
use common::{Early, Tone};
use convolver::Conv;
use sampler_ir::fx::space::{Config, Network, Topology};
use std::sync::Arc;

/// Longest predelay and decay accepted.
pub const MAX_PREDELAY_SECONDS: f64 = 2.0;
pub const MAX_DECAY_SECONDS: f64 = 60.0;
/// Largest `size` (base lengths scale by it) and modulation depth accepted.
pub const MAX_SIZE: f64 = 1.5;
pub const MAX_MODULATION_SECONDS: f64 = 0.0015;

fn within(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

fn network_valid(n: &Network) -> bool {
    n.bands.is_none_or(|b| {
        within(b.low, 0.05, 20.0) && within(b.high, 0.05, 20.0) && within(b.low_hz, 10.0, 20_000.0) && within(b.high_hz, b.low_hz, 192_000.0)
    }) && n.gate_seconds.is_none_or(|g| within(g, 0.001, 10.0))
        && within(n.decay_seconds, 0.01, MAX_DECAY_SECONDS)
        && within(n.size, 0.05, MAX_SIZE)
        && within(n.damping_hz, 20.0, 192_000.0)
        && within(n.modulation_seconds, 0.0, MAX_MODULATION_SECONDS)
        && within(n.modulation_hz, 0.01, 20.0)
        && within(n.diffusion, 0.0, 0.75)
        && within(n.predelay_seconds, 0.0, MAX_PREDELAY_SECONDS)
        && within(n.input_cutoff_hz, 20.0, 192_000.0)
        && within(n.early, 0.0, 4.0)
        && within(n.low_shelf_db, -60.0, 0.0)
        && within(n.width, 0.0, 1.0)
        && within(n.echo_feedback, 0.0, 0.95)
        && (n.topology != Topology::Freeverb
            || n.freeverb.is_some_and(|f| {
                within(f.feedback, 0.0, 0.999) && within(f.damp, 0.0, 1.0) && within(f.allpass, 0.0, 0.99) && within(f.input_gain, 0.0, 4.0) && within(f.width, 0.0, 1.0)
            }))
        && (n.topology != Topology::Galois || n.galois.is_some_and(|g| g.controls.iter().all(|v| (0.0..=1.0).contains(v))))
        && (n.topology != Topology::SparkVerb
            || n.spark.is_some_and(|s| {
                let w = |v: f32, lo: f64, hi: f64| within(f64::from(v), lo, hi);
                (1..=5).contains(&s.quality)
                    && s.mode <= 2
                    && w(s.room_m, 1.0, 100.0)
                    && w(s.shape, 0.0, 1.0)
                    && w(s.decay, 0.01, 100.0)
                    && w(s.decay_low, 0.01, 100.0)
                    && w(s.decay_high, 0.01, 100.0)
                    && w(s.freq_low, 1.0, 192_000.0)
                    && w(s.freq_high, 1.0, 192_000.0)
                    && w(s.mod_cents, 0.0, 100.0)
                    && w(s.mod_hz, 0.01, 20.0)
                    && w(s.width, 0.0, 1.0)
                    && w(s.rolloff_hz, 1.0, 192_000.0)
                    && w(s.predelay_seconds, 0.0, 0.1)
                    && s.diffuser.is_none_or(|(g, ms)| w(g, 0.0, 1.0) && w(ms, 0.1, 50.0))
            }))
}

/// Whether `c` can be built: finite, in range, and not empty.
pub(crate) fn valid(c: &Config) -> bool {
    (c.network.is_some() || c.impulse.is_some())
        && c.dry.is_finite()
        && c.wet.is_finite()
        && c.network.is_none_or(|n| network_valid(&n))
}

/// Frames of output after the input falls silent until the unit is quiet.
/// `impulse` is the configuration's impulse, if any.
pub(crate) fn tail_frames(c: &Config, rate: u32, impulse: Option<&Impulse>) -> u32 {
    let net = c.network.map_or(0.0, |n| {
        let slowest = n.bands.map_or(1.0, |b| b.low.max(b.high).max(1.0));
        let echo = if n.echo_feedback > 0.0 && n.predelay_seconds > 0.0 {
            // Repeats until the echo is 120 dB down, each one predelay apart.
            n.predelay_seconds * (1.0 + 6.0 / -n.echo_feedback.log10())
        } else {
            n.predelay_seconds
        };
        (echo + 2.0 * n.decay_seconds * slowest) * f64::from(rate)
    });
    let conv = impulse.map_or(0.0, |i| convolver::tail_frames(i) as f64);
    net.max(conv).min(f64::from(u32::MAX)) as u32
}

enum Core {
    Fdn(fdn::Fdn),
    Comb(comb::CombAllpass),
    Plate(plate::Plate),
    Diffuser(comb::Diffuser),
    Freeverb(freeverb::Freeverb),
    Galois(Box<galois::GaloisCore>),
    Spark(sparkverb::Spark),
}

/// Closes the wet signal a set time after the input falls silent.
struct Gate {
    hold: usize,
    left: usize,
    gain: f32,
    coef: f32,
}

impl Gate {
    fn new(seconds: f64, rate: f32) -> Self {
        Self { hold: (seconds * f64::from(rate)) as usize, left: 0, gain: 0.0, coef: common::one_pole(50.0, rate) }
    }
    #[inline(always)]
    fn tick(&mut self, input_peak: f32) -> f32 {
        // -60 dBFS opens the gate.
        if input_peak > 1e-3 {
            self.left = self.hold;
        } else {
            self.left = self.left.saturating_sub(1);
        }
        let target = if self.left > 0 { 1.0 } else { 0.0 };
        self.gain = crate::dsp::kernels::one_pole32(self.gain, target, self.coef);
        self.gain
    }
}

struct Net {
    core: Core,
    tone: Tone,
    early: Option<Early>,
    gate: Option<Gate>,
    pos: usize,
}

impl Net {
    fn new(n: &Network, rate: u32) -> Result<Self, Error> {
        let core = match n.topology {
            Topology::Fdn => Core::Fdn(fdn::Fdn::new(n, rate)?),
            Topology::CombAllpass => Core::Comb(comb::CombAllpass::new(n, rate)?),
            Topology::Plate => Core::Plate(plate::Plate::new(n, rate)?),
            Topology::Diffuser => Core::Diffuser(comb::Diffuser::new(n, rate)?),
            Topology::Freeverb => Core::Freeverb(freeverb::Freeverb::new(n, rate)?),
            Topology::Galois => Core::Galois(Box::new(galois::GaloisCore::new(n, rate)?)),
            Topology::SparkVerb => Core::Spark(sparkverb::Spark::new(n, rate)?),
        };
        let rate = rate as f32;
        // The Galois and SparkVerb cores do their own predelay, width and tone.
        let native = matches!(n.topology, Topology::Galois | Topology::SparkVerb);
        Ok(Self {
            core,
            tone: if native { Tone::new(1.0, 0.0, rate) } else { Tone::new(n.width, n.low_shelf_db, rate) },
            early: (!native && n.early > 0.0).then(|| Early::new(n.early, n.size, (n.predelay_seconds as f32 * rate) as usize, rate)),
            gate: n.gate_seconds.filter(|_| !native).map(|g| Gate::new(g, rate)),
            pos: 0,
        })
    }

    fn clear(&mut self) {
        match &mut self.core {
            Core::Fdn(c) => c.clear(),
            Core::Comb(c) => c.clear(),
            Core::Plate(c) => c.clear(),
            Core::Diffuser(c) => c.clear(),
            Core::Freeverb(c) => c.clear(),
            Core::Galois(c) => c.clear(),
            Core::Spark(c) => c.clear(),
        }
        self.tone.clear();
        if let Some(e) = &mut self.early {
            e.clear();
        }
        if let Some(g) = &mut self.gate {
            (g.left, g.gain) = (0, 0.0);
        }
    }

    /// Replace `l`/`r` with the wet signal.
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        // The early taps and the gate read the dry input.
        let mut input = [[0f32; crate::dsp::BLOCK]; 2];
        let n = l.len().min(crate::dsp::BLOCK);
        input[0][..n].copy_from_slice(&l[..n]);
        input[1][..n].copy_from_slice(&r[..n]);
        match &mut self.core {
            Core::Fdn(c) => c.process(l, r),
            Core::Comb(c) => c.process(l, r),
            Core::Plate(c) => c.process(l, r),
            Core::Diffuser(c) => c.process(l, r),
            Core::Freeverb(c) => c.process(l, r),
            Core::Galois(c) => c.process(l, r),
            Core::Spark(c) => c.process(l, r),
        }
        for i in 0..n {
            let pos = self.pos;
            self.pos = pos.wrapping_add(1);
            let (mut a, mut b) = (l[i], r[i]);
            if let Some(e) = &mut self.early {
                let [ea, eb] = e.tick(pos, 0.5 * (input[0][i] + input[1][i]));
                (a, b) = (a + ea, b + eb);
            }
            if let Some(g) = &mut self.gate {
                let gain = g.tick(input[0][i].abs().max(input[1][i].abs()));
                (a, b) = (a * gain, b * gain);
            }
            [l[i], r[i]] = self.tone.tick(a, b);
        }
    }
}

/// One reverb or convolution unit's state.
pub(crate) struct Space {
    net: Option<Net>,
    conv: Option<Conv>,
    dry: f32,
    wet: f32,
}

impl Space {
    pub(crate) fn new(c: &Config, rate: u32, impulses: &[Arc<Impulse>]) -> Result<Self, Error> {
        if rate == 0 || !valid(c) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            net: c.network.as_ref().map(|n| Net::new(n, rate)).transpose()?,
            conv: c
                .impulse
                .map(|i| impulses.get(i.0).map(|i| Conv::new(i)).ok_or(Error::InvalidInput))
                .transpose()?,
            dry: c.dry as f32,
            wet: c.wet as f32,
        })
    }

    pub(crate) fn clear(&mut self) {
        if let Some(n) = &mut self.net {
            n.clear();
        }
        if let Some(c) = &mut self.conv {
            c.clear();
        }
    }

    /// Exchange the convolution with `upload`'s, taking its mix too.
    pub(crate) fn swap_convolution(&mut self, upload: &mut ConvolutionUpload) -> Result<(), Error> {
        let conv = self.conv.as_mut().ok_or(Error::InvalidInput)?;
        std::mem::swap(conv, &mut upload.conv);
        (self.dry, self.wet) = (upload.dry, upload.wet);
        Ok(())
    }

    /// Process up to [`crate::dsp::BLOCK`] frames of each channel in place.
    pub(crate) fn process(&mut self, [left, right]: [&mut [f32]; 2]) {
        let n = left.len().min(right.len()).min(crate::dsp::BLOCK);
        let (left, right) = (&mut left[..n], &mut right[..n]);
        let mut input = [[0f32; crate::dsp::BLOCK]; 2];
        input[0][..n].copy_from_slice(left);
        input[1][..n].copy_from_slice(right);
        let mut wet = [[0f32; crate::dsp::BLOCK]; 2];
        if let Some(net) = &mut self.net {
            wet[0][..n].copy_from_slice(left);
            wet[1][..n].copy_from_slice(right);
            let [wl, wr] = &mut wet;
            net.process(&mut wl[..n], &mut wr[..n]);
        }
        if let Some(conv) = &mut self.conv {
            let (mut cl, mut cr) = (input[0], input[1]);
            conv.process([&mut cl[..n], &mut cr[..n]]);
            for i in 0..n {
                wet[0][i] += cl[i];
                wet[1][i] += cr[i];
            }
        }
        for i in 0..n {
            left[i] = wet[0][i] * self.wet + input[0][i] * self.dry;
            right[i] = wet[1][i] * self.wet + input[1][i] * self.dry;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::ImpulseRef;

    fn run(c: &Config, impulses: &[Arc<Impulse>], frames: usize) -> (Vec<f32>, Vec<f32>) {
        let mut s = Space::new(c, 48_000, impulses).unwrap();
        let mut l = vec![0.0; frames];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
            s.process([l, r]);
        }
        (l, r)
    }

    #[test]
    fn every_topology_rings_and_stays_finite() {
        for topology in [Topology::Fdn, Topology::CombAllpass, Topology::Plate, Topology::Diffuser] {
            let c = Config::network(Network { topology, early: 0.5, ..Network::DEFAULT });
            let (l, r) = run(&c, &[], 48_000);
            assert!(l.iter().chain(&r).all(|v| v.is_finite()), "{topology:?}");
            let e: f32 = l.iter().map(|v| v * v).sum();
            assert!(e > 1e-4, "{topology:?} silent: {e}");
        }
    }

    #[test]
    fn dry_and_wet_mix_linearly() {
        let ir = Arc::new(Impulse::new(vec![0.5], vec![0.5]).unwrap());
        let c = Config::convolution(ImpulseRef(0), 0.25, 2.0);
        let (l, _) = run(&c, &[ir], 128);
        assert!((l[0] - (0.25 + 2.0 * 0.5)).abs() < 1e-6, "{}", l[0]);
        assert!(l[1..].iter().all(|v| v.abs() < 1e-6));
    }

    #[test]
    fn convolution_and_network_sum() {
        let ir = Arc::new(Impulse::new(vec![1.0], vec![1.0]).unwrap());
        let net = Network::DEFAULT;
        let both = Config { network: Some(net), impulse: Some(ImpulseRef(0)), dry: 0.0, wet: 1.0 };
        let (a, _) = run(&both, &[ir.clone()], 4800);
        let (b, _) = run(&Config::network(net), &[], 4800);
        // A unit impulse response adds the input itself to the network's tail.
        assert!((a[0] - (b[0] + 1.0)).abs() < 1e-5);
        assert!((a[100] - b[100]).abs() < 1e-5);
    }

    #[test]
    fn gate_closes_the_tail() {
        let open = Config::network(Network { decay_seconds: 3.0, ..Network::DEFAULT });
        let gated = Config::network(Network { decay_seconds: 3.0, gate_seconds: Some(0.1), ..Network::DEFAULT });
        let (a, _) = run(&open, &[], 48_000);
        let (b, _) = run(&gated, &[], 48_000);
        let late = |v: &[f32]| v[24_000..].iter().map(|x| x * x).sum::<f32>();
        assert!(late(&a) > 1e-4);
        assert!(late(&b) < late(&a) * 1e-3, "{} vs {}", late(&b), late(&a));
    }

    #[test]
    fn rejects_empty_and_out_of_range() {
        assert!(!valid(&Config { network: None, impulse: None, dry: 0.0, wet: 1.0 }));
        assert!(!valid(&Config::network(Network { size: 0.0, ..Network::DEFAULT })));
        assert!(Space::new(&Config::convolution(ImpulseRef(3), 0.0, 1.0), 48_000, &[]).is_err());
    }

    #[test]
    fn tail_follows_decay_predelay_and_impulse() {
        let c = Config::network(Network { decay_seconds: 2.0, predelay_seconds: 0.5, ..Network::DEFAULT });
        assert_eq!(tail_frames(&c, 48_000, None), ((0.5 + 4.0) * 48_000.0) as u32);
        let ir = Impulse::new(vec![1.0; 1000], vec![1.0; 1000]).unwrap();
        let c = Config::convolution(ImpulseRef(0), 0.0, 1.0);
        assert!(tail_frames(&c, 48_000, Some(&ir)) >= 1000);
    }
}
