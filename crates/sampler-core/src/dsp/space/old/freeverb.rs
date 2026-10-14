#![allow(dead_code)]
// The Kontakt legacy Reverb (0x15) network, exact to `FUN_140aa3d10` (process),
// `FUN_14094c760` (delay lengths) and the case-0x3e setter in `FUN_1409619b0`.
//
// Per channel: input channel mix, gain, a lowpass, a predelay, eight damped
// feedback combs summed into four serial allpasses. The comb and allpass
// lengths are `int((channel_offset_ms + tuning_ms) * rate / 1000)`, with the
// classic Freeverb tuning expressed in milliseconds (samples at 44.1 kHz /
// 44.1) and the right channel 23/44.1 ms longer.
//
// The input lowpass is the one stage not reproduced exactly: Kontakt runs a
// shared biquad-prototype filter object (vtable `0x144e95870`) whose
// prototype coefficients are not written anywhere in the static image, so a
// one-pole at the configured `input_cutoff_hz` stands in; `None` bypasses it.

fn one_pole(hz: f32, rate: f32) -> f32 { 1.0 - (-std::f32::consts::TAU * hz.min(rate * 0.49) / rate).exp() }
use crate::Error;
use sampler_ir::fx::space::Network;

const COMBS: usize = 8;
const ALLPASSES: usize = 4;
/// Comb tunings in milliseconds (1116, 1188, ... samples / 44.1 kHz).
const COMB_MS: [f64; COMBS] = [
    25.306_122_448_979_593,
    26.938_775_510_204_08,
    28.956_916_099_773_24,
    30.748_299_319_727_92,
    32.244_897_959_183_675,
    33.809_523_809_523_81,
    35.306_122_448_979_59,
    36.666_666_666_666_664,
];
/// Allpass tunings in milliseconds (556, 441, 341, 225 samples / 44.1 kHz).
const ALLPASS_MS: [f64; ALLPASSES] = [12.607_709_750_566_894, 10.0, 7.732_426_303_854_876, 5.102_040_816_326_531];
/// The right channel's delay offset in milliseconds (23 samples / 44.1 kHz).
const SPREAD_MS: f64 = 0.521_541_950_113_378_7;
/// Predelay ring size.
const PRE: usize = 0x2000;

/// A fixed delay: `tick` returns the value written `len` calls ago.
struct Ring {
    buf: Vec<f32>,
    at: usize,
}

impl Ring {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len.max(1)], at: 0 }
    }
    #[inline(always)]
    fn read(&self) -> f32 {
        self.buf[self.at]
    }
    #[inline(always)]
    fn write(&mut self, v: f32) {
        self.buf[self.at] = v;
        self.at += 1;
        if self.at == self.buf.len() {
            self.at = 0;
        }
    }
}

struct Channel {
    pre: Vec<f32>,
    combs: [Ring; COMBS],
    store: [f32; COMBS],
    allpass: [Ring; ALLPASSES],
    lp: f32,
}

pub(crate) struct Freeverb {
    channels: [Channel; 2],
    /// Predelay read index and delay in samples.
    rd: usize,
    pre: usize,
    feedback: f32,
    damp: f32,
    gain: f32,
    input_gain: f32,
    beta: f32,
    lp: Option<f32>,
}

/// `int((offset + tuning) * rate * 0.001)`, as the native computes it.
fn length(offset: f32, tuning: f64, rate: f32) -> usize {
    (((f64::from(offset) + tuning) * (f64::from(rate) * 0.001)) as i32).max(1) as usize
}

impl Freeverb {
    pub(crate) fn new(n: &Network, rate: u32) -> Result<Self, Error> {
        let f = n.freeverb.ok_or(Error::InvalidInput)?;
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = rate as f32;
        let offsets = [0.0f32, (0.0f64 + SPREAD_MS) as f32];
        // iVar12 = (int)(ms * rate * 0.001f); a predelay beyond the ring is dropped.
        let pre = ((n.predelay_seconds * 1000.0) as f32 * rate * 0.001) as usize;
        let channel = |c: usize| Channel {
            pre: vec![0.0; PRE],
            combs: std::array::from_fn(|i| Ring::new(length(offsets[c], COMB_MS[i], rate))),
            store: [0.0; COMBS],
            allpass: std::array::from_fn(|i| Ring::new(length(offsets[c], ALLPASS_MS[i], rate))),
            lp: 0.0,
        };
        Ok(Self {
            channels: [channel(0), channel(1)],
            rd: 0,
            pre: if pre >= PRE { 0 } else { pre },
            feedback: f.feedback as f32,
            damp: f.damp as f32,
            gain: f.allpass as f32,
            input_gain: f.input_gain as f32,
            beta: (2.0 * (1.0 - f.width)) as f32,
            lp: (n.input_cutoff_hz < 0.49 * f64::from(rate)).then(|| one_pole(n.input_cutoff_hz as f32, rate)),
        })
    }

    pub(crate) fn clear(&mut self) {
        for c in &mut self.channels {
            c.pre.fill(0.0);
            for r in c.combs.iter_mut().chain(c.allpass.iter_mut()) {
                r.buf.fill(0.0);
                r.at = 0;
            }
            (c.store, c.lp) = ([0.0; COMBS], 0.0);
        }
        self.rd = 0;
    }

    /// Replace `l`/`r` with the wet signal.
    pub(crate) fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        let (damp, keep, fb, g) = (self.damp, 1.0 - self.damp, self.feedback, self.gain);
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            let mut x = [*a, *b];
            if self.beta > 1e-12 {
                // mean = 0.5*L + 0.5*R; x = beta*mean + (1-beta)*x
                let mean = 0.5 * x[1] + 0.5 * x[0];
                let alpha = 1.0 - self.beta;
                x = x.map(|v| self.beta * mean + alpha * v);
            }
            let w = (self.rd + self.pre) & (PRE - 1);
            let mut out = [0f32; 2];
            for (c, ch) in self.channels.iter_mut().enumerate() {
                let mut v = x[c] * self.input_gain;
                if let Some(k) = self.lp {
                    ch.lp += k * (v - ch.lp);
                    v = ch.lp;
                }
                ch.pre[w] = v;
                let d = ch.pre[self.rd];
                let mut o = [0f32; COMBS];
                for j in 0..COMBS {
                    o[j] = ch.combs[j].read();
                    ch.store[j] = o[j] * damp + keep * ch.store[j];
                    ch.combs[j].write(ch.store[j] * fb + d);
                }
                // The native adds in this order.
                let mut s = o[3] + o[2] + o[1] + o[0] + o[7] + o[6] + o[5] + o[4];
                for ap in &mut ch.allpass {
                    let y = s * -g + ap.read();
                    ap.write(y * g + s);
                    s = y;
                }
                out[c] = s;
            }
            self.rd = (self.rd + 1) & (PRE - 1);
            (*a, *b) = (out[0], out[1]);
        }
    }
}

