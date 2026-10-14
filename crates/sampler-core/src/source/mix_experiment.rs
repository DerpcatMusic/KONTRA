//! Test-only independent-frame SIMD experiment. Production mixing stays unchanged.
use super::*;
use crate::Envelope;

fn native_shape() -> Envelope {
    Envelope::new(96, 32, 96, 0.3, 96).unwrap().with_native_primary(&sampler_ir::SourceAhdsr {
        group: 0, slot: 0, attack_ms: 2., attack_curve: -0.5,
        hold_ms: 1., decay_ms: 2., sustain: 0.3, release_ms: 2.,
        ahd_only: false, native_amplitude: true,
    }, 48000).unwrap()
}

#[inline(always)]
fn constant_mix<'a>(input: impl Iterator<Item = &'a Frame>, output: &mut [Frame],
    gain: f32, gains: [f32; 2], level: f32) {
    if level == 1. {
        for (frame, source) in output.iter_mut().zip(input) {
            for c in 0..2 { frame[c] += source[c] * gain * gains[c]; }
        }
    } else {
        for (frame, source) in output.iter_mut().zip(input) {
            for c in 0..2 { frame[c] += source[c] * gain * gains[c] * level; }
        }
    }
}

fn candidate<'a>(input: impl Iterator<Item = &'a Frame>, output: &mut [Frame],
    envelope: &mut EnvelopeState, gain: f32, gains: [f32; 2]) {
    if output.len().min(input.size_hint().0) >= 16
        && let Some(level) = envelope.constant_level()
    {
        sampler_simd::dispatch(#[inline(always)] || {
            constant_mix(input, output, gain, gains, level)
        });
    } else {
        super::mix(input, output, envelope, gain, gains);
    }
}

#[test]
fn constant_mix_simd_preserves_pcm_and_dynamic_envelope_continuation() {
    let input: Vec<Frame> = (0..257).map(|i| match i % 7 {
        0 => [-0., 0.],
        1 => [f32::MIN_POSITIVE, -f32::MIN_POSITIVE],
        2 => [f32::from_bits(1), -f32::from_bits(1)],
        _ => [(i as f32 * 0.731).sin(), (i as f32 * 0.417).cos()],
    }).collect();
    let shapes = [Envelope::default(),
        Envelope::new(0, 0, 0, 0.8, 17).unwrap(),
        Envelope::new(0, 0, 0, 0., 17).unwrap(),
        Envelope::new(19, 7, 53, 0.8, 37).unwrap(),
        Envelope::new(19, 7, 53, 0.8, 37).unwrap().with_curves(
            crate::EnvelopeCurve::exponential(2.).unwrap(),
            crate::EnvelopeCurve::exponential(-3.).unwrap(),
            crate::EnvelopeCurve::exponential(-1.).unwrap()),
        Envelope::one_shot(17, 7, 41),
        native_shape(),
    ];
    for shape in shapes {
        for reverse in [false, true] {
            for gain in [-0., 0., 0.731, -1.201] {
                let (mut before, mut after) = (EnvelopeState::new(shape), EnvelopeState::new(shape));
                for (block, len) in [0usize, 1, 3, 4, 7, 15, 16, 17, 63, 64, 65, 128, 257].into_iter().enumerate() {
                    // A shorter input leaves the unmatched output suffix untouched.
                    let take = if block % 3 == 0 { len.saturating_sub(1) } else { len };
                    let mut expected = vec![[0.123, -0.567]; len];
                    let mut actual = expected.clone();
                    let gains = [[0.7, -0.2], [1., 1.], [0., -0.], [-1., 0.375]][block % 4];
                    if reverse {
                        super::mix(input[..take].iter().rev(), &mut expected, &mut before, gain, gains);
                        candidate(input[..take].iter().rev(), &mut actual, &mut after, gain, gains);
                    } else {
                        super::mix(input[..take].iter(), &mut expected, &mut before, gain, gains);
                        candidate(input[..take].iter(), &mut actual, &mut after, gain, gains);
                    }
                    assert_eq!(expected.iter().map(|f| f.map(f32::to_bits)).collect::<Vec<_>>(),
                        actual.iter().map(|f| f.map(f32::to_bits)).collect::<Vec<_>>());
                    assert_eq!(format!("{before:?}"), format!("{after:?}"));
                    if block == 7 { before.release(); after.release(); }
                }
            }
        }
    }
}

#[test]
#[ignore = "diagnostic stage timing; run optimized and pin caller; no whole-callback claim"]
fn constant_mix_simd_stage_evidence() {
    use std::{hint::black_box, time::Instant};
    let input: Vec<Frame> = (0..128).map(|i| [(i as f32 * 0.731).sin(), (i as f32 * 0.417).cos()]).collect();
    for (name, len, shape) in [
        ("short", 3, Envelope::default()),
        ("unity", 128, Envelope::default()),
        ("sustain", 128, Envelope::new(0, 0, 0, 0.8, 17).unwrap()),
        ("zero", 128, Envelope::new(0, 0, 0, 0., 17).unwrap()),
        ("moving", 128, Envelope::new(u32::MAX, 0, 0, 0.8, 17).unwrap()),
        ("native", 128, native_shape()),
    ] {
        let iterations = if name == "native" { 10_000 } else { 50_000 };
        for reverse in [false, true] {
            for round in 0..4 {
                let mut hashes = [0; 2];
                let mut times = [0; 2];
                for mode in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                    let mut output = [[0.; 2]; 128];
                    let mut envelope = EnvelopeState::new(shape);
                    // Production gains vary per voice. Do not let the test-only
                    // candidate specialize fixed gains while the production
                    // baseline retains its general runtime arguments.
                    let gain = black_box(0.731);
                    let gains = black_box([0.7, -0.2]);
                    let start = Instant::now();
                    for _ in 0..iterations {
                        let source = black_box(&input[..len]);
                        let target = black_box(&mut output[..len]);
                        let env = black_box(&mut envelope);
                        if reverse {
                            if mode == 0 { super::mix(source.iter().rev(), target, env, gain, gains); }
                            else { candidate(source.iter().rev(), target, env, gain, gains); }
                        } else if mode == 0 { super::mix(source.iter(), target, env, gain, gains); }
                        else { candidate(source.iter(), target, env, gain, gains); }
                    }
                    times[mode] = start.elapsed().as_nanos();
                    hashes[mode] = output.iter().flatten().fold(0_u64, |h, x| h.rotate_left(7) ^ u64::from(x.to_bits()));
                    black_box(envelope);
                }
                assert_eq!(hashes[0], hashes[1]);
                eprintln!("MIX_STAGE {{\"name\":\"{name}\",\"frames\":{len},\"reverse\":{reverse},\"round\":{round},\"iterations\":{iterations},\"before_ns\":{},\"candidate_ns\":{},\"hash\":{}}}", times[0], times[1], hashes[0]);
            }
        }
    }
}
