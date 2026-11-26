//! A dual independent stereo delay: independent left/right pre-delay lines
//! feeding independent left/right main delay lines with self-feedback,
//! stability-clamped cross-channel feedback, independent wet output levels,
//! and a stereo width matrix. Outputs the wet signal only.

/// Longest delay time [`DualDelay::new`] allocates per line, ms.
pub const DUAL_MAX_MS: f32 = 4000.0;

/// Settings in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DualDelaySettings {
    pub predelay_ms: [f32; 2],
    pub time_ms: [f32; 2],
    pub feedback: [f32; 2],
    pub cross_feedback: f32,
    pub level: [f32; 2],
    /// Stereo image: 0 swaps channels, 0.5 sums to mono, 1 is normal stereo.
    pub width: f32,
}

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 || !x.is_finite() {
        0.0
    } else {
        x
    }
}

/// Dual independent stereo delay processor. Zero allocations in `set`/`process`.
#[derive(Clone, Debug, PartialEq)]
pub struct DualDelay {
    rate: f32,
    glide: f32,
    max_frames: f32,
    pre_buf: [Box<[f32]>; 2],
    main_buf: [Box<[f32]>; 2],
    mask: usize,
    write: usize,
    cur_pre: [f32; 2],
    target_pre: [f32; 2],
    cur_main: [f32; 2],
    target_main: [f32; 2],
    gain_smooth: f32,
    cur_feedback: [f32; 2],
    feedback: [f32; 2],
    cur_cross: f32,
    cross: f32,
    cur_level: [f32; 2],
    level: [f32; 2],
    cur_width: f32,
    width: f32,
    written: f32,
}

impl DualDelay {
    /// Allocates the four delay rings for `rate` up to `max_ms`. Not real-time.
    #[must_use]
    pub fn new(s: DualDelaySettings, rate: u32, max_ms: f32) -> Self {
        let rate_f = rate.max(1) as f32;
        let max_frames = (max_ms.max(1.0) * 0.001 * rate_f).ceil();
        let cap = (max_frames as usize + 4).next_power_of_two().max(16);
        // ~15 ms glide for delay times, ~5 ms glide for level/feedback/width.
        let glide = 1.0 - (-1.0 / (0.015 * rate_f)).exp();
        let gain_smooth = 1.0 - (-1.0 / (0.005 * rate_f)).exp();
        let mut d = Self {
            rate: rate_f,
            glide,
            max_frames: (cap - 2) as f32,
            pre_buf: [
                vec![0.0; cap].into_boxed_slice(),
                vec![0.0; cap].into_boxed_slice(),
            ],
            main_buf: [
                vec![0.0; cap].into_boxed_slice(),
                vec![0.0; cap].into_boxed_slice(),
            ],
            mask: cap - 1,
            write: 0,
            cur_pre: [0.0; 2],
            target_pre: [0.0; 2],
            cur_main: [1.0; 2],
            target_main: [1.0; 2],
            gain_smooth,
            cur_feedback: [0.0; 2],
            feedback: [0.0; 2],
            cur_cross: 0.0,
            cross: 0.0,
            cur_level: [0.5; 2],
            level: [0.5; 2],
            cur_width: 1.0,
            width: 1.0,
            written: 0.0,
        };
        d.set(s);
        // Snap initial state so the first block starts at the saved setting.
        d.cur_pre = d.target_pre;
        d.cur_main = d.target_main;
        d.cur_feedback = d.feedback;
        d.cur_cross = d.cross;
        d.cur_level = d.level;
        d.cur_width = d.width;
        d
    }

    /// Updates settings without allocating.
    pub fn set(&mut self, s: DualDelaySettings) {
        let rate = self.rate;
        let max_frames = self.max_frames;
        self.target_pre = s
            .predelay_ms
            .map(|ms| (ms.max(0.0) * 0.001 * rate).clamp(0.0, max_frames));
        self.target_main = s
            .time_ms
            .map(|ms| (ms.max(0.0) * 0.001 * rate).clamp(1.0, max_frames));
        self.feedback = s.feedback.map(|f| f.clamp(0.0, 0.99));
        self.level = s.level.map(|l| l.clamp(0.0, 2.0));
        // Stability clamp: self-feedback + cross-feedback <= 1.0 on each side.
        let headroom = (1.0 - self.feedback[0])
            .min(1.0 - self.feedback[1])
            .max(0.0);
        self.cross = s.cross_feedback.clamp(0.0, headroom);
        self.width = s.width.clamp(0.0, 1.0);
    }

    /// Clears all delay buffers and snaps glides.
    pub fn reset(&mut self) {
        for b in &mut self.pre_buf {
            b.fill(0.0);
        }
        for b in &mut self.main_buf {
            b.fill(0.0);
        }
        self.write = 0;
        self.cur_pre = self.target_pre;
        self.cur_main = self.target_main;
        self.cur_feedback = self.feedback;
        self.cur_cross = self.cross;
        self.cur_level = self.level;
        self.cur_width = self.width;
        self.written = 0.0;
    }

    /// Maximum frames of delay memory currently held.
    #[must_use]
    pub fn memory(&self) -> usize {
        let pre = self.cur_pre[0]
            .max(self.cur_pre[1])
            .max(self.target_pre[0])
            .max(self.target_pre[1]);
        let main = self.cur_main[0]
            .max(self.cur_main[1])
            .max(self.target_main[0])
            .max(self.target_main[1]);
        (pre + main).ceil() as usize + 2
    }

    /// Peak magnitude written into any delay line during the last `process` call.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Advances delay-time glides over `frames` silent frames.
    pub fn idle(&mut self, frames: usize) {
        let decay = (1.0 - self.glide).powi(frames.min(i32::MAX as usize) as i32);
        for (c, &t) in self.cur_pre.iter_mut().zip(&self.target_pre) {
            *c = t + (*c - t) * decay;
        }
        for (c, &t) in self.cur_main.iter_mut().zip(&self.target_main) {
            *c = t + (*c - t) * decay;
        }
        self.written = 0.0;
    }

    #[inline]
    fn read_linear(buf: &[f32], mask: usize, write: usize, delay: f32) -> f32 {
        let i0 = delay as usize;
        let frac = delay - i0 as f32;
        let a = buf
            .get(write.wrapping_sub(i0) & mask)
            .copied()
            .unwrap_or(0.0);
        let b = buf
            .get(write.wrapping_sub(i0 + 1) & mask)
            .copied()
            .unwrap_or(0.0);
        a + frac * (b - a)
    }

    /// Processes stereo audio in place, replacing `io` with the wet output.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let mask = self.mask;
        let glide = self.glide;
        let gs = self.gain_smooth;
        let mut peak = 0.0f32;

        for frame in io {
            let [in_l, in_r] = *frame;
            let wi = self.write;

            // Step delay-time and gain/width glides for click-free live drags.
            for (c, &t) in self.cur_pre.iter_mut().zip(&self.target_pre) {
                *c += glide * (t - *c);
            }
            for (c, &t) in self.cur_main.iter_mut().zip(&self.target_main) {
                *c += glide * (t - *c);
            }
            for (c, &t) in self.cur_feedback.iter_mut().zip(&self.feedback) {
                *c += gs * (t - *c);
            }
            for (c, &t) in self.cur_level.iter_mut().zip(&self.level) {
                *c += gs * (t - *c);
            }
            self.cur_cross += gs * (self.cross - self.cur_cross);
            self.cur_width += gs * (self.width - self.cur_width);
            let fb_l = self.cur_feedback[0];
            let fb_r = self.cur_feedback[1];
            let cross = self.cur_cross;
            let lvl_l = self.cur_level[0];
            let lvl_r = self.cur_level[1];
            let w = self.cur_width;
            let inv_w = 1.0 - w;

            // Write inputs into pre-delay rings, then read pre-delayed samples
            // (0-frame predelay reads back the current sample with zero latency).
            if let Some(s) = self.pre_buf[0].get_mut(wi) {
                *s = in_l;
            }
            if let Some(s) = self.pre_buf[1].get_mut(wi) {
                *s = in_r;
            }
            let pre_l = Self::read_linear(&self.pre_buf[0], mask, wi, self.cur_pre[0]);
            let pre_r = Self::read_linear(&self.pre_buf[1], mask, wi, self.cur_pre[1]);

            // Read main delay lines before writing the next feedback sum.
            let main_l = Self::read_linear(&self.main_buf[0], mask, wi, self.cur_main[0]);
            let main_r = Self::read_linear(&self.main_buf[1], mask, wi, self.cur_main[1]);

            let next_l = flush(pre_l + fb_l * main_l + cross * main_r);
            let next_r = flush(pre_r + fb_r * main_r + cross * main_l);
            if let Some(s) = self.main_buf[0].get_mut(wi) {
                *s = next_l;
            }
            if let Some(s) = self.main_buf[1].get_mut(wi) {
                *s = next_r;
            }
            peak = peak
                .max(in_l.abs())
                .max(in_r.abs())
                .max(next_l.abs())
                .max(next_r.abs());

            // Independent wet levels + stereo width matrix.
            let ol = main_l * lvl_l;
            let or = main_r * lvl_r;
            *frame = [w * ol + inv_w * or, inv_w * ol + w * or];

            self.write = wi.wrapping_add(1) & mask;
        }
        self.written = peak;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(
        pre: [f32; 2],
        time: [f32; 2],
        fb: [f32; 2],
        cross: f32,
        level: [f32; 2],
        width: f32,
    ) -> DualDelaySettings {
        DualDelaySettings {
            predelay_ms: pre,
            time_ms: time,
            feedback: fb,
            cross_feedback: cross,
            level,
            width,
        }
    }

    #[test]
    fn independent_lr_echoes_land_at_exact_predelay_plus_time() {
        let rate = 48_000;
        // L: 10 ms pre + 20 ms main = 30 ms (1440 frames); R: 0 ms pre + 50 ms main = 50 ms (2400 frames).
        let mut d = DualDelay::new(
            settings([10.0, 0.0], [20.0, 50.0], [0.5, 0.0], 0.0, [1.0, 1.0], 1.0),
            rate,
            200.0,
        );
        let mut io = vec![[0.0; 2]; 4800];
        io[0] = [1.0, 1.0];
        d.process(&mut io);
        assert!(
            (io[1440][0] - 1.0).abs() < 1e-4,
            "L first echo: {}",
            io[1440][0]
        );
        assert!(
            (io[2400][0] - 0.5).abs() < 1e-4,
            "L repeat at 50ms: {}",
            io[2400][0]
        );
        assert!(
            (io[2400][1] - 1.0).abs() < 1e-4,
            "R first echo: {}",
            io[2400][1]
        );
    }

    #[test]
    fn cross_feedback_is_clamped_stable_and_width_swaps_or_monos() {
        let rate = 48_000;
        // Cross feedback requested at 1.0 with fb=0.8 must clamp to 0.2 and stay bounded.
        let mut d = DualDelay::new(
            settings([0.0, 0.0], [10.0, 10.0], [0.8, 0.8], 1.0, [1.0, 1.0], 0.0),
            rate,
            100.0,
        );
        let mut io = vec![[0.0; 2]; 48_000];
        io[0] = [1.0, 0.0];
        d.process(&mut io);
        assert!(io.iter().flatten().all(|v| v.is_finite() && v.abs() <= 2.0));
        // Width = 0 swaps L and R: left impulse appears on right channel at 10ms (480 frames).
        assert!((io[480][1] - 1.0).abs() < 1e-4 && io[480][0].abs() < 1e-4);
    }
}
