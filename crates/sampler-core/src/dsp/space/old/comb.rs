#![allow(dead_code)]
// Comb/allpass layouts.
//
// [`CombAllpass`]: per channel a predelay feeds eight parallel damped
// feedback combs whose read taps sum into four serial allpasses. This is the
// structure the Kontakt legacy Reverb (0x15, `FUN_140aa3d10`) is proved to
// have: comb recurrence `h = d*delayed + (1-d)*h_prev`, write
// `input + feedback*h`; allpass `out = delayed - g*in`, write `in + g*out`.
// Delay lengths and the control laws are not recovered, so the lengths are
// the classic Schroeder/Moorer tuning.
//
// [`Diffuser`]: only the allpass chain, for smearing effects.

use super::common::{Allpass, Front, Line, one_pole};
use crate::Error;
use super::common::biased_one_pole32;
use sampler_ir::fx::space::Network;

const COMBS: usize = 8;
const ALLPASSES: usize = 4;
/// Comb and allpass lengths in samples at 44.1 kHz and size 1.
const COMB_LEN: [f32; COMBS] = [1116., 1188., 1277., 1356., 1422., 1491., 1557., 1617.];
const ALLPASS_LEN: [f32; ALLPASSES] = [556., 441., 341., 225.];
/// The right channel's lines are this much longer (44.1 kHz samples).
const SPREAD: f32 = 23.0;
/// Combs share the input; this keeps a unit impulse near unit total level.
const INPUT_GAIN: f32 = 0.125;

struct Comb {
    line: Line,
    len: usize,
    feedback: f32,
    store: f32,
}

struct Channel {
    front: Front,
    combs: [Comb; COMBS],
    allpass: [Allpass; ALLPASSES],
}

/// Two channels of eight combs and four allpasses.
pub(crate) struct CombAllpass {
    pos: usize,
    channels: [Channel; 2],
    damp: f32,
}

fn lengths(rate: f32, size: f32, spread: f32) -> impl Iterator<Item = [usize; 2]> {
    let scale = rate / 44_100.0 * size;
    COMB_LEN
        .into_iter()
        .chain(ALLPASS_LEN)
        .map(move |n| [n, n + spread].map(|n| ((n * scale) as usize).max(2)))
}

impl CombAllpass {
    pub(crate) fn new(s: &Network, rate: u32) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let rt60 = s.decay_seconds as f32;
        let all: Vec<[usize; 2]> = lengths(rate, s.size as f32, SPREAD).collect();
        let channel = |c: usize| Channel {
            front: Front::new(s.predelay_seconds, s.echo_feedback, s.input_cutoff_hz, rate),
            combs: std::array::from_fn(|i| {
                let len = all[i][c];
                Comb {
                    line: Line::new(len + 1),
                    len,
                    // Each pass through a comb of length d loses 60 dB over rt60.
                    feedback: 10f32.powf(-3.0 * len as f32 / (rt60 * rate)),
                    store: 0.0,
                }
            }),
            allpass: std::array::from_fn(|i| Allpass::new(all[COMBS + i][c], s.diffusion as f32)),
        };
        Ok(Self { pos: 0, channels: [channel(0), channel(1)], damp: one_pole(s.damping_hz as f32, rate) })
    }

    pub(crate) fn clear(&mut self) {
        for ch in &mut self.channels {
            ch.front.clear();
            for c in &mut ch.combs {
                c.line.clear();
                c.store = 0.0;
            }
            ch.allpass.iter_mut().for_each(Allpass::clear);
        }
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        sampler_simd::dispatch(
            #[inline(always)]
            || {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let pos = self.pos;
                    self.pos = pos.wrapping_add(1);
                    let [cl, cr] = &mut self.channels;
                    (*l, *r) = (cl.tick(pos, *l, self.damp), cr.tick(pos, *r, self.damp));
                }
            },
        );
    }
}

impl Channel {
    #[inline(always)]
    fn tick(&mut self, pos: usize, x: f32, damp: f32) -> f32 {
        let input = self.front.tick(pos, x) * INPUT_GAIN;
        let mut sum = 0.0;
        for c in &mut self.combs {
            let delayed = c.line.read(pos, c.len);
            c.store = biased_one_pole32(c.store, (delayed - c.store) * damp);
            c.line.write(pos, input + c.feedback * c.store);
            sum += delayed;
        }
        for a in &mut self.allpass {
            sum = a.tick(pos, sum);
        }
        sum
    }
}

/// Four allpasses per channel (lengths in ms at size 1).
const DIFFUSER_MS: [f32; ALLPASSES] = [5.0, 7.5, 11.0, 15.5];

pub(crate) struct Diffuser {
    pos: usize,
    channels: [(Front, [Allpass; ALLPASSES]); 2],
}

impl Diffuser {
    pub(crate) fn new(s: &Network, rate: u32) -> Result<Self, Error> {
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let channel = |c: usize| {
            (
                Front::new(s.predelay_seconds, s.echo_feedback, s.input_cutoff_hz, rate),
                std::array::from_fn(|i| {
                    let ms = DIFFUSER_MS[i] * (1.0 + 0.04 * c as f32);
                    Allpass::new(((ms * 0.001 * rate * s.size as f32) as usize).max(2), s.diffusion as f32)
                }),
            )
        };
        Ok(Self { pos: 0, channels: [channel(0), channel(1)] })
    }

    pub(crate) fn clear(&mut self) {
        for (front, chain) in &mut self.channels {
            front.clear();
            chain.iter_mut().for_each(Allpass::clear);
        }
    }

    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let pos = self.pos;
            self.pos = pos.wrapping_add(1);
            for (v, (front, chain)) in [l, r].into_iter().zip(&mut self.channels) {
                let mut x = front.tick(pos, *v);
                for a in chain {
                    x = a.tick(pos, x);
                }
                *v = x;
            }
        }
    }
}

