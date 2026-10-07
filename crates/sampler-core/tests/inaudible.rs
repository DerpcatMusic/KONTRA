//! A releasing voice whose output stays under about -120 dBFS is freed;
//! audible voices are not, and the mix moves by less than that threshold.
use sampler_core::*;
mod support;

const RELEASE: u32 = 480_000; // ten seconds

fn runtime() -> Runtime {
    let mut x = 0x9E37_79B9u32;
    let frames = (0..600_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let v = (x as f32 / u32::MAX as f32 - 0.5) * 0.5;
            [v, -v]
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let region = Region {
        sample: 0,
        key_low: 40,
        key_high: 80,
        root_key: Some(60),
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::new(4, 0, 8, 1.0, RELEASE).unwrap(),
        playback: Playback::default(),
    };
    let plan = Prepared::new(48000, vec![Pcm::new(48000, frames).unwrap()], vec![region], 64).unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 0,
            performances: 1,
            families: 8,
            decisions: 0,
            expressions: 8,
            voices: 8,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn input(key: u8) -> Input {
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key, external_id: None }
}

/// Play `(key, gain)` notes, release after 1000 frames, render 4000 more.
fn play(notes: &[(u8, f64)]) -> (Runtime, Vec<Frame>) {
    let mut rt = runtime();
    for &(key, gain) in notes {
        rt.trigger_with_expression(
            input(key),
            key,
            1.,
            Expression { gain, ..Expression::default() },
        )
        .unwrap();
    }
    let mut out = Vec::new();
    let mut block = |rt: &mut Runtime, frames: usize| {
        let mut b = vec![[0.; 2]; frames];
        support::without_heap(|| rt.render(&mut b).unwrap());
        out.extend(b);
    };
    block(&mut rt, 1000);
    for &(key, _) in notes {
        rt.note_off(input(key), None).unwrap();
    }
    for _ in 0..4000 / 64 {
        block(&mut rt, 64);
    }
    (rt, out)
}

#[test]
fn only_inaudible_releasing_voices_are_freed() {
    // The first note is -180 dB, the second full level; both release for ten seconds.
    let (rt, mixed) = play(&[(50, 1e-9), (51, 1.0)]);
    assert_eq!(rt.voice_count(), 1, "the loud voice stays, the inaudible one is freed");
    let (alone, reference) = play(&[(51, 1.0)]);
    assert_eq!(alone.voice_count(), 1);
    assert!(mixed.iter().any(|f| f[0].abs() > 1e-3), "the loud voice sounds");
    let worst = mixed
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
        .fold(0., f32::max);
    assert!(worst < 1e-6, "freeing moved the mix by {worst}");
    // Held voices are never freed, however quiet.
    let mut rt = runtime();
    rt.trigger_with_expression(input(50), 50, 1., Expression { gain: 1e-9, ..Expression::default() }).unwrap();
    for _ in 0..100 {
        rt.render(&mut [[0.; 2]; 64]).unwrap();
    }
    assert_eq!(rt.voice_count(), 1);
}
