//! Universal `timemod` DSP: one modulated multi-tap delay line plus a first-order
//! allpass cascade around one LFO. Every delay, chorus, flanger, phaser and
//! ensemble effect is a [`Config`]. All state lives in the per-stage
//! [`ProcessorState`] and a slab of `[f64; 2]` frames the chain allocates once
//! (`TimeMod::frames`): no allocation, no per-sample trig, block-rate flushing.
//!
//! Native laws: the leapfrog LFO, the Legacy Phaser and Phaser-filter allpass
//! coefficients and the synced rate getter come from Kontakt 8.13.1
//! (Legacy Phaser audio 0x140aa3830, 0x1409619b0, speed getter 0x140a736a0;
//! Phaser filter 0x1407c9a70). Interpolation, pan, saturation and the sweep
//! law are generic.
use super::{Planar, ProcessorState, flush};
use crate::Error;
use sampler_ir::fx::timemod::{
    Allpass, Config, DelayTime, Interpolation, LfoShape, MAX_TAPS, MAX_VOICES, Native, Order, Rate,
    SweepLaw,
};
use std::f64::consts::{PI, TAU};

mod diffuse;
mod dual;
mod mt;
mod tape;
mod psyche;
mod twin;
mod uvi_legacy;
#[cfg(test)]
mod tests;

/// Frames between generic allpass coefficient refreshes (Replika's native
/// control quantum, 0x1408f98d0).
const CHUNK: usize = 32;
/// Legacy Delay ring frames (setter clamp 0x40000, mask 0x27c).
const LEGACY_RING: usize = 0x40000;
/// Legacy Flanger ring frames (0x140960f00: length 0x1000, mask 0xfff).
const FLANGER_RING: usize = 0x1000;
/// Samples the ring keeps beyond the longest read for neighbour taps.
const GUARD: usize = 4;

/// The original plain stereo delay stage: integer frames, feedback matrix,
/// linear dry/wet. A thin constructor over [`TimeMod`].
#[derive(Clone, Copy, Debug)]
pub struct Delay(Config);

impl Delay {
    pub fn new(frames: u32, feedback: [[f64; 2]; 2], dry: f64, wet: f64) -> Result<Self, Error> {
        let config = Config::delay(frames, feedback, dry, wet);
        config.check().map_err(|_| Error::InvalidInput)?;
        Ok(Self(config))
    }
    pub(crate) fn config(self) -> Config {
        self.0
    }
}

#[derive(Clone, Copy)]
struct Head {
    time: DelayTime,
    depth: f64,
    /// Per voice: output gains for L and R (level, voice gain and pan folded).
    gain: [[f64; 2]; MAX_VOICES],
}

pub(crate) struct TimeMod {
    cfg: Config,
    rate: f64,
    cap: usize,
    ap_at: usize,
    fb_at: usize,
    tap_at: usize,
    frames: usize,
    heads: [Head; MAX_TAPS],
    taps: usize,
    voices: usize,
    voice_gain: f64,
    /// (cos, sin, phase) of every voice's LFO offset.
    voice: [(f64, f64, f64); MAX_VOICES],
    lfo_used: bool,
    min_delay: f64,
}

#[inline(always)]
fn exp_poly_legacy(scaled: f32) -> f32 {
    // Legacy Phaser polynomial, f32 operation order of 0x140962c75.
    let v = scaled * 3.;
    let square = scaled * scaled * 9.;
    let cube = square * v;
    let fourth = cube * v;
    let fifth = fourth * v;
    if v.abs() < f32::from_bits(0x3727c5ac) {
        1.
    } else {
        fifth * v * f32::from_bits(0x3ab60b61)
            + fifth * f32::from_bits(0x3c088889)
            + fourth * f32::from_bits(0x3d2aaaab)
            + cube * f32::from_bits(0x3e2aaaab)
            + v
            + 1.
            + square * 0.5
    }
}

/// Phaser filter coefficient from the normalized cutoff (0x1407c9b19).
fn filter_coefficient(cutoff: f32) -> f32 {
    let v = cutoff * 3.;
    let square = cutoff * cutoff * 9.;
    let cube = square * v;
    let fourth = cube * cutoff * 3.;
    let fifth = fourth * v;
    let exponential = if v.abs() < f32::from_bits(0x3727c5ac) {
        1.
    } else {
        fifth * cutoff * 3. * f32::from_bits(0x3ab60b61)
            + fifth * f32::from_bits(0x3c088889)
            + cube * f32::from_bits(0x3e2aaaab)
            + fourth * f32::from_bits(0x3d2aaaab)
            + square * 0.5
            + v
            + 1.
    };
    let frequency = (f64::from((f64::from(exponential - 1.) * 0.05431093007467753) as f32)
        * 0.32999999999999996
        + 0.02) as f32;
    (1. - frequency) / (frequency + 1.)
}

#[inline(always)]
fn legacy_coefficient(sine: f32, depth: f32) -> f32 {
    let mapped = (exp_poly_legacy(sine * depth) - 1.) * f32::from_bits(0x3c8bc709);
    (f32::from_bits(0x3f7b17e5) - mapped) / (mapped + f32::from_bits(0x3f82740e))
}

/// Rational tanh, exact at +-3 and beyond.
#[inline(always)]
fn soft(x: f64) -> f64 {
    if x.abs() >= 3. { x.signum() } else { x * (27. + x * x) / (27. + 9. * x * x) }
}

#[inline(always)]
fn shape(kind: LfoShape, phase: f64) -> f64 {
    let x = phase - phase.floor();
    match kind {
        LfoShape::Sine => unreachable!("sine runs the leapfrog"),
        LfoShape::Triangle => {
            if x < 0.25 { 4. * x } else if x < 0.75 { 2. - 4. * x } else { 4. * x - 4. }
        }
        LfoShape::Saw => 2. * x - 1.,
        LfoShape::Square => if x < 0.5 { 1. } else { -1. },
    }
}

impl TimeMod {
    pub(crate) fn compile(cfg: Config, rate: u32) -> Result<Self, Error> {
        cfg.check().map_err(|_| Error::InvalidInput)?;
        if rate == 0 {
            return Err(Error::InvalidInput);
        }
        let rate = f64::from(rate);
        let (mut cap, mut taps, mut voices) = (0, 0, 1);
        let mut heads = [Head { time: DelayTime::Frames(1), depth: 0., gain: [[1.; 2]; MAX_VOICES] }; MAX_TAPS];
        let mut voice = [(1., 0., 0.); MAX_VOICES];
        let mut voice_gain = 1.;
        let mut min_delay = 1.;
        if let Some(line) = &cfg.line {
            taps = usize::from(line.tap_count);
            voices = usize::from(line.voices);
            voice_gain = 1. / voices as f64;
            if matches!(line.interpolation, Interpolation::Hermite | Interpolation::Lagrange) {
                min_delay = 2.;
            }
            let scale = line.right_scale.max(1.);
            let mut longest = line.max_ms * 0.001 * rate * scale;
            for (head, tap) in heads.iter_mut().zip(&line.taps[..taps]) {
                head.time = tap.time;
                head.depth = tap.depth_ms * 0.001 * rate;
                longest = longest.max(scale * match tap.time {
                    DelayTime::Frames(n) => f64::from(n),
                    DelayTime::Ms(ms) => (ms + tap.depth_ms) * 0.001 * rate,
                    DelayTime::Beats(_) => 0.,
                });
                for v in 0..voices {
                    let pan = if voices > 1 {
                        line.voice_spread * (2. * v as f64 / (voices - 1) as f64 - 1.)
                    } else {
                        0.
                    };
                    let p = (tap.pan + pan).clamp(-1., 1.);
                    head.gain[v] = [
                        tap.level * voice_gain * (1. - p).min(1.),
                        tap.level * voice_gain * (1. + p).min(1.),
                    ];
                }
            }
            for (v, slot) in voice.iter_mut().enumerate().take(voices) {
                let cycles = line.voice_phase * v as f64 / voices as f64;
                *slot = ((cycles * TAU).cos(), (cycles * TAU).sin(), cycles);
            }
            cap = longest.ceil() as usize + GUARD;
            match line.native {
                Native::None => {}
                Native::LegacyDelay { .. } => cap = LEGACY_RING,
                Native::LegacyFlanger { .. } => cap = FLANGER_RING,
                Native::UviDualDelay(_) => cap = dual::ring(rate) + GUARD,
                Native::UviLegacyDelay(d) => cap = uvi_legacy::capacity(&d, rate),
                Native::UviDiffuseDelay(_) => cap = diffuse::capacity(rate),
                Native::UviTapeEcho(_) => cap = tape::capacity(rate),
                Native::TwinDelay(_) => cap = twin::capacity(rate),
                Native::PsycheDelay(_) => cap = psyche::capacity(rate),
            }
        }
        let stages = cfg.allpass.map_or(0, |a| usize::from(a.stages));
        let (ap_at, fb_at) = (cap, cap + 2 * stages);
        let tap_at = fb_at + 2;
        let lfo_used = cfg.line.is_some_and(|l| l.taps[..taps].iter().any(|t| t.depth_ms > 0.))
            || cfg.allpass.is_some_and(|a| !matches!(a.law, SweepLaw::Filter { .. }));
        Ok(Self {
            cfg,
            rate,
            cap,
            ap_at,
            fb_at,
            tap_at,
            frames: tap_at + if cfg.line.is_some() { MAX_TAPS } else { 0 },
            heads,
            taps,
            voices,
            voice_gain,
            voice,
            lfo_used,
            min_delay,
        })
    }

    /// Phaser filter (Kontakt filter type 13): four fixed allpass stages from
    /// the normalized cutoff and resonance, half dry, half wet.
    pub(crate) fn phaser_filter([cutoff, resonance]: [f32; 2]) -> Result<Self, Error> {
        let cfg = Config {
            line: None,
            allpass: Some(Allpass {
                stages: 4,
                law: SweepLaw::Filter { cutoff },
                feedback: (f64::from(resonance) * 0.85) as f32,
            }),
            order: Order::LineThenAllpass,
            lfo: sampler_ir::fx::timemod::Lfo { shape: LfoShape::Sine, rate: Rate::Hz(0.), phase: 0. },
            dry: 0.5,
            wet: 0.5,
        };
        // The rate only sizes unused LFO tables.
        Self::compile(cfg, 48_000)
    }

    /// Slab frames this stage needs.
    pub(crate) fn frames(&self) -> usize {
        self.frames
    }

    pub(crate) fn trace_frames(&self) -> u32 {
        if self.cfg.dry == 0. { self.cap.saturating_sub(GUARD) as u32 } else { 0 }
    }

    pub(crate) fn trace_parameters(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("dry", self.cfg.dry),
            ("wet", self.cfg.wet),
            ("line_frames", self.cap.saturating_sub(GUARD) as f64),
            ("stages", self.cfg.allpass.map_or(0., |a| f64::from(a.stages))),
        ]
    }

    fn hz(&self, tempo: f64) -> f32 {
        match self.cfg.lfo.rate {
            Rate::Hz(hz) => hz,
            Rate::Sync { unit, speed } => {
                // Native getter 0x140a736a0: f32 order, tempo below 0.1 reads 120.
                let tempo = tempo as f32;
                let tempo = if tempo < f32::from_bits(0x3dcccccd) { 120. } else { tempo };
                (1. / (((unit * 60.) / tempo) * speed.max(1.))).clamp(0., 8.)
            }
        }
    }

    /// Process `len` frames in place. Returns whether a nonfinite value
    /// entered the stage (the caller resets its state).
    pub(super) fn process(
        &self,
        state: &mut ProcessorState,
        slab: &mut [[f64; 2]],
        block: &mut Planar,
        len: usize,
        tempo: f64,
    ) -> bool {
        let cfg = &self.cfg;
        let (cap, stages) = (self.cap, cfg.allpass.map_or(0, |a| usize::from(a.stages)));
        let sine = cfg.lfo.shape == LfoShape::Sine;
        // Reset validity, not the potentially long ring: a reused voice or a
        // faulted chain never reads samples from its previous lifetime.
        if state.aux[15] == 0. {
            slab[if cfg.line.is_some_and(|l| l.native != Native::None) { 0 } else { cap }..].fill([0.; 2]);
            (state.aux[6], state.aux[7]) = (0., 0.);
            let phase = cfg.lfo.phase;
            if sine {
                state.z[0] = [0., 1.];
                let angle = f64::from(1. - phase) * TAU;
                state.z[1] = [f64::from(angle.sin() as f32), f64::from(angle.cos() as f32)];
            } else {
                state.z[0] = [0., 0.];
                state.z[1] = [f64::from(1. - phase), 0.];
            }
            state.aux[15] = 1.;
        }
        if let Some(Native::LegacyDelay { time, unit, feedback, damping, pan }) = cfg.line.map(|l| l.native) {
            return self.legacy_delay(state, slab, block, len, tempo, [time, unit, feedback, damping, pan]);
        }
        if let Some(Native::UviDualDelay(d)) = cfg.line.map(|l| l.native) {
            return self.dual_delay(state, slab, block, len, tempo, d);
        }
        if let Some(Native::PsycheDelay(p)) = cfg.line.map(|l| l.native) {
            return self.psyche_delay(state, slab, block, len, tempo, p);
        }
        if let Some(Native::TwinDelay(t)) = cfg.line.map(|l| l.native) {
            return self.twin_delay(state, slab, block, len, tempo, t);
        }
        if let Some(Native::UviTapeEcho(t)) = cfg.line.map(|l| l.native) {
            return self.uvi_tape_echo(state, slab, block, len, tempo, t);
        }
        if let Some(Native::UviDiffuseDelay(d)) = cfg.line.map(|l| l.native) {
            return self.uvi_diffuse_delay(state, slab, block, len, tempo, d);
        }
        if let Some(Native::UviLegacyDelay(d)) = cfg.line.map(|l| l.native) {
            return self.uvi_legacy_delay(state, slab, block, len, tempo, d);
        }
        if let Some(Native::LegacyFlanger { color, depth, feedback }) = cfg.line.map(|l| l.native) {
            return self.legacy_flanger(state, slab, block, len, tempo, [color, depth, feedback]);
        }
        let hz = self.hz(tempo);
        let step = (2. * (f64::from(hz) * PI / self.rate).sin()) as f32;
        let inc = f64::from(hz) / self.rate;

        // Tempo-synced heads glide to their target (block rate).
        let mut base = [0.; MAX_TAPS];
        let max_d = (cap.saturating_sub(GUARD)) as f64;
        for (k, head) in self.heads[..self.taps].iter().enumerate() {
            let target = match head.time {
                DelayTime::Frames(n) => f64::from(n),
                DelayTime::Ms(ms) => ms * 0.001 * self.rate,
                DelayTime::Beats(b) => {
                    let t = if tempo < 0.1 { 120. } else { tempo };
                    b * 60. / t * self.rate
                }
            };
            base[k] = if matches!(head.time, DelayTime::Beats(_)) {
                let cur = &mut slab[self.tap_at + k][0];
                if !cur.is_finite() || *cur == 0. {
                    *cur = target;
                }
                *cur
            } else {
                target
            };
        }
        let beat_glide = |slab: &mut [[f64; 2]], base: &mut [f64; MAX_TAPS]| {
            for (k, head) in self.heads[..self.taps].iter().enumerate() {
                if let DelayTime::Beats(b) = head.time {
                    let t = if tempo < 0.1 { 120. } else { tempo };
                    let target = b * 60. / t * self.rate;
                    let cur = &mut slab[self.tap_at + k][0];
                    *cur += (target - *cur) * 0.002;
                    base[k] = *cur;
                }
            }
        };
        let any_beats = self.heads[..self.taps].iter().any(|h| matches!(h.time, DelayTime::Beats(_)));

        let (mut position, mut filled) = (state.delay_position as usize, state.delay_filled as usize);
        let mut finite = true;
        let [left, right] = block;
        let mut coefficient = [0f32; 2];
        let (mut ramp, mut chunk) = ([state.aux[3] as f32, state.aux[4] as f32], state.aux[2] as usize);
        if let Some(Allpass { law: SweepLaw::Filter { cutoff }, .. }) = cfg.allpass {
            coefficient = [filter_coefficient(cutoff); 2];
        }
        let mut primed = state.aux[5] != 0.;
        let mut previous = [state.aux[0] as f32, state.aux[1] as f32];

        for i in 0..len {
            let x = [left[i], right[i]];
            // LFO: native leapfrog for sine, phase accumulator otherwise.
            let mut lfo = [0f64; 2];
            let mut lfo_cos = [0f64; 2];
            if self.lfo_used {
                for c in 0..2 {
                    if sine {
                        let [mut s, mut co] = state.z[c].map(|v| v as f32);
                        s -= co * step;
                        co += s * step;
                        state.z[c] = [f64::from(s), f64::from(co)];
                        (lfo[c], lfo_cos[c]) = (f64::from(s), f64::from(co));
                    } else {
                        state.z[c][0] += inc;
                        state.z[c][0] -= state.z[c][0].floor();
                        lfo[c] = shape(cfg.lfo.shape, state.z[c][0]);
                    }
                }
            }
            let mut sig = x;
            let stage_pass = |sig: &mut [f64; 2], slab: &mut [[f64; 2]], chunk: &mut usize, primed: &mut bool, ramp: &mut [f32; 2], previous: &mut [f32; 2], coefficient: &mut [f32; 2], finite: &mut bool| {
                let ap = cfg.allpass.expect("section present");
                match ap.law {
                    SweepLaw::Legacy { depth } => {
                        for c in 0..2 {
                            coefficient[c] = legacy_coefficient(lfo[c] as f32, depth);
                        }
                    }
                    SweepLaw::Filter { .. } => {}
                    SweepLaw::Sweep { min_hz, max_hz, depth } => {
                        if *chunk == 0 {
                            for c in 0..2 {
                                let u = 0.5 + 0.5 * lfo[c] * f64::from(depth);
                                let f = (f64::from(min_hz) * (f64::from(max_hz) / f64::from(min_hz)).powf(u))
                                    .min(self.rate * 0.45);
                                let t = (PI * f / self.rate).tan();
                                let target = ((1. - t) / (1. + t)) as f32;
                                if !*primed {
                                    previous[c] = target;
                                }
                                ramp[c] = (target - previous[c]) / CHUNK as f32;
                            }
                            *primed = true;
                        }
                        for c in 0..2 {
                            previous[c] += ramp[c];
                            coefficient[c] = previous[c];
                        }
                        *chunk = (*chunk + 1) % CHUNK;
                    }
                }
                for c in 0..2 {
                    let last = self.ap_at + 2 * (stages - 1) + 1;
                    let mut input = slab[last][c] as f32 * ap.feedback + sig[c] as f32;
                    for s in 0..stages {
                        let (xi, yi) = (self.ap_at + 2 * s, self.ap_at + 2 * s + 1);
                        let output = (input + slab[yi][c] as f32) * coefficient[c] - slab[xi][c] as f32;
                        slab[xi][c] = f64::from(input);
                        slab[yi][c] = f64::from(output);
                        input = output;
                    }
                    sig[c] = f64::from(input);
                    *finite &= input.is_finite();
                }
            };
            if cfg.order == Order::AllpassThenLine && cfg.allpass.is_some() {
                stage_pass(&mut sig, slab, &mut chunk, &mut primed, &mut ramp, &mut previous, &mut coefficient, &mut finite);
            }
            if let Some(line) = &cfg.line {
                let source = usize::from(line.feedback.source);
                let input = [
                    line.input[0][0] * sig[0] + line.input[0][1] * sig[1],
                    line.input[1][0] * sig[0] + line.input[1][1] * sig[1],
                ];
                let mut out = [0f64; 2];
                let mut fed = [0f64; 2];
                for (k, head) in self.heads[..self.taps].iter().enumerate() {
                    for v in 0..self.voices {
                        let (vc, vs, vp) = self.voice[v];
                        for c in 0..2 {
                            let mod_value = if head.depth == 0. {
                                0.
                            } else if sine {
                                lfo[c] * vc + lfo_cos[c] * vs
                            } else {
                                shape(cfg.lfo.shape, state.z[c][0] + vp)
                            };
                            let d = (base[k] * if c == 1 { line.right_scale } else { 1. } + head.depth * mod_value).clamp(self.min_delay, max_d.max(self.min_delay));
                            let sample = read(slab, cap, position, filled, d, line.interpolation, c);
                            out[c] += sample * head.gain[v][c];
                            if k == source {
                                fed[c] += sample * self.voice_gain;
                            }
                        }
                    }
                }
                // Recirculating path: loop filters, then saturation.
                let fb = &line.feedback;
                for c in 0..2 {
                    if fb.lowpass > 0. {
                        let lp = &mut slab[self.fb_at][c];
                        *lp += fb.lowpass * (fed[c] - *lp);
                        fed[c] = *lp;
                    }
                    if fb.highpass > 0. {
                        let hp = &mut slab[self.fb_at + 1][c];
                        *hp += (1. - fb.highpass) * (fed[c] - *hp);
                        fed[c] -= *hp;
                    }
                    if fb.drive > 0. {
                        let g = 1. + fb.drive;
                        fed[c] = soft(fed[c] * g) / g;
                    }
                }
                let next: [f64; 2] = std::array::from_fn(|c| input[c] + fb.matrix[c][0] * fed[0] + fb.matrix[c][1] * fed[1]);
                finite &= next[0].is_finite() & next[1].is_finite();
                slab[position] = next.map(flush);
                filled = (filled + 1).min(cap);
                position = if position + 1 == cap { 0 } else { position + 1 };
                sig = out;
                if any_beats {
                    beat_glide(slab, &mut base);
                }
            }
            if cfg.order == Order::LineThenAllpass && cfg.allpass.is_some() {
                stage_pass(&mut sig, slab, &mut chunk, &mut primed, &mut ramp, &mut previous, &mut coefficient, &mut finite);
            }
            left[i] = cfg.dry * x[0] + cfg.wet * sig[0];
            right[i] = cfg.dry * x[1] + cfg.wet * sig[1];
        }
        state.delay_position = position as u32;
        state.delay_filled = filled as u32;
        (state.aux[0], state.aux[1]) = (f64::from(previous[0]), f64::from(previous[1]));
        (state.aux[2], state.aux[3], state.aux[4], state.aux[5]) = (chunk as f64, f64::from(ramp[0]), f64::from(ramp[1]), f64::from(u8::from(primed)));
        // Retained state decays to denormals only between blocks of silence.
        for frame in &mut slab[self.ap_at..self.tap_at] {
            *frame = frame.map(flush);
        }
        !finite
    }
}

impl TimeMod {
    /// Kontakt Legacy Flanger audio (0x140aa2350), f32 operation order. Wet-only:
    /// the 4-point Lagrange read of a ring the input and the negated output feed.
    fn legacy_flanger(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, [color, depth, feedback]: [f32; 3]) -> bool {
        const MASK: usize = FLANGER_RING - 1;
        let base = ((color * self.rate as f32 * f32::from_bits(0x3a83126f)) as i32).clamp(0, FLANGER_RING as i32);
        let mut excursion = f64::from(depth) * 50.;
        if excursion as i32 >= base {
            excursion = f64::from(base - 1);
        }
        let fb = feedback * f32::from_bits(0x3f733333);
        let step = (2. * (f64::from(self.hz(tempo)) * PI / self.rate).sin()) as f32;
        let mut w = state.delay_position as usize & MASK;
        let origin = ((w as i32 - base + FLANGER_RING as i32) % FLANGER_RING as i32) as f32 + FLANGER_RING as f32;
        let mut finite = true;
        let [left, right] = block;
        for i in 0..len {
            let input = [left[i] as f32, right[i] as f32];
            slab[w] = input.map(f64::from);
            let mut out = [0f32; 2];
            for c in 0..2 {
                let [mut s, mut co] = state.z[c].map(|v| v as f32);
                s -= co * step;
                co += s * step;
                state.z[c] = [f64::from(s), f64::from(co)];
                let position = (f64::from(s) * excursion + f64::from(origin + i as f32)) as f32;
                let whole = position as u32;
                let t = position - whole as f32;
                let at = |k: u32| slab[(k as usize) & MASK][c] as f32;
                let (a, b, cc, d) = (at(whole.wrapping_sub(2)), at(whole.wrapping_sub(1)), at(whole), at(whole.wrapping_add(1)));
                let (up, down) = (t + 1., t - 1.);
                let y = (up * d * t * down - t * a * (down + -1.) * down) * f32::from_bits(0x3e2aaaab) + (up * b * down - up * cc * t) * (down + -1.) * 0.5;
                out[c] = y;
                slab[w][c] = f64::from(slab[w][c] as f32 - y * fb);
                finite &= y.is_finite();
            }
            (left[i], right[i]) = (self.cfg.dry * f64::from(input[0]) + self.cfg.wet * f64::from(out[0]), self.cfg.dry * f64::from(input[1]) + self.cfg.wet * f64::from(out[1]));
            w = (w + 1) & MASK;
        }
        state.delay_position = w as u32;
        for frame in &mut slab[..FLANGER_RING] {
            *frame = frame.map(flush);
        }
        !finite
    }

    /// Kontakt Legacy Delay audio (0x140aa1130, stereo branch), f32 operation
    /// order. The low-pass states lag one sample and are both the wet output
    /// and the recirculated signal; `pan` crosses the channels.
    fn legacy_delay(&self, state: &mut ProcessorState, slab: &mut [[f64; 2]], block: &mut Planar, len: usize, tempo: f64, [time, unit, feedback, damping, pan]: [f32; 5]) -> bool {
        const MASK: usize = LEGACY_RING - 1;
        let ms = if unit > f32::from_bits(0x3089705f) {
            let tempo = tempo as f32;
            let tempo = if tempo < f32::from_bits(0x3dcccccd) { 120. } else { tempo };
            // Getter 0x140a74bf0 via 0x14072b380: a synced count below one
            // unit reads as one (flag +0x20, set by 0x140a723e0), and the
            // result clamps to [0, 5800] ms (constructor 0x140701ac2/ac6).
            (((unit * 60.) / tempo) * time.max(1.) * 1000.).clamp(0., 5800.)
        } else {
            time
        };
        let frames = (ms * self.rate as f32 * f32::from_bits(0x3a83126f)) as i32;
        let delay = frames.clamp(0, LEGACY_RING as i32) as usize;
        let g = feedback * f32::from_bits(0x3f7ae148);
        let c = 1. - damping * f32::from_bits(0x3f333333);
        let (straight, cross) = ((1. - pan) * g, g * pan);
        let mut w = state.delay_position as usize & MASK;
        let mut lp = [state.aux[6] as f32, state.aux[7] as f32];
        let mut finite = true;
        let [left, right] = block;
        for i in 0..len {
            let (a, b) = (left[i] as f32, right[i] as f32);
            let mut ring = [a * g, straight * b];
            let out = lp;
            ring[0] = straight * lp[0] + ring[0];
            ring[1] = straight * lp[1] + ring[1];
            ring[0] = cross * lp[1] + ring[0];
            ring[1] = cross * lp[0] + ring[1];
            slab[w] = ring.map(|v| flush(f64::from(v)));
            let read = slab[(w + LEGACY_RING - delay) & MASK];
            for k in 0..2 {
                lp[k] = (read[k] as f32 - lp[k]) * c + lp[k];
                lp[k] = if lp[k].is_subnormal() { 0. } else { lp[k] };
                finite &= lp[k].is_finite();
            }
            (left[i], right[i]) = (self.cfg.dry * f64::from(a) + self.cfg.wet * f64::from(out[0]), self.cfg.dry * f64::from(b) + self.cfg.wet * f64::from(out[1]));
            w = (w + 1) & MASK;
        }
        state.delay_position = w as u32;
        (state.aux[6], state.aux[7]) = (f64::from(lp[0]), f64::from(lp[1]));
        !finite
    }
}

/// Interpolated read `d` frames behind the write position of channel `c`.
/// Frames the ring has not written yet read as silence.
#[inline(always)]
fn read(slab: &[[f64; 2]], cap: usize, position: usize, filled: usize, d: f64, kind: Interpolation, c: usize) -> f64 {
    let at = |k: usize| -> f64 {
        if k > filled {
            return 0.;
        }
        let m = position + cap - k;
        slab[if m >= cap { m - cap } else { m }][c]
    };
    match kind {
        Interpolation::Nearest => return at((d + 0.5) as usize),
        Interpolation::Floor => return at(d as usize),
        Interpolation::Ceil => return at(d.ceil() as usize),
        _ => {}
    }
    let i = d as usize;
    let x = d - i as f64;
    match kind {
        Interpolation::Linear => {
            let (a, b) = (at(i), at(i + 1));
            a + (b - a) * x
        }
        Interpolation::Hermite => {
            let (m, a, b, n) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let c1 = 0.5 * (b - m);
            let c2 = m - 2.5 * a + 2. * b - 0.5 * n;
            let c3 = 0.5 * (n - m) + 1.5 * (a - b);
            ((c3 * x + c2) * x + c1) * x + a
        }
        Interpolation::Lagrange => {
            let (m, a, b, n) = (at(i - 1), at(i), at(i + 1), at(i + 2));
            let (xm, xp, x2) = (x - 1., x + 1., x - 2.);
            -x * xm * x2 / 6. * m + xp * xm * x2 / 2. * a - xp * x * x2 / 2. * b + xp * x * xm / 6. * n
        }
        Interpolation::Nearest | Interpolation::Floor | Interpolation::Ceil => unreachable!(),
    }
}
