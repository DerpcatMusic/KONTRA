use super::super::BLOCK;
use super::*;
use sampler_ir::fx::eq::{Band, Config, SolidSection};

const RATE: u32 = 48_000;

fn stage(band: EqBand) -> Stage {
    band.compile(RATE, &mut Vec::new()).unwrap()
}

/// Steady-state gain in dB of `stage` on a sine at `hz` (left channel).
fn response_db(stage: Stage, hz: f64) -> f64 {
    let mut state = ProcessorState::default();
    let (mut num, mut den) = (0., 0.);
    for block in 0..96 {
        let mut b: Planar = std::array::from_fn(|_| {
            std::array::from_fn(|i| {
                ((block * BLOCK + i) as f64 * std::f64::consts::TAU * hz / f64::from(RATE)).sin()
            })
        });
        let input = b[0];
        stage.process(&mut state, &[], &mut b, BLOCK, (block * BLOCK) as u64);
        if block >= 48 {
            num += b[0].iter().map(|v| v * v).sum::<f64>();
            den += input.iter().map(|v| v * v).sum::<f64>();
        }
    }
    10. * (num / den).log10()
}

#[test]
fn generic_band_shelf_level_and_enable_follow_their_parameters() {
    let c = Parameter::Constant;
    let low = |gain, reference, enabled| EqBand {
        gain_db: c(gain),
        reference_db: c(reference),
        enabled: c(enabled),
        ..EqBand::rbj(Kind::LowShelf, 1000., 0.707, 0.)
    };
    assert!((response_db(stage(low(12., 0., 1.)), 20.) - 12.).abs() < 0.1);
    // Relative gain: 12 dB above a 12 dB reference is flat.
    assert!(response_db(stage(low(12., 12., 1.)), 20.).abs() < 1e-6);
    assert_eq!(response_db(stage(low(12., 0., 0.)), 20.), 0.);
    let level = EqBand { gain_db: c(6.0206), ..EqBand::rbj(Kind::Level, 1000., 0.707, 0.) };
    assert!((response_db(stage(level), 1000.) - 6.0206).abs() < 1e-3);
}

#[test]
fn proportional_q_narrows_a_bell_as_gain_grows() {
    let bell = |gain, prop| {
        let band = EqBand { proportional_q: prop, ..EqBand::rbj(Kind::Peak, 1000., 1., gain) };
        let Stage::Generic(g) = stage(band) else { unreachable!() };
        g.targets(&[], 0).1
    };
    assert_eq!(bell(12., 0.), 1.);
    assert!((bell(12., 1.) - 2.).abs() < 1e-12);
    assert!((bell(-6., 1.) - 1.5).abs() < 1e-12);
}

#[test]
fn solid_knobs_run_through_the_spline_tables() {
    let c = Parameter::Constant;
    // HMF bell, frequency knob .5 (2619.29 Hz), gain knob 1 (+20 dB), Q knob .5 (q = .5/.4).
    let band = EqBand {
        law: Law::Solid(SolidSection::HighMid),
        frequency: c(0.5),
        q: c(0.5),
        gain_db: c(1.),
        ..EqBand::rbj(Kind::Peak, 0., 0., 0.)
    };
    // Native: +20 dB x C_7628(.5) x C_7A58(.5) at 2619.29 Hz x C_7E88(1).
    assert!((response_db(stage(band), 2619.29) - 20. * 1.0165 * 0.9945).abs() < 0.01);
    // Knob .5 is the 0 dB knot of every gain table.
    let flat = EqBand { gain_db: c(0.5), ..band };
    assert_eq!(response_db(stage(flat), 2619.29), 0.);
}

#[test]
fn native_legacy_law_is_the_same_recurrence_as_the_standalone_stage() {
    let c = Parameter::Constant;
    let band = EqBand {
        law: Law::KontaktLegacy,
        frequency: c(0.5),
        q: c(0.5),
        gain_db: c(9.),
        ..EqBand::rbj(Kind::Peak, 0., 0., 0.)
    };
    let direct = KontaktLegacyEqSettings { frequency: c(0.5), bandwidth: c(0.5), gain_db: c(9.) }
        .compile(RATE, &mut Vec::new())
        .unwrap();
    let a = stage(band);
    let (mut sa, mut sb) = (ProcessorState::default(), ProcessorState::default());
    for block in 0..4 {
        let mut x: Planar = std::array::from_fn(|ch| {
            std::array::from_fn(|i| f64::from(((i + ch * 7 + block * 3) as f32 * 0.21).sin() * 0.3))
        });
        let mut y = x;
        a.process(&mut sa, &[], &mut x, 37, 0);
        direct.process(&mut sb, &[], &mut y, 37, 0);
        assert_eq!(x.map(|c| c.map(f64::to_bits)), y.map(|c| c.map(f64::to_bits)));
    }
}

#[test]
fn invalid_bands_are_rejected_before_rendering() {
    let c = Parameter::Constant;
    let legacy = |frequency, gain| EqBand {
        law: Law::KontaktLegacy,
        frequency: c(frequency),
        q: c(0.5),
        gain_db: c(gain),
        ..EqBand::rbj(Kind::Peak, 0., 0., 0.)
    };
    assert!(legacy(1.2, 0.).compile(RATE, &mut Vec::new()).is_err());
    assert!(legacy(0.5, 19.).compile(RATE, &mut Vec::new()).is_err());
    assert!(EqBand { kind: Kind::LowShelf, ..legacy(0.5, 0.) }.compile(RATE, &mut Vec::new()).is_err());
    assert!(EqBand::rbj(Kind::Peak, f64::NAN, 1., 0.).compile(RATE, &mut Vec::new()).is_err());
    assert!(EqBand::rbj(Kind::Peak, 1000., 1., 0.).compile(0, &mut Vec::new()).is_err());
}

/// Steady-state dB of the lowered stages in series at `hz`.
fn chain_db(stages: &[Processor], hz: f64) -> f64 {
    let compiled: Vec<Stage> = stages
        .iter()
        .filter_map(|p| match p {
            Processor::Eq(band) => Some(stage(*band)),
            _ => None,
        })
        .collect();
    let mut states = vec![ProcessorState::default(); compiled.len()];
    let (mut num, mut den) = (0., 0.);
    for block in 0..96 {
        let mut b: Planar = std::array::from_fn(|_| {
            std::array::from_fn(|i| {
                ((block * BLOCK + i) as f64 * std::f64::consts::TAU * hz / f64::from(RATE)).sin()
            })
        });
        let input = b[0];
        for (s, st) in compiled.iter().zip(&mut states) {
            s.process(st, &[], &mut b, BLOCK, (block * BLOCK) as u64);
        }
        if block >= 48 {
            num += b[0].iter().map(|v| v * v).sum::<f64>();
            den += input.iter().map(|v| v * v).sum::<f64>();
        }
    }
    10. * (num / den).log10()
}

#[test]
fn static_lowering_is_one_stage_type_and_expands_filter_orders() {
    let mut high = Band::new(Kind::HighPass, 100., 0.7, 0.);
    let mut count = |poles| {
        high.poles = poles;
        lower(&Config::band(high)).len()
    };
    assert_eq!([count(1), count(2), count(3), count(4)], [1, 1, 2, 2]);
    let mut cfg = Config::new(&[
        Band::new(Kind::Peak, 1000., 1., 12.),
        Band::new(Kind::Peak, 2000., 1., 0.), // flat: nothing
        Band { enabled: false, ..Band::new(Kind::Peak, 3000., 1., 6.) },
    ])
    .unwrap();
    cfg.output_gain_db = 6.0206;
    let stages = lower(&cfg);
    assert_eq!(stages.len(), 2);
    assert!(matches!(stages[0], Processor::Eq(_)));
    assert!((chain_db(&stages[..1], 1000.) - 12.).abs() < 0.05);
    assert!(matches!(stages[1], Processor::Gain(g) if (g - 2.).abs() < 1e-3));
}

#[test]
fn butterworth_section_qs() {
    assert!((butterworth_q(4, 1) - 1.306_562_96).abs() < 1e-8);
    assert!((butterworth_q(4, 2) - 0.541_196_10).abs() < 1e-8);
    assert!((butterworth_q(3, 1) - 1.).abs() < 1e-12);
}

#[test]
fn multi_band_configs_keep_their_order() {
    let cfg = Config::new(&[
        Band::new(Kind::LowShelf, 100., 0.7, 6.),
        Band::new(Kind::HighShelf, 8000., 0.7, -6.),
    ])
    .unwrap();
    let stages = lower(&cfg);
    assert_eq!(stages.len(), 2);
    assert!((chain_db(&stages[..1], 20.) - 6.).abs() < 0.2 && chain_db(&stages[1..], 20.).abs() < 0.1);
}

/// One magnitude table for the one kernel: every kind, at 44.1/48/96 kHz, against its analogue
/// prototype up to 20 kHz (and below 0.45 of the rate). The design matches DC, the band
/// frequency and Nyquist exactly; between them the error is the table's worst case.
#[test]
fn every_kind_matches_its_analogue_prototype() {
    let kinds = [
        (Kind::Peak, 12.), (Kind::Peak, -12.),
        (Kind::LowShelf, 12.), (Kind::LowShelf, -12.),
        (Kind::HighShelf, 12.), (Kind::HighShelf, -12.),
        (Kind::LowPass, 0.), (Kind::HighPass, 0.), (Kind::BandPass, 0.), (Kind::Notch, 0.),
        (Kind::FirstOrderLowShelf, 12.), (Kind::FirstOrderHighShelf, -12.),
    ];
    let mut worst: Vec<(Kind, f64, f64)> = Vec::new();
    for (kind, boost) in kinds {
        let mut max = (0f64, 0f64);
        let mut at_case = (0u32, 0f64, 0f64);
        let mut baseline = 0f64;
        for rate in [44_100u32, 48_000, 96_000] {
            for hz in [60., 500., 3_000., 9_000.] {
                for q in [0.5, 0.707, 2.] {
                    let f = if matches!(kind, Kind::FirstOrderLowShelf | Kind::FirstOrderHighShelf) {
                        kernel::first_order(kind, rate, hz, boost)
                    } else {
                        kernel::design(kind, rate, hz, q, boost)
                    }
                    .expect("stable design");
                    let mut probe = 20.;
                    while probe < 20_000f64.min(0.45 * f64::from(rate)) {
                        let want = kernel::analog_db(kind, hz, q, boost, probe);
                        let got = 20. * kernel::magnitude(&f, rate, probe).log10();
                        // Notch and band-pass zeros: compare only where the analogue is above -30 dB
                        // (a 3-point match does not fix stop-band depth).
                        if want > -30. {
                            let e = (got - want).abs();
                            if kind == Kind::Peak {
                                // Baseline: the RBJ bilinear bell the old generic stage used.
                                let (a, w0) = (10f64.powf(boost / 40.), std::f64::consts::TAU * hz / f64::from(rate));
                                let al = w0.sin() / (2. * q);
                                let c = -2. * w0.cos();
                                let rbj = kernel::Biquad {
                                    b: [(1. + al * a) / (1. + al / a), c / (1. + al / a), (1. - al * a) / (1. + al / a)],
                                    a: [c / (1. + al / a), (1. - al / a) / (1. + al / a)],
                                };
                                let b = (20. * kernel::magnitude(&rbj, rate, probe).log10() - want).abs();
                                if b > baseline {
                                    baseline = b;
                                }
                            }
                            if e > max.0 {
                                max = (e, probe);
                                at_case = (rate, hz, q);
                            }
                        }
                        probe *= 1.07;
                    }
                }
            }
        }
        worst.push((kind, max.0, max.1));
        eprintln!("{kind:?} boost {boost} worst case {at_case:?} (RBJ bilinear baseline {baseline:.3} dB)");
    }
    for (kind, error, at) in &worst {
        eprintln!("{kind:?}: max |error| {error:.4} dB at {at:.0} Hz");
    }
    // The worst cases are resonant (Q 2) shelves with the band at 0.2 fs, where three matched
    // points cannot follow the analogue overshoot; every other kind is within about 1.7 dB there
    // and the bilinear RBJ bell it replaces is 5.3 dB off in the same corner.
    for (kind, error, at) in &worst {
        assert!(*error < 3.5, "{kind:?} {error} dB at {at} Hz");
    }
}

/// UVI ParametricEQ coefficient routines run under emulation at 48 kHz: peak 0x1416403e0 (1 kHz,
/// +6 dB, Q 0.7), low pass 0x14163fd30 (16 kHz, Q 0.7), high pass 0x14163fe30 (30 Hz, Q 2) and high
/// shelf 0x141640220 (7.4 kHz, -4 dB at A = 10^(dB/20), slope 1: cookbook -8 dB, Q 1/sqrt 2).
#[test]
fn cookbook_is_the_uvi_parametric_eq_law() {
    let cases = [
        (Kind::Peak, 1000., 0.7, 6., [1.0616237889328701, -1.8601148764443352, 0.814541945454859, -1.8601148764443352, 0.8761657343877289]),
        (Kind::LowPass, 16000., 0.7, 0., [0.46336638514573497, 0.9267327702914699, 0.46336638514573497, 0.6178218468609797, 0.2356436937219601]),
        (Kind::HighPass, 30., 2., 0., [0.9990153661693021, -1.9980307323386042, 0.9990153661693021, -1.998023029282513, 0.9980384353946954]),
        (Kind::HighShelf, 7400., std::f64::consts::FRAC_1_SQRT_2, -8., [0.5355857724005818, -0.25310761134475296, 0.11318659779714484, -0.9348776648168456, 0.33054242366981945]),
    ];
    for (kind, hz, q, db, native) in cases {
        let f = kernel::cookbook(kind, 48_000, hz, q, db).unwrap();
        let got = [f.b[0], f.b[1], f.b[2], f.a[0], f.a[1]];
        for (g, n) in got.iter().zip(native) {
            assert!((g - n).abs() < 1e-12, "{kind:?} {got:?} vs {native:?}");
        }
    }
}

/// UVI DigitalEq bands (design 0x1412840f0 run under emulation, band at 0.03 of the rate): the
/// cascade of `digital::section`s has the native magnitude in dB at 0.005, 0.025, 0.031, 0.04 and
/// 0.2 of the rate. Cases: (kind, order, Q, bandwidth octaves, gain dB, native dB).
#[test]
fn uvi_digital_bands_match_the_native_design() {
    #[rustfmt::skip]
    let cases = [
        (Kind::LowPass, 16, 0.707, 1.0, 0.0, [-0.0, -0.012463, -5.882805, -40.302723, -283.424431]),
        (Kind::LowPass, 3, 2.0, 1.0, 0.0, [0.109116, 5.141362, 5.439655, -3.698343, -53.07619]),
        (Kind::HighPass, 6, 0.3, 1.0, 0.0, [-93.529325, -15.724999, -9.613235, -4.199574, -0.000192]),
        (Kind::HighPass, 1, 0.707, 1.0, 0.0, [-15.706399, -3.878554, -2.869384, -1.930968, -0.072901]),
        (Kind::BandPass, 8, 0.707, 0.5, 0.0, [-196.043872, -5.397914, -0.0, -36.405667, -213.8112]),
        (Kind::BandPass, 3, 3.0, 2.0, 0.0, [-35.173084, 0.269756, 0.007962, 0.777403, -41.960655]),
        (Kind::Notch, 4, 1.5, 1.0, 0.0, [0.001441, -22.139856, -82.397559, -3.252279, 0.000518]),
        (Kind::LowShelf, 12, 0.707, 1.0, 9.0, [9.0, 8.870528, 2.926321, 0.010227, 0.0]),
        (Kind::LowShelf, 1, 0.707, 1.0, -6.0, [-5.826741, -3.523617, -2.904812, -2.183326, -0.107571]),
        (Kind::HighShelf, 6, 2.5, 1.0, -9.0, [0.000182, 3.758496, -8.714174, -10.76938, -9.000043]),
        (Kind::Peak, 2, 0.707, 0.4, 6.0, [3.7e-05, 1.514287, 5.979415, 0.305949, 1.4e-05]),
        (Kind::Peak, 16, 0.4, 2.0, -12.0, [-0.0, -12.0, -12.0, -11.999992, -0.0]),
        (Kind::Peak, 3, 5.0, 1.0, 9.0, [-0.045827, 10.640866, 9.026842, 19.062124, -0.026992]),
        (Kind::HighShelf, 3, 0.2, 1.0, 6.0, [0.507624, 2.700421, 3.054053, 3.47268, 5.662092]),
    ];
    for (kind, order, q, bw, gain, native) in cases {
        let count = digital::sections(kind, order);
        assert!(digital::section(kind, order, 0.03, gain, q, bw, count).is_none());
        for (probe, want) in [0.005, 0.025, 0.031, 0.04, 0.2].into_iter().zip(native) {
            let db: f64 = (0..count)
                .map(|i| {
                    let f = digital::section(kind, order, 0.03, gain, q, bw, i).unwrap();
                    20. * kernel::magnitude(&f, RATE, probe * f64::from(RATE)).log10()
                })
                .sum();
            let close = if want < -150. { db < -140. } else { (db - want).abs() < 2e-3 };
            assert!(close, "{kind:?} order {order} at {probe}: {db} vs native {want}");
        }
    }
}
