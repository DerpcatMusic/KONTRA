//! Authored/prepared source rejection plus actual Engine PCM and note ending.
//! Staged only: execution belongs to the central integration owner.
use crate::{audio::Sample, engine::{Bank, Engine, envelope::Ahdsr, instrument::ControllerPolicy},
    fx::{Chain, Effect, Kind, Params, params::{Filter, StereoModeller}},
    import::{Group, Zone}};

fn group() -> Group {
    Group { native_volume_env: true,
        volume_env: Some(crate::import::Ahdsr { attack_curve: 1., attack_ms: 0., hold_ms: 0.,
            decay_ms: 0., sustain: 0.25, release_ms: 0., unknown_flag: 0, unknown_tail: vec![] }),
        ..Default::default() }
}
fn bank(group: Group, frames: Vec<[f32; 2]>) -> anyhow::Result<Bank> {
    let path = std::path::PathBuf::from("finite-envelope-source");
    Bank::from_samples(vec![group], vec![Zone { sample: path.clone(), ..Default::default() }],
        vec![(path, Sample { rate: 48_000, frames })])
}
fn engine(mut bank: Bank, lanes: bool) -> Engine {
    bank.initial_state.as_mut().unwrap().controller_policy = ControllerPolicy::Native;
    let mut engine = Engine::default(); engine.reset(48_000.); engine.set_lanes(lanes);
    engine.set_bank(Some(Box::new(bank))); engine
}
fn mixer(gain: f32) -> Chain {
    Chain { slots: vec![Effect { slot: 0, kind: Kind::StereoModeller, version: 0,
        bypass: false, output_gain: gain, dry_level: 0.,
        params: Params::StereoModeller(StereoModeller { spread: 0., pan: 0., pseudo_stereo: false }) }] }
}

#[test]
fn rejected_authored_and_prepared_envelopes_repair_to_complete_pcm_and_exact_end() {
    const OFF: usize = 128;
    const END: usize = OFF + 32;
    const N: usize = 384;
    let source: Vec<_> = (0..4096).map(|i| [((i * 13 % 31) as f32 - 15.) / 128.; 2]).collect();
    let path = std::path::PathBuf::from("finite-envelope-source");
    let error = Bank::from_samples(vec![group()], vec![Zone { sample: path.clone(), ..Default::default() }],
        vec![(path, Sample { rate: 0, frames: source.clone() })]).err()
        .expect("zero source rate must reject before installation");
    assert!(error.to_string().contains("Invalid sample rate in finite-envelope-source"), "{error:#}");
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut corrupt = source.clone();
        corrupt[73][1] = bad;
        let error = bank(group(), corrupt).err().expect("supplied PCM must reject before packing");
        assert!(error.to_string().contains("Nonfinite PCM") &&
            error.to_string().contains("frame 73, channel 1"), "{error:#}");
        let mut authored = group(); authored.volume_env.as_mut().unwrap().attack_curve = bad;
        let error = bank(authored, source.clone()).err().expect("authored source must reject before installation");
        assert!(format!("{error:#}").contains("envelope"), "{error:#}");
        let corrupt_fx = Group { fx: mixer(bad), ..group() };
        let error = bank(corrupt_fx.clone(), source.clone()).err().expect("active FX gain must reject before installation");
        assert!(format!("{error:#}").contains("group FX"), "{error:#}");
        let error = bank(group(), source.clone()).unwrap().with_prepared_settings(|rows| {
            rows[0].filter = crate::engine::GroupSettings::from(&corrupt_fx).filter;
        }).err().expect("prepared active FX must reject before installation");
        assert!(format!("{error:#}").contains("group FX"), "{error:#}");
        let error = bank(group(), source.clone()).unwrap().with_prepared_settings(|rows| {
            rows[0].envelope.as_mut().unwrap().sustain = bad;
        }).err().expect("prepared source must reject before installation");
        assert!(format!("{error:#}").contains("envelope"), "{error:#}");
        let error = bank(Group::default(), source.clone()).unwrap().with_prepared_settings(|rows| {
            rows[0].flex = Some(super::Flex { points: vec![super::FlexPoint {
                seconds: 0.001, level: 0.5, curve: bad }].into_boxed_slice(),
                sustain: 0, one_shot: false, loop_start: None });
        }).err().expect("prepared Flex source must reject before installation");
        assert!(format!("{error:#}").contains("envelope"), "{error:#}");
    }
    for target in [crate::import::ModTarget::Pitch,
        crate::import::ModTarget::Module { param: "filterCutoff".into(), slot: 0 }] {
        let mut envelope = group().volume_env.unwrap(); envelope.attack_curve = f32::NAN;
        let authored = Group { envelopes: vec![crate::modulation::ModEnvelope {
            native_ahd_only: false, env: envelope, targets: vec![crate::import::ModAssignment {
                cc_default: None, name: "Authored source".into(), source: crate::import::ModSource::Unassigned,
                target, intensity: 0.5, invert: false, lag_ms: 0, shaper: None }] }],
            fx: crate::fx::Chain { slots: vec![crate::fx::Effect { slot: 0, kind: crate::fx::Kind::Filter,
                version: 0, bypass: false, output_gain: 1., dry_level: 0.,
                params: crate::fx::Params::Filter(crate::fx::params::Filter { filter_type: 2,
                    cutoff: 0.3, resonance: 0., extra: [0.; 3], native_flag: None }) }] },
            ..Default::default() };
        let error = bank(authored, source.clone()).err().expect("routed source must reject before installation");
        assert!(format!("{error:#}").contains("envelope"), "{error:#}");
    }
    for finite in [f32::MIN, f32::MAX] {
        assert!(bank(group(), source.clone()).unwrap().with_prepared_settings(|rows| {
            rows[0].envelope.as_mut().unwrap().attack = finite;
        }).is_ok(), "finite legacy timing is not a new range restriction");
    }
    let mut authored = group();
    authored.volume_env.as_mut().unwrap().attack_curve = 0.;
    authored.volume_env.as_mut().unwrap().sustain = 1.;
    authored.volume_env.as_mut().unwrap().release_ms = f32::INFINITY;
    assert!(bank(authored, source.clone()).is_err(), "authored infinity is not the internal UNITY sentinel");
    assert!(bank(group(), source.clone()).unwrap().with_prepared_settings(|rows| {
        rows[0].envelope = Some(Ahdsr { sustain: 0.25, ..Ahdsr::UNITY });
    }).is_err(), "the prepared exception requires the complete canonical UNITY value");
    assert!(bank(group(), source.clone()).unwrap().with_prepared_settings(|rows| {
        let envelope = rows[0].envelope.as_mut().unwrap();
        envelope.attack = 0.01;
        envelope.curve = -10.;
    }).is_err(), "finite native curves must remain inside the saved source domain");
    // The independently authored source contains the expected target curve:
    // sustain1/4, immediate logical release at128, then the old target endpoint
    // interpolates to0 over32 frames. Its physical EOF is the expected end.
    let reference: Vec<_> = source[..END].iter().enumerate().map(|(at, frame)| {
        let gain = if at < OFF { 0.25 } else { 0.25 * (1. - (at - OFF) as f32 / 32.) };
        // This short physical reference gets its own last48-frame EOF fade.
        // Compensate its source, preserving the independent physical END.
        let eof_gain = ((END - at) as f32 / 48.).min(1.);
        frame.map(|v| v * (gain / eof_gain))
    }).collect();
    for (lanes, gain) in [(false, 1.), (true, 1.), (false, 0.5), (true, 0.5)] {
        // The same supplied PCM repairs its rate to 48 kHz in bank(), then runs
        // through the actual source/envelope clocks and exact retirement below.
        let mut repaired_group = group();
        if gain != 1. { repaired_group.fx = mixer(gain); }
        let repaired = bank(repaired_group, source.clone()).unwrap().with_prepared_settings(|rows| {
            rows[0].envelope = Some(Ahdsr { sustain: 0.25, release: 0., ..Ahdsr::UNITY });
        }).unwrap();
        let dry = bank(Group::default(), reference.iter().map(|frame| frame.map(|v| v * gain)).collect())
            .unwrap().with_prepared_settings(|rows| {
            rows[0].envelope = Some(Ahdsr::UNITY);
        }).unwrap();
        let mut actual = engine(repaired, lanes); let mut oracle = engine(dry, lanes);
        let (mut a, mut b) = ([[0.; N]; 2], [[0.; N]; 2]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
            let mut start = 0;
            for end in [OFF, END, END + 1, N] {
                let mut part = 0;
                while start < end {
                    let n = [1, 7, 31, 127][part % 4].min(end - start);
                    let [left, right] = &mut a;
                    actual.render_checked(&mut left[start..start + n], &mut right[start..start + n]).unwrap();
                    let [left, right] = &mut b;
                    oracle.render_checked(&mut left[start..start + n], &mut right[start..start + n]).unwrap();
                    start += n; part += 1;
                }
                assert_eq!(actual.active_voices(), usize::from(end <= END),
                    "ordinary Native retirement observes its zero endpoint on the next emitted frame");
                assert_eq!(oracle.active_voices(), usize::from(end < END));
                if end == OFF { actual.note_off(0, 60); }
            }
        }), 0);
        let error = a.iter().flatten().zip(b.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0., f32::max);
        assert!(error < 2e-6, "repaired input complete stereo PCM lanes{lanes} gain{gain}: {error}");
        assert!(a.iter().flatten().all(|x| x.is_finite()));
        assert!(a.iter().any(|channel| channel[..END].iter().any(|x| x.abs() > 0.001)));
        assert!(a.iter().all(|channel| channel[END..].iter().all(|&x| x == 0.)));
    }
}

#[test]
fn unused_filter_metadata_rejects_incompatible_live_type_without_changing_complete_pcm() {
    const OFF: usize = 128;
    const END: usize = OFF + 33;
    const N: usize = 384;
    // A DC component and low tone reach the authored low-pass's pass band.
    let source: Vec<_> = (0..4096).map(|i| [0.125 + 0.03125
        * (std::f32::consts::TAU * 110. * i as f32 / 48_000.).sin(); 2]).collect();
    let filtered = |extra| Group { fx: Chain { slots: vec![Effect { slot: 0,
        kind: Kind::Filter, version: 0, bypass: false, output_gain: 0.5, dry_level: 0.,
        params: Params::Filter(Filter { filter_type: 2, cutoff: 0.3, resonance: 0.,
            extra, native_flag: None }) }] }, ..group() };
    let mut corrupt_active = filtered([0.; 3]);
    let Params::Filter(params) = &mut corrupt_active.fx.slots[0].params else { unreachable!() };
    params.cutoff = f32::NAN;
    assert!(bank(corrupt_active, source.clone()).is_err(), "consumed filter keys must reject");
    for lanes in [false, true] {
        let mut actual = engine(bank(filtered([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]), source.clone()).unwrap(), lanes);
        let mut oracle = engine(bank(filtered([0.; 3]), source.clone()).unwrap(), lanes);
        let (mut a, mut b) = ([[0.; N]; 2], [[0.; N]; 2]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            actual.note_on(0, 60, 127); oracle.note_on(0, 60, 127);
            let mut start = 0;
            for end in [73, OFF, END, N] {
                if start == 73 {
                    let filter = actual.state.settings[0].filter.as_mut().unwrap();
                    assert_eq!(filter.knob(0, super::super::filter::Knob::Type), Some(2.));
                    assert!(!filter.set_knob(0, super::super::filter::Knob::Type, 33.),
                        "Ladder would consume the previously unused corrupt Gain knob");
                    assert_eq!(filter.knob(0, super::super::filter::Knob::Type), Some(2.));
                }
                let mut part = 0;
                while start < end {
                    let n = [1, 7, 31, 127][part % 4].min(end - start);
                    let [left, right] = &mut a;
                    actual.render_checked(&mut left[start..start + n], &mut right[start..start + n]).unwrap();
                    let [left, right] = &mut b;
                    oracle.render_checked(&mut left[start..start + n], &mut right[start..start + n]).unwrap();
                    start += n; part += 1;
                }
                assert_eq!(actual.active_voices(), usize::from(end < END));
                assert_eq!(oracle.active_voices(), actual.active_voices());
                if end == OFF { actual.note_off(0, 60); oracle.note_off(0, 60); }
            }
        }), 0);
        // The independently authored control retains the old type throughout;
        // this is an atomic retarget regression, not a filter transfer oracle.
        assert_eq!(a, b, "rejected retarget changed complete stereo PCM lanes{lanes}");
        assert!(a.iter().flatten().all(|v| v.is_finite()));
        assert!(a.iter().flatten().any(|v| v.abs() > 0.001));
        assert!(a.iter().all(|channel| channel[END..].iter().all(|&v| v == 0.)));
    }
}

#[test]
fn nonfinite_live_expression_retains_complete_pcm_ends_and_valid_recovery() {
    use crate::engine::Expression;
    const OFF: usize = 320;
    const END: usize = OFF + 33;
    const N: usize = 512;
    let source: Vec<_> = (0..4096).map(|i| [((i * 13 % 31) as f32 - 15.) / 128.; 2]).collect();
    let initial = Expression { gain: 0.5, pan: -0.3, ..Default::default() };
    let recovery = Expression { gain: 0.75, tune: 12., pan: 0.4, ..Default::default() };
    for lanes in [false, true] {
        let (mut actual, mut oracle, mut unchanged) = (
            engine(bank(group(), source.clone()).unwrap(), lanes),
            engine(bank(group(), source.clone()).unwrap(), lanes),
            engine(bank(group(), source.clone()).unwrap(), lanes));
        let (mut a, mut b, mut old) = ([[0.; N]; 2], [[0.; N]; 2], [[0.; N]; 2]);
        assert_eq!(crate::plugin::tests::allocations(|| {
            for engine in [&mut actual, &mut oracle, &mut unchanged] {
                engine.set_expression_on(2, 60, initial);
                engine.note_on(2, 60, 127);
            }
            let mut start = 0;
            for end in [73, 149, 223, 257, OFF, END, N] {
                match start {
                    73 => actual.set_expression_on(2, 60, Expression { gain: f32::NAN, ..recovery }),
                    149 => actual.set_expression_on(2, 60, Expression { pan: f32::INFINITY, ..recovery }),
                    223 => actual.set_expression_on(2, 60, Expression { tune: f32::NEG_INFINITY, ..recovery }),
                    257 => {
                        actual.set_expression_on(2, 60, recovery);
                        oracle.set_expression_on(2, 60, recovery);
                    }
                    _ => {},
                }
                let mut part = 0;
                while start < end {
                    let n = [1, 7, 31, 127][part % 4].min(end - start);
                    for (engine, pcm) in [(&mut actual, &mut a), (&mut oracle, &mut b), (&mut unchanged, &mut old)] {
                        let [left, right] = pcm;
                        engine.render_checked(&mut left[start..start + n], &mut right[start..start + n]).unwrap();
                    }
                    start += n; part += 1;
                }
                for engine in [&mut actual, &mut oracle, &mut unchanged] {
                    assert_eq!(engine.active_voices(), usize::from(end < END));
                    if end == OFF { engine.note_off(2, 60); }
                }
            }
        }), 0);
        assert_eq!(a, b, "rejected expression changed complete stereo PCM or valid recovery lanes{lanes}");
        assert!(a.iter().flatten().all(|v| v.is_finite()));
        assert!(a.iter().zip(&old).any(|(new, old)| new[257..OFF].iter()
            .zip(&old[257..OFF]).any(|(new, old)| (new - old).abs() > 0.001)),
            "the later finite expression must change actual PCM");
        assert!(a.iter().all(|channel| channel[END..].iter().all(|&v| v == 0.)));
    }
}
