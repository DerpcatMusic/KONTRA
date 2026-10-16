//! Test-only HQ wrapper convergence research; not a product admission path.
//! No product HQ admission. No native code or vendor input is included.
use super::Ladder;
use crate::dsp::kaiser::{Kaiser, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResearchError {
    Rate,
    Knob,
    Contract,
    Channels,
    Frames,
    ControlView,
    Input,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Contract {
    FixedHqMovingGain,
    StaticKnobsLiveQuality,
}

#[derive(Clone, Copy)]
struct RateBank {
    rate: f32,
    correction: [f32; 4],
    ticks: u32,
    inverse: f32,
    seed: Ladder,
}

struct PreparedHq {
    knobs: [f32; 3],
    banks: [RateBank; 2],
    factor2: Kaiser,
}

impl PreparedHq {
    /// Worker-only constructor. Experimental supported cells, not a new rate policy.
    fn new(host_rate: f32, knobs: [f32; 3]) -> Result<Self, ResearchError> {
        if ![22_050.0, 44_100.0, 88_200.0].contains(&host_rate) {
            return Err(ResearchError::Rate);
        }
        if !valid_knobs(knobs) {
            return Err(ResearchError::Knob);
        }
        super::prepare();
        let banks = std::array::from_fn(|quality| {
            let factor = (quality + 1) as u32;
            let rate = host_rate * factor as f32;
            let mut seed = Ladder::default();
            seed.set_taps(4);
            seed.record_version(0); // captured ordinary cutoff, never inferred saved-version law
            seed.tune(knobs, rate);
            seed.controls.started = true;
            let ticks = (rate * 0.002 / (factor * 32) as f32 + 0.5) as u32;
            let ticks = ticks.max(1);
            RateBank {
                rate,
                correction: [
                    seed.correction,
                    seed.correction_reciprocal,
                    seed.correction_previous,
                    seed.correction_gain,
                ],
                ticks,
                inverse: 1.0 / (ticks * factor * 32) as f32,
                seed,
            }
        });
        Ok(Self {
            knobs,
            banks,
            factor2: Kaiser::new(2),
        })
    }
}

fn valid_knobs([cutoff, resonance, gain]: [f32; 3]) -> bool {
    cutoff.is_finite()
        && resonance.is_finite()
        && gain.is_finite()
        && (0.0..=1.0).contains(&cutoff)
        && (0.0..=1.0).contains(&resonance)
        && (-1.0..=1.0).contains(&gain)
}

#[derive(Clone, Copy, Debug)]
struct BoundaryControls {
    current: [f32; 3],
    target: [f32; 3],
    increment: [f32; 3],
    remaining: i32,
    pending: bool,
    active: bool,
}

impl BoundaryControls {
    fn initial(target: [f32; 3]) -> Self {
        Self {
            current: target,
            target,
            increment: [0.0; 3],
            remaining: 0,
            pending: true,
            active: false,
        }
    }

    /// Existing ordinary native three-lane law, separate from the host phase.
    /// Inactive signed countdown wrapping is deliberately retained, not clamped.
    fn tick(&mut self, bank: &RateBank) {
        if self.pending {
            self.pending = false;
            for lane in 0..3 {
                let increment = (self.target[lane] - self.current[lane]) * bank.inverse;
                if f64::from(increment * increment) < 1e-15 {
                    self.current[lane] = self.target[lane];
                    self.increment[lane] = 0.0;
                } else {
                    self.increment[lane] = increment;
                }
            }
            let squares = self.increment.map(|x| x * x);
            if (squares[0] + squares[1]) + squares[2] > 0.0 {
                self.remaining = bank.ticks as i32;
                self.active = true;
            }
        } else {
            self.remaining = self.remaining.wrapping_sub(1);
            if self.remaining == 0 {
                self.current = self.target;
                self.increment = [0.0; 3];
                self.active = false;
            }
        }
    }

    fn advance_four(&mut self) {
        if self.active {
            for lane in 0..3 {
                self.current[lane] += 4.0 * self.increment[lane];
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Routes<'a> {
    gain: Option<&'a [f32]>,
    cutoff: Option<&'a [f32]>,
    resonance: Option<&'a [f32]>,
    quality: Option<&'a [f32]>,
}

#[derive(Clone, Copy)]
struct ResearchVoice<'a> {
    prepared: &'a PreparedHq,
    kernel: Ladder,
    resampler: [State; 2],
    clock: BoundaryControls,
    knobs: [f32; 3],
    host_countdown: u8,
    quartet_phase: u8,
    quality: bool,
    contract: Contract,
}

impl<'a> ResearchVoice<'a> {
    fn new(p: &'a PreparedHq, contract: Contract) -> Self {
        let kernel = p.banks[1].seed;
        Self {
            prepared: p,
            clock: BoundaryControls::initial(kernel.controls.target),
            kernel,
            resampler: [State::default(); 2],
            knobs: p.knobs,
            host_countdown: 0,
            quartet_phase: 0,
            quality: true,
            contract,
        }
    }

    fn boundaries(&self, frames: usize) -> usize {
        let initial = usize::from(self.host_countdown);
        if frames == 0 || (initial != 0 && frames <= initial) {
            0
        } else if initial == 0 {
            frames.div_ceil(32)
        } else {
            (frames - initial).div_ceil(32)
        }
    }

    fn validate(
        &self,
        left: &[f32],
        right: &[f32],
        routes: Routes<'_>,
    ) -> Result<(), ResearchError> {
        if left.len() != right.len() {
            return Err(ResearchError::Channels);
        }
        if left.len() > 1024 {
            return Err(ResearchError::Frames);
        }
        if left.iter().chain(right).any(|v| !v.is_finite()) {
            return Err(ResearchError::Input);
        }
        if left.is_empty() {
            return Ok(());
        }
        // No unproved combined moving-knob/quality behavior is silently implemented.
        let forbidden = match self.contract {
            Contract::FixedHqMovingGain => {
                routes.quality.is_some() || routes.cutoff.is_some() || routes.resonance.is_some()
            }
            Contract::StaticKnobsLiveQuality => {
                routes.gain.is_some() || routes.cutoff.is_some() || routes.resonance.is_some()
            }
        };
        if forbidden {
            return Err(ResearchError::Contract);
        }
        let needed = self.boundaries(left.len());
        for values in [routes.gain, routes.cutoff, routes.resonance, routes.quality]
            .into_iter()
            .flatten()
        {
            if values.len() < needed {
                return Err(ResearchError::ControlView);
            }
            if values[..needed]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(ResearchError::Knob);
            }
        }
        Ok(())
    }

    /// Source-derived bounded state transition, never construction/tap design.
    fn change_quality(&mut self, quality: bool) {
        let p = self.prepared;
        if quality == self.quality {
            return;
        }
        if quality {
            self.resampler = [State::default(); 2];
        }
        // Turning off deliberately retains the unused resampler states.
        self.quality = quality;
        let bank = &p.banks[usize::from(quality)];
        self.kernel.correction = bank.correction[0];
        self.kernel.correction_reciprocal = bank.correction[1];
        self.kernel.correction_previous = bank.correction[2];
        self.kernel.correction_gain = bank.correction[3];
        self.clock.current = self.clock.target;
        self.clock.pending = true;
        // Increment/active/countdown/quartet are NOT guessed reset laws.
        self.kernel.coefficients(self.clock.current, bank.rate);
    }

    /// Callback-shaped: borrowed prepared data/slices plus fixed mutable state only.
    /// No constructors, allocation, locks, I/O or transcendental tap setup here.
    fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        routes: Routes<'_>,
    ) -> Result<(), ResearchError> {
        self.validate(left, right, routes)?;
        let p = self.prepared;
        let mut boundary = 0;
        for i in 0..left.len() {
            if self.host_countdown == 0 {
                if let Some(values) = routes.gain {
                    self.knobs[2] = values[boundary];
                    // Reuse represented target conversion. This temporary bounded Copy
                    // is a convergence seam, not a proposed shipping optimization.
                    let bank = &p.banks[usize::from(self.quality)];
                    let mut target = bank.seed;
                    target.tune(self.knobs, bank.rate);
                    self.clock.target = target.controls.target;
                    self.clock.pending = true;
                }
                let desired = routes.quality.map_or(self.quality, |v| v[boundary] > 0.5);
                self.clock.tick(&p.banks[usize::from(self.quality)]);
                self.change_quality(desired);
                self.host_countdown = 32;
                boundary += 1;
            }
            let factor = usize::from(self.quality) + 1;
            let mut l = [0.0; 2];
            let mut r = [0.0; 2];
            if self.quality {
                p.factor2.up(&mut self.resampler[0], left[i], &mut l);
                p.factor2.up(&mut self.resampler[1], right[i], &mut r);
            } else {
                l[0] = left[i];
                r[0] = right[i];
            }
            for internal in 0..factor {
                if self.quartet_phase == 0 {
                    self.clock.advance_four();
                    self.kernel
                        .coefficients(self.clock.current, p.banks[usize::from(self.quality)].rate);
                }
                self.kernel.process_held(
                    &mut l[internal..internal + 1],
                    &mut r[internal..internal + 1],
                );
                self.quartet_phase = (self.quartet_phase + 1) & 3;
            }
            if self.quality {
                left[i] = p.factor2.down(&mut self.resampler[0], &l);
                right[i] = p.factor2.down(&mut self.resampler[1], &r);
            } else {
                left[i] = l[0];
                right[i] = r[0];
            }
            self.host_countdown -= 1;
        }
        Ok(())
    }

    fn audit(&self) -> Audit {
        let p = self.prepared;
        let b = &p.banks[usize::from(self.quality)];
        Audit {
            poles: self.kernel.state.map(|c| c.map(f32::to_bits)),
            correction: [
                self.kernel.correction,
                self.kernel.correction_reciprocal,
                self.kernel.correction_previous,
                self.kernel.correction_gain,
            ]
            .map(f32::to_bits),
            current: self.clock.current.map(f32::to_bits),
            target: self.clock.target.map(f32::to_bits),
            increment: self.clock.increment.map(f32::to_bits),
            remaining: self.clock.remaining,
            pending: self.clock.pending,
            active: self.clock.active,
            host_countdown: self.host_countdown,
            quartet_phase: self.quartet_phase,
            quality: self.quality,
            ticks: b.ticks,
            inverse: b.inverse.to_bits(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Audit {
    poles: [[u32; 5]; 2],
    correction: [u32; 4],
    current: [u32; 3],
    target: [u32; 3],
    increment: [u32; 3],
    remaining: i32,
    pending: bool,
    active: bool,
    host_countdown: u8,
    quartet_phase: u8,
    quality: bool,
    ticks: u32,
    inverse: u32,
}

fn pcm_hash(left: &[f32], right: &[f32]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for v in left.iter().chain(right) {
        for b in v.to_bits().to_le_bytes() {
            h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn authored_input(kind: u8) -> ([f32; 160], [f32; 160], usize) {
    let mut l = [0.0; 160];
    let mut r = [0.0; 160];
    l[0] = 0.125;
    r[0] = -0.0625;
    let frames = if kind == 0 { 64 } else { 160 };
    for i in 0..frames - 32 {
        // Match the frozen author's f64 division then f32 cast, not f32 division.
        l[32 + i] = (((i * 7919 % 127) as i32 - 63) as f64 / 1700.0) as f32;
        r[32 + i] = (((i * 3571 % 131) as i32 - 65) as f64 / 3100.0) as f32;
    }
    (l, r, frames)
}

// Authored inputs and original-code result hashes; immutable native reference.
// Source image SHA256 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8
// (kind, host_rate, frames, full-planar-PCM FNV64, current/target/increment/tick_inverse bits, ticks)
const WRAPPER_CASES: [(u8, f32, usize, u64, [u32; 4], u32); 6] = [
    (
        0,
        22050.0,
        64,
        0xa65c0393e424e084,
        [0x3fff6570, 0x3fff6572, 0x3c7ecae4, 0x3c800000],
        1,
    ),
    (
        0,
        44100.0,
        64,
        0xb8a84989504ac552,
        [0x3faa7720, 0x3fff6572, 0x3ba9dc98, 0x3baaaaab],
        3,
    ),
    (
        0,
        88200.0,
        64,
        0x0b42c16c298f2f22,
        [0x3f953b90, 0x3fff6572, 0x3b29dc98, 0x3b2aaaab],
        6,
    ),
    (
        1,
        22050.0,
        160,
        0x44e097821e4c2c66,
        [0x3f800000, 0x3f800000, 0x00000000, 0x3c800000],
        1,
    ),
    (
        1,
        44100.0,
        160,
        0x0b893c8ae443d586,
        [0x3f800000, 0x3f800000, 0x00000000, 0x3baaaaab],
        3,
    ),
    (
        1,
        88200.0,
        160,
        0xb47bc8115331c61b,
        [0x3f800000, 0x3f800000, 0x00000000, 0x3b2aaaab],
        6,
    ),
];

#[test]
fn original_wrapper_fixed_hq_gain_and_quality_partition_convergence() {
    for (kind, rate, frames, hash, fields, ticks) in WRAPPER_CASES {
        let knobs = [0.5, 0.7, 0.0];
        let p = PreparedHq::new(rate, knobs).unwrap();
        let contract = if kind == 0 {
            Contract::FixedHqMovingGain
        } else {
            Contract::StaticKnobsLiveQuality
        };
        let mut initial = ResearchVoice::new(&p, contract);
        initial
            .process(&mut [0.0; 32], &mut [0.0; 32], Routes::default())
            .unwrap();
        let mut whole_voice = initial;
        let (mut l, mut r, n) = authored_input(kind);
        assert_eq!(frames, n);
        let whole_routes = if kind == 0 {
            Routes {
                gain: Some(&[0.0, 0.5]),
                ..Routes::default()
            }
        } else {
            Routes {
                quality: Some(&[1., 1., 1., 0., 1.]),
                ..Routes::default()
            }
        };
        whole_voice
            .process(&mut l[..n], &mut r[..n], whole_routes)
            .unwrap();
        assert_eq!(
            pcm_hash(&l[..n], &r[..n]),
            hash,
            "original whole PCM kind={kind} rate={rate}"
        );
        let final_state = whole_voice.audit();
        assert_eq!(
            [
                final_state.current[0],
                final_state.target[0],
                final_state.increment[0],
                final_state.inverse
            ],
            fields,
            "independent original control fields kind={kind} rate={rate}"
        );
        assert_eq!(final_state.ticks, ticks);
        let mut split_voice = initial;
        let (mut sl, mut sr, _) = authored_input(kind);
        split_voice
            .process(
                &mut sl[..17],
                &mut sr[..17],
                if kind == 0 {
                    Routes {
                        gain: Some(&[0.]),
                        ..Routes::default()
                    }
                } else {
                    Routes {
                        quality: Some(&[1.]),
                        ..Routes::default()
                    }
                },
            )
            .unwrap();
        assert_eq!(split_voice.host_countdown, 15);
        assert_eq!(split_voice.quartet_phase, 2);
        split_voice
            .process(
                &mut sl[17..64],
                &mut sr[17..64],
                if kind == 0 {
                    Routes {
                        gain: Some(&[0.5]),
                        ..Routes::default()
                    }
                } else {
                    Routes {
                        quality: Some(&[1.]),
                        ..Routes::default()
                    }
                },
            )
            .unwrap();
        if kind == 1 {
            split_voice
                .process(
                    &mut sl[64..160],
                    &mut sr[64..160],
                    Routes {
                        quality: Some(&[1., 0., 1.]),
                        ..Routes::default()
                    },
                )
                .unwrap();
        }
        assert_eq!(
            pcm_hash(&sl[..n], &sr[..n]),
            hash,
            "original split PCM kind={kind} rate={rate}"
        );
        assert_eq!(
            l[..n].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            sl[..n].iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(
            r[..n].iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
            sr[..n].iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(final_state, split_voice.audit());
        // Debug preserves finite f32 roundtrips and signed zero; test observer only.
        // State's sibling-private fields are not accessed via unsafe/transmute.
        assert_eq!(
            format!("{:?}", whole_voice.resampler),
            format!("{:?}", split_voice.resampler)
        );
    }
}

#[test]
fn research_callback_errors_and_zero_length_are_atomic() {
    let knobs = [0.5, 0.7, 0.0];
    let p = PreparedHq::new(44_100., knobs).unwrap();
    let mut v = ResearchVoice::new(&p, Contract::StaticKnobsLiveQuality);
    let before = v.audit();
    v.process(&mut [], &mut [], Routes::default()).unwrap();
    assert_eq!(v.audit(), before);
    let mut l = [0.125; 17];
    let mut r = [-0.0625; 17];
    assert_eq!(
        v.process(
            &mut l,
            &mut r,
            Routes {
                quality: Some(&[]),
                ..Routes::default()
            }
        ),
        Err(ResearchError::ControlView)
    );
    assert_eq!(
        v.process(
            &mut l,
            &mut r,
            Routes {
                quality: Some(&[f32::NAN]),
                ..Routes::default()
            }
        ),
        Err(ResearchError::Knob)
    );
    assert_eq!(
        v.process(
            &mut l,
            &mut r,
            Routes {
                gain: Some(&[0.5]),
                quality: Some(&[0.]),
                ..Routes::default()
            }
        ),
        Err(ResearchError::Contract)
    );
    assert_eq!(v.audit(), before);
    assert_eq!(l, [0.125; 17]);
    assert_eq!(r, [-0.0625; 17]);
    assert!(matches!(
        PreparedHq::new(44_100., [f32::NAN, 0.7, 0.0]),
        Err(ResearchError::Knob)
    ));
    for rate in [0., -1., f32::NAN, f32::INFINITY, 48_000.] {
        assert!(matches!(
            PreparedHq::new(rate, knobs),
            Err(ResearchError::Rate)
        ));
    }
}

#[test]
fn research_history_ownership_and_reuse_is_bounded() {
    // Fixed-storage/no-drop API facts. This is NOT an allocator-count measurement.
    assert!(!std::mem::needs_drop::<ResearchVoice<'_>>());
    assert!(std::mem::size_of::<ResearchVoice<'_>>() <= 2048);
    let knobs = [0.5, 0.7, 0.0];
    let p = PreparedHq::new(44_100., knobs).unwrap();
    let mut v = ResearchVoice::new(&p, Contract::StaticKnobsLiveQuality);
    let (mut l, mut r, _) = authored_input(1);
    v.process(
        &mut l[..64],
        &mut r[..64],
        Routes {
            quality: Some(&[1., 1.]),
            ..Routes::default()
        },
    )
    .unwrap();
    let histories = v.resampler;
    let poles = v.kernel.state.map(|c| c.map(f32::to_bits));
    let phase = v.quartet_phase;
    v.change_quality(true);
    assert_eq!(format!("{:?}", v.resampler), format!("{:?}", histories));
    v.change_quality(false);
    assert_eq!(format!("{:?}", v.resampler), format!("{:?}", histories));
    v.change_quality(true);
    assert_eq!(v.resampler, [State::default(); 2]);
    assert_eq!(v.kernel.state.map(|c| c.map(f32::to_bits)), poles);
    assert_eq!(v.quartet_phase, phase);
    let fresh = ResearchVoice::new(&p, Contract::StaticKnobsLiveQuality);
    assert_eq!(fresh.resampler, [State::default(); 2]);
    assert_eq!(fresh.host_countdown, 0);
    assert_eq!(fresh.quartet_phase, 0);
}

/// Optional external integration-counting seam; the checker owns instrumentation.
/// Preparation is outside the supplied callback guard. No production export exists.
pub(crate) fn callback_storage_case(check: impl FnOnce(&mut dyn FnMut())) {
    let knobs = [0.5, 0.7, 0.0];
    let p = PreparedHq::new(44_100.0, knobs).unwrap();
    let fixed_seed = ResearchVoice::new(&p, Contract::FixedHqMovingGain);
    let live_seed = ResearchVoice::new(&p, Contract::StaticKnobsLiveQuality);
    let (fixed_l, fixed_r, _) = authored_input(0);
    let (live_l, live_r, _) = authored_input(1);
    let mut process = || {
        for _ in 0..8 {
            let mut fixed = fixed_seed;
            let mut live = live_seed;
            fixed
                .process(&mut [0.0; 32], &mut [0.0; 32], Routes::default())
                .unwrap();
            live.process(&mut [0.0; 32], &mut [0.0; 32], Routes::default())
                .unwrap();
            let (mut l, mut r) = (fixed_l, fixed_r);
            fixed
                .process(
                    &mut l[..64],
                    &mut r[..64],
                    Routes {
                        gain: Some(&[0.0, 0.5]),
                        ..Routes::default()
                    },
                )
                .unwrap();
            let (mut l, mut r) = (live_l, live_r);
            live.process(
                &mut l,
                &mut r,
                Routes {
                    quality: Some(&[1., 1., 1., 0., 1.]),
                    ..Routes::default()
                },
            )
            .unwrap();
            std::hint::black_box((fixed.audit(), live.audit(), l, r));
        }
    };
    check(&mut process);
}

#[test]
fn research_callback_fixed_storage_case_runs_without_drop_owners() {
    callback_storage_case(|callback| callback());
}
