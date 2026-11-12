//! [`CHOIR_VOICES`] swept stereo delay lines that feed back into one
//! another. Per frame: every line's input is the input plus a circulant mix
//! of the last reads; each voice's tap (its last read plus its input) turns
//! in the stereo field onto a bus, which a filter shapes; then each line
//! filters its input, writes it, and reads it back at a delay its sweep
//! sets: a bent triangle, plus an optional cosine, rounded by a one-pole.
//! Physical settings in [`ChoirSettings`].
//!
//! Every per-voice quantity is a row over the voices, so a frame's voice
//! loops run straight over `[f32; 7]` fields.

use kontra_dsp_core::{Biquad, Section, bilinear};

use crate::{flush, fract, sine};

/// The voices.
pub const CHOIR_VOICES: usize = 7;
const V: usize = CHOIR_VOICES;
/// The longest delay a line holds, s.
pub const CHOIR_MAX_S: f32 = 0.5;
/// The read's floor, frames: the cubic needs a newer frame.
const FLOOR: f32 = 2.0;
/// How long a rest delay glides to a new length.
/// ponytail: the reference ramps its knobs and re-runs its laws; this
/// glides what a knob moves (rest delays, sweep depth, bus levels and gain,
/// and the loop's mix) by one exponential, not the laws.
const GLIDE_S: f32 = 0.015;
/// Glides of the loop's mix after which it stands at its setting: 12 time
/// constants leave 6·10⁻⁶ of the step.
const SETTLE: f32 = 12.0;

type Row = [f32; V];

/// What glides to its setting: the rest delays and the sweep's depth
/// (frames), the bus rotation's cosine and sine rows times the levels, and
/// the gain out.
#[derive(Debug, Clone, Copy)]
struct Live {
    rest: Row,
    swing: Row,
    turn: [Row; 2],
    gain: f32,
}

/// Voice `v` of `r`.
fn at(r: &Row, v: usize) -> f32 {
    r.get(v).copied().unwrap_or(0.0)
}

/// `f` per voice.
fn row(f: impl FnMut(usize) -> f32) -> Row {
    std::array::from_fn(f)
}

/// A choir in physical units, a field per row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChoirSettings {
    /// Each line's delay at the sweep's low point, and how far the sweep's
    /// full travel moves it, s.
    pub delay_s: Row,
    pub swing_s: Row,
    /// The triangle LFO's rate and offset (cycles); it bends `u ∈ 0..1` to
    /// `(1 − b)·u + b·u²`.
    pub lfo_hz: Row,
    pub start: Row,
    pub bend: Row,
    /// A cosine LFO from its low point, adding `depth·(1 + ½·cos)` swings;
    /// depth 0 for none.
    pub wobble_hz: Row,
    pub wobble: Row,
    /// The one-pole on the sweep, Hz (infinite for none).
    pub smooth_hz: Row,
    /// Each line's input filter.
    pub filter: [Section; V],
    /// Each line's gain on what comes back to it.
    pub loop_gain: Row,
    /// The circulant's first row: line `i` takes
    /// `loop_gain[i]·Σₖ scatter[(k − i) mod 7]·read_k`.
    pub scatter: Row,
    /// The reads' sign, in the loop and on the bus.
    pub sign: f32,
    /// The input into the lines.
    pub input: f32,
    /// Each tap's level on the bus and its turn (radians, a rotation).
    pub level: Row,
    pub angle: Row,
    /// The bus's filter and its gain out.
    pub bus: Section,
    pub gain: f32,
}

/// See [`ChoirSettings`].
#[derive(Debug)]
pub struct Choir {
    s: ChoirSettings,
    rate: f32,
    /// [`V`] lines of `len` frames (a power of two), line `v` from `v·len`.
    ring: Box<[[f32; 2]]>,
    len: usize,
    /// Where the next frame goes.
    at: usize,
    /// `None` until the first frame, which starts at the settings.
    live: Option<Live>,
    glide: f32,
    /// The loop: the circulant times the loop gains and the sign, gliding
    /// to its setting for `mix_left` more frames.
    mix: [Row; V],
    mix_aim: [Row; V],
    mix_left: u32,
    alpha: Row,
    /// Both LFOs' steps and phases, cycles.
    step: [[f64; V]; 2],
    cycle: [[f64; V]; 2],
    /// The rounded sweep, 0..1 (more with the cosine).
    sweep: Row,
    /// The last reads, per channel.
    read: [Row; 2],
    /// The line filters, transposed direct form II with `a0 = 1`:
    /// `b0, b1, b2, a1, a2`, and per channel two states.
    c: [Row; 5],
    z: [[Row; 2]; 2],
    bus: Biquad,
    written: f32,
}

impl Choir {
    /// Lines of [`CHOIR_MAX_S`]. Not real-time: allocates.
    #[must_use]
    pub fn new(s: ChoirSettings, rate: u32) -> Self {
        let len = ((CHOIR_MAX_S * rate as f32) as usize + 8).next_power_of_two();
        let mut c = Self {
            s,
            rate: rate as f32,
            ring: vec![[0.0; 2]; len * V].into(),
            len,
            at: 0,
            live: None,
            glide: 1.0 - (-1.0 / (GLIDE_S * rate as f32)).exp(),
            mix: [[0.0; V]; V],
            mix_aim: [[0.0; V]; V],
            mix_left: 0,
            alpha: [0.0; V],
            step: [[0.0; V]; 2],
            cycle: [[0.0; V]; 2],
            sweep: [0.0; V],
            read: [[0.0; V]; 2],
            c: [[0.0; V]; 5],
            z: [[[0.0; V]; 2]; 2],
            bus: Biquad::analog(s.bus, rate),
            written: 0.0,
        };
        c.set(s);
        c
    }

    /// Changes the settings while it plays; never allocates.
    pub fn set(&mut self, s: ChoirSettings) {
        self.s = s;
        let rate = self.rate as u32;
        self.mix_aim = std::array::from_fn(|i| {
            row(|k| s.sign * at(&s.loop_gain, i) * at(&s.scatter, (k + V - i) % V))
        });
        // Before the first frame it starts at the settings; after, it glides.
        match self.live {
            None => (self.mix, self.mix_left) = (self.mix_aim, 0),
            Some(_) => self.mix_left = (SETTLE * GLIDE_S * self.rate) as u32,
        }
        self.alpha = s
            .smooth_hz
            .map(|hz| (-std::f32::consts::TAU * hz / self.rate).exp());
        self.step = [s.lfo_hz, s.wobble_hz].map(|r| r.map(|hz| f64::from(hz / self.rate)));
        let filters = s.filter.map(|f| bilinear(f, rate));
        self.c = std::array::from_fn(|k| {
            // b0, b1, b2 then a1, a2, each over a0.
            let k = if k < 3 { k } else { k + 1 };
            row(|v| {
                let f = filters.get(v).copied().unwrap_or_default();
                (f.get(k).copied().unwrap_or(0.0) / f.get(3).copied().unwrap_or(1.0)) as f32
            })
        });
        self.bus.retune(&Biquad::analog(s.bus, rate));
    }

    /// Each line's length, frames: how long silent input can still echo.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.len
    }

    /// The loudest value the last [`Self::process`] wrote into the lines.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Lets `frames` silent frames pass: the clocks and the write point
    /// move on, and whatever glides or rounds arrives (it sleeps for at
    /// least [`Self::memory`] frames).
    pub fn idle(&mut self, frames: usize) {
        self.live = Some(self.aim());
        (self.mix, self.mix_left) = (self.mix_aim, 0);
        self.at = (self.at + frames) & (self.len - 1);
        for (cycle, step) in self.cycle.iter_mut().zip(&self.step) {
            for (c, s) in cycle.iter_mut().zip(step) {
                *c = fract(*c + s * frames as f64);
            }
        }
        self.sweep = self.shape();
    }

    /// Where the settings put what glides.
    fn aim(&self) -> Live {
        let s = &self.s;
        Live {
            rest: s.delay_s.map(|d| d * self.rate),
            swing: s.swing_s.map(|d| d * self.rate),
            turn: [f32::cos, f32::sin].map(|f| row(|v| at(&s.level, v) * f(at(&s.angle, v)))),
            gain: s.gain,
        }
    }

    /// Each sweep before its rounding, at the clocks' phases.
    fn shape(&self) -> Row {
        let s = &self.s;
        let [p1, p2] = &self.cycle;
        row(|v| {
            let phase = fract(p1.get(v).copied().unwrap_or(0.0) + f64::from(at(&s.start, v)));
            let u = 1.0 - (1.0 - 2.0 * phase).abs() as f32;
            let bend = at(&s.bend, v);
            let wobble = at(&s.wobble, v);
            let cos = match wobble == 0.0 {
                true => 0.0,
                false => {
                    let p = p2.get(v).copied().unwrap_or(0.0);
                    (1.0 - 0.5 * sine(p + 0.25) as f32) * wobble
                }
            };
            (1.0 - bend) * u + bend * u * u + cos
        })
    }

    /// Line `v`, both channels, `d` frames back from the newest frame (at
    /// least [`FLOOR`]), 4-point cubic.
    fn tap(&self, v: usize, d: f32) -> [f32; 2] {
        let whole = d as usize;
        let t = d - whole as f32;
        let mask = self.len - 1;
        let line = self.ring.get(v * self.len..).unwrap_or_default();
        // The newest frame sits at `at − 1`: one back is it (no delay).
        let back = |b: usize| {
            line.get((self.at + self.len - b) & mask)
                .copied()
                .unwrap_or([0.0; 2])
        };
        let ([m0, m1], [a0, a1], [b0, b1], [c0, c1]) = (
            back(whole - 1),
            back(whole),
            back(whole + 1),
            back(whole + 2),
        );
        // Catmull-Rom from y0 (t = 0) to y1 (t = 1).
        let cubic = |ym1: f32, y0: f32, y1: f32, y2: f32| {
            let a = -0.5 * ym1 + 1.5 * y0 - 1.5 * y1 + 0.5 * y2;
            let b = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
            let k = -0.5 * ym1 + 0.5 * y1;
            ((a * t + b) * t + k) * t + y0
        };
        [cubic(m0, a0, b0, c0), cubic(m1, a1, b1, c1)]
    }

    /// Replaces `io` with the wet signal.
    ///
    /// ponytail: frame by frame in f32, the voice loops straight over the
    /// `[f32; 7]` rows (no lane-wide vectors yet).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let aim = self.aim();
        let mut live = self.live.unwrap_or(aim);
        let glide = self.glide;
        let toward = |now: &mut Row, aim: &Row| {
            for (now, aim) in now.iter_mut().zip(aim) {
                *now += (aim - *now) * glide;
            }
        };
        let longest = (self.len - 4) as f32;
        let mut written = 0.0f32;
        let dot = |a: &Row, b: &Row| a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>();
        for frame in io.iter_mut() {
            // The lines' inputs, then the taps onto the bus.
            let mut into = [[0.0f32; V]; 2];
            let mut taps = [[0.0f32; V]; 2];
            let per_channel = into.iter_mut().zip(&mut taps).zip(&self.read).zip(*frame);
            for (((into, taps), read), x) in per_channel {
                for (into, m) in into.iter_mut().zip(&self.mix) {
                    *into = dot(m, read) + s.input * x;
                }
                for ((tap, &read), &into) in taps.iter_mut().zip(read).zip(&*into) {
                    *tap = s.sign * read + into;
                }
            }
            let [l, r] = &taps;
            let [cos, sin] = &live.turn;
            let mut bus = [dot(cos, l) - dot(sin, r), dot(sin, l) + dot(cos, r)];
            self.bus.process(std::slice::from_mut(&mut bus));
            *frame = bus.map(|b| b * live.gain);
            // Each line's filter, into the ring.
            let [b0, b1, b2, a1, a2] = &self.c;
            for ((x, [z1, z2]), ch) in into.iter().zip(&mut self.z).zip(0..) {
                let coeffs = b0.iter().zip(b1).zip(b2).zip(a1).zip(a2);
                let states = x.iter().zip(z1.iter_mut()).zip(z2.iter_mut());
                let lines = self.ring.chunks_exact_mut(self.len);
                for ((((((&b0, &b1), &b2), &a1), &a2), ((&x, z1), z2)), line) in
                    coeffs.zip(states).zip(lines)
                {
                    let y = b0 * x + *z1;
                    *z1 = flush(b1 * x - a1 * y + *z2);
                    *z2 = flush(b2 * x - a2 * y);
                    written = written.max(y.abs());
                    if let Some(slot) = line.get_mut(self.at).and_then(|f| f.get_mut(ch)) {
                        *slot = y;
                    }
                }
            }
            self.at = (self.at + 1) & (self.len - 1);
            // Each sweep, then the reads.
            let shape = self.shape();
            for ((sweep, shape), alpha) in self.sweep.iter_mut().zip(shape).zip(&self.alpha) {
                *sweep = shape + (*sweep - shape) * alpha;
            }
            toward(&mut live.rest, &aim.rest);
            toward(&mut live.swing, &aim.swing);
            for (now, aim) in live.turn.iter_mut().zip(&aim.turn) {
                toward(now, aim);
            }
            live.gain += (aim.gain - live.gain) * glide;
            if self.mix_left > 0 {
                self.mix_left -= 1;
                match self.mix_left {
                    0 => self.mix = self.mix_aim,
                    _ => {
                        for (now, aim) in self.mix.iter_mut().zip(&self.mix_aim) {
                            toward(now, aim);
                        }
                    }
                }
            }
            let mut read = [[0.0f32; V]; 2];
            let [read_l, read_r] = &mut read;
            let voices = (live.rest.iter().zip(&live.swing).zip(&self.sweep))
                .zip(read_l.iter_mut().zip(read_r.iter_mut()));
            for (v, (((&rest, &swing), &sweep), (l, r))) in voices.enumerate() {
                // `max` maps NaN to the floor.
                let d = (rest + swing * sweep).max(FLOOR).min(longest);
                [*l, *r] = self.tap(v, d);
            }
            self.read = read;
            for (cycle, step) in self.cycle.iter_mut().zip(&self.step) {
                for (c, s) in cycle.iter_mut().zip(step) {
                    let next = *c + s;
                    *c = if next >= 1.0 { fract(next) } else { next };
                }
            }
        }
        self.live = Some(live);
        self.written = written;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_reads_back_at_its_rest_delay_and_feeds_back_through_the_circulant() {
        let flat = Section {
            hz: 1000.0,
            num: [1.0, 1.0, 1.0],
            den: [1.0, 1.0, 1.0],
        };
        let mut level = [0.0; V];
        level[0] = 1.0;
        let s = ChoirSettings {
            // Line 1 is half as long as the others.
            delay_s: [0.01, 0.005, 0.01, 0.01, 0.01, 0.01, 0.01],
            swing_s: [0.0; V],
            lfo_hz: [0.0; V],
            start: [0.0; V],
            bend: [0.0; V],
            wobble_hz: [0.0; V],
            wobble: [0.0; V],
            smooth_hz: [f32::INFINITY; V],
            filter: [flat; V],
            loop_gain: [0.5; V],
            // Line 0 takes line 1's read only.
            scatter: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            sign: 1.0,
            input: 1.0,
            level,
            angle: [0.0; V],
            bus: flat,
            gain: 1.0,
        };
        let mut c = Choir::new(s, 48_000);
        let mut io = vec![[0.0f32; 2]; 1200];
        io[0] = [1.0, 0.0];
        c.process(&mut io);
        // Straight through; line 1's read fed back into line 0 at half
        // (240 frames); line 0's own read (480); then both again (720).
        let left: Vec<_> = io.iter().map(|f| f[0]).collect();
        for (at, v) in [(0, 1.0), (240, 0.5), (480, 1.0), (720, 0.75)] {
            assert!((left[at] - v).abs() < 1e-5, "{at}: {}", left[at]);
        }
        let rest: f32 = left.iter().map(|v| v.abs()).sum::<f32>() - 3.25;
        assert!(rest.abs() < 1e-4, "{rest}");

        // A knob glides: the gain set to nothing is still there a frame on.
        c.idle(c.memory());
        c.set(ChoirSettings { gain: 0.0, ..s });
        let mut one = [[1.0f32, 0.0]];
        c.process(&mut one);
        assert!(one[0][0] > 0.99, "{}", one[0][0]);

        // Woken, a slowly rounded sweep stands where its clock does.
        c.set(ChoirSettings {
            lfo_hz: [1.0; V],
            smooth_hz: [0.01; V],
            ..s
        });
        c.idle(12_000);
        assert!((c.sweep[0] - 0.5).abs() < 1e-3, "{}", c.sweep[0]);
    }
}
