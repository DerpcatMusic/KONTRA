//! Compare a batchable native EQ chain with identical, distinct chain owners
//! that force the existing per-voice path in slot order.
use sampler_core::*;
mod support;

const NOTES: usize = 192;
const KEYS: usize = 16;

fn chain() -> VoiceChain {
    let mut stages = Vec::new();
    for (frequency, gain) in [(0.3, 6.), (0.55, -4.), (0.7, 3.)] {
        let mut band = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, frequency, 0.5, gain);
        band.law = sampler_ir::fx::eq::Law::KontaktLegacy;
        stages.push(Processor::Eq(band));
    }
    VoiceChain::new(Vec::new(), stages, 128).unwrap()
}

fn runtime(batchable: bool, threads: usize) -> Runtime {
    let pcm = Pcm::new(48000, (0..4096).map(|i| [(i as f32 * 0.13).sin(), (i as f32 * 0.07).cos()]).collect::<Vec<_>>().into_boxed_slice()).unwrap();
    let regions = (0..KEYS).map(|key| Region {
        sample: 0, key_low: 60 + key as u8, key_high: 60 + key as u8,
        root_key: None, velocity_low: 0., velocity_high: 1., gain: 0.01,
        envelope: Envelope::new(key as u32, 2, 32, 0.8, 7 + key as u32 * 3).unwrap(),
        playback: Playback { transpose_semitones: 3.17,
            loop_range: Some(Loop { start: 64, end: 128, mode: LoopMode::Continuous, shape: LoopShape::Wrap, passes: None }), ..Playback::default() },
    }).collect();
    let plan = Prepared::new(48000, vec![pcm], regions, NOTES).unwrap()
        .with_voice_chains((0..if batchable { 1 } else { KEYS }).map(|_| chain()).collect(),
            (0..KEYS).map(|key| Some(if batchable { 0 } else { key })).collect()).unwrap();
    let limits = Limits::for_plan(&plan, NOTES, NOTES);
    Runtime::new(plan, limits).unwrap().with_threads(Threads::Fixed(threads))
}

fn play(batchable: bool, threads: usize) -> Vec<Frame> {
    let mut rt = runtime(batchable, threads);
    let mut notes = Vec::new();
    for id in 0..NOTES {
        let key = 60 + (id % KEYS) as u8;
        notes.push(rt.trigger(Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key, external_id: Some(id as i32) }, key, 0.5 + id as f64 / 512.).unwrap());
    }
    assert_eq!(rt.voice_count(), NOTES);
    let mut all = Vec::new();
    for iteration in 0..120 {
        if iteration == 30 {
            // Unequal native release times and authored processor tails.
            for &note in &notes[..NOTES/2] { support::without_heap(|| rt.release(note).unwrap()); }
        }
        if iteration == 50 {
            for &note in &notes[NOTES/2..] { support::without_heap(|| rt.release(note).unwrap()); }
        }
        let len = [1, 3, 31, 32, 33, 64, 65, 128, 256][iteration % 9];
        let mut output = vec![[0.; 2]; len];
        support::without_heap(|| rt.render(&mut output).unwrap());
        all.extend(output);
    }
    assert_eq!(rt.voice_count(), 0);
    if threads > 1 { assert!(rt.parallel_blocks() > 10); }
    all
}

#[test]
fn native_eq_batches_match_scalar_chain_owners_and_parallel_tails_without_heap() {
    let expected = play(false, 1);
    assert!(expected.iter().flatten().any(|v| *v != 0.));
    for threads in [1, 2, 4] {
        let actual = play(true, threads);
        assert_eq!(actual.len(), expected.len());
        assert!(actual.iter().zip(&expected).all(|(a,b)| a.map(f32::to_bits) == b.map(f32::to_bits)), "{threads} threads changed PCM");
    }
}
