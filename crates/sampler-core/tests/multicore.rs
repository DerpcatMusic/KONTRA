//! Multicore rendering is a pure speedup: any thread count produces the
//! single-threaded output bit for bit, and rendering stays heap-free.
use sampler_core::*;
mod support;

const LAYERS: usize = 4;
const NOTES: usize = 40;

fn runtime(threads: usize) -> Runtime {
    // Pseudo-random, distinct per layer, so a misplaced voice cannot hide.
    let samples: Vec<Pcm> = (0..LAYERS)
        .map(|layer| {
            let mut x = 0x9E37_79B9u32.wrapping_mul(layer as u32 + 1);
            let frames = (0..6000)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    let v = (x as f32 / u32::MAX as f32 - 0.5) * 0.2;
                    [v, -v * 0.5]
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Pcm::new(48000, frames).unwrap()
        })
        .collect();
    let regions = (0..LAYERS)
        .map(|sample| Region {
            sample,
            key_low: 48,
            key_high: 48 + NOTES as u8,
            root_key: Some(60),
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: Envelope::new(4, 2, 8, 0.5, 8).unwrap(),
            playback: Playback {
                transpose_semitones: 7.,
                ..Playback::default()
            },
        })
        .collect();
    let filter = Processor::StateVariable(StateVariableFilter {
        mode: SvfMode::LowPass,
        cutoff_hz: Parameter::Constant(3000.),
        q: Parameter::Constant(0.7),
    });
    let plan = Prepared::new(48000, samples, regions, LAYERS * (NOTES + 1))
        .unwrap()
        // Layer 2 has no chain, so runs mix lane batches with single voices.
        .with_voice_chains(
            vec![VoiceChain::new(vec![filter], vec![], 128).unwrap()],
            vec![Some(0), Some(0), None, Some(0)],
        )
        .unwrap();
    let rt = Runtime::new(
        plan,
        Limits {
            notes: NOTES,
            channels: 0,
            performances: 1,
            families: NOTES,
            decisions: 0,
            expressions: NOTES,
            voices: NOTES * LAYERS,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
    .with_threads(Threads::Fixed(threads));
    assert_eq!(rt.threads(), threads);
    rt
}

fn play(rt: &mut Runtime, blocks: &[usize]) -> Vec<Frame> {
    for id in 0..NOTES {
        rt.trigger_with_expression(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 48 + id as u8,
                external_id: Some(id as i32),
            },
            48 + id as u8,
            1.,
            Expression { gain: 0.5 + id as f64 * 0.02, ..Expression::default() },
        )
        .unwrap();
    }
    assert_eq!(rt.voice_count(), NOTES * LAYERS);
    let mut out = Vec::new();
    for &len in blocks.iter().cycle().take(120) {
        let mut block = vec![[0.; 2]; len];
        support::without_heap(|| rt.render(&mut block).unwrap());
        out.extend(block);
    }
    out
}

#[test]
fn any_thread_count_renders_the_single_threaded_output_exactly() {
    let blocks = [64, 37, 128, 64, 200, 1];
    let expected = play(&mut runtime(1), &blocks);
    assert!(expected.iter().any(|f| f[0] != 0.), "the workload is silent");
    for threads in [2, 3, 4] {
        let mut rt = runtime(threads);
        let actual = play(&mut rt, &blocks);
        assert_eq!(rt.voice_count(), 0, "voices ended at {threads} threads");
        assert!(rt.parallel_blocks() > 10, "the pool rendered {} blocks", rt.parallel_blocks());
        assert!(
            actual.iter().zip(&expected).all(|(a, e)| a.map(f32::to_bits) == e.map(f32::to_bits)),
            "{threads} threads differ from one"
        );
    }
}
