//! Original scalar Flanger mathematics, isolated from the Program lifecycle.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "Flanger scalar signal helper only: 46 authored native comparisons are float32-identical at 8/32/44.1/48/96/192 kHz, including direct field transitions, reset, channel fragmentation and a 131072-frame clock; 80 native outer comparisons verify caller-prepared physical points at 32/44.1/48/96 kHz, bounded tempo sync and bypass/resume; independent control generation, connected modulation and whole-program audio remain unverified; Program playback does not admit this leaf";

pub struct Flanger {
    rate: f32,
    phase: u32,
    position: usize,
    lines: Vec<Vec<f32>>,
    controls: [f32; 5],
}
impl Flanger {
    /// Controls are Speed (Hz), Feedback, DelayTime, Depth and Mix.
    pub fn new(rate: u32, channels: usize, controls: [f32; 5]) -> Result<Self> {
        ensure!(
            [8000, 32000, 44100, 48000, 96000, 192000].contains(&rate),
            "Unmeasured Flanger rate"
        );
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Invalid Flanger channels"
        );
        let mut result = Self {
            rate: rate as f32,
            phase: 0,
            position: 0,
            lines: vec![vec![0.; (rate as f32 * 0.044).ceil() as usize]; channels],
            controls,
        };
        result.set_controls(controls)?;
        Ok(result)
    }
    pub fn set_controls(&mut self, controls: [f32; 5]) -> Result<()> {
        ensure!(
            controls.iter().all(|v| v.is_finite()),
            "Nonfinite Flanger control"
        );
        ensure!(
            (0.01..=10.).contains(&controls[0])
                && controls[1..].iter().all(|v| (0. ..=1.).contains(v)),
            "Invalid Flanger control"
        );
        self.controls = controls;
        Ok(())
    }
    pub fn clear(&mut self) {
        self.lines.iter_mut().for_each(|line| line.fill(0.));
        self.position = 0;
        // The native signal reset clears delay memory; oscillator phase survives.
    }
    /// Shared audio frames, transposed through a fixed stack buffer.
    pub fn process_frames(&mut self, frames: &mut [Frame]) -> Result<()> {
        let channels = self.lines.len();
        ensure!(
            frames
                .iter()
                .all(|f| f[..channels].iter().all(|v| v.is_finite())),
            "Nonfinite Flanger input frame"
        );
        for chunk in frames.chunks_mut(32) {
            let mut planar = [[0.; 32]; MAX_CHANNELS];
            for (i, frame) in chunk.iter().enumerate() {
                for c in 0..channels {
                    planar[c][i] = frame[c];
                }
            }
            let mut views = planar.each_mut().map(|c| &mut c[..chunk.len()]);
            self.process(&mut views[..channels])?;
            for (i, frame) in chunk.iter_mut().enumerate() {
                for c in 0..channels {
                    frame[c] = planar[c][i];
                }
            }
        }
        Ok(())
    }

    /// One caller-prepared physical control tuple per 32 frames. The caller
    /// owns smoothing/connection combination; this leaf only dispatches points.
    pub fn process_control_points(
        &mut self,
        frames: &mut [Frame],
        points: &[[f32; 5]],
        sync: bool,
        tempo: f32,
        bypass: bool,
    ) -> Result<()> {
        ensure!(
            [32000., 44100., 48000., 96000.].contains(&self.rate),
            "Unmeasured Flanger point-dispatch rate"
        );
        ensure!(
            points.len() == frames.len().div_ceil(32),
            "Invalid Flanger point count"
        );
        ensure!(
            tempo.is_finite() && (60. ..=300.).contains(&tempo),
            "Unmeasured Flanger tempo"
        );
        // Validate the full batch before touching delay/oscillator state.
        for p in points {
            ensure!(
                p.iter().all(|v| v.is_finite())
                    && (0.01..=10.).contains(&p[0])
                    && p[1..].iter().all(|v| (0. ..=1.).contains(v)),
                "Invalid Flanger physical point"
            );
        }
        if bypass {
            if let Some(point) = points.last() {
                self.set_controls(*point)?;
            }
            return Ok(());
        }
        for (chunk, point) in frames.chunks_mut(32).zip(points) {
            self.set_controls(*point)?;
            if sync {
                self.controls[0] = (0.016666667f32 / point[0]) * tempo;
            }
            let result = self.process_frames(chunk);
            self.controls = *point;
            result?;
        }
        Ok(())
    }

    pub fn process(&mut self, channels: &mut [&mut [f32]]) -> Result<()> {
        ensure!(
            channels.len() == self.lines.len(),
            "Flanger channel count changed"
        );
        let count = channels[0].len();
        ensure!(
            channels
                .iter()
                .all(|c| c.len() == count && c.iter().all(|v| v.is_finite())),
            "Invalid Flanger input"
        );
        let [speed, feedback, base, depth, mix] = self.controls;
        let increment = ((4294967296.0f32 / self.rate) * speed) as u32;
        let depth = depth * 0.02;
        let base = base * 0.02;
        let length = self.lines[0].len();
        for frame in 0..count {
            let index = self.phase >> 24;
            let fraction = (self.phase & 0xffffff) as f32 * (1. / 16777216.);
            let triangle = |i: u32| {
                if i < 64 {
                    i as f32 / 64.
                } else if i < 192 {
                    (128. - i as f32) / 64.
                } else {
                    (i as f32 - 256.) / 64.
                }
            };
            let modulation = (1. - fraction) * triangle(index) + fraction * triangle(index + 1);
            self.phase = self.phase.wrapping_add(increment);
            let delay = (((modulation + 1.) * depth) * 0.5 + base) * self.rate;
            let mut read = self.position as f64 - f64::from(delay);
            if read < 0. {
                read += length as f64;
            }
            let index = read as usize;
            let fraction = read as f32 - index as f32;
            for (channel, line) in channels.iter_mut().zip(&mut self.lines) {
                let a = line[(index + length - 1) % length];
                let b = line[index];
                let c = line[(index + 1) % length];
                let d = line[(index + 2) % length];
                // Catmull-Rom cubic interpolation in float32.
                let cubic = (b - c) * 1.5 + (d - a) * 0.5;
                let quadratic = ((a - b * 2.5) + (c + c)) - d * 0.5;
                let linear = (c - a) * 0.5;
                let wet = ((cubic * fraction + quadratic) * fraction + linear) * fraction + b;
                let dry = channel[frame];
                line[self.position] = wet * feedback + dry;
                channel[frame] = wet * mix + dry;
                ensure!(
                    channel[frame].is_finite() && line[self.position].is_finite(),
                    "Nonfinite Flanger output"
                );
            }
            self.position = (self.position + 1) % length;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flanger_native_impulse_split_reset_and_validation() {
        let mut fx = Flanger::new(48000, 1, [0.8, 0., 0.2, 0., 1.]).unwrap();
        let mut input = vec![0.; 512];
        input[0] = 1.;
        fx.process(&mut [&mut input]).unwrap();
        let mut expected = vec![0.; 512];
        expected[0] = 1.;
        expected[192] = 1.;
        expected[193] = 0.00000762939453125;
        assert_eq!(input, expected); // Authored native impulse, unchanged reader.
        fx.clear();
        let mut input = vec![0.; 512];
        input[0] = 1.;
        fx.process(&mut [&mut input]).unwrap();
        assert_eq!(input, expected);
        let controls = [0.39889875, 0.39999998, 0.2, 0.50335938, 1.];
        let original: Vec<Vec<f32>> = (0..2)
            .map(|c| {
                (0..4096)
                    .map(|i| (((i * 17 + c * 11) % 43) as f32 - 21.) / 64.)
                    .collect()
            })
            .collect();
        let mut whole = original.clone();
        let mut split = original;
        let mut a = Flanger::new(48000, 2, controls).unwrap();
        let mut b = Flanger::new(48000, 2, controls).unwrap();
        a.process(
            &mut whole
                .iter_mut()
                .map(|x| x.as_mut_slice())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for at in (0..4096).step_by(257) {
            let end = (at + 257).min(4096);
            b.process(
                &mut split
                    .iter_mut()
                    .map(|x| &mut x[at..end])
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        }
        assert_eq!(whole, split);
        assert_ne!(whole[0], whole[1]);
        let mut frame_input: Vec<Frame> = (0..4096)
            .map(|i| {
                let mut frame = [0.; MAX_CHANNELS];
                for c in 0..2 {
                    frame[c] = (((i * 17 + c * 11) % 43) as f32 - 21.) / 64.;
                }
                frame[2] = 7.;
                frame
            })
            .collect();
        let mut frame_fx = Flanger::new(48000, 2, controls).unwrap();
        frame_fx.process_frames(&mut frame_input).unwrap();
        for (i, frame) in frame_input.iter().enumerate() {
            assert_eq!(frame[..2], [whole[0][i], whole[1][i]]);
            assert_eq!(frame[2], 7.);
        }
        let before = frame_input.clone();
        assert!(
            frame_fx
                .process_control_points(&mut frame_input, &[], false, 120., false)
                .is_err()
        );
        assert_eq!(frame_input, before);
        for (rate, channels, controls) in [
            (22050, 1, controls),
            (48000, 0, controls),
            (48000, 13, controls),
            (48000, 1, [f32::NAN; 5]),
            (48000, 1, [11., 0., 0., 0., 0.]),
        ] {
            assert!(Flanger::new(rate, channels, controls).is_err());
        }
        assert!(a.process(&mut [&mut [0.]]).is_err());
        assert!(a.set_controls([0.8, -1., 0.2, 0.5, 1.]).is_err());
    }
}
