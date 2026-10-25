//! Synthetic source/page-handoff contract. No bank, file, decoder, sleeps or
//! worker threads. Root owns compilation/execution; held DecodeJobs are the clock.
//! Neither these tests nor the proposed explicit contract assert native timing.
use sampler_core::*;
mod support;

const RATE: u32 = 48_000;
const START: usize = 5_952;
const SELECTED: usize = 89;
const OBSERVED_SPAN: usize = 155; // First unavailable physical frame 6107.
const REQUESTED: u32 = 1_019;
const SERVICE: u32 = 4_224;
const BLOCK: usize = 128;
const LENGTH: usize = PAGE_FRAMES * 4;
const FIRST_FADE: f32 = f32::from_bits(0x3caa_aaa0);

fn input(id: i32) -> Input {
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0,
        key: 60, external_id: Some(id) }
}
fn frame(sample: usize, index: usize) -> Frame {
    let mut out = [0.; 2];
    // Distinct exact binary values reveal offset loss/restart and channel skew.
    out[sample] = ((index % 31 + 1) as f32) / 64.;
    out
}

struct Fixture {
    rt: Runtime,
    worker: StreamWorker,
    assets: [Pcm; 2],
}
impl Fixture {
    fn new(retained: usize, runway: Option<u32>, chain: bool) -> Self {
        Self::new_with_pitch(retained, runway, chain, 0.)
    }
    fn new_with_pitch(retained: usize, runway: Option<u32>, chain: bool, pitch: f64) -> Self {
        Self::configured([retained; 2], runway,
            chain.then(|| vec![Processor::Gain(1.)]), Envelope::default(), pitch, 4)
    }
    fn configured(heads: [usize; 2], runway: Option<u32>, processors: Option<Vec<Processor>>,
                  envelope: Envelope, pitch: f64, voices: usize) -> Self {
        let assets: [Pcm; 2] = std::array::from_fn(|sample| {
            let asset = Pcm::streamed(RATE, LENGTH).unwrap();
            let retained = heads[sample];
            if retained != 0 {
                drop(asset.set_ranges(vec![(START, (START..START + retained)
                    .map(|i| frame(sample, i)).collect::<Vec<_>>().into_boxed_slice())]).unwrap());
            }
            asset
        });
        assert_ne!(assets[0].asset_id(), assets[1].asset_id());
        let regions = (0..2).map(|sample| Region { sample, key_low: 60,
            key_high: 60, root_key: None, velocity_low: 0., velocity_high: 1.,
            gain: 1., envelope,
            playback: Playback { start: START, ..Default::default() },
        }).collect();
        let mut plan = Prepared::new(RATE, assets.to_vec(), regions, voices).unwrap();
        if let Some(processors) = processors {
            plan = plan.with_voice_chains(
                vec![VoiceChain::new(processors, vec![], 0).unwrap()],
                vec![Some(0); 2],
            ).unwrap();
        }
        if pitch != 0. {
            plan = plan.with_voice_modulation(vec![ModProgram {
                sources: vec![ModSource::Constant],
                routes: vec![ModRoute::new(0, ModTarget::Pitch, pitch)],
                ..Default::default()
            }], vec![Some(0); 2], vec![0; 2]).unwrap();
        }
        let limits = Limits::for_plan(&plan, voices, voices);
        let (cache, worker) = StreamCache::new(voices * 2).unwrap();
        let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
        rt.set_cold_starts(true);
        rt.set_stream_horizon(SERVICE).unwrap();
        if let Some(runway) = runway { rt.set_stream_start_runway(runway).unwrap(); }
        Self { rt, worker, assets }
    }
    fn layered(&mut self) -> (NoteId, FamilyId) {
        let mut note = None;
        support::without_heap(|| note = Some(self.rt.trigger(input(1), 60, 1.).unwrap()));
        let note = note.unwrap();
        let family = self.rt.note_families(note).unwrap().next().unwrap();
        assert_eq!(self.rt.family_voice_count(family), Ok(2));
        (note, family)
    }
    fn service(&mut self) -> bool {
        let mut result = None;
        support::without_heap(|| result = Some(self.rt.service_streaming(SERVICE).unwrap()));
        result.unwrap()
    }
    fn jobs(&mut self) -> Vec<DecodeJob> {
        assert!(!self.service());
        let mut out = Vec::new(); // Worker/control side, never callback.
        while let Some(job) = self.worker.next_job() { out.push(job); }
        assert_eq!(out.len(), 4, "two assets, current page 1 and next page 2");
        for job in &out {
            assert!(self.assets.iter().any(|a| a.asset_id() == job.key().asset));
            assert!([1, 2].contains(&job.key().index));
        }
        out
    }
    fn publish(&mut self, mut job: DecodeJob) {
        let sample = self.assets.iter().position(|a| a.asset_id() == job.key().asset).unwrap();
        let start = job.range().start;
        for (offset, out) in job.frames_mut().iter_mut().enumerate() { *out = frame(sample, start + offset); }
        self.worker.complete(job, Ok(())).unwrap();
    }
    fn render(&mut self, out: &mut [Frame], partition: usize) {
        support::without_heap(|| {
            for part in out.chunks_mut(partition) { self.rt.render(part).unwrap(); }
        });
    }
    fn cursor(&self, sample: usize) -> (VoiceId, u64, bool, bool) {
        let mut found = None;
        support::without_heap(|| {
            assert!(self.rt.visit_stream_demand(1, |d| {
                if d.key.asset == self.assets[sample].asset_id() {
                    assert!(found.is_none());
                    found = Some((d.voice, d.source_frame, d.starved, d.starved));
                }
                true
            }).unwrap());
        });
        found.expect("one live source per asset")
    }
    fn current_page_ready(&mut self) {
        for sample in 0..2 {
            let asset = &self.assets[sample];
            self.rt.stream_cache_mut().unwrap().request(asset, START / PAGE_FRAMES, 0).unwrap();
        }
        while let Some(job) = self.worker.next_job() { self.publish(job); }
        assert!(self.service()); // No live voices yet; poll only.
    }
}

#[test]
fn legacy_first_window_contract_reproduces_short_runway_starvation() {
    for retained in [SELECTED, OBSERVED_SPAN] {
        let mut f = Fixture::new(retained, None, false);
        f.layered();
        assert_eq!(f.rt.stats().cold_starts, 0);
        let held = f.jobs();
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out[0], [frame(0, START)[0], frame(1, START)[1]]);
        if retained == OBSERVED_SPAN {
            assert_eq!(f.rt.stream_underruns(), 0);
            let mut first = [None, None];
            support::without_heap(|| {
                assert!(f.rt.visit_stream_demand(BLOCK as u32, |d| {
                    if !d.resident && first[usize::from(d.key.asset == f.assets[1].asset_id())].is_none() {
                        let sample = usize::from(d.key.asset == f.assets[1].asset_id());
                        first[sample] = Some((d.frames.start, d.deadline - f.rt.now(), d.source_frame, d.status));
                    }
                    true
                }).unwrap());
            });
            assert_eq!(first, [Some((6107, 27, 6080, PageStatus::Pending)); 2]);
            f.render(&mut out, BLOCK);
        }
        assert_eq!(f.rt.stream_underruns(), 2);
        assert_eq!(held.len(), 4);
        // Held jobs drop only here, on control.
    }
}

#[test]
fn short_heads_wait_for_requested_runway_without_advancing_or_heap_work() {
    for retained in [SELECTED, OBSERVED_SPAN] {
        for chain in [false, true] {
            for partition in [1, 64, BLOCK] {
                let mut f = Fixture::new(retained, Some(REQUESTED), chain);
                let (_, family) = f.layered();
                let before = [f.cursor(0), f.cursor(1)];
                assert_eq!(f.rt.stats().cold_starts, 2);
                assert!(before.iter().all(|c| c.1 == START as u64 && c.2 && c.3));
                assert!(!f.assets.iter().any(Pcm::take_cold), "present heads need page handoff, not head reload");
                let jobs = f.jobs();
                let mut out = [[0.; 2]; 2_880]; // Beyond old 50 ms timeout.
                f.render(&mut out, partition);
                assert_eq!(out, [[0.; 2]; 2_880]);
                assert_eq!([f.cursor(0), f.cursor(1)], before);
                assert_eq!(f.rt.family_voice_count(family), Ok(2));
                assert_eq!(f.rt.stream_underruns(), 0);
                assert_eq!(f.rt.now(), 2_880);
                for job in jobs { f.publish(job); }
                assert!(f.service());
                let mut first = [[0.; 2]; BLOCK];
                f.render(&mut first, partition);
                assert_eq!(first[0][0].to_bits(), (frame(0, START)[0] * FIRST_FADE).to_bits());
                assert_eq!(first[0][1].to_bits(), (frame(1, START)[1] * FIRST_FADE).to_bits());
                assert_eq!(f.cursor(0).1, (START + BLOCK) as u64);
                assert_eq!(f.cursor(1).1, (START + BLOCK) as u64);
                assert_eq!(f.rt.stream_underruns(), 0);
            }
        }
    }
}

#[test]
fn one_ready_sibling_does_not_split_an_unsounded_family_cohort() {
    for chain in [false, true] {
        // One fully sufficient head and one short head at the SAME deadline.
        let mut f = Fixture::configured([REQUESTED as usize, SELECTED], Some(REQUESTED),
            chain.then(|| vec![Processor::Gain(1.)]), Envelope::default(), 0., 4);
        let (_, family) = f.layered();
        assert_eq!(f.rt.stats().cold_starts, 1);
        let before = [f.cursor(0).1, f.cursor(1).1];
        let mut held = Vec::new();
        assert!(!f.service());
        while let Some(job) = f.worker.next_job() { held.push(job); }
        let mut out = [[0.; 2]; 320];
        f.render(&mut out, 113);
        assert_eq!(out, [[0.; 2]; 320]);
        assert_eq!([f.cursor(0).1, f.cursor(1).1], before);
        let mut later = Vec::new();
        for job in held {
            if job.key().asset == f.assets[1].asset_id() && job.key().index == 1 { f.publish(job); }
            else { later.push(job); }
        }
        assert!(!f.service(), "unrelated service pages remain missing");
        f.render(&mut out[..BLOCK], BLOCK);
        assert_eq!(out[0][0], frame(0, START)[0]);
        assert_eq!(out[0][1].to_bits(), (frame(1, START)[1] * FIRST_FADE).to_bits());
        assert_eq!([f.cursor(0).1, f.cursor(1).1], [(START + BLOCK) as u64; 2]);
        assert_eq!(f.rt.family_voice_count(family), Ok(2));
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

#[test]
fn complete_heads_or_ready_pages_keep_immediate_warm_source_timing() {
    for from_pages in [false, true] {
        let mut f = Fixture::new(if from_pages { SELECTED } else { REQUESTED as usize }, Some(REQUESTED), false);
        if from_pages { f.current_page_ready(); }
        f.rt.set_cold_starts(false);
        f.layered();
        assert_eq!(f.rt.stats().cold_starts, 0);
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out[0], [frame(0, START)[0], frame(1, START)[1]], "no cold fade/delay for a ready runway");
        assert_eq!(f.cursor(0).1, (START + BLOCK) as u64);
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

#[test]
fn a_ready_first_window_cannot_bypass_explicit_runway_refusal() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    f.rt.set_cold_starts(false);
    let note = f.rt.note_on(input(1), 60, 1.).unwrap();
    let family = f.rt.create_family(note).unwrap();
    support::without_heap(|| assert_eq!(f.rt.start_family(
        family, 0, 0, 1., Envelope::default(),
        Playback { start: START, ..Default::default() }), Err(Error::NotReady)));
    assert_eq!(f.rt.voice_count(), 0);
    assert!(!f.assets[0].take_cold());
    assert!(f.worker.next_job().is_none(), "admission never submits jobs");
}

#[test]
fn held_runway_rechecks_live_pitch_even_when_first_page_is_ready() {
    for chain in [false, true] {
        let mut f = Fixture::new(SELECTED, Some(REQUESTED), chain);
        let (note, _) = f.layered();
        let before = [f.cursor(0), f.cursor(1)];
        let mut held = Vec::new();
        for job in f.jobs() {
            if job.key().index == 1 { f.publish(job); } else { held.push(job); }
        }
        assert!(!f.service());
        let expression = f.rt.expression_id(note).unwrap();
        support::without_heap(|| f.rt.set_expression(expression,
            Expression { pitch_semitones: 24., ..Default::default() }).unwrap());
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out, [[0.; 2]; BLOCK], "first-page readiness is stale for the larger live-pitch runway");
        assert_eq!([f.cursor(0), f.cursor(1)], before);
        for job in held { f.publish(job); }
        f.service();
        f.render(&mut out, BLOCK);
        assert!(out.iter().any(|x| *x != [0.; 2]));
        assert_eq!(f.cursor(0).1, (START + 4 * BLOCK) as u64);
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

#[test]
fn head_publication_cannot_leave_a_stale_ready_release_for_held_sources() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    f.layered();
    let before = [f.cursor(0), f.cursor(1)];
    let held = f.jobs();
    for (sample, asset) in f.assets.iter().enumerate() {
        drop(asset.set_ranges(vec![(START, (START..START + REQUESTED as usize)
            .map(|i| frame(sample, i)).collect::<Vec<_>>().into_boxed_slice())]).unwrap());
    }
    // Observe complete current demand, then replace that immutable generation
    // before rendering: observation must not become an admission cache.
    assert!(f.rt.visit_stream_demand(REQUESTED, |d| d.resident).unwrap());
    for asset in &f.assets { drop(asset.set_ranges(vec![]).unwrap()); }
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    assert_eq!(out, [[0.; 2]; BLOCK]);
    assert_eq!([f.cursor(0), f.cursor(1)], before);
    for job in held { f.publish(job); }
    assert!(f.service());
    f.render(&mut out, BLOCK);
    assert_eq!(out[0][0].to_bits(), (frame(0, START)[0] * FIRST_FADE).to_bits());
    assert_eq!(f.rt.stream_underruns(), 0);
}

#[test]
fn late_completions_cannot_revive_cancelled_voice_generations() {
    for stop in [false, true] {
        let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
        let (_, family) = f.layered();
        let old = [f.cursor(0).0, f.cursor(1).0];
        let jobs = f.jobs();
        support::without_heap(|| {
            if stop { f.rt.stop_family(family).unwrap(); }
            else { f.rt.release_family(family).unwrap(); }
            f.rt.render(&mut [[0.; 2]; BLOCK]).unwrap();
        });
        assert!(old.iter().all(|id| !f.rt.voice_active(*id)));
        for job in jobs { f.publish(job); }
        assert!(f.service());
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out, [[0.; 2]; BLOCK]);
        let note = f.rt.note_on(input(2), 60, 1.).unwrap();
        let next_family = f.rt.create_family(note).unwrap();
        let new = f.rt.start_family(next_family, 0, f.rt.now(), 1., Envelope::default(),
            Playback { start: START, ..Default::default() }).unwrap();
        f.rt.finish_family(next_family).unwrap();
        assert!(!old.contains(&new), "slot reuse keeps generational handles");
        assert!(!old.iter().any(|id| f.rt.voice_active(*id)));
        f.render(&mut out, BLOCK);
        assert!(out.iter().any(|x| x[0] != 0.));
    }
}

#[test]
fn ready_or_started_sources_keep_existing_advancing_starvation_instead_of_replaying() {
    let mut f = Fixture::new(REQUESTED as usize, Some(REQUESTED), false);
    f.layered(); // Fully ready at admission: never attach a held-onset guard.
    let held = f.jobs();
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    for asset in &f.assets { drop(asset.set_ranges(vec![]).unwrap()); }
    f.render(&mut out, BLOCK);
    assert_eq!(f.rt.stream_underruns(), 2);
    assert_eq!(f.cursor(0).1, (START + 2 * BLOCK) as u64);
    // Old: a warm source must never become a held onset. New: losing its pages starves it,
    // and it keeps advancing (asserted above) rather than replaying.
    assert!(f.cursor(0).2, "a warm source that lost its pages is starved and keeps advancing");
    for job in held { f.publish(job); }
    assert!(f.service());
    f.render(&mut out, BLOCK);
    assert_eq!(f.cursor(0).1, (START + 3 * BLOCK) as u64);
    assert_eq!(f.rt.stream_underruns(), 2);
}

#[test]
fn startup_contract_is_bounded_by_reservation_and_immutable_while_voices_live() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    support::without_heap(|| {
        assert_eq!(f.rt.set_stream_start_runway(0), Err(Error::InvalidInput));
        assert_eq!(f.rt.set_stream_start_runway(SERVICE + 1), Err(Error::InvalidInput));
        assert_eq!(f.rt.set_stream_horizon(REQUESTED - 1), Err(Error::InvalidInput));
    });
    f.layered();
    support::without_heap(|| assert_eq!(f.rt.set_stream_start_runway(1), Err(Error::InvalidInput)));
    let held = f.jobs();
    assert_eq!(f.rt.stats().stream_cache_bytes, 8 * PAGE_FRAMES * std::mem::size_of::<Frame>());
    assert_eq!(held.len(), 4, "no separate startup job pool or unbounded retries");
}

#[test]
fn invalidated_decode_serial_does_not_release_a_held_source() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    f.layered();
    let before = f.cursor(0);
    let jobs = f.jobs();
    let stale = PageKey { asset: f.assets[0].asset_id(), index: 1 };
    support::without_heap(|| assert_eq!(f.rt.stream_cache_mut().unwrap().invalidate(stale), Ok(true)));
    for job in jobs { f.publish(job); }
    assert!(!f.service(), "stale completion is discarded; a replacement request is pending");
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    assert_eq!(f.cursor(0), before);
    assert!(out.iter().all(|x| x[0] == 0.));
    assert!(out.iter().all(|x| *x == [0.; 2]), "stale completion holds the cohort");
    let replacement = f.worker.next_job().unwrap();
    assert_eq!(replacement.key(), stale);
    f.publish(replacement);
    assert!(f.service());
    f.render(&mut out, BLOCK);
    assert_eq!(out[0][0].to_bits(), (frame(0, START)[0] * FIRST_FADE).to_bits());
    assert_eq!(f.rt.stream_underruns(), 0);
}

#[test]
fn delayed_start_keeps_its_exact_offset_until_both_deadline_and_runway_are_ready() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    let note = f.rt.note_on(input(1), 60, 1.).unwrap();
    let family = f.rt.create_family(note).unwrap();
    let id = f.rt.start_family(family, 0, 256, 1., Envelope::default(),
        Playback { start: START, ..Default::default() }).unwrap();
    f.rt.finish_family(family).unwrap();
    assert!(!f.service());
    let mut jobs = Vec::new();
    while let Some(job) = f.worker.next_job() { jobs.push(job); }
    assert_eq!(jobs.len(), 2);
    let mut out = [[0.; 2]; 384];
    f.render(&mut out, BLOCK);
    assert_eq!(out, [[0.; 2]; 384]);
    assert_eq!(f.cursor(0), (id, START as u64, true, true));
    for job in jobs { f.publish(job); }
    assert!(f.service());
    f.render(&mut out[..BLOCK], BLOCK);
    assert_eq!(out[0][0].to_bits(), (frame(0, START)[0] * FIRST_FADE).to_bits());
    assert_eq!(f.cursor(0).1, (START + BLOCK) as u64);
    assert_eq!(f.rt.stream_underruns(), 0);
}

#[test]
fn short_service_fragments_still_request_the_admitted_runway_but_zero_frame_poll_does_not() {
    let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
    let note = f.rt.note_on(input(1), 60, 1.).unwrap();
    let expression = f.rt.expression_id(note).unwrap();
    f.rt.set_expression(expression, Expression { pitch_semitones: 24., ..Default::default() }).unwrap();
    let family = f.rt.create_family(note).unwrap();
    for sample in 0..2 {
        f.rt.start_family(family, sample, 0, 1., Envelope::default(),
            Playback { start: START, ..Default::default() }).unwrap();
    }
    f.rt.finish_family(family).unwrap();
    support::without_heap(|| assert_eq!(f.rt.service_streaming(0), Ok(true)));
    assert!(f.worker.next_job().is_none());
    support::without_heap(|| assert_eq!(f.rt.service_streaming(BLOCK as u32), Ok(false)));
    let mut jobs = Vec::new();
    while let Some(job) = f.worker.next_job() { jobs.push(job); }
    assert_eq!(jobs.len(), 4, "1019 frames at 4x need page 2 even for a 128-frame service call");
    assert_eq!(jobs.iter().filter(|j| j.key().index == 2).count(), 2);
    for job in jobs { f.publish(job); }
    support::without_heap(|| assert_eq!(f.rt.service_streaming(BLOCK as u32), Ok(true)));
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    assert!(out.iter().any(|x| *x != [0.; 2]));
    assert_eq!(f.rt.stream_underruns(), 0);
}

#[test]
fn authored_forward_and_reverse_offsets_are_preserved_across_page_handoff() {
    for offset in [START, START + 64, START + 1000, 2 * PAGE_FRAMES + 7] {
        for direction in [Direction::Forward, Direction::Reverse] {
            let mut f = Fixture::new(SELECTED, Some(REQUESTED), false);
            let note = f.rt.note_on(input(1), 60, 1.).unwrap();
            let family = f.rt.create_family(note).unwrap();
            let playback = match direction {
                Direction::Forward => Playback { start: offset, ..Default::default() },
                Direction::Reverse => Playback { end: Some(offset + 1), direction, ..Default::default() },
            };
            let id = f.rt.start_family(family, 0, 0, 1., Envelope::default(), playback).unwrap();
            f.rt.finish_family(family).unwrap();
            assert_eq!(f.cursor(0), (id, offset as u64, true, true));
            assert!(!f.service());
            let mut held = Vec::new();
            while let Some(job) = f.worker.next_job() { held.push(job); }
            let mut out = [[0.; 2]; BLOCK];
            f.render(&mut out, BLOCK);
            assert_eq!(out, [[0.; 2]; BLOCK]);
            assert_eq!(f.cursor(0).1, offset as u64);
            for job in held { f.publish(job); }
            assert!(f.service());
            f.render(&mut out, BLOCK);
            assert_eq!(out[0][0].to_bits(), (frame(0, offset)[0] * FIRST_FADE).to_bits());
            assert_eq!(f.cursor(0).1, match direction {
                Direction::Forward => offset + BLOCK, Direction::Reverse => offset - BLOCK,
            } as u64);
            assert_eq!(f.rt.stream_underruns(), 0);
        }
    }
}

#[test]
fn held_voice_modulation_requests_the_pages_required_by_its_rendered_runway() {
    for chain in [false, true] {
        let mut f = Fixture::new_with_pitch(SELECTED, Some(REQUESTED), chain, 48.);
        f.layered();
        // Initial admission/service have not projected voice modulation yet.
        for job in f.jobs() { f.publish(job); }
        assert!(f.service());
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out, [[0.; 2]; BLOCK]);
        assert_eq!(f.rt.stream_underruns(), 0);
        // Rendering projected 16x pitch: its 1019-frame runway reaches page 3.
        // R16 incorrectly reports ready here and never requests that page.
        support::without_heap(|| assert_eq!(f.rt.service_streaming(BLOCK as u32), Ok(false)));
        let mut jobs = Vec::new();
        while let Some(job) = f.worker.next_job() { jobs.push(job); }
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|j| j.key().index == 3));
        for job in jobs { f.publish(job); }
        support::without_heap(|| assert_eq!(f.rt.service_streaming(BLOCK as u32), Ok(true)));
        f.render(&mut out, BLOCK);
        assert!(out.iter().any(|x| *x != [0.; 2]));
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

fn stateful() -> Vec<Processor> {
    vec![Processor::Biquad(Biquad::new(RATE, FilterKind::LowPass, 2400., 0.7).unwrap())]
}

#[test]
fn held_live_script_pitch_requests_actual_page_keys_and_audible_output() {
    for chain in [false, true] {
        let mut f = Fixture::new(SELECTED, Some(REQUESTED), chain);
        let (note, _) = f.layered();
        for job in f.jobs() { f.publish(job); }
        assert!(f.service());
        support::without_heap(|| f.rt.set_note_param_with_immediate(
            note, ModTarget::Pitch, 48., false, true).unwrap());
        let mut out = [[0.; 2]; BLOCK];
        f.render(&mut out, BLOCK);
        assert_eq!(out, [[0.; 2]; BLOCK]);
        support::without_heap(|| assert_eq!(f.rt.service_streaming(1), Ok(false)));
        let mut count = 0;
        while let Some(job) = f.worker.next_job() {
            assert_eq!(job.key().index, 3);
            assert!(f.assets.iter().any(|a| a.asset_id() == job.key().asset));
            count += 1; f.publish(job);
        }
        assert_eq!(count, 2);
        support::without_heap(|| assert_eq!(f.rt.service_streaming(1), Ok(true)));
        f.render(&mut out, BLOCK);
        assert!(out.iter().any(|x| x[0] != 0.));
        assert!(out.iter().any(|x| x[1] != 0.));
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

#[test]
fn whole_warm_pcm_and_lifecycle_match_default_explicit_one_and_requested_runway() {
    for chain in [false, true] {
        for from_pages in [false, true] {
          for purge in [false, true] {
            let mut baseline = None;
            for runway in [None, Some(1), Some(REQUESTED)] {
                let envelope = Envelope::one_shot(19, 157, 227);
                let mut f = Fixture::configured(
                    [if from_pages { SELECTED } else { REQUESTED as usize }; 2],
                    runway, chain.then(stateful), envelope, 0., 4);
                if from_pages { f.current_page_ready(); }
                f.layered();
                assert_eq!(f.rt.stats().cold_starts, 0);
                let mut pcm = vec![[0.; 2]; 640];
                let mut lifecycle = Vec::new();
                for (block, part) in pcm.chunks_mut(64).enumerate() {
                    if purge && block == 2 {
                        for asset in &f.assets { drop(asset.set_ranges(vec![]).unwrap()); }
                    }
                    f.render(part, 64);
                    lifecycle.push((f.rt.voice_count(), f.rt.stream_underruns()));
                }
                assert_eq!(f.rt.voice_count(), 0);
                let value = (pcm, lifecycle);
                if let Some(expected) = &baseline { assert_eq!(&value, expected); }
                else { baseline = Some(value); }
            }
          }
        }
    }
}

#[test]
fn finite_envelope_and_stateful_dsp_hold_until_one_absolute_publication_boundary() {
    for chain in [false, true] {
        let mut expected = None;
        for partition in [1, 64, 128, 113] {
            let envelope = Envelope::one_shot(23, 179, 197);
            let mut f = Fixture::configured([SELECTED; 2], Some(REQUESTED),
                chain.then(stateful), envelope, 0., 4);
            f.layered();
            let jobs = f.jobs();
            let before = [f.cursor(0).1, f.cursor(1).1];
            let mut before_publication = [[0.; 2]; 320];
            f.render(&mut before_publication, partition);
            assert_eq!(f.rt.now(), 320);
            assert_eq!(before_publication, [[0.; 2]; 320]);
            assert_eq!([f.cursor(0).1, f.cursor(1).1], before);
            assert_eq!(f.rt.voice_count(), 2);
            for job in jobs { f.publish(job); }
            assert!(f.service());
            let mut pcm = vec![[0.; 2]; 640];
            f.render(&mut pcm, partition);
            assert_eq!(f.rt.voice_count(), 0);
            assert_eq!(f.rt.stream_underruns(), 0);
            if let Some(expected) = &expected { assert_eq!(&pcm, expected); }
            else {
                // Same cold engine/source at its first eligible render, without
                // consuming any attack, one-shot lifetime or filter prefix.
                let mut reference = Fixture::configured([SELECTED; 2], Some(REQUESTED),
                    chain.then(stateful), envelope, 0., 4);
                reference.layered();
                for job in reference.jobs() { reference.publish(job); }
                assert!(reference.service());
                let mut immediate = vec![[0.; 2]; 640];
                reference.render(&mut immediate, partition);
                assert_eq!(pcm, immediate);
                expected = Some(pcm);
            }
        }
    }
}

#[test]
fn distinct_notes_and_distinct_authored_deadlines_do_not_share_a_hold() {
    for separate_note in [false, true] {
        let mut f = Fixture::new(SELECTED, Some(REQUESTED), true);
        let first = f.rt.note_on(input(1), 60, 1.).unwrap();
        let a = f.rt.create_family(first).unwrap();
        let b = if separate_note {
            let other = f.rt.note_on(input(2), 60, 1.).unwrap();
            f.rt.create_family(other).unwrap()
        } else { a };
        let deadline = if separate_note { 0 } else { 64 };
        f.rt.start_family(a, 0, 0, 1., Envelope::default(), Playback { start: START, ..Default::default() }).unwrap();
        f.rt.start_family(b, 1, deadline, 1., Envelope::default(), Playback { start: START, ..Default::default() }).unwrap();
        f.rt.finish_family(a).unwrap();
        if b != a { f.rt.finish_family(b).unwrap(); }
        let mut held = Vec::new();
        for job in f.jobs() {
            if job.key().asset == f.assets[0].asset_id() && job.key().index == 1 { f.publish(job); }
            else { held.push(job); }
        }
        f.service();
        let mut out = [[0.; 2]; 256];
        f.render(&mut out, 113);
        assert!(out.iter().any(|x| x[0] != 0.));
        assert!(out.iter().all(|x| x[1] == 0.));
        assert_eq!(f.cursor(0).1, (START + 256) as u64);
        assert_eq!(f.cursor(1).1, START as u64);
    }
}

#[test]
fn terminal_decoder_failure_retires_every_unsounded_sibling_and_late_jobs_do_not_revive_it() {
    let mut f = Fixture::configured([REQUESTED as usize, SELECTED], Some(REQUESTED),
        Some(stateful()), Envelope::one_shot(31, 271, 151), 0., 4);
    let (_, family) = f.layered();
    let ids = [f.cursor(0).0, f.cursor(1).0];
    let mut jobs = Vec::new();
    assert!(!f.service());
    while let Some(job) = f.worker.next_job() { jobs.push(job); }
    let failing = jobs.iter().position(|j| j.key().asset == f.assets[1].asset_id()
        && j.key().index == 1).unwrap();
    let job = jobs.swap_remove(failing);
    f.worker.complete(job, Err(DecodeFailure::InvalidSamples)).unwrap();
    support::without_heap(|| assert_eq!(f.rt.service_streaming(SERVICE),
        Err(StreamError::DecodeFailed(DecodeFailure::InvalidSamples))));
    assert!(ids.iter().all(|id| !f.rt.voice_active(*id)));
    assert_eq!(f.rt.family_voice_count(family), Err(Error::StaleHandle));
    for job in jobs { f.publish(job); }
    let _ = f.rt.service_streaming(SERVICE);
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    assert_eq!(out, [[0.; 2]; BLOCK]);
    assert_eq!(f.rt.voice_count(), 0);
}

#[test]
fn near_maximum_ping_pong_contract_admits_services_and_releases_with_one_page() {
    let mut f = Fixture::new(0, None, false);
    f.rt.set_stream_horizon(u32::MAX).unwrap();
    f.rt.set_stream_start_runway(u32::MAX).unwrap();
    let note = f.rt.note_on(input(1), 60, 1.).unwrap();
    let family = f.rt.create_family(note).unwrap();
    let playback = Playback { end: Some(32), loop_range: Some(Loop {
        start: 0, end: 4, mode: LoopMode::Continuous, shape: LoopShape::PingPong,
        passes: None,
    }), ..Default::default() };
    support::without_heap(|| {
        f.rt.start_family(family, 0, 0, 1., Envelope::default(), playback).unwrap();
        f.rt.finish_family(family).unwrap();
        assert_eq!(f.rt.service_streaming(1), Ok(false));
        f.rt.render(&mut [[0.; 2]; BLOCK]).unwrap();
    });
    let job = f.worker.next_job().unwrap();
    assert_eq!(job.key().index, 0);
    assert!(f.worker.next_job().is_none());
    f.publish(job);
    support::without_heap(|| assert_eq!(f.rt.service_streaming(1), Ok(true)));
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, 1);
    assert!(out.iter().any(|x| x[0] != 0.));
    assert_eq!(f.rt.stream_underruns(), 0);
    // Service remains bounded after onset; it cannot fall back to an exact
    // u32::MAX-frame loop walk once the pending marker has been cleared.
    support::without_heap(|| assert_eq!(f.rt.service_streaming(u32::MAX), Ok(true)));
}

#[test]
fn unrequestable_conservative_geometry_fails_admission_instead_of_holding_forever() {
    // A complete physical view exceeding this unchanged eight-page pool.
    let huge = Pcm::streamed(RATE, PAGE_FRAMES * 1000).unwrap();
    let plan = Prepared::new(RATE, vec![huge], vec![], 4).unwrap();
    let (cache, _worker) = StreamCache::new(8).unwrap();
    let limits = Limits::for_plan(&plan, 4, 4);
    let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
    rt.set_cold_starts(true);
    rt.set_stream_horizon(u32::MAX).unwrap();
    rt.set_stream_start_runway(u32::MAX).unwrap();
    let note = rt.note_on(input(1), 60, 1.).unwrap();
    let family = rt.create_family(note).unwrap();
    support::without_heap(|| assert_eq!(rt.start_family(family, 0, 0, 1., Envelope::default(),
        Playback { loop_range: Some(Loop { start: 0, end: 4, mode: LoopMode::Continuous,
            shape: LoopShape::PingPong, passes: None }), ..Default::default() }), Err(Error::Capacity)));
    assert_eq!(rt.voice_count(), 0);
}

#[test]
fn cohort_gate_runs_on_the_actual_parallel_path_and_matches_serial_pcm() {
    let mut baseline = None;
    for threads in [1, 2] {
        let mut f = Fixture::configured([SELECTED; 2], Some(REQUESTED),
            Some(stateful()), Envelope::default(), 0., 64);
        f.rt.set_threads(Threads::Fixed(threads));
        for n in 0..32 { f.rt.trigger(input(n), 60, 1.).unwrap(); }
        assert_eq!(f.rt.voice_count(), 64);
        let jobs = f.jobs();
        let mut silent = [[0.; 2]; 320];
        f.render(&mut silent, 64);
        assert_eq!(silent, [[0.; 2]; 320]);
        for job in jobs { f.publish(job); }
        assert!(f.service());
        let mut pcm = vec![[0.; 2]; 512];
        f.render(&mut pcm, 64);
        assert!(pcm.iter().any(|x| *x != [0.; 2]));
        assert_eq!(f.rt.stream_underruns(), 0);
        if threads > 1 { assert!(f.rt.parallel_blocks() > 0); }
        if let Some(expected) = &baseline { assert_eq!(&pcm, expected); }
        else { baseline = Some(pcm); }
    }
}

#[test]
fn release_of_mixed_ready_cohort_has_no_unsounded_dsp_tail_or_late_resurrection() {
    for stop in [false, true] {
        let mut f = Fixture::configured([REQUESTED as usize, SELECTED], Some(REQUESTED),
            Some(stateful()), Envelope::one_shot(23, 179, 197), 0., 4);
        let (_, family) = f.layered();
        let ids = [f.cursor(0).0, f.cursor(1).0];
        let mut jobs = Vec::new();
        f.service();
        while let Some(job) = f.worker.next_job() { jobs.push(job); }
        let mut silent = [[0.; 2]; 320];
        f.render(&mut silent, 113);
        assert_eq!(silent, [[0.; 2]; 320]);
        support::without_heap(|| {
            if stop { f.rt.stop_family(family).unwrap(); }
            else { f.rt.release_family(family).unwrap(); }
        });
        assert!(ids.iter().all(|id| !f.rt.voice_active(*id)));
        for job in jobs { f.publish(job); }
        f.service();
        f.render(&mut silent, 113);
        assert_eq!(silent, [[0.; 2]; 320]);
        assert_eq!(f.rt.voice_count(), 0);
    }
}

#[test]
fn source_ending_before_requested_runway_releases_after_its_finite_footprint() {
    let mut f = Fixture::new(0, Some(REQUESTED), false);
    let note = f.rt.note_on(input(1), 60, 1.).unwrap();
    let family = f.rt.create_family(note).unwrap();
    let id = f.rt.start_family(family, 0, 0, 1., Envelope::default(),
        Playback { start: LENGTH - 8, ..Default::default() }).unwrap();
    f.rt.finish_family(family).unwrap();
    support::without_heap(|| assert_eq!(f.rt.service_streaming(1), Ok(false)));
    let job = f.worker.next_job().unwrap();
    assert_eq!(job.key().index, 3);
    assert!(f.worker.next_job().is_none());
    let mut silent = [[0.; 2]; 320];
    f.render(&mut silent, 113);
    assert_eq!(silent, [[0.; 2]; 320]);
    f.publish(job);
    support::without_heap(|| assert_eq!(f.rt.service_streaming(1), Ok(true)));
    let mut out = [[0.; 2]; BLOCK];
    f.render(&mut out, BLOCK);
    assert!(out[..8].iter().any(|x| x[0] != 0.));
    assert_eq!(out[8..], [[0.; 2]; BLOCK - 8]);
    assert!(!f.rt.voice_active(id));
    assert_eq!(f.rt.stream_underruns(), 0);
}
// Append to R18 candidate crates/sampler-core/tests/stream_start_runway.rs.
// Authored static fixtures only: no compiler or test execution in R19.

#[test]
fn r19_pending_realtime_onsets_release_after_switching_to_offline() {
    for chain in [false, true] {
        let mut f = Fixture::new(SELECTED, Some(REQUESTED), chain);
        f.layered();
        let ids = [f.cursor(0).0, f.cursor(1).0];
        let jobs = f.jobs();
        let mut held = [[0.; 2]; BLOCK];
        f.render(&mut held, 64);
        assert_eq!(held, [[0.; 2]; BLOCK]);
        for job in jobs { f.publish(job); }
        assert!(f.service());
        // Every required job is already complete. This fixture needs no sleeps,
        // background decoder, file access or offline timeout to exercise the bug.
        f.rt.set_offline(true);
        let mut out = [[0.; 2]; 64];
        f.render(&mut out, 64);
        assert!(out.iter().any(|x| *x != [0.; 2]),
            "a ready pending cohort must not stay silent forever in offline mode");
        assert_eq!([f.cursor(0).1, f.cursor(1).1], [(START + 64) as u64; 2]);
        assert!(ids.iter().all(|id| f.rt.voice_active(*id)));
        assert_eq!(f.rt.stream_underruns(), 0);
    }
}

#[test]
fn r19_playing_expression_write_is_projected_before_the_next_service() {
    for chain in [false, true] {
        for runway in [None, Some(1), Some(REQUESTED)] {
            let mut f = Fixture::new(SELECTED, runway, chain);
            let (note, _) = f.layered();
            for job in f.jobs() { f.publish(job); }
            assert!(f.service());
            // Pages 1 and 2 are ready. Arrive exactly 64 frames before page 3,
            // with the onset marker already consumed and no underrun.
            let distance = 3 * PAGE_FRAMES - START - 64;
            let mut prefix = vec![[0.; 2]; distance];
            f.render(&mut prefix, BLOCK);
            assert_eq!([f.cursor(0).1, f.cursor(1).1], [(3 * PAGE_FRAMES - 64) as u64; 2]);
            assert_eq!(f.rt.stream_underruns(), 0);
            let expression = f.rt.expression_id(note).unwrap();
            support::without_heap(|| f.rt.set_expression(expression,
                Expression { pitch_semitones: 48., ..Default::default() }).unwrap());
            // This is an applied current expression, not a future command or a
            // prediction of new voice/script modulation. A 64-frame 16x read
            // needs page 3 before any more audio can safely be produced.
            support::without_heap(|| assert_eq!(f.rt.service_streaming(64), Ok(false)));
            let mut jobs = Vec::new();
            while let Some(job) = f.worker.next_job() { jobs.push(job); }
            assert_eq!(jobs.len(), 2);
            assert!(jobs.iter().all(|job| job.key().index == 3));
            for job in jobs { f.publish(job); }
            support::without_heap(|| assert_eq!(f.rt.service_streaming(64), Ok(true)));
            let mut out = [[0.; 2]; BLOCK];
            f.render(&mut out, BLOCK);
            assert!(out.iter().any(|x| *x != [0.; 2]));
            assert_eq!([f.cursor(0).1, f.cursor(1).1],
                [(3 * PAGE_FRAMES - 64 + 16 * BLOCK) as u64; 2]);
            assert_eq!(f.rt.stream_underruns(), 0);
        }
    }
}
