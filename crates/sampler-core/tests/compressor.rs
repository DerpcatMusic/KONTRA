//! Dynamics stage through the runtime: the output must not depend on the
//! render block size, at voice and bus scope, with held controls equal to
//! constants. The level laws themselves are tested in `dsp/dynamics/*`.
use sampler_core::*;
use sampler_ir::fx::dynamics::{Direction, Generic, Law, Link};
mod support;

fn limits() -> Limits {
    Limits {
        notes: 4,
        channels: 0,
        performances: 1,
        families: 8,
        voices: 8,
        expressions: 4,
        decisions: 0,
        commands: 8,
        behaviors: 0,
        behavior_cells: 0,
        behavior_fuel: 0,
        note_cells: 0,
    }
}

const FRAMES: usize = 300;
const RATE: u32 = 48000;

fn source() -> Vec<[f32; 2]> {
    // A loud/quiet pattern with a polarity-opposed stretch, so link matters.
    (0..FRAMES)
        .map(|i| {
            let x = ((i * 37 % 101) as f32 / 50. - 1.) * if i < 150 { 0.9 } else { 0.05 };
            if (200..250).contains(&i) {
                [x, -x]
            } else {
                [x, x * 0.5 + 0.01]
            }
        })
        .collect()
}

fn settings(link: Link) -> DynamicsSettings {
    DynamicsSettings {
        law: Law::Generic(Generic {
            threshold_db: -20.,
            ratio: 4.,
            knee_db: 6.,
            direction: Direction::Downward,
            detector: sampler_ir::fx::dynamics::Detector::Peak,
            topology: sampler_ir::fx::dynamics::Topology::FeedForward,
            lookahead: sampler_ir::Time::Seconds(0.),
            sidechain_highpass_hz: 0.,
            range_db: 0.,
        }),
        link,
        makeup: 1.5,
        mix: 1.,
        threshold: Parameter::Constant(-20.),
        ratio: Parameter::Constant(4.),
        attack_seconds: Parameter::Constant(0.0005),
        release_seconds: Parameter::Constant(0.002),
    }
}

fn plan() -> Prepared {
    Prepared::new(
        RATE,
        vec![Pcm::new(RATE, source().into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::default(),
            playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
}

fn run(prepared: Prepared, block: usize) -> Vec<[f32; 2]> {
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut audio = vec![[0f32; 2]; FRAMES];
    support::without_heap(|| {
        rt.trigger(
            Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(1),
            },
            60,
            1.,
        )
        .unwrap();
        for chunk in audio.chunks_mut(block) {
            rt.render(chunk).unwrap();
        }
    });
    audio
}

fn voice(s: DynamicsSettings) -> Prepared {
    plan()
        .with_voice_chains(
            vec![VoiceChain::new(vec![Processor::Dynamics(s)], vec![], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap()
}

fn bus(s: DynamicsSettings) -> Prepared {
    plan()
        .with_buses(
            vec![Bus {
                processors: vec![Processor::Dynamics(s)],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 0,
            }],
            vec![Some(0)],
        )
        .unwrap()
}

fn close(actual: &[[f32; 2]], expected: &[[f32; 2]]) {
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        for c in 0..2 {
            assert!((a[c] - e[c]).abs() < 3e-7, "frame {i}: {a:?} != {e:?}");
        }
    }
}

#[test]
fn voice_and_bus_dynamics_are_independent_of_block_size() {
    for link in [Link::Mean, Link::Max, Link::Off] {
        let s = settings(link);
        let expected = run(voice(s), FRAMES);
        let source = source();
        // The stage acts: it is not the identity.
        assert!(
            expected
                .iter()
                .zip(&source)
                .any(|(a, b)| (a[0] - b[0]).abs() > 1e-3)
        );
        for block in [1, 7, 64, 129, 300] {
            close(&run(voice(s), block), &expected);
            close(&run(bus(s), block), &expected);
        }
    }
}

#[test]
fn invalid_dynamics_control_domains_are_rejected_before_rendering() {
    for (field, low, high) in [(0, 0., 7000.), (1, 0.5, 4.), (2, -0.01, 1.), (3, 0., -0.01)] {
        let mut s = settings(Link::Mean);
        let parameter = Parameter::Control(ControlRange {
            control: ControlId(1),
            low,
            high,
            ramp_frames: 0,
        });
        match field {
            0 => s.threshold = parameter,
            1 => s.ratio = parameter,
            2 => s.attack_seconds = parameter,
            _ => s.release_seconds = parameter,
        }
        let result = VoiceChain::new(vec![Processor::Dynamics(s)], vec![], 0);
        assert!(matches!(result, Err(Error::InvalidInput)), "field {field}");
    }
}

#[test]
fn held_dynamics_controls_preserve_the_constant_result() {
    for link in [Link::Mean, Link::Off] {
        let original = settings(link);
        let expected = run(voice(original), FRAMES);
        let mut controlled = original;
        let defaults = [
            original.threshold,
            original.ratio,
            original.attack_seconds,
            original.release_seconds,
        ];
        let ranges = [(-60., 0.), (1., 20.), (0., 1.), (0., 5.)];
        let definitions = defaults
            .into_iter()
            .zip(ranges)
            .enumerate()
            .map(|(n, (p, (min, max)))| {
                let Parameter::Constant(default) = p else {
                    unreachable!()
                };
                ControlDefinition {
                    id: ControlId(n as u128 + 1),
                    domain: ControlDomain::Real { min, max },
                    default: ControlValue::Real(default),
                }
            })
            .collect::<Vec<_>>();
        let [threshold, ratio, attack, release] = std::array::from_fn(|n| {
            Parameter::Control(ControlRange {
                control: definitions[n].id,
                low: ranges[n].0,
                high: ranges[n].1,
                ramp_frames: 480,
            })
        });
        controlled.threshold = threshold;
        controlled.ratio = ratio;
        controlled.attack_seconds = attack;
        controlled.release_seconds = release;
        for block in [1, 7, 64, 129, 300] {
            let prepared = plan()
                .with_controls(definitions.clone())
                .unwrap()
                .with_voice_chains(
                    vec![VoiceChain::new(vec![Processor::Dynamics(controlled)], vec![], 0).unwrap()],
                    vec![Some(0)],
                )
                .unwrap();
            close(&run(prepared, block), &expected);
        }
    }
}
