//! What the plot draws: the engine's frame, smoothed for the eye, and the
//! depth curves the bands make.
//!
//! The engine publishes a frame per block and the editor draws sixty a second,
//! so the frames are eased: a rise is followed fast and a fall slowly, the way
//! a meter is read. Nothing here is a picture of anything -- every number is the
//! engine's, and with no audio running the spectrum is absent and the
//! reduction is zero rather than an invented silhouette.

use smooth_core::plot::{Frame, PLOT_BINS};

/// How much of the distance to a rising value one frame covers, and to a
/// falling one.
const SPECTRUM_RISE: f32 = 0.55;
const SPECTRUM_FALL: f32 = 0.10;
const REDUCTION_RISE: f32 = 0.65;
const REDUCTION_FALL: f32 = 0.18;

/// The floor a silent bin is held at, in decibels.
pub const FLOOR_DB: f32 = -120.0;

/// The plot's data, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotView {
    /// The input spectrum, eased, in dB relative to a full-scale sine.
    pub spectrum: [f32; PLOT_BINS],
    /// The reduction each channel took, eased. A cut is positive.
    pub reduction: [[f32; PLOT_BINS]; 2],
    /// What the bands ask of the reduction, per channel. Positive is deeper.
    pub depth: [[f32; PLOT_BINS]; 2],
    /// Whether the engine has published a frame yet.
    pub live: bool,
}

impl Default for PlotView {
    fn default() -> Self {
        Self {
            spectrum: [FLOOR_DB; PLOT_BINS],
            reduction: [[0.0; PLOT_BINS]; 2],
            depth: [[0.0; PLOT_BINS]; 2],
            live: false,
        }
    }
}

fn ease(now: f32, target: f32, rise: f32, fall: f32) -> f32 {
    let step = if target > now { rise } else { fall };
    now + (target - now) * step
}

impl PlotView {
    /// Take one published frame in.
    pub fn advance(&mut self, frame: &Frame) {
        self.live = true;
        for (now, &target) in self.spectrum.iter_mut().zip(&frame.spectrum) {
            *now = ease(*now, target.max(FLOOR_DB), SPECTRUM_RISE, SPECTRUM_FALL);
        }
        for (channel, target) in self.reduction.iter_mut().zip(&frame.reduction) {
            for (now, &target) in channel.iter_mut().zip(target) {
                let target = if target.is_finite() { target.max(0.0) } else { 0.0 };
                *now = ease(*now, target, REDUCTION_RISE, REDUCTION_FALL);
            }
        }
    }

    /// Whether the picture has settled: no reduction, no spectrum.
    pub fn at_rest(&self) -> bool {
        self.spectrum.iter().all(|v| *v < FLOOR_DB + 0.1)
            && self.reduction.iter().flatten().all(|v| *v < 0.01)
    }

    /// Let a frame's worth of time pass with nothing published: what was
    /// there fades, so a stopped transport does not leave a frozen picture.
    pub fn idle(&mut self) {
        self.advance(&Frame::default());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rise_is_followed_faster_than_a_fall() {
        let mut view = PlotView::default();
        let mut frame = Frame::default();
        frame.spectrum[10] = -20.0;
        view.advance(&frame);
        let risen = view.spectrum[10] - FLOOR_DB;
        view.advance(&Frame::default());
        let fallen = (-20.0_f32.max(view.spectrum[10])) - FLOOR_DB;
        assert!(risen > 0.0);
        assert!(view.spectrum[10] > FLOOR_DB + 0.5 * risen, "fell too fast: {fallen}");
    }

    #[test]
    fn silence_decays_to_the_floor_and_a_cut_to_nothing() {
        let mut view = PlotView::default();
        let mut frame = Frame::default();
        frame.spectrum.fill(-30.0);
        frame.reduction[0].fill(6.0);
        for _ in 0..8 {
            view.advance(&frame);
        }
        for _ in 0..200 {
            view.idle();
        }
        assert!(view.spectrum.iter().all(|v| *v < FLOOR_DB + 0.1));
        assert!(view.reduction[0].iter().all(|v| *v < 0.01));
    }
}
