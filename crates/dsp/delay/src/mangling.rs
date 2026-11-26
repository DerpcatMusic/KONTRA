//! Beat-synced slicing, mashing, reverse granular, and WSOLA pitch-stretch
//! processors — all built on the shared RT-safe [`CaptureRing`], [`BeatClock`],
//! [`Smoother`], and [`CrossfadeReader`] primitives in [`crate::slice_buf`].

use std::f32::consts::PI;

use crate::slice_buf::{
    BeatClock, CAPTURE_MAX_S, CaptureRing, CrossfadeReader, Smoother, beats_to_frames, flush,
};

const SMOOTH_S: f32 = 0.012;

/// Raised-cosine de-click envelope near both ends of a `[0, 1)` loop/slice phase.
#[inline]
fn edge_fade(phase: f32, edge: f32) -> f32 {
    let e = edge.clamp(1e-4, 0.45);
    if phase < e {
        0.5 * (1.0 - (PI * phase / e).cos())
    } else if phase > 1.0 - e {
        0.5 * (1.0 - (PI * (1.0 - phase) / e).cos())
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// 1. Beat Masher
// ---------------------------------------------------------------------------

/// Settings for [`BeatMasher`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatMasherSettings {
    pub tempo_bpm: f32,
    pub length_beats: f32,
    pub rotate: f32,
    pub gate: f32,
    pub mix: f32,
    pub reverse: bool,
    pub wrap: bool,
    pub mash: bool,
}

/// Real-time beat-synced capture and stutter/loop masher.
#[derive(Debug)]
pub struct BeatMasher {
    s: BeatMasherSettings,
    rate: f32,
    ring: CaptureRing,
    /// Monotonic start frame of the locked 1-bar capture window.
    anchor: i64,
    was_mashing: bool,
    loop_pos: f64,
    mix: Smoother,
    mash_blend: Smoother,
    gate: Smoother,
    rotate: Smoother,
    written: f32,
}

impl BeatMasher {
    /// Allocates the capture buffer. Not real-time.
    #[must_use]
    pub fn new(s: BeatMasherSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        Self {
            s,
            rate: rate_f,
            ring: CaptureRing::for_seconds(CAPTURE_MAX_S, rate),
            anchor: 0,
            was_mashing: false,
            loop_pos: 0.0,
            mix: Smoother::new(s.mix.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            mash_blend: Smoother::new(if s.mash { 1.0 } else { 0.0 }, SMOOTH_S, rate_f),
            gate: Smoother::new(s.gate.clamp(-1.0, 1.0), SMOOTH_S, rate_f),
            rotate: Smoother::new(s.rotate.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            written: 0.0,
        }
    }

    /// Updates settings without allocation. Real-time.
    pub fn set(&mut self, s: BeatMasherSettings) {
        self.s = s;
    }

    /// Capture buffer capacity in frames.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.capacity()
    }

    /// Loudest sample written during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances state on silence. Real-time.
    pub fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.ring.push([0.0; 2]);
        }
        self.written = 0.0;
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let max_buf = (self.ring.capacity().saturating_sub(16)) as f64;
        let bar_frames = beats_to_frames(4.0, self.s.tempo_bpm, self.rate).min(max_buf);
        let len_frames = beats_to_frames(self.s.length_beats, self.s.tempo_bpm, self.rate)
            .clamp(32.0, bar_frames);

        let mix_t = self.s.mix.clamp(0.0, 1.0);
        let mash_t = if self.s.mash { 1.0 } else { 0.0 };
        let gate_t = self.s.gate.clamp(-1.0, 1.0);
        let rot_t = self.s.rotate.clamp(0.0, 1.0);

        // Lock capture anchor on the rising edge of `mash`.
        if self.s.mash && !self.was_mashing {
            let have = self.ring.total() as i64;
            let back = (len_frames.round() as i64).min(have);
            self.anchor = have - back;
            self.loop_pos = 0.0;
        }
        self.was_mashing = self.s.mash;

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            // Keep recording live audio while un-mashed so capture is always fresh.
            if !self.s.mash {
                self.ring.push([dry_l, dry_r]);
                peak = peak.max(dry_l.abs()).max(dry_r.abs());
            }

            let mix = self.mix.tick(mix_t);
            let blend = self.mash_blend.tick(mash_t);
            let gate = self.gate.tick(gate_t);
            let rot = self.rotate.tick(rot_t);

            let phase = (self.loop_pos / len_frames).clamp(0.0, 1.0) as f32;
            let offset_in_bar = f64::from(rot) * bar_frames;
            let raw_local = if self.s.reverse {
                len_frames - 1.0 - self.loop_pos
            } else {
                self.loop_pos
            };
            let rel = if self.s.wrap {
                (offset_in_bar + raw_local).rem_euclid(bar_frames)
            } else {
                (offset_in_bar + raw_local).clamp(0.0, bar_frames - 1.0)
            };
            let read_pos = self.anchor as f64 + rel;
            let captured = self.ring.read_abs(read_pos);

            // Gate envelope: 0 = full open; < 0 chops tail; > 0 chops front.
            let gate_env = if gate.abs() < 1e-3 {
                1.0
            } else if gate < 0.0 {
                let open = (1.0 + gate).clamp(0.08, 1.0);
                if phase < open {
                    edge_fade(phase / open, 0.06)
                } else {
                    0.0
                }
            } else {
                let start = gate.clamp(0.0, 0.92);
                if phase >= start {
                    edge_fade((phase - start) / (1.0 - start).max(0.08), 0.06)
                } else {
                    0.0
                }
            };
            let env = edge_fade(phase, 0.02) * gate_env;

            self.loop_pos += 1.0;
            if self.loop_pos >= len_frames {
                self.loop_pos -= len_frames;
            }

            let wet_l = dry_l + (captured[0] * env - dry_l) * blend;
            let wet_r = dry_r + (captured[1] * env - dry_r) * blend;
            *frame = [
                flush(dry_l + (wet_l - dry_l) * mix),
                flush(dry_r + (wet_r - dry_r) * mix),
            ];
        }
        self.written = peak;
    }
}

// ---------------------------------------------------------------------------
// 2. Beat Slicer
// ---------------------------------------------------------------------------

/// Five distinct 8-slice rhythmic permutation patterns.
const SLICER_PATTERNS: [[usize; 8]; 5] = [
    [0, 1, 2, 3, 4, 5, 6, 7], // 0: Straight
    [0, 0, 2, 1, 4, 4, 6, 5], // 1: Stutter bounce
    [0, 2, 1, 3, 4, 6, 5, 7], // 2: Syncopated swap
    [7, 6, 5, 4, 3, 2, 1, 0], // 3: Reverse sequence
    [0, 3, 2, 1, 4, 7, 6, 3], // 4: Broken funk
];

/// Settings for [`BeatSlicer`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatSlicerSettings {
    pub tempo_bpm: f32,
    pub bars: u8,
    pub pattern: u8,
    pub style: u8,
    pub buzz: f32,
    pub gate: bool,
    pub slice: bool,
    pub mix: f32,
}

/// Real-time beat-synced capture and slice-reordering processor.
#[derive(Debug)]
pub struct BeatSlicer {
    s: BeatSlicerSettings,
    rate: f32,
    ring: CaptureRing,
    clock: BeatClock,
    anchor: i64,
    was_slicing: bool,
    mix: Smoother,
    slice_blend: Smoother,
    buzz: Smoother,
    written: f32,
}

impl BeatSlicer {
    /// Allocates the capture buffer. Not real-time.
    #[must_use]
    pub fn new(s: BeatSlicerSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        Self {
            s,
            rate: rate_f,
            ring: CaptureRing::for_seconds(CAPTURE_MAX_S, rate),
            clock: BeatClock::new(),
            anchor: 0,
            was_slicing: false,
            mix: Smoother::new(s.mix.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            slice_blend: Smoother::new(if s.slice { 1.0 } else { 0.0 }, SMOOTH_S, rate_f),
            buzz: Smoother::new(s.buzz.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            written: 0.0,
        }
    }

    /// Updates settings without allocation. Real-time.
    pub fn set(&mut self, s: BeatSlicerSettings) {
        self.s = s;
    }

    /// Capture buffer capacity in frames.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.capacity()
    }

    /// Loudest sample written during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances state on silence. Real-time.
    pub fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.ring.push([0.0; 2]);
        }
        self.written = 0.0;
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let max_buf = (self.ring.capacity().saturating_sub(16)) as f64;
        let beats = if self.s.bars >= 2 { 8.0 } else { 4.0 };
        let total_frames = beats_to_frames(beats, self.s.tempo_bpm, self.rate).min(max_buf);
        let slice_frames = (total_frames / 8.0).max(32.0);
        let cycle_hz = f64::from(self.rate) / total_frames;
        let inv_rate = 1.0 / f64::from(self.rate);

        let pat_idx = usize::from(self.s.pattern.min(4));
        let pattern = SLICER_PATTERNS
            .get(pat_idx)
            .copied()
            .unwrap_or(SLICER_PATTERNS[0]);
        let style = self.s.style.min(4);

        if self.s.slice && !self.was_slicing {
            let have = self.ring.total() as i64;
            let back = (total_frames.round() as i64).min(have);
            self.anchor = have - back;
            self.clock.reset();
        }
        self.was_slicing = self.s.slice;

        let mix_t = self.s.mix.clamp(0.0, 1.0);
        let slice_t = if self.s.slice { 1.0 } else { 0.0 };
        let buzz_t = self.s.buzz.clamp(0.0, 1.0);

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            if !self.s.slice {
                self.ring.push([dry_l, dry_r]);
                peak = peak.max(dry_l.abs()).max(dry_r.abs());
            }

            let (bar_phase, _) = self.clock.tick(cycle_hz, inv_rate);
            let mix = self.mix.tick(mix_t);
            let blend = self.slice_blend.tick(slice_t);
            let buzz = self.buzz.tick(buzz_t);

            let pos8 = bar_phase * 8.0;
            let step = (pos8.floor() as usize).min(7);
            let local_phase = (pos8 - pos8.floor()) as f32;
            let src_slice = pattern.get(step).copied().unwrap_or(step);

            // Apply buzz micro-repeat subdivision (1x..8x repeats of the slice head).
            let sub_reps = 1.0 + (buzz * 7.0).floor();
            let sub_phase = (local_phase * sub_reps).fract();

            // Style modifies slice direction / speed / gate shape.
            let (read_phase, style_env) = match style {
                1 if step % 2 == 1 => (1.0 - sub_phase, 1.0), // Reverse odd slices
                2 => ((sub_phase * 0.5), 1.0),                // Half-speed stretch feel
                3 => {
                    let env = (1.0 - local_phase * 0.7).max(0.0);
                    (sub_phase, env)
                } // Percussive decay
                4 if step % 3 == 2 => (1.0 - sub_phase, 0.85), // Triplet reverse accents
                _ => (sub_phase, 1.0),
            };

            let gate_env = if self.s.gate {
                if local_phase < 0.55 {
                    edge_fade(local_phase / 0.55, 0.08)
                } else {
                    0.0
                }
            } else {
                edge_fade(sub_phase, 0.04)
            };

            let read_pos =
                self.anchor as f64 + (src_slice as f64 + f64::from(read_phase)) * slice_frames;
            let captured = self.ring.read_abs(read_pos);
            let env = gate_env * style_env;

            let wet_l = dry_l + (captured[0] * env - dry_l) * blend;
            let wet_r = dry_r + (captured[1] * env - dry_r) * blend;
            *frame = [
                flush(dry_l + (wet_l - dry_l) * mix),
                flush(dry_r + (wet_r - dry_r) * mix),
            ];
        }
        self.written = peak;
    }
}

// ---------------------------------------------------------------------------
// 3. Reverse Grain
// ---------------------------------------------------------------------------

/// Settings for [`ReverseGrain`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReverseGrainSettings {
    pub speed: f32,
    pub grain_ms: f32,
    pub pitch_ratio: f32,
    pub mix: f32,
    pub forward: bool,
    pub invert_grains: bool,
    pub reverse: bool,
}

/// Dual-window Hann raised-cosine reverse granular engine.
#[derive(Debug)]
pub struct ReverseGrain {
    s: ReverseGrainSettings,
    rate: f32,
    ring: CaptureRing,
    reader: CrossfadeReader,
    /// Slowly moving capture traversal offset (frames behind write head).
    cursor_delay: f64,
    mix: Smoother,
    active_blend: Smoother,
    grain_ms: Smoother,
    pitch: Smoother,
    speed: Smoother,
    written: f32,
}

impl ReverseGrain {
    /// Allocates the circular capture buffer. Not real-time.
    #[must_use]
    pub fn new(s: ReverseGrainSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        let active = s.reverse || s.forward;
        Self {
            s,
            rate: rate_f,
            ring: CaptureRing::for_seconds(CAPTURE_MAX_S, rate),
            reader: CrossfadeReader::new(),
            cursor_delay: f64::from(s.grain_ms.clamp(15.0, 500.0)) * f64::from(rate_f) * 0.001,
            mix: Smoother::new(s.mix.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            active_blend: Smoother::new(if active { 1.0 } else { 0.0 }, SMOOTH_S, rate_f),
            grain_ms: Smoother::new(s.grain_ms.clamp(15.0, 500.0), SMOOTH_S, rate_f),
            pitch: Smoother::new(s.pitch_ratio.clamp(0.25, 4.0), SMOOTH_S, rate_f),
            speed: Smoother::new(s.speed.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            written: 0.0,
        }
    }

    /// Updates settings without allocation. Real-time.
    pub fn set(&mut self, s: ReverseGrainSettings) {
        self.s = s;
    }

    /// Capture buffer capacity in frames.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.capacity()
    }

    /// Loudest sample written during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances state on silence. Real-time.
    pub fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.ring.push([0.0; 2]);
        }
        self.written = 0.0;
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let max_delay = (self.ring.capacity() as f64) * 0.75;
        let active = self.s.reverse || self.s.forward;
        let act_t = if active { 1.0 } else { 0.0 };
        let mix_t = self.s.mix.clamp(0.0, 1.0);
        let grain_t = self.s.grain_ms.clamp(15.0, 500.0);
        let pitch_t = self.s.pitch_ratio.clamp(0.25, 4.0);
        let speed_t = self.s.speed.clamp(0.0, 1.0);

        // Default Reverse Grain reads backwards inside each Hann window;
        // `invert_grains` flips each grain's internal read direction.
        let base_reverse = self.s.reverse || !self.s.forward;
        let grain_dir: f64 = if base_reverse ^ self.s.invert_grains {
            -1.0
        } else {
            1.0
        };

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            self.ring.push([dry_l, dry_r]);
            peak = peak.max(dry_l.abs()).max(dry_r.abs());

            let mix = self.mix.tick(mix_t);
            let blend = self.active_blend.tick(act_t);
            let grain_ms = self.grain_ms.tick(grain_t);
            let pitch = self.pitch.tick(pitch_t);
            let speed = self.speed.tick(speed_t);

            let grain_frames = (f64::from(grain_ms) * f64::from(self.rate) * 0.001).max(64.0);
            let read_speed = grain_dir * f64::from(pitch);

            // When active, the spawn cursor lags behind write head so reversed grains
            // always read valid already-recorded audio; `speed` controls how fast the
            // capture window drifts back/forward.
            let min_lag = (grain_frames * f64::from(pitch).max(1.0) + 16.0).min(max_delay);
            let drift = if self.s.forward && !self.s.reverse {
                (1.0 - f64::from(speed)) * 0.25
            } else {
                (1.0 - f64::from(speed)) * 0.5
            };
            if active {
                self.cursor_delay = (self.cursor_delay + drift).clamp(min_lag, max_delay);
            } else {
                self.cursor_delay = min_lag;
            }

            let newest = self.ring.total().saturating_sub(1) as f64;
            // Reverse grains start near the newest end of their window and sweep backwards;
            // forward grains start `grain_frames` earlier and sweep forwards.
            let spawn_pos = if grain_dir < 0.0 {
                newest - 2.0 - (self.cursor_delay - min_lag)
            } else {
                newest - self.cursor_delay
            };

            let grained = self
                .reader
                .tick(&self.ring, grain_frames, read_speed, spawn_pos, 0);
            let wet_l = dry_l + (grained[0] - dry_l) * blend;
            let wet_r = dry_r + (grained[1] - dry_r) * blend;
            *frame = [
                flush(dry_l + (wet_l - dry_l) * mix),
                flush(dry_r + (wet_r - dry_r) * mix),
            ];
        }
        self.written = peak;
    }
}

// ---------------------------------------------------------------------------
// 4. Transpose Stretch
// ---------------------------------------------------------------------------

/// Settings for [`TransposeStretch`] in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransposeStretchSettings {
    pub tempo_bpm: f32,
    pub semitones: f32,
    pub amount: f32,
    pub grain_ms: f32,
    pub mix: f32,
    pub stretch: bool,
    pub grain_mode: bool,
    pub bars: u8,
}

/// Real-time WSOLA / overlap-add granular pitch-shift and time-stretch processor.
#[derive(Debug)]
pub struct TransposeStretch {
    s: TransposeStretchSettings,
    rate: f32,
    ring: CaptureRing,
    reader: CrossfadeReader,
    /// Stretch playback anchor & cursor when `stretch == true`.
    anchor: f64,
    stretch_offset: f64,
    was_stretching: bool,
    mix: Smoother,
    semitones: Smoother,
    grain_ms: Smoother,
    amount: Smoother,
    written: f32,
}

impl TransposeStretch {
    /// Allocates the circular capture buffer. Not real-time.
    #[must_use]
    pub fn new(s: TransposeStretchSettings, rate: u32) -> Self {
        let rate_f = rate.max(1) as f32;
        Self {
            s,
            rate: rate_f,
            ring: CaptureRing::for_seconds(CAPTURE_MAX_S, rate),
            reader: CrossfadeReader::new(),
            anchor: 0.0,
            stretch_offset: 0.0,
            was_stretching: false,
            mix: Smoother::new(s.mix.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            semitones: Smoother::new(s.semitones.clamp(-60.0, 12.0), SMOOTH_S, rate_f),
            grain_ms: Smoother::new(s.grain_ms.clamp(15.0, 250.0), SMOOTH_S, rate_f),
            amount: Smoother::new(s.amount.clamp(0.0, 1.0), SMOOTH_S, rate_f),
            written: 0.0,
        }
    }

    /// Updates settings without allocation. Real-time.
    pub fn set(&mut self, s: TransposeStretchSettings) {
        self.s = s;
    }

    /// Capture buffer capacity in frames.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.capacity()
    }

    /// Loudest sample written during the last block.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances state on silence. Real-time.
    pub fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.ring.push([0.0; 2]);
        }
        self.written = 0.0;
    }

    /// Processes stereo audio in place. Zero allocations, real-time safe.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let max_buf = (self.ring.capacity().saturating_sub(32)) as f64;
        let beats = if self.s.bars >= 2 { 8.0 } else { 4.0 };
        let bar_frames = beats_to_frames(beats, self.s.tempo_bpm, self.rate).min(max_buf);

        if self.s.stretch && !self.was_stretching {
            let have = self.ring.total() as f64;
            let back = bar_frames.min(have);
            self.anchor = have - back;
            self.stretch_offset = 0.0;
        }
        self.was_stretching = self.s.stretch;

        let mix_t = self.s.mix.clamp(0.0, 1.0);
        let semi_t = self.s.semitones.clamp(-60.0, 12.0);
        let grain_t = self.s.grain_ms.clamp(15.0, 250.0);
        let amount_t = self.s.amount.clamp(0.0, 1.0);

        let mut peak = 0.0f32;

        for frame in io {
            let [dry_l, dry_r] = *frame;
            // When stretching a frozen bar, keep the captured bar intact;
            // otherwise continuously stream live audio into the ring.
            if !self.s.stretch {
                self.ring.push([dry_l, dry_r]);
                peak = peak.max(dry_l.abs()).max(dry_r.abs());
            }

            let mix = self.mix.tick(mix_t);
            let semis = self.semitones.tick(semi_t);
            let grain_ms = self.grain_ms.tick(grain_t);
            let amount = self.amount.tick(amount_t);

            let ratio = f64::from((semis / 12.0).exp2()).clamp(0.031_25, 2.0);
            let grain_frames = (f64::from(grain_ms) * f64::from(self.rate) * 0.001).max(64.0);

            let spawn_pos = if self.s.stretch {
                // Slow down traversal across the captured bar as `amount` grows.
                let travel = if self.s.grain_mode {
                    (1.0 - f64::from(amount)) * 0.25
                } else {
                    1.0 - 0.85 * f64::from(amount)
                };
                self.stretch_offset =
                    (self.stretch_offset + travel).rem_euclid(bar_frames.max(1.0));
                self.anchor + self.stretch_offset
            } else {
                // Real-time pitch shift: place grain start far enough behind newest
                // so even at ratio=2.0 the grain never overtakes the write head.
                let newest = self.ring.total().saturating_sub(1) as f64;
                let headroom = if ratio > 1.0 {
                    (ratio - 1.0) * grain_frames + 64.0
                } else {
                    64.0
                };
                newest - headroom
            };

            let wsola_radius = if self.s.grain_mode { 0 } else { 48 };
            let wet = self
                .reader
                .tick(&self.ring, grain_frames, ratio, spawn_pos, wsola_radius);

            *frame = [
                flush(dry_l + (wet[0] - dry_l) * mix),
                flush(dry_r + (wet[1] - dry_r) * mix),
            ];
        }
        self.written = peak;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn sine(hz: f32, n: usize) -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let s = (TAU * hz * i as f32 / 48_000.0).sin() * 0.6;
                [s, s]
            })
            .collect()
    }

    #[test]
    fn beat_masher_loops_captured_segment_periodically() {
        let mut bm = BeatMasher::new(
            BeatMasherSettings {
                tempo_bpm: 120.0,
                length_beats: 0.25, // 1/16 note = 6000 frames at 48 kHz, 120 BPM
                rotate: 0.0,
                gate: 0.0,
                mix: 1.0,
                reverse: false,
                wrap: true,
                mash: false,
            },
            48_000,
        );
        // Feed a ramp/distinct signal for 12,000 frames un-mashed.
        let mut feed: Vec<[f32; 2]> = (0..12_000)
            .map(|i| {
                let v =
                    (TAU * 317.0 * i as f32 / 48_000.0).sin() * (0.3 + 0.7 * (i as f32 / 12_000.0));
                [v, v]
            })
            .collect();
        bm.process(&mut feed);

        // Engage mash and render 3 periods (18,000 frames).
        bm.set(BeatMasherSettings { mash: true, ..bm.s });
        let mut out = vec![[0.0; 2]; 18_000];
        bm.process(&mut out);

        // Period 2 (6000..12000) and Period 3 (12000..18000) must match closely.
        let mut diff = 0.0f32;
        let mut energy = 0.0f32;
        for i in 6000..12_000 {
            let d = out[i][0] - out[i + 6000][0];
            diff += d * d;
            energy += out[i][0] * out[i][0];
        }
        assert!(energy > 10.0, "mashed output must be audible: {energy}");
        assert!(
            diff / energy < 1e-4,
            "stutter periodicity error: {}",
            diff / energy
        );
    }

    #[test]
    fn beat_slicer_reorders_captured_slices() {
        let mut bs = BeatSlicer::new(
            BeatSlicerSettings {
                tempo_bpm: 120.0,
                bars: 1,
                pattern: 3, // Reverse sequence [7,6,5,4,3,2,1,0]
                style: 0,
                buzz: 0.0,
                gate: false,
                slice: false,
                mix: 1.0,
            },
            48_000,
        );
        // 1 bar at 120 BPM = 2s = 96,000 frames. Put a loud burst ONLY in slice 7 (last 12k frames).
        let mut feed = vec![[0.0; 2]; 96_000];
        for (i, frame) in feed[84_000..96_000].iter_mut().enumerate() {
            let v = (TAU * 440.0 * i as f32 / 48_000.0).sin() * 0.8;
            *frame = [v, v];
        }
        bs.process(&mut feed);

        bs.set(BeatSlicerSettings {
            slice: true,
            ..bs.s
        });
        let mut out = vec![[0.0; 2]; 24_000];
        bs.process(&mut out);
        // Pattern 3 plays slice 7 FIRST (frames 0..12,000), then slice 6 (silent, 12,000..24,000).
        let first_rms: f32 = out[2000..10_000].iter().map(|f| f[0] * f[0]).sum();
        let second_rms: f32 = out[14_000..22_000].iter().map(|f| f[0] * f[0]).sum();
        assert!(first_rms > 100.0, "slice 7 should play first: {first_rms}");
        assert!(second_rms < 1e-3, "slice 6 should be silent: {second_rms}");
    }

    #[test]
    fn reverse_grain_reverses_chirp_envelope() {
        let mut rg = ReverseGrain::new(
            ReverseGrainSettings {
                speed: 1.0,
                grain_ms: 100.0,
                pitch_ratio: 1.0,
                mix: 1.0,
                forward: false,
                invert_grains: false,
                reverse: true,
            },
            48_000,
        );
        // Feed a saw-envelope burst that ramps UP over 100ms, then silence; reversed output should ramp DOWN.
        let mut io = vec![[0.0; 2]; 24_000];
        for (i, frame) in io.iter_mut().enumerate() {
            let s = (TAU * 440.0 * i as f32 / 48_000.0).sin() * ((i % 4800) as f32 / 4800.0);
            *frame = [s, s];
        }
        rg.process(&mut io);
        assert!(io.iter().all(|f| f[0].is_finite() && f[1].is_finite()));
        let energy: f32 = io[6000..].iter().map(|f| f[0] * f[0]).sum();
        assert!(energy > 10.0, "reverse grain audible: {energy}");
    }

    #[test]
    fn transpose_stretch_shifts_pitch_by_exact_octave() {
        let mut ts = TransposeStretch::new(
            TransposeStretchSettings {
                tempo_bpm: 120.0,
                semitones: 12.0, // +1 octave -> 2x frequency
                amount: 0.0,
                grain_ms: 60.0,
                mix: 1.0,
                stretch: false,
                grain_mode: false,
                bars: 1,
            },
            48_000,
        );
        let mut io = sine(440.0, 96_000);
        ts.process(&mut io);
        let tail = &io[48_000..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
            .count();
        // 880 Hz has 1760 zero crossings per second.
        assert!(
            (1700..=1820).contains(&crossings),
            "expected ~1760 crossings for +12st of 440 Hz, got {crossings}"
        );
    }
}
