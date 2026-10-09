//! Compressor against an independent whole-timeline recurrence, across block
//! partitions, at voice and bus scope. DSP_SYSTEM_INVENTORY "Subtype selection
//! and compressor linking" (linked detector); the level law is unverified.
use sampler_core::*;
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

fn settings(link: bool) -> CompressorSettings {
    CompressorSettings {
        threshold_db: Parameter::Constant(-20.),
        ratio: Parameter::Constant(4.),
        attack_seconds: Parameter::Constant(0.0005),
        release_seconds: Parameter::Constant(0.002),
        makeup: 1.5,
        link,
    }
}

fn reference(input: &[[f32; 2]], s: CompressorSettings) -> Vec<[f64; 2]> {
    let [threshold, ratio, attack, release] =
        [s.threshold_db, s.ratio, s.attack_seconds, s.release_seconds].map(|p| {
            let Parameter::Constant(v) = p else { panic!("reference constants"); }; v
        });
    let (a, r) = (
        (-1. / (attack * f64::from(RATE))).exp(),
        (-1. / (release * f64::from(RATE))).exp(),
    );
    let mut gr = [0f64; 2];
    input
        .iter()
        .map(|f| {
            let x = [f64::from(f[0]), f64::from(f[1])];
            let det = if s.link {
                [((x[0] + x[1]) / 2.).abs(); 2]
            } else {
                [x[0].abs(), x[1].abs()]
            };
            let mut out = [0.; 2];
            for c in 0..2 {
                let c_in = if s.link { 0 } else { c };
                let level = 20. * det[c_in].max(1e-300).log10();
                let target = ((level - threshold) * (1. - 1. / ratio)).max(0.);
                let k = if target > gr[c_in] { a } else { r };
                if c == c_in {
                    gr[c] = target + k * (gr[c] - target);
                }
                out[c] = x[c] * s.makeup * 10f64.powf(-gr[c_in] / 20.);
            }
            out
        })
        .collect()
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

fn close(actual: &[[f32; 2]], expected: &[[f64; 2]]) {
    for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
        for c in 0..2 {
            assert!(
                (f64::from(a[c]) - e[c]).abs() < 3e-7,
                "frame {i}: {a:?} != {e:?}"
            );
        }
    }
}

#[test]
fn voice_and_bus_compressors_follow_the_recurrence_across_block_sizes() {
    for link in [true, false] {
        let expected = reference(&source(), settings(link));
        let stage = || Processor::Compressor(settings(link));
        for block in [1, 7, 64, 129, 300] {
            let voice = plan()
                .with_voice_chains(
                    vec![VoiceChain::new(vec![stage()], vec![], 0).unwrap()],
                    vec![Some(0)],
                )
                .unwrap();
            close(&run(voice, block), &expected);
            let bus = plan()
                .with_buses(
                    vec![Bus {
                        processors: vec![stage()],
                        sends: vec![BusSend {
                            bus: None,
                            gain: 1.,
                        }],
                        tail_frames: 0,
                    }],
                    vec![Some(0)],
                )
                .unwrap();
            close(&run(bus, block), &expected);
        }
    }
}

#[test]
fn invalid_compressor_control_domains_are_rejected_before_rendering() {
    for (field, low, high) in [(0, 0., 7000.), (1, 0.5, 4.), (2, -0.01, 1.), (3, 0., -0.01)] {
        let mut s = settings(true);
        let parameter = Parameter::Control(ControlRange {
            control: ControlId(1),
            low,
            high,
            ramp_frames: 0,
        });
        match field {
            0 => s.threshold_db = parameter,
            1 => s.ratio = parameter,
            2 => s.attack_seconds = parameter,
            _ => s.release_seconds = parameter,
        }
        let result = VoiceChain::new(vec![Processor::Compressor(s)], vec![], 0);
        assert!(matches!(result, Err(Error::InvalidInput)), "field {field}");
    }
}

#[test]
fn held_compressor_controls_preserve_the_existing_recurrence() {
    for link in [true, false] {
        let original = settings(link);
        let expected = reference(&source(), original);
        let mut controlled = original;
        let defaults = [
            original.threshold_db,
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
        controlled.threshold_db = threshold;
        controlled.ratio = ratio;
        controlled.attack_seconds = attack;
        controlled.release_seconds = release;
        for block in [1, 7, 64, 129, 300] {
            let prepared = plan()
                .with_controls(definitions.clone())
                .unwrap()
                .with_voice_chains(
                    vec![
                        VoiceChain::new(vec![Processor::Compressor(controlled)], vec![], 0)
                            .unwrap(),
                    ],
                    vec![Some(0)],
                )
                .unwrap();
            close(&run(prepared, block), &expected);
        }
    }
}
