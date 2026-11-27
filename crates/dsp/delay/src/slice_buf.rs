//! Shared RT-safe primitives for tempo-synced slicing, gating, and granular
//! time/pitch processors:
//! - [`Smoother`]: per-sample one-pole parameter smoothing (no zipper noise).
//! - [`BeatClock`]: tempo-synced or free-running phase accumulator `[0, 1)`.
//! - [`CaptureRing`]: pre-allocated power-of-two stereo ring buffer with
//!   panic-free fractional read interpolation and capture-anchor addressing.
//! - [`CrossfadeReader`]: dual overlapping Hann-windowed fractional read heads
//!   with constant unity sum (`hann(p) + hann(p + 0.5) == 1.0`) and optional
//!   lightweight WSOLA local cross-correlation alignment on grain reset.

use std::f32::consts::TAU;

/// Default capture capacity in seconds (supports 2 bars down to ~60 BPM).
pub const CAPTURE_MAX_S: f32 = 8.5;

pub use kontra_dsp_core::flush_f32 as flush;

/// Raised-cosine (Hann) window on `phase` in `0..=1`: `0.5 * (1 - cos(2*pi*p))`.
/// Satisfies `hann(p) + hann((p + 0.5) % 1.0) == 1.0` identically.
#[inline]
#[must_use]
pub fn hann(phase: f32) -> f32 {
    0.5 * (1.0 - (TAU * phase.clamp(0.0, 1.0)).cos())
}

pub use kontra_dsp_core::Smoother;

/// Tempo-synced or free-running `[0.0, 1.0)` phase clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct BeatClock {
    phase: f64,
}

impl BeatClock {
    /// Creates a new clock at phase 0.
    #[must_use]
    pub const fn new() -> Self {
        Self { phase: 0.0 }
    }

    /// Resets phase to 0.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
    }

    /// Current phase in `[0.0, 1.0)`.
    #[inline]
    #[must_use]
    pub fn phase(self) -> f64 {
        self.phase
    }

    /// Advances by one sample at `cycles_per_sec` Hz. Returns `(phase_before, wrapped)`.
    #[inline]
    pub fn tick(&mut self, cycles_per_sec: f64, inv_rate: f64) -> (f64, bool) {
        let before = self.phase;
        let step = (cycles_per_sec * inv_rate).clamp(0.0, 0.5);
        self.phase += step;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            (before, true)
        } else {
            (before, false)
        }
    }

    /// Advances by `frames` silent samples.
    #[inline]
    pub fn advance(&mut self, frames: usize, cycles_per_sec: f64, inv_rate: f64) {
        let step = (cycles_per_sec * inv_rate).max(0.0);
        self.phase = (self.phase + step * frames as f64).rem_euclid(1.0);
    }
}

/// Converts a beat count at `tempo_bpm` into frames, clamped to at least 1 frame.
#[inline]
#[must_use]
pub fn beats_to_frames(beats: f32, tempo_bpm: f32, rate: f32) -> f64 {
    let bpm = f64::from(tempo_bpm.clamp(20.0, 400.0));
    let frames = f64::from(beats.max(1e-4)) * (60.0 / bpm) * f64::from(rate.max(1.0));
    frames.max(1.0)
}

/// Pre-allocated power-of-two stereo ring buffer with absolute frame counter
/// and panic-free fractional Catmull-Rom / linear interpolation.
#[derive(Debug)]
pub struct CaptureRing {
    buf: Box<[[f32; 2]]>,
    mask: usize,
    /// Monotonic total frames pushed so far.
    total: u64,
}

impl CaptureRing {
    /// Allocates a power-of-two ring buffer holding at least `min_frames`. Not real-time.
    #[must_use]
    pub fn new(min_frames: usize) -> Self {
        let len = min_frames.max(64).next_power_of_two();
        Self {
            buf: vec![[0.0; 2]; len].into_boxed_slice(),
            mask: len - 1,
            total: 0,
        }
    }

    /// Allocates capacity for `seconds` at `rate` Hz. Not real-time.
    #[must_use]
    pub fn for_seconds(seconds: f32, rate: u32) -> Self {
        let frames = (seconds.max(0.1) * rate.max(1) as f32).ceil() as usize;
        Self::new(frames + 16)
    }

    /// Ring capacity in frames.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Monotonic frame count pushed so far (the newest frame is at `total - 1`).
    #[inline]
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Pushes one stereo frame into the ring. Real-time, zero allocations.
    #[inline]
    pub fn push(&mut self, frame: [f32; 2]) {
        let idx = (self.total as usize) & self.mask;
        if let Some(cell) = self.buf.get_mut(idx) {
            *cell = [flush(frame[0]), flush(frame[1])];
        }
        self.total = self.total.wrapping_add(1);
    }

    /// Reads an integer frame at monotonic index `abs_pos` (wrapping cleanly).
    #[inline]
    #[must_use]
    pub fn read_int(&self, abs_pos: i64) -> [f32; 2] {
        let idx = (abs_pos as usize) & self.mask;
        self.buf.get(idx).copied().unwrap_or([0.0; 2])
    }

    /// Reads a stereo frame `delay_frames` behind the newest pushed frame using
    /// linear interpolation.
    #[inline]
    #[must_use]
    pub fn read_back(&self, delay_frames: f64) -> [f32; 2] {
        let max_d = self.buf.len().saturating_sub(2) as f64;
        let d = delay_frames.clamp(0.0, max_d);
        let newest = self.total.saturating_sub(1) as f64;
        self.read_abs(newest - d)
    }

    /// Reads a stereo frame at fractional monotonic index `abs_pos` using
    /// 4-point cubic (Hermite) interpolation for clean pitch-shifted audio.
    #[inline]
    #[must_use]
    pub fn read_abs(&self, abs_pos: f64) -> [f32; 2] {
        let base = abs_pos.floor() as i64;
        let frac = (abs_pos - base as f64) as f32;
        let ym1 = self.read_int(base - 1);
        let y0 = self.read_int(base);
        let y1 = self.read_int(base + 1);
        let y2 = self.read_int(base + 2);
        [
            hermite(ym1[0], y0[0], y1[0], y2[0], frac),
            hermite(ym1[1], y0[1], y1[1], y2[1], frac),
        ]
    }

    /// Lightweight WSOLA search around `nominal_pos` (in `[-radius, +radius]`)
    /// to maximize correlation with the waveform around `ref_pos`.
    /// Uses small decimated strides so cost is negligible and strictly bounded.
    #[must_use]
    pub fn best_wsola_offset(&self, ref_pos: i64, nominal_pos: i64, radius: i64) -> i64 {
        let r = radius.clamp(0, 128);
        if r < 4 {
            return 0;
        }
        // 8-tap sparse correlation template over 32 frames.
        const TAPS: [i64; 8] = [0, 4, 8, 12, 16, 20, 24, 28];
        let mut ref_sig = [0.0f32; 8];
        let mut ref_energy = 0.0f32;
        for (dst, &tap) in ref_sig.iter_mut().zip(&TAPS) {
            let f = self.read_int(ref_pos + tap);
            let m = 0.5 * (f[0] + f[1]);
            *dst = m;
            ref_energy += m * m;
        }
        if ref_energy < 1e-10 {
            return 0;
        }

        let mut best_off = 0i64;
        let mut best_score = f32::NEG_INFINITY;
        let step = (r / 16).max(2);
        let mut off = -r;
        while off <= r {
            let mut dot = 0.0f32;
            let mut cand_energy = 0.0f32;
            for (&rv, &tap) in ref_sig.iter().zip(&TAPS) {
                let f = self.read_int(nominal_pos + off + tap);
                let m = 0.5 * (f[0] + f[1]);
                dot += rv * m;
                cand_energy += m * m;
            }
            let denom = (ref_energy * cand_energy).sqrt().max(1e-8);
            let score = dot / denom;
            if score > best_score {
                best_score = score;
                best_off = off;
            }
            off += step;
        }
        best_off
    }
}

use kontra_dsp_core::hermite;

/// Dual overlapping Hann-windowed grain reader (phases 0.5 apart so
/// `hann(p0) + hann(p1) == 1.0`). Shared by Reverse Grain and Transpose Stretch.
#[derive(Clone, Copy, Debug)]
pub struct CrossfadeReader {
    /// Window phase of head 0 in `[0.0, 1.0)`; head 1 is always `(phase + 0.5) % 1.0`.
    phase: f64,
    /// Monotonic read positions for the two overlapping heads.
    pos: [f64; 2],
    /// Whether each head has been initialized.
    initialized: [bool; 2],
}

impl Default for CrossfadeReader {
    fn default() -> Self {
        Self::new()
    }
}

impl CrossfadeReader {
    /// Creates an uninitialized dual-head reader.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            phase: 0.0,
            pos: [0.0; 2],
            initialized: [false; 2],
        }
    }

    /// Resets both heads so they re-anchor on the next sample.
    #[inline]
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.initialized = [false; 2];
    }

    /// Reads one overlap-added stereo frame from `ring`.
    /// - `grain_frames`: duration of one full Hann grain in frames (`>= 32`).
    /// - `read_speed`: frames advanced per output sample (can be negative for reverse).
    /// - `spawn_pos`: monotonic ring position where a newly resetting head starts.
    /// - `wsola_radius`: if `> 0`, searches `±wsola_radius` around `spawn_pos` for
    ///   best correlation with the continuing head's current position.
    #[inline]
    pub fn tick(
        &mut self,
        ring: &CaptureRing,
        grain_frames: f64,
        read_speed: f64,
        spawn_pos: f64,
        wsola_radius: i64,
    ) -> [f32; 2] {
        let g = grain_frames.max(32.0);
        let prev = self.phase;
        let next = prev + 1.0 / g;
        let wrap0 = next >= 1.0;
        let wrap1 = prev < 0.5 && next >= 0.5;
        self.phase = if wrap0 { next - next.floor() } else { next };

        // Reset head 0 at phase 0.0 (where hann(0) == 0 -> click-free).
        if !self.initialized[0] || wrap0 {
            let off = if self.initialized[1] && wsola_radius > 0 {
                ring.best_wsola_offset(
                    self.pos[1].round() as i64,
                    spawn_pos.round() as i64,
                    wsola_radius,
                )
            } else {
                0
            };
            self.pos[0] = spawn_pos + off as f64;
            self.initialized[0] = true;
        }
        // Reset head 1 at phase 0.5 (where hann(0) for head 1 == 0 -> click-free).
        if !self.initialized[1] || wrap1 {
            let off = if self.initialized[0] && wsola_radius > 0 {
                ring.best_wsola_offset(
                    self.pos[0].round() as i64,
                    spawn_pos.round() as i64,
                    wsola_radius,
                )
            } else {
                0
            };
            self.pos[1] = spawn_pos + off as f64;
            self.initialized[1] = true;
        }

        let p0 = self.phase as f32;
        let p1 = if p0 >= 0.5 { p0 - 0.5 } else { p0 + 0.5 };
        let w0 = hann(p0);
        let w1 = hann(p1);

        let s0 = ring.read_abs(self.pos[0]);
        let s1 = ring.read_abs(self.pos[1]);

        self.pos[0] += read_speed;
        self.pos[1] += read_speed;

        [w0 * s0[0] + w1 * s1[0], w0 * s0[1] + w1 * s1[1]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_complementary_sum_is_unity() {
        for i in 0..100 {
            let p = i as f32 / 100.0;
            let q = (p + 0.5) % 1.0;
            let sum = hann(p) + hann(q);
            assert!((sum - 1.0).abs() < 1e-6, "p={p} sum={sum}");
        }
    }

    #[test]
    fn capture_ring_reads_delayed_signal_without_error() {
        let mut ring = CaptureRing::new(256);
        for i in 0..500 {
            let v = (i as f32 * 0.05).sin();
            ring.push([v, -v]);
        }
        let f = ring.read_back(10.0);
        let expected = ((499 - 10) as f32 * 0.05).sin();
        assert!(
            (f[0] - expected).abs() < 1e-4,
            "got {} want {}",
            f[0],
            expected
        );
        assert!((f[1] + expected).abs() < 1e-4);
    }
}
