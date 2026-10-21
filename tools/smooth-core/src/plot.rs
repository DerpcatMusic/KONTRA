//! What the engine shows: the plot's data, and the way it leaves the audio
//! thread.
//!
//! The reference's graph is the reduction and nothing else: the engine hands
//! it the gain it is applying to each channel -- linear, one for no
//! reduction -- on a [`PLOT_BINS`]-point grid, and the graph draws those
//! points as they come, a polyline of up to 1024 samples between the two
//! ends. The grid here is the reference's own axis, 20 Hz to 20 kHz. The gain crosses
//! threads in a [`Telemetry`] of atomics -- no lock, no allocation -- and the
//! editor copies a [`Frame`] out of it when it draws.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// How many points the plot carries: the reference's polyline cap.
pub const PLOT_BINS: usize = 1024;

/// The sweep's ends, in hertz.
pub const SWEEP_LOW: f32 = 20.0;
pub const SWEEP_HIGH: f32 = 20_000.0;

/// The reference's axis is `ln(f + 150)`, laid out evenly from 170 to 20150:
/// a log axis whose low end is eased, so 20 Hz is the left edge and 20 kHz the
/// right. The two ends are `ln(20 + 150)` and `ln(20000 + 150)`.
pub const AXIS_OFFSET: f32 = 150.0;
pub const AXIS_LOW: f32 = SWEEP_LOW + AXIS_OFFSET;
pub const AXIS_HIGH: f32 = SWEEP_HIGH + AXIS_OFFSET;

/// Where a frequency sits along the axis, zero at the left and one at the right.
pub fn axis_position(frequency: f32) -> f32 {
    let frequency = frequency.clamp(SWEEP_LOW, SWEEP_HIGH);
    ((frequency + AXIS_OFFSET) / AXIS_LOW).ln() / (AXIS_HIGH / AXIS_LOW).ln()
}

/// The frequency at a place along the axis.
pub fn axis_frequency(position: f32) -> f32 {
    AXIS_LOW * (AXIS_HIGH / AXIS_LOW).powf(position.clamp(0.0, 1.0)) - AXIS_OFFSET
}

/// The frequency a plot point stands for: the points are even along the axis.
pub fn plot_frequency(index: usize) -> f32 {
    axis_frequency(index as f32 / (PLOT_BINS - 1) as f32)
}

/// One frame of what the engine is doing.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The gain each channel is taking, linear: one is no reduction.
    pub gain: [[f32; PLOT_BINS]; 2],
    /// The sidechain meter: the largest signed detector sample in the window
    /// this frame covers. Signed, not smoothed, and zero with no window.
    pub detector: f32,
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            gain: [[1.0; PLOT_BINS]; 2],
            detector: 0.0,
        }
    }
}

/// The audio thread's side of the plot: written with relaxed atomics, read by
/// the editor. A frame may be torn across a publish; the next one repairs it.
#[derive(Debug)]
pub struct Telemetry {
    gain: [[AtomicU32; PLOT_BINS]; 2],
    detector: AtomicU32,
    sequence: AtomicU64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            gain: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(1.0_f32.to_bits()))),
            detector: AtomicU32::new(0.0_f32.to_bits()),
            sequence: AtomicU64::new(0),
        }
    }
}

impl Telemetry {
    /// Publish one frame. Called from the audio thread.
    pub fn publish(&self, frame: &Frame) {
        for (slots, values) in self.gain.iter().zip(&frame.gain) {
            for (slot, value) in slots.iter().zip(values) {
                slot.store(value.to_bits(), Ordering::Relaxed);
            }
        }
        self.detector.store(frame.detector.to_bits(), Ordering::Relaxed);
        self.sequence.fetch_add(1, Ordering::Release);
    }

    /// How many frames have been published: it moves when there is
    /// something new to draw.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::Acquire)
    }

    /// The latest frame.
    pub fn read(&self) -> Frame {
        Frame {
            gain: std::array::from_fn(|c| {
                std::array::from_fn(|i| f32::from_bits(self.gain[c][i].load(Ordering::Relaxed)))
            }),
            detector: f32::from_bits(self.detector.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{Engine, EngineConfig, Settings};

    #[test]
    fn the_engine_s_frame_is_unity_at_rest_and_never_a_boost() {
        let mut engine = Engine::new(48_000.0, EngineConfig::default()).unwrap();
        let rest = engine.plot_frame();
        assert!(rest.gain.iter().flatten().all(|g| (*g - 1.0).abs() < 1.0e-6));
        let settings = Settings::default();
        for t in 0..30_000 {
            let s = (t as f32 * 2.0 * std::f32::consts::PI * 1000.0 / 48_000.0).sin() * 0.5;
            engine.process_sample([s, s], &settings);
        }
        let frame = engine.plot_frame();
        assert!(frame.gain.iter().flatten().all(|g| g.is_finite() && *g > 0.0 && *g <= 1.0001));
    }

    #[test]
    fn the_sidechain_meter_is_the_signed_peak_of_each_window() {
        let mut engine = Engine::new(48_000.0, EngineConfig::default()).unwrap();
        let settings = Settings::default();
        // A window of a negative constant reads as negative: the peak is signed.
        for _ in 0..512 {
            engine.process_sample([-0.25, -0.25], &settings);
        }
        let first = engine.plot_frame();
        assert_eq!(first.detector, -0.25);
        // The next window starts empty, so an idle window reads zero, not the old peak.
        assert_eq!(engine.plot_frame().detector, 0.0);
    }

    #[test]
    fn the_axis_runs_from_twenty_hertz_to_twenty_kilohertz() {
        assert!((plot_frequency(0) - 20.0).abs() < 0.01);
        assert!((plot_frequency(PLOT_BINS - 1) - 20_000.0).abs() < 1.0);
        assert!((axis_position(axis_frequency(0.37)) - 0.37).abs() < 1.0e-5);
    }
}
