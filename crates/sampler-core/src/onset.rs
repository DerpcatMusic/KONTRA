//! Pending sampler onsets, grouped once per segment by authored ownership.
//! No cohort owns a voice/plan beyond the segment. Slot generations and family
//! generations are re-read from live arenas; cancellation removes membership.
use crate::{Runtime, prepare::Ranges, source::ReadFrames};
use sampler_pool::SnapshotPin;

#[derive(Clone, Copy)]
struct Probe {
    voice: usize,
    family: (usize, u64),
    at: u64,
    ready: bool,
}
impl Probe {
    fn cohort(self) -> (usize, u64, u64) {
        (self.family.0, self.family.1, self.at)
    }
}

pub(super) struct Storage {
    probes: Vec<Probe>,
    pub heads: Box<[Option<SnapshotPin<Ranges>>]>,
    pub residents: Box<[Option<crate::prepare::ResidentPin>]>,
}
impl Storage {
    pub fn new(voices: usize) -> Self {
        Self {
            probes: Vec::with_capacity(voices),
            heads: (0..voices).map(|_| None).collect(),
            residents: (0..voices).map(|_| None).collect(),
        }
    }
    pub fn clear(&mut self) {
        for p in &self.probes {
            self.heads[p.voice] = None;
            self.residents[p.voice] = None;
        }
        self.probes.clear();
    }
}

impl Runtime {
    pub(super) fn prepare_onsets(&mut self, at: u64, frames: usize) {
        if frames == 0 || self.pending_onsets == 0 {
            return;
        }
        debug_assert!(self.onsets.probes.is_empty());
        // Reuse the offline control prepass: advance() caches on the absolute
        // control grid, so the later scalar/worker prelude cannot double-advance.
        // Only pending sources enter this prepass; playing voices are untouched.
        let mut next = self.voices.first;
        while let Some(i) = next {
            next = self.voices.slots[i].next;
            let v = self.voices.slots[i].value.as_ref().unwrap();
            if !v.started || v.cursor.pending_start_runway() <= 1 {
                continue;
            }
            self.prepare_voice(i, at, frames);
            let v = self.voices.slots[i].value.as_ref().unwrap();
            let f = self.families.get(v.family.0).unwrap();
            let n = self.notes.get(f.note.0).unwrap();
            let source = self.plans.get(n.plan.0).unwrap().source(v.sample);
            let ready = match source {
                Some(crate::SourceStorageRef::Channels(family)) => family.resident_view().frame_count() != 0,
                Some(crate::SourceStorageRef::Stereo(asset)) => {
                    if let Some(frames) = asset.pin_resident() {
                        self.onsets.residents[i] = Some(frames);
                        true
                    } else if let Some(cache) = &self.stream_cache {
                        let head = asset.pin_head();
                        let source = crate::source::PagedFrames { cache: cache.reader(), asset: asset.asset_id(), head: &head };
                        let ready = v.cursor.reservation_pages(self.stream_horizon) <= cache.capacity()
                            && v.cursor.runway_ready(v.cursor.pending_start_runway(), v.envelope, |range| source.covers(range));
                        self.onsets.heads[i] = Some(head);
                        ready
                    } else { false }
                }
                None => false,
            };
            // Exactly one entry per live voice. Growth installs matching storage.
            debug_assert!(self.onsets.probes.len() < self.onsets.probes.capacity());
            self.onsets.probes.push(Probe {
                voice: i,
                family: (v.family.0.index, v.family.0.generation),
                at: v.start_at,
                ready,
            });
        }
        // In-place worst-case O(V log V), no family-by-voice scans or heap work.
        // A held family never delays a different deadline, note or release family.
        self.onsets.probes.sort_unstable_by_key(|p| p.cohort());
        let mut begin = 0;
        while begin < self.onsets.probes.len() {
            let key = self.onsets.probes[begin].cohort();
            let mut end = begin + 1;
            let mut ready = self.onsets.probes[begin].ready;
            while end < self.onsets.probes.len() && self.onsets.probes[end].cohort() == key {
                ready &= self.onsets.probes[end].ready;
                end += 1;
            }
            if ready {
                self.pending_onsets -= end - begin;
                for p in &self.onsets.probes[begin..end] {
                    self.voices.slots[p.voice]
                        .value
                        .as_mut()
                        .unwrap()
                        .cursor
                        .admit_start_runway();
                }
            }
            begin = end;
        }
    }
}

impl Runtime {
    pub(super) fn fail_held_onsets(&mut self, cache: &crate::stream::StreamCache,
                                 error: crate::stream::StreamError) {
        debug_assert!(self.onsets.probes.is_empty());
        let mut next = self.voices.first;
        while let Some(i) = next {
            next = self.voices.slots[i].next;
            let v = self.voices.slots[i].value.as_ref().unwrap();
            if !v.held_onset() { continue; }
            let f = self.families.get(v.family.0).unwrap();
            let n = self.notes.get(f.note.0).unwrap();
            let asset = self.plans.get(n.plan.0).unwrap().source(v.sample)
                .and_then(crate::SourceStorageRef::stereo).map(crate::Pcm::asset_id);
            let failed = matches!(error, crate::stream::StreamError::Capacity
                | crate::stream::StreamError::Disconnected | crate::stream::StreamError::SequenceExhausted)
                || asset.is_some_and(|asset| cache.onset_failed(asset));
            if v.cursor.pending_start_runway() <= 1 {
                // Keep legacy default-one terminal failure behavior per source.
                if failed && !matches!(error, crate::stream::StreamError::Capacity
                    | crate::stream::StreamError::SequenceExhausted) {
                    self.mark_processing_onset_failed(i);
                    self.end_voice(crate::VoiceId(self.voices.id(i)));
                }
                continue;
            }
            self.onsets.probes.push(Probe { voice: i,
                family: (v.family.0.index, v.family.0.generation), at: v.start_at, ready: !failed });
        }
        self.onsets.probes.sort_unstable_by_key(|p| p.cohort());
        let mut begin = 0;
        while begin < self.onsets.probes.len() {
            let key = self.onsets.probes[begin].cohort();
            let mut end = begin + 1;
            let mut failed = !self.onsets.probes[begin].ready;
            while end < self.onsets.probes.len() && self.onsets.probes[end].cohort() == key {
                failed |= !self.onsets.probes[end].ready;
                end += 1;
            }
            if failed {
                for p in begin..end {
                    let i = self.onsets.probes[p].voice;
                    self.mark_processing_onset_failed(i);
                    self.end_voice(crate::VoiceId(self.voices.id(i)));
                }
            }
            begin = end;
        }
        self.onsets.probes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    #[test]
    fn cohort_release_and_actual_render_use_the_same_pinned_publication() {
        for chain in [false, true] {
            for resident in [false, true] {
                let assets: Vec<_> = (0..2).map(|channel| {
                    let mut frame = [0.; 2]; frame[channel] = 0.5;
                    if resident {
                        let pcm = Pcm::new(48000, vec![frame; 4096].into_boxed_slice()).unwrap();
                        pcm.residency_acquire();
                        pcm
                    } else {
                        Pcm::headed(48000, 4096, &vec![frame; 2048]).unwrap()
                    }
                }).collect();
                let mut plan = Prepared::new(48000, assets.clone(), vec![], 4).unwrap();
                if chain {
                    plan = plan.with_voice_chains(vec![VoiceChain::new(
                        vec![Processor::Gain(1.)], vec![], 0).unwrap()], vec![]).unwrap();
                }
                let limits = Limits::for_plan(&plan, 4, 4);
                let (cache, _worker) = StreamCache::new(8).unwrap();
                let mut rt = Runtime::new(plan, limits).unwrap().with_stream_cache(cache);
                rt.set_cold_starts(true);
                rt.set_stream_horizon(1024).unwrap();
                rt.set_stream_start_runway(1019).unwrap();
                let note = rt.note_on(Input { protocol: Protocol::Native, port: 0, group: 0,
                    channel: 0, key: 60, external_id: Some(1) }, 60, 1.).unwrap();
                let family = rt.create_family(note).unwrap();
                for sample in 0..2 {
                    let id = rt.start_family(family, sample, 0, 1., Envelope::default(), Playback::default()).unwrap();
                    // Explicit starts have no region to attach the prepared chain.
                    if chain { rt.voices.get_mut(id.0).unwrap().chain = Some(0); }
                }
                rt.finish_family(family).unwrap();
                rt.prepare_onsets(0, 64);
                assert_eq!(rt.pending_onsets, 0);
                // Publish a purge after the cohort decision. Retired storage is
                // pinned across every source read, including the batched path.
                for asset in &assets {
                    if resident { asset.residency_release(); asset.residency_collect(); }
                    else { drop(asset.set_ranges(vec![]).unwrap()); }
                }
                let mut out = [[0.; 2]; 64];
                rt.render_prepared_voices::<false>(&mut out, 0);
                rt.onsets.clear();
                assert_eq!(out, [[0.5; 2]; 64]);
                assert_eq!(rt.stream_underruns(), 0);
                assert_eq!(rt.pending_onsets, 0);
            }
        }
    }
}

#[cfg(test)]
mod plan_scope_tests {
    use crate::*;
    #[test]
    fn adopted_stretch_plan_preserves_legacy_path_and_cannot_release_old_sampler_generation() {
        let old = Pcm::headed(48000, 4096, &[[0.5, 0.]; 89]).unwrap();
        let old_plan = Prepared::new(48000, vec![old.clone()], vec![], 4).unwrap();
        let limits = Limits::for_plan(&old_plan, 4, 4);
        let (mut rt, mut control) = Runtime::with_plan_updates(old_plan, limits, 3, 1).unwrap();
        let (cache, mut worker) = StreamCache::new(8).unwrap();
        rt = rt.with_stream_cache(cache);
        rt.set_cold_starts(true);
        rt.set_stream_horizon(1024).unwrap();
        rt.set_stream_start_runway(1019).unwrap();
        let input = |n| Input { protocol: Protocol::Native, port: 0, group: 0,
            channel: 0, key: 60, external_id: Some(n) };
        let note = rt.note_on(input(1), 60, 1.).unwrap();
        let family = rt.create_family(note).unwrap();
        let held = rt.start_family(family, 0, 0, 1., Envelope::default(), Playback::default()).unwrap();
        rt.finish_family(family).unwrap();
        let old_generation = rt.note_plan(note).unwrap();
        let new = Pcm::headed(48000, 4096, &[[0., 0.25]; 2048]).unwrap();
        let mut new_plan = Prepared::new(48000, vec![new], vec![], 4).unwrap();
        new_plan.stretch_groups = vec![Some(crate::source::StretchSpec {
            mode: crate::source::StretchMode::TimeMachine2, hq: false,
            speed: 1., smooth: 0.5, grain_ms: 20.,
        })].into_boxed_slice();
        let request = control.submit(Box::new(new_plan)).unwrap();
        assert_eq!(rt.poll_plan_update(), Ok(Some(request)));
        assert_eq!(rt.note_plan(note), Ok(old_generation));
        let next_note = rt.note_on(input(2), 60, 1.).unwrap();
        let next_family = rt.create_family(next_note).unwrap();
        let warm = rt.start_family(next_family, 0, 0, 1., Envelope::default(), Playback::default()).unwrap();
        rt.finish_family(next_family).unwrap();
        assert_eq!(rt.voices.get(warm.0).unwrap().cursor.pending_start_runway(), 0);
        assert!(!rt.voices.get(warm.0).unwrap().cursor.bounded_demand());
        assert_eq!(rt.service_streaming(1), Ok(false));
        let job = worker.next_job().unwrap();
        assert_eq!(job.key().asset, old.asset_id());
        let mut out = [[0.; 2]; 64];
        rt.render(&mut out).unwrap();
        assert_eq!(out, [[0., 0.25]; 64]);
        assert_eq!(rt.voices.get(held.0).unwrap().cursor.trace_position(), 0);
        assert!(rt.voices.get(held.0).unwrap().held_onset());
    }
}
