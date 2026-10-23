//! The bus-owned state of a Replika unit on path 1 (piece 3c part 5c, `k-replika-gaps.md`).
//!
//! The mode-1 chain holds six heap rings (12.6 MB at 48 kHz), which the packed slab state (`ReplikaState::store`) cannot
//! carry from block to block. A bus therefore owns one [`Stage`] per Replika unit whose index-28 path is 1, next to its
//! state and delay slab (`FilterContext::replikas`). Other paths keep the slab frame of mode 0.
//!
//! Lifetime: [`Stage::new`] runs on the control side at prepare and allocates the rings. [`Stage::reset`] runs on the
//! audio thread when the processor state is reset and clears the chain in place (no allocation). [`Stage::process`]
//! runs in 32-frame quanta: index 28 is written at each quantum start, as the native wrapper writes it
//! (`tools/re/replika_kemu_vectors.py`), so each write runs the spread snap. The quantum phase is the stream's frame
//! counter (`ReplikaState::frame`), carried across blocks, so block sizes do not move the writes.

use super::{ReplikaCore, ReplikaState, index};
use sampler_ir::fx::timemod::ReplikaControls;

/// The native control quantum: index 28 is written at each multiple of 32 frames of the stream.
const QUANTUM: usize = 32;

/// One Replika unit's persistent state on path 1: the core (its controls and write count) and the state with the chain.
pub(crate) struct Stage {
    core: ReplikaCore,
    state: ReplikaState,
    rate: f32,
}

impl Stage {
    /// Whether the unit runs the mode-1 chain at `rate`: index 28 selects path 1, and the rate is 48 kHz or below (the
    /// start is not ported above it, see `ReplikaCore::mode1_start`).
    pub(crate) fn applies(rate: f32, controls: &ReplikaControls) -> bool {
        super::head::path_for_index(controls.path) == 1 && rate <= 48_000.
    }

    /// Control side only (prepare): builds the core and allocates the chain's rings. `None` when [`Stage::applies`] is false.
    pub(crate) fn new(rate: f32, controls: &ReplikaControls) -> Option<Self> {
        let core = ReplikaCore::from_controls(rate, controls);
        let mode1 = core.mode1_start()?;
        let mut state = ReplikaState::new(rate);
        state.mode1 = Some(Box::new(mode1));
        Some(Self { core, state, rate })
    }

    /// Audio thread, no allocation: the chain and the rest of the state back to their start.
    pub(crate) fn reset(&mut self) {
        let Some(mut mode1) = self.state.mode1.take() else {
            return;
        };
        mode1.reset(self.rate);
        self.state = ReplikaState::new(self.rate);
        self.state.mode1 = Some(mode1);
    }

    /// One block of any length: index 28 is written at each quantum start, and the chain runs in quanta between them.
    pub(crate) fn process(&mut self, input: [&[f64]; 2], output: [&mut [f64]; 2]) {
        let [in_l, in_r] = input;
        let [out_l, out_r] = output;
        let n = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        let mut at = 0;
        while at < n {
            let phase = self.state.frame % QUANTUM;
            if phase == 0 {
                let path = self.core.raw[index::SELECT];
                self.core.set_param(index::SELECT, path);
            }
            let end = at + (QUANTUM - phase).min(n - at);
            self.core.process(
                &mut self.state,
                &mut [],
                [&in_l[at..end], &in_r[at..end]],
                [&mut out_l[at..end], &mut out_r[at..end]],
            );
            at = end;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const IMPULSE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/re/engine/vectors/replika-native-mode1-index28-v0p3.json"
    ));

    /// A reset replays the start: the impulse vector in mixed blocks, then reset, then the same again. Both runs are equal
    /// bit for bit (the rings are cleared, and no chain or write count leaks), and match the native vector.
    #[test]
    fn reset_replays_the_mode1_start() {
        let v: Value = serde_json::from_str(IMPULSE).unwrap();
        let samples = |c: usize| -> Vec<f64> {
            v["stim"][c].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
        };
        let (stim_l, stim_r) = (samples(0), samples(1));
        let controls = ReplikaControls { path: 0.3, ..ReplikaControls::default() };
        let mut stage = Stage::new(48_000., &controls).unwrap();
        let run = |stage: &mut Stage| {
            let (mut l, mut r) = (vec![0.; stim_l.len()], vec![0.; stim_l.len()]);
            for (a, b) in [(0, 512), (512, 1512), (1512, 2024), (2024, 3024), (3024, 4096)] {
                stage.process([&stim_l[a..b], &stim_r[a..b]], [&mut l[a..b], &mut r[a..b]]);
            }
            (l, r)
        };
        let first = run(&mut stage);
        stage.reset();
        let second = run(&mut stage);
        assert!(first == second, "a reset must replay the start");
        let want = v["outL"].as_array().unwrap().iter().zip(v["outR"].as_array().unwrap());
        let err = first
            .0
            .iter()
            .zip(&first.1)
            .zip(want)
            .map(|((l, r), (wl, wr))| {
                (*l as f32 - wl.as_f64().unwrap() as f32).abs().max((*r as f32 - wr.as_f64().unwrap() as f32).abs())
            })
            .fold(0f32, f32::max);
        assert!(err <= 1e-5, "max abs error {err:e}");
    }
}
