//! Measured MultiLFO endpoint candidate. It is deliberately not admitted by ModGraph.
//! The caller owns event timing, random-seed lifecycle and connected Freq/Depth points.

pub const FIDELITY_DIAGNOSTIC: &str = "MultiLFO is not executable by this renderer: the isolated Sine+Noise endpoint helper has measured clock, seeded retrigger and global-context event behavior, but connected Freq/Depth clocks, hosted event/RNG ordering, other waveforms and whole-program audio remain unverified";

#[derive(Clone, Copy)]
pub struct Settings {
    pub rate: f32,
    pub frequency: f32,
    pub depth: f32,
    pub smooth: f32,
    pub noise_depth: f32,
}

impl Settings {
    fn valid(self) -> bool {
        [32000., 44100., 48000., 96000.].contains(&self.rate)
            && self.frequency.is_finite()
            && (0.01..=20.).contains(&self.frequency)
            && self.depth.is_finite()
            && (0.0..=1.).contains(&self.depth)
            && self.smooth.is_finite()
            && (0.0..=1.).contains(&self.smooth)
            && self.noise_depth.is_finite()
            && (0.0..=1.).contains(&self.noise_depth)
    }
}

// Standard MT19937, canonical double from two uint32 draws (low draw first).
struct Random {
    words: [u32; 624],
    index: usize,
}
impl Random {
    fn new(seed: u32) -> Self {
        let mut words = [0; 624];
        words[0] = seed;
        for i in 1..624 {
            let previous = words[i - 1];
            words[i] = 1812433253u32
                .wrapping_mul(previous ^ (previous >> 30))
                .wrapping_add(i as u32);
        }
        Self { words, index: 624 }
    }
    fn uint(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let bits = (self.words[i] & 0x80000000) | (self.words[(i + 1) % 624] & 0x7fffffff);
                self.words[i] = self.words[(i + 397) % 624]
                    ^ (bits >> 1)
                    ^ if bits & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut bits = self.words[self.index];
        self.index += 1;
        bits ^= bits >> 11;
        bits ^= (bits << 7) & 0x9d2c5680;
        bits ^= (bits << 15) & 0xefc60000;
        bits ^= bits >> 18;
        bits
    }
    fn noise(&mut self) -> f64 {
        let low = self.uint() as f64;
        let high = self.uint() as f64;
        ((low + high * 4294967296.) / 18446744073709551616.) * 2. - 1.
    }
}

/// SineDepth=1, Triangle/Saw/Square=0, NormalizeOutput/Bipolar=true,
/// SyncToHost/Invert=false and RiseTime=0. Endpoints are float32; phase/math are double.
pub struct MultiLfo {
    random: Random,
    noise: [f64; 2],
    phase: f64,
    smoothed: f64,
    raw: f64,
    last: f32,
    pending_retrigger: bool,
}
impl Default for MultiLfo {
    fn default() -> Self {
        Self::new()
    }
}
impl MultiLfo {
    pub fn new() -> Self {
        let mut random = Random::new(5489);
        let noise = [random.noise(), random.noise()];
        Self {
            random,
            noise,
            phase: 0.,
            smoothed: 0.,
            raw: 0.,
            last: 0.,
            pending_retrigger: false,
        }
    }
    /// Explicit MT seed, already supplied by the caller's native-compatible event/RNG lifecycle.
    pub fn retrigger(&mut self, seed: u32, phase: f32) -> bool {
        if !phase.is_finite() || !(0.0..=1.0).contains(&phase) {
            return false;
        }
        self.random = Random::new(seed);
        self.noise = [self.random.noise(), self.random.noise()];
        self.phase = phase as f64;
        self.smoothed = 0.;
        self.raw = 0.;
        self.last = 0.;
        self.pending_retrigger = true;
        true
    }
    fn waveform(&self, noise_depth: f64) -> f64 {
        (self.noise[usize::from(self.phase >= 0.5)] * noise_depth
            + (self.phase * std::f64::consts::TAU).sin())
            * (1. / (1. + noise_depth))
    }
    /// Emits ceil(frames/32)+1 endpoints into caller storage; last point is padded to 32.
    /// Persistent phase and smoothing advance by the real frame count, including partial intervals.
    pub fn process(&mut self, frames: usize, settings: Settings, points: &mut [f32]) -> bool {
        if frames == 0
            || frames > 8192
            || !settings.valid()
            || points.len() != frames.div_ceil(32) + 1
        {
            return false;
        }
        let delta = 1. / (settings.rate as f64 / settings.frequency as f64);
        let smooth_samples = settings.smooth as f64 * settings.rate as f64;
        let noise_depth = settings.noise_depth as f64;
        if self.pending_retrigger {
            self.raw = self.waveform(noise_depth);
            self.last = if smooth_samples == 0. {
                (self.raw * settings.depth as f64) as f32
            } else {
                0.
            };
            self.pending_retrigger = false;
        }
        let mut remaining = frames;
        let mut step = 0;
        for point in &mut points[..frames.div_ceil(32)] {
            *point = self.last;
            step = remaining.min(32);
            remaining -= step;
            self.phase += step as f64 * delta;
            if self.phase >= 1. {
                self.phase %= 1.;
                self.noise = [self.random.noise(), self.random.noise()];
            }
            let alpha = if smooth_samples == 0. {
                1.
            } else {
                (step as f64 / smooth_samples).min(1.)
            };
            self.smoothed += alpha * (self.raw - self.smoothed);
            self.raw = self.waveform(noise_depth);
            self.last = ((if smooth_samples == 0. {
                self.raw
            } else {
                self.smoothed
            }) * settings.depth as f64) as f32;
        }
        let end = points.len() - 1;
        points[end] = ((self.last - points[end - 1]) * 32.) / step as f32 + points[end - 1];
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_endpoint_vector_and_invalid_call_preserves_state() {
        let settings = Settings {
            rate: 48000.,
            frequency: 6.0631094,
            depth: 1.,
            smooth: 0.099999987,
            noise_depth: 0.64999998,
        };
        let mut source = MultiLfo::new();
        let mut beyond_measured = [0.; 258];
        assert!(!source.process(8193, settings, &mut beyond_measured));
        assert!(!source.process(usize::MAX, settings, &mut []));
        let mut wrong_shape = [0.; 8];
        assert!(!source.process(256, settings, &mut wrong_shape));
        assert!(!source.retrigger(1, f32::NAN));
        let mut points = [0.; 9];
        assert!(source.process(256, settings, &mut points));
        let native = [
            0.,
            0.,
            -0.0018120629247277975,
            -0.0035095082130283117,
            -0.00509323226287961,
            -0.006564191076904535,
            -0.007923400960862637,
            -0.009171937592327595,
            -0.01031093392521143,
        ];
        assert_eq!(points, native);
    }
}
