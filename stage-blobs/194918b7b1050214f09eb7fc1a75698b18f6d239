//! Original rational saturation for the measured Drive Mode=0, Oversampling=0
//! boundary. This is not registered with Program playback.
//! Official controls: https://lua.uvi.net/_elements.html#drive
//! Authored native outer-wrapper comparisons are documented in
//! docs/UVI_DRIVE_SCALAR_EVIDENCE.md.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{ensure, Result};

pub const FIDELITY_DIAGNOSTIC: &str = "Drive Mode=0/Oversampling=0 scalar boundary only; 1152 authored native outer-wrapper comparisons at 8/32/44.1/48/96/192 kHz and 1/2/6/12 channels are float32-identical, including static DriveAmount edits and dry bypass, with no replacement arithmetic hooks. Hosted smoothing, internal Drive ramps, other modes, oversampling, voice cloning and whole-program audio remain unverified; Program playback does not admit this leaf";

pub struct DriveScalar {
    channels: usize,
    coefficient: f32,
}

impl DriveScalar {
    pub fn new(channels: usize, rate: f64, mode: i32, oversampling: i32) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Drive needs 1..12 channels"
        );
        ensure!(
            [8_000., 32_000., 44_100., 48_000., 96_000., 192_000.].contains(&rate),
            "Drive scalar helper rate has no authored native comparison"
        );
        ensure!(
            mode == 0 && oversampling == 0,
            "Drive scalar helper only proves Mode=0/Oversampling=0"
        );
        Ok(Self {
            channels,
            coefficient: 0.,
        })
    }

    /// Direct static DriveAmount boundary; no hosted smoothing or internal ramp.
    pub fn configure(&mut self, drive: f64) -> Result<()> {
        ensure!(
            drive.is_finite() && (0.0..=1.0).contains(&drive),
            "Invalid DriveAmount"
        );
        let amount = (drive as f32) * 0.95;
        let ratio = amount / (1. - amount);
        self.coefficient = ratio + ratio;
        Ok(())
    }

    /// This stateless boundary passes a bypassed buffer through unchanged.
    pub fn process(&self, frames: &mut [Frame], bypass: bool) -> Result<()> {
        ensure!(
            frames
                .iter()
                .all(|f| f[..self.channels].iter().all(|x| x.is_finite())),
            "Nonfinite Drive input"
        );
        if bypass {
            return Ok(());
        }
        let gain = self.coefficient + 1.;
        for frame in frames {
            for sample in &mut frame[..self.channels] {
                *sample = (*sample / (sample.abs() * self.coefficient + 1.)) * gain;
                ensure!(sample.is_finite(), "Nonfinite Drive output");
            }
        }
        Ok(())
    }
}
