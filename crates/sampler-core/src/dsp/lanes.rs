//! Voice-batched stages. Up to [`VOICES`] voices of one chain run each stage
//! together, lane `2 v + c` holding voice `v` channel `c`, so the per-frame
//! recurrences of different voices interleave and vectorize. Every lane does
//! exactly the scalar stage's arithmetic in the same order: results are
//! bit-identical to [`super::process`] voice by voice.
use super::{
    BLOCK, PreparedProcessor, ProcessorState,
    control::ControlRamp,
    flush,
    svf::{CacheRef, FilterBank},
};

pub(crate) const VOICES: usize = 8;
pub(crate) const LANES: usize = 2 * VOICES;
type Lanes = [f64; LANES];
pub(crate) type LaneBlock = [Lanes; BLOCK];

/// The voices of one batch and the frames each runs this block.
pub(crate) struct Batch {
    /// Real voices; later lanes are padding whose results are discarded.
    pub count: usize,
    pub expressions: [Option<(crate::ExpressionId, crate::Expression)>; VOICES],
    /// Frames each lane runs; padding lanes run `len`.
    pub ends: [usize; LANES],
    pub len: usize,
}

impl Batch {
    fn uniform(&self) -> bool {
        self.ends.iter().all(|&end| end == self.len)
    }
}

/// Each batch voice's chain state.
pub(crate) type Cells<'a> = [Option<sampler_pool::Claim<'a, ProcessorState>>; VOICES];

/// Run `stages` (cell indices from `first`) over the batch's lanes.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn process(
    stages: &[PreparedProcessor],
    first: usize,
    cells: &mut Cells<'_>,
    batch: &Batch,
    block: &mut LaneBlock,
    parameters: &[ControlRamp],
    at: u64,
    filters: &mut FilterBank,
) {
    let len = batch.len;
    let uniform = batch.uniform();
    for (index, stage) in stages.iter().enumerate() {
        let cell = first + index;
        match stage {
            PreparedProcessor::Gain(gain) => {
                for x in &mut block[..len] {
                    x.iter_mut().for_each(|v| *v *= gain);
                }
            }
            PreparedProcessor::StereoMatrix(m) => {
                for x in &mut block[..len] {
                    for pair in x.as_chunks_mut::<2>().0 {
                        let [l, r] = *pair;
                        *pair = [m[0][0] * l + m[0][1] * r, m[1][0] * l + m[1][1] * r];
                    }
                }
            }
            PreparedProcessor::Rectify(mode) => {
                for x in &mut block[..len] {
                    x.iter_mut().for_each(|v| *v = mode.apply(*v));
                }
            }
            PreparedProcessor::ControlGain(lane) => {
                let ramp = parameters[*lane];
                for (i, x) in block[..len].iter_mut().enumerate() {
                    let gain = ramp.value(at + i as u64);
                    x.iter_mut().for_each(|v| *v *= gain);
                }
            }
            PreparedProcessor::Biquad(filter) => {
                // Scalar layout: z[channel] = [z0, z1].
                let mut z = [[0.; LANES]; 2];
                for v in 0..batch.count {
                    let state = cells[v].as_ref().expect("batch voice")[cell].z;
                    for c in 0..2 {
                        (z[0][2 * v + c], z[1][2 * v + c]) = (state[c][0], state[c][1]);
                    }
                }
                if uniform {
                    biquad::<false>(&mut z, block, batch, filter.b, filter.a);
                } else {
                    biquad::<true>(&mut z, block, batch, filter.b, filter.a);
                }
                for v in 0..batch.count {
                    cells[v].as_mut().expect("batch voice")[cell].z =
                        std::array::from_fn(|c| [z[0][2 * v + c], z[1][2 * v + c]].map(flush));
                }
            }
            PreparedProcessor::StateVariable(filter) => {
                // Scalar layout: z = [s0, s1], each [left, right].
                let mut s = [[0.; LANES]; 2];
                let mut refs = [None; VOICES];
                for v in 0..batch.count {
                    let state = cells[v].as_ref().expect("batch voice")[cell].z;
                    for c in 0..2 {
                        (s[0][2 * v + c], s[1][2 * v + c]) = (state[0][c], state[1][c]);
                    }
                    let end = batch.ends[2 * v];
                    refs[v] =
                        Some(filters.prepare(*filter, end, parameters, at, batch.expressions[v]));
                }
                let refs = refs.map(|r| r.unwrap_or(refs[0].expect("a batch has a voice")));
                svf(&mut s, block, batch, uniform, filters, refs);
                for v in 0..batch.count {
                    cells[v].as_mut().expect("batch voice")[cell].z =
                        std::array::from_fn(|i| [s[i][2 * v], s[i][2 * v + 1]].map(flush));
                }
            }
            PreparedProcessor::Delay { .. }
            | PreparedProcessor::Compressor(_)
            | PreparedProcessor::Decimate(_)
            | PreparedProcessor::Daft(_)
            | PreparedProcessor::Branch { .. } => {
                unreachable!("delay, compressor and decimator chains render per voice")
            }
            PreparedProcessor::Reverb(_)
            | PreparedProcessor::Convolution(_)
            | PreparedProcessor::Mix { .. } => {
                unreachable!("reverbs and convolutions are bus processors")
            }
        }
    }
}

/// Multiply each voice's lanes by its envelope levels.
pub(crate) fn scale(block: &mut LaneBlock, v: usize, levels: &[f64; BLOCK], len: usize) {
    for (x, level) in block[..len].iter_mut().zip(levels) {
        x[2 * v] *= level;
        x[2 * v + 1] *= level;
    }
}

#[inline(always)]
fn biquad<const MASK: bool>(
    z: &mut [Lanes; 2],
    block: &mut LaneBlock,
    batch: &Batch,
    [b0, b1, b2]: [f64; 3],
    [a1, a2]: [f64; 2],
) {
    let [mut z0, mut z1] = *z;
    for (i, x) in block[..batch.len].iter_mut().enumerate() {
        for k in 0..LANES {
            let input = x[k];
            let y = b0 * input + z0[k];
            let next = [b1 * input - a1 * y + z1[k], b2 * input - a2 * y];
            if !MASK || i < batch.ends[k] {
                (z0[k], z1[k]) = (next[0], next[1]);
            }
            x[k] = y;
        }
    }
    *z = [z0, z1];
}

#[inline(always)]
fn svf(
    s: &mut [Lanes; 2],
    block: &mut LaneBlock,
    batch: &Batch,
    uniform: bool,
    filters: &FilterBank,
    refs: [CacheRef; VOICES],
) {
    if let Some(high) = filters.cache(refs[0]).one_pole_high() {
        let caches = refs.map(|r| filters.cache(r));
        let b = |i: usize, k: usize| {
            let c = caches[k / 2];
            c.coefficients[if c.uniform { 0 } else { i }].a3
        };
        if uniform {
            one_pole::<false>(s, block, batch, high, b);
        } else {
            one_pole::<true>(s, block, batch, high, b);
        }
        return;
    }
    let [m0, mk, m2] = filters.cache(refs[0]).mix();
    let splat = |c: super::svf::Coefficients| {
        [
            [c.a1; LANES],
            [c.a2; LANES],
            [c.a3; LANES],
            [mk * c.k; LANES],
        ]
    };
    if refs.iter().all(|r| *r == refs[0]) {
        let cache = filters.cache(refs[0]);
        if cache.uniform {
            let c = splat(cache.coefficients[0]);
            run(s, block, batch, uniform, m0, m2, |_| c);
        } else {
            let coefficients = &cache.coefficients;
            run(s, block, batch, uniform, m0, m2, |i| splat(coefficients[i]));
        }
    } else {
        let caches = refs.map(|r| &filters.cache(r).coefficients);
        run(s, block, batch, uniform, m0, m2, |i| {
            let mut lanes = [[0.; LANES]; 4];
            for (v, coefficients) in caches.iter().enumerate() {
                let c = coefficients[i];
                for lane in [2 * v, 2 * v + 1] {
                    lanes[0][lane] = c.a1;
                    lanes[1][lane] = c.a2;
                    lanes[2][lane] = c.a3;
                    lanes[3][lane] = mk * c.k;
                }
            }
            lanes
        });
    }
}

/// `y += (x - y) b` in `s[0]`, bit-identical to the scalar stage.
#[inline(always)]
fn one_pole<const MASK: bool>(
    s: &mut [Lanes; 2],
    block: &mut LaneBlock,
    batch: &Batch,
    high: bool,
    b: impl Fn(usize, usize) -> f64,
) {
    let mut y = s[0];
    for (i, x) in block[..batch.len].iter_mut().enumerate() {
        for k in 0..LANES {
            let next = y[k] + (x[k] - y[k]) * b(i, k);
            if !MASK || i < batch.ends[k] {
                y[k] = next;
            }
            x[k] = if high { x[k] - next } else { next };
        }
    }
    s[0] = y;
}

#[inline(always)]
fn run(
    s: &mut [Lanes; 2],
    block: &mut LaneBlock,
    batch: &Batch,
    uniform: bool,
    m0: f64,
    m2: f64,
    coefficients: impl Fn(usize) -> [Lanes; 4],
) {
    if uniform {
        recurrence::<false>(s, block, batch, m0, m2, coefficients);
    } else {
        recurrence::<true>(s, block, batch, m0, m2, coefficients);
    }
}

#[inline(always)]
fn recurrence<const MASK: bool>(
    s: &mut [Lanes; 2],
    block: &mut LaneBlock,
    batch: &Batch,
    m0: f64,
    m2: f64,
    coefficients: impl Fn(usize) -> [Lanes; 4],
) {
    let [mut s0, mut s1] = *s;
    for (i, x) in block[..batch.len].iter_mut().enumerate() {
        let [a1, a2, a3, km] = coefficients(i);
        for k in 0..LANES {
            let (ic1, ic2) = (s0[k], s1[k]);
            let v3 = x[k] - ic2;
            let band = a1[k] * ic1 + a2[k] * v3;
            let low = ic2 + a2[k] * ic1 + a3[k] * v3;
            if !MASK || i < batch.ends[k] {
                (s0[k], s1[k]) = (2. * band - ic1, 2. * low - ic2);
            }
            x[k] = m0 * x[k] + km[k] * band + m2 * low;
        }
    }
    *s = [s0, s1];
}
