//! What the plot draws: the engine's gain and the depth curves the bands make.
//!
//! The reference reads the engine's curve verbatim -- its ballistics are the
//! engine's own attack and release -- so nothing here eases or smooths: a
//! frame in is a frame drawn, and with no audio running the last one stays.

use smooth_core::plot::{Frame, PLOT_BINS};

/// The plot's data, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotView {
    /// The gain each channel is taking, linear: one is no reduction.
    pub gain: [[f32; PLOT_BINS]; 2],
    /// What the bands ask of the reduction, per channel, in dB. Positive is deeper.
    pub depth: [[f32; PLOT_BINS]; 2],
}

impl Default for PlotView {
    fn default() -> Self {
        Self {
            gain: [[1.0; PLOT_BINS]; 2],
            depth: [[0.0; PLOT_BINS]; 2],
        }
    }
}

impl PlotView {
    /// Take one published frame in.
    pub fn advance(&mut self, frame: &Frame) {
        for (now, &gain) in self.gain.iter_mut().flatten().zip(frame.gain.iter().flatten()) {
            *now = if gain.is_finite() { gain } else { 1.0 };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_drawn_as_it_came() {
        let mut view = PlotView::default();
        let mut frame = Frame::default();
        frame.gain[0][10] = 0.5;
        frame.gain[1][3] = f32::NAN;
        view.advance(&frame);
        assert_eq!(view.gain[0][10], 0.5);
        assert_eq!(view.gain[1][3], 1.0);
    }
}
