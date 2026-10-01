//! Algorithmic reverb for Kontakt's modern "Reverb" (`BParFXGaloisReverb`).
//!
//! An 8-line feedback delay network with a Hadamard mix, per-line damping and
//! slow delay modulation. Kontakt's algorithm is not public; this reproduces
//! the controls, not the exact sound. Normalized parameter mappings are
//! documented (with confidence) in `audits/EFFECTS.md`.

use super::params;
use std::f32::consts::TAU;

const LINES: usize = 8;
/// Mutually prime-ish base lengths in ms (Hall scale), spread over ~1 octave.
const BASE_MS: [f32; LINES] = [31.7, 37.1, 41.3, 43.9, 53.3, 59.9, 67.1, 73.7];
const MAX_PREDELAY_MS: f32 = 250.0;
const ALLPASS_MS: [f32; 2] = [4.77, 3.59];
/// Tiny DC bias keeping decaying feedback out of subnormal floats (slow on x86).
const ANTI_DENORMAL: f32 = 1e-20;
/// Frames between exact LFO values; in between they are interpolated
/// linearly. At 0.7 Hz that is off by under 3e-7 of the swing: a
/// modulated delay off by a hundred-thousandth of a frame.
const LFO_STEP: usize = 16;

/// Power-of-two ring buffer; `mask` wraps indices without branches.
struct Line {
    buf: Vec<f32>,
    mask: usize,
}

impl Line {
    fn new(min_len: usize) -> Self {
        let len = (min_len + 2).next_power_of_two();
        Self {
            buf: vec![0.0; len],
            mask: len - 1,
        }
    }

    #[inline]
    fn read(&self, pos: usize, delay: usize) -> f32 {
        self.buf[pos.wrapping_sub(delay) & self.mask]
    }

    #[inline]
    fn write(&mut self, pos: usize, value: f32) {
        self.buf[pos & self.mask] = value;
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
    }
}

pub struct Reverb {
    pos: usize,
    predelay: Line,
    predelay_len: usize,
    /// Input diffusion allpasses, `[channel][stage]`.
    allpass: [[Line; 2]; 2],
    allpass_len: [usize; 2],
    diffusion: f32,
    /// The feedback lines' ring, one frame of all [`LINES`] per entry:
    /// a frame's writes are one store and its reads share one mask. The
    /// length is a power of two.
    lines: Box<[[f32; LINES]]>,
    delay: [f32; LINES],
    feedback: [f32; LINES],
    damp_coef: f32,
    damp_state: [f32; LINES],
    mod_depth: f32,
    lfo_phase: f32,
    lfo_step: f32,
    /// `lfo_phase.sin_cos()`.
    lfo: [f32; 2],
    input_coef: f32,
    /// 0 while frozen: the input no longer reaches the network.
    input_gain: f32,
    input_state: [f32; 2],
    shelf_coef: f32,
    shelf_gain: f32,
    shelf_state: [f32; 2],
    width: f32,
    /// Frames the network takes to decay by 60 dB.
    rt60: f32,
    rate: f32,
}

/// One-pole lowpass coefficient for cutoff `hz`.
fn one_pole(hz: f32, sample_rate: f32) -> f32 {
    1.0 - (-TAU * hz.min(sample_rate * 0.49) / sample_rate).exp()
}

impl Reverb {
    /// Buffers are sized for the largest room any setting reaches, so
    /// [`set`](Self::set) can change every parameter without allocating.
    pub fn new(p: &params::Reverb, sample_rate: f32) -> Self {
        let ms = |v: f32| v * 0.001 * sample_rate;
        // Hall at full size and full modulation.
        let (scale, mod_depth) = (1.5, ms(1.5));
        let longest = ms(BASE_MS[LINES - 1] * scale);
        let lines = vec![[0.0; LINES]; ((longest + mod_depth) as usize + 4).next_power_of_two()];
        let allpass_max = ALLPASS_MS.map(|t| ms(t * scale).max(1.0) as usize);
        let mut rv = Self {
            pos: 0,
            predelay: Line::new(ms(MAX_PREDELAY_MS) as usize),
            predelay_len: 0,
            allpass: std::array::from_fn(|_| allpass_max.map(Line::new)),
            allpass_len: allpass_max,
            diffusion: 0.0,
            lines: lines.into(),
            delay: [0.0; LINES],
            feedback: [0.0; LINES],
            damp_coef: 0.0,
            damp_state: [0.0; LINES],
            mod_depth: 0.0,
            lfo_phase: 0.0,
            lfo_step: TAU * 0.7 / sample_rate,
            lfo: [0.0, 1.0],
            input_coef: 0.0,
            input_gain: 1.0,
            input_state: [0.0; 2],
            shelf_coef: one_pole(250.0, sample_rate),
            shelf_gain: 0.0,
            shelf_state: [0.0; 2],
            width: 0.0,
            rt60: 0.0,
            rate: sample_rate,
        };
        rv.set(p);
        rv
    }

    /// Apply new settings (a script's `$ENGINE_PAR_RV2_*`) without
    /// allocating; the tail keeps ringing through the change.
    pub fn set(&mut self, p: &params::Reverb) {
        let sample_rate = self.rate;
        let n = |v: f32| v.clamp(0.0, 1.0);
        let ms = |v: f32| v * 0.001 * sample_rate;
        let hall = p.room_type >= 0.5;
        let scale = (0.5 + n(p.size)) * if hall { 1.0 } else { 0.55 };
        let rt60 = 0.2 * 100f32.powf(n(p.time));
        self.mod_depth = ms(n(p.modulation) * 1.5);
        self.delay = BASE_MS.map(|base| ms(base * scale));
        // Each pass through a line of length d must lose 60 dB over rt60.
        self.feedback = self.delay.map(|d| 10f32.powf(-3.0 * d / (rt60 * sample_rate)));
        self.predelay_len = ms(n(p.predelay) * MAX_PREDELAY_MS) as usize;
        self.allpass_len = ALLPASS_MS.map(|t| ms(t * scale).max(1.0) as usize);
        self.diffusion = 0.75 * n(p.diffusion);
        self.damp_coef = one_pole(18_000.0 * 0.05f32.powf(n(p.damping)), sample_rate);
        self.input_coef = one_pole(20_000.0 * 0.025f32.powf(n(p.high_cut)), sample_rate);
        self.shelf_gain = 10f32.powf(-18.0 * n(p.low_shelf) / 20.0) - 1.0;
        self.width = n(p.stereo);
        self.rt60 = rt60 * sample_rate;
        // Freeze: lossless, undamped feedback holds the tail; new input is muted.
        let frozen = p.freeze >= 0.5;
        self.input_gain = if frozen { 0.0 } else { 1.0 };
        if frozen {
            (self.feedback, self.damp_coef, self.rt60) = ([1.0; LINES], 1.0, f32::INFINITY);
        }
    }

    /// Frames of output after input at most `peak` falls silent, until it
    /// is below −120 dBFS: 60 dB per reverb time.
    pub fn tail(&self, peak: f32) -> usize {
        let db = 20.0 * (peak / super::processor::SILENCE).max(1.0).log10();
        // A frozen tail rings for good; the cap keeps summed tails from overflowing.
        self.predelay_len + ((self.rt60 * db / 60.0) as usize).min(usize::MAX / 64)
    }

    /// Silences the network.
    pub fn clear(&mut self) {
        self.predelay.clear();
        self.allpass.iter_mut().flatten().for_each(Line::clear);
        self.lines.fill([0.0; LINES]);
        self.damp_state = [0.0; LINES];
        self.input_state = [0.0; 2];
        self.shelf_state = [0.0; 2];
    }

    /// Replaces `left`/`right` with the wet (100%) reverb signal. Runs the
    /// lines with AVX2 when the CPU has it, to the same result.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the running CPU supports AVX2.
            return unsafe { self.process_avx2(left, right) };
        }
        self.process_with::<false>(left, right);
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn process_avx2(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_with::<true>(left, right);
    }

    /// `AVX2` only from [`process_avx2`](Self::process_avx2).
    #[inline(always)]
    fn process_with<const AVX2: bool>(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.chunks_mut(LFO_STEP).zip(right.chunks_mut(LFO_STEP)) {
            let from = self.lfo;
            for _ in 0..left.len() {
                self.advance_lfo();
            }
            let (sin, cos) = self.lfo_phase.sin_cos();
            self.lfo = [sin, cos];
            let n = left.len() as f32;
            let slope = [(sin - from[0]) / n, (cos - from[1]) / n];
            for (i, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let t = (i + 1) as f32;
                let lfo = [from[0] + slope[0] * t, from[1] + slope[1] * t];
                [*l, *r] = self.tick::<AVX2>([*l, *r], lfo);
            }
        }
    }

    fn advance_lfo(&mut self) {
        self.lfo_phase += self.lfo_step;
        if self.lfo_phase > TAU {
            self.lfo_phase -= TAU;
        }
    }

    /// One frame; `[sin, cos]` of the LFO phase.
    #[inline(always)]
    fn tick<const AVX2: bool>(&mut self, input: [f32; 2], lfo: [f32; 2]) -> [f32; 2] {
        let pos = self.pos;
        self.pos = pos.wrapping_add(1);

        // The input is summed to mono; the network decorrelates the outputs.
        self.predelay.write(pos, 0.5 * self.input_gain * (input[0] + input[1]));
        let dry = self.predelay.read(pos, self.predelay_len);

        let mut diffused = [0.0; 2];
        for (ch, out) in diffused.iter_mut().enumerate() {
            let state = &mut self.input_state[ch];
            *state += (dry - *state) * self.input_coef + ANTI_DENORMAL;
            let mut x = *state;
            for (line, &len) in self.allpass[ch].iter_mut().zip(&self.allpass_len) {
                let delayed = line.read(pos, len);
                let v = x + self.diffusion * delayed;
                line.write(pos, v);
                x = delayed - self.diffusion * v;
            }
            *out = x;
        }

        #[cfg(target_arch = "x86_64")]
        // SAFETY: `AVX2` is only set on CPUs that support it.
        let taps = if AVX2 { unsafe { self.lines_avx2(pos, lfo, diffused) } } else { self.lines(pos, lfo, diffused) };
        #[cfg(not(target_arch = "x86_64"))]
        let taps = self.lines(pos, lfo, diffused);

        let l = (taps[0] - taps[2] + taps[4] - taps[6]) * 0.5;
        let r = (taps[1] - taps[3] + taps[5] - taps[7]) * 0.5;
        let (mid, side) = (0.5 * (l + r), 0.5 * (l - r) * self.width);
        let mut out = [mid + side, mid - side];
        for (ch, v) in out.iter_mut().enumerate() {
            let low = &mut self.shelf_state[ch];
            *low += (*v - *low) * self.shelf_coef;
            *v += *low * self.shelf_gain;
        }
        out
    }

    /// Reads the modulated taps of every line, mixes them and writes the
    /// damped feedback plus `diffused` input back; returns the taps.
    #[inline(always)]
    fn lines(&mut self, pos: usize, [sin, cos]: [f32; 2], diffused: [f32; 2]) -> [f32; LINES] {
        // Quadrature LFOs on alternate lines decorrelate modes without pitch wobble.
        let lfo = [sin, cos, -sin, -cos, sin, cos, -sin, -cos];
        let mask = self.lines.len() - 1;
        let taps: [f32; LINES] = std::array::from_fn(|i| {
            // A linear-interpolated read at a fractional delay.
            let d = self.delay[i] + self.mod_depth * (0.5 + 0.5 * lfo[i]);
            let whole = d as usize;
            let frac = d - whole as f32;
            let a = self.lines[pos.wrapping_sub(whole) & mask][i];
            let b = self.lines[pos.wrapping_sub(whole + 1) & mask][i];
            a + (b - a) * frac
        });
        let mut mix = taps;
        hadamard(&mut mix);
        let row = &mut self.lines[pos & mask];
        for (i, (y, damped)) in row.iter_mut().zip(&mut self.damp_state).enumerate() {
            *damped += (mix[i] * self.feedback[i] - *damped) * self.damp_coef + ANTI_DENORMAL;
            *y = *damped + diffused[i % 2];
        }
        taps
    }

    /// [`lines`](Self::lines) with the eight lines in the lanes of one
    /// vector: the same operations in the same order, so the same result.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    fn lines_avx2(&mut self, pos: usize, [sin, cos]: [f32; 2], [dl, dr]: [f32; 2]) -> [f32; LINES] {
        use std::arch::x86_64::*;
        let mask = self.lines.len() - 1;
        let lfo = _mm256_setr_ps(sin, cos, -sin, -cos, sin, cos, -sin, -cos);
        let half = _mm256_set1_ps(0.5);
        // SAFETY: loads of `[f32; 8]` arrays; the gathers read
        // `lines[(pos - k) & mask][lane]`, inside the ring.
        unsafe {
            let depth = _mm256_mul_ps(_mm256_set1_ps(self.mod_depth), _mm256_add_ps(half, _mm256_mul_ps(half, lfo)));
            let d = _mm256_add_ps(_mm256_loadu_ps(self.delay.as_ptr()), depth);
            let whole = _mm256_cvttps_epi32(d);
            let frac = _mm256_sub_ps(d, _mm256_cvtepi32_ps(whole));
            // Only the low bits survive the mask, so 32-bit positions do.
            let at = _mm256_sub_epi32(_mm256_set1_epi32(pos as i32), whole);
            let (m, lane) = (_mm256_set1_epi32(mask as i32), _mm256_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7));
            let index = |at| _mm256_add_epi32(_mm256_slli_epi32::<3>(_mm256_and_si256(at, m)), lane);
            let base = self.lines.as_ptr().cast::<f32>();
            let a = _mm256_i32gather_ps::<4>(base, index(at));
            let b = _mm256_i32gather_ps::<4>(base, index(_mm256_sub_epi32(at, _mm256_set1_epi32(1))));
            let taps = _mm256_add_ps(a, _mm256_mul_ps(_mm256_sub_ps(b, a), frac));

            // Hadamard butterflies: the lower lane of each pair takes a + b,
            // the upper a - b.
            let mut x = taps;
            let swap = _mm256_permute_ps::<0b1011_0001>(x);
            x = _mm256_blend_ps::<0b1010_1010>(_mm256_add_ps(x, swap), _mm256_sub_ps(swap, x));
            let swap = _mm256_permute_ps::<0b0100_1110>(x);
            x = _mm256_blend_ps::<0b1100_1100>(_mm256_add_ps(x, swap), _mm256_sub_ps(swap, x));
            let swap = _mm256_permute2f128_ps::<0x01>(x, x);
            x = _mm256_blend_ps::<0b1111_0000>(_mm256_add_ps(x, swap), _mm256_sub_ps(swap, x));
            let mix = _mm256_mul_ps(x, _mm256_set1_ps((LINES as f32).sqrt().recip()));

            let damped = _mm256_loadu_ps(self.damp_state.as_ptr());
            let step = _mm256_mul_ps(
                _mm256_sub_ps(_mm256_mul_ps(mix, _mm256_loadu_ps(self.feedback.as_ptr())), damped),
                _mm256_set1_ps(self.damp_coef),
            );
            let damped = _mm256_add_ps(damped, _mm256_add_ps(step, _mm256_set1_ps(ANTI_DENORMAL)));
            _mm256_storeu_ps(self.damp_state.as_mut_ptr(), damped);
            let row = &mut self.lines[pos & mask];
            _mm256_storeu_ps(row.as_mut_ptr(), _mm256_add_ps(damped, _mm256_setr_ps(dl, dr, dl, dr, dl, dr, dl, dr)));
            let mut out = [0.0; LINES];
            _mm256_storeu_ps(out.as_mut_ptr(), taps);
            out
        }
    }
}

/// In-place orthonormal 8-point Walsh-Hadamard transform.
#[inline]
fn hadamard(x: &mut [f32; LINES]) {
    let mut h = 1;
    while h < LINES {
        for i in (0..LINES).step_by(2 * h) {
            for j in i..i + h {
                let (a, b) = (x[j], x[j + h]);
                x[j] = a + b;
                x[j + h] = a - b;
            }
        }
        h *= 2;
    }
    let norm = (LINES as f32).sqrt().recip();
    for v in x {
        *v *= norm;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(time: f32) -> params::Reverb {
        params::Reverb {
            room_type: 0.0,
            time,
            size: 0.5,
            damping: 0.5,
            modulation: 0.5,
            diffusion: 0.5,
            predelay: 0.0,
            high_cut: 0.0,
            low_shelf: 0.0,
            stereo: 1.0,
            freeze: 0.0,
        }
    }

    fn energy(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum()
    }

    #[test]
    fn impulse_decays_and_stays_finite() {
        let sr = 48_000.0;
        let mut rv = Reverb::new(&params(0.37), sr);
        let mut l = vec![0.0; 4 * 48_000];
        let mut r = l.clone();
        l[0] = 1.0;
        r[0] = 1.0;
        for (l, r) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
            rv.process(l, r);
        }
        assert!(l.iter().chain(&r).all(|v| v.is_finite()));
        let early = energy(&l[..24_000]);
        let late = energy(&l[3 * 48_000..]);
        assert!(early > 1e-3, "reverb produced no tail: {early}");
        // rt60 ~1.1 s: the fourth second must be far below the first half-second.
        assert!(late < early * 1e-4, "early {early} late {late}");
    }

    #[test]
    fn freeze_holds_the_tail_and_mutes_the_input() {
        let mut p = params(0.2);
        let mut rv = Reverb::new(&p, 48_000.0);
        let run = |rv: &mut Reverb, input: f32, frames: usize| {
            let (mut l, mut r) = (vec![input; frames], vec![input; frames]);
            for (l, r) in l.chunks_mut(128).zip(r.chunks_mut(128)) {
                rv.process(l, r);
            }
            energy(&l)
        };
        run(&mut rv, 0.1, 4_800);
        p.freeze = 1.0;
        rv.set(&p);
        let held = run(&mut rv, 0.0, 48_000);
        // A short room would fall by far more than 60 dB in these 3 s; loud
        // input meanwhile must not reach the frozen tail.
        let later = run(&mut rv, 1.0, 3 * 48_000) / 3.0;
        assert!(held > 0.0 && later > 0.5 * held && later < 2.0 * held, "{held} {later}");
        assert!(rv.tail(1.0) > 48_000 * 3600);
        p.freeze = 0.0;
        rv.set(&p);
        run(&mut rv, 0.0, 3 * 48_000);
        assert!(run(&mut rv, 0.0, 48_000) < held * 1e-6);
    }

    #[test]
    fn maximum_time_is_stable() {
        let mut rv = Reverb::new(&params(1.0), 44_100.0);
        let mut l = vec![0.5; 256];
        let mut r = vec![-0.5; 256];
        for _ in 0..2_000 {
            rv.process(&mut l, &mut r);
            assert!(l.iter().chain(&r).all(|v| v.is_finite() && v.abs() < 100.0));
            l.fill(0.0);
            r.fill(0.0);
        }
    }

    /// The interpolated LFO against an exact one every frame: within
    /// -100 dB of the peak.
    #[test]
    fn interpolated_lfo_matches_exact() {
        let mut p = params(0.6);
        p.modulation = 1.0;
        let (mut fast, mut exact) = (Reverb::new(&p, 48_000.0), Reverb::new(&p, 48_000.0));
        let input: Vec<f32> = (0..96_000).map(|i| (i as f32 * 0.05).sin() * (i as f32 * 0.0007).cos()).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        for (l, r) in l.chunks_mut(100).zip(r.chunks_mut(100)) {
            fast.process(l, r);
        }
        let (mut worst, mut peak) = (0f32, 0f32);
        for (i, &x) in input.iter().enumerate() {
            exact.advance_lfo();
            let (sin, cos) = exact.lfo_phase.sin_cos();
            let [el, er] = exact.tick::<false>([x, x], [sin, cos]);
            worst = worst.max((el - l[i]).abs()).max((er - r[i]).abs());
            peak = peak.max(el.abs());
        }
        assert!(worst < 1e-5 * peak, "worst {worst} of peak {peak}");
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn avx2_lines_match_scalar_bit_for_bit() {
        if !std::arch::is_x86_feature_detected!("avx2") {
            return;
        }
        let (mut simd, mut scalar) = (Reverb::new(&params(0.8), 44_100.0), Reverb::new(&params(0.8), 44_100.0));
        let input: Vec<f32> = (0..20_000).map(|i| (i as f32 * 0.03).sin()).collect();
        let (mut l, mut r) = (input.clone(), input.clone());
        let (mut l2, mut r2) = (input.clone(), input);
        for c in 0..l.len() / 64 {
            let range = c * 64..(c + 1) * 64;
            // SAFETY: checked above.
            unsafe { simd.process_avx2(&mut l[range.clone()], &mut r[range.clone()]) };
            scalar.process_with::<false>(&mut l2[range.clone()], &mut r2[range]);
        }
        let bits = |v: &[f32]| v.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!((bits(&l), bits(&r)), (bits(&l2), bits(&r2)));
    }

    #[test]
    fn hadamard_preserves_energy() {
        let mut x = [1.0, 0.0, 0.5, -2.0, 0.0, 0.25, 0.0, 1.0];
        let before = energy(&x);
        hadamard(&mut x);
        assert!((energy(&x) - before).abs() < 1e-5);
    }
}
