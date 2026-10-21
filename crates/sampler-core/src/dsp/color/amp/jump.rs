//! Kontakt Jump (`BEffectJump`, vtable 0x144fc0330, ctor 0x140a86880, reader 0x140d01e10):
//! a JCM800-style preamp, boost stage, two gain stages, a P/N splitter and a power tube
//! (`NI::DSP::JUMP::WaveShapingTables`). Its per-chunk process is 0x140aa33c0 (32-sample
//! chunks) into the kernel 0x140aac6f0.
//!
//! Only the default-knob behaviour is measured. The knob laws are not read, so every knob is
//! ignored and the preset below is the shared chain fitted to one native trace (the Wine
//! oracle's default step, `jump_default.f32le`, left channel). The fit has a relative RMS
//! error of 0.242 (fit_chain.py); it is not parity.
// ponytail: fixed chain, knobs ignored, one-trace fit; add knob laws and per-stage tables
// (kernel 0x140aac6f0, stage state at 0x4680..0x49bc) when they are read.
use super::chain::{Engine as Chain, Spec, State as ChainState, KNOTS};
use super::super::super::Planar;

/// The fitted preset: pre high-pass 160 Hz, post low-pass 3 kHz, input gain 2, 48-sample latency.
pub(crate) const PRESET: Spec = Spec {
    pre_fc: 160.0,
    post_fc: 3000.0,
    gain: 2.0,
    delay: 48,
    table: [
        1.633978472e-01, 1.542131415e-01, 1.514852117e-01, 1.476105822e-01, 1.453915931e-01, 1.459502801e-01,
        1.411882524e-01, 1.374592891e-01, 1.058939257e-02, -1.200082680e-01, -1.358300400e-01, -1.531861306e-01,
        -1.518108250e-01, -1.558403395e-01, -1.609432846e-01, -1.655023668e-01, -1.744491427e-01,
    ],
};

const _: () = assert!(PRESET.table.len() == KNOTS);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    c: [ChainState; 2],
}

impl State {
    pub(crate) fn finite(&self) -> bool {
        self.c.iter().all(|c| c.finite())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Engine {
    chain: Chain,
}

impl Engine {
    pub(crate) fn new(rate: u32) -> Self {
        Self { chain: Chain::new(PRESET, rate) }
    }

    pub(crate) fn process(&self, s: &mut State, block: &mut Planar, len: usize) {
        self.chain.process(&mut s.c, block, len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stimulus of stimrender.py `stim_ramp`: 0.6 s 200 Hz sine ramping 0 -> 1.0 at 48 kHz.
    fn stimulus() -> Vec<f64> {
        let n = 28_800;
        (0..n)
            .map(|i| (2.0 * std::f64::consts::PI * 200.0 * i as f64 / 48_000.0).sin() * i as f64 / (n - 1) as f64)
            .collect()
    }

    /// The default-step left channel of the native trace, against this chain with its latency.
    /// Tolerance is the fit residual (0.242) plus margin, not parity.
    #[test]
    fn default_trace_within_fit_tolerance() {
        let native: Vec<f64> = include_bytes!("jump_default.f32le")
            .chunks_exact(4)
            .map(|b| f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])))
            .collect();
        let x = stimulus();
        assert_eq!(native.len(), x.len());
        let engine = Chain::new(PRESET, 48_000);
        let mut s = ChainState::default();
        let model: Vec<f64> = x.iter().map(|&v| engine.sample(&mut s, v)).collect();
        let d = PRESET.delay;
        let (mut err, mut energy) = (0.0, 0.0);
        for t in d..native.len() {
            err += (model[t] - native[t]).powi(2);
            energy += native[t].powi(2);
        }
        let rel = (err / energy).sqrt();
        assert!(rel < 0.25, "relative RMS {rel:.4} exceeds the fit tolerance");
    }
}
