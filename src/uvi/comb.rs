//! Original feedback-comb kernel, independently compared with authored native
//! UVI Workstation 4.0.9 impulses. Parameters: https://lua.uvi.net/_elements.html
//! Plus/Minus topology: official Falcon manual, effect appendix, Comb Filter.
//!
//! This kernel is deliberately not admitted by playback preflight: native
//! connected-source startup and matrix/control generation remain unverified.
use super::dsp::{Frame, MAX_CHANNELS};
use anyhow::{Result, ensure};

pub const FIDELITY_DIAGNOSTIC: &str = "CombFilter audio and physical control-point dispatch are native-verified at 32/44.1/48/96 kHz; connected-source startup and matrix/control generation remain unverified";

/// The measured audio kernel and physical control-point dispatcher. This does not implement the
/// surrounding Falcon control-variable lifecycle.
pub struct CombKernel {
    channels: usize,
    rate: f32,
    mode: u32,
    ring: Vec<f32>,
    length: usize,
    cursor: usize,
    delay: usize,
    fraction: f32,
    feedback: f32,
}
impl CombKernel {
    pub fn new(channels: usize, rate: f64, freq: f64, q: f64, mode: u32) -> Result<Self> {
        ensure!(
            (1..=MAX_CHANNELS).contains(&channels),
            "Invalid CombFilter channel count"
        );
        ensure!(
            [32_000., 44_100., 48_000., 96_000.].contains(&rate),
            "Unmeasured CombFilter sample rate"
        );
        ensure!(
            freq.is_finite() && (20. ..=20_000.).contains(&freq),
            "Invalid CombFilter frequency"
        );
        ensure!(
            q.is_finite() && (0. ..=1.).contains(&q),
            "Invalid CombFilter resonance"
        );
        ensure!(mode <= 1, "Invalid CombFilter mode");
        let frequency = (freq as f32).min(rate as f32 * 0.5);
        // Period is 1/f seconds. Native scalar measurements establish the
        // half-period reciprocal's float rounding before doubling.
        let period = (1. / ((frequency + frequency) / rate as f32)) * 2.;
        let delay = period as usize;
        // Native history capacity supports later edits down to 20 Hz.
        let length = ((rate * 0.05).ceil() as usize + 513).next_power_of_two();
        Ok(Self {
            channels,
            rate: rate as f32,
            mode,
            ring: vec![0.; channels * length],
            length,
            cursor: 0,
            delay,
            fraction: period - delay as f32,
            feedback: q as f32 * if mode == 0 { 1. } else { -1. },
        })
    }
    pub fn process(&mut self, frames: &mut [Frame]) {
        let mask = self.length - 1;
        for frame in frames {
            let newer = self.cursor.wrapping_sub(self.delay) & mask;
            let older = newer.wrapping_sub(1) & mask;
            for (channel, x) in frame[..self.channels].iter_mut().enumerate() {
                let buffer = &mut self.ring[channel * self.length..(channel + 1) * self.length];
                let delayed = buffer[newer] + (buffer[older] - buffer[newer]) * self.fraction;
                *x += delayed * self.feedback;
                buffer[self.cursor] = *x;
            }
            self.cursor = (self.cursor + 1) & mask;
        }
    }
    /// Apply measured physical Freq/Q points held for each 32-frame span.
    /// This consumes the surrounding controller's output; it does not generate
    /// matrix sources or smoothing. Bypass updates controls while freezing audio.
    pub fn process_control_points(
        &mut self,
        frames: &mut [Frame],
        freq: &[f32],
        q: &[f32],
        bypass: bool,
    ) -> Result<()> {
        let count = frames.len().div_ceil(32);
        ensure!(
            freq.len() == count && q.len() == count,
            "Invalid CombFilter control-point count"
        );
        ensure!(
            freq.iter()
                .all(|v| v.is_finite() && (20. ..=20_000.).contains(v)),
            "Invalid CombFilter Freq points"
        );
        ensure!(
            q.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)),
            "Invalid CombFilter Q points"
        );
        if bypass {
            if let Some(index) = count.checked_sub(1) {
                self.configure(freq[index], q[index]);
            }
            return Ok(());
        }
        for ((chunk, freq), q) in frames.chunks_mut(32).zip(freq).zip(q) {
            self.configure(*freq, *q);
            self.process(chunk);
        }
        Ok(())
    }
    fn configure(&mut self, freq: f32, q: f32) {
        let frequency = freq.min(self.rate * 0.5);
        let period = (1. / ((frequency + frequency) / self.rate)) * 2.;
        self.delay = period as usize;
        self.fraction = period - self.delay as f32;
        self.feedback = q * if self.mode == 0 { 1. } else { -1. };
    }
    pub fn clear(&mut self) {
        self.ring.fill(0.);
        self.cursor = 0;
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.ring.capacity() * std::mem::size_of::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_native_scalar_impulses_and_fragmented_channels() {
        // Authored outputs from native execution, not vendor audio or code.
        for &(rate, freq, q, mode, taps) in FIXTURES {
            let mut kernel = CombKernel::new(12, rate, freq, q, mode).unwrap();
            let mut frames = vec![[0.; MAX_CHANNELS]; 4096];
            for c in 0..12 {
                frames[c][c] = 1.;
            }
            for range in [0..1, 1..32, 32..49, 49..305, 305..4096] {
                kernel.process(&mut frames[range]);
            }
            for &(index, bits) in taps {
                for c in 0..12 {
                    assert_eq!(
                        frames[index + c][c].to_bits(),
                        bits,
                        "rate={rate},freq={freq},q={q},mode={mode},index={index},channel={c}"
                    );
                }
            }
            kernel.clear();
            let mut silence = [[0.; MAX_CHANNELS]; 32];
            kernel.process(&mut silence);
            assert_eq!(silence, [[0.; MAX_CHANNELS]; 32]);
        }
        assert!(CombKernel::new(1, 88_200., 1000., 0.5, 0).is_err());
        assert!(CombKernel::new(0, 48_000., 1000., 0.5, 0).is_err());
        assert!(CombKernel::new(1, 48_000., f64::NAN, 0.5, 0).is_err());
        assert!(CombKernel::new(1, 48_000., 1000., 1.1, 0).is_err());
        assert!(CombKernel::new(1, 48_000., 1000., 0.5, 2).is_err());
    }
    #[test]
    fn native_delay_growth_after_bypassed_edit_preserves_history() {
        for &(rate, mode, taps) in DYNAMIC_FIXTURES {
            let mut kernel = CombKernel::new(1, rate, 1008.4754, 0.93642187, mode).unwrap();
            let mut warm = vec![[0.; MAX_CHANNELS]; 128];
            warm[0][0] = 1.;
            kernel
                .process_control_points(&mut warm, &[1008.4754; 4], &[0.93642187; 4], false)
                .unwrap();
            let mut dry = [[0.125; MAX_CHANNELS]; 64];
            kernel
                .process_control_points(&mut dry, &[20.; 2], &[0.93642187; 2], true)
                .unwrap();
            assert_eq!(dry, [[0.125; MAX_CHANNELS]; 64]);
            let mut resumed = vec![[0.; MAX_CHANNELS]; 5000];
            let count = resumed.len().div_ceil(32);
            kernel
                .process_control_points(
                    &mut resumed,
                    &vec![20.; count],
                    &vec![0.93642187; count],
                    false,
                )
                .unwrap();
            for &(index, bits) in taps {
                assert_eq!(
                    resumed[index][0].to_bits(),
                    bits,
                    "rate={rate},mode={mode},index={index}"
                );
            }
            let before = kernel.memory_bytes();
            assert!(
                kernel
                    .process_control_points(&mut resumed[..32], &[f32::NAN], &[0.5], false)
                    .is_err()
            );
            assert_eq!(kernel.memory_bytes(), before);
        }
    }
    type DynamicFixture = (f64, u32, &'static [(usize, u32)]);
    #[rustfmt::skip]
    const DYNAMIC_FIXTURES: &[DynamicFixture] = &[
        (32000.0, 0, &[(1472, 0x3f6fb958), (1503, 0x3e717c05), (1504, 0x3f241c97), (1534, 0x3d734200), (1535, 0x3ea5511d), (1536, 0x3ee0b275), (1565, 0x3c750b53), (1566, 0x3df9cbdd)]),
        (32000.0, 1, &[(1472, 0xbf6fb958), (1503, 0x3e717c05), (1504, 0x3f241c97), (1534, 0xbd734200), (1535, 0xbea5511d), (1536, 0xbee0b275), (1565, 0x3c750b53), (1566, 0x3df9cbdd)]),
        (44100.0, 0, &[(2077, 0x3f6fb958), (2120, 0x3e7300eb), (2121, 0x3f23bb5e), (2163, 0x3d7653fb), (2164, 0x3ea5f8cd), (2165, 0x3edfa889), (4282, 0x3f607b99), (4325, 0x3e638dcb)]),
        (44100.0, 1, &[(2077, 0xbf6fb958), (2120, 0x3e7300eb), (2121, 0x3f23bb5e), (2163, 0xbd7653fb), (2164, 0xbea5f8cd), (2165, 0xbedfa889), (4282, 0x3f607b99), (4325, 0xbe638dcb)]),
        (48000.0, 0, &[(2272, 0x3f6fb958), (2319, 0x3eb51c78), (2320, 0x3f05ed5d), (2366, 0x3e08d44d), (2367, 0x3eca5d1b), (2368, 0x3e95a493), (4672, 0x3f607b99), (4719, 0x3ea998b4)]),
        (48000.0, 1, &[(2272, 0xbf6fb958), (2319, 0x3eb51c78), (2320, 0x3f05ed5d), (2366, 0xbe08d44d), (2367, 0xbeca5d1b), (2368, 0xbe95a493), (4672, 0x3f607b99), (4719, 0xbea998b4)]),
        (96000.0, 0, &[(4672, 0x3f6fb958), (4767, 0x3f351c78), (4768, 0x3e2d7c85)]),
        (96000.0, 1, &[(4672, 0xbf6fb958), (4767, 0x3f351c78), (4768, 0x3e2d7c85)]),
    ];
    type Fixture = (f64, f64, f64, u32, &'static [(usize, u32)]);
    #[rustfmt::skip]
    const FIXTURES: &[Fixture] = &[
        (32000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (801, 0x3dc08d3f), (802, 0x3f57a7b0), (1602, 0x3c10d429)]),
        (32000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (801, 0xbdc08d3f), (802, 0xbf57a7b0), (1602, 0x3c10d429)]),
        (32000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (31, 0x3e80f0a3), (32, 0x3f2f4106), (62, 0x3d81e30a)]),
        (32000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (31, 0xbe80f0a3), (32, 0xbf2f4106), (62, 0x3d81e30a)]),
        (32000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f6fb958), (4, 0x3f607b99), (6, 0x3f5235ea)]),
        (32000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf6fb958), (4, 0x3f607b99), (6, 0xbf5235ea)]),
        (44100.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (1105, 0x3f537c2b), (1106, 0x3de1e96a), (2210, 0x3f2eb5ea)]),
        (44100.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (1105, 0xbf537c2b), (1106, 0xbde1e96a), (2210, 0x3f2eb5ea)]),
        (44100.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (43, 0x3e81c04a), (44, 0x3f2ed933), (86, 0x3d8386b6)]),
        (44100.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (43, 0xbe81c04a), (44, 0xbf2ed933), (86, 0x3d8386b6)]),
        (44100.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f3e949e), (3, 0x3e4492e7), (4, 0x3f0de0f1)]),
        (44100.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf3e949e), (3, 0xbe4492e7), (4, 0x3f0de0f1)]),
        (48000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (1202, 0x3e10716d), (1203, 0x3f4b9cfd), (2404, 0x3ca2ff9a)]),
        (48000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (1202, 0xbe10716d), (1203, 0xbf4b9cfd), (2404, 0x3ca2ff9a)]),
        (48000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (47, 0x3ec1685f), (48, 0x3f0f0529), (94, 0x3e121e8a)]),
        (48000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (47, 0xbec1685f), (48, 0xbf0f0529), (94, 0x3e121e8a)]),
        (48000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (2, 0x3f0fd59a), (3, 0x3ebfc77d), (4, 0x3ea1a0a8)]),
        (48000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (2, 0xbf0fd59a), (3, 0xbebfc77d), (4, 0x3ea1a0a8)]),
        (96000.0, 39.905247, 0.93642187, 0, &[(0, 0x3f800000), (2405, 0x3e90716d), (2406, 0x3f2780a1)]),
        (96000.0, 39.905247, 0.93642187, 1, &[(0, 0x3f800000), (2405, 0xbe90716d), (2406, 0xbf2780a1)]),
        (96000.0, 1008.4754, 0.93642187, 0, &[(0, 0x3f800000), (95, 0x3f41685f), (96, 0x3e3943e5), (190, 0x3f121e8a)]),
        (96000.0, 1008.4754, 0.93642187, 1, &[(0, 0x3f800000), (95, 0xbf41685f), (96, 0xbe3943e5), (190, 0x3f121e8a)]),
        (96000.0, 20000.0, 0.93642187, 0, &[(0, 0x3f800000), (4, 0x3e3fc76e), (5, 0x3f3fc77d), (8, 0x3d0fab33)]),
        (96000.0, 20000.0, 0.93642187, 1, &[(0, 0x3f800000), (4, 0xbe3fc76e), (5, 0xbf3fc77d), (8, 0x3d0fab33)]),
    ];
}
