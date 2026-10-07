//! Daft filter through the runtime: identical output for every block partition
//! (the 32-frame control grid is absolute), DC passes at unity, and it filters.
//! Parameter laws are unit-tested beside the kernel (DSP_SYSTEM_INVENTORY).
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

const FRAMES: usize = 1000;

fn plan(signal: Vec<[f32; 2]>, cutoff: f64, response: f64) -> Prepared {
    let daft = Processor::Daft(DaftSettings {
        gain: Parameter::Constant(0.),
        cutoff: Parameter::Constant(cutoff),
        resonance: Parameter::Constant(0.3),
        response: Parameter::Constant(response),
    });
    Prepared::new(
        48000,
        vec![Pcm::new(48000, signal.into_boxed_slice()).unwrap()],
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
    .with_voice_chains(vec![VoiceChain::new(vec![daft], vec![], 0).unwrap()], vec![Some(0)])
    .unwrap()
}

fn run(prepared: Prepared, block: usize, start: usize) -> Vec<[f32; 2]> {
    let mut rt = Runtime::new(prepared, limits()).unwrap();
    let mut audio = vec![[0f32; 2]; FRAMES];
    support::without_heap(|| {
        // Start the note `start` frames in, so the voice meets the grid mid-quantum.
        let (head, tail) = audio.split_at_mut(start);
        for chunk in head.chunks_mut(block.max(1)) {
            rt.render(chunk).unwrap();
        }
        rt.trigger(
            Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) },
            60,
            1.,
        )
        .unwrap();
        for chunk in tail.chunks_mut(block) {
            rt.render(chunk).unwrap();
        }
    });
    audio
}

#[test]
fn output_does_not_depend_on_the_block_partition() {
    let signal: Vec<[f32; 2]> =
        (0..FRAMES).map(|i| [((i * 7) % 23) as f32 / 12. - 1., ((i * 5) % 19) as f32 / 10. - 0.9]).collect();
    for start in [0, 13] {
        let whole = run(plan(signal.clone(), 0.5, 0.), FRAMES - start, start);
        for block in [1, 7, 31, 32, 33, 64, 129] {
            assert_eq!(run(plan(signal.clone(), 0.5, 0.), block, start), whole, "block {block}, start {start}");
        }
    }
}

#[test]
fn low_pass_keeps_dc_and_removes_nyquist_high_pass_the_reverse() {
    let dc = vec![[0.5f32; 2]; FRAMES];
    let low = run(plan(dc.clone(), 0.3, 0.), 64, 0);
    assert!((low[900][0] - 0.5).abs() < 1e-3, "{:?}", low[900]);
    let alt: Vec<[f32; 2]> = (0..FRAMES).map(|i| if i % 2 == 0 { [0.5; 2] } else { [-0.5; 2] }).collect();
    let cut = run(plan(alt.clone(), 0.3, 0.), 64, 0);
    assert!(cut[900][0].abs() < 0.05, "{:?}", cut[900]);
    let high = run(plan(dc, 0.3, 1.), 64, 0);
    assert!(high[900][0].abs() < 1e-3, "{:?}", high[900]);
}
