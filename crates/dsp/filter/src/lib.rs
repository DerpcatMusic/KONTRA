//! Filters: one state-variable shape per class (03-DSP.md §3). A fixed
//! state-variable stage is the bilinear transform of its analog prototype, so
//! it runs as a biquad; formats load their own cutoff, damping and gain laws.
//! A cutoff that moves runs the same shape as [`Svf`], whose trapezoidal
//! integrators stay stable and click-free under per-span coefficient changes,
//! which a direct-form biquad's state does not guarantee (its state is not the
//! filter's energy). One shape, two runners: they agree to 1e-5 at a fixed cutoff.

mod cascade;
mod clocked;
mod formant;
mod huovilainen;
mod ladder;
mod notches;
mod pass_bank;
mod sallen_key;
mod spectral_morph;
mod wah;

pub use cascade::{Cascade, CascadeSettings, CascadeStage};
pub use clocked::{Clocked, ClockedShape};
pub use formant::{Formant, VowelSet, Vowels};
pub use huovilainen::Huovilainen;
pub use kontra_dsp_core::Biquad;
use kontra_dsp_core::flush;
pub use ladder::Ladder;
pub use notches::{MAX_STAGES, NotchSettings, Notches};
pub use pass_bank::{PassBank, PassBankSettings};
pub use sallen_key::SallenKey;
pub use spectral_morph::{SpectralMorph, SpectralMorphSettings};
pub use wah::{Wah, WahSettings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Low,
    High,
    Band,
    Notch,
}

/// `stages` two-pole stages in series (none: one pole) at `hz`, each with
/// its damping `k` (`1/Q`), then `gain`. A band stage passes `s/(s² + ks + 1)`,
/// the raw band tap: its peak is `1/k`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub pass: Pass,
    pub hz: f32,
    pub stages: u8,
    /// An alternative kernel instead of the stages; runs in [`Svf`].
    pub ladder: Option<LadderKind>,
    pub damping: [f32; 3],
    pub gain: f32,
}

/// Alternative filter kernels: a first-order pole, a ladder by tap selector
/// 1..=12, or a [`SallenKey`] two-pole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LadderKind {
    /// An exponential first-order low/high-pass.
    OnePole,
    ZeroDelay(u8),
    Explicit {
        taps: u8,
        fat: f32,
    },
    SallenKey {
        morph: f32,
        curve: [f32; 2],
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Kernel {
    OnePole([f64; 2]),
    ZeroDelay(Ladder),
    // Boxed: its halfband is several times the zero-delay ladder; built at load.
    Explicit(Box<Huovilainen>),
    SallenKey(SallenKey),
}

/// The shape's sections at `rate`; the cutoff is held below 0.49 of the rate.
#[must_use]
pub fn sections(shape: &Shape, rate: u32) -> Vec<Biquad> {
    let rate = f64::from(rate.max(1));
    let hz = f64::from(shape.hz).max(1.0).min(0.49 * rate);
    let g = (std::f64::consts::PI * hz / rate).tan();
    let mut out: Vec<Biquad> = if shape.stages == 0 {
        // 1/(s + 1) or s/(s + 1).
        let num = match shape.pass {
            Pass::High => [1.0, -1.0],
            _ => [g, g],
        };
        vec![Biquad::new([num[0], num[1], 0.0, 1.0 + g, g - 1.0, 0.0])]
    } else {
        shape
            .damping
            .iter()
            .take(usize::from(shape.stages))
            .map(|&k| stage(shape.pass, f64::from(k), g))
            .collect()
    };
    if let Some(first) = out.first_mut() {
        first.scale(f64::from(shape.gain));
    }
    out
}

/// `(b2 s² + b1 s + b0)/(s² + k s + 1)` through `s = (1/g)(1 − z⁻¹)/(1 + z⁻¹)`.
fn stage(pass: Pass, k: f64, g: f64) -> Biquad {
    let [b2, b1, b0] = match pass {
        Pass::Low => [0.0, 0.0, 1.0],
        Pass::High => [1.0, 0.0, 0.0],
        Pass::Band => [0.0, 1.0, 0.0],
        Pass::Notch => [1.0, 0.0, 1.0],
    };
    let gg = g * g;
    Biquad::new([
        b2 + b1 * g + b0 * gg,
        2.0 * (b0 * gg - b2),
        b2 - b1 * g + b0 * gg,
        1.0 + k * g + gg,
        2.0 * (gg - 1.0),
        1.0 - k * g + gg,
    ])
}

/// The shape run sample by sample as a zero-delay state-variable filter
/// (trapezoidal integrators), so its cutoff can move every span without
/// clicks. At a fixed cutoff it equals [`sections`].
#[derive(Clone, Debug, PartialEq)]
pub struct Svf {
    shape: Shape,
    hz: f32,
    g: f64,
    one_pole_a: f64,
    /// Per stage and channel, the two integrator states.
    state: [[[f64; 2]; 2]; 3],
    rate: f64,
    ladder: Option<Kernel>,
    /// An explicit ladder's box kept for a switch into one, so the audio
    /// thread neither allocates nor frees it ([`Self::keep_spare`]).
    spare: Option<Box<Huovilainen>>,
}

impl Svf {
    #[must_use]
    pub fn new(shape: Shape) -> Self {
        let mut svf = Self {
            shape,
            hz: 0.0,
            g: 0.0,
            one_pole_a: 1.0,
            state: [[[0.0; 2]; 2]; 3],
            rate: 1.0,
            ladder: None,
            spare: None,
        };
        if let Some(LadderKind::Explicit { taps, fat }) = shape.ladder {
            svf.spare = Some(Box::new(Huovilainen::new(taps, fat)));
        }
        let ladder = shape.ladder.and_then(|l| svf.kernel(l));
        svf.ladder = ladder;
        svf
    }

    /// Keeps an explicit ladder's box at hand, off the audio thread, so a
    /// later [`Self::retune`] can switch to any ladder.
    pub fn keep_spare(&mut self) {
        let explicit = matches!(self.ladder, Some(Kernel::Explicit(_)));
        if !explicit && self.spare.is_none() {
            self.spare = Some(Box::new(Huovilainen::new(1, 1.0)));
        }
    }

    /// A kernel for `kind` without allocating: an explicit ladder takes the
    /// spare box; `None` without one.
    fn kernel(&mut self, kind: LadderKind) -> Option<Kernel> {
        Some(match kind {
            LadderKind::OnePole => Kernel::OnePole([0.0; 2]),
            LadderKind::ZeroDelay(taps) => Kernel::ZeroDelay(Ladder::new(taps)),
            LadderKind::Explicit { taps, fat } => {
                let mut b = self.spare.take()?;
                *b = Huovilainen::new(taps, fat);
                Kernel::Explicit(b)
            }
            LadderKind::SallenKey { morph, curve } => {
                Kernel::SallenKey(SallenKey::new(morph, curve))
            }
        })
    }

    /// Moves the cutoff to `hz` at `rate`, held below 0.49 of the rate.
    pub fn set_cutoff(&mut self, hz: f32, rate: u32) {
        let rate = f64::from(rate.max(1));
        let hz = f64::from(hz).max(1.0).min(0.49 * rate);
        // Same cutoff and rate give the same `g` and `one_pole_a`; trig/exp is the cost.
        if self.hz != hz as f32 || self.rate != rate || self.g == 0.0 {
            self.g = (std::f64::consts::PI * hz / rate).tan();
            self.one_pole_a = (-std::f64::consts::TAU * hz / rate).exp();
        }
        self.hz = hz as f32;
        self.rate = rate;
    }

    pub fn process(&mut self, io: &mut [[f32; 2]]) {
        if io.is_empty() {
            return;
        }
        let (g, gain) = (self.g, f64::from(self.shape.gain));
        if let Some(ladder) = &mut self.ladder {
            let last = usize::from(self.shape.stages.clamp(1, 3) - 1);
            let k = self.shape.damping.get(last).copied().unwrap_or(2.0);
            let (hz, r) = (f64::from(self.hz), 1.0 - f64::from(k) / 2.0);
            match ladder {
                Kernel::OnePole(state) => {
                    if *state == [0.0; 2] && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0) {
                        return;
                    }
                    let one_minus_a = 1.0 - self.one_pole_a;
                    let [mut sl, mut sr] = *state;
                    if self.shape.pass == Pass::High {
                        for frame in io {
                            let xl = f64::from(frame[0]);
                            let xr = f64::from(frame[1]);
                            sl += (xl - sl) * one_minus_a;
                            sr += (xr - sr) * one_minus_a;
                            frame[0] = ((xl - sl) * gain) as f32;
                            frame[1] = ((xr - sr) * gain) as f32;
                        }
                    } else {
                        for frame in io {
                            let xl = f64::from(frame[0]);
                            let xr = f64::from(frame[1]);
                            sl += (xl - sl) * one_minus_a;
                            sr += (xr - sr) * one_minus_a;
                            frame[0] = (sl * gain) as f32;
                            frame[1] = (sr * gain) as f32;
                        }
                    }
                    *state = [sl, sr];
                    flush(state);
                }
                Kernel::ZeroDelay(l) => {
                    l.tune(hz, self.rate, r);
                    l.process(io, gain);
                }
                Kernel::Explicit(l) => {
                    l.tune(hz, self.rate, r);
                    l.process(io, gain);
                }
                Kernel::SallenKey(l) => {
                    l.tune(hz, self.rate, r);
                    l.process(io, gain);
                }
            }
            return;
        }
        if self.state.iter().flatten().flatten().all(|&s| s == 0.0)
            && io.iter().all(|f| f[0] == 0.0 && f[1] == 0.0)
        {
            return;
        }
        let stages = usize::from(self.shape.stages);
        let pass = self.shape.pass;
        if stages == 0 {
            let g_norm = g / (1.0 + g);
            let mut sl = self.state[0][0][0];
            let mut sr = self.state[0][1][0];
            if pass == Pass::High {
                for frame in io {
                    let vl = f64::from(frame[0]);
                    let vr = f64::from(frame[1]);
                    let low_l = (vl - sl) * g_norm + sl;
                    let low_r = (vr - sr) * g_norm + sr;
                    sl = 2.0 * low_l - sl;
                    sr = 2.0 * low_r - sr;
                    frame[0] = ((vl - low_l) * gain) as f32;
                    frame[1] = ((vr - low_r) * gain) as f32;
                }
            } else {
                for frame in io {
                    let vl = f64::from(frame[0]);
                    let vr = f64::from(frame[1]);
                    let low_l = (vl - sl) * g_norm + sl;
                    let low_r = (vr - sr) * g_norm + sr;
                    sl = 2.0 * low_l - sl;
                    sr = 2.0 * low_r - sr;
                    frame[0] = (low_l * gain) as f32;
                    frame[1] = (low_r * gain) as f32;
                }
            }
            self.state[0][0][0] = sl;
            self.state[0][1][0] = sr;
            flush(self.state.as_flattened_mut().as_flattened_mut());
            return;
        }
        #[inline(always)]
        fn svf_stage(
            pass: Pass,
            v: [f64; 2],
            ic1: &mut [f64; 2],
            ic2: &mut [f64; 2],
            [k, a1, a2, a3]: [f64; 4],
        ) -> [f64; 2] {
            let v3_l = v[0] - ic2[0];
            let v3_r = v[1] - ic2[1];
            let band_l = a1 * ic1[0] + a2 * v3_l;
            let band_r = a1 * ic1[1] + a2 * v3_r;
            let low_l = ic2[0] + a2 * ic1[0] + a3 * v3_l;
            let low_r = ic2[1] + a2 * ic1[1] + a3 * v3_r;
            ic1[0] = 2.0 * band_l - ic1[0];
            ic1[1] = 2.0 * band_r - ic1[1];
            ic2[0] = 2.0 * low_l - ic2[0];
            ic2[1] = 2.0 * low_r - ic2[1];
            match pass {
                Pass::Low => [low_l, low_r],
                Pass::Band => [band_l, band_r],
                Pass::High => [v[0] - k * band_l - low_l, v[1] - k * band_r - low_r],
                Pass::Notch => [v[0] - k * band_l, v[1] - k * band_r],
            }
        }
        #[inline(always)]
        fn run_n<const N: usize>(
            io: &mut [[f32; 2]],
            state: &mut [[[f64; 2]; 2]; 3],
            c: [[f64; 4]; 3],
            pass: Pass,
            gain: f64,
        ) {
            let mut ic1 = [[0.0f64; 2]; N];
            let mut ic2 = [[0.0f64; 2]; N];
            for ((i1, i2), st) in ic1.iter_mut().zip(&mut ic2).zip(state.iter()) {
                *i1 = [st[0][0], st[1][0]];
                *i2 = [st[0][1], st[1][1]];
            }
            for frame in io {
                let mut v = [f64::from(frame[0]), f64::from(frame[1])];
                for ((i1, i2), &cs) in ic1.iter_mut().zip(&mut ic2).zip(c.iter()) {
                    v = svf_stage(pass, v, i1, i2, cs);
                }
                frame[0] = (v[0] * gain) as f32;
                frame[1] = (v[1] * gain) as f32;
            }
            for (st, (&i1, &i2)) in state.iter_mut().zip(ic1.iter().zip(&ic2)) {
                *st = [[i1[0], i2[0]], [i1[1], i2[1]]];
            }
        }
        // Per stage: k, then the TPT gains a1, a2, a3.
        let c = self.shape.damping.map(|k| {
            let k = f64::from(k);
            let a1 = 1.0 / (1.0 + g * (g + k));
            [k, a1, g * a1, g * g * a1]
        });
        match (stages, pass) {
            (1, Pass::Low) => run_n::<1>(io, &mut self.state, c, Pass::Low, gain),
            (1, Pass::High) => run_n::<1>(io, &mut self.state, c, Pass::High, gain),
            (1, Pass::Band) => run_n::<1>(io, &mut self.state, c, Pass::Band, gain),
            (1, Pass::Notch) => run_n::<1>(io, &mut self.state, c, Pass::Notch, gain),
            (2, Pass::Low) => run_n::<2>(io, &mut self.state, c, Pass::Low, gain),
            (2, Pass::High) => run_n::<2>(io, &mut self.state, c, Pass::High, gain),
            (2, Pass::Band) => run_n::<2>(io, &mut self.state, c, Pass::Band, gain),
            (2, Pass::Notch) => run_n::<2>(io, &mut self.state, c, Pass::Notch, gain),
            (_, Pass::Low) => run_n::<3>(io, &mut self.state, c, Pass::Low, gain),
            (_, Pass::High) => run_n::<3>(io, &mut self.state, c, Pass::High, gain),
            (_, Pass::Band) => run_n::<3>(io, &mut self.state, c, Pass::Band, gain),
            (_, Pass::Notch) => run_n::<3>(io, &mut self.state, c, Pass::Notch, gain),
        }
        flush(self.state.as_flattened_mut().as_flattened_mut());
    }

    /// Follows `shape` at `rate`: cutoff, damping, gain, pass and stage
    /// count move in place, keeping the state. A different ladder (or none)
    /// swaps its kernel, starting from silence, without allocating or
    /// freeing: an explicit ladder needs the spare box, and false without
    /// one (it keeps its shape then).
    pub fn retune(&mut self, shape: Shape, rate: u32) -> bool {
        if shape.ladder != self.shape.ladder {
            // The box a switch leaves becomes the spare, not freed here
            // (only one box ever exists, so the spare is empty then).
            let old = match self.ladder.take() {
                Some(Kernel::Explicit(b)) if self.spare.is_none() => {
                    self.spare = Some(b);
                    None
                }
                other => other,
            };
            if let Some(kind) = shape.ladder {
                match self.kernel(kind) {
                    Some(k) => self.ladder = Some(k),
                    None => {
                        self.ladder = old;
                        return false;
                    }
                }
            }
        }
        self.shape = shape;
        if shape.hz != self.hz {
            self.set_cutoff(shape.hz, rate);
        }
        true
    }

    /// Moves the stages' damping `k` (`1/Q`) without touching the state.
    pub fn set_damping(&mut self, damping: [f32; 3]) {
        self.shape.damping = damping;
    }

    /// The cutoff as last set, in Hz.
    #[must_use]
    pub fn cutoff(&self) -> f32 {
        self.hz
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.ladder.as_ref().is_none_or(|l| match l {
            Kernel::OnePole(state) => state.iter().all(|s| s.abs() < 1e-10),
            Kernel::ZeroDelay(l) => l.is_silent(),
            Kernel::Explicit(l) => l.is_silent(),
            Kernel::SallenKey(l) => l.is_silent(),
        }) && self
            .state
            .iter()
            .flatten()
            .flatten()
            .all(|s| s.abs() < 1e-10)
    }

    pub fn reset(&mut self) {
        self.state = [[[0.0; 2]; 2]; 3];
        match &mut self.ladder {
            Some(Kernel::OnePole(state)) => *state = [0.0; 2],
            Some(Kernel::ZeroDelay(l)) => l.reset(),
            Some(Kernel::Explicit(l)) => l.reset(),
            Some(Kernel::SallenKey(l)) => l.reset(),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_one_pole_matches_the_reference_impulse() {
        for (pass, expected) in [
            (Pass::Low, [0.480_297_33, 0.249_611_81, 0.129_723_92]),
            (Pass::High, [0.519_702_7, -0.249_611_81, -0.129_723_92]),
        ] {
            let mut filter = Svf::new(Shape {
                pass,
                hz: 5000.0,
                stages: 0,
                ladder: Some(LadderKind::OnePole),
                damping: [2.0; 3],
                gain: 1.0,
            });
            filter.set_cutoff(5000.0, 48_000);
            let mut samples = [[1.0, 0.0], [0.0; 2], [0.0; 2]];
            filter.process(&mut samples);
            for (sample, want) in samples.iter().zip(expected) {
                assert!((sample[0] - want).abs() < 1e-6, "{pass:?}: {sample:?}");
                assert_eq!(sample[1], 0.0);
            }
        }
    }

    /// Steady-state gain of a sine at `hz` through the shape, in dB.
    fn gain_db(shape: &Shape, hz: f64) -> f64 {
        let mut f = sections(shape, 48_000);
        let mut io: Vec<[f32; 2]> = (0..48_000)
            .map(|n| {
                let v = (std::f64::consts::TAU * hz * f64::from(n) / 48_000.0).sin() as f32;
                [v, v]
            })
            .collect();
        for s in &mut f {
            s.process(&mut io);
        }
        let peak = io[24_000..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
        20.0 * f64::from(peak).log10()
    }

    fn shape(pass: Pass, stages: u8) -> Shape {
        let k = std::f32::consts::SQRT_2;
        Shape {
            pass,
            hz: 1000.0,
            stages,
            ladder: None,
            damping: [k; 3],
            gain: 1.0,
        }
    }

    /// A switch to another ladder plays as that ladder built fresh, and an
    /// explicit ladder's box is reused, never built or freed in the switch.
    #[test]
    fn a_ladder_switch_plays_the_new_ladder_and_keeps_its_box() {
        let stages = shape(Pass::Low, 1);
        let explicit = Shape {
            ladder: Some(LadderKind::Explicit { taps: 4, fat: 1.0 }),
            ..stages
        };
        let zero_delay = Shape {
            ladder: Some(LadderKind::ZeroDelay(2)),
            ..stages
        };
        let impulse = |n: usize| if n == 0 { [1.0, -0.5] } else { [0.0; 2] };
        let play = |svf: &mut Svf| {
            let mut io: Vec<[f32; 2]> = (0..256).map(impulse).collect();
            svf.process(&mut io);
            io
        };
        let fresh = |shape: Shape| {
            let mut svf = Svf::new(shape);
            svf.set_cutoff(shape.hz, 48_000);
            play(&mut svf)
        };
        let mut plain = Svf::new(stages);
        plain.set_cutoff(stages.hz, 48_000);
        assert!(!plain.retune(explicit, 48_000), "no spare box, no switch");
        let mut svf = Svf::new(stages);
        svf.set_cutoff(stages.hz, 48_000);
        svf.keep_spare();
        let boxed = svf.spare.as_deref().map(std::ptr::from_ref);
        for shape in [explicit, zero_delay, explicit, stages] {
            svf.reset();
            assert!(svf.retune(shape, 48_000));
            assert_eq!(play(&mut svf), fresh(shape), "{:?}", shape.ladder);
            let now = match &svf.ladder {
                Some(Kernel::Explicit(b)) => Some(std::ptr::from_ref(&**b)),
                _ => svf.spare.as_deref().map(std::ptr::from_ref),
            };
            assert_eq!(now, boxed, "the one box moves between ladder and spare");
        }
    }

    #[test]
    fn cutoffs_sit_3_db_down_and_slopes_follow_the_order() {
        for (pass, far) in [(Pass::Low, 4000.0), (Pass::High, 250.0)] {
            assert!((gain_db(&shape(pass, 0), 1000.0) + 3.01).abs() < 0.05);
            assert!((gain_db(&shape(pass, 1), 1000.0) + 3.01).abs() < 0.05);
            // Two octaves out: about 12 and 24 dB down.
            let (one, two) = (gain_db(&shape(pass, 0), far), gain_db(&shape(pass, 1), far));
            assert!((one + 12.3).abs() < 1.0, "{pass:?} {one}");
            assert!((two + 24.1).abs() < 1.0, "{pass:?} {two}");
        }
        assert!(gain_db(&shape(Pass::Low, 1), 100.0).abs() < 0.05);
    }

    #[test]
    fn the_running_filter_matches_its_sections() {
        for (pass, stages) in [
            (Pass::Low, 0),
            (Pass::High, 0),
            (Pass::Band, 2),
            (Pass::Notch, 3),
        ] {
            let shape = Shape {
                damping: [0.7, 1.2, 0.3],
                gain: 0.8,
                ..shape(pass, stages)
            };
            let impulse = |n: usize| if n == 0 { [1.0, -0.5] } else { [0.0; 2] };
            let mut a: Vec<[f32; 2]> = (0..512).map(impulse).collect();
            let mut b = a.clone();
            for s in &mut sections(&shape, 48_000) {
                s.process(&mut a);
            }
            let mut svf = Svf::new(shape);
            svf.set_cutoff(shape.hz, 48_000);
            svf.process(&mut b);
            let worst = a
                .iter()
                .zip(&b)
                .map(|(x, y)| (x[0] - y[0]).abs().max((x[1] - y[1]).abs()));
            assert!(worst.fold(0.0, f32::max) < 1e-5, "{pass:?} {stages}");
        }
    }

    #[test]
    fn a_band_peaks_at_one_over_its_damping_and_a_notch_removes_its_centre() {
        let band = Shape {
            damping: [0.5; 3],
            ..shape(Pass::Band, 1)
        };
        assert!((gain_db(&band, 1000.0) - 20.0 * 2f64.log10()).abs() < 0.05);
        assert!(gain_db(&shape(Pass::Notch, 1), 1000.0) < -40.0);
    }

    #[test]
    fn the_running_filter_is_block_size_free_resets_clean_and_stays_finite() {
        let shape = Shape {
            damping: [0.7, 1.2, 0.3],
            ..shape(Pass::Low, 3)
        };
        let impulse = |n: usize| if n == 0 { [1.0, -0.5] } else { [0.0; 2] };
        let whole: Vec<[f32; 2]> = {
            let mut io: Vec<_> = (0..512).map(impulse).collect();
            let mut svf = Svf::new(shape);
            svf.set_cutoff(shape.hz, 48_000);
            svf.process(&mut io);
            io
        };
        let mut svf = Svf::new(shape);
        svf.set_cutoff(shape.hz, 48_000);
        for chunk in [1, 7, 512] {
            svf.reset();
            let mut io: Vec<_> = (0..512).map(impulse).collect();
            for part in io.chunks_mut(chunk) {
                svf.process(part);
            }
            assert_eq!(io, whole, "chunks of {chunk}");
        }
        for (hz, k, rate) in [(1e9, 0.0, 1), (0.0, 1e9, 384_000), (20.0, 1e-9, 8_000)] {
            let mut svf = Svf::new(Shape {
                damping: [k; 3],
                ..shape
            });
            svf.set_cutoff(hz, rate);
            let mut io: Vec<_> = (0..4096).map(|n| [((n % 7) as f32 - 3.0); 2]).collect();
            svf.process(&mut io);
            assert!(
                io.iter().flatten().all(|v| v.is_finite()),
                "{hz} {k} {rate}"
            );
        }
    }
}

kontra_dsp_core::impl_dsp_core!(PassBank, Wah, Svf, SpectralMorph);
