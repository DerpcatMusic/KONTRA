//! Admitted primary 0x3f/v11 amplitude source. The source owns finite stages
//! at sampleRate/32; its destination interpolates those points at audio rate.
//! Pitch/module AHDSRs and Flex keep their independently supported paths.

use super::voice::{Ahdsr, Phase};

const BASE: f32 = 0.075;
const START: f32 = 1.075;

#[derive(Clone, Copy, Debug)]
struct Source {
    phase: Phase,
    counts: [u32; 4],
    coefficients: [f32; 3],
    attack_start: f32,
    attack_positive: bool,
    sustain: f32,
    ahd: bool,
    left: u32,
    state: f32,
    mul: f32,
    offset: f32,
    coefficient: f32,
}

impl Source {
    fn new(p: &Ahdsr, rate: f32) -> Self {
        let rate = rate * (1. / 32.);
        let counts = [p.attack, p.hold, p.decay, p.release].map(|t| (t * rate) as u32);
        // Native physical setter rounds the base/ratio to f32 before pow.
        let base = (((1. - p.curve.abs()) as f64 * 500_000f64.ln()) - 20_000f64.ln()).exp() as f32;
        let positive = p.curve > 0.;
        let start = if positive { 1. + base } else { base };
        let ratio = if positive {
            base / start
        } else {
            (base + 1.) / base
        };
        let pow = |ratio: f64, n| {
            if n == 0 {
                1.
            } else {
                ratio.powf(1. / n as f64) as f32
            }
        };
        let coefficients = [
            pow(ratio as f64, counts[0]),
            (43f64 / 3.).powf(-1. / counts[2].max(1) as f64) as f32,
            (43f64 / 3.).powf(-1. / counts[3].max(1) as f64) as f32,
        ];
        let mut source = Self {
            phase: Phase::Attack,
            counts,
            coefficients,
            attack_start: start,
            attack_positive: positive,
            sustain: (p.sustain + BASE) - BASE,
            ahd: p.ahd_only,
            left: 0,
            state: BASE,
            mul: 0.,
            offset: 0.,
            coefficient: 1.,
        };
        source.enter(Phase::Attack);
        source
    }

    fn value(&self) -> f32 {
        (self.state - BASE) * self.mul + self.offset
    }

    fn enter(&mut self, mut phase: Phase) {
        // Zero-duration stages have no published point. Sustain/Done are
        // indefinite; every other stage is bounded by its prepared count.
        loop {
            let (count, next) = match phase {
                Phase::Attack => (self.counts[0], Phase::Hold),
                Phase::Hold => (self.counts[1], Phase::Decay),
                Phase::Decay => (
                    self.counts[2],
                    if self.ahd {
                        Phase::Done
                    } else {
                        Phase::Sustain
                    },
                ),
                Phase::Release => (self.counts[3], Phase::Done),
                _ => break,
            };
            if count != 0 {
                break;
            }
            phase = next;
        }
        let old = self.value();
        self.phase = phase;
        (
            self.state,
            self.mul,
            self.offset,
            self.coefficient,
            self.left,
        ) = match phase {
            Phase::Attack => (
                self.attack_start,
                if self.attack_positive { -1. } else { 1. },
                if self.attack_positive {
                    self.attack_start - BASE
                } else {
                    BASE - self.attack_start
                },
                self.coefficients[0],
                self.counts[0],
            ),
            Phase::Hold => (START, 1., 0., 1., self.counts[1]),
            Phase::Decay => (
                START,
                if self.ahd { 1. } else { 1. - self.sustain },
                if self.ahd { 0. } else { self.sustain },
                self.coefficients[1],
                self.counts[2],
            ),
            Phase::Sustain => (self.sustain + BASE, 1., 0., 1., 0),
            Phase::Release => (START, old, 0., self.coefficients[2], self.counts[3]),
            _ => (BASE, 0., 0., 1., 0),
        };
    }

    fn release(&mut self) {
        if !self.ahd && !matches!(self.phase, Phase::Release | Phase::Done) {
            self.enter(Phase::Release);
        }
    }

    fn point(&mut self) -> f32 {
        let value = self.value();
        self.state *= self.coefficient;
        if self.left != 0 {
            self.left -= 1;
            if self.left == 0 {
                self.enter(match self.phase {
                    Phase::Attack => Phase::Hold,
                    Phase::Hold => Phase::Decay,
                    Phase::Decay => {
                        if self.ahd {
                            Phase::Done
                        } else {
                            Phase::Sustain
                        }
                    }
                    Phase::Release => Phase::Done,
                    _ => self.phase,
                });
            }
        }
        value
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Native {
    source: Source,
    previous: f32,
    current: f32,
    level: f32,
    offset: u8,
    primed: bool,
}

impl Native {
    pub fn new(p: &Ahdsr, rate: f32) -> Self {
        Self {
            source: Source::new(p, rate),
            previous: 0.,
            current: 0.,
            level: 0.,
            offset: 0,
            primed: false,
        }
    }
    pub fn release(&mut self) {
        self.source.release();
    }
    pub fn done(&self) -> bool {
        self.source.phase == Phase::Done && self.primed && self.previous == 0. && self.current == 0.
    }
    pub fn phase(&self) -> Phase {
        self.source.phase
    }
    pub fn level(&self) -> f32 {
        self.level
    }
    fn frame(&mut self) -> f32 {
        if self.offset == 0 {
            self.previous = self.current;
            // Volume destination clamps only the published point, never the
            // source recurrence or the arbitrary-stage release baseline.
            self.current = self.source.point().max(0.);
            if !self.primed {
                self.previous = self.current;
                self.primed = true;
            }
        }
        self.level =
            self.previous + (self.current - self.previous) * (self.offset as f32 * (1. / 32.));
        self.offset = (self.offset + 1) & 31;
        self.level
    }
    pub fn skip(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }
    pub fn render(&mut self, out: &mut [f32]) {
        for x in out {
            *x = self.frame();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(curve: f32) -> Ahdsr {
        Ahdsr {
            curve,
            attack: 0.12501293,
            hold: 0.,
            decay: 0.,
            sustain: 1.,
            release: 0.2500013,
            ahd_only: false,
        }
    }

    #[test]
    fn native_primary_ahdsr_curves_finite_counts_and_arbitrary_release() {
        // Independent scalar transcription of the native setter/render SSE
        // sequence: f32 base, ratio, pow result and every state multiply. These
        // checkpoints include cancellation in the two target additions at
        // curve zero; an ideal unrounded exp curve is not that recurrence.
        for (curve, start, coefficient_bits, checkpoints) in [
            (
                -1.,
                0x3851b717,
                0x3f86f62c,
                [0, 0x36368000, 0x3cc8ae68, 0x3f72cb62],
            ),
            (
                -0.5,
                0x3d10d0c3,
                0x3f825523,
                [0, 0x3a28e540, 0x3e83a4af, 0x3f7b41e2],
            ),
            (
                0.,
                0x41c80000,
                0x3f8006e0,
                [0, 0x3babe000, 0x3f1f0840, 0x3f7eaa80],
            ),
            (
                0.5,
                0x3f848686,
                0x3f7b6b1a,
                [0, 0x3c97cbc0, 0x3f69029f, 0x3f7fd5c7],
            ),
            (
                1.,
                0x3f8001a3,
                0x3f72cb83,
                [0, 0x3d534a80, 0x3f7f7dc7, 0x3f7fffd2],
            ),
        ] {
            let p = params(curve);
            let mut source = Source::new(&p, 48_000.);
            assert_eq!(source.counts, [187, 0, 0, 375]);
            assert_eq!(source.attack_start.to_bits(), start);
            assert_eq!(source.coefficients[0].to_bits(), coefficient_bits);
            let initial = f64::from(f32::from_bits(start));
            let coefficient = f64::from(f32::from_bits(coefficient_bits));
            let sign = if curve > 0. { -1. } else { 1. };
            let offset = f64::from(if curve > 0. {
                f32::from_bits(start) - BASE
            } else {
                BASE - f32::from_bits(start)
            });
            for n in 0..187 {
                // Closed form using the independently known rounded native
                // coefficient. Gamma_n bounds n f32 recurrence roundings;
                // two target operations add their own unit-roundoff bound.
                let state = initial * coefficient.powi(n);
                let reference = (state - f64::from(BASE)) * sign + offset;
                let u = f64::from(f32::EPSILON) * 0.5;
                let gamma = (n as f64 * u) / (1. - n as f64 * u);
                let magnitude = (1. + gamma) * state.abs() + f64::from(BASE);
                let bound = gamma * state.abs() + (2. * u + u * u) * magnitude + u * offset.abs();
                let actual = source.point();
                assert!(
                    (f64::from(actual) - reference).abs() <= bound,
                    "curve {curve} point {n}: {actual} vs {reference}, rounding bound {bound}"
                );
                if let Some(i) = [0, 1, 117, 186].iter().position(|&point| point == n) {
                    assert_eq!(actual.to_bits(), checkpoints[i], "curve {curve} point {n}");
                }
                assert!((0. ..=1.).contains(&actual));
            }
            assert_eq!(source.phase, Phase::Sustain);
            assert_eq!(source.point(), 1.);
        }
        let staged = Ahdsr {
            hold: 0.01,
            decay: 0.1,
            sustain: 0.3,
            ..params(1.)
        };
        let mut decay = Source::new(&staged, 48_000.);
        for _ in 0..202 {
            decay.point();
        }
        assert_eq!(decay.phase, Phase::Decay);
        for n in 0..150 {
            let reference = 0.3 + 0.7 * (1.075 * (43f64 / 3.).powf(-(n as f64) / 150.) - 0.075);
            assert!((decay.point() as f64 - reference).abs() < 0.00002);
        }
        assert_eq!(decay.phase, Phase::Sustain);
        assert!((decay.point() - 0.3).abs() < 0.000001);
        for elapsed in [0, 17, 187, 194, 202, 211, 400] {
            let mut source = Source::new(&staged, 48_000.);
            for _ in 0..elapsed {
                source.point();
            }
            let level = source.value();
            source.release();
            for n in 0..375 {
                let expected =
                    level as f64 * (1.075 * (43f64 / 3.).powf(-(n as f64) / 375.) - 0.075);
                assert!((source.point() as f64 - expected).abs() < 0.00002);
            }
            assert_eq!(source.phase, Phase::Done);
            assert_eq!(source.point(), 0.);
        }
        let mut ahd = params(0.);
        ahd.attack = 0.;
        ahd.hold = 64. / 48_000.;
        ahd.decay = 128. / 48_000.;
        ahd.ahd_only = true;
        let mut source = Source::new(&ahd, 48_000.);
        source.release();
        assert_eq!(source.phase, Phase::Hold);
        for _ in 0..6 {
            source.point();
        }
        assert_eq!(source.phase, Phase::Done);
        let zero = Ahdsr {
            attack: 0.,
            hold: 0.,
            decay: 0.,
            release: 0.,
            ..params(0.)
        };
        let mut source = Source::new(&zero, 48_000.);
        assert_eq!(source.point(), 1.);
        source.release();
        assert_eq!(source.point(), 0.);
    }

    #[test]
    fn native_primary_ahdsr_interpolation_partition_and_finite_tail_without_heap() {
        let p = Ahdsr {
            attack: 0.,
            sustain: 0.3,
            ..params(0.)
        };
        let mut whole = Native::new(&p, 48_000.);
        whole.skip(13);
        whole.release();
        let mut split = whole;
        let mut expected = vec![0.; 12_083];
        let mut actual = vec![0.; expected.len()];
        whole.render(&mut expected);
        let allocations = crate::test_support::allocations(|| {
            let mut from = 0;
            while from < actual.len() {
                let n = [17, 111][(from / 128) & 1].min(actual.len() - from);
                split.render(&mut actual[from..from + n]);
                from += n;
            }
        });
        assert_eq!(allocations, 0);
        assert_eq!(actual, expected);
        assert!(whole.done() && split.done());
        assert_eq!(*actual.last().unwrap(), 0.);
        // 375 finite source points followed by one interpolated endpoint;
        // release gain changes the values, not the stage's duration.
        let mut quiet = Native::new(&Ahdsr { sustain: 0.03, ..p }, 48_000.);
        quiet.skip(13);
        quiet.release();
        let mut q = vec![0.; actual.len()];
        quiet.render(&mut q);
        for (l, r) in actual.iter().zip(q) {
            assert!((*l * 0.1 - r).abs() < 0.000001);
        }
    }
    #[test]
    fn native_primary_ahdsr_resident_pedals_retrigger_eof_and_fragmentation_without_heap() {
        use crate::{
            audio::Sample,
            engine::{Bank, Engine},
            import::{Group, Loop, Zone},
        };
        let create = |looped: bool, ahd: bool| {
            let group = Group {
                native_volume_env: true,
                volume_env: Some(crate::import::Ahdsr {
                    attack_curve: 1.,
                    attack_ms: 125.012924,
                    hold_ms: 10.,
                    decay_ms: 100.,
                    sustain: 0.3,
                    release_ms: 250.0013,
                    unknown_flag: u8::from(ahd),
                    unknown_tail: Vec::new(),
                }),
                ..Default::default()
            };
            let zone = Zone {
                root: 60,
                low_key: 60,
                high_key: 60,
                loop_range: looped.then_some(Loop {
                    start: 0,
                    end: 256,
                    crossfade: 0,
                    alternating: false,
                    until_release: false,
                }),
                ..Default::default()
            };
            let sample = Sample {
                rate: 48_000,
                frames: vec![[0.25; 2]; 4096],
            };
            let bank =
                Bank::from_samples(vec![group], vec![zone], vec![(Default::default(), sample)])
                    .unwrap();
            let mut e = Engine::default();
            e.set_bank(Some(Box::new(bank)));
            e
        };
        let (mut whole, mut split, mut eof, mut ahd) = (
            create(true, false),
            create(true, false),
            create(false, false),
            create(true, true),
        );
        let p = Ahdsr {
            curve: 1.,
            attack: 125.012924 * 0.001,
            hold: 0.01,
            decay: 0.1,
            sustain: 0.3,
            release: 250.0013 * 0.001,
            ahd_only: false,
        };
        let mut reference = Native::new(&p, 48_000.);
        let (mut a, mut ar, mut b, mut br, mut envelope) =
            ([0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128]);
        let mut initial = [0.; 128];
        let allocations = crate::test_support::allocations(|| {
            for e in [&mut whole, &mut split] {
                e.note_on(7, 60, 127);
            }
            for block in 0..140 {
                if block == 13 {
                    for e in [&mut whole, &mut split] {
                        e.cc(7, 64, 127);
                        e.note_off(7, 60);
                    }
                }
                if block == 31 {
                    for e in [&mut whole, &mut split] {
                        e.cc(7, 64, 0);
                    }
                    reference.release();
                }
                reference.render(&mut envelope);
                whole.render(&mut a, &mut ar);
                for (from, to) in [(0, 17), (17, 128)] {
                    split.render(&mut b[from..to], &mut br[from..to]);
                }
                assert_eq!(a, b);
                assert_eq!(ar, br);
                if block == 0 {
                    initial = a;
                }
                if block < 31 {
                    let v = &whole.player.voices[0];
                    assert_eq!(v.env.level(), reference.level());
                    assert!(!v.released);
                }
                if block == 31 {
                    assert!(whole.player.voices[0].released);
                }
                // Each constant sample/channel has one fixed voice gain;
                // independently generated amplitude must be the audible ratio.
                if let Some(v) = whole.player.voices.first() {
                    assert!(a
                        .iter()
                        .zip(envelope)
                        .all(|(a, env)| (*a - env * 0.25 * v.gains[0]).abs() < 1e-7));
                }
            }
            assert!(whole.player.voices.is_empty() && split.player.voices.is_empty());
            assert!(!whole.key_down(7, 60));
            // A new note owns a new source counter/interpolator.
            for e in [&mut whole, &mut split] {
                e.note_on(7, 60, 127);
            }
            whole.render(&mut a, &mut ar);
            split.render(&mut b, &mut br);
            assert_eq!(a, initial);
            assert_eq!(a, b);
            // Sostenuto pins only the captured attack, without resetting its clock.
            for e in [&mut whole, &mut split] {
                e.cc(7, 66, 127);
                e.note_off(7, 60);
            }
            whole.render(&mut a, &mut ar);
            split.render(&mut b, &mut br);
            assert!(!whole.player.voices[0].released);
            assert_eq!(a, b);
            for e in [&mut whole, &mut split] {
                e.cc(7, 66, 0);
            }
            for _ in 0..100 {
                whole.render(&mut a, &mut ar);
                split.render(&mut b, &mut br);
            }
            assert!(whole.player.voices.is_empty() && split.player.voices.is_empty());
            // A finite sample may end while its physical key stays held.
            eof.note_on(7, 60, 127);
            for _ in 0..40 {
                eof.render(&mut a, &mut ar);
            }
            assert!(eof.player.voices.is_empty());
            assert!(eof.key_down(7, 60));
            eof.note_off(7, 60);
            eof.render(&mut a, &mut ar);
            assert!(!eof.key_down(7, 60));
            ahd.note_on(7, 60, 127);
            ahd.render(&mut a[..13], &mut ar[..13]);
            ahd.note_off(7, 60);
            ahd.render(&mut a, &mut ar);
            assert_eq!(
                ahd.player.voices[0].env.phase(),
                Phase::Attack,
                "AHD ignores early key release"
            );
            for _ in 0..100 {
                ahd.render(&mut a, &mut ar);
            }
            assert!(
                ahd.player.voices.is_empty(),
                "AHD ends at the finite decay, without sustaining"
            );
        });
        assert_eq!(allocations, 0);
        assert_eq!(whole.dropped_commands(), 0);
        assert_eq!(split.dropped_commands(), 0);
    }
}
