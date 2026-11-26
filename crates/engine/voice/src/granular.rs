//! Real-time, zero-allocation Hann-windowed overlap-add granular voice engine
//! (`kontra_ir::Generator::Granular`).
//!
//! Uses a fixed-size inline grain pool ([`MAX_GRAINS`]) and reads directly from
//! a [`Region`]'s resident/streamed [`kontra_stream::Sample`] pages via
//! caller-provided scratch buffers, allocating nothing on the audio thread.

use kontra_stream::{Decoded, Mix, StreamClient};

use crate::zone::Region;

/// Maximum simultaneously active grains per voice (inline, zero allocation).
pub const MAX_GRAINS: usize = 16;

/// Declarative granular synthesis parameters prepared per group.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GranularSpec {
    /// Grain duration in milliseconds (clamped to `>= 2.0`).
    pub grain_ms: f32,
    /// Grain spawn density in grains per second (clamped to `0.1..=200.0`).
    pub density: f32,
    /// Normalized cloud center within `region.start..region.end` (`0.0..=1.0`).
    pub position: f32,
    /// Additional grain pitch shift in semitones on top of the note's pitch.
    pub pitch_semitones: f32,
    /// Stereo pan and subtle position scatter amount (`0.0..=1.0`).
    pub spread: f32,
}

#[derive(Clone, Copy, Debug)]
struct Grain {
    active: bool,
    /// Output-frame delay within the current block before this grain starts.
    delay: u32,
    /// Current fractional source frame in `region.start..region.end`.
    pos: f64,
    /// Frames already emitted by this grain.
    age: u32,
    /// Total duration of this grain in output frames (`>= 4`).
    len: u32,
    /// Equal-power stereo pan weights.
    pan_l: f32,
    pan_r: f32,
}

impl Grain {
    const EMPTY: Self = Self {
        active: false,
        delay: 0,
        pos: 0.0,
        age: 0,
        len: 4,
        pan_l: 1.0,
        pan_r: 1.0,
    };
}

/// Per-voice real-time granular synthesizer state (inline, zero heap allocation).
#[derive(Clone, Copy, Debug)]
pub struct Granular {
    spec: GranularSpec,
    grains: [Grain; MAX_GRAINS],
    spawn_accum: f32,
    rng: u64,
    output_rate: f32,
}

impl Granular {
    /// Creates a granular generator ready to fire its first grain immediately.
    #[must_use]
    pub fn new(spec: GranularSpec, output_rate: u32) -> Self {
        let rate = output_rate.max(1) as f32;
        let seed = (spec.position.to_bits() as u64)
            ^ ((spec.grain_ms.to_bits() as u64).rotate_left(17))
            ^ ((spec.density.to_bits() as u64).rotate_left(33))
            ^ 0x9E37_79B9_7F4A_7C15;
        Self {
            spec: GranularSpec {
                grain_ms: if spec.grain_ms.is_finite() {
                    spec.grain_ms.max(2.0)
                } else {
                    50.0
                },
                density: if spec.density.is_finite() {
                    spec.density.clamp(0.1, 200.0)
                } else {
                    12.0
                },
                position: if spec.position.is_finite() {
                    spec.position.clamp(0.0, 1.0)
                } else {
                    0.0
                },
                pitch_semitones: if spec.pitch_semitones.is_finite() {
                    spec.pitch_semitones.clamp(-48.0, 48.0)
                } else {
                    0.0
                },
                spread: if spec.spread.is_finite() {
                    spec.spread.clamp(0.0, 1.0)
                } else {
                    0.0
                },
            },
            grains: [Grain::EMPTY; MAX_GRAINS],
            // Fire the first grain on frame 0 so notes have zero onset latency.
            spawn_accum: 1.0,
            rng: if seed == 0 { 1 } else { seed },
            output_rate: rate,
        }
    }

    #[inline]
    fn next_unit_f32(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        // 24-bit mantissa uniform in [0, 1).
        ((x >> 40) as u32) as f32 * (1.0 / 16_777_216.0)
    }

    /// Renders `out.len()` stereo frames into `out`, returning the number of
    /// missing (non-resident) source frames encountered.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        region: &Region,
        client: &StreamClient,
        mix: Option<&Mix>,
        decoded: &mut Decoded,
        scratch_win: &mut [[f32; 2]],
        step: f64,
        out: &mut [[f32; 2]],
    ) -> u32 {
        out.fill([0.0; 2]);
        let start_f = region.start;
        let end_f = region.end.min(region.sample.frames());
        let region_len = end_f.saturating_sub(start_f);
        if region_len < 2 || out.is_empty() || scratch_win.len() < 4 {
            return 0;
        }

        let grain_len = ((self.spec.grain_ms * 0.001 * self.output_rate).round() as u32).max(4);
        let spawn_rate = self.spec.density / self.output_rate;
        let pitch_mul = 2.0_f64.powf(f64::from(self.spec.pitch_semitones) / 12.0);
        let grain_step = (step * pitch_mul).clamp(0.01, 16.0);

        // Schedule new grains across this block.
        for i in 0..out.len() {
            if self.spawn_accum >= 1.0 {
                self.spawn_accum -= 1.0;
                self.spawn_grain(i as u32, start_f, end_f, grain_len);
            }
            self.spawn_accum += spawn_rate;
        }

        // Normalize by sqrt(expected concurrent grains) for consistent loudness across densities.
        let expected_overlap = (self.spec.density * self.spec.grain_ms * 0.001).max(1.0);
        let norm = 1.0 / expected_overlap.sqrt();
        let max_pos = (end_f - 1) as f64;
        let min_pos = start_f as f64;
        let mut missing = 0u32;

        for grain in &mut self.grains {
            if !grain.active {
                continue;
            }
            let mut block_pos = grain.delay as usize;
            grain.delay = 0;

            while block_pos < out.len() && grain.age < grain.len {
                let remain_out = out.len() - block_pos;
                let remain_grain = (grain.len - grain.age) as usize;
                // Cap frames per inner chunk so source span fits inside `scratch_win`.
                let max_out_for_win =
                    (((scratch_win.len() - 2) as f64 / grain_step).floor() as usize).max(1);
                let count = remain_out.min(remain_grain).min(max_out_for_win);

                let lo_f = grain.pos.floor().clamp(min_pos, max_pos);
                let lo = lo_f as u64;
                let hi_f = (grain.pos + (count as f64) * grain_step)
                    .ceil()
                    .clamp(min_pos, max_pos);
                let span = ((hi_f as u64).saturating_sub(lo) as usize + 2)
                    .min(scratch_win.len())
                    .min((end_f.saturating_sub(lo)) as usize);
                if span == 0 {
                    grain.active = false;
                    break;
                }
                let Some(win) = scratch_win.get_mut(..span) else {
                    break;
                };
                missing += region
                    .sample
                    .read_cached(client, lo, win, mix, Some(decoded))
                    .missing;

                // Incremental double-precision complex phasor for Hann window:
                // zero per-sample `cos()` calls, exact to < 1e-12 across the chunk.
                let d_angle = std::f64::consts::TAU / f64::from(grain.len);
                let (sin_d, cos_d) = d_angle.sin_cos();
                let (mut sin_p, mut cos_p) = (f64::from(grain.age) * d_angle).sin_cos();
                let gain_l = 0.5 * norm * grain.pan_l;
                let gain_r = 0.5 * norm * grain.pan_r;
                let lo_f64 = lo as f64;
                let last_idx = span.saturating_sub(1);
                let dst_slice = out
                    .get_mut(block_pos..block_pos + count)
                    .unwrap_or_default();
                for dst in dst_slice {
                    let rel = grain.pos - lo_f64;
                    let idx0 = (rel.floor() as usize).min(last_idx);
                    let idx1 = (idx0 + 1).min(last_idx);
                    let frac = (rel - rel.floor()).clamp(0.0, 1.0) as f32;
                    let s0 = win.get(idx0).copied().unwrap_or([0.0; 2]);
                    let s1 = win.get(idx1).copied().unwrap_or(s0);
                    let sl = s0[0] + (s1[0] - s0[0]) * frac;
                    let sr = s0[1] + (s1[1] - s0[1]) * frac;

                    // Hann window: 0.5 * (1 - cos(2*pi*age/len)), pre-scaled by norm * pan.
                    let hann = 1.0 - cos_p as f32;
                    dst[0] += sl * (hann * gain_l);
                    dst[1] += sr * (hann * gain_r);

                    let next_cos = cos_p * cos_d - sin_p * sin_d;
                    let next_sin = sin_p * cos_d + cos_p * sin_d;
                    cos_p = next_cos;
                    sin_p = next_sin;

                    grain.pos += grain_step;
                    if grain.pos >= max_pos {
                        // Wrap cleanly inside the region window so sustained clouds never stall.
                        grain.pos = min_pos + (grain.pos - min_pos) % (region_len as f64);
                    }
                    grain.age += 1;
                }
                block_pos += count;
            }

            if grain.age >= grain.len {
                grain.active = false;
            }
        }

        missing
    }

    fn spawn_grain(&mut self, delay: u32, start_f: u64, end_f: u64, len: u32) {
        // Choose a free slot, or steal the oldest grain if the pool is full.
        let slot = self
            .grains
            .iter()
            .position(|g| !g.active)
            .unwrap_or_else(|| {
                self.grains
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, g)| g.age)
                    .map_or(0, |(i, _)| i)
            });

        let span = end_f.saturating_sub(start_f) as f64;
        let jitter = (self.next_unit_f32() * 2.0 - 1.0) * 0.05 * self.spec.spread;
        let norm_pos = (self.spec.position + jitter).clamp(0.0, 1.0);
        let pos = start_f as f64 + f64::from(norm_pos) * (span - 1.0).max(0.0);

        let pan = (self.next_unit_f32() * 2.0 - 1.0) * self.spec.spread;
        let angle = (pan + 1.0) * 0.25 * std::f32::consts::PI;
        let (pan_l, pan_r) = if self.spec.spread > 0.0 {
            (
                angle.cos() * std::f32::consts::SQRT_2,
                angle.sin() * std::f32::consts::SQRT_2,
            )
        } else {
            (1.0, 1.0)
        };

        if let Some(g) = self.grains.get_mut(slot) {
            *g = Grain {
                active: true,
                delay,
                pos,
                age: 0,
                len,
                pan_l,
                pan_r,
            };
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::f32::consts::TAU;
    use std::sync::Arc;

    use kontra_stream::{BlobKey, Encoding, LoadRequest, MemSource, SampleFmt, Store, StoreConfig};

    use super::*;

    fn sine_region(store: &mut Store, frames: usize, hz: f32, rate: u32) -> Region {
        let mut bytes = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let s = (TAU * hz * i as f32 / rate as f32).sin();
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let fmt = SampleFmt {
            encoding: Encoding::F32,
            channels: 1,
        };
        let sample = store
            .load_many_now(vec![LoadRequest {
                key: BlobKey {
                    file: "<grain-test>".into(),
                    offset: 0,
                    fmt,
                },
                source: Arc::new(MemSource::new(fmt, bytes)),
                frames: frames as u64,
                pin: std::iter::once(0..frames as u64).collect(),
            }])
            .into_iter()
            .next()
            .unwrap()
            .unwrap();
        Region {
            sample,
            start: 0,
            end: frames as u64,
            loops: None,
            mix: None,
            table: None,
            reverse: false,
        }
    }

    #[test]
    fn granular_renders_smooth_audible_stereo_cloud_with_spread() {
        const RATE: u32 = 48_000;
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let region = sine_region(&mut store, 8192, 440.0, RATE);
        let client = store.client();
        let mut decoded = Decoded::new();
        let mut scratch = vec![[0.0; 2]; 512];

        let mut gran = Granular::new(
            GranularSpec {
                grain_ms: 40.0,
                density: 30.0,
                position: 0.25,
                pitch_semitones: 0.0,
                spread: 0.8,
            },
            RATE,
        );
        let mut out = vec![[0.0; 2]; 4096];
        let mut missing = 0;
        for chunk in out.chunks_mut(256) {
            missing += gran.render(
                &region,
                &client,
                None,
                &mut decoded,
                &mut scratch,
                1.0,
                chunk,
            );
        }
        assert_eq!(missing, 0);
        let peak_l = out.iter().map(|f| f[0].abs()).fold(0.0_f32, f32::max);
        let peak_r = out.iter().map(|f| f[1].abs()).fold(0.0_f32, f32::max);
        let diff = out
            .iter()
            .map(|f| (f[0] - f[1]).abs())
            .fold(0.0_f32, f32::max);
        assert!(peak_l > 0.2 && peak_l < 4.0, "peak_l={peak_l}");
        assert!(peak_r > 0.2 && peak_r < 4.0, "peak_r={peak_r}");
        assert!(
            diff > 0.05,
            "spread must produce stereo decorrelation: {diff}"
        );
    }
}
