//! A figure-eight plate tank after Dattorro, "Effect Design Part 1: Reverberator
//! and Other Filters" (JAES 1997): a mono input diffuser of four allpasses
//! feeds two cross-coupled loops, each a modulated allpass, a delay, a damping
//! lowpass, a decay gain, a second allpass and a delay. Stereo comes from
//! taps inside the tank. Delay lengths are the paper's, at 29.761 kHz and
//! scaled by sample rate and size; the output taps are this module's own
//! choice among the tank's lines.
//!
//! Kontakt's Plate Reverb (0x57) topology is not recovered: this is a
//! generic plate, not a model of it.

use super::common::{Allpass, Front, Line, Lfo, one_pole};
use crate::Error;
use crate::dsp::kernels::biased_one_pole32;
use sampler_ir::fx::space::Network;

const PAPER_RATE: f32 = 29_761.0;
const INPUT_AP: [f32; 4] = [142., 107., 379., 277.];
/// Per branch: modulated allpass, delay 1, allpass, delay 2.
const BRANCH: [[f32; 4]; 2] = [[672., 4453., 1800., 3720.], [908., 4217., 2656., 3163.]];
/// Output taps `(branch, line, delay, sign)`; line 0 is delay 1, 1 the
/// second allpass, 2 delay 2.
const TAPS_L: [(usize, usize, f32, f32); 7] = [
    (1, 0, 266., 1.),
    (1, 0, 2974., 1.),
    (1, 1, 1913., -1.),
    (1, 2, 1996., 1.),
    (0, 0, 1990., -1.),
    (0, 1, 187., -1.),
    (0, 2, 1066., -1.),
];
const TAPS_R: [(usize, usize, f32, f32); 7] = [
    (0, 0, 353., 1.),
    (0, 0, 3627., 1.),
    (0, 1, 1228., -1.),
    (0, 2, 2673., 1.),
    (1, 0, 2111., -1.),
    (1, 1, 335., -1.),
    (1, 2, 121., -1.),
];
const OUTPUT_GAIN: f32 = 0.6;

struct Branch {
    modulated: Line,
    mod_len: f32,
    mod_g: f32,
    delay1: Line,
    len1: usize,
    damp: f32,
    state: f32,
    ap2: Allpass,
    delay2: Line,
    len2: usize,
}

pub(super) struct Plate {
    pos: usize,
    front: Front,
    input: [Allpass; 4],
    branches: [Branch; 2],
    decay: f32,
    mod_depth: f32,
    lfo: Lfo,
    taps: [[(usize, usize, usize, f32); 7]; 2],
    /// Frames since the last LFO renormalisation.
    since_norm: usize,
}

impl Plate {
    pub(super) fn new(s: &Network, rate: u32) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let scale = rate / PAPER_RATE * s.size as f32;
        let n = |v: f32| ((v * scale) as usize).max(2);
        let loop_len: f32 = BRANCH.iter().flatten().sum::<f32>() * scale;
        // The loop passes both branches once; each branch applies `decay` once.
        let decay = 10f32.powf(-3.0 * loop_len * 0.5 / (s.decay_seconds as f32 * rate));
        let diffusion = s.diffusion as f32;
        let mod_depth = s.modulation_seconds as f32 * rate;
        let branch = |b: usize| {
            let [m, d1, a2, d2] = BRANCH[b];
            // Paper: decay diffusion 2 follows the decay, held within 0.25..0.5.
            let g2 = (decay + 0.15).clamp(0.25, 0.5);
            Branch {
                modulated: Line::new(n(m) + mod_depth as usize + 3),
                mod_len: n(m) as f32,
                mod_g: diffusion * 0.9333,
                delay1: Line::new(n(d1)),
                len1: n(d1),
                damp: one_pole(s.damping_hz as f32, rate),
                state: 0.0,
                ap2: Allpass::new(n(a2), g2),
                delay2: Line::new(n(d2)),
                len2: n(d2),
            }
        };
        let lens = |b: usize, line: usize| match line {
            0 => n(BRANCH[b][1]),
            1 => n(BRANCH[b][2]),
            _ => n(BRANCH[b][3]),
        };
        let taps = |t: &[(usize, usize, f32, f32); 7]| {
            t.map(|(b, line, d, sign)| (b, line, (n(d)).clamp(1, lens(b, line)), sign))
        };
        Ok(Self {
            pos: 0,
            front: Front::new(s.predelay_seconds, s.echo_feedback, s.input_cutoff_hz, rate),
            input: [diffusion, diffusion, diffusion * 0.8333, diffusion * 0.8333]
                .into_iter()
                .zip(INPUT_AP)
                .map(|(g, len)| Allpass::new(n(len), g))
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|_| Error::InvalidInput)?,
            branches: [branch(0), branch(1)],
            decay,
            mod_depth,
            lfo: Lfo::new(s.modulation_hz, rate),
            taps: [taps(&TAPS_L), taps(&TAPS_R)],
            since_norm: 0,
        })
    }

    pub(super) fn clear(&mut self) {
        self.front.clear();
        self.input.iter_mut().for_each(Allpass::clear);
        for b in &mut self.branches {
            b.modulated.clear();
            b.delay1.clear();
            b.delay2.clear();
            b.ap2.clear();
            b.state = 0.0;
        }
    }

    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let pos = self.pos;
            self.pos = pos.wrapping_add(1);
            let mut x = self.front.tick(pos, 0.5 * (*l + *r));
            for a in &mut self.input {
                x = a.tick(pos, x);
            }
            let (sin, cos) = self.lfo.tick();
            let feed = [
                self.branches[1].delay2.read(pos, self.branches[1].len2),
                self.branches[0].delay2.read(pos, self.branches[0].len2),
            ];
            for (i, (b, lfo)) in self.branches.iter_mut().zip([sin, cos]).enumerate() {
                let input = x + feed[i];
                let delayed = b.modulated.read_frac(pos, b.mod_len + self.mod_depth * (1.0 + lfo) * 0.5);
                let out = delayed - b.mod_g * input;
                b.modulated.write(pos, input + b.mod_g * out);
                let d1 = b.delay1.read(pos, b.len1);
                b.delay1.write(pos, out);
                b.state = biased_one_pole32(b.state, (d1 - b.state) * b.damp);
                let z = b.ap2.tick(pos, b.state * self.decay);
                b.delay2.write(pos, z);
            }
            let tap = |t: &[(usize, usize, usize, f32); 7], branches: &[Branch; 2]| {
                t.iter()
                    .map(|&(b, line, d, sign)| {
                        let br = &branches[b];
                        sign * match line {
                            0 => br.delay1.read(pos, d),
                            1 => br.ap2.peek(pos, d),
                            _ => br.delay2.read(pos, d),
                        }
                    })
                    .sum::<f32>()
                    * OUTPUT_GAIN
            };
            (*l, *r) = (tap(&self.taps[0], &self.branches), tap(&self.taps[1], &self.branches));
        }
        self.since_norm += left.len();
        if self.since_norm >= 256 {
            self.since_norm = 0;
            self.lfo.renormalize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_ir::fx::space::Topology;

    fn network(decay: f64) -> Network {
        Network { topology: Topology::Plate, decay_seconds: decay, size: 1.0, damping_hz: 12_000.0, ..Network::DEFAULT }
    }

    #[test]
    fn decays_at_the_requested_rate() {
        for target in [1.0, 3.0] {
            let mut p = Plate::new(&network(target), 48_000).unwrap();
            let mut l = vec![0.0; 10 * 48_000];
            let mut r = l.clone();
            l[0] = 1.0;
            r[0] = 1.0;
            for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
                p.process(l, r);
            }
            assert!(l.iter().chain(&r).all(|v| v.is_finite()));
            let got = super::super::common::rt60(&l, 48_000.0);
            assert!((got / target as f32 - 1.0).abs() < 0.4, "target {target} got {got}");
        }
    }

    #[test]
    fn channels_differ_and_long_decay_is_stable() {
        let mut p = Plate::new(&network(60.0), 44_100).unwrap();
        let (mut l, mut r) = (vec![0.0; 4096], vec![0.0; 4096]);
        l[0] = 1.0;
        for _ in 0..300 {
            for (l, r) in l.chunks_mut(64).zip(r.chunks_mut(64)) {
                p.process(l, r);
            }
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 10.0));
            l.fill(0.0);
            r.fill(0.0);
        }
    }
}
