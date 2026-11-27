//! Shared drive building blocks reused across nonlinear processors
//! (`AmpStack`, `ColorSat`, etc.): DC blocker and 3-band EQ tone stack.

use kontra_dsp_core::{Biquad, Side, flush_f32, peak, shelf};

/// First-order stereo DC blocker (`y[n] = x[n] - x[n-1] + r * y[n-1]`).
#[derive(Clone, Debug)]
pub struct DcBlocker {
    st: [[f32; 2]; 2],
    r: f32,
}

impl DcBlocker {
    #[must_use]
    pub const fn new(r: f32) -> Self {
        Self {
            st: [[0.0; 2]; 2],
            r,
        }
    }

    /// Processes a single sample on channel `ch` (`0` or `1`).
    #[inline]
    pub fn tick_ch(&mut self, ch: usize, x: f32) -> f32 {
        let Some(st) = self.st.get_mut(ch) else {
            return x;
        };
        let y = flush_f32(x - st[0] + self.r * st[1]);
        st[0] = x;
        st[1] = y;
        y
    }

    pub fn reset(&mut self) {
        self.st = [[0.0; 2]; 2];
    }
}

/// Three-band stereo tone stack (low shelf, peaking mid, high shelf).
#[derive(Clone, Debug)]
pub struct ToneStack3 {
    bass: Biquad,
    mid: Biquad,
    treble: Biquad,
}

impl ToneStack3 {
    #[must_use]
    pub fn new(bass_hz: f32, mid_hz: f32, treble_hz: f32, rate: u32) -> Self {
        Self {
            bass: shelf(bass_hz, 0.0, 0.7, Side::Low, rate),
            mid: peak(mid_hz, 1.0, 0.0, rate),
            treble: shelf(treble_hz, 0.0, 0.7, Side::High, rate),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn retune(
        &mut self,
        bass_hz: f32,
        bass_db: f32,
        mid_hz: f32,
        mid_q: f32,
        mid_db: f32,
        treble_hz: f32,
        treble_db: f32,
        rate: u32,
    ) {
        self.bass
            .retune(&shelf(bass_hz, bass_db, 0.7, Side::Low, rate));
        self.mid.retune(&peak(mid_hz, mid_q, mid_db, rate));
        self.treble
            .retune(&shelf(treble_hz, treble_db, 0.7, Side::High, rate));
    }

    #[inline]
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        self.bass.process(io);
        self.mid.process(io);
        self.treble.process(io);
    }

    pub fn reset(&mut self) {
        self.bass.reset();
        self.mid.reset();
        self.treble.reset();
    }
}
