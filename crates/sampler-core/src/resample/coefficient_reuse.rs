//! Test-only exact coefficient reuse. No PCM, voice state or production path is cached.
use super::*;
use crate::Frame;

const FRAMES: usize = 64;
const ROW: usize = MAX_WIDTH * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    bank: usize,
    fraction: u64,
    step: u64,
    frames: usize,
    fused: bool,
}

impl Key {
    fn new<const FUSED: bool>(kernel: &Kernel, fraction: f64, step: f64, frames: usize)
        -> Option<(Self, &'static Polyphase)> {
        if frames < 4 || frames > FRAMES || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction)
            || kernel.quality != ResampleQuality::Realtime || !step.is_finite() || !(1.0 < step && step <= 2.0)
        { return None; }
        // Kernel's banks are immutable OnceLock storage: no address reuse/ABA.
        let banks: &'static [Polyphase; STRETCHES] = kernel.bank;
        let bank = banks.iter().find(|bank| bank.stretch >= step)?;
        Some((Self { bank: bank as *const _ as usize, fraction: fraction.to_bits(),
            step: step.to_bits(), frames, fused: FUSED }, bank))
    }
}

/// One cohort only. All capacity is prepared off audio; filling never reallocates
/// or clears unrelated rows. Assets/generations are deliberately absent from
/// the key: weights are independent of every source sample and source lifetime.
struct Tile {
    key: Option<Key>,
    weights: Box<[f32]>,
    fills: usize,
}

struct View<'a, const FUSED: bool> {
    weights: &'a [f32],
    stride: usize,
    frames: usize,
}

impl Tile {
    fn new() -> Self {
        Self { key: None, weights: vec![0.; FRAMES * ROW].into_boxed_slice(), fills: 0 }
    }

    #[inline(always)]
    fn fill<const FUSED: bool>(&mut self, key: Key, bank: &Polyphase) {
        if self.key == Some(key) { return; }
        assert_eq!(key.bank, bank as *const _ as usize);
        assert_eq!(key.fused, FUSED);
        let mut fraction = f64::from_bits(key.fraction);
        let step = f64::from_bits(key.step);
        for frame in 0..key.frames {
            let position = fraction * PHASES as f64;
            let phase = (position as u32 as usize).min(PHASES - 1);
            let t = (position - phase as f64) as f32;
            let (a, b) = bank.rows[phase * bank.stride..][..2 * bank.stride].split_at(bank.stride);
            for (i, (&a, &b)) in a.iter().zip(b).enumerate() {
                self.weights[frame * ROW + i] = if FUSED { t.mul_add(b - a, a) } else { a + t * (b - a) };
            }
            // Literal source run recurrence, never fraction + frame * step.
            let phase = fraction + step;
            let whole = phase as i64;
            fraction = phase - whole as f64;
        }
        self.key = Some(key);
        self.fills += 1;
    }

    fn view<const FUSED: bool>(&self, key: Key, bank: &Polyphase) -> Option<View<'_, FUSED>> {
        (self.key == Some(key) && key.fused == FUSED && key.bank == bank as *const _ as usize)
            .then_some(View { weights: &self.weights, stride: bank.stride, frames: key.frames })
    }
}

impl<const FUSED: bool> View<'_, FUSED> {
    #[inline(always)]
    fn four(&self, first: usize, windows: [&[Frame]; 4]) -> [Frame; 4] {
        assert!(first + 4 <= self.frames);
        let mut rows: [&[[f32; CHUNK]]; 4] = [&[]; 4];
        let mut inputs: [&[[f32; CHUNK]]; 4] = [&[]; 4];
        for frame in 0..4 {
            rows[frame] = self.weights[(first + frame) * ROW..][..self.stride].as_chunks::<CHUNK>().0;
            inputs[frame] = windows[frame].as_flattened()[..self.stride].as_chunks::<CHUNK>().0;
        }
        let mut sums = [[[0_f32; 4]; 2]; 4];
        for chunk in 0..self.stride / CHUNK {
            for frame in 0..4 {
                let weights = &rows[frame][chunk];
                let input = &inputs[frame][chunk];
                for half in 0..2 {
                    for k in 0..4 {
                        let i = 4 * half + k;
                        sums[frame][half][k] = if FUSED {
                            weights[i].mul_add(input[i], sums[frame][half][k])
                        } else { sums[frame][half][k] + weights[i] * input[i] };
                    }
                }
            }
        }
        std::array::from_fn(|frame| {
            let sum = std::hint::black_box(sums[frame]);
            let half: [f32; 4] = std::array::from_fn(|k| sum[0][k] + sum[1][k]);
            [half[0] + half[2], half[1] + half[3]]
        })
    }
}

/// Do not initialize/fill a tile for unique phases, plain/cubic/high quality,
/// short tails or unsupported clocks. A single repeated cohort is bounded.
fn cohort(keys: &[Option<Key>]) -> u8 {
    assert!(keys.len() <= 8);
    let mut selected = 0_u8;
    for (i, key) in keys.iter().enumerate() {
        let Some(key) = key else { continue };
        let mask = keys.iter().enumerate().fold(0_u8, |mask, (j, other)|
            mask | if other == &Some(*key) { 1 << j } else { 0 });
        if mask.count_ones() >= 4 && mask.count_ones() > selected.count_ones() {
            selected = mask;
        }
        if selected == 0xff || i == 7 { break; }
    }
    selected
}

#[test]
fn tile_weights_preserve_fused_unfused_outputs_and_source_replacement() {
    let kernel = Kernel::new(ResampleQuality::Realtime);
    let mut tile = Tile::new();
    assert_eq!(tile.weights.len() * std::mem::size_of::<f32>(), 26 * 1024);
    for fused in [false, true] {
        for step in [1.0000001, 1.09, 1.201, 1.5, 1.9999999, 2.] {
            for fraction in [-0., 0., f64::from_bits(1), 0.001, 0.37, 1. - f64::EPSILON, 1.] {
                let (key, bank) = if fused { Key::new::<true>(&kernel, fraction, step, 64) }
                    else { Key::new::<false>(&kernel, fraction, step, 64) }.unwrap();
                if fused { tile.fill::<true>(key, bank); } else { tile.fill::<false>(key, bank); }
                let fills = tile.fills;
                if fused { tile.fill::<true>(key, bank); } else { tile.fill::<false>(key, bank); }
                assert_eq!(fills, tile.fills);
                let mut fractions = [0.; FRAMES];
                let mut f = fraction;
                for value in &mut fractions {
                    *value = f;
                    let phase = f + step;
                    f = phase - (phase as i64) as f64;
                }
                for generation in 0..3 {
                    let data: Vec<Frame> = (0..MAX_WIDTH + 12).map(|i| match (i + generation) % 11 {
                        0 => [-0., 0.], 1 => [f32::from_bits(1), -f32::MIN_POSITIVE],
                        _ => [((i + generation) as f32 * 0.731).sin(), ((i + 3 * generation) as f32 * 0.417).cos()],
                    }).collect();
                    for first in (0..FRAMES).step_by(4) {
                        let windows = std::array::from_fn(|i| &data[3*i..3*i + bank.width()]);
                        let phases = std::array::from_fn(|i| fractions[first+i]);
                        let expected = if fused { bank.dot_four::<true>(phases, windows) }
                            else { bank.dot_four::<false>(phases, windows) };
                        let actual = if fused { tile.view::<true>(key, bank).unwrap().four(first, windows) }
                            else { tile.view::<false>(key, bank).unwrap().four(first, windows) };
                        assert_eq!(expected.map(|f| f.map(f32::to_bits)), actual.map(|f| f.map(f32::to_bits)));
                    }
                }
            }
        }
    }
}

#[test]
fn cohort_gate_skips_plain_unique_short_invalid_and_changed_phase_keys() {
    let realtime = Kernel::new(ResampleQuality::Realtime);
    let high = Kernel::new(ResampleQuality::High);
    for (quality, step, fraction, frames) in [(&realtime, 1., 0., 64), (&high, 1.201, 0.3, 64),
        (&realtime, 1.201, 0.3, 3), (&realtime, f64::NAN, 0.3, 64),
        (&realtime, 1.201, f64::NAN, 64), (&realtime, 1.201, 0.3, 65)] {
        assert!(Key::new::<true>(quality, fraction, step, frames).is_none());
    }
    let key = Key::new::<true>(&realtime, 0.37, 1.201, 64).unwrap().0;
    let (_, bank) = Key::new::<true>(&realtime, 0.37, 1.201, 64).unwrap();
    let mut tile = Tile::new();
    assert!(std::mem::size_of::<Tile>() + tile.weights.len() * std::mem::size_of::<f32>() <= 27 * 1024);
    assert!(tile.view::<true>(key, bank).is_none());
    tile.fill::<true>(key, bank);
    assert!(tile.view::<true>(key, bank).is_some());
    assert!(tile.view::<false>(key, bank).is_none());
    assert_eq!(cohort(&[Some(key); 8]), 0xff);
    assert_eq!(cohort(&[Some(key); 3]), 0);
    let mut keys = [Some(key); 8];
    for (i, value) in keys.iter_mut().enumerate() {
        value.as_mut().unwrap().fraction = (i as f64 * 0.01).to_bits();
    }
    assert_eq!(cohort(&keys), 0);
    keys[..4].fill(Some(key));
    assert_eq!(cohort(&keys), 0x0f);
    let mut changed = key;
    changed.step = f64::from_bits(changed.step).next_up().to_bits();
    assert_ne!(changed, key);
    assert!(tile.view::<true>(changed, bank).is_none());
    changed = key; changed.fused = false; assert_ne!(changed, key);
    assert!(tile.view::<true>(changed, bank).is_none());
    changed = key; changed.frames = 32; assert_ne!(changed, key);
    assert!(tile.view::<true>(changed, bank).is_none());
    changed = key; changed.bank += 1; assert_ne!(changed, key);
    assert!(tile.view::<true>(changed, bank).is_none());
}

#[inline(always)]
fn render_block<const FUSED: bool>(kernel: &Kernel, tile: &mut Tile, assets: &[Vec<Frame>],
    output: &mut [[Frame; FRAMES]; 8], iteration: usize, voices: usize, shifted: bool, cached: bool) {
    let base = ((iteration % 1021) as f64 + 0.37) / 1021.;
    let mut phases = [[0.; FRAMES]; 8];
    let mut offsets = [[0; FRAMES]; 8];
    let mut keys = [None; 8];
    let mut banks = [&kernel.bank[0]; 8];
    for voice in 0..voices {
        let step = 1.201 + if shifted { voice as f64 * 0.001 } else { 0. };
        let mut fraction = (base + if shifted { voice as f64 * 0.013 } else { 0. }).fract();
        banks[voice] = kernel.polyphase(step).unwrap();
        if cached { keys[voice] = Key::new::<FUSED>(kernel, fraction, step, FRAMES).map(|p| p.0); }
        let mut offset = 0;
        for i in 0..FRAMES {
            phases[voice][i] = fraction;
            offsets[voice][i] = offset;
            let next = fraction + step;
            let whole = next as i64;
            fraction = next - whole as f64;
            offset += whole as usize;
        }
    }
    let mask = if cached { cohort(&keys[..voices]) } else { 0 };
    if mask != 0 {
        let first = mask.trailing_zeros() as usize;
        tile.fill::<FUSED>(keys[first].unwrap(), banks[first]);
    }
    for voice in 0..voices {
        let bank = banks[voice];
        let view = if mask & (1 << voice) != 0 {
            tile.view::<FUSED>(keys[voice].unwrap(), bank)
        } else { None };
        for first in (0..FRAMES).step_by(4) {
            let windows = std::array::from_fn(|i| {
                let offset = offsets[voice][first + i];
                &assets[voice][offset..offset + bank.width()]
            });
            let samples = match &view {
                Some(view) => view.four(first, windows),
                None => bank.dot_four::<FUSED>(std::array::from_fn(|i| phases[voice][first+i]), windows),
            };
            output[voice][first..first+4].copy_from_slice(&samples);
        }
    }
}

#[test]
fn tile_cohorts_preserve_whole_source_blocks_with_fresh_and_unique_phases() {
    let kernel = Kernel::new(ResampleQuality::Realtime);
    let assets: Vec<Vec<Frame>> = (0..8).map(|voice| (0..512).map(|i| {
        [((i + voice) as f32 * 0.731).sin(), ((i + 3*voice) as f32 * 0.417).cos()]
    }).collect()).collect();
    let mut tile = Tile::new();
    for voices in [2, 4, 8] {
        for shifted in [false, true] {
            for iteration in [0, 1, 63, 1020, 1021] {
                for fused in [false, true] {
                    let mut expected = [[[0.; 2]; FRAMES]; 8];
                    let mut actual = expected;
                    let fills = tile.fills;
                    if fused {
                        render_block::<true>(&kernel, &mut tile, &assets, &mut expected, iteration, voices, shifted, false);
                        render_block::<true>(&kernel, &mut tile, &assets, &mut actual, iteration, voices, shifted, true);
                    } else {
                        render_block::<false>(&kernel, &mut tile, &assets, &mut expected, iteration, voices, shifted, false);
                        render_block::<false>(&kernel, &mut tile, &assets, &mut actual, iteration, voices, shifted, true);
                    }
                    assert_eq!(expected.map(|b| b.map(|f| f.map(f32::to_bits))),
                        actual.map(|b| b.map(|f| f.map(f32::to_bits))));
                    if shifted || voices < 4 { assert_eq!(fills, tile.fills); }
                }
            }
        }
    }
}

#[test]
#[ignore = "diagnostic fresh-block coefficient stage; no whole-callback or state integration claim"]
fn coefficient_reuse_stage_evidence() {
    use std::{hint::black_box, time::Instant};
    let kernel = Kernel::new(ResampleQuality::Realtime);
    let assets: Vec<Vec<Frame>> = (0..8).map(|voice| (0..512).map(|i| {
        [((i + voice) as f32 * 0.731).sin(), ((i + 3*voice) as f32 * 0.417).cos()]
    }).collect()).collect();
    let iterations = 2_000;
    for (voices, shifted) in [(2, false), (4, false), (8, false), (8, true)] {
        for round in 0..4 {
            let mut times = [0; 2];
            let mut hashes = [0; 2];
            for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let mut tile = Tile::new();
                tile.weights.fill(0.); // Touch fixed storage outside the measured phase.
                let mut output = [[[0.; 2]; FRAMES]; 8];
                let start = Instant::now();
                for iteration in 0..iterations {
                    sampler_simd::dispatch_fused((black_box(&kernel), black_box(&mut tile),
                        black_box(&assets[..]), black_box(&mut output), black_box(iteration),
                        black_box(voices), black_box(shifted), mode != 0),
                        #[inline(always)] |(k,t,a,o,i,v,s,c)| render_block::<false>(k,t,a,o,i,v,s,c),
                        #[inline(always)] |(k,t,a,o,i,v,s,c)| render_block::<true>(k,t,a,o,i,v,s,c));
                }
                times[mode] = start.elapsed().as_nanos();
                hashes[mode] = output.iter().flatten().flatten().fold(0_u64,
                    |h, x| h.rotate_left(7) ^ u64::from(x.to_bits()));
                assert_eq!(tile.fills, if mode != 0 && voices >= 4 && !shifted { iterations } else { 0 });
            }
            assert_eq!(hashes[0], hashes[1]);
            eprintln!("COEFFICIENT_STAGE {{\"voices\":{voices},\"shifted\":{shifted},\"frames\":{FRAMES},\"iterations\":{iterations},\"round\":{round},\"before_ns\":{},\"candidate_ns\":{},\"hash\":{}}}", times[0], times[1], hashes[0]);
        }
    }
}
