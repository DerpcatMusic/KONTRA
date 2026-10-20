//! What the engine shows: the plot's data, and the way it leaves the audio
//! thread.
//!
//! The reference's graph is the reduction and nothing else: the engine hands
//! it the gain it is applying to each channel -- linear, one for no
//! reduction -- on a [`PLOT_BINS`]-point grid, and the graph draws those
//! points as they come, a polyline of up to 1024 samples between the two
//! ends. The grid here is log-spaced from 20 Hz to 20 kHz. The gain crosses
//! threads in a [`Telemetry`] of atomics -- no lock, no allocation -- and the
//! editor copies a [`Frame`] out of it when it draws. The depth curve is not
//! telemetry: it is a pure function of the settings, so the editor asks for
//! it with [`depth_curves`] and it is there whether or not audio is running.

use crate::dsp::Settings;
use crate::weighting::WeightBanks;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// How many points the plot carries: the reference's polyline cap.
pub const PLOT_BINS: usize = 1024;

/// The sweep's ends, in hertz.
pub const SWEEP_LOW: f32 = 20.0;
pub const SWEEP_HIGH: f32 = 20_000.0;

/// The frequency a plot point stands for.
pub fn plot_frequency(index: usize) -> f32 {
    SWEEP_LOW * (SWEEP_HIGH / SWEEP_LOW).powf(index as f32 / (PLOT_BINS - 1) as f32)
}

/// One frame of what the engine is doing.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The gain each channel is taking, linear: one is no reduction.
    pub gain: [[f32; PLOT_BINS]; 2],
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            gain: [[1.0; PLOT_BINS]; 2],
        }
    }
}

/// The audio thread's side of the plot: written with relaxed atomics, read by
/// the editor. A frame may be torn across a publish; the next one repairs it.
#[derive(Debug)]
pub struct Telemetry {
    gain: [[AtomicU32; PLOT_BINS]; 2],
    sequence: AtomicU64,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            gain: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(1.0_f32.to_bits()))),
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
        }
    }
}

/// The depth curve of each channel, in decibels: what the bands ask of the
/// reduction at each frequency. Positive is deeper.
///
/// This is the engine's own weighting -- the same `Wb` bank the detector is
/// multiplied by -- evaluated on the plot's sweep, so the curve on screen is
/// the curve the engine uses, not a picture of it. `None` if the settings
/// are outside what the weighting accepts.
pub fn depth_curves(settings: &Settings) -> Option<[[f32; PLOT_BINS]; 2]> {
    let frequencies: Vec<f32> = (0..PLOT_BINS).map(plot_frequency).collect();
    let mut banks = WeightBanks::new(&frequencies);
    let count = settings.weight_group_count.min(settings.weight_groups.len());
    banks
        .build(
            &frequencies,
            &settings.bands,
            settings.hard,
            2,
            &settings.weight_groups[..count],
            settings.weight_stereo_mode,
            settings.spatial_curve_scalar,
        )
        .ok()?;
    Some(std::array::from_fn(|c| {
        std::array::from_fn(|i| 20.0 * banks.wb[c][i].max(1.0e-6).log10())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::{Engine, EngineConfig};

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

    use crate::weighting::{BandRecord, NormalizedRecord, Shape, normalized_from_center, normalized_from_q, normalized_from_slope};

    fn bell(frequency: f32, depth_db: f32) -> BandRecord {
        BandRecord::from_normalized(NormalizedRecord {
            used: 1.0,
            enabled: 1.0,
            frequency: normalized_from_center(frequency),
            depth: (depth_db + 12.0) / 24.0,
            q: normalized_from_q(0.707),
            slope: normalized_from_slope(6.0),
            focus: 0.5,
            selector: 0.0,
            shape: 2.0 / 7.0,
        })
        .unwrap()
    }

    #[test]
    fn a_flat_set_of_bands_is_a_flat_depth_curve() {
        let curves = depth_curves(&Settings::default()).expect("the default bands");
        assert!(curves[0].iter().all(|v| v.abs() < 1.0e-3), "{:?}", &curves[0][..8]);
    }

    #[test]
    fn a_bell_peaks_at_its_own_centre_and_dies_away_from_it() {
        let mut settings = Settings::default();
        settings.bands = std::array::from_fn(|_| {
            let mut record = BandRecord::flat(1000.0);
            record.enabled = false;
            record
        });
        let mut record = bell(1000.0, 6.0);
        assert_eq!(record.shape, Shape::Bell);
        record.enabled = true;
        settings.bands[0] = record;
        let curve = depth_curves(&settings).expect("one bell")[0];
        let (peak, _) = curve
            .iter()
            .enumerate()
            .fold((0, f32::MIN), |best, (i, &v)| if v > best.1 { (i, v) } else { best });
        let centre = plot_frequency(peak);
        assert!((centre / 1000.0).log2().abs() < 0.2, "peak at {centre} Hz");
        eprintln!("PEAK {} dB for a 6 dB bell, hard={}", curve[peak], settings.hard);
        assert!(curve[peak] > 1.0, "peak {} dB", curve[peak]);
        assert!(curve[0].abs() < 0.2 && curve[PLOT_BINS - 1].abs() < 0.2);
    }
}
