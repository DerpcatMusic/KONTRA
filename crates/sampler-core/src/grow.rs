//! Growing the voice pool while rendering. The control thread builds larger
//! per-voice storage (`PlanControl::grow_voices`); the audio thread swaps it
//! in at the start of a block, moving every live voice's state across with
//! no allocation, and sends the old storage back to be freed.

use super::*;
use crate::{
    dsp::DspState,
    parallel,
    voice_mod::{ModShape, VoiceModState},
};
use rtrb::{Consumer, Producer};
use std::sync::{Arc, atomic::AtomicBool};

/// What sizes one live plan's per-voice state.
#[derive(Clone, Copy)]
pub(crate) struct Dims {
    pub request: u64,
    chain: (usize, usize),
    source_channels: usize,
    modulation: ModShape,
}

impl Dims {
    pub fn of(request: u64, plan: &Prepared) -> Self {
        Self {
            request,
            chain: DspState::shape(plan),
            source_channels: plan.signal_channels(),
            modulation: plan.voice_modulation.shape(),
        }
    }
}

struct PlanStorage {
    request: u64,
    cells: sampler_pool::Slab<dsp::ProcessorState>,
    delays: sampler_pool::Slab<[f64; 2]>,
    channels: dsp::ChannelVoiceStorage,
    modulation: VoiceModState,
}

/// Larger per-voice storage for every live plan, in flight to the audio
/// thread and back (holding the old storage afterwards).
pub(crate) struct Growth {
    voices: usize,
    slots: Box<[Slot<Voice>]>,
    free: Box<[u64]>,
    activity: Box<[u64]>,
    onsets: onset::Storage,
    scratch: Option<parallel::Scratch>,
    plans: Vec<PlanStorage>,
    note_params: Option<crate::script_params::NoteParamsGrowth>,
    #[cfg(test)]
    drop_probe: Option<tests::DropProbe>,
}

impl Growth {
    /// Control side: allocates.
    pub fn build(voices: usize, live: &[Dims], parallel: bool) -> Result<Self, Error> {
        let (slots, free) = Arena::<Voice>::blank(voices);
        let plans = live
            .iter()
            .map(|d| {
                let (cells, delays) = DspState::voice_storage(d.chain, voices)?;
                Ok(PlanStorage {
                    request: d.request,
                    cells,
                    delays,
                    channels: DspState::channel_voice_storage(d.source_channels, voices)?,
                    modulation: VoiceModState::with_shape(d.modulation, voices)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            voices,
            slots,
            free,
            activity: vec![0; voices.div_ceil(64)].into_boxed_slice(),
            onsets: onset::Storage::new(voices),
            scratch: parallel.then(|| parallel::Scratch::new(voices)),
            plans,
            note_params: None,
            #[cfg(test)]
            drop_probe: None,
        })
    }

    pub fn note_params(growth: crate::script_params::NoteParamsGrowth) -> Self {
        Self {
            voices: 0,
            slots: Box::new([]),
            free: Box::new([]),
            activity: Box::new([]),
            onsets: onset::Storage::new(0),
            scratch: None,
            plans: Vec::new(),
            note_params: Some(growth),
            #[cfg(test)]
            drop_probe: None,
        }
    }
}

/// Audio-side ends of the growth channel.
pub(crate) struct GrowthQueues {
    pub incoming: Consumer<Growth>,
    pub outgoing: Producer<Growth>,
    /// A blocked return keeps its owner here until control drains the queue.
    /// Runtime and both endpoints must be destroyed off audio after processing stops.
    pub pending_return: Option<Growth>,
    /// The control-side thread to wake when the pool fills or a growth returns.
    pub waker: Option<std::thread::Thread>,
}

impl Runtime {
    /// Slots in the voice pool.
    pub fn voice_capacity(&self) -> usize {
        self.voices.slots.len()
    }

    pub fn note_params_capacity(&self) -> usize {
        self.note_params.capacity()
    }

    /// Control side: wake `thread` (an unpark, which is allocation free) when
    /// the voice pool fills up and when a growth comes back, so it can sleep
    /// until there is work for `PlanControl::grow_voices`. Needs a runtime
    /// from `with_plan_updates`.
    /// Install or replace the waker off audio: replacing a `Thread` may release
    /// its last shared owner.
    pub fn set_growth_waker(&mut self, thread: std::thread::Thread) {
        if let Some(queues) = &mut self.growth {
            queues.waker = Some(thread);
        }
    }

    /// Adopt a queued growth, if any. Audio thread: no allocation.
    pub(super) fn apply_growth(&mut self) {
        let Some(queues) = &mut self.growth else {
            return;
        };
        if let Some(growth) = queues.pending_return.take() {
            if let Err(rtrb::PushError::Full(growth)) = queues.outgoing.push(growth) {
                queues.pending_return = Some(growth);
            }
            if let Some(thread) = &queues.waker {
                thread.unpark();
            }
            // Even a successful flush ends this callback's growth work. The
            // next owner cannot overwrite a blocked return or delay its retirement.
            return;
        }
        let Ok(mut growth) = queues.incoming.pop() else {
            return;
        };
        if self.grow_with(&mut growth) {
            if growth.voices != 0 {
                self.voice_growths += 1;
            }
        } else {
            self.growth_failures += 1;
        }
        let queues = self.growth.as_mut().unwrap();
        // PlanControl admits only one in-flight growth. Preserve ownership even
        // if a future/internal producer violates that invariant.
        if let Err(rtrb::PushError::Full(growth)) = queues.outgoing.push(growth) {
            queues.pending_return = Some(growth);
        }
        if let Some(thread) = &self.growth.as_ref().unwrap().waker {
            thread.unpark();
        }
    }

    fn grow_with(&mut self, g: &mut Growth) -> bool {
        if let Some(params) = &mut g.note_params {
            let adopted = self.note_params.adopt(params);
            if adopted {
                self.note_pressure.store(0, Ordering::Relaxed);
            }
            return adopted;
        }
        let old = self.voices.slots.len();
        if g.voices <= old {
            return false;
        }
        let parallel_ready = match &self.parallel {
            Some(p) => g.scratch.is_some() && p.voices() < g.voices,
            None => true,
        };
        // Every plan generation still sized for fewer voices needs its storage.
        let covered = self
            .plans
            .slots
            .iter()
            .filter_map(|s| s.value.as_ref())
            .all(|gen_| {
                gen_.dsp.voices >= g.voices || g.plans.iter().any(|p| p.request == gen_.request)
            });
        if !parallel_ready || !covered {
            return false;
        }
        for gen_ in self.plans.slots.iter_mut().filter_map(|s| s.value.as_mut()) {
            if gen_.dsp.voices >= g.voices {
                continue;
            }
            let p = g
                .plans
                .iter_mut()
                .find(|p| p.request == gen_.request)
                .unwrap();
            gen_.dsp.adopt(g.voices, &mut p.cells, &mut p.delays);
            gen_.dsp.adopt_channel_voices(&mut p.channels);
            gen_.modulation.adopt(&mut p.modulation);
        }
        // Scratch contains no outstanding pins outside render_voices.
        std::mem::swap(&mut self.onsets, &mut g.onsets);
        self.voices.grow(&mut g.slots, &mut g.free);
        g.activity[..self.voice_activity.len()].swap_with_slice(&mut self.voice_activity);
        std::mem::swap(&mut self.voice_activity, &mut g.activity);
        if let (Some(p), Some(s)) = (&mut self.parallel, &mut g.scratch) {
            p.swap_scratch(s);
        }
        true
    }

    /// Set when the pool is three quarters full; the control side grows it.
    pub(super) fn note_voice_pressure(&self) {
        if self.voices.available() * 4 <= self.voices.slots.len()
            && !self.voice_pressure.swap(true, Ordering::Relaxed)
            && let Some(thread) = self.growth.as_ref().and_then(|q| q.waker.as_ref())
        {
            thread.unpark();
        }
    }

    pub(super) fn note_note_pressure(&self) {
        let capacity = self.note_params.capacity();
        if self.notes.occupied * 4 >= capacity * 3
            && self.note_pressure.swap(capacity, Ordering::Relaxed) != capacity
            && let Some(thread) = self.growth.as_ref().and_then(|q| q.waker.as_ref())
        {
            thread.unpark();
        }
    }
}

pub(crate) type Pressure = Arc<AtomicBool>;

/// Capacity against which audio observed note pressure; zero means none.
pub(crate) type NotePressure = Arc<std::sync::atomic::AtomicUsize>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DropState {
        drops: AtomicUsize,
        callback_drops: AtomicUsize,
        wrong_thread_drops: AtomicUsize,
        in_callback: AtomicBool,
        control_thread: std::thread::ThreadId,
    }

    impl DropState {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                drops: AtomicUsize::new(0),
                callback_drops: AtomicUsize::new(0),
                wrong_thread_drops: AtomicUsize::new(0),
                in_callback: AtomicBool::new(false),
                control_thread: std::thread::current().id(),
            })
        }
    }

    pub(super) struct DropProbe(Arc<DropState>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.drops.fetch_add(1, Ordering::Relaxed);
            if self.0.in_callback.load(Ordering::Relaxed) {
                self.0.callback_drops.fetch_add(1, Ordering::Relaxed);
            }
            if std::thread::current().id() != self.0.control_thread {
                self.0.wrong_thread_drops.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn tracked(mut growth: Growth, state: &Arc<DropState>) -> Growth {
        growth.drop_probe = Some(DropProbe(state.clone()));
        growth
    }

    fn fixture() -> (Runtime, Dims, Producer<Growth>, Consumer<Growth>) {
        let plan = Prepared::new(48000, vec![], vec![], 0).unwrap();
        let dims = Dims::of(0, &plan);
        let limits = Limits::for_plan(&plan, 512, 2);
        let mut runtime = Runtime::new_with_note_params(plan, limits, 128, &|| false).unwrap();
        let (submit, incoming) = rtrb::RingBuffer::new(1);
        let (outgoing, returned) = rtrb::RingBuffer::new(1);
        runtime.growth = Some(GrowthQueues {
            incoming,
            outgoing,
            pending_return: None,
            waker: None,
        });
        (runtime, dims, submit, returned)
    }

    fn callback(runtime: &mut Runtime, state: &DropState) {
        struct CallbackScope<'a>(&'a AtomicBool);
        impl Drop for CallbackScope<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Relaxed);
            }
        }
        assert!(!state.in_callback.swap(true, Ordering::Relaxed));
        let scope = CallbackScope(&state.in_callback);
        runtime.apply_growth();
        drop(scope);
    }

    fn assert_reclaimed(state: &DropState, expected: usize) {
        assert_eq!(state.callback_drops.load(Ordering::Relaxed), 0);
        assert_eq!(state.wrong_thread_drops.load(Ordering::Relaxed), 0);
        assert_eq!(
            state.drops.load(Ordering::Relaxed),
            expected,
            "each returned owner must be destroyed on control, never forgotten"
        );
    }

    #[test]
    fn full_return_queue_reclaims_every_growth_on_control() {
        // Public PlanControl cannot create this state: growing gates its sole
        // request until it drains the previous return. Deliberately inject a
        // full return queue to exercise defensive ownership, not public reachability.
        for note_pages in [false, true] {
            let (mut runtime, dims, mut submit, mut returned) = fixture();
            let state = DropState::new();
            let older = tracked(Growth::build(2, &[dims], false).unwrap(), &state);
            assert!(
                runtime
                    .growth
                    .as_mut()
                    .unwrap()
                    .outgoing
                    .push(older)
                    .is_ok()
            );
            let next = if note_pages {
                Growth::note_params(
                    crate::script_params::NoteParamsGrowth::build(128, 256, 512).unwrap(),
                )
            } else {
                Growth::build(4, &[dims], false).unwrap()
            };
            assert!(submit.push(tracked(next, &state)).is_ok());
            callback(&mut runtime, &state);
            assert_eq!(runtime.voice_capacity(), if note_pages { 2 } else { 4 });
            assert_eq!(
                runtime.note_params_capacity(),
                if note_pages { 256 } else { 128 }
            );
            assert_eq!(state.drops.load(Ordering::Relaxed), 0);
            drop(returned.pop().unwrap()); // control drains the previously full slot
            callback(&mut runtime, &state);
            drop(returned.pop().ok()); // the old mem::forget path loses this owner
            assert_reclaimed(&state, 2);
        }
    }

    #[test]
    fn pending_return_stops_growth_and_flushes_alone_even_on_success() {
        let (mut runtime, dims, mut submit, mut returned) = fixture();
        let state = DropState::new();
        let older = tracked(Growth::build(2, &[dims], false).unwrap(), &state);
        assert!(
            runtime
                .growth
                .as_mut()
                .unwrap()
                .outgoing
                .push(older)
                .is_ok()
        );
        assert!(
            submit
                .push(tracked(Growth::build(4, &[dims], false).unwrap(), &state))
                .is_ok()
        );
        callback(&mut runtime, &state);
        assert!(
            submit
                .push(tracked(Growth::build(8, &[dims], false).unwrap(), &state))
                .is_ok()
        );
        for _ in 0..4 {
            callback(&mut runtime, &state);
            assert_eq!(
                runtime.voice_capacity(),
                4,
                "retirement backpressure must stop adoption"
            );
            assert!(!runtime.growth.as_ref().unwrap().incoming.is_empty());
            assert_eq!(state.drops.load(Ordering::Relaxed), 0);
        }
        drop(returned.pop().unwrap());
        callback(&mut runtime, &state);
        assert_eq!(
            runtime.voice_capacity(),
            4,
            "a successful flush still ends this callback's growth work"
        );
        assert!(!runtime.growth.as_ref().unwrap().incoming.is_empty());
        drop(returned.pop().unwrap());
        callback(&mut runtime, &state);
        assert_eq!(runtime.voice_capacity(), 8);
        drop(returned.pop().unwrap());
        assert_reclaimed(&state, 3);
    }

    #[test]
    fn refused_growth_and_disconnected_return_remain_owned_until_off_audio_shutdown() {
        for note_pages in [false, true] {
            let (mut runtime, dims, mut submit, returned) = fixture();
            let state = DropState::new();
            let older = tracked(Growth::build(2, &[dims], false).unwrap(), &state);
            assert!(
                runtime
                    .growth
                    .as_mut()
                    .unwrap()
                    .outgoing
                    .push(older)
                    .is_ok()
            );
            let refused = if note_pages {
                Growth::note_params(
                    crate::script_params::NoteParamsGrowth::build(256, 512, 512).unwrap(),
                )
            } else {
                Growth::build(2, &[dims], false).unwrap()
            };
            assert!(submit.push(tracked(refused, &state)).is_ok());
            drop(returned); // dropping this first endpoint must not free either payload
            callback(&mut runtime, &state);
            for _ in 0..4 {
                callback(&mut runtime, &state);
            }
            assert_eq!(runtime.voice_capacity(), 2);
            assert_eq!(runtime.note_params_capacity(), 128);
            assert_eq!(runtime.stats().growth_failures, 1);
            assert_eq!(state.drops.load(Ordering::Relaxed), 0);
            drop(runtime); // explicitly off callback; last ring endpoint may now reclaim
            drop(submit);
            assert_reclaimed(&state, 2);
        }
    }
}
