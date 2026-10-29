//! Three explicit (Chamberlin) state-variable stages in series. Each stage
//! morphs from low through band to high pass and blends into the signal by
//! its wet amount; its band state passes a soft knee in the loop. The output
//! is normalised by the stages' resonant peaks and glides to its gain.

/// One stage: q (damping `2 − 2q`), the highest integrator gain it runs at,
/// its resonant peak at full wet, morph (0 low, 0.5 band, 1 high) and wet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CascadeStage {
    pub q: f32,
    pub limit: f32,
    pub peak: f32,
    pub morph: f32,
    pub wet: f32,
}

/// Stage 0's corner in Hz, stages 1 and 2 `shifts` octaves above the one
/// before, and the output gain (linear).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CascadeSettings {
    pub hz: f32,
    pub shifts: [f32; 2],
    pub stages: [CascadeStage; 3],
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Coefficients {
    g: f32,
    d: f32,
    /// Low, band and high pass weights.
    weights: [f32; 3],
    wet: f32,
}

/// The knee on each stage's band state: `(a − c·|s|)·s`, `s` within ±4.
const KNEE: (f32, f32) = (0.06, 0.0075);
/// The output gain's glide, per second.
const GLIDE: f32 = 20.0;

/// Zero for a denormal-range state, so a decaying tail stays cheap.
fn flush(v: f32) -> f32 {
    if v.abs() < 1e-20 { 0.0 } else { v }
}

/// A stereo cascade with its stage states and output glide.
#[derive(Clone, Debug, PartialEq)]
pub struct Cascade {
    stages: [Coefficients; 3],
    gain: f32,
    glide: f32,
    /// Radians per sample per Hz.
    per_hz: f32,
    /// Per channel: band and low state per stage, then the glided gain.
    state: [[f32; 7]; 2],
}

impl Cascade {
    /// At `settings` from the start (the gain does not glide in).
    #[must_use]
    pub fn new(settings: CascadeSettings, rate: u32) -> Self {
        let mut c = Self {
            stages: [Coefficients::default(); 3],
            gain: 0.0,
            glide: GLIDE / rate.max(1) as f32,
            per_hz: std::f32::consts::TAU / rate.max(1) as f32,
            state: [[0.0; 7]; 2],
        };
        c.set(settings);
        for s in &mut c.state {
            s[6] = c.gain;
        }
        c
    }

    /// Retunes to `settings`; the gain glides there. No allocation.
    pub fn set(&mut self, s: CascadeSettings) {
        let per_hz = self.per_hz;
        let [s1, s2] = s.shifts;
        let hz = [s.hz, s.hz * s1.exp2(), s.hz * (s1 + s2).exp2()];
        let mut comp = [0.0f32; 3];
        for (n, (st, c)) in s.stages.iter().zip(&mut self.stages).enumerate() {
            let g = (hz[n] * per_hz).min(st.limit).max(f32::MIN_POSITIVE);
            let t = st.morph.clamp(0.0, 1.0);
            let weights = if t < 0.5 {
                [1.0 - 2.0 * t, 2.0 * t, 0.0]
            } else {
                [0.0, 2.0 - 2.0 * t, 2.0 * t - 1.0]
            };
            let wet = st.wet.clamp(0.0, 1.0);
            *c = Coefficients {
                g,
                d: 2.0 - 2.0 * st.q,
                weights,
                wet,
            };
            comp[n] = wet * st.peak + (1.0 - wet);
        }
        // Pairs of stages whose peaks meet: each term shrinks as their
        // corners part (by `6·ratio − 5`).
        let [ga, gb, gc] = self.stages.map(|c| c.g);
        let [a1, b1, c1] = comp.map(|c| c - 1.0);
        let pair = |x: f32, y: f32, ratio: f32| {
            let d = ratio * 6.0 - 5.0;
            if d.abs() > 1e-12 { x * y / d } else { 0.0 }
        };
        let terms = pair(a1, b1, gb / ga) + pair(c1, b1, gc / gb) + pair(c1, a1, gc / ga);
        let sum = terms + comp[1] + comp[2] + comp[0];
        let norm = if sum.abs() > 1e-12 { 3.0 / sum } else { 1.0 };
        self.gain = s.gain * norm;
    }

    /// Filters `io` in place.
    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        let (stages, gain, glide) = (self.stages, self.gain, self.glide);
        for frame in io.iter_mut() {
            for (x, s) in frame.iter_mut().zip(&mut self.state) {
                let mut y = *x;
                for (c, st) in stages.iter().zip(s.chunks_exact_mut(2)) {
                    let (b, l) = (st[0], st[1]);
                    let knee = b.clamp(-4.0, 4.0);
                    let hp = (KNEE.0 - knee.abs() * KNEE.1) * knee + (y - (b * c.d + l));
                    let b = hp * c.g + b;
                    let l = b * c.g + l;
                    st[0] = flush(b);
                    st[1] = flush(l);
                    let mode = b * c.weights[1] + hp * c.weights[2] + l * c.weights[0];
                    y += (mode - y) * c.wet;
                }
                s[6] += (gain - s[6]) * glide;
                *x = s[6] * y;
            }
        }
    }
}
