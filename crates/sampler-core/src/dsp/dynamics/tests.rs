//! Engine-level checks: every law through the one public path.
use super::*;
use sampler_ir::Time;
use sampler_ir::fx::dynamics::{Direction, Generic, Maximizer, Native, Topology};

const BLOCK: usize = super::super::BLOCK;

fn settings(law: Law, link: Link, makeup: f64, mix: f64, knobs: [f64; 4]) -> DynamicsSettings {
    let [t, r, a, rl] = knobs.map(Parameter::Constant);
    DynamicsSettings { law, link, makeup, mix, threshold: t, ratio: r, attack_seconds: a, release_seconds: rl }
}

/// Output bits of `s` over a stereo test signal in uneven blocks, FNV-folded.
fn fingerprint(s: DynamicsSettings, rate: u32) -> u64 {
    assert!(s.valid());
    let mut cells = 0;
    let d = s.prepare(rate, &mut vec![], &mut cells).unwrap();
    let mut slab = vec![[0.; 2]; cells];
    let mut state = ProcessorState::default();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let (mut n, mut k) = (0usize, 0usize);
    let sizes = [64, 17, 1, 64, 33, 64, 5, 64];
    while n < 12_000 {
        let len = sizes[k % sizes.len()];
        k += 1;
        let mut block = [[0.; BLOCK]; 2];
        for i in 0..len {
            let t = (n + i) as f64 / f64::from(rate);
            let env = if (n + i) % 3000 < 1500 { 1.2 } else { 0.05 };
            let l = env * ((t * 440. * std::f64::consts::TAU).sin() * 0.7 + (t * 3100. * std::f64::consts::TAU).sin() * 0.3);
            let r = env * (t * 997. * std::f64::consts::TAU).cos() * 0.5;
            (block[0][i], block[1][i]) = (l, r);
        }
        d.process(&mut state, &mut slab, &[], &mut block, len, n as u64);
        for i in 0..len {
            for ch in 0..2 {
                hash = (hash ^ block[ch][i].to_bits()).wrapping_mul(0x100_0000_01b3).rotate_left(29);
            }
        }
        n += len;
    }
    hash
}

fn generic(direction: Direction, detector: Detector, topology: Topology, lookahead_ms: f64, hp: f64, knee: f64) -> Law {
    Law::Generic(Generic {
        threshold_db: -20.,
        ratio: 4.,
        knee_db: knee,
        direction,
        detector,
        topology,
        lookahead: Time::Milliseconds(lookahead_ms),
        sidechain_highpass_hz: hp,
        range_db: 0.,
    })
}

fn cases() -> Vec<(&'static str, DynamicsSettings, u32)> {
    let native = Native { threshold_db: 0., ratio: 0. };
    let max = Maximizer {
        threshold_db: -6.,
        attack_ms: 1.,
        lookahead_ms: 2.,
        release_ms: 10.,
        ceiling_db: -0.1,
        knee_db: 3.,
        release_blend: 0.1,
        slew_ms: 20.,
        mode: 1,
        true_peak: false,
    };
    let ff = Topology::FeedForward;
    let rms = Detector::Rms { window: Time::Milliseconds(5.) };
    vec![
        ("generic peak", settings(generic(Direction::Downward, Detector::Peak, ff, 0., 0., 0.), Link::Off, 1., 1., [-20., 4., 0.001, 0.05]), 48_000),
        ("generic knee la mean", settings(generic(Direction::Downward, Detector::Peak, ff, 3., 80., 6.), Link::Mean, 1.5, 0.7, [-20., 4., 0.001, 0.05]), 48_000),
        ("generic rms max", settings(generic(Direction::Downward, rms, ff, 0., 0., 0.), Link::Max, 1., 1., [-30., 8., 0.003, 0.1]), 44_100),
        ("generic true peak", settings(generic(Direction::Downward, Detector::TruePeak, ff, 1., 0., 2.), Link::Off, 1., 1., [-6., 20., 0., 0.05]), 48_000),
        ("generic feedback", settings(generic(Direction::Downward, Detector::Peak, Topology::Feedback, 0., 0., 0.), Link::Max, 1., 1., [-20., 4., 0.001, 0.05]), 48_000),
        ("generic upward", settings(generic(Direction::Upward, Detector::Peak, ff, 0., 0., 4.), Link::Off, 1., 1., [-30., 3., 0.001, 0.05]), 48_000),
        ("generic expand", settings(generic(Direction::Expand, rms, ff, 0., 0., 0.), Link::Off, 1., 0.5, [-25., 3., 0.001, 0.05]), 48_000),
        ("classic", settings(Law::KontaktClassic(native), Link::Mean, 1., 1., [-12., 0.5, 0.001, 0.1]), 48_000),
        ("classic off mix", settings(Law::KontaktClassic(native), Link::Off, 1.2, 0.6, [-6., 0.8, 0.0, 0.02]), 44_100),
        ("enhanced", settings(Law::KontaktEnhanced(native), Link::Mean, 1., 1., [-12., 0.5, 0.001, 0.1]), 48_000),
        ("enhanced off mix", settings(Law::KontaktEnhanced(native), Link::Off, 0.8, 0.4, [-20., 0.02, 0.01, 0.03]), 48_000),
        ("pro", settings(Law::KontaktPro(native), Link::Max, 1., 1., [-20., 0.5, 0.001, 0.1]), 48_000),
        ("pro mix", settings(Law::KontaktPro(native), Link::Max, 1.3, 0.5, [-10., 0.9, 0.00005, 0.5]), 96_000),
        ("limiter", settings(Law::KontaktLimiter(native), Link::Max, 1., 1., [6., 0., 0., 0.1]), 48_000),
        ("limiter mix", settings(Law::KontaktLimiter(native), Link::Max, 0.9, 0.5, [12., 0., 0., 0.01]), 44_100),
        ("hilbert", settings(Law::KontaktHilbert(native), Link::Max, 1., 1., [0.5, 1., 0., 0.05]), 48_000),
        ("hilbert 96k mix", settings(Law::KontaktHilbert(native), Link::Max, 1.1, 0.5, [0.2, 0.6, 0., 0.3]), 96_000),
        ("feedback", settings(Law::KontaktFeedback { native, high_quality: false }, Link::Max, 1., 1., [3., 8., 0.002, 0.1]), 48_000),
        ("feedback hq off", settings(Law::KontaktFeedback { native, high_quality: true }, Link::Off, 1.4, 0.7, [10., 20., 0.0005, 0.05]), 44_100),
        ("transient", settings(Law::KontaktTransient { attack: 0.9, sustain: 0.2, gain: 0.6, clip: false }, Link::Off, 1., 1., [0.; 4]), 48_000),
        ("transient clip", settings(Law::KontaktTransient { attack: 0.1, sustain: 0.9, gain: 0.3, clip: true }, Link::Off, 1., 1., [0.; 4]), 44_100),
        ("bus", settings(Law::KontaktBusComp { threshold: 0.2, makeup: 0.4, attack: 0.5, release: 0.5, ratio: 0.6, link: true }, Link::Off, 1., 0.8, [0.; 4]), 48_000),
        ("bus auto free", settings(Law::KontaktBusComp { threshold: 0.1, makeup: 0.2, attack: 0.2, release: 1., ratio: 1., link: false }, Link::Off, 1., 1., [0.; 4]), 48_000),
        ("maximizer soft", settings(Law::UviMaximizer(max), Link::Max, 1., 1., [0.; 4]), 48_000),
        ("maximizer brick tp", settings(Law::UviMaximizer(Maximizer { mode: 0, true_peak: true, knee_db: 0., ..max }), Link::Max, 1., 1., [0.; 4]), 44_100),
        ("maximizer hard", settings(Law::UviMaximizer(Maximizer { mode: 2, release_blend: 0.7, ..max }), Link::Max, 1., 1., [0.; 4]), 96_000),
    ]
}

/// Output bits of every law before the engine was unified into one pipeline.
const GOLDEN: &[(&str, u64)] = &[
    ("generic peak", 0x0b234606c124428e),
    ("generic knee la mean", 0x4ec588d822303cb0),
    ("generic rms max", 0xbc7558b943ac256b),
    ("generic true peak", 0xd5d9b76e7dfe07e1),
    ("generic feedback", 0x925cb511f1005be4),
    ("generic upward", 0x146475643c750832),
    ("generic expand", 0x6d87256952a2996d),
    ("classic", 0xf20861a7734578ef),
    ("classic off mix", 0x04453875d9a1cbf3),
    ("enhanced", 0x940a3b4e40dec64b),
    ("enhanced off mix", 0x4213fed2060682fe),
    ("pro", 0x63d5a45c16c8abc2),
    ("pro mix", 0x827f7e23abea469e),
    ("limiter", 0xc6150cc2408f5657),
    ("limiter mix", 0xbd8b804e64b8526b),
    ("hilbert", 0x6881215429ca8cac),
    ("hilbert 96k mix", 0x96a80d7003f8ddc0),
    ("feedback", 0x8edb73e29bd4bbce),
    ("feedback hq off", 0x8db678df6125aacd),
    ("transient", 0x853f08b0665bdaae),
    ("transient clip", 0x0971909af8d5158a),
    ("bus", 0xac9e7ba00aaebf2e),
    ("bus auto free", 0x222c621303ded51f),
    ("maximizer soft", 0x237cad28f4a94200),
    ("maximizer brick tp", 0x061b59db6af50fec),
    ("maximizer hard", 0x89bcd291a67286cd),
];

#[test]
fn every_law_matches_its_golden_output_bits() {
    let got: Vec<_> = cases().into_iter().map(|(name, s, rate)| (name, fingerprint(s, rate))).collect();
    assert_eq!(got.len(), GOLDEN.len());
    let bad: Vec<_> = got.iter().zip(GOLDEN).filter(|(g, w)| g.1 != w.1).map(|(g, _)| format!("{} {:#018x}", g.0, g.1)).collect();
    assert!(bad.is_empty(), "{bad:#?}");
}

/// ns/frame per golden case; run optimized with `--ignored --nocapture`.
#[test]
#[ignore]
fn bench_every_law() {
    let only = std::env::var("BENCH_ONLY").unwrap_or_default();
    for (name, s, rate) in cases().into_iter().filter(|c| c.0.starts_with(only.as_str())) {
        let mut cells = 0;
        let d = s.prepare(rate, &mut vec![], &mut cells).unwrap();
        let mut slab = vec![[0.; 2]; cells];
        let mut state = ProcessorState::default();
        let input: Vec<[[f64; BLOCK]; 2]> = (0..94)
            .map(|b| {
                std::array::from_fn(|ch| {
                    std::array::from_fn(|i| {
                        let n = (b * BLOCK + i) as f64;
                        let env = if (b * BLOCK + i) % 3000 < 1500 { 1.2 } else { 0.05 };
                        env * if ch == 0 { (n * 0.0576).sin() } else { 0.5 * (n * 0.1305).cos() }
                    })
                })
            })
            .collect();
        let frames = 500_000usize;
        let mut best = f64::MAX;
        for _ in 0..6 {
            let t = std::time::Instant::now();
            for b in 0..frames / BLOCK {
                let mut block = input[b % input.len()];
                d.process(&mut state, &mut slab, &[], &mut block, BLOCK, (b * BLOCK) as u64);
                std::hint::black_box(&block);
            }
            best = best.min(t.elapsed().as_nanos() as f64 / frames as f64);
        }
        eprintln!("{name:24} {best:6.2} ns/frame");
    }
}

const NATIVE: Native = Native { threshold_db: 0., ratio: 0. };

/// `input` through `law` (make-up 1, fully wet) in whole blocks.
fn run_law(law: Law, link: Link, knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let shared = Shared { link, makeup: 1., mix: 1. };
    let mut state = ProcessorState::default();
    let mut cells = vec![[0.; 2]; laws::cells(&law, 48_000).unwrap()];
    let mut out = vec![];
    for chunk in input.chunks(BLOCK) {
        let mut block = [[0.; BLOCK]; 2];
        for (i, f) in chunk.iter().enumerate() {
            (block[0][i], block[1][i]) = (f[0], f[1]);
        }
        laws::process(law, &shared, knobs, 48_000, &mut state, &mut cells, &mut block, chunk.len());
        out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
    }
    out
}

fn sine(amp: f64, hz: f64, frames: usize) -> Vec<[f64; 2]> {
    (0..frames)
        .map(|i| {
            let v = amp * (i as f64 * hz * std::f64::consts::TAU / 48_000.).sin();
            [v, v]
        })
        .collect()
}

/// The kemu native-vector stimulus (`tools/re/dynemu.py NAME` prints each core's vectors): 1024-frame bursts.
fn stimulus() -> Vec<[f64; 2]> {
    (0..4096)
        .map(|n| {
            let env = if (n / 1024) % 2 == 0 { 1.2 } else { 0.05 };
            let n = n as f64;
            [env * (n * 0.0576).sin(), env * 0.5 * (n * 0.1305).cos()]
        })
        .collect()
}

/// `out` at the native core's sampled frames, to f32 precision.
fn assert_native(out: &[[f64; 2]], native: &[(usize, f32, f32)]) {
    for &(n, l, r) in native {
        for (got, want) in out[n].iter().zip([l, r]) {
            let tol = 1e-6 * f64::from(want.abs()).max(1e-3);
            assert!((got - f64::from(want)).abs() <= tol, "{n}: {:?} vs {l} {r}", out[n]);
        }
    }
}

fn peak(out: &[[f64; 2]]) -> f64 {
    out.iter().map(|f| f[0].abs()).fold(0., f64::max)
}

mod generic {
    use super::*;
    use crate::dsp::dynamics::computer::{MAX_BOOST_DB, Ratio, table_db, table_gain};

    fn g(direction: Direction, knee_db: f64) -> Generic {
        Generic {
            threshold_db: -20.,
            ratio: 4.,
            knee_db,
            direction,
            detector: Detector::Peak,
            topology: Topology::FeedForward,
            lookahead: Time::ZERO,
            sidechain_highpass_hz: 0.,
            range_db: 0.,
        }
    }

    fn at(g: &Generic, ratio: f64) -> Ratio {
        laws::generic_ratio(g, [g.threshold_db, ratio, 0., 0.], 2)
    }

    fn lin(db: f64) -> f64 {
        (db / computer::DB_PER_NEPER).exp()
    }

    #[test]
    fn hard_knee_reduction_is_over_times_slope() {
        let c = at(&g(Direction::Downward, 0.), 4.);
        assert_eq!(c.target(lin(-30.)), 0.);
        assert!((c.target(lin(-8.)) - 12. * 0.75).abs() < 1e-5);
    }

    #[test]
    fn soft_knee_is_continuous_and_meets_the_hard_curve() {
        let c = at(&g(Direction::Downward, 12.), 4.);
        // Zero at the lower knee edge, the hard value at the upper one.
        assert!(c.target(lin(-26.)).abs() < 1e-5);
        assert!((c.target(lin(-14.)) - 6. * 0.75).abs() < 1e-5);
        // Centre: slope * (W/2)^2 / (2W) = slope * W / 8.
        assert!((c.target(lin(-20.)) - 0.75 * 12. / 8.).abs() < 1e-5);
        let mut previous = 0.;
        for step in 0..=240 {
            let value = c.target(lin(-30. + f64::from(step) * 0.05));
            assert!(value >= previous - 1e-12);
            previous = value;
        }
    }

    #[test]
    fn expander_lowers_the_level_below_the_threshold() {
        let c = at(&g(Direction::Expand, 0.), 3.);
        assert_eq!(c.target(lin(-10.)), 0.);
        assert!((c.target(lin(-30.)) - 10. * 2.).abs() < 1e-5);
    }

    #[test]
    fn upward_compression_raises_quiet_levels_to_a_ceiling() {
        let c = at(&g(Direction::Upward, 0.), 2.);
        assert!((c.target(lin(-40.)) - 20. * 0.5).abs() < 1e-5);
        assert_eq!(c.target(0.), MAX_BOOST_DB);
    }

    #[test]
    fn tables_track_libm() {
        for k in 0..2000 {
            let level = 1e-9 * 1.0137f64.powi(k);
            assert!((table_db(level) - 20. * level.log10()).abs() < 1e-5, "{level}");
            let db = -150. + k as f64 * 0.1;
            assert!((table_gain(db) / 10f64.powf(db / 20.) - 1.).abs() < 1e-6, "{db}");
        }
    }

    #[test]
    fn true_peak_sees_the_inter_sample_peak() {
        // fs/4 sine at 45 degrees: samples at 0.707, true peak 1 (+3 dB).
        let mut law = g(Direction::Downward, 0.);
        law.threshold_db = -2.;
        let input: Vec<[f64; 2]> = (0..256)
            .map(|i| {
                let v = (std::f64::consts::FRAC_PI_2 * i as f64 + std::f64::consts::FRAC_PI_4).sin();
                [v, v]
            })
            .collect();
        let mut out = [0.; 2];
        for (i, detector) in [Detector::Peak, Detector::TruePeak].into_iter().enumerate() {
            law.detector = detector;
            out[i] = peak(&run_law(Law::Generic(law), Link::Off, [-2., 100., 0., 0.], &input)[192..]);
        }
        assert!((out[0] - 0.7071).abs() < 1e-3, "{out:?}");
        assert!(out[1] < 0.84, "{out:?}");
    }
}

mod classic {
    use super::*;
    use crate::dsp::dynamics::laws::Classic;

    fn run(knobs: [f64; 4], link: Link, input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        run_law(Law::KontaktClassic(NATIVE), link, knobs, input)
    }

    #[test]
    fn coefficients_follow_the_setter() {
        // Threshold -12 dB -> 0.25; ratio 0.5 -> g = 0.25^0.5.
        let c = Classic::new([-12., 0.5, 0.01, 0.1], 48_000);
        assert!((f64::from(c.threshold) - 0.25).abs() < 1e-6);
        assert!((f64::from(c.gain) - 0.5).abs() < 1e-5);
        // 10 ms at 48 kHz = 480 frames; 100 ms = 4800.
        assert!((c.attack - 0.625_f64.powf(1. / 480.)).abs() < 1e-12);
        assert!((c.release - (1. / 3_f64).powf(1. / 4800.)).abs() < 1e-12);
        // Zero time still moves one frame at a time.
        assert!((Classic::new([0., 0., 0., 0.], 48_000).attack - 0.625).abs() < 1e-12);
    }

    #[test]
    fn below_the_threshold_the_signal_is_untouched() {
        let input = vec![[0.1, -0.05]; 256];
        let out = run([-12., 0.5, 0.01, 0.1], Link::Mean, &input);
        for (o, i) in out.iter().zip(&input) {
            assert_eq!(*o, [f64::from(i[0] as f32), f64::from(i[1] as f32)]);
        }
    }

    #[test]
    fn a_loud_signal_settles_to_the_fixed_gain() {
        // Zero attack: the follower jumps to 1 on the first loud sample.
        let out = run([-12., 0.5, 0., 0.1], Link::Mean, &vec![[1., 1.]; 64]);
        // First sample: the follower has taken one 0.625 step toward the hold.
        assert!((out[0][0] - (1. - 0.5 * 0.375)).abs() < 1e-6, "{}", out[0][0]);
        assert!((out[63][0] - 0.5).abs() < 0.01, "{}", out[63][0]);
    }

    #[test]
    fn the_signed_mean_cancels_opposite_channels_only_when_linked() {
        let input = vec![[1., -1.]; 64];
        let linked = run([-12., 0.5, 0., 0.1], Link::Mean, &input);
        let free = run([-12., 0.5, 0., 0.1], Link::Off, &input);
        assert_eq!(linked[63], [1., -1.]);
        assert!((free[63][0] - 0.5).abs() < 0.01 && (free[63][1] + 0.5).abs() < 0.01);
    }

    #[test]
    fn release_returns_to_unity_without_overshoot() {
        let mut input = vec![[1., 1.]; 32];
        input.extend(vec![[0.01, 0.01]; 4800]);
        let out = run([-12., 0.5, 0., 0.02], Link::Mean, &input);
        // The follower may still be catching up with the hold for a few frames.
        let tail: Vec<f64> = out[132..].iter().map(|f| f[0] / 0.01).collect();
        let bad: Vec<_> = tail.windows(2).enumerate().filter(|(_, w)| w[1] < w[0] - 1e-6).take(3).collect();
        assert!(bad.is_empty(), "monotone release {bad:?}");
        assert!(tail.last().unwrap() > &0.99);
        assert!(tail.iter().all(|g| *g <= 1.0000001));
    }
}

mod enhanced {
    use super::*;

    /// The documented witness: -6 dB, ratio .5, attack 1 ms, release 100 ms,
    /// 48 kHz, four repeated `[1, -.5]` frames (extracted-law vector).
    #[test]
    fn extracted_law_witness() {
        let out = run_law(Law::KontaktEnhanced(NATIVE), Link::Off, [-6., 0.5, 0.001, 0.1], &[[1., -0.5]; 4]);
        let left = [0.999157310, 0.997557640, 0.995283782, 0.992414534];
        for (got, want) in out.iter().zip(left) {
            assert!((got[0] - want).abs() < 1e-8, "{} vs {want}", got[0]);
        }
        assert!(out.iter().all(|f| f[1] == -0.5));
    }

    #[test]
    fn linked_opposite_channels_pass_unchanged() {
        let out = run_law(Law::KontaktEnhanced(NATIVE), Link::Mean, [-6., 0.5, 0.001, 0.1], &[[1., -1.]; 4]);
        assert!(out.iter().all(|f| *f == [1., -1.]));
    }
}

mod pro {
    use super::*;
    use crate::dsp::dynamics::laws::{KONTAKT_LATENCY as LATENCY, Pro};

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        run_law(Law::KontaktPro(NATIVE), Link::Max, knobs, input)
    }

    #[test]
    fn audio_is_delayed_by_the_window() {
        let mut input = vec![[0.; 2]; 100];
        input[0] = [0.01, -0.02];
        let out = run([0., 0.5, 0.001, 0.1], &input);
        assert!(out[..LATENCY].iter().all(|f| *f == [0., 0.]));
        // Far below the threshold: unity, 32 frames late.
        assert!((out[LATENCY][0] - f64::from(0.01_f32)).abs() < 1e-7, "{:?}", out[LATENCY]);
        assert!((out[LATENCY][1] + f64::from(0.02_f32)).abs() < 1e-7);
    }

    #[test]
    fn steady_loud_input_settles_to_the_static_gain() {
        // 0 dBFS against -20 dB, saved ratio .5: 20 dB over, half removed,
        // plus the threshold-tracking make-up (0.666 * .5 * 20 dB).
        let out = run([-20., 0.5, 0.001, 0.1], &vec![[1., 1.]; 4800]);
        let gain = out[4799][0];
        let want = 2_f64.powf((-10. + 0.5 * 20. * 0.666) * 0.1661);
        assert!((gain - want).abs() < 0.01, "{gain} vs {want}");
        assert!(gain < 1.);
    }

    #[test]
    fn the_gain_leads_a_burst_by_the_window() {
        // A burst after silence: its gain starts falling before the burst
        // leaves the delay, because the window saw it 32 frames earlier.
        let mut input = vec![[0.; 2]; 400];
        input[100..300].fill([1., 1.]);
        let out = run([-20., 0.5, 0.0001, 0.1], &input);
        let (first, settled) = (out[132][0], out[299][0]);
        assert!(first < 1. && first > settled * 0.9, "{first} {settled}");
    }

    #[test]
    fn the_channel_maximum_drives_both_channels() {
        let out = run([-20., 0.5, 0.001, 0.1], &vec![[1., 0.01]; 4800]);
        let ratio = out[4799][1] / out[4799][0];
        assert!((ratio - 0.01).abs() < 1e-6, "one gain for both channels: {ratio}");
    }

    #[test]
    fn detector_low_pass_has_unity_dc_gain_and_clamped_times() {
        let c = Pro::new([-20., 0.5, 0.0001, 0.001], 48_000);
        // Release clamps at 10 ms, attack at 0.1 ms.
        let same = Pro::new([-20., 0.5, 0.0001, 0.01], 48_000);
        assert_eq!(c.release.to_bits(), same.release.to_bits());
        let dc = (2. * c.b0 + c.b1) / (1. - c.a1 - c.a2);
        assert!((dc - 1.).abs() < 1e-3, "{dc}");
    }
}

mod limiter {
    use super::*;
    use crate::dsp::dynamics::laws::{KONTAKT_LATENCY as LATENCY, Limiter};

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        run_law(Law::KontaktLimiter(NATIVE), Link::Max, knobs, input)
    }

    #[test]
    fn audio_is_delayed_by_the_window() {
        let mut input = vec![[0.; 2]; 100];
        input[0] = [0.01, -0.02];
        let out = run([0., 0., 0., 0.1], &input);
        assert!(out[..LATENCY].iter().all(|f| *f == [0., 0.]));
        // A tiny sample sees y ~ 0: unity gain, 32 frames late.
        assert!((out[LATENCY][0] - f64::from(0.01_f32)).abs() < 1e-5, "{:?}", out[LATENCY]);
        assert!((out[LATENCY][1] + f64::from(0.02_f32)).abs() < 1e-5);
    }

    #[test]
    fn steady_full_scale_settles_to_the_soft_curve() {
        // y -> 1 (unity filter DC gain), gain 1/(1 + 1/4) = 0.8.
        let out = run([0., 0., 0., 0.1], &vec![[1., 1.]; 4800]);
        assert!((out[4799][0] - 0.8).abs() < 1e-3, "{}", out[4799][0]);
    }

    #[test]
    fn a_hot_input_is_held_at_full_scale() {
        // 12 saved dB is x4: y -> 4 > 2, gain 1/y, output exactly full scale.
        let out = run([12., 0., 0., 0.1], &vec![[1., -1.]; 4800]);
        assert!((out[4799][0] - 1.).abs() < 1e-3, "{}", out[4799][0]);
        assert!((out[4799][1] + 1.).abs() < 1e-3);
        assert!(out.iter().flatten().all(|v| v.abs() <= 1.));
    }

    #[test]
    fn the_channel_maximum_drives_both_channels() {
        let out = run([0., 0., 0., 0.1], &vec![[1., 0.01]; 4800]);
        let ratio = out[4799][1] / out[4799][0];
        assert!((ratio - 0.01).abs() < 1e-5, "{ratio}");
    }

    #[test]
    fn release_is_the_half_life_of_the_level_follower() {
        let c = Limiter::new([0., 0., 0., 0.1], 48_000);
        // 100 ms = 4800 frames: after that many, the gap halves (to ln2 accuracy).
        let remaining = (1. - c.release).powi(4800);
        assert!((remaining - 0.5).abs() < 0.02, "{remaining}");
        // Gain is 2^(x/6): six saved dB double the input.
        assert!((Limiter::new([6., 0., 0., 0.1], 48_000).gain - 2.).abs() < 1e-6);
    }
}

mod hilbert {
    use super::*;
    use crate::dsp::dynamics::laws::{Hilbert, hilbert_window};

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        run_law(Law::KontaktHilbert(NATIVE), Link::Max, knobs, input)
    }

    #[test]
    fn window_and_latency_follow_the_rate() {
        let latency = laws::latency(&Law::KontaktHilbert(NATIVE), 48_000);
        assert_eq!((hilbert_window(44_100), latency), (128, 127));
        assert_eq!(hilbert_window(96_000), 256);
    }

    #[test]
    fn quiet_audio_passes_at_the_ceiling_after_the_latency() {
        // Unity drive (x0 = 1), ceiling -0.35 dB (x2 = 1).
        let mut input = vec![[0.; 2]; 400];
        input[10] = [0.01, -0.02];
        let out = run([1., 1., 0., 0.01], &input);
        assert!(out[..10 + 127].iter().all(|f| f[0].abs() < 1e-9));
        let ceiling = 10f64.powf(-0.35 / 20.);
        assert!((out[137][0] - 0.01 * ceiling).abs() < 1e-5, "{:?}", out[137]);
        assert!((out[137][1] + 0.02 * ceiling).abs() < 1e-5);
    }

    #[test]
    fn a_hot_sine_never_passes_the_ceiling() {
        // 20 dB of drive into a full-scale sine: every output peak stays at
        // or under the ceiling, and the limiter is doing real work.
        let out = run([0.5, 1., 0., 0.05], &sine(1., 440., 48_000));
        let ceiling = 10f64.powf(-0.35 / 20.);
        let peak = peak(&out);
        assert!(peak <= ceiling * 1.000_01, "{peak}");
        assert!(peak > ceiling * 0.9, "{peak}");
    }

    /// Native `NI::DSP::HilbertLimiter` (ctor 0x141adbea0, prepare 0x141ae23a0,
    /// setter 0x141aea680, process 0x141ae3400) under tools/re/kemu.py, 48 kHz,
    /// controls drive .5, release .05, ceiling 1.
    #[test]
    fn matches_the_native_core() {
        let out = run([0.5, 1., 0., 0.05], &stimulus());
        let native: [(usize, f32, f32); 10] = [
            (0, 0.0, 0.0),
            (1, 0.0, 0.0),
            (10, 0.0, 0.0),
            (100, 0.0, 0.0),
            (1000, 0.01855362020432949, 0.3244099020957947),
            (1500, -0.02422836422920227, -0.02323775552213192),
            (2047, -0.0468631274998188, 0.028381407260894775),
            (2500, -0.9601988196372986, -0.10931152105331421),
            (3000, 0.8182634711265564, -0.22763866186141968),
            (4095, 0.05741879716515541, -0.03505796566605568),
        ];
        assert_native(&out, &native);
    }

    #[test]
    fn hilbert_pair_is_ninety_degrees_apart_mid_band() {
        // A 1 kHz sine: the envelope magnitude settles flat (no ripple at 2f).
        let c = Hilbert::new([1., 1., 0., 0.01], 48_000);
        let mut rings = [[0f32; 8]; 7 * 4];
        let mut mags = vec![];
        for n in 0..4_000usize {
            let x = (n as f32 * 1000. * std::f32::consts::TAU / 48_000.).sin();
            let mut lanes = [x; 4];
            let mut last = lanes;
            for (s, coef) in c.allpass.iter().enumerate() {
                last = lanes;
                for (k, lane) in lanes.iter_mut().enumerate() {
                    let ring = &mut rings[s * 4 + k];
                    let r = ring[(n + 1 - c.delay) & 7];
                    let v = (*lane - r) * coef[k];
                    ring[(n + 1) & 7] = v + *lane;
                    *lane = v + r;
                }
            }
            mags.push((last[0] * last[0] + lanes[1] * lanes[1]).sqrt());
        }
        let tail = &mags[3_000..];
        let (lo, hi) = tail.iter().fold((f32::MAX, 0f32), |(a, b), m| (a.min(*m), b.max(*m)));
        assert!(hi - lo < 0.01 && (hi - 1.).abs() < 0.01, "{lo} {hi}");
    }
}

mod feedback {
    use super::*;
    use crate::dsp::dynamics::laws::Feedback;

    fn run(knobs: [f64; 4], input: &[[f64; 2]]) -> Vec<[f64; 2]> {
        run_law(Law::KontaktFeedback { native: NATIVE, high_quality: true }, Link::Max, knobs, input)
    }

    #[test]
    fn ratio_law_is_continuous_at_the_knee_top() {
        for x in [0.1f32, 0.5, 1.] {
            let c = Feedback::new([1., f64::from(x * 19. + 1.), 0.01, 0.1], true, 48_000).curve;
            let l = c.knee_top / c.threshold;
            let knee = (c.quartic * l.ln().powi(4)).exp();
            assert!((knee - c.g0).abs() < 1e-3 * c.g0, "{x}: {knee} {}", c.g0);
            assert!(c.threshold > 0. && l > 1. && c.slope > 1.);
        }
    }

    /// Native `NI::DSP::FBComp` (vtable 0x1479aad18, prepare 0x141aa1280,
    /// setter 0x141ac56d0, process 0x141aa80d0) under tools/re/kemu.py, 48 kHz,
    /// controls 0..5 = .6 .5 .3 .4 .5 1, 6 (high quality) on, 8 on.
    #[test]
    fn matches_the_native_core() {
        let [input, output, attack, release, ratio] = [0.6f32, 0.5, 0.3, 0.4, 0.5];
        let knobs = [
            f64::from(10f32.powf(input * 2.5 - 0.5)),
            f64::from(ratio * 19. + 1.),
            f64::from(attack * f32::from_bits(0x3d4c_6061) + f32::from_bits(0x38d8_d874)),
            f64::from(release * f32::from_bits(0x3f93_3334) + 0.05),
        ];
        let law = Law::KontaktFeedback { native: NATIVE, high_quality: true };
        let mut state = ProcessorState::default();
        let mut cells = vec![[0.; 2]; laws::cells(&law, 48_000).unwrap()];
        let shared = Shared { link: Link::Max, makeup: f64::from(10f32.powf(output * 2.5 - 2.)), mix: 1. };
        let mut out = vec![];
        for chunk in stimulus().chunks(BLOCK) {
            let mut block = [[0.; BLOCK]; 2];
            for (i, f) in chunk.iter().enumerate() {
                (block[0][i], block[1][i]) = (f[0], f[1]);
            }
            laws::process(law, &shared, knobs, 48_000, &mut state, &mut cells, &mut block, chunk.len());
            out.extend((0..chunk.len()).map(|i| [block[0][i], block[1][i]]));
        }
        let native: [(usize, f32, f32); 10] = [
            (0, 1.2859280009385184e-19, 0.7715567946434021),
            (1, 0.08883419632911682, 1.190497875213623),
            (10, 1.1349705457687378, 0.29483091831207275),
            (100, -0.8551082611083984, 0.7469831109046936),
            (1000, 0.3325933814048767, 0.021470604464411736),
            (1500, -0.027021536603569984, 0.0051622469909489155),
            (2047, -0.0297965370118618, -0.013756771571934223),
            (2500, -0.1530163437128067, 0.13874685764312744),
            (3000, -0.010451612062752247, -0.06928698718547821),
            (4095, -0.006039154715836048, 0.013333257287740707),
        ];
        assert_native(&out, &native);
    }

    #[test]
    fn ratio_one_is_transparent_apart_from_the_filters() {
        let out = run([1., 1., 0.001, 0.1], &sine(0.5, 1000., 9600));
        assert!((peak(&out[4800..]) - 0.5).abs() < 0.01, "{}", peak(&out[4800..]));
    }

    #[test]
    fn a_loud_signal_is_compressed_more_than_a_quiet_one() {
        // The detector high-passes the rectified drive, so a steady sine
        // reads about 0.36 of its driven peak: drive 10 puts 1.0 above T.
        let knobs = [10., 20., 0.001, 0.1];
        let loud = peak(&run(knobs, &sine(1., 1000., 24_000))[19_200..]);
        let quiet = peak(&run(knobs, &sine(0.1, 1000., 24_000))[19_200..]);
        // 20 dB in gives clearly less than 20 dB out.
        assert!(loud / quiet < 5., "{loud} {quiet}");
    }
}

mod transient {
    use super::*;
    use crate::dsp::dynamics::detector::tenth_root;

    fn run([attack, sustain, gain]: [f64; 3], input: &[[f64; 2]]) -> Vec<f64> {
        let law = Law::KontaktTransient { attack, sustain, gain, clip: false };
        run_law(law, Link::Off, [0.; 4], input).iter().map(|f| f[0]).collect()
    }

    fn bursts() -> Vec<[f64; 2]> {
        (0..48_000)
            .map(|i| {
                // Decaying plucks (100 ms time constant), one every 250 ms.
                let t = (i % 12_000) as f64 / 48_000.;
                let v = 0.5 * (-t / 0.1).exp() * (i as f64 * 440. * std::f64::consts::TAU / 48_000.).sin();
                [v, v]
            })
            .collect()
    }

    fn rms(v: &[f64]) -> f64 {
        (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt()
    }

    #[test]
    fn neutral_controls_pass_audio_at_unity() {
        let out = run([0.5; 3], &bursts());
        let input: Vec<f64> = bursts().iter().map(|f| f[0]).collect();
        let (o, i) = (rms(&out[24_000..36_000]), rms(&input[24_000..36_000]));
        assert!((o / i - 1.).abs() < 0.01, "{o} {i}");
    }

    #[test]
    fn attack_boosts_onsets_and_sustain_shapes_tails() {
        let input = bursts();
        let onset = |c| rms(&run(c, &input)[24_000..24_480]);
        let tail = |c| rms(&run(c, &input)[30_000..34_000]);
        assert!(onset([1., 0.5, 0.5]) > onset([0.5; 3]) * 1.2);
        assert!(tail([0.5, 0., 0.5]) < tail([0.5; 3]) * 0.9);
    }

    /// Native Transient Master core (ctor 0x141a83550, reset 0x141aa0b80,
    /// prepare 0x141aa27a0, setter 0x141ac87b0, process 0x141aaac90) under
    /// tools/re/kemu.py, 48 kHz, controls .9 .2 .6, no clip; output scale
    /// (+0x70) 1 and the runtime 2^(i/60-20) table seeded.
    #[test]
    fn matches_the_native_core() {
        let law = Law::KontaktTransient { attack: 0.9, sustain: 0.2, gain: 0.6, clip: false };
        let out = run_law(law, Link::Off, [0.; 4], &stimulus());
        let native: [(usize, f32, f32); 10] = [
            (0, 0.0, 3.302320718765259),
            (1, 0.3693881332874298, 3.176668167114258),
            (10, 3.2479841709136963, 0.7567840218544006),
            (100, -2.6673171520233154, 2.3343536853790283),
            (1000, 3.002772569656372, 0.22991815209388733),
            (1500, -0.11723051965236664, 0.009873159229755402),
            (2047, -0.07166774570941925, -0.028402572497725487),
            (2500, -0.9919859170913696, 0.9227460026741028),
            (3000, -0.08136296272277832, -0.3886581361293793),
            (4095, -0.008966117165982723, 0.02501962147653103),
        ];
        assert_native(&out, &native);
    }

    #[test]
    fn table_matches_powf_inside_its_range() {
        for x in [0.001f32, 0.25, 1., 2.9] {
            assert!((tenth_root(x) - x.powf(0.1)).abs() < 1e-4, "{x}");
        }
    }
}

mod bus {
    use super::*;
    use crate::dsp::dynamics::laws::{BUS_THRESHOLD, Bus};

    #[test]
    fn tables_and_coefficients_match_the_native_values() {
        // KONTAKT_DSP_LAWS: table 0 at .05 is -17.32959641360375; 48 kHz
        // attack 3 ms .998451592027297, release 50 ms .9999070278424647.
        assert!((BUS_THRESHOLD.eval(0.05) + 17.32959641360375).abs() < 1e-12);
        let k = Bus::new([0.5, 0.2, 0.5, 0.5, 0.5], 48_000);
        assert_eq!(k.attack, 0.998451592027297);
        assert_eq!(k.release, 0.9999070278424647);
        assert!((k.makeup - 1.).abs() < 1e-8);
    }

    /// Native `G_BusComp` (ctor 0x141a7fc80, prepare 0x141aa1ea0, setter
    /// 0x141ac69e0, process 0x141aaa1e0) under tools/re/kemu.py, 48 kHz,
    /// controls .2 .4 .5 .5 .6, link on, mix 1, 64-frame blocks.
    #[test]
    fn matches_the_native_core() {
        let law = Law::KontaktBusComp { threshold: 0.2, makeup: 0.4, attack: 0.5, release: 0.5, ratio: 0.6, link: true };
        let out = run_law(law, Link::Off, [0.; 4], &stimulus());
        let native: [(usize, f32, f32); 10] = [
            (0, 0.0, 1.0981236696243286),
            (1, 0.12437347322702408, 1.0710428953170776),
            (10, 1.033585548400879, 0.24923169612884521),
            (100, -0.308159738779068, 0.2730134129524231),
            (1000, 0.09638936072587967, 0.006861408706754446),
            (1500, -0.005180184729397297, 0.0014615225372835994),
            (2047, -0.005944662727415562, -0.0029721420723944902),
            (2500, -0.054362811148166656, 0.04921232908964157),
            (3000, -0.0012858589179813862, -0.018826641142368317),
            (4095, -0.001422968809492886, 0.0026967115700244904),
        ];
        assert_native(&out, &native);
    }

    #[test]
    fn feedback_detector_reaches_the_ratio() {
        // Ratio 10 (x = 1), threshold well below the signal: 20 dB more input
        // gives about 2 dB more output.
        let law = Law::KontaktBusComp { threshold: 0., makeup: 0.2, attack: 0., release: 0., ratio: 1., link: true };
        let at = |amp| peak(&run_law(law, Link::Off, [0.; 4], &sine(amp, 1000., 96_000))[76_800..]);
        let db = 20. * (at(1.) / at(0.1)).log10();
        assert!((db - 2.).abs() < 0.5, "{db}");
    }
}

mod maximizer {
    use super::*;
    use crate::dsp::dynamics::laws::{Maximizer as Controls, maximizer_latency, maximizer_window};

    const DEFAULT: Maximizer = Maximizer {
        threshold_db: -6.,
        attack_ms: 0.,
        lookahead_ms: 2.,
        release_ms: 10.,
        ceiling_db: -0.1,
        knee_db: 0.,
        release_blend: 0.1,
        slew_ms: 20.,
        mode: 0,
        true_peak: false,
    };

    fn run(m: Maximizer, amp: f64) -> Vec<[f64; 2]> {
        run_law(Law::UviMaximizer(m), Link::Max, [0.; 4], &sine(amp, 997., 300 * BLOCK))
    }

    #[test]
    fn window_and_latency_follow_the_native_rounding() {
        assert_eq!(maximizer_window(2., 48_000), 96);
        let tp = Controls::from(&Maximizer { true_peak: true, ..DEFAULT });
        assert_eq!(maximizer_latency(&tp, 44_100), 94);
    }

    #[test]
    fn quiet_audio_gets_the_threshold_to_ceiling_gain() {
        let out = run(DEFAULT, 0.1);
        let want = 0.1 * 10f64.powf(5.9 / 20.);
        assert!((peak(&out[9600..]) / want - 1.).abs() < 0.01, "{}", peak(&out[9600..]));
    }

    #[test]
    fn loud_audio_stays_at_the_ceiling() {
        for m in [
            DEFAULT,
            Maximizer { true_peak: true, knee_db: 3., ..DEFAULT },
            Maximizer { mode: 2, attack_ms: 1., ..DEFAULT },
        ] {
            let p = peak(&run(m, 1.)[9600..]);
            assert!(p <= 10f64.powf(-0.1 / 20.) * 1.001 && p > 0.9, "{p}");
        }
    }

    #[test]
    fn true_peak_phases_pass_dc() {
        // BS.1770-4 Annex 2 phases sum to 1.0016 / 0.9730 (published taps).
        for p in &detector::PHASES {
            let sum: f32 = p.iter().map(|b| f32::from_bits(*b)).sum();
            assert!((sum - 1.).abs() < 0.03, "{sum}");
        }
    }
}
