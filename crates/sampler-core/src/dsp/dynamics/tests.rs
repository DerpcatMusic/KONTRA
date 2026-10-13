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
    ("feedback", 0xce0595ad975b01d7),
    ("feedback hq off", 0xfa906825c5df7ba5),
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
    for ((name, h), (gn, gh)) in got.iter().zip(GOLDEN) {
        assert_eq!((name, h), (gn, gh));
    }
}
