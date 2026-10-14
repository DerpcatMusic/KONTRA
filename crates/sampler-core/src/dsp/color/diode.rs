//! UVI Workstation 4.0.9 DiodeClipper as the color chain's shaper (kernel 0x1412584e0,
//! solver 0x141258040, prepare 0x141257ee0): a `tanh` input stage into an RC high-pass /
//! low-pass network around an asymmetric diode pair solved by damped Newton steps in f64
//! (at most 10, step clamped to 0.5, voltage to 1.5), then a 1 Hz DC blocker and the output
//! gain in f32 (model uvicolor/diode.py, checked against the emulated kernel).
use super::super::Planar;
pub use sampler_ir::fx::color::Diode;

const TWO_PI: f64 = 6.2831854820251465; // 0x141e6ec58
const K: f64 = 22.075055187637968; // 0x141e6ec60
const IS: f64 = 5.5461999999999995e-06; // 0x141e6ec40

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Engine {
    g: f64,
    wh: f64,
    wt: f64,
    a: f64,
    dc: f32,
    gout: f32,
}

/// Per channel `[s, y]` (f64) and the DC blocker's memory.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct State {
    solver: [[f64; 2]; 2],
    dc: [f32; 2],
}

impl State {
    pub(super) fn finite(&self) -> bool {
        self.solver.iter().flatten().all(|v| v.is_finite()) && self.dc.iter().all(|v| v.is_finite())
    }
}

impl Engine {
    pub(super) fn new(d: Diode, rate: u32) -> Self {
        let fs = rate as f32;
        let inv = 1.0 / f64::from(fs);
        let pole = ((1.0 / f64::from(fs)) * -std::f64::consts::TAU).exp() as f32;
        Self {
            g: 10f64.powf(f64::from(d.drive) * 0.05),
            wh: f64::from(d.high_pass) * TWO_PI * inv,
            wt: f64::from(d.tone) * TWO_PI * inv,
            a: 10f64.powf(f64::from(d.asymmetry)),
            dc: 1.0 - pole,
            gout: 10f64.powf(f64::from(d.output_gain) * 0.05) as f32,
        }
    }

    pub(super) fn process(&self, state: &mut State, block: &mut Planar, len: usize) {
        let (wh, wt) = (self.wh, self.wt);
        let ka = self.a * K;
        let d = wt + wh + 1.0;
        let c1 = wt * IS;
        let (c2, c3) = (c1 * K, c1 * ka);
        for ch in 0..2 {
            let [mut s, mut y] = state.solver[ch];
            let mut z = state.dc[ch];
            for v in &mut block[ch][..len] {
                let u = (f64::from(*v as f32) * self.g * 0.2222222222222222).tanh() * 4.5;
                let r = ((u * wt + y * wh) - s * wt) + y;
                let mut next = s;
                for it in 1.. {
                    let em = (-(y * ka)).exp() * (1.0 / d);
                    let ep = (y * ka).exp();
                    let eb = (y * K * (self.a + 1.0)).exp();
                    let t1 = c2 * eb;
                    let t2 = (eb - 1.0) * c1;
                    let j = ((c3 + ep + t1) * wh + ep * wt + t1 + c3 + ep) * em;
                    if j.abs() < 1e-10 {
                        break;
                    }
                    let step = ((((y * d * ep + t2 * wh) - r * ep) + t2) * em) / -j;
                    let step = if step > 0.5 { 0.5 } else if step <= -0.5 { -0.5 } else { step };
                    y += step;
                    y = if y > 1.5 { 1.5 } else if y <= -1.5 { -1.5 } else { y };
                    next = ((u - y) * wh + s) / (wh + 1.0);
                    if step.abs() < 0.001 || it > 9 {
                        break;
                    }
                }
                s = next;
                let out = y as f32;
                z += (out - z) * self.dc;
                *v = f64::from((out - z) * self.gout);
            }
            state.solver[ch] = [s, y];
            state.dc[ch] = z;
        }
    }
}
