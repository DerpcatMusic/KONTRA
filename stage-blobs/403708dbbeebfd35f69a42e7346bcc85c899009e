//! Growing the voice pool mid-performance keeps every sounding voice and the
//! output bit-identical to a pool that was big enough from the start.
use sampler_core::*;
mod support;

#[test]
fn note_parameter_pages_grow_without_moving_live_state_or_audio_heap_work() {
    fn make(initial: usize) -> (Runtime, PlanControl) {
        let plan = Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[0.002; 2]; 6000].into_boxed_slice()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: Some(60),
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            512,
        )
        .unwrap();
        let limits = Limits {
            families: 512,
            ..Limits::for_plan(&plan, 512, 1024)
        };
        Runtime::with_plan_updates_and_note_capacity(plan, limits, 2, 1, initial).unwrap()
    }
    fn note(rt: &mut Runtime, id: i32) -> Result<NoteId, Error> {
        rt.trigger(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id),
            },
            60,
            1.,
        )
    }
    fn block(a: &mut Runtime, b: &mut Runtime) {
        let mut left = [[0.; 2]; 64];
        let mut right = left;
        support::without_heap(|| {
            a.render(&mut left).unwrap();
            b.render(&mut right).unwrap();
        });
        assert_eq!(
            left.map(|p| p.map(f32::to_bits)),
            right.map(|p| p.map(f32::to_bits))
        );
        assert!(left.iter().any(|p| p[0] != 0.));
    }
    let (mut small, mut control) = make(128);
    let (mut full, _) = make(512);
    for id in 0..97 {
        let a = note(&mut small, id).unwrap();
        let b = note(&mut full, id).unwrap();
        small
            .set_note_param(a, ModTarget::Decibels, -6., false)
            .unwrap();
        full.set_note_param(b, ModTarget::Decibels, -6., false)
            .unwrap();
    }
    assert!(control.note_pressure());
    block(&mut small, &mut full);
    assert_eq!(control.grow_note_params(256), Ok(256));
    assert_eq!(small.note_params_capacity(), 128);
    assert_eq!(control.grow_note_params(512), Err(PlanError::Capacity));
    note(&mut small, 97).unwrap();
    note(&mut full, 97).unwrap();
    assert!(
        !control.note_pressure(),
        "pressure on the old capacity must not request growth of the queued capacity"
    );
    block(&mut small, &mut full);
    assert_eq!(small.note_params_capacity(), 256);
    assert!(
        !control.note_pressure(),
        "adoption clears pressure against the old capacity"
    );
    for id in 98..256 {
        note(&mut small, id).unwrap();
        note(&mut full, id).unwrap();
    }
    assert_eq!(note(&mut small, 256), Err(Error::Capacity));
    assert_eq!(control.grow_note_params(512), Ok(512));
    block(&mut small, &mut full);
    assert_eq!(small.note_params_capacity(), 512);
    for id in 256..300 {
        note(&mut small, id).unwrap();
        note(&mut full, id).unwrap();
    }
    block(&mut small, &mut full);
    assert_eq!(small.stats().voice_growths, 0);
    assert_eq!(control.grow_note_params(513), Err(PlanError::Capacity));
}

const LAYERS: usize = 4;
const NOTES: usize = 40;

fn runtime(voices: usize, threads: usize) -> (Runtime, PlanControl) {
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
    let (rt, control) = Runtime::with_plan_updates(
        plan,
        Limits {
            notes: NOTES,
            channels: 0,
            performances: 1,
            families: NOTES,
            decisions: 0,
            expressions: NOTES,
            voices,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
        2,
        1,
    )
    .unwrap();
    let rt = rt.with_threads(Threads::Fixed(threads));
    assert_eq!(rt.threads(), threads);
    (rt, control)
}

fn trigger(rt: &mut Runtime, ids: std::ops::Range<usize>) {
    for id in ids {
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
            Expression {
                gain: 0.5 + id as f64 * 0.02,
                ..Expression::default()
            },
        )
        .unwrap();
    }
}

fn blocks(rt: &mut Runtime, count: usize, out: &mut Vec<Frame>) {
    for len in [64, 37, 128, 64, 200, 1].into_iter().cycle().take(count) {
        let mut block = vec![[0.; 2]; len];
        support::without_heap(|| rt.render(&mut block).unwrap());
        out.extend(block);
    }
}

/// Four notes fill a 16-voice pool; the rest arrive after it grew.
fn performance(rt: &mut Runtime, mut grow: impl FnMut(&mut Runtime)) -> Vec<Frame> {
    let mut out = Vec::new();
    trigger(rt, 0..4);
    blocks(rt, 3, &mut out);
    grow(rt);
    blocks(rt, 1, &mut out);
    trigger(rt, 4..NOTES);
    blocks(rt, 100, &mut out);
    out
}

#[test]
fn a_grown_pool_renders_what_a_big_pool_would() {
    for threads in [1, 2] {
        let (mut big, _control) = runtime(NOTES * LAYERS, threads);
        let expected = performance(&mut big, |_| {});
        assert!(expected.iter().any(|f| f[0] != 0.), "silent");

        let (mut small, mut control) = runtime(16, threads);
        let actual = performance(&mut small, |rt| {
            assert!(control.voice_pressure(), "no pressure at a full pool");
            assert_eq!(control.grow_voices(NOTES * LAYERS), Ok(NOTES * LAYERS));
            assert_eq!(
                rt.stats().voice_capacity,
                16,
                "grew before the audio thread took it"
            );
        });
        let stats = small.stats();
        assert_eq!(
            (
                stats.voice_capacity,
                stats.voice_growths,
                stats.growth_failures
            ),
            (NOTES * LAYERS, 1, 0)
        );
        assert_eq!(stats.voice_drops, 0);
        assert!(
            actual
                .iter()
                .zip(&expected)
                .all(|(a, e)| a.map(f32::to_bits) == e.map(f32::to_bits)),
            "{threads} threads: grown pool differs"
        );
        if threads > 1 {
            assert!(small.parallel_blocks() > 10);
        }
        // The old storage came back; a second growth is accepted.
        assert_eq!(
            control.grow_voices(NOTES * LAYERS * 2),
            Ok(NOTES * LAYERS * 2)
        );
    }
}
