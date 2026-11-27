//! Unified moldable spatial / reverb processor (`Space`) covering every
//! reverberation, diffusion, and impulse topology through one static-dispatch
//! surface:
//! - Pure stereo diffusion cloud ([`Diffuser`], zero feedback tank)
//! - Algorithmic tanks ([`Fdn`], [`Combs`], [`Hadamard`], [`Network`], [`Plate`])
//! - Zero-latency partitioned-FFT convolution ([`Convolver`])
//! - Hybrid convolution + FDN morph (`Space::Hybrid`), crossfading smoothly
//!   from early impulse response (`blend = 0.0`) to algorithmic tail (`blend = 1.0`).

use crate::fdn::{MAX_PREDELAY_MS, MAX_SIZE};
use crate::{Combs, Convolver, Fdn, Hadamard, Network, Plate, Settings};
use kontra_dsp_core::{DelayLine, flush_f32};

/// Mutually prime stereo allpass delay times (ms) at `size = 1.0`.
const DIFF_L_MS: [f32; 4] = [3.7, 7.1, 13.3, 21.7];
const DIFF_R_MS: [f32; 4] = [4.3, 8.3, 14.9, 23.9];

/// Pure 4-stage stereo Schroeder allpass diffusion cloud (zero feedback tank).
///
/// Shares [`Settings`] with every algorithmic reverb: `size` scales the
/// allpass stages, `diffusion` sets the allpass coefficient, `predelay_ms`
/// offsets onset, `high_cut_hz` damps the output, and `stereo` sets width.
pub struct Diffuser {
    rate: f32,
    predelay: [DelayLine; 2],
    stages: [[DelayLine; 4]; 2],
    gain: f32,
    high_cut: f32,
    out: [f32; 2],
    stereo: f32,
}

impl Diffuser {
    #[must_use]
    pub fn new(s: Settings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        let frames = |ms: f32| (ms * 0.001 * rate).round().max(1.0) as usize;
        let mut d = Self {
            rate,
            predelay: [
                DelayLine::new(frames(MAX_PREDELAY_MS)),
                DelayLine::new(frames(MAX_PREDELAY_MS)),
            ],
            stages: [
                DIFF_L_MS.map(|ms| DelayLine::new(frames(ms * MAX_SIZE))),
                DIFF_R_MS.map(|ms| DelayLine::new(frames(ms * MAX_SIZE))),
            ],
            gain: 0.5,
            high_cut: 0.0,
            out: [0.0; 2],
            stereo: 1.0,
        };
        d.set(s);
        d
    }

    pub fn set(&mut self, s: Settings) {
        let rate = self.rate;
        let frames = |ms: f32| (ms * 0.001 * rate).round().max(1.0) as usize;
        let pre = frames(s.predelay_ms.clamp(0.0, MAX_PREDELAY_MS));
        let size = s.size.clamp(0.25, MAX_SIZE);
        for ((pre_line, bank), times) in self
            .predelay
            .iter_mut()
            .zip(&mut self.stages)
            .zip([DIFF_L_MS, DIFF_R_MS])
        {
            pre_line.set_len(pre);
            for (line, ms) in bank.iter_mut().zip(times) {
                line.set_len(frames(ms * size));
            }
        }
        self.gain = 0.78 * s.diffusion.clamp(0.0, 1.0);
        self.high_cut =
            (-std::f32::consts::TAU * s.high_cut_hz.clamp(20.0, 0.49 * rate) / rate).exp();
        self.stereo = s.stereo.clamp(0.0, 1.0);
    }

    pub fn reset(&mut self) {
        for p in &mut self.predelay {
            p.reset();
        }
        for bank in &mut self.stages {
            for line in bank {
                line.reset();
            }
        }
        self.out = [0.0; 2];
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let g = self.gain;
        let hc = self.high_cut;
        let stereo = self.stereo;
        for frame in io {
            let mut lr = [0.0f32; 2];
            for ((((pre_line, bank), out_st), dst), &inp) in self
                .predelay
                .iter_mut()
                .zip(&mut self.stages)
                .zip(&mut self.out)
                .zip(&mut lr)
                .zip(frame.iter())
            {
                let mut x = pre_line.read_len();
                pre_line.push(inp);
                for stage in bank {
                    let d = stage.read_len();
                    let y = flush_f32(d - g * x);
                    stage.push(flush_f32(x + g * y));
                    x = y;
                }
                let cut = flush_f32(x + hc * (*out_st - x));
                *out_st = cut;
                *dst = cut;
            }
            let mid = 0.5 * (lr[0] + lr[1]);
            let side = 0.5 * (lr[0] - lr[1]) * stereo;
            *frame = [mid + side, mid - side];
        }
    }
}

/// One unified spatial processor molding across pure diffusion, algorithmic
/// reverb topologies, partitioned-FFT convolution, and hybrid IR+FDN blends.
pub enum Space {
    Fdn(Box<Fdn>),
    Combs(Box<Combs>),
    Hadamard(Box<Hadamard>),
    Network(Box<Network>),
    Plate(Box<Plate>),
    Convolver(Box<Convolver>),
    Diffuser(Box<Diffuser>),
    /// Smoothly morphs between partitioned-FFT convolution (`blend = 0.0`)
    /// and algorithmic FDN tail (`blend = 1.0`) with zero audio-thread allocation.
    Hybrid {
        early: Box<Convolver>,
        late: Box<Fdn>,
        blend: f32,
    },
}

impl Space {
    #[must_use]
    pub fn fdn(s: Settings, rate: u32) -> Self {
        Self::Fdn(Box::new(Fdn::new(s, rate)))
    }

    #[must_use]
    pub fn combs(law: &kontra_law::CombNetwork, s: Settings, rate: u32) -> Self {
        Self::Combs(Box::new(Combs::new(law, s, rate)))
    }

    #[must_use]
    pub fn hadamard(law: kontra_law::HadamardNetwork, s: Settings, rate: u32) -> Self {
        Self::Hadamard(Box::new(Hadamard::new(law, s, rate)))
    }

    #[must_use]
    pub fn network(law: &kontra_law::ReverbNetwork, s: Settings, rate: u32) -> Self {
        Self::Network(Box::new(Network::new(law, s, rate)))
    }

    #[must_use]
    pub fn plate(s: Settings, rate: u32) -> Self {
        Self::Plate(Box::new(Plate::new(s, rate)))
    }

    #[must_use]
    pub fn convolver(c: Convolver) -> Self {
        Self::Convolver(Box::new(c))
    }

    #[must_use]
    pub fn diffuser(s: Settings, rate: u32) -> Self {
        Self::Diffuser(Box::new(Diffuser::new(s, rate)))
    }

    #[must_use]
    pub fn hybrid(early: Convolver, late_settings: Settings, rate: u32, blend: f32) -> Self {
        Self::Hybrid {
            early: Box::new(early),
            late: Box::new(Fdn::new(late_settings, rate)),
            blend: blend.clamp(0.0, 1.0),
        }
    }

    /// Updates hybrid convolution-to-FDN morph ratio (`0.0` = pure IR, `1.0` = pure FDN).
    pub fn set_blend(&mut self, blend: f32) {
        if let Self::Hybrid { blend: b, .. } = self {
            *b = blend.clamp(0.0, 1.0);
        }
    }

    /// Updates algorithmic/diffusion parameters in place (no-op for pure convolution).
    pub fn set(&mut self, s: Settings) {
        match self {
            Self::Fdn(u) => u.set(s),
            Self::Combs(u) => u.set(s),
            Self::Hadamard(u) => u.set(s),
            Self::Network(u) => u.set(s),
            Self::Plate(u) => u.set(s),
            Self::Diffuser(u) => u.set(s),
            Self::Hybrid { late, .. } => late.set(s),
            Self::Convolver(_) => {}
        }
    }

    /// Resets internal delay/filter state where supported.
    pub fn reset(&mut self) {
        match self {
            Self::Fdn(u) => u.reset(),
            Self::Plate(u) => u.reset(),
            Self::Convolver(u) => u.reset(),
            Self::Diffuser(u) => u.reset(),
            Self::Hybrid { early, late, .. } => {
                early.reset();
                late.reset();
            }
            Self::Combs(_) | Self::Hadamard(_) | Self::Network(_) => {}
        }
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        match self {
            Self::Fdn(u) => u.process(io),
            Self::Combs(u) => u.process(io),
            Self::Hadamard(u) => u.process(io),
            Self::Network(u) => u.process(io),
            Self::Plate(u) => u.process(io),
            Self::Convolver(u) => u.process(io),
            Self::Diffuser(u) => u.process(io),
            Self::Hybrid { early, late, blend } => {
                let w_late = *blend;
                let w_early = 1.0 - w_late;
                for chunk in io.chunks_mut(64) {
                    let mut late_buf = [[0.0f32; 2]; 64];
                    let Some(late_slice) = late_buf.get_mut(..chunk.len()) else {
                        continue;
                    };
                    late_slice.copy_from_slice(chunk);
                    early.process(chunk);
                    late.process(late_slice);
                    for (dst, src) in chunk.iter_mut().zip(late_slice.iter()) {
                        dst[0] = w_early * dst[0] + w_late * src[0];
                        dst[1] = w_early * dst[1] + w_late * src[1];
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn memory(&self) -> Option<usize> {
        match self {
            Self::Network(u) => Some(u.memory()),
            Self::Convolver(u) => Some(u.taps()),
            Self::Hybrid { early, .. } => Some(early.taps()),
            Self::Fdn(_)
            | Self::Combs(_)
            | Self::Hadamard(_)
            | Self::Plate(_)
            | Self::Diffuser(_) => None,
        }
    }

    #[must_use]
    pub fn written(&self) -> f32 {
        match self {
            Self::Network(u) => u.written(),
            _ => 0.0,
        }
    }

    pub fn idle(&mut self, silence: &mut [[f32; 2]]) {
        if let Self::Network(u) = self {
            u.idle(silence);
        }
    }
}
