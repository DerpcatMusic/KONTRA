//! Falcon rectifier and Formant Crusher decimator kernels against the spec
//! equations (DSP_FORMAT_SPECIFICATION, WaveShaper rectifier kernels and Formant
//! Crusher fractional decimation), across block partitions at voice and bus scope.
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

fn rectify(mode: Rectifier) -> Processor {
    Processor::Color(ColorSettings::curve(CurveSettings::Rectifier(mode)))
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

fn both_scopes(stage: Processor, expected: &[[f64; 2]]) {
    for block in [1, 7, 64, 129, 300] {
        let voice = plan()
            .with_voice_chains(
                vec![VoiceChain::new(vec![stage], vec![], 0).unwrap()],
                vec![Some(0)],
            )
            .unwrap();
        close(&run(voice, block), expected);
        let bus = plan()
            .with_buses(
                vec![Bus {
                    processors: vec![stage],
                    sends: vec![BusSend {
                        bus: None,
                        gain: 1.,
                    }],
                    tail_frames: 0,
                }],
                vec![Some(0)],
            )
            .unwrap();
        close(&run(bus, block), expected);
    }
}

#[test]
fn rectifiers_follow_their_branches() {
    let wide = |f: fn(f64) -> f64| -> Vec<[f64; 2]> {
        source()
            .iter()
            .map(|f2| [f(f64::from(f2[0])), f(f64::from(f2[1]))])
            .collect()
    };
    both_scopes(rectify(Rectifier::Full), &wide(f64::abs));
    both_scopes(rectify(Rectifier::Half), &wide(|x| x.max(0.)));
}

#[test]
fn decimator_holds_and_ramps_at_a_fractional_period() {
    let hertz = f64::from(RATE) / 6.6;
    let blend = 0.7f32;
    let fs = f64::from(RATE as f32);
    let period = 1.0 / f64::from((hertz / fs + hertz / fs) as f32);
    let mut phase = 0f64;
    let (mut current, mut held, mut delta) = ([0f32; 2], [0f32; 2], [0f32; 2]);
    let expected: Vec<[f64; 2]> = source()
        .iter()
        .map(|f| {
            let reload = phase <= 0.;
            let mut out = [0.; 2];
            for c in 0..2 {
                if reload {
                    held[c] = f[c];
                    delta[c] = (held[c] - current[c]) / period as f32;
                }
                let old = current[c];
                current[c] = old + delta[c];
                out[c] = f64::from((held[c] - old) * blend + old);
            }
            if reload {
                phase += period;
            }
            phase -= 1.;
            out
        })
        .collect();
    let decimator = Decimator { hertz: [hertz; 2], blend };
    both_scopes(Processor::Color(ColorSettings { decimator: Some(decimator), ..ColorSettings::IDENTITY }), &expected);
}

#[test]
fn batched_voices_rectify_like_scalar_voices() {
    let prepared = plan()
        .with_voice_chains(
            vec![VoiceChain::new(vec![rectify(Rectifier::Full)], vec![], 0).unwrap()],
            vec![Some(0)],
        )
        .unwrap();
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut audio = vec![[0f32; 2]; FRAMES];
    support::without_heap(|| {
        for (id, velocity) in [(1, 1.), (2, 0.5), (3, 0.25)] {
            let input = Input {
                protocol: Protocol::Clap,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id),
            };
            rt.trigger(input, 60, velocity).unwrap();
        }
        for chunk in audio.chunks_mut(37) {
            rt.render(chunk).unwrap();
        }
    });
    let expected: Vec<[f64; 2]> = source()
        .iter()
        .map(|f| [f64::from(f[0]).abs() * 1.75, f64::from(f[1]).abs() * 1.75])
        .collect();
    close(&audio, &expected);
}

#[test]
fn rack_branches_sum_at_their_gains_with_nested_stages() {
    // Branch 1: |x| at 0.5 then x2 gain (count 2). Branch 2: empty at 0.25.
    // Expected: 0.5 * 2 * |x| + 0.25 * x.
    let expected: Vec<[f64; 2]> = source()
        .iter()
        .map(|f| {
            std::array::from_fn(|c| {
                let x = f64::from(f[c]);
                x.abs() * 1.0 + 0.25 * x
            })
        })
        .collect();
    let stages = vec![
        Processor::Branch {
            count: 2,
            gain: 0.5,
            first: true,
            last: false,
        },
        rectify(Rectifier::Full),
        Processor::Gain(2.),
        Processor::Branch {
            count: 0,
            gain: 0.25,
            first: false,
            last: true,
        },
    ];
    for block in [1, 7, 64, 129] {
        let voice = plan()
            .with_voice_chains(
                vec![VoiceChain::new(vec![], stages.clone(), 0).unwrap()],
                vec![Some(0)],
            )
            .unwrap();
        close(&run(voice, block), &expected);
        let bus = plan()
            .with_buses(
                vec![Bus {
                    processors: stages.clone(),
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
