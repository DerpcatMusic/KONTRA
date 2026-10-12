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
    complete: bool,
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
            complete: false,
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

    fn next_phase(&self, phase: Phase) -> Phase {
        match phase {
            Phase::Attack => Phase::Hold,
            Phase::Hold => Phase::Decay,
            Phase::Decay => {
                if self.ahd {
                    Phase::Done
                } else if self.complete {
                    Phase::Release
                } else {
                    Phase::Sustain
                }
            }
            Phase::Release => Phase::Done,
            _ => phase,
        }
    }

    fn enter(&mut self, mut phase: Phase) {
        // Zero-duration stages have no published point. Sustain/Done are
        // indefinite; every other stage is bounded by its prepared count.
        loop {
            let count = match phase {
                Phase::Attack => self.counts[0],
                Phase::Hold => self.counts[1],
                Phase::Decay => self.counts[2],
                Phase::Release => self.counts[3],
                _ => break,
            };
            if count != 0 {
                break;
            }
            phase = self.next_phase(phase);
        }
        self.setup(phase);
    }

    // Queued native release uses stage setup without skipping its zero count.
    fn setup(&mut self, phase: Phase) {
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

    fn advance_point(&mut self) -> (f32, bool) {
        let value = self.value();
        self.state *= self.coefficient;
        let expired = if self.left != 0 {
            self.left -= 1;
            self.left == 0
        } else { false };
        (value, expired)
    }

    fn point(&mut self) -> f32 {
        let (value, expired) = self.advance_point();
        if expired {
            self.enter(self.next_phase(self.phase));
        }
        value
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Interpolation {
    previous: f32,
    current: f32,
    level: f32,
    offset: u8,
    primed: bool,
}

impl Interpolation {
    fn frame(&mut self, mut point: impl FnMut() -> f32) -> f32 {
        if self.offset == 0 {
            self.previous = self.current;
            // Volume destination clamps only the published point, never the
            // source recurrence or the arbitrary-stage release baseline.
            self.current = point().max(0.);
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
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Native {
    source: Source,
    interpolation: Interpolation,
}

impl Native {
    pub(super) fn supports_completion(p: &Ahdsr, rate: f32) -> bool {
        !p.ahd_only && matches!(Source::new(p, rate).phase, Phase::Attack | Phase::Hold | Phase::Decay)
    }
    pub(super) fn force_completion(&mut self) {
        // Native reset precedes event+B3 installation; never retroactively
        // leave an already selected initial Sustain stage.
        self.source.complete = true;
    }
    pub fn new(p: &Ahdsr, rate: f32) -> Self {
        Self {
            source: Source::new(p, rate),
            interpolation: Interpolation::default(),
        }
    }
    pub fn release(&mut self) {
        self.source.release();
    }
    pub fn done(&self) -> bool {
        self.source.phase == Phase::Done && self.interpolation.primed
            && self.interpolation.previous == 0. && self.interpolation.current == 0.
    }
    pub fn phase(&self) -> Phase {
        self.source.phase
    }
    pub fn level(&self) -> f32 {
        self.interpolation.level
    }
    fn frame(&mut self) -> f32 {
        let Self { source, interpolation } = self;
        interpolation.frame(|| source.point())
    }
    pub fn skip(&mut self, n: usize) {
        let _ = self.skip_terminal(n);
    }
    pub fn render(&mut self, out: &mut [f32]) {
        let _ = self.render_terminal(out);
    }

    pub(super) fn skip_terminal(&mut self, n: usize) -> Option<usize> {
        self.consume(None, n)
    }

    pub(super) fn render_terminal(&mut self, out: &mut [f32]) -> Option<usize> {
        let n = out.len();
        self.consume(Some(out), n)
    }

    fn consume(&mut self, mut out: Option<&mut [f32]>, n: usize) -> Option<usize> {
        let mut terminal = self.done().then_some(0);
        for at in 0..n {
            let value = self.frame();
            if let Some(out) = out.as_deref_mut() { out[at] = value; }
            // Both interpolated endpoints become zero before this frame is
            // published: its index is the first permanently silent boundary.
            if terminal.is_none() && self.done() { terminal = Some(at); }
        }
        terminal
    }
}

// The native +98 copy excludes prepared geometry, interpolation, and the
// clone-local completion byte. Keep that boundary explicit in the cache.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ending {
    phase: Phase,
    state: f32,
    left: u32,
    mul: f32,
    offset: f32,
    countdown: i32,
    note_off: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Candidate {
    pub points: [f32; 32],
    pub count: usize,
    pub ending: Ending,
    source: Source,
    own_done: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Shared {
    native: Native,
    countdown: i32,
    note_off: bool,
    own_done: bool,
    generation: Option<u64>,
    point_cursor: usize,
    frames_remaining: usize,
}

impl Shared {
    pub(super) fn new(p: &Ahdsr, rate: f32) -> Self {
        let mut native = Native::new(p, rate);
        Self::indefinite_count(&mut native.source);
        Self { native, countdown: i32::MAX, note_off: false, own_done: false,
            generation: None, point_cursor: 0, frames_remaining: 0 }
    }

    fn indefinite_count(source: &mut Source) {
        // Native prepared Sustain/Done rows D0/D8 contain INT_MAX. Ordinary
        // Source keeps its existing zero-as-indefinite representation.
        if matches!(source.phase, Phase::Sustain | Phase::Done) {
            source.left = i32::MAX as u32;
        }
    }

    fn resolve(source: &mut Source, countdown: &mut i32) {
        // ae42e1 tests countdown before ae4323 tests finite-stage exhaustion.
        if *countdown == 0 {
            *countdown = i32::MAX;
            if !matches!(source.phase, Phase::Release | Phase::Done) {
                source.setup(Phase::Release);
            }
        } else if source.left == 0 {
            let next = if source.phase == Phase::Sustain { Phase::Release }
                else { source.next_phase(source.phase) };
            source.enter(next);
            Self::indefinite_count(source);
        }
    }

    pub(super) fn stage(&self, count: usize, off: Option<i32>) -> Result<Candidate, &'static str> {
        if count > 32 { return Err("shared AHDSR range exceeds 32 source points"); }
        if off.is_some_and(|off| off < 0) {
            return Err("shared AHDSR release countdown is negative");
        }
        let mut source = self.native.source;
        if source.ahd || source.complete || source.counts.iter().any(|&n| n > i32::MAX as u32) {
            return Err("shared AHDSR source geometry is outside the sealed capability");
        }
        let mut countdown = off.unwrap_or(self.countdown);
        let note_off = self.note_off || off.is_some();
        let mut points = [0.; 32];
        for point in &mut points[..count] {
            // A queued zero-length Release at the previous window's final
            // point is advanced only when this waveform requests another.
            while source.left == 0 || countdown == 0 {
                Self::resolve(&mut source, &mut countdown);
            }
            let (value, expired) = source.advance_point();
            *point = value;
            countdown -= 1;
            if countdown == 0 || expired {
                Self::resolve(&mut source, &mut countdown);
            }
        }
        let ending = Ending { phase: source.phase, state: source.state, left: source.left,
            mul: source.mul, offset: source.offset, countdown, note_off };
        Ok(Candidate { points, count, ending, source,
            own_done: self.own_done || source.phase == Phase::Done })
    }

    fn points_needed(&self, n: usize) -> usize {
        (n + 31 - ((32 - usize::from(self.native.interpolation.offset)) & 31)) / 32
    }

    pub(super) fn validate_binding(&self, candidate: &Candidate, generation: u64,
        frames_remaining: usize, suffix: usize) -> Result<(), &'static str> {
        if self.generation == Some(generation) { return Ok(()); }
        if frames_remaining > 1024 || suffix > candidate.count
            || self.points_needed(frames_remaining) > candidate.count - suffix {
            return Err("shared AHDSR cache cannot cover the clone's sealed range");
        }
        Ok(())
    }

    fn bind(&mut self, generation: u64, frames_remaining: usize, suffix: usize) {
        self.generation = Some(generation);
        self.frames_remaining = frames_remaining;
        self.point_cursor = suffix;
    }

    pub(super) fn commit_candidate(&mut self, candidate: &Candidate, generation: u64,
        frames_remaining: usize, suffix: usize) -> Result<(), &'static str> {
        self.validate_binding(candidate, generation, frames_remaining, suffix)?;
        if self.generation != Some(generation) {
            self.native.source = candidate.source;
            self.countdown = candidate.ending.countdown;
            self.note_off = candidate.ending.note_off;
            self.own_done = candidate.own_done;
            self.bind(generation, frames_remaining, suffix);
        }
        Ok(())
    }

    pub(super) fn copy_shared_end(&mut self, candidate: &Candidate, generation: u64,
        frames_remaining: usize, suffix: usize) -> Result<(), &'static str> {
        self.validate_binding(candidate, generation, frames_remaining, suffix)?;
        if self.generation != Some(generation) {
            let end = candidate.ending;
            let source = &mut self.native.source;
            source.phase = end.phase;
            source.state = end.state;
            source.left = end.left;
            source.mul = end.mul;
            source.offset = end.offset;
            source.coefficient = match end.phase {
                Phase::Attack => source.coefficients[0],
                Phase::Decay => source.coefficients[1],
                Phase::Release => source.coefficients[2],
                _ => 1.,
            };
            self.countdown = end.countdown;
            self.note_off = end.note_off;
            self.bind(generation, frames_remaining, suffix);
        }
        Ok(())
    }

    pub(super) fn phase(&self) -> Phase { self.native.phase() }
    pub(super) fn level(&self) -> f32 { self.native.level() }
    pub(super) fn done(&self) -> bool { self.own_done && self.frames_remaining == 0 }
    pub(super) fn release_is_sealed(&self) -> bool {
        self.generation.is_some() && self.frames_remaining != 0
    }

    pub(super) fn can_consume(&self, points_len: usize, n: usize) -> bool {
        self.generation.is_some() && n <= self.frames_remaining
            && self.point_cursor <= points_len
            && self.points_needed(n) <= points_len - self.point_cursor
    }

    pub(super) fn render_cached(&mut self, points: &[f32], out: &mut [f32]) -> Option<usize> {
        let n = out.len();
        self.consume(points, Some(out), n)
    }

    pub(super) fn skip_cached(&mut self, points: &[f32], n: usize) -> Option<usize> {
        self.consume(points, None, n)
    }

    fn consume(&mut self, points: &[f32], mut out: Option<&mut [f32]>, n: usize) -> Option<usize> {
        assert!(self.can_consume(points.len(), n), "unsealed shared AHDSR consumer");
        for at in 0..n {
            let cursor = &mut self.point_cursor;
            let value = self.native.interpolation.frame(|| {
                let value = points[*cursor];
                *cursor += 1;
                value
            });
            if let Some(out) = out.as_deref_mut() { out[at] = value; }
        }
        self.frames_remaining -= n;
        // Native waveform completion retires after the full range has been
        // consumed, even when its last interpolation endpoint is nonzero.
        self.done().then_some(n)
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
    fn native_terminal_matches_authored_point_clock_without_heap() {
        for release in [0., 0.01007, 0.2500013] {
            for phase in [0, 13, 31] {
                let p = Ahdsr { attack: 0., release, ..params(0.) };
                let points = (release * 1500.) as usize;
                let next_tick = if phase == 0 { 0 } else { 32 - phase };
                // Finite source points, then one interpolation to the final
                // zero endpoint. The next tick publishes only zero forever.
                let expected = next_tick + (points + 1) * 32;
                let mut seed = Native::new(&p, 48_000.);
                seed.skip(128 + phase);
                seed.release();
                let (mut whole, mut split, mut muted) = (seed, seed, seed);
                let (mut a, mut b) = (vec![0.; expected + 128], vec![0.; expected + 128]);
                let (mut whole_end, mut split_end, mut muted_end) = (None, None, None);
                assert_eq!(crate::plugin::tests::allocations(|| {
                    whole_end = whole.render_terminal(&mut a);
                    let mut from = 0;
                    let mut chunk = 0;
                    while from < b.len() {
                        let n = [1, 7, 31, 128][chunk % 4].min(b.len() - from);
                        if let Some(offset) = split.render_terminal(&mut b[from..from + n]) {
                            split_end.get_or_insert(from + offset);
                        }
                        if let Some(offset) = muted.skip_terminal(n) {
                            muted_end.get_or_insert(from + offset);
                        }
                        from += n;
                        chunk += 1;
                    }
                }), 0);
                assert_eq!(whole_end, Some(expected), "release={release} phase={phase}");
                assert_eq!(split_end, whole_end);
                assert_eq!(muted_end, whole_end);
                assert_eq!(a, b);
                assert!(a[expected - 1] > 0.);
                assert!(a[expected..].iter().all(|&value| value == 0.));
            }
        }
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
        let allocations = crate::plugin::tests::allocations(|| {
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
        let allocations = crate::plugin::tests::allocations(|| {
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
