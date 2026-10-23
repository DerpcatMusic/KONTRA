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
