//! Exact source output reuse within one existing, at-most-eight-voice batch.
//!
//! Snapshot guards stay alive through the batch: comparing a fresh guard's
//! pointer cannot mistake a newly published allocation for a retired one.
//! Envelope/gain, every processor state and slot-order summation remain per voice.
//! Each cache belongs to one batch with one immutable resampling kernel. A
//! quality change happens between batches; kernel quality is therefore not keyed.
use super::{Cursor, Direction, Loop, Resident};
use crate::{
    Voice,
    dsp::{Begun, Planar, PreparedVoiceChain},
    prepare::ResidentFrames,
    resample::Kernel,
};
use sampler_pool::SnapshotRead;

const ENTRIES: usize = 8;
type Levels<'a> = Option<SnapshotRead<'a, Box<[Box<[crate::Frame]>]>>>;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Hint {
    sample: usize,
    position: u64,
    fraction: u64,
    step: u64,
    base_step: u64,
}

impl Hint {
    fn new(voice: &Voice) -> Option<Self> {
        let c = &voice.cursor;
        if voice.stream_pages != 0
            || voice.stretch.is_some()
            || c.wavetable.is_some()
            || c.wavetable_base.is_some()
            || c.loops.is_some()
            || c.starvation.is_some()
            || c.fade_in != 0
            || c.cold_hold
        {
            return None;
        }
        Some(Self {
            sample: voice.sample,
            position: c.position,
            fraction: c.fraction.to_bits(),
            step: c.step.to_bits(),
            base_step: voice.base_step.to_bits(),
        })
    }
}

/// Conservatively miss an opportunity if pitch is about to converge. False
/// positives still pass the exhaustive key/current-snapshot comparison below.
/// Only voices with a possible partner pay for cache initialization or lookup.
pub(crate) fn candidates<'a>(voices: impl IntoIterator<Item = &'a Voice>) -> u8 {
    let mut hints = [None; ENTRIES];
    let mut mask = 0;
    for (i, voice) in voices.into_iter().enumerate() {
        if let Some(hint) = Hint::new(voice) {
            for (j, previous) in hints[..i].iter().enumerate() {
                if previous == &Some(hint) {
                    mask |= (1 << i) | (1 << j);
                }
            }
            hints[i] = Some(hint);
        }
    }
    mask
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CursorKey {
    start: usize,
    end: usize,
    direction: Direction,
    loop_range: Option<Loop>,
    controlled_loop: Option<(Loop, [u32; 2])>,
    position: u64,
    fraction: u64,
    step: u64,
    exit: Option<u64>,
    last: [u32; 2],
    fade_frames: u32,
    lead: u64,
}

impl CursorKey {
    #[inline]
    fn new(cursor: Cursor) -> Option<Self> {
        // Deliberately exhaustive: a future cursor field must be considered
        // before this optimization can compile. NaNs and signed zeros use bits.
        let Cursor {
            wavetable,
            wavetable_base,
            start,
            end,
            direction,
            loop_range,
            controlled_loop,
            loops,
            position,
            fraction,
            step,
            exit,
            last,
            starvation,
            fade_in,
            fade_frames,
            cold_hold,
            lead,
        } = cursor;
        if wavetable.is_some()
            || wavetable_base.is_some()
            || loops.is_some()
            || starvation.is_some()
            || fade_in != 0
            || cold_hold
        {
            return None;
        }
        Some(Self {
            start,
            end,
            direction,
            loop_range,
            controlled_loop: controlled_loop.map(|(r, offsets)| (r, offsets.map(f32::to_bits))),
            position,
            fraction: fraction.to_bits(),
            step: step.to_bits(),
            exit,
            last: last.map(f32::to_bits),
            fade_frames,
            lead,
        })
    }

    fn cursor(self) -> Cursor {
        let Self {
            start,
            end,
            direction,
            loop_range,
            controlled_loop,
            position,
            fraction,
            step,
            exit,
            last,
            fade_frames,
            lead,
        } = self;
        Cursor {
            wavetable: None,
            wavetable_base: None,
            start,
            end,
            direction,
            loop_range,
            controlled_loop: controlled_loop.map(|(r, offsets)| (r, offsets.map(f32::from_bits))),
            loops: None,
            position,
            fraction: f64::from_bits(fraction),
            step: f64::from_bits(step),
            exit,
            last: last.map(f32::from_bits),
            starvation: None,
            fade_in: 0,
            fade_frames,
            cold_hold: false,
            lead,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    chain: *const PreparedVoiceChain,
    sample: usize,
    cursor: CursorKey,
    frames: usize,
    envelope_remaining: usize,
    envelope_done: bool,
    tail_remaining: Option<u32>,
}

impl Key {
    fn new(chain: &PreparedVoiceChain, voice: &Voice, frames: usize) -> Option<Self> {
        if voice.stretch.is_some() {
            return None;
        }
        Some(Self {
            chain,
            sample: voice.sample,
            cursor: CursorKey::new(voice.cursor)?,
            frames,
            envelope_remaining: voice.envelope.remaining(),
            envelope_done: voice.envelope.done(),
            tail_remaining: voice.tail_remaining,
        })
    }
}

struct Entry<'a> {
    key: Key,
    after: CursorKey,
    planar: Planar,
    begun: Option<Begun>,
    frames: ResidentFrames<'a>,
    levels: Levels<'a>,
}

#[derive(Default)]
pub(crate) struct ResidentReuse<'a> {
    entries: [Option<Entry<'a>>; ENTRIES],
    len: usize,
    #[cfg(test)]
    hits: usize,
}

fn levels_id(levels: &Levels<'_>) -> (*const Box<[crate::Frame]>, usize) {
    let levels = levels.as_deref().map_or(&[][..], |levels| &levels[..]);
    (
        if levels.is_empty() {
            std::ptr::null()
        } else {
            levels.as_ptr()
        },
        levels.len(),
    )
}

impl<'a> ResidentReuse<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn begin(
        &mut self,
        chain: &PreparedVoiceChain,
        voice: &mut Voice,
        resident: ResidentFrames<'a>,
        levels: Levels<'a>,
        frames: usize,
        kernel: &Kernel,
        planar: &mut Planar,
        stretch: Option<&mut super::StretchEngine>,
    ) -> Option<Begun> {
        let key = Key::new(chain, voice, frames);
        if let Some(key) = key {
            for entry in self.entries[..self.len].iter().flatten() {
                if entry.key == key
                    && entry.frames.as_ptr() == resident.as_ptr()
                    && entry.frames.len() == resident.len()
                    && levels_id(&entry.levels) == levels_id(&levels)
                {
                    voice.cursor = entry.after.cursor();
                    *planar = entry.planar;
                    #[cfg(test)]
                    {
                        self.hits += 1;
                    }
                    return entry.begun;
                }
            }
        }
        let pcm = Resident {
            frames: &resident,
            levels: levels.as_deref().map_or(&[], |l| l),
        };
        let begun = chain.begin(voice, &pcm, frames, kernel, planar, stretch);
        if let (Some(key), Some(after)) = (
            key.filter(|_| self.len < ENTRIES),
            CursorKey::new(voice.cursor),
        ) {
            self.entries[self.len] = Some(Entry {
                key,
                after,
                planar: *planar,
                begun,
                frames: resident,
                levels,
            });
            self.len += 1;
        }
        begun
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Envelope, EnvelopeState, Limits, Pcm, Playback, Prepared, Region, Runtime, VoiceChain,
    };

    fn fixture() -> (Runtime, Voice, Pcm) {
        let pcm = Pcm::new(
            48000,
            (0..8192)
                .map(|i| [(i as f32 * 0.173).sin(), (i as f32 * 0.271).cos()])
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        )
        .unwrap();
        let plan = Prepared::new(
            48000,
            vec![pcm.clone()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 0.1,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            8,
        )
        .unwrap()
        .with_voice_chains(
            vec![VoiceChain::new(vec![], vec![], 17).unwrap()],
            vec![Some(0)],
        )
        .unwrap();
        let limits = Limits::for_plan(&plan, 8, 8);
        let mut rt = Runtime::new(plan, limits).unwrap();
        rt.trigger(
            crate::Input {
                protocol: crate::Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(0),
            },
            60,
            0.7,
        )
        .unwrap();
        let voice = rt.voices.slots.iter().find_map(|s| s.value).unwrap();
        (rt, voice, pcm)
    }

    fn bits(block: Planar) -> [[u64; crate::dsp::BLOCK]; 2] {
        block.map(|channel| channel.map(f64::to_bits))
    }

    fn cached<'a>(
        cache: &mut ResidentReuse<'a>,
        pcm: &'a Pcm,
        chain: &PreparedVoiceChain,
        mut voice: Voice,
        kernel: &Kernel,
    ) -> Planar {
        let mut block = [[0.; crate::dsp::BLOCK]; 2];
        cache.begin(
            chain,
            &mut voice,
            pcm.resident_frames().unwrap(),
            pcm.try_levels(),
            64,
            kernel,
            &mut block,
            None,
        );
        block
    }

    #[test]
    fn identical_source_work_reuses_exact_cursor_and_samples_before_voice_gain() {
        let (rt, original, pcm) = fixture();
        let chain = &rt.plans.get(original.plan.0).unwrap().prepared.voice_chains[0];
        eprintln!(
            "ResidentReuse stack bytes: {}",
            size_of::<ResidentReuse<'_>>()
        );
        assert!(size_of::<ResidentReuse<'_>>() < 16 * 1024);
        for direction in [Direction::Forward, Direction::Reverse] {
            for shape in [
                None,
                Some(super::super::LoopShape::Wrap),
                Some(super::super::LoopShape::PingPong),
                Some(super::super::LoopShape::Crossfade { frames: 7 }),
                Some(super::super::LoopShape::EqualPowerCrossfade { frames: 7 }),
            ] {
                for step in [0.73, 1., 1.23, 2.3, 8.1] {
                    for quality in [
                        crate::ResampleQuality::Realtime,
                        crate::ResampleQuality::High,
                    ] {
                        let kernel = Kernel::new(quality);
                        for frames in [1, 3, 31, 64] {
                            let mut before = original;
                            before.cursor = Playback {
                                direction,
                                loop_range: shape.map(|shape| Loop {
                                    start: 64,
                                    end: 128,
                                    mode: super::super::LoopMode::UntilRelease,
                                    shape,
                                    passes: None,
                                }),
                                ..Playback::default()
                            }
                            .cursor(8192, 48000, 48000)
                            .unwrap();
                            before.cursor.step = step;
                            before.cursor.position = 122;
                            before.cursor.fraction = 0.123456789;
                            for released in [false, true] {
                                let mut first = before;
                                if released {
                                    first.cursor.release();
                                }
                                let mut second = first;
                                // Source generation must not depend on the actual voice's
                                // amplitude or envelope levels; both are applied later.
                                second.gain = -0.123;
                                second.envelope =
                                    EnvelopeState::new(Envelope::new(17, 2, 31, 0.3, 13).unwrap());
                                let mut expected = second;
                                let mut expected_block = [[0.; crate::dsp::BLOCK]; 2];
                                let resident = pcm.resident_frames().unwrap();
                                let levels = pcm.try_levels();
                                let source = Resident {
                                    frames: &resident,
                                    levels: levels.as_deref().map_or(&[], |l| l),
                                };
                                let expected_begun = chain.begin(
                                    &mut expected,
                                    &source,
                                    frames,
                                    &kernel,
                                    &mut expected_block,
                                    None,
                                );
                                let mut cache = ResidentReuse::default();
                                let mut block = [[0.; crate::dsp::BLOCK]; 2];
                                cache.begin(
                                    chain,
                                    &mut first,
                                    pcm.resident_frames().unwrap(),
                                    pcm.try_levels(),
                                    frames,
                                    &kernel,
                                    &mut block,
                                    None,
                                );
                                let actual_begun = cache.begin(
                                    chain,
                                    &mut second,
                                    pcm.resident_frames().unwrap(),
                                    pcm.try_levels(),
                                    frames,
                                    &kernel,
                                    &mut block,
                                    None,
                                );
                                assert_eq!(cache.hits, 1);
                                assert_eq!(bits(block), bits(expected_block));
                                assert_eq!(actual_begun, expected_begun);
                                assert!(
                                    CursorKey::new(second.cursor)
                                        == CursorKey::new(expected.cursor)
                                );
                                for _ in 0..80 {
                                    assert_eq!(
                                        second.envelope.next().to_bits(),
                                        expected.envelope.next().to_bits()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn reused_tail_settlement_matches_every_begun_field() {
        let (rt, original, pcm) = fixture();
        let chain = &rt.plans.get(original.plan.0).unwrap().prepared.voice_chains[0];
        for position in [0, 8190, 8192] {
            for tail in [None, Some(0), Some(3), Some(80)] {
                for remaining in [0, 3, 100] {
                    for frames in [1, 7, 64] {
                        let mut before = original;
                        before.cursor.position = position;
                        before.envelope = EnvelopeState::new(Envelope::one_shot(0, remaining, 0));
                        before.tail_remaining = tail;
                        let mut expected = before;
                        let mut expected_block = [[0.; crate::dsp::BLOCK]; 2];
                        let resident = pcm.resident_frames().unwrap();
                        let source = Resident {
                            frames: &resident,
                            levels: &[],
                        };
                        let expected_begun = chain.begin(
                            &mut expected,
                            &source,
                            frames,
                            &rt.kernel,
                            &mut expected_block,
                            None,
                        );
                        let mut cache = ResidentReuse::default();
                        let mut first = before;
                        let mut second = before;
                        let mut block = [[0.; crate::dsp::BLOCK]; 2];
                        cache.begin(
                            chain,
                            &mut first,
                            pcm.resident_frames().unwrap(),
                            pcm.try_levels(),
                            frames,
                            &rt.kernel,
                            &mut block,
                            None,
                        );
                        let actual = cache.begin(
                            chain,
                            &mut second,
                            pcm.resident_frames().unwrap(),
                            pcm.try_levels(),
                            frames,
                            &rt.kernel,
                            &mut block,
                            None,
                        );
                        assert_eq!(cache.hits, 1);
                        assert_eq!(actual, expected_begun);
                        assert_eq!(bits(block), bits(expected_block));
                        assert!(CursorKey::new(second.cursor) == CursorKey::new(expected.cursor));
                    }
                }
            }
        }
    }

    #[test]
    fn preflight_skips_distinct_assets_phases_and_ineligible_voices() {
        let (_, original, _) = fixture();
        assert_eq!(candidates([&original]), 0);
        assert_eq!(candidates([&original, &original]), 0b11);
        let mut other = original;
        other.sample += 1;
        assert_eq!(candidates([&original, &other]), 0);
        let mut phase = original;
        phase.cursor.fraction = 0.123;
        assert_eq!(candidates([&original, &phase]), 0);
        let mut pitch = original;
        pitch.base_step += 0.01;
        assert_eq!(candidates([&original, &pitch]), 0);
        assert_eq!(
            candidates([&original, &other, &original, &phase, &other]),
            0b10111
        );
        let mut ineligible = original;
        ineligible.cursor.starvation = Some(0);
        assert_eq!(candidates([&ineligible, &ineligible]), 0);
        let mut amplitude = original;
        amplitude.gain = 0.77;
        assert_eq!(candidates([&original, &amplitude]), 0b11);
    }

    #[test]
    fn keys_reject_cursor_release_tail_source_and_partial_block_differences() {
        let (rt, original, pcm) = fixture();
        let chain = &rt.plans.get(original.plan.0).unwrap().prepared.voice_chains[0];
        let key = Key::new(chain, &original, 64).unwrap();
        let different =
            |voice| assert!(Key::new(chain, &voice, 64).is_none_or(|other| other != key));
        let mut v = original;
        v.sample += 1;
        different(v);
        let mut v = original;
        v.tail_remaining = Some(3);
        different(v);
        let mut v = original;
        v.envelope = EnvelopeState::new(Envelope::one_shot(0, 3, 0));
        different(v);
        let range = Loop {
            start: 64,
            end: 128,
            mode: super::super::LoopMode::UntilRelease,
            shape: super::super::LoopShape::Wrap,
            passes: None,
        };
        let mut v = original;
        v.cursor.loop_range = Some(range);
        different(v);
        let mut v = original;
        v.cursor.controlled_loop = Some((range, [f32::from_bits(0x7fc01234), -0.]));
        different(v);
        let mut v = original;
        v.cursor.wavetable = Some(crate::Wavetable::default());
        different(v);
        let mut v = original;
        v.cursor.wavetable_base = Some(crate::Wavetable::default());
        different(v);
        let mut v = original;
        v.cursor.loops = Some(super::super::LoopSlots {
            wavetable: None,
            slots: [None; 8],
            exits: [None; 8],
        });
        different(v);
        for edit in [
            |c: &mut Cursor| c.start += 1,
            |c: &mut Cursor| c.end -= 1,
            |c: &mut Cursor| c.direction = Direction::Reverse,
            |c: &mut Cursor| c.position += 1,
            |c: &mut Cursor| c.fraction = -0.,
            |c: &mut Cursor| c.step += 0.01,
            |c: &mut Cursor| c.exit = Some(0),
            |c: &mut Cursor| c.last[0] = -0.,
            |c: &mut Cursor| c.fade_frames += 1,
            |c: &mut Cursor| c.lead += 1,
            |c: &mut Cursor| c.starvation = Some(0),
            |c: &mut Cursor| c.fade_in = 1,
            |c: &mut Cursor| c.cold_hold = true,
        ] {
            let mut v = original;
            edit(&mut v.cursor);
            different(v);
        }
        assert!(Key::new(chain, &original, 63).unwrap() != key);
        let mut cache = ResidentReuse::default();
        for sample in 0..17 {
            let mut v = original;
            v.sample = sample;
            cache.begin(
                chain,
                &mut v,
                pcm.resident_frames().unwrap(),
                pcm.try_levels(),
                64,
                &rt.kernel,
                &mut [[0.; crate::dsp::BLOCK]; 2],
                None,
            );
        }
        assert_eq!(cache.hits, 0);
        assert_eq!(cache.len, ENTRIES);
    }

    #[test]
    fn current_mipmap_and_frame_generations_are_checked_while_old_guards_live() {
        let (rt, mut original, pcm) = fixture();
        let chain = &rt.plans.get(original.plan.0).unwrap().prepared.voice_chains[0];
        original.cursor.step = 4.3;
        pcm.want_levels(4.3, 1);
        assert!(crate::service_mipmaps(std::slice::from_ref(&pcm), usize::MAX, 0) > 0);
        let mut cache = ResidentReuse::default();
        let first = cached(&mut cache, &pcm, chain, original, &rt.kernel);
        assert_eq!(crate::service_mipmaps(std::slice::from_ref(&pcm), 0, 0), 0);
        assert!(crate::service_mipmaps(std::slice::from_ref(&pcm), usize::MAX, 0) > 0);
        assert_eq!(
            bits(first),
            bits(cached(&mut cache, &pcm, chain, original, &rt.kernel))
        );
        assert_eq!(
            cache.hits, 0,
            "new level generation must miss even with identical values"
        );
        cached(&mut cache, &pcm, chain, original, &rt.kernel);
        assert_eq!(cache.hits, 1);
        // Also retain an entry with no levels, so the subsequent frame reload
        // must miss on its resident generation alone, not just a level change.
        assert_eq!(crate::service_mipmaps(std::slice::from_ref(&pcm), 0, 0), 0);
        cached(&mut cache, &pcm, chain, original, &rt.kernel);
        assert_eq!(cache.hits, 1);
        pcm.set_residency_loader(|| Ok(vec![(0, vec![[0.25, -0.125]; 8192].into_boxed_slice())]))
            .unwrap();
        pcm.residency_acquire();
        pcm.residency_release();
        assert!(
            !pcm.residency_collect(),
            "cache guards retain old storage until control reclaims it"
        );
        assert!(pcm.resident_frames().is_none());
        pcm.residency_acquire();
        pcm.residency_reload().unwrap();
        let changed = cached(&mut cache, &pcm, chain, original, &rt.kernel);
        assert_eq!(
            cache.hits, 1,
            "same source slot with newly published frames must miss"
        );
        assert!(bits(first) != bits(changed));
        drop(cache);
        assert!(pcm.residency_collect());
        pcm.residency_release();
    }
}
