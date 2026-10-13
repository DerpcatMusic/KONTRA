use super::super::BLOCK;
use super::*;
use sampler_ir::fx::eq::{Band, Config};

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
    assert!((response_db(stage(band), 2619.29) - 20.).abs() < 0.2);
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

#[test]
fn static_lowering_makes_batchable_stages_and_expands_filter_orders() {
    let mut high = Band::new(Kind::HighPass, 100., 0.7, 0.);
    let mut count = |poles| {
        high.poles = poles;
        lower(&Config::band(high), RATE).unwrap().len()
    };
    assert_eq!([count(1), count(2), count(3), count(4)], [1, 1, 2, 2]);
    let mut cfg = Config::new(&[
        Band::new(Kind::Peak, 1000., 1., 12.),
        Band::new(Kind::Peak, 2000., 1., 0.), // flat: nothing
        Band { enabled: false, ..Band::new(Kind::Peak, 3000., 1., 6.) },
    ])
    .unwrap();
    cfg.output_gain_db = 6.0206;
    let stages = lower(&cfg, RATE).unwrap();
    assert_eq!(stages.len(), 2);
    let Processor::Biquad(b) = stages[0] else { panic!("fixed bell is a plain biquad") };
    assert!((20. * b.magnitude(1000.).log10() - 12.).abs() < 1e-9);
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
    let mags: Vec<f64> = lower(&cfg, RATE)
        .unwrap()
        .iter()
        .map(|s| match s {
            Processor::Biquad(b) => 20. * b.magnitude(20.).log10(),
            _ => unreachable!(),
        })
        .collect();
    assert!(mags[0] > 5.9 && mags[1].abs() < 0.1);
}
