//! A pitch-shifting delay: on each side two read heads glide through a
//! line at a speed against the write head and are crossfaded on a parabola.
//! A head restarts when its fade turns on, placed so its grain is centred
//! on the side's delay; its speed sets the pitch (1 an echo, 2 an octave
//! up, negative backwards). The heads' sum goes back into the lines.
//! The heads read through a six-tap cubic, widened by the speed when it
//! speeds up so the read does not alias.

/// The settings in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShifterSettings {
    /// Each side's delay, the centre of its grains.
    pub ms: [f32; 2],
    /// Each side's heads' speed over the write head's.
    pub speed: [f32; 2],
    /// The heads' sum back into the lines.
    pub feedback: f32,
    /// The other side's share of each line's feedback.
    pub cross: f32,
}

/// The longest delay a side takes, ms.
pub const SHIFTER_MAX_MS: f32 = 4000.0;
/// A grain's travel at speed 0 (no head motion against the line), ms per
/// unit of motion: a still grain lasts 2 s.
const TRAVEL_MS: f32 = 500.0;
/// The crossfade's offset: just under ½, so a head's fade passes through
/// zero (and the head restarts) as its phase crosses 0.
const HALF: f32 = f32::from_bits(0x3eff_f2e5);

/// One side's clock: its fade phase and step, the heads' motion, and the
/// delay a head restarts at.
#[derive(Clone, Copy, Debug, Default)]
struct Side {
    phase: f32,
    sign: f32,
    step: f32,
    motion: f32,
    restart: f32,
}

/// A head `whole + frac` frames behind the newest frame, kept apart so a
/// long delay keeps its fraction's precision. It may run past the newest
/// frame, and then reads the line's far end.
#[derive(Clone, Copy, Debug, Default)]
struct Head {
    whole: i64,
    frac: f32,
    on: bool,
}

impl Head {
    fn at(frames: f32) -> Self {
        let whole = frames.floor();
        Self {
            whole: whole as i64,
            frac: frames - whole,
            on: true,
        }
    }

    fn advance(&mut self, motion: f32) {
        let a = motion + self.frac;
        let whole = a.floor();
        self.frac = a - whole;
        self.whole += whole as i64;
    }
}

#[derive(Debug)]
pub struct Shifter {
    s: ShifterSettings,
    rate: f32,
    ring: Box<[[f32; 2]]>,
    /// Where the newest frame is.
    at: usize,
    sides: [Side; 2],
    /// Two heads per side, left first.
    heads: [Head; 4],
    /// Each side's last output, fed back.
    last: [f32; 2],
    written: f32,
}

impl Shifter {
    /// Allocates the line: not real-time.
    #[must_use]
    pub fn new(s: ShifterSettings, rate: u32) -> Self {
        let rate = rate.max(1) as f32;
        // The longest delay, its grain's travel and the read's taps.
        let frames = (rate * 0.001 * (2.0 * SHIFTER_MAX_MS) + 64.0) as usize;
        let mut d = Self {
            s,
            rate,
            ring: vec![[0.0; 2]; frames.next_power_of_two()].into(),
            at: 0,
            sides: [Side::default(); 2],
            heads: [Head::default(); 4],
            last: [0.0; 2],
            written: 0.0,
        };
        d.set(s);
        for (k, head) in d.heads.iter_mut().enumerate() {
            *head = Head::at(d.sides.get(k / 2).map_or(0.0, |s| s.restart));
        }
        for side in &mut d.sides {
            side.sign = 0.5;
        }
        d
    }

    /// New settings; heads keep their place until they restart. Real-time.
    pub fn set(&mut self, s: ShifterSettings) {
        self.s = s;
        for (side, (&ms, &speed)) in self.sides.iter_mut().zip(s.ms.iter().zip(&s.speed)) {
            let ms = ms.clamp(1.0, SHIFTER_MAX_MS);
            let motion = 1.0 - speed;
            let travel = motion.abs() * TRAVEL_MS;
            let q = (travel / ms).max(0.5);
            side.motion = motion;
            side.step = (4.0 / self.rate) * q;
            side.restart = (ms - (travel / q).copysign(motion)) * (self.rate * 0.001);
        }
    }

    /// The line's length, frames: how long silent input can still echo.
    #[must_use]
    pub fn memory(&self) -> usize {
        self.ring.len()
    }

    /// The loudest value the last [`Self::process`] wrote into the line.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Runs `io` (wet out). Real-time.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let mask = self.ring.len() - 1;
        let (feedback, cross) = (self.s.feedback, self.s.cross);
        // A head speeding up widens its kernel; the left takes the right's
        // width when only the right speeds up (the native read's).
        let widen = |speed: f32| (speed.abs() > 1.0).then(|| 1.0 / speed.abs());
        let [l, r] = self.s.speed.map(widen);
        let scales = [l.or(r).unwrap_or(1.0), r.unwrap_or(1.0)];
        let mut written = 0.0f32;
        for frame in io {
            let mut fades = [0.0f32; 4];
            for (j, side) in self.sides.iter_mut().enumerate() {
                side.phase += side.step;
                if side.phase > 1.0 {
                    side.phase -= 2.0;
                    side.sign = -side.sign;
                }
                let v = (side.phase * side.phase - 1.0) * side.sign;
                for (n, fade) in [v + HALF, HALF - v].into_iter().enumerate() {
                    let k = 2 * j + n;
                    if let (Some(slot), Some(head)) = (fades.get_mut(k), self.heads.get_mut(k)) {
                        *slot = fade;
                        if fade <= 0.0 {
                            head.on = false;
                        } else if !head.on {
                            *head = Head::at(side.restart);
                        }
                    }
                }
            }
            let [l, r] = self.last;
            let input = [
                ((r - l) * cross + l) * feedback + frame[0],
                ((l - r) * cross + r) * feedback + frame[1],
            ];
            for (k, head) in self.heads.iter_mut().enumerate() {
                let motion = self.sides.get(k / 2).map_or(0.0, |s| s.motion);
                head.advance(motion);
            }
            self.at = (self.at + 1) & mask;
            if let Some(cell) = self.ring.get_mut(self.at) {
                *cell = input.map(flush);
            }
            written = written.max(input[0].abs()).max(input[1].abs());
            let mut out = [0.0f32; 2];
            for (k, (head, fade)) in self.heads.iter().zip(fades).enumerate() {
                let ch = k / 2;
                let scale = scales.get(ch).copied().unwrap_or(1.0);
                let y = read(&self.ring, self.at, *head, ch, scale);
                if let Some(o) = out.get_mut(ch) {
                    *o += fade.max(0.0) * y;
                }
            }
            self.last = out;
            *frame = out;
        }
        self.written = written;
    }

    /// `frames` silent frames: run as [`Self::process`] runs them, on
    /// `silence` as scratch, so the clocks and heads move on exactly.
    // ponytail: processes the silence; a closed-form skip if idle slots cost.
    pub fn idle(&mut self, silence: &mut [[f32; 2]]) {
        silence.fill([0.0; 2]);
        self.process(silence);
    }
}

/// The six-tap cubic at `t` taps from the read point, zero from 3 out.
fn kernel(t: f32) -> f32 {
    if t < 1.0 {
        t * t * (t * (4.0 / 3.0) - 7.0 / 3.0) + 1.0
    } else if t < 2.0 {
        ((3.0 - t * (7.0 / 12.0)) * t - 59.0 / 12.0) * t + 2.5
    } else if t < 3.0 {
        ((t * (1.0 / 12.0) - 2.0 / 3.0) * t + 1.75) * t - 1.5
    } else {
        0.0
    }
}

/// Channel `ch` of `ring` where `head` is behind `newest`, the kernel's
/// taps `scale` apart (1, or under it to widen it).
fn read(ring: &[[f32; 2]], newest: usize, head: Head, ch: usize, scale: f32) -> f32 {
    let mask = ring.len() as i64 - 1;
    let at = |back: i64| {
        let i = (newest as i64 - back) & mask;
        usize::try_from(i)
            .ok()
            .and_then(|i| ring.get(i))
            .and_then(|f| f.get(ch))
            .copied()
            .unwrap_or(0.0)
    };
    let mut acc = 0.0;
    // Newer frames, then older, each `t` taps out.
    let (mut t, mut back) = (scale * head.frac, head.whole);
    while t < 3.0 {
        acc += kernel(t) * at(back);
        t += scale;
        back -= 1;
    }
    let (mut t, mut back) = (scale * (1.0 - head.frac), head.whole + 1);
    while t < 3.0 {
        acc += kernel(t) * at(back);
        t += scale;
        back += 1;
    }
    scale * acc
}

fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, n: usize) -> Vec<[f32; 2]> {
        (0..n)
            .map(|i| {
                let v = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_still_head_echoes_at_its_delay_and_unity() {
        let s = ShifterSettings {
            ms: [100.0, 50.0],
            speed: [1.0; 2],
            feedback: 0.0,
            cross: 0.0,
        };
        let mut d = Shifter::new(s, 48_000);
        let mut io = vec![[0.0; 2]; 6000];
        io[0] = [1.0, 1.0];
        d.process(&mut io);
        assert!((io[4800][0] - 1.0).abs() < 1e-3, "{}", io[4800][0]);
        assert!((io[2400][1] - 1.0).abs() < 1e-3, "{}", io[2400][1]);
        assert!(io[4700][0].abs() < 1e-6);
    }

    #[test]
    fn speed_two_plays_an_octave_up() {
        let s = ShifterSettings {
            ms: [200.0; 2],
            speed: [2.0; 2],
            feedback: 0.0,
            cross: 0.0,
        };
        let mut d = Shifter::new(s, 48_000);
        let mut io = tone(500.0, 96_000);
        d.process(&mut io);
        // Zero crossings over the second second: 1 kHz has 2000.
        let tail = &io[48_000..];
        let crossings = tail
            .windows(2)
            .filter(|w| (w[0][0] < 0.0) != (w[1][0] < 0.0))
            .count();
        assert!((1990..=2010).contains(&crossings), "{crossings}");
    }
}
