//! Kontakt Tone Machine render kernel (`BToneMachine` vtable slot 23, native `0x140ce3ff0`).
//!
//! Ported with the grain scheduler it calls after each control block (`0x140cda5d0`) and the
//! mark search that scheduler uses (`0x140ce5120`, `0x14053cb70`). Scalar semantics of the SIMD
//! width the oracle ran: the `DAT_14a2a5918 < 6` branch (16 lane pointers copied, no gather).
//!
//! Each voice has two read heads. Head `h` makes a direct read at `pos[h]`; when its crossfade
//! length is at least one frame it also reads at `xpos[h]` and blends the two with the cubic
//! window `w(2 * phase)`. Head 1 renders into the lanes, and a final stage crossfades the
//! caller's output into the lanes with the ramp `ramp`.
//!
//! Not wired: the source pass (native `0x14086e030`) is behind [`Source`], the mark table is
//! built by the loader (not recovered), and the parameter setters (`0x140cdaa10`,
//! `0x140cdaea0`) are not ported. See `docs/re/engine/k-tone-machine-kernel.md`.
#![allow(dead_code)] // not wired yet, see the module docs

/// Control curves are sampled once per `1 << CONTROL_SHIFT` frames (native `>> 5`).
const CONTROL_SHIFT: u32 = 5;
/// Largest number of frames one render call may produce (scratch size for lanes and taps).
pub const CAP: usize = 4096;
/// `DAT_1447149a8`: base of the level gain, `gain = 4^level`.
const GAIN_BASE: f64 = 4.0;
/// `DAT_14470bad8`: level clamp; the lower clamp is its negation (`DAT_144fd0dd8`).
const LEVEL_LIMIT: f32 = 1.5;
/// `DAT_1447148a4`: floor of the divisor curve (`0.001f`, bits `0x3a83126f`).
const DIVISOR_FLOOR: f32 = f32::from_bits(0x3a83_126f);
/// `DAT_14471489c`: ramp-step epsilon (`DAT_144fd0dd0` is its negation).
const RATE_EPS: f32 = 1e-6;
/// `DAT_144f38790`: minimum rate for the scheduler's forward mark walk.
const WALK_EPS: f64 = 1e-5;

/// Source frames: the native source pass (`0x14086e030`) as a trait.
pub trait Source {
    /// Channel count (native `FUN_140883ce0`, source `+0x1790`).
    fn channels(&self) -> usize;
    /// Reads up to `*remaining` frames at `*pos` into `dst[k][at..]` for every channel, advances
    /// `*pos` and decrements `*remaining` by the frames produced (native `*p7` and `*p10`).
    fn read(&mut self, pos: &mut i32, remaining: &mut i32, dst: &mut [Vec<f32>], at: usize);
}

/// The mark table the Tone object reaches through `+0x20`: header `head` (first dword, compared
/// against positions) and `marks` (the dword at `+8` of each 12-byte entry).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MarkTable {
    pub head: i32,
    pub marks: Vec<i32>,
}

/// Cubic in-out window of `x = 2 * phase` (native `fVar18`) for `x` in `[0, 2]`.
fn window(x: f32) -> f32 {
    if x <= 1.0 {
        let x = x.max(0.0);
        x * x * x * 0.5
    } else {
        let y = (2.0 - x).max(0.0);
        1.0 - y * y * y * 0.5
    }
}

/// `speed * rate * gain` as the kernel forms it (f64 product, f32 result).
fn rate32(speed: f32, rate: f64, gain: f32) -> f32 {
    (f64::from(speed) * rate * f64::from(gain)) as f32
}

/// Tone object state. Native offsets are in the comments; the constructor zeroes everything
/// the kernel does not name.
#[derive(Clone, Debug)]
pub struct Tone {
    /// `+0x30` (f64) rate ratio and `+0x38` (f32) speed, both set by the parameter setters.
    pub rate: f64,
    pub speed: f32,
    /// `+0x40`, `+0xc0`, `+0xc8`: per-32-frame control curves (empty means null).
    pub level_curve: Vec<f32>,
    pub divisor_curve: Vec<f32>,
    pub ratio_curve: Vec<f32>,
    /// `+0x3c` clamped level, `+0x48` last level, `+0x4c` cached `4^level`.
    level: f32,
    level_last: f32,
    gain: f32,
    /// `+0x50`: scheduler ratio, clamped to `[0, 1]` for the lengths.
    ratio: f32,
    /// `+0x58` (f64): time accumulator in frames.
    time: f64,
    /// `+0x60`: current mark index, `+0x64`: end time, `+0x68`/`+0x6c`: loop start and end
    /// (negative means no loop).
    mark: i32,
    end: i32,
    loop_start: i32,
    loop_end: i32,
    /// `+0x70` loop hit, `+0x71` head 0 fade suppressed, `+0xb4` done.
    loop_hit: bool,
    fade_off: bool,
    done: bool,
    /// `+0xbc` period in frames (`-1` before analysis), `+0xd0` divisor, `+0xd4` `period / divisor`.
    period: i32,
    divisor: f32,
    unit: f32,
    /// `+0xd8` ramp from 0 to 1, `+0xdc` ramp scale, `+0xe0` last mark, `+0xe4` reset base.
    ramp: f32,
    ramp_scale: f32,
    last_mark: i32,
    reset: i32,
    /// Per head (`+0x74/+0x78`, `+0x84/+0x88`, `+0x94/+0x98`, `+0x9c/+0xa0`, `+0xa4/+0xa8`,
    /// `+0xac/+0xb0`): direct position, crossfade position, direct length, crossfade length,
    /// window phase and phase increment.
    pos: [i32; 2],
    xpos: [i32; 2],
    len: [f32; 2],
    xlen: [f32; 2],
    phase: [f32; 2],
    inc: [f32; 2],
    /// Scratch: head 1 lanes (native `local_168`) and crossfade taps (`local_1e8`).
    lanes: Vec<Vec<f32>>,
    taps: Vec<Vec<f32>>,
}

impl Tone {
    /// Native constructor state (`FUN_140cd94c0`): gain `1.0`, period `-1`, divisor `1.0`.
    pub fn new(channels: usize) -> Self {
        Self {
            rate: 0.0,
            speed: 0.0,
            level_curve: Vec::new(),
            divisor_curve: Vec::new(),
            ratio_curve: Vec::new(),
            level: 0.0,
            level_last: 0.0,
            gain: 1.0,
            ratio: 0.0,
            time: 0.0,
            mark: -1,
            end: 0,
            loop_start: -1,
            loop_end: -1,
            loop_hit: false,
            fade_off: false,
            done: false,
            period: -1,
            divisor: 1.0,
            unit: 0.0,
            ramp: 0.0,
            ramp_scale: 0.0,
            last_mark: -1,
            reset: 0,
            pos: [0; 2],
            xpos: [0; 2],
            len: [0.0; 2],
            xlen: [0.0; 2],
            phase: [0.0; 2],
            inc: [0.0; 2],
            lanes: vec![vec![0.0; CAP]; channels],
            taps: vec![vec![0.0; CAP]; channels],
        }
    }

    pub fn done(&self) -> bool {
        self.done
    }

    /// Native `FUN_14053cb70`: mark value at `index`, clamped to the table (0 when absent).
    fn value(marks: Option<&MarkTable>, index: i32) -> i32 {
        match marks {
            Some(t) if !t.marks.is_empty() => {
                t.marks[index.max(0).min(t.marks.len() as i32 - 1) as usize]
            }
            _ => 0,
        }
    }

    /// Native `FUN_140ce5120`: the mark index at or after `pos`, searching from `start`.
    /// `nearest` skips the closer-neighbour tweak.
    fn search(&self, marks: Option<&MarkTable>, pos: i32, start: i32, nearest: bool) -> i32 {
        let last = marks.map_or(0, |t| t.marks.len() as i32) - 1;
        if marks.map_or(0, |t| t.head) <= pos {
            return last;
        }
        let mut i = start;
        while i > 0 {
            if Self::value(marks, i.min(last)) < pos {
                break;
            }
            i -= 1;
        }
        while i < last {
            let prev = Self::value(marks, i.max(0).min(last));
            let next = Self::value(marks, (i + 1).max(0).min(last));
            if next >= pos {
                if !nearest && next - pos < pos - prev && i + 1 < last {
                    return i + 1;
                }
                return i;
            }
            i += 1;
        }
        if i >= last - 1 && (self.loop_end < 0 || self.loop_start < 0) {
            return last;
        }
        0
    }

    /// Native `FUN_140cda5d0`: the grain scheduler. Returns `false` when the voice is finished.
    fn schedule(&mut self, marks: Option<&MarkTable>) -> bool {
        let base = self.pos[0] - self.reset;
        if self.loop_end >= 0 && self.loop_start >= 0 && self.loop_start <= self.time as i32 {
            self.loop_hit = true;
            self.time = f64::from(self.loop_end);
        }
        let last = marks.map_or(0, |t| t.marks.len() as i32) - 1;
        if f64::from(self.end) <= self.time
            || last <= self.mark
            || Self::value(marks, self.mark) > self.end
        {
            return false;
        }

        let m = self.search(marks, self.time as i32, self.mark, true);
        self.mark = m;
        let v = Self::value(marks, m);
        let at = if v <= self.end { v } else { self.end };
        self.pos[0] = at;
        self.reset = at;
        self.xpos[0] = at + base;
        self.unit = self.period as f32 / self.divisor;
        let m2 = self.search(marks, self.time as i32, self.mark, true);

        self.refresh_gain();
        let rate = rate32(self.speed, self.rate, self.gain);
        let mut m3 = m2;
        let mut t_end = self.time;
        if WALK_EPS <= f64::from(rate) {
            let step = f64::from(rate * self.unit);
            t_end = self.time + step;
            if t_end < f64::from(self.end) {
                loop {
                    if last <= m3 {
                        break;
                    }
                    m3 = self.search(marks, t_end as i32, m3, true);
                    if m3 != m2 {
                        break;
                    }
                    t_end += step;
                    if t_end >= f64::from(self.end) {
                        break;
                    }
                }
            }
        }
        let v3 = Self::value(marks, m3);
        let at = if v3 <= self.end { v3 } else { self.end };
        self.pos[1] = at;
        self.xpos[1] = at + base;
        if self.last_mark != m3 {
            self.ramp_scale = if t_end == self.time {
                0.0
            } else {
                (1.0 / (t_end - self.time)) as f32
            };
            self.ramp = 0.0;
            self.last_mark = m3;
        }

        // Window lengths: the ratio clamped to [0, 1] scales the crossfade length.
        let one = 1.0f32;
        let ratio = self.ratio.max(0.0).min(1.0);
        let head = marks.map_or(0, |t| t.head);
        for h in 0..2 {
            self.len[h] = self.unit;
            self.xlen[h] = self.unit;
            if (head as f32) < self.pos[h] as f32 + self.len[h] {
                self.len[h] = (head - self.pos[h]) as f32;
            }
            let mut x = self.xlen[h];
            if (head as f32) < self.xpos[h] as f32 + x {
                x = (head - self.xpos[h]) as f32;
            }
            x *= ratio;
            if x < one {
                x = one;
            }
            self.xlen[h] = x;
            self.phase[h] = 0.0;
            self.inc[h] = one / x;
        }
        true
    }

    /// Native `+0x3c` / `+0x48` / `+0x4c`: gain is `4^level`, recomputed only when the level changes.
    fn refresh_gain(&mut self) {
        if self.level != self.level_last {
            self.level_last = self.level;
            self.gain = GAIN_BASE.powf(f64::from(self.level)) as f32;
        }
    }

    /// Native `0x140ce3ff0`: renders `*frames` frames (decremented as they are produced) into
    /// `out`, one element per channel, starting at index 0.
    pub fn render(
        &mut self,
        src: &mut impl Source,
        marks: Option<&MarkTable>,
        frames: &mut i32,
        out: &mut [Vec<f32>],
    ) {
        assert!(
            *frames as usize <= CAP,
            "render block exceeds the Tone scratch"
        );
        let nch = src.channels();
        self.divisor = 1.0;
        self.level = 0.0;
        if self.reset > 0x7fff_fffe {
            self.reset = self.pos[0] - self.period;
        }
        // Output and lane offset (native `local_268` / `local_168` advance), tap offset,
        // and the frame positions of the three control curves (`local_2b4/2b0/2ac`).
        let (mut o, mut pos_level, mut pos_div, mut pos_ratio) = (0usize, 0usize, 0usize, 0usize);
        loop {
            if let Some(&v) = self.level_curve.get(pos_level >> CONTROL_SHIFT) {
                self.level = if v >= -LEVEL_LIMIT {
                    if v > LEVEL_LIMIT { LEVEL_LIMIT } else { v }
                } else {
                    -LEVEL_LIMIT
                };
            }
            if let Some(&v) = self.divisor_curve.get(pos_div >> CONTROL_SHIFT) {
                self.divisor = if v < DIVISOR_FLOOR { DIVISOR_FLOOR } else { v };
            }
            if let Some(&v) = self.ratio_curve.get(pos_ratio >> CONTROL_SHIFT) {
                self.ratio = v;
            }

            // Head loop. The direct read always runs; the crossfade read and blend run when the
            // crossfade length is at least one frame. Head 1 leaves its produced count for the mix.
            let mut produced = 0i32;
            let mut t = 0usize;
            for h in 0..2 {
                let left = *frames;
                let want = left.min(self.len[h] as i32).max(0);
                let mut p = self.pos[h];
                let mut rem = want;
                if h == 0 {
                    src.read(&mut p, &mut rem, &mut *out, o);
                } else {
                    src.read(&mut p, &mut rem, &mut self.lanes, o);
                }
                let direct = want - rem;
                self.pos[h] = p;
                self.len[h] -= direct as f32;
                produced = direct;

                if self.xlen[h] < 1.0 {
                    continue;
                }
                let want_x = left.min(self.xlen[h] as i32).max(0);
                let mut px = self.xpos[h];
                let mut rem_x = want_x;
                src.read(&mut px, &mut rem_x, &mut self.taps, t);
                let xdone = want_x - rem_x;
                self.xpos[h] = px;
                self.xlen[h] -= xdone as f32;
                let fade = if h == 0 && self.fade_off { 0.0 } else { 1.0f32 };
                for j in 0..xdone.max(0) as usize {
                    let w = window(self.phase[h] * 2.0);
                    for k in 0..nch {
                        let dst: &mut Vec<f32> = if h == 0 {
                            &mut out[k]
                        } else {
                            &mut self.lanes[k]
                        };
                        let a = w * dst[o + j];
                        dst[o + j] = (1.0 - w) * self.taps[k][t + j] * fade + a;
                    }
                    self.phase[h] += self.inc[h];
                }
                if h == 1 {
                    t += xdone.max(0) as usize;
                }
            }

            // Mix stage: the caller's output fades into the lanes with the ramp `+0xd8`, which
            // advances by the per-frame step `speed * rate * gain * ramp_scale`.
            self.refresh_gain();
            let mut step = rate32(self.speed, self.rate, self.gain);
            step = if RATE_EPS <= step || self.ramp_scale <= -RATE_EPS {
                step * self.ramp_scale
            } else {
                0.0
            };
            let n = produced.max(0);
            for f in 0..n as usize {
                for k in 0..nch {
                    out[k][o + f] =
                        (1.0 - self.ramp) * out[k][o + f] + self.lanes[k][o + f] * self.ramp;
                }
                self.ramp = (self.ramp + step).min(1.0);
            }
            if n > 0 {
                self.time += f64::from(rate32(self.speed, self.rate, self.gain) * n as f32);
            }

            *frames -= n;
            o += n as usize;
            pos_level += n as usize;
            pos_div += n as usize;
            pos_ratio += n as usize;
            if *frames < 1 {
                return;
            }
            if n > 0 {
                self.fade_off = false;
            }
            if !self.schedule(marks) {
                self.done = true;
                // Nothing produced and nothing scheduled: the native loop would spin here.
                if n == 0 {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source whose channel `k` frame `p` is `p + 1000 * k`.
    struct Ramp(usize);
    impl Source for Ramp {
        fn channels(&self) -> usize {
            self.0
        }
        fn read(&mut self, pos: &mut i32, remaining: &mut i32, dst: &mut [Vec<f32>], at: usize) {
            for j in 0..*remaining as usize {
                for (k, c) in dst.iter_mut().enumerate() {
                    c[at + j] = (*pos as usize + j + 1000 * k) as f32;
                }
            }
            *pos += *remaining;
            *remaining = 0;
        }
    }

    #[test]
    fn window_and_gain_laws_match_the_native_constants() {
        // Cubic in-out in x = 2 * phase: 0 at 0, 1/2 at the midpoint, 1 at the end.
        assert_eq!(window(0.0), 0.0);
        assert!((window(1.0) - 0.5).abs() < 1e-7);
        assert!((window(2.0) - 1.0).abs() < 1e-7);
        // Level is clamped to [-1.5, 1.5] and the gain is 4^level.
        let mut t = Tone::new(1);
        t.level_curve = vec![3.0];
        t.speed = 1.0;
        t.rate = 1.0;
        let mut frames = 0;
        t.render(&mut Ramp(1), None, &mut frames, &mut vec![Vec::new()]);
        assert_eq!(t.level, LEVEL_LIMIT);
        assert_eq!(t.gain, 8.0);
    }

    #[test]
    fn open_ramp_passes_head_one_straight_through() {
        // Ramp fully open (`ramp = 1`, no scale): the output is head 1's direct read.
        let mut t = Tone::new(2);
        t.len = [0.0, 64.0];
        t.xlen = [0.0, 0.0];
        t.ramp = 1.0;
        t.divisor = 1.0;
        let mut out = vec![vec![0.0f32; 64]; 2];
        let mut frames = 64;
        t.render(&mut Ramp(2), None, &mut frames, &mut out);
        assert_eq!(frames, 0);
        for k in 0..2 {
            for j in 0..64 {
                assert_eq!(out[k][j], (j + 1000 * k) as f32, "channel {k} frame {j}");
            }
        }
    }
}
