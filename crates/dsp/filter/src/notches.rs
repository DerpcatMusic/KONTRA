//! A phaser built from second-order allpasses: `sections` trapezoidal
//! state-variable allpasses (`x − 2k·band`) at one LFO-swept corner, around a
//! feedback loop, summed with the input. Each section makes one notch; its Q
//! sets how close the notches sit (Q ½ is two first-order stages at the
//! corner, above ½ they crowd it, below they spread out).

/// Flushes a state that would decay into the subnormal range.
fn flush(x: f32) -> f32 {
    if x.abs() < 1e-20 { 0.0 } else { x }
}

/// The most sections (notches) it runs.
pub const MAX_SECTIONS: usize = 12;
/// Frames between coefficient updates.
const TICK: usize = 32;

/// A notch phaser in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NotchSettings {
    /// 1..=[`MAX_SECTIONS`].
    pub sections: u8,
    /// The sections' corner with the LFO at its centre.
    pub hz: f32,
    pub q: f32,
    /// The LFO's swing of the corner, octaves either side.
    pub octaves: f32,
    /// The LFO's swing of the Q, octaves either side (`q·2^(±q_octaves)`).
    pub q_octaves: f32,
    /// The sections' output back into the first.
    pub feedback: f32,
    pub lfo_hz: f32,
    /// Cycles the right LFO runs ahead of the left.
    pub phase: f32,
    /// The share of `(input ± sections)/2` in the output; the rest is input.
    pub mix: f32,
    /// The sections enter the sum inverted: notches and peaks swap.
    pub invert: bool,
}

/// One channel's sections: their integrator states and shared coefficients.
#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    /// Each section's two integrator states.
    s: [[f32; 2]; MAX_SECTIONS],
    /// `g`, `k` and `1/(1 + g(g + k))`.
    c: [f32; 3],
    /// The last sections' output, for the loop.
    last: f32,
}

impl Channel {
    fn retune(&mut self, hz: f32, q: f32, rate: f32) {
        let g = (std::f32::consts::PI * hz.clamp(10.0, 0.45 * rate) / rate).tan();
        let k = 1.0 / q.clamp(0.02, 40.0);
        self.c = [g, k, 1.0 / (1.0 + g * (g + k))];
    }

    fn run(&mut self, x: f32, n: usize, feedback: f32) -> f32 {
        let [g, k, a1] = self.c;
        let mut u = x + feedback * self.last;
        for [ic1, ic2] in self.s.iter_mut().take(n) {
            let v3 = u - *ic2;
            let v1 = a1 * *ic1 + g * a1 * v3;
            let v2 = *ic2 + g * v1;
            *ic1 = flush(2.0 * v1 - *ic1);
            *ic2 = flush(2.0 * v2 - *ic2);
            u -= 2.0 * k * v1;
        }
        self.last = u;
        u
    }
}

/// See [`NotchSettings`]; it never allocates.
#[derive(Debug)]
pub struct Notches {
    s: NotchSettings,
    rate: f32,
    /// The left LFO's phase, cycles.
    cycle: f64,
    /// Frames until the next coefficient update.
    due: usize,
    ch: [Channel; 2],
    written: f32,
}

impl Notches {
    #[must_use]
    pub fn new(s: NotchSettings, rate: u32) -> Self {
        Self {
            s,
            rate: rate as f32,
            cycle: 0.0,
            due: 0,
            ch: [Channel::default(); 2],
            written: 0.0,
        }
    }

    pub fn set(&mut self, s: NotchSettings) {
        self.s = s;
    }

    /// The coefficients for the LFO at `cycle`.
    fn retune(&mut self, cycle: f64) {
        let s = self.s;
        for (ch, ahead) in self.ch.iter_mut().zip([0.0, f64::from(s.phase)]) {
            let lfo = (std::f64::consts::TAU * (cycle + ahead)).sin() as f32;
            let hz = s.hz * (s.octaves * lfo).exp2();
            ch.retune(hz, s.q * (s.q_octaves * lfo).exp2(), self.rate);
        }
    }

    /// Replaces `io` with its output (input included, per `mix`).
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let s = self.s;
        let n = usize::from(s.sections).clamp(1, MAX_SECTIONS);
        let wet = 0.5 * s.mix * if s.invert { -1.0 } else { 1.0 };
        // Below 1, so the loop round the allpasses (gain 1) cannot ring up.
        let feedback = s.feedback.clamp(-0.98, 0.98);
        let dry = 1.0 - s.mix + 0.5 * s.mix;
        let step = f64::from(s.lfo_hz / self.rate);
        let mut written = 0.0f32;
        for frame in io {
            if self.due == 0 {
                self.retune(self.cycle);
                self.due = TICK;
            }
            self.due -= 1;
            for (v, ch) in frame.iter_mut().zip(&mut self.ch) {
                let y = ch.run(*v, n, feedback);
                written = written.max(y.abs());
                *v = dry * *v + wet * y;
            }
            self.cycle = (self.cycle + step).fract();
        }
        self.written = written;
    }

    /// Frames after which silent input leaves it silent.
    // ponytail: 2 s covers its longest ring heard (feedback 95 %, Q 3.2 at
    // 46 Hz); derive it from the settings if a slower tail turns up.
    #[must_use]
    pub fn memory(&self) -> usize {
        2 * self.rate as usize
    }

    /// The loudest value the last [`Self::process`] put through the sections.
    #[must_use]
    pub fn written(&self) -> f32 {
        self.written
    }

    /// Silent frames: the LFO and the coefficient ticks move on as
    /// [`Self::process`] would move them, so waking matches never sleeping.
    pub fn idle(&mut self, frames: usize) {
        let step = f64::from(self.s.lfo_hz / self.rate);
        if frames > self.due {
            // The last tick inside, as `process` would have taken it.
            let last = self.due + (frames - 1 - self.due) / TICK * TICK;
            self.retune((self.cycle + step * last as f64).fract());
            self.due = TICK - (frames - last);
        } else {
            self.due -= frames;
        }
        self.cycle = (self.cycle + step * frames as f64).fract();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The magnitude at `hz` of the settled response, by a 1 s sine.
    fn gain(s: NotchSettings, hz: f32) -> f32 {
        let mut n = Notches::new(s, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|i| {
                let x = (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
                [x, x]
            })
            .collect();
        n.process(&mut io);
        let tail = &io[24_000..];
        (tail.iter().map(|f| f[0] * f[0]).sum::<f32>() / tail.len() as f32 * 2.0).sqrt()
    }

    #[test]
    fn its_notches_sit_where_kontakts_phasis_puts_them() {
        // Kontakt 8.13.1 Phasis, two notches at 622 Hz, spread ½ (Q ½): 258
        // and 1,500 Hz (impulse, modfx-audio/phasis); a peak at the corner.
        let s = NotchSettings {
            sections: 2,
            hz: 622.0,
            q: 0.5,
            octaves: 0.0,
            q_octaves: 0.0,
            feedback: 0.0,
            lfo_hz: 0.0,
            phase: 0.0,
            mix: 1.0,
            invert: false,
        };
        for hz in [258.0, 1500.0] {
            assert!(gain(s, hz) < 0.02, "{hz}: {}", gain(s, hz));
        }
        assert!((gain(s, 622.0) - 1.0).abs() < 0.02);
        // Inverted, they swap.
        let inverted = NotchSettings { invert: true, ..s };
        assert!(gain(inverted, 622.0) < 0.02);
    }
}
