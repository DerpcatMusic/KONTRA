//! Pending sampler onsets, grouped once per segment by authored ownership.
//! No cohort owns a voice/plan beyond the segment. Slot generations and family
//! generations are re-read from live arenas; cancellation removes membership.
use crate::{Runtime, prepare::Ranges, source::ReadFrames};
use sampler_pool::SnapshotPin;

#[derive(Clone, Copy)]
struct Probe {
    voice: usize,
    /// Owning immutable generation, which may become retired before cleanup.
    plan: crate::PlanId,
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
    /// Called after joined source rendering. The recorded generation/index
    /// also names heads of a voice ended or moved to a retired plan this segment.
    /// SnapshotPin release only decrements reader authority; the source's
    /// ordinary offaudio collector owns retired heavy storage.
    pub(super) fn clear_onsets(&mut self) {
        for probe in &self.onsets.probes {
            if let Some(generation)=self.plans.get(probe.plan.0) {
                generation.dsp.channel_heads.claim(probe.voice).fill_with(|| None);
            }
        }
        self.onsets.clear();
    }
    /// Default-one cold processing sources need an owner admission before
    /// the owner hold test: raw rendering cannot clear a hold if that test
    /// prevents raw rendering. Larger startup runways remain the ordinary
    /// take/family cohort's exclusive admission decision.
    pub(crate) fn prepare_processing_cold_start(&mut self, owner:&crate::topology::ProcessingOwner,
        at:u64,frames:usize)->bool {
        let held=owner.members.iter().flatten().any(|id|self.voices.get(id.0)
            .is_some_and(|v|v.cursor.holding_onset()));
        if !held {return true;}
        if owner.members.iter().flatten().any(|id|self.voices.get(id.0)
            .is_some_and(|v|!v.started || v.cursor.pending_start_runway()>1)) {return false;}
        let mut ready=true;
        for id in owner.members.iter().flatten() {
            if self.voices.get(id.0).is_none() {continue;}
            // prepare_voice first resolves and prepares the SAME provider
            // owner row before inspecting the physical cursor's pitched runway.
            if self.prepare_voice(id.0.index,at,frames).is_none() {ready=false;continue;}
            let Some(v)=self.voices.get(id.0) else {ready=false;continue;};
            let Some(generation)=self.plans.get(owner.plan.0) else {return false;};
            let Some(source)=generation.source(v.sample) else {ready=false;continue;};
            let mut staged=false;
            let member_ready=match source {
                crate::SourceStorageRef::Stereo(asset)=> {
                    if let Some(resident)=self.onsets.residents[id.0.index].as_ref() {
                        v.cursor.runway_ready(1,v.envelope,|range|resident.covers(range))
                    } else if let Some(head)=self.onsets.heads[id.0.index].as_ref() {
                        self.stream_cache.as_ref().is_some_and(|cache| {
                            let view=crate::source::PagedFrames {cache:cache.reader(),asset:asset.asset_id(),head};
                            v.cursor.runway_ready(1,v.envelope,|range|view.covers(range))
                        })
                    } else if let Some(resident)=asset.pin_resident() {
                        let yes=v.cursor.runway_ready(1,v.envelope,|range|resident.covers(range));
                        self.onsets.residents[id.0.index]=Some(resident);staged=true;yes
                    } else if let Some(cache)=&self.stream_cache {
                        let head=asset.pin_head();
                        let view=crate::source::PagedFrames {cache:cache.reader(),asset:asset.asset_id(),head:&head};
                        let yes=v.cursor.runway_ready(1,v.envelope,|range|view.covers(range));
                        self.onsets.heads[id.0.index]=Some(head);staged=true;yes
                    } else {false}
                }
                crate::SourceStorageRef::Channels(family) if family.is_paged()=> {
                    if let Some(cache)=&self.stream_cache {
                        let count=family.packs().len();let mut heads=generation.dsp.channel_heads.claim(id.0.index);
                        if heads.len()<count {false} else {
                            // Complete prepass pins are authoritative even if a
                            // newer head was published since cohort admission.
                            if heads[..count].iter().any(Option::is_none) {
                                heads.fill_with(||None);
                                for (slot,pack) in heads.iter_mut().zip(family.packs()) {*slot=Some(pack.pin_head());}
                                staged=true;
                            }
                            family.packs().iter().zip(heads.iter()).all(|(pack,head)| {
                                head.as_ref().is_some_and(|head| {
                                    let view=crate::source::PagedFrames {cache:cache.reader(),asset:pack.asset_id(),head};
                                    v.cursor.runway_ready(1,v.envelope,|range|view.covers(range))
                                })
                            })
                        }
                    } else {false}
                }
                crate::SourceStorageRef::Channels(family)=>crate::SourceStorageRef::Channels(family).resident_view().is_some(),
            };
            ready &=member_ready;
            if staged {
                debug_assert!(self.onsets.probes.len()<self.onsets.probes.capacity());
                self.onsets.probes.push(Probe {voice:id.0.index,plan:owner.plan,
                    family:(v.family.0.index,v.family.0.generation),at:v.start_at,ready:member_ready});
            }
        }
        if ready {
            for id in owner.members.iter().flatten() {
                if let Some(v)=self.voices.get_mut(id.0) {v.cursor.admit_processing_cold_start();}
            }
        }
        ready
    }
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
                Some(crate::SourceStorageRef::Channels(source)) => {
                    if !source.is_paged() { crate::SourceStorageRef::Channels(source).resident_view().is_some() }
                    else if let Some(cache) = &self.stream_cache {
                        let count = source.packs().len();
                        let reservation = v.cursor.family_reservation_pages(self.stream_horizon).checked_mul(count);
                        let generation=self.plans.get(n.plan.0).unwrap();
                        let mut heads=generation.dsp.channel_heads.claim(i);
                        heads.fill_with(|| None);
                        let mut ready=reservation.is_some_and(|n|n<=cache.capacity()) && heads.len()>=count;
                        if heads.len()>=count {
                            for (slot,asset) in heads.iter_mut().zip(source.packs()) {
                                *slot=Some(asset.pin_head());
                            }
                            for (asset,head) in source.packs().iter().zip(heads.iter()) {
                                let Some(head)=head.as_ref() else {ready=false;continue;};
                                let view=crate::source::PagedFrames {cache:cache.reader(),asset:asset.asset_id(),head};
                                ready &= v.cursor.runway_ready(v.cursor.pending_start_runway(),v.envelope,|range|view.covers(range));
                            }
                        }
                        ready
                    } else { false }
                },
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
                plan: n.plan,
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
                    self.max_onset_wait_frames =
                        self.max_onset_wait_frames.max(at.saturating_sub(p.at));
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
            let source=self.plans.get(n.plan.0).unwrap().source(v.sample);
            let failed = matches!(error, crate::stream::StreamError::Capacity
                | crate::stream::StreamError::Disconnected | crate::stream::StreamError::SequenceExhausted)
                || source.is_some_and(|source|(0..source.physical_count()).any(|i|
                    source.physical(i).is_some_and(|asset|cache.onset_failed(asset.asset_id()))));
            if v.cursor.pending_start_runway() <= 1 {
                // Keep legacy default-one terminal failure behavior per source.
                if failed && !matches!(error, crate::stream::StreamError::Capacity
                    | crate::stream::StreamError::SequenceExhausted) {
                    self.mark_processing_onset_failed(i);
                    self.end_voice(crate::VoiceId(self.voices.id(i)));
                }
                continue;
            }
            self.onsets.probes.push(Probe { voice: i, plan: n.plan,
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
                rt.clear_onsets();
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

#[cfg(test)]
mod wide_publication_tests {
    use crate::*;
    use sampler_ir as ir;
    fn fixture()->(Prepared,Vec<Pcm>) {
        let packs=(0..3).map(|pair|Pcm::headed(48000,4096,&vec![[0.0625*(pair+1) as f32,0.125*(pair+1) as f32];2048]).unwrap()).collect::<Vec<_>>();
        let family=SourceStorageFamily::new_paged(ChannelCount::new(6).unwrap(),packs.clone()).unwrap();
        let sibling=Pcm::headed(48000,4096,&[[0.03125,-0.0625];2048]).unwrap();
        let mut all=packs;all.push(sibling.clone());
        let asset=|i|ir::Asset {location:ir::AssetLocation::Path(format!("owned-{i}.wav")),encoding:ir::Encoding::Wav,
            residency:ir::AssetResidency::Streamable,root_key:None,loops:vec![]};
        let zone=|i|ir::Zone {pitch:ir::KeyTracking::Fixed,velocity:ir::VelocityResponse::None,..ir::Zone::new(ir::AssetRef(i))};
        let mut cells=vec![0.;12];cells[3]=1.;cells[11]=1.;
        let instrument=ir::Instrument {assets:vec![asset(0),asset(1)],zones:vec![zone(0),zone(1)],
            processing_scopes:vec![ir::ProcessingScope {members:vec![ir::ZoneRef(0)],sum:ir::ProcessingSumLaw::OrderedF32,
                cadence:ir::ProcessingCadence::CoreCell64,chain:None,amplitude:None,output:None,
                signal:Some(ir::ProcessingSignal {source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,
                    input:ir::SignalLayout {channels:6},controls:vec![],nodes:vec![
                        ir::ProcessingSignalNode::OnePole(ir::ChannelOnePole {cutoff_hz:ir::SignalParameter {constant:1379.,control:None,expression:None},
                            mode:ir::ChannelOnePoleMode::LowPass,arithmetic:ir::ChannelOnePoleArithmetic::CoreOnePoleF64,tail_frames:0}),
                        ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix {input:ir::SignalLayout {channels:6},output:ir::SignalLayout {channels:2},
                            cells,arithmetic:ir::MatrixArithmetic::SeparateMulAddF32})]})}],..Default::default()};
        let plan=crate::lower::lower_sources(&instrument,48000,vec![family.into(),sibling.into()],|_,p|Ok::<_,crate::lower::LowerError>(p)).unwrap();
        (plan,all)
    }
    fn start(rt:&mut Runtime) {
        rt.set_cold_starts(true);rt.set_stream_horizon(1024).unwrap();rt.set_stream_start_runway(1019).unwrap();
        rt.trigger(Input {protocol:Protocol::Native,port:0,group:0,channel:0,key:60,external_id:Some(77)},60,1.).unwrap();
        rt.prepare_onsets(0,64);assert_eq!(rt.pending_onsets,0);
    }
    #[test]
    fn wide_cohort_render_reuses_the_exact_retained_middle_head_publication() {
        let (plan,packs)=fixture();let (reference_plan,_reference_packs)=fixture();let limits=Limits::for_plan(&plan,8,8);
        let (cache,_worker)=StreamCache::new(32).unwrap();let mut actual=Runtime::new(plan,limits).unwrap().with_stream_cache(cache);
        let (cache,_worker2)=StreamCache::new(32).unwrap();let mut expected=Runtime::new(reference_plan,limits).unwrap().with_stream_cache(cache);
        start(&mut actual);start(&mut expected);
        // A normal residency/head publication after cohort admission cannot
        // replace any admitted pair. Retire only the middle original pack.
        packs[1].set_ranges(vec![]).unwrap();packs[1].set_ranges(vec![(0,vec![[0.9,-0.8];2048].into_boxed_slice())]).unwrap();
        let mut a=[[0.;2];64];let mut b=a;
        actual.render_prepared_voices::<false>(&mut a,0);expected.render_prepared_voices::<false>(&mut b,0);
        assert_eq!(a,b);assert!(a.iter().flatten().any(|v|*v!=0.));
        let state=|rt:&Runtime|rt.voices.slots.iter().filter_map(|s|s.value.as_ref().map(|v|(v.cursor.trace_position(),v.envelope.remaining()))).collect::<Vec<_>>();
        assert_eq!(state(&actual),state(&expected));
        for (a,b) in actual.processing_owners.slots.iter().zip(&expected.processing_owners.slots) {
            if let (Some(a),Some(b))=(&a.value,&b.value) {assert_eq!(a.envelope_remaining_for_test(),b.envelope_remaining_for_test());}
        }
        let a=actual.plans.get(actual.active_plan.0).unwrap();let b=expected.plans.get(expected.active_plan.0).unwrap();
        assert_eq!(&*a.dsp.owner_signal_history.claim(0),&*b.dsp.owner_signal_history.claim(0));
        actual.clear_onsets();expected.clear_onsets();
        assert!(actual.onsets.probes.is_empty());assert_eq!(actual.stream_underruns(),0);
    }
    #[test]
    fn wide_onset_cleanup_names_the_retired_generation_even_after_voice_end() {
        for end_before_cleanup in [false,true] {
            let (plan,_packs)=fixture();let limits=Limits::for_plan(&plan,8,8);
            let (mut rt,mut control)=Runtime::with_plan_updates(plan,limits,2,1).unwrap();let (cache,_worker)=StreamCache::new(32).unwrap();rt=rt.with_stream_cache(cache);
            start(&mut rt);let old=rt.active_plan();
            let old_voice=rt.onsets.probes[0].voice;
            assert!(rt.plans.get(old.0).unwrap().dsp.channel_heads.claim(old_voice).iter().any(Option::is_some));
            let (incoming,_new_packs)=fixture();control.submit(Box::new(incoming)).unwrap();rt.poll_plan_update().unwrap();assert_ne!(rt.active_plan(),old);
            if end_before_cleanup {rt.panic();}
            rt.clear_onsets();assert!(rt.onsets.probes.is_empty());
            if let Some(g)=rt.plans.get(old.0) {assert!(g.dsp.channel_heads.claim(old_voice).iter().all(Option::is_none));}
            assert!(rt.plans.get(rt.active_plan().0).unwrap().dsp.channel_heads.claim(old_voice).iter().all(Option::is_none));
        }
    }
}
