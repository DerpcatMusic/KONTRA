//! Source-free characterization, not a proposed/native-verified admission policy.
//! Two authored assets are intentionally selected by ONE logical note/family.
//! Worker jobs are held explicitly; output frames, never sleeps, control storage.
//! Unity-rate stereo impulses identify each asset without a library or decoder.
use sampler_core::*;
mod support;

const RATE: u32 = 48_000;
const HORIZON: u32 = 128;
const HOLD: usize = 2_880; // 60 ms, beyond the historical 50 ms onset timeout.
const PROBE: usize = 128;
// Existing 48-frame cold-recovery fade's first gain, frozen as IEEE-754 PCM.
const COLD_FIRST: f32 = f32::from_bits(0x3caa_aaa0);

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(i32::from(key)),
    }
}

fn pcm(sample: usize, frame: usize) -> Frame {
    let mut result = [0.; 2];
    result[sample] = match (sample, frame) {
        (0, 0) => 1.,
        (0, 64) | (1, 0) => 0.5,
        (1, 64) => 0.25,
        _ => 0.,
    };
    result
}

struct Fixture {
    rt: Runtime,
    worker: StreamWorker,
    assets: [Pcm; 2],
}

impl Fixture {
    fn new(heads: [usize; 2], separate_notes: bool, envelope: Envelope) -> Self {
        let assets: [Pcm; 2] = std::array::from_fn(|sample| {
            if heads[sample] == 0 {
                Pcm::streamed(RATE, PAGE_FRAMES).unwrap()
            } else {
                let head: Vec<_> = (0..heads[sample]).map(|i| pcm(sample, i)).collect();
                Pcm::headed(RATE, PAGE_FRAMES, &head).unwrap()
            }
        });
        assert_ne!(assets[0].asset_id(), assets[1].asset_id());
        let regions = (0..2)
            .map(|sample| {
                let key = if separate_notes {
                    60 + sample as u8
                } else {
                    60
                };
                Region {
                    sample,
                    key_low: key,
                    key_high: key,
                    root_key: None,
                    velocity_low: 0.,
                    velocity_high: 1.,
                    gain: 1.,
                    envelope,
                    playback: Playback::default(),
                }
            })
            .collect();
        let plan = Prepared::new(RATE, assets.to_vec(), regions, 2).unwrap();
        let limits = Limits::for_plan(&plan, 4, 4);
        let (cache, worker) = StreamCache::new(8).unwrap();
        let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
        rt.set_cold_starts(true);
        rt.set_stream_horizon(HORIZON).unwrap();
        Self { rt, worker, assets }
    }

    fn layered(&mut self) -> (NoteId, FamilyId) {
        let mut note = None;
        support::without_heap(|| note = Some(self.rt.trigger(input(60), 60, 1.).unwrap()));
        let note = note.unwrap();
        let mut families = self.rt.note_families(note).unwrap();
        let family = families.next().unwrap();
        assert!(families.next().is_none());
        assert_eq!(self.rt.family_note(family), Ok(note));
        assert_eq!(self.rt.family_voice_count(family), Ok(2));
        assert_eq!(
            (
                self.rt.note_count(),
                self.rt.family_count(),
                self.rt.voice_count()
            ),
            (1, 1, 2)
        );
        (note, family)
    }

    fn jobs(&mut self) -> [DecodeJob; 2] {
        service(&mut self.rt, false);
        let first = self.worker.next_job().unwrap();
        let second = self.worker.next_job().unwrap();
        assert!(self.worker.next_job().is_none());
        let jobs = if first.key().asset == self.assets[0].asset_id() {
            [first, second]
        } else {
            [second, first]
        };
        for (sample, job) in jobs.iter().enumerate() {
            assert_eq!(job.key(), self.key(sample));
            assert_eq!(job.range(), 0..PAGE_FRAMES);
        }
        jobs
    }

    fn key(&self, sample: usize) -> PageKey {
        PageKey {
            asset: self.assets[sample].asset_id(),
            index: 0,
        }
    }

    fn publish(&mut self, sample: usize, mut job: DecodeJob) {
        assert_eq!(job.key(), self.key(sample));
        let start = job.range().start;
        for (offset, frame) in job.frames_mut().iter_mut().enumerate() {
            *frame = pcm(sample, start + offset);
        }
        // Worker buffer initialization/completion is deliberately off audio.
        self.worker.complete(job, Ok(())).unwrap();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CursorReceipt {
    voice: VoiceId,
    source_frame: u64,
    cold: bool,
    starved: bool,
    resident: bool,
    status: PageStatus,
}

fn cursor(f: &Fixture, sample: usize) -> Option<CursorReceipt> {
    let mut result = None;
    support::without_heap(|| {
        assert!(
            f.rt.visit_stream_demand(1, |d| {
                if d.key.asset == f.assets[sample].asset_id() {
                    assert!(result.is_none(), "one page per voice in this fixture");
                    result = Some(CursorReceipt {
                        voice: d.voice,
                        source_frame: d.source_frame,
                        cold: d.cold,
                        starved: d.starved,
                        resident: d.resident,
                        status: d.status,
                    });
                }
                true
            })
            .unwrap()
        );
    });
    result
}

fn service(rt: &mut Runtime, ready: bool) {
    support::without_heap(|| assert_eq!(rt.service_streaming(HORIZON), Ok(ready)));
}

fn render(rt: &mut Runtime, output: &mut [Frame], partition: usize) {
    support::without_heap(|| {
        for block in output.chunks_mut(partition) {
            rt.render(block).unwrap();
        }
    });
}

fn expected(length: usize, onsets: [usize; 2], cold: [bool; 2]) -> Vec<Frame> {
    let mut result = vec![[0.; 2]; length];
    for (sample, (onset, cold)) in onsets.into_iter().zip(cold).enumerate() {
        if onset < length {
            result[onset][sample] = pcm(sample, 0)[sample] * if cold { COLD_FIRST } else { 1. };
        }
        if onset + 64 < length {
            result[onset + 64][sample] = pcm(sample, 64)[sample];
        }
    }
    result
}

fn receipt(case: &str, f: &Fixture, audio: &[Frame]) {
    let rt = &f.rt;
    let cursors = [0, 1].map(|sample| {
        cursor(f, sample).map(|c| serde_json::json!({
        "asset_label": sample, "source_frame": c.source_frame, "cold": c.cold,
        "starved": c.starved, "resident": c.resident, "page_status": format!("{:?}", c.status)
    }))
    });
    // Control-side source-free timing receipt; no paths, vendor PCM or originals.
    let impulses: Vec<_> = audio
        .iter()
        .enumerate()
        .filter_map(|(frame, pcm)| {
            (*pcm != [0.; 2]).then(|| {
                serde_json::json!({
                    "output_frame": frame, "pcm_bits": pcm.map(f32::to_bits)
                })
            })
        })
        .collect();
    println!(
        "ONSET_RECEIPT {}",
        serde_json::json!({
            "case": case, "sample_rate": RATE, "now": rt.now(),
            "notes": rt.note_count(), "families": rt.family_count(),
            "voices": rt.voice_count(), "cold_starts": rt.stats().cold_starts,
            "cursors": cursors, "distinct_assets": f.assets[0].asset_id() != f.assets[1].asset_id(),
            "underruns": rt.stream_underruns(), "impulses": impulses,
            "policy": "existing_behavior_only", "native_parity": "unverified"
        })
    );
}

#[test]
fn one_selection_two_distinct_assets_preserve_independently_delayed_attacks() {
    for hold in [0, 128, HOLD] {
        for early in [0, 1] {
            for partition in [1, 64, 127] {
                let mut f = Fixture::new([0, 0], false, Envelope::default());
                let (note, family) = f.layered();
                let mut jobs = f.jobs().map(Some);
                let before = [cursor(&f, 0).unwrap(), cursor(&f, 1).unwrap()];
                assert_ne!(before[0].voice, before[1].voice);
                for c in before {
                    assert_eq!(
                        (c.source_frame, c.cold, c.starved, c.status),
                        (0, true, true, PageStatus::Pending)
                    );
                }
                let late = 1 - early;
                f.publish(early, jobs[early].take().unwrap());
                service(&mut f.rt, false);
                let mut audio = vec![[0.; 2]; hold + PROBE];
                render(&mut f.rt, &mut audio[..hold], partition);
                let waiting = cursor(&f, late).unwrap();
                assert_eq!(waiting, before[late], "held cursor/voice must not age");
                assert_eq!(cursor(&f, early).unwrap().source_frame, hold as u64);
                assert_eq!(f.rt.family_note(family), Ok(note));
                assert_eq!(f.rt.family_voice_count(family), Ok(2));
                if partition == 64 {
                    receipt(
                        &format!("waiting_hold_{hold}_early_{early}"),
                        &f,
                        &audio[..hold],
                    );
                }
                f.publish(late, jobs[late].take().unwrap());
                service(&mut f.rt, true);
                render(&mut f.rt, &mut audio[hold..], partition);
                let mut onsets = [0, 0];
                onsets[late] = hold;
                assert_eq!(audio, expected(audio.len(), onsets, [true, true]));
                for (sample, before) in before.into_iter().enumerate() {
                    let c = cursor(&f, sample).unwrap();
                    assert_eq!(c.voice, before.voice);
                    assert_eq!(
                        (c.cold, c.starved, c.status),
                        (false, false, PageStatus::Ready)
                    );
                    assert_eq!(c.source_frame, (audio.len() - onsets[sample]) as u64);
                }
                assert_eq!(
                    (
                        f.rt.voice_count(),
                        f.rt.stats().cold_starts,
                        f.rt.stream_underruns()
                    ),
                    (2, 2, 0)
                );
                if partition == 64 {
                    receipt(
                        &format!("one_selection_hold_{hold}_early_{early}"),
                        &f,
                        &audio,
                    );
                }
            }
        }
    }
}

#[test]
fn distinct_note_control_keeps_owners_separate_with_the_same_storage_skew() {
    let mut f = Fixture::new([0, 0], true, Envelope::default());
    let mut notes = [None, None];
    support::without_heap(|| {
        for (sample, note) in notes.iter_mut().enumerate() {
            let key = 60 + sample as u8;
            *note = Some(f.rt.trigger(input(key), key, 1.).unwrap());
        }
    });
    let notes = notes.map(Option::unwrap);
    let families = notes.map(|n| f.rt.note_families(n).unwrap().next().unwrap());
    assert_ne!(notes[0], notes[1]);
    assert_ne!(families[0], families[1]);
    for (family, note) in families.into_iter().zip(notes) {
        assert_eq!(f.rt.family_note(family), Ok(note));
        assert_eq!(f.rt.family_voice_count(family), Ok(1));
    }
    let [a, b] = f.jobs();
    f.publish(0, a);
    service(&mut f.rt, false);
    let mut audio = vec![[0.; 2]; HOLD + PROBE];
    render(&mut f.rt, &mut audio[..HOLD], 64);
    assert_eq!(cursor(&f, 1).unwrap().source_frame, 0);
    f.publish(1, b);
    service(&mut f.rt, true);
    render(&mut f.rt, &mut audio[HOLD..], 64);
    assert_eq!(audio, expected(audio.len(), [0, HOLD], [true, true]));
    assert_eq!(
        (f.rt.note_count(), f.rt.family_count(), f.rt.voice_count()),
        (2, 2, 2)
    );
    receipt("distinct_notes_hold_2880", &f, &audio);
}

#[test]
fn partial_208_frame_warm_head_sounds_while_its_cold_sibling_waits() {
    let mut f = Fixture::new([208, 0], false, Envelope::default());
    f.layered();
    service(&mut f.rt, false);
    let b = f.worker.next_job().unwrap();
    assert_eq!(b.key(), f.key(1));
    assert!(
        f.worker.next_job().is_none(),
        "warm head covers initial horizon"
    );
    let warm = cursor(&f, 0).unwrap();
    assert_eq!(
        (warm.cold, warm.resident, warm.status),
        (false, true, PageStatus::Missing)
    );
    let mut audio = vec![[0.; 2]; HOLD + PROBE];
    render(&mut f.rt, &mut audio[..128], 64);
    assert_eq!(cursor(&f, 0).unwrap().source_frame, 128);
    assert_eq!(cursor(&f, 1).unwrap().source_frame, 0);
    service(&mut f.rt, false);
    let a = f.worker.next_job().unwrap();
    assert_eq!(a.key(), f.key(0));
    // Complete the warm asset's tail before frame 208: no tail underrun confound.
    f.publish(0, a);
    service(&mut f.rt, false);
    render(&mut f.rt, &mut audio[128..HOLD], 64);
    assert_eq!(
        (
            cursor(&f, 1).unwrap().source_frame,
            cursor(&f, 1).unwrap().cold
        ),
        (0, true)
    );
    f.publish(1, b);
    service(&mut f.rt, true);
    render(&mut f.rt, &mut audio[HOLD..], 64);
    assert_eq!(audio, expected(audio.len(), [0, HOLD], [false, true]));
    assert_eq!((f.rt.stats().cold_starts, f.rt.stream_underruns()), (1, 0));
    receipt("208_frame_head_and_cold_sibling", &f, &audio);
}

#[test]
fn authored_layer_delay_is_preserved_even_when_both_pages_are_already_ready() {
    let mut f = Fixture::new([0, 0], false, Envelope::default());
    // Prime both actual worker pages before admission, without selecting a note.
    for asset in &f.assets {
        f.rt.stream_cache_mut()
            .unwrap()
            .request(asset, 0, 0)
            .unwrap();
    }
    let a = f.worker.next_job().unwrap();
    let b = f.worker.next_job().unwrap();
    let (a, b) = if a.key() == f.key(0) { (a, b) } else { (b, a) };
    f.publish(0, a);
    f.publish(1, b);
    service(&mut f.rt, true);
    let mut owner = None;
    support::without_heap(|| {
        let note = f.rt.note_on(input(60), 60, 1.).unwrap();
        let family = f.rt.create_family(note).unwrap();
        for (sample, at) in [(0, 0), (1, 128)] {
            f.rt.start_family(
                family,
                sample,
                at,
                1.,
                Envelope::default(),
                Playback::default(),
            )
            .unwrap();
        }
        f.rt.finish_family(family).unwrap();
        owner = Some((note, family));
    });
    let (note, family) = owner.unwrap();
    assert_eq!(f.rt.family_note(family), Ok(note));
    assert_eq!(f.rt.pending_commands(), 1);
    assert_eq!(f.rt.stats().cold_starts, 0);
    let mut audio = [[0.; 2]; 256];
    render(&mut f.rt, &mut audio[..128], 64);
    assert_eq!(cursor(&f, 1).unwrap().source_frame, 0);
    assert!(!cursor(&f, 1).unwrap().cold);
    render(&mut f.rt, &mut audio[128..], 64);
    assert_eq!(
        audio.as_slice(),
        expected(audio.len(), [0, 128], [false, false])
    );
    assert_eq!(f.rt.pending_commands(), 0);
    receipt("authored_128_frame_delay_ready_pages", &f, &audio);
}

#[test]
fn terminal_failure_of_one_asset_does_not_end_its_ready_sibling() {
    let mut f = Fixture::new([0, 0], false, Envelope::default());
    let (_, family) = f.layered();
    let [a, b] = f.jobs();
    let failed_voice = cursor(&f, 1).unwrap().voice;
    f.publish(0, a);
    service(&mut f.rt, false);
    let mut audio = [[0.; 2]; 256];
    render(&mut f.rt, &mut audio[..128], 64);
    f.worker
        .complete(b, Err(DecodeFailure::InvalidSamples))
        .unwrap();
    support::without_heap(|| {
        assert_eq!(
            f.rt.service_streaming(HORIZON),
            Err(StreamError::DecodeFailed(DecodeFailure::InvalidSamples))
        );
        assert!(!f.rt.voice_active(failed_voice));
        assert_eq!(f.rt.family_voice_count(family), Ok(1));
    });
    assert!(cursor(&f, 1).is_none());
    render(&mut f.rt, &mut audio[128..], 64);
    assert_eq!(
        audio.as_slice(),
        expected(audio.len(), [0, audio.len()], [true, true])
    );
    assert_eq!(f.rt.stream_underruns(), 0);
    receipt("terminal_failed_cold_sibling", &f, &audio);
}

#[test]
fn unavailable_page_holds_only_its_sibling_until_explicit_request_replacement() {
    let mut f = Fixture::new([0, 0], false, Envelope::default());
    let (_, family) = f.layered();
    let [a, b] = f.jobs();
    f.publish(0, a);
    service(&mut f.rt, false);
    let mut audio = [[0.; 2]; 384];
    render(&mut f.rt, &mut audio[..128], 64);
    let key = f.key(1);
    f.worker
        .complete(b, Err(DecodeFailure::Unavailable))
        .unwrap();
    // Observe the failed page directly; do not depend on wall-clock retry/backoff.
    support::without_heap(|| {
        assert_eq!(
            f.rt.stream_cache_mut().unwrap().poll(),
            Some(PageUpdate::Failed(key, DecodeFailure::Unavailable))
        );
    });
    let failed = cursor(&f, 1).unwrap();
    assert_eq!(
        (failed.source_frame, failed.cold, failed.status),
        (0, true, PageStatus::Failed(DecodeFailure::Unavailable))
    );
    render(&mut f.rt, &mut audio[128..256], 64);
    assert_eq!(cursor(&f, 1), Some(failed));
    assert_eq!(f.rt.family_voice_count(family), Ok(2));
    // Explicit invalidation models control recovery, not automatic retry policy.
    support::without_heap(|| {
        assert_eq!(f.rt.stream_cache_mut().unwrap().invalidate(key), Ok(true))
    });
    service(&mut f.rt, false);
    let replacement = f.worker.next_job().unwrap();
    assert_eq!(replacement.key(), key);
    f.publish(1, replacement);
    service(&mut f.rt, true);
    render(&mut f.rt, &mut audio[256..], 64);
    assert_eq!(
        audio.as_slice(),
        expected(audio.len(), [0, 256], [true, true])
    );
    receipt("unavailable_page_explicit_replacement", &f, &audio);
}

#[test]
fn invalidated_held_page_discards_stale_completion_before_replacement_onset() {
    let mut f = Fixture::new([0, 0], false, Envelope::default());
    f.layered();
    let [a, obsolete] = f.jobs();
    f.publish(0, a);
    service(&mut f.rt, false);
    let mut audio = [[0.; 2]; 384];
    render(&mut f.rt, &mut audio[..128], 64);
    let key = f.key(1);
    support::without_heap(|| {
        assert_eq!(f.rt.stream_cache_mut().unwrap().invalidate(key), Ok(true))
    });
    f.publish(1, obsolete);
    support::without_heap(|| {
        assert_eq!(
            f.rt.stream_cache_mut().unwrap().poll(),
            Some(PageUpdate::Discarded(key))
        );
    });
    service(&mut f.rt, false);
    let replacement = f.worker.next_job().unwrap();
    assert_eq!(replacement.key(), key);
    render(&mut f.rt, &mut audio[128..256], 64);
    assert_eq!(
        (
            cursor(&f, 1).unwrap().source_frame,
            cursor(&f, 1).unwrap().cold
        ),
        (0, true)
    );
    f.publish(1, replacement);
    service(&mut f.rt, true);
    render(&mut f.rt, &mut audio[256..], 64);
    assert_eq!(
        audio.as_slice(),
        expected(audio.len(), [0, 256], [true, true])
    );
    receipt("invalidated_job_discard_then_replacement", &f, &audio);
}

#[test]
fn note_off_or_family_release_during_wait_preserves_late_released_attack() {
    for family_release in [false, true] {
        let mut f = Fixture::new([0, 0], false, Envelope::new(0, 0, 0, 1., 128).unwrap());
        let (note, family) = f.layered();
        let [a, b] = f.jobs();
        f.publish(0, a);
        service(&mut f.rt, false);
        let mut audio = vec![[0.; 2]; HOLD + PROBE];
        render(&mut f.rt, &mut audio[..128], 64);
        support::without_heap(|| {
            if family_release {
                f.rt.release_family(family).unwrap();
            } else {
                assert_eq!(f.rt.note_off(input(60), None), Ok(note));
            }
        });
        render(&mut f.rt, &mut audio[128..HOLD], 64);
        assert_eq!(f.rt.family_voice_count(family), Ok(1));
        let waiting = cursor(&f, 1).unwrap();
        assert_eq!((waiting.source_frame, waiting.cold), (0, true));
        assert!(
            cursor(&f, 0).is_none(),
            "ready sibling's release clock already ended"
        );
        f.publish(1, b);
        service(&mut f.rt, true);
        render(&mut f.rt, &mut audio[HOLD..], 64);
        let mut want = expected(audio.len(), [0, HOLD], [true, true]);
        want[HOLD + 64][1] = 0.125; // 0.25 * (1 - 64/128), frozen release PCM.
        assert_eq!(audio, want);
        assert_eq!((f.rt.voice_count(), f.rt.family_count()), (0, 0));
        assert_eq!(f.rt.stream_underruns(), 0);
        receipt(
            if family_release {
                "family_release_while_waiting"
            } else {
                "note_off_while_waiting"
            },
            &f,
            &audio,
        );
    }
}

#[test]
fn canceling_the_logical_owner_prevents_held_page_completion_from_resurrecting_layers() {
    let mut f = Fixture::new([0, 0], false, Envelope::default());
    let (note, _) = f.layered();
    let [a, b] = f.jobs();
    f.publish(0, a);
    service(&mut f.rt, false);
    let mut audio = [[0.; 2]; 256];
    render(&mut f.rt, &mut audio[..128], 64);
    support::without_heap(|| f.rt.cancel_input_note(note).unwrap());
    assert_eq!((f.rt.voice_count(), f.rt.family_count()), (0, 0));
    f.publish(1, b);
    service(&mut f.rt, true);
    render(&mut f.rt, &mut audio[128..], 64);
    assert_eq!(&audio[128..], &[[0.; 2]; 128]);
    assert!(cursor(&f, 0).is_none() && cursor(&f, 1).is_none());
    assert_eq!(f.rt.pending_commands(), 0);
    receipt("cancel_owner_before_held_completion", &f, &audio);
}
