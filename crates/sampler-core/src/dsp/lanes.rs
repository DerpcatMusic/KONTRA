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
    let mut index = 0;
    while index < stages.len() {
        let (stage, cell) = (&stages[index], first + index);
        index += 1;
        match stage {
            PreparedProcessor::Mix { count, lanes } => {
                let inner = index..index + usize::from(*count);
                index = inner.end;
                let [dry, wet, bypass] = lanes.map(|lane| parameters[lane]);
                let last = at + len.saturating_sub(1) as u64;
                // As the scalar stage: a fully bypassed block skips the inner
                // processors, whose state rests.
                // ponytail: a voice shorter than the batch decides this over the batch's
                // frames, so a bypass ramp ending inside it can advance state the scalar
                // stage would hold; the output is the same.
                let off = bypass.value(at) >= 1. && bypass.value(last) >= 1.;
                let dry_block = *block;
                if !off {
                    process(
                        &stages[inner.clone()],
                        first + inner.start,
                        cells,
                        batch,
                        block,
                        parameters,
                        at,
                        filters,
                    );
                }
                for (i, (x, d)) in block[..len].iter_mut().zip(&dry_block).enumerate() {
                    let t = at + i as u64;
                    let b = bypass.value(t);
                    let (direct, through) = (dry.value(t) * (1. - b) + b, wet.value(t) * (1. - b));
                    for (v, d) in x.iter_mut().zip(d) {
                        let wet_part = if off { 0. } else { through * *v };
                        *v = direct * d + wet_part;
                    }
                }
            }
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
            PreparedProcessor::Gainer { dry, gain, k } => {
                let mut current = [0.0f32; VOICES];
                let mut initialized = [false; VOICES];
                for v in 0..batch.count {
                    let state = &cells[v].as_ref().expect("batch voice")[cell];
                    current[v] = state.z[0][0] as f32;
                    initialized[v] = state.aux[0] != 0.0;
                }
                for (i, x) in block[..len].iter_mut().enumerate() {
                    let target = gain.value(parameters, at + i as u64, None) as f32;
                    for v in 0..batch.count {
                        if i >= batch.ends[2 * v] { continue; }
                        if !initialized[v] { (current[v], initialized[v]) = (target, true); }
                        let m = dry + f64::from(current[v]);
                        x[2 * v] *= m;
                        x[2 * v + 1] *= m;
                        current[v] += (target - current[v]) * *k as f32;
                    }
                }
                for v in 0..batch.count {
                    let state = &mut cells[v].as_mut().expect("batch voice")[cell];
                    state.z[0][0] = f64::from(current[v]);
                    state.aux[0] = if initialized[v] { 1.0 } else { 0.0 };
                }
            }
            PreparedProcessor::StereoModeller { stereo, .. } => {
                assert!(stereo.batches());
                let mut width = [0.0f32; VOICES];
                let mut pan = [0.0f32; VOICES];
                let mut delta = [0.0f32; VOICES];
                let mut initialized = [false; VOICES];
                for v in 0..batch.count {
                    let state = &cells[v].as_ref().expect("batch voice")[cell];
                    (width[v], pan[v]) = (state.aux[0] as f32, state.aux[1] as f32);
                    initialized[v] = state.aux[2] != 0.0;
                }
                for (i, x) in block[..len].iter_mut().enumerate() {
                    let [target_width, target_pan] = stereo.targets(parameters, at + i as u64);
                    for v in 0..batch.count {
                        let end = batch.ends[2 * v];
                        if i >= end { continue; }
                        if !initialized[v] {
                            (width[v], pan[v], initialized[v]) = (target_width, target_pan, true);
                        }
                        let (l, r) = super::stereo::matrix(x[2 * v] as f32, x[2 * v + 1] as f32, width[v]);
                        x[2 * v] = f64::from(l * (1.0 - pan[v].max(0.0)));
                        x[2 * v + 1] = f64::from(r * (1.0 + pan[v].min(0.0)));
                        width[v] += (target_width - width[v]) * (1.0f32 / 180.0);
                        if i >= end / 4 * 4 || i % 4 != 3 {
                            delta[v] = (target_pan - pan[v]) * f32::from_bits(0x3a11a2b4);
                        }
                        pan[v] += delta[v];
                    }
                }
                for v in 0..batch.count {
                    let state = &mut cells[v].as_mut().expect("batch voice")[cell];
                    (state.aux[0], state.aux[1], state.aux[2]) = (f64::from(width[v]), f64::from(pan[v]), if initialized[v] { 1.0 } else { 0.0 });
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
                    biquad::<false>(&mut z, block, batch, filter);
                } else {
                    biquad::<true>(&mut z, block, batch, filter);
                }
                for v in 0..batch.count {
                    cells[v].as_mut().expect("batch voice")[cell].z =
                        std::array::from_fn(|c| [z[0][2 * v + c], z[1][2 * v + c]].map(flush));
                }
            }
            PreparedProcessor::PeakingEq(eq) => {
                if eq.is_flat(parameters, at, len) { continue; }
                for v in 0..batch.count {
                    let end = batch.ends[2 * v];
                    let mut planar = [[0.; BLOCK]; 2];
                    for (i, x) in block[..end].iter().enumerate() {
                        (planar[0][i], planar[1][i]) = (x[2 * v], x[2 * v + 1]);
                    }
                    eq.process(&mut cells[v].as_mut().expect("batch voice")[cell], parameters, &mut planar, end, at);
                    for (i, x) in block[..end].iter_mut().enumerate() {
                        (x[2 * v], x[2 * v + 1]) = (planar[0][i], planar[1][i]);
                    }
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
            | PreparedProcessor::LoFi(_)
            | PreparedProcessor::Daft(_)
            | PreparedProcessor::LadderLP4 { .. }
            | PreparedProcessor::Branch { .. } => {
                unreachable!("delay, compressor and decimator chains render per voice")
            }
            PreparedProcessor::Reverb(_) | PreparedProcessor::Convolution(_) => {
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
    filter: &super::Biquad,
) {
    let [mut z0, mut z1] = *z;
    for (i, x) in block[..batch.len].iter_mut().enumerate() {
        for k in 0..LANES {
            let (y, next) = filter.sample(x[k], [z0[k], z1[k]]);
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{Biquad, FilterKind};

    #[test]
    fn eq_lane_pcm_and_histories_match_scalar_across_flat_and_masked_blocks() {
        for rate in [8000, 48000, 192000] {
            for hz in [20., 1000., f64::from(rate) * 0.49] {
                for width in [0.3, 3.] {
                    for len in [0usize, 1, 3, 17, BLOCK] {
                        let initial: Vec<_> = (0..VOICES).map(|v| {
                            let mut state = ProcessorState::default();
                            state.z = [[v as f64 * 0.003, -0.02], [0.01, v as f64 * -0.004]];
                            state
                        }).collect();
                        let mut expected_state = initial.clone();
                        let slab = sampler_pool::Slab::new(initial.into_boxed_slice(), 1);
                        let mut cells: Cells<'_> = std::array::from_fn(|v| Some(slab.claim(v)));
                        let batch = Batch { count: VOICES, expressions: [None; VOICES],
                            ends: std::array::from_fn(|k| len.saturating_sub(k / 2)), len };
                        let mut bank = FilterBank::new(&[], 0).unwrap();
                        for gain in [12., 0., -12.] {
                            let eq = super::super::PeakingEq { frequency: super::super::Parameter::Constant(((hz / 20f64).log10() / 3.).clamp(0., 1.)), bandwidth: super::super::Parameter::Constant(((width - 0.3f64) / 2.7).clamp(0., 1.)),
                                gain_db: super::super::Parameter::Constant(gain) }.compile(rate, &mut Vec::new()).unwrap();
                            let mut block: LaneBlock = std::array::from_fn(|i| std::array::from_fn(|k|
                                ((i + k) as f64 * 0.137).sin() * 0.2));
                            let mut expected = block;
                            for v in 0..VOICES {
                                let end = batch.ends[2 * v];
                                let mut planar = [[0.; BLOCK]; 2];
                                for i in 0..end { (planar[0][i], planar[1][i]) = (expected[i][2 * v], expected[i][2 * v + 1]); }
                                eq.process(&mut expected_state[v], &[], &mut planar, end, 0);
                                for i in 0..end { (expected[i][2 * v], expected[i][2 * v + 1]) = (planar[0][i], planar[1][i]); }
                            }
                            process(&[PreparedProcessor::PeakingEq(eq)], 0, &mut cells, &batch, &mut block, &[], 0, &mut bank);
                            assert_eq!(block.map(|f| f.map(f64::to_bits)), expected.map(|f| f.map(f64::to_bits)));
                            for v in 0..VOICES {
                                let actual = &cells[v].as_ref().unwrap()[0];
                                assert_eq!(actual.z.map(|c| c.map(f64::to_bits)), expected_state[v].z.map(|c| c.map(f64::to_bits)));
                                assert_eq!(actual.aux.map(f64::to_bits), expected_state[v].aux.map(f64::to_bits));
                            }
                        }
                    }
                }
            }
        }
    }

    // Frozen 608a20a1 arithmetic: independent of the shared sample helper.
    fn reference(filter: Biquad, x: f64, z: [f64; 2]) -> (f64, [f64; 2]) {
        let ([b0, b1, b2], [a1, a2]) = (filter.b, filter.a);
        let y = b0 * x + z[0];
        (y, [b1 * x - a1 * y + z[1], b2 * x - a2 * y])
    }

    #[test]
    fn biquad_scalar_and_lanes_match_frozen_pcm_and_state_bits() {
        for rate in [44100, 48000, 96000] {
            for kind in [
                FilterKind::LowPass, FilterKind::HighPass, FilterKind::BandPass,
                FilterKind::Notch, FilterKind::AllPass,
                FilterKind::Peak { gain_db: -12. }, FilterKind::Peak { gain_db: 12. },
                FilterKind::LowShelf { gain_db: -12. }, FilterKind::LowShelf { gain_db: 12. },
                FilterKind::HighShelf { gain_db: -12. }, FilterKind::HighShelf { gain_db: 12. },
            ] {
                for hz in [40., 1000., f64::from(rate) * 0.49] {
                    for q in [0.2, 0.707, 4.] {
                        let filter = Biquad::new(rate, kind, hz, q).unwrap();
                        for len in [0, 1, 3, 4, 17, BLOCK] {
                            for masked in [false, true] {
                                let batch = Batch {
                                    count: VOICES,
                                    expressions: [None; VOICES],
                                    ends: std::array::from_fn(|k| if masked { len.saturating_sub(k / 2) } else { len }),
                                    len,
                                };
                                let mut z = [[0.; LANES]; 2];
                                let mut expected_z = z;
                                let mut scalar = [ProcessorState::default()];
                                let mut bank = FilterBank::new(&[], 0).unwrap();
                                for block_index in 0..3 {
                                    let mut block: LaneBlock = std::array::from_fn(|i| std::array::from_fn(|k| {
                                        match block_index {
                                            0 => if i == 0 { 1. - k as f64 * 0.1 } else { 0. },
                                            1 => ((i + k) as f64 * 0.137).sin() * 0.2,
                                            _ => if i % 2 == 0 { -0.0 } else { f64::from_bits(1) },
                                        }
                                    }));
                                    let mut expected = block;
                                    let mut planar = [[0.; BLOCK]; 2];
                                    for i in 0..BLOCK {
                                        (planar[0][i], planar[1][i]) = (block[i][0], block[i][1]);
                                    }
                                    for (i, frame) in expected[..len].iter_mut().enumerate() {
                                        for k in 0..LANES {
                                            let (y, next) = reference(filter, frame[k], [expected_z[0][k], expected_z[1][k]]);
                                            frame[k] = y;
                                            if i < batch.ends[k] {
                                                (expected_z[0][k], expected_z[1][k]) = (next[0], next[1]);
                                            }
                                        }
                                    }
                                    if masked {
                                        biquad::<true>(&mut z, &mut block, &batch, &filter);
                                    } else {
                                        biquad::<false>(&mut z, &mut block, &batch, &filter);
                                        super::super::process::<false>(
                                            &[PreparedProcessor::Biquad(filter)], &mut scalar,
                                            &mut planar, len, &[], 0, &mut [],
                                            &mut super::super::svf::FilterContext {
                                                bank: &mut bank, expression: None,
                                                reverbs: &mut [], convolutions: &mut [],
                                            }, None,
                                        );
                                        for i in 0..len {
                                            for c in 0..2 {
                                                assert_eq!(planar[c][i].to_bits(), expected[i][c].to_bits());
                                            }
                                        }
                                        for c in 0..2 {
                                            assert_eq!(scalar[0].z[c].map(f64::to_bits),
                                                [expected_z[0][c], expected_z[1][c]].map(flush).map(f64::to_bits));
                                        }
                                    }
                                    assert_eq!(block.map(|frame| frame.map(f64::to_bits)), expected.map(|frame| frame.map(f64::to_bits)));
                                    assert_eq!(z.map(|row| row.map(f64::to_bits)), expected_z.map(|row| row.map(f64::to_bits)));
                                    // Both public processing paths flush retained histories per block.
                                    z = z.map(|row| row.map(flush));
                                    expected_z = expected_z.map(|row| row.map(flush));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
