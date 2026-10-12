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
    modulation: ModShape,
}

impl Dims {
    pub fn of(request: u64, plan: &Prepared) -> Self {
        Self {
            request,
            chain: DspState::shape(plan),
            modulation: plan.voice_modulation.shape(),
        }
    }
}

struct PlanStorage {
    request: u64,
    cells: sampler_pool::Slab<dsp::ProcessorState>,
    delays: sampler_pool::Slab<[f64; 2]>,
    modulation: VoiceModState,
}

/// Larger per-voice storage for every live plan, in flight to the audio
/// thread and back (holding the old storage afterwards).
pub(crate) struct Growth {
    voices: usize,
    slots: Box<[Slot<Voice>]>,
    free: Box<[u64]>,
    activity: Box<[u64]>,
    scratch: Option<parallel::Scratch>,
    plans: Vec<PlanStorage>,
    note_params: Option<crate::script_params::NoteParamsGrowth>,
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
                    modulation: VoiceModState::with_shape(d.modulation, voices)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self {
            voices,
            slots,
            free,
            activity: vec![0; voices.div_ceil(64)].into_boxed_slice(),
            scratch: parallel.then(|| parallel::Scratch::new(voices)),
            plans,
            note_params: None,
        })
    }

    pub fn note_params(growth: crate::script_params::NoteParamsGrowth) -> Self {
        Self { voices: 0, slots: Box::new([]), free: Box::new([]), activity: Box::new([]),
            scratch: None, plans: Vec::new(), note_params: Some(growth) }
    }
}

/// Audio-side ends of the growth channel.
pub(crate) struct GrowthQueues {
    pub incoming: Consumer<Growth>,
    pub outgoing: Producer<Growth>,
    /// The control-side thread to wake when the pool fills or a growth returns.
    pub waker: Option<std::thread::Thread>,
}

impl Runtime {
    /// Slots in the voice pool.
    pub fn voice_capacity(&self) -> usize {
        self.voices.slots.len()
    }

    pub fn note_params_capacity(&self) -> usize { self.note_params.capacity() }

    /// Control side: wake `thread` (an unpark, which is allocation free) when
    /// the voice pool fills up and when a growth comes back, so it can sleep
    /// until there is work for `PlanControl::grow_voices`. Needs a runtime
    /// from `with_plan_updates`.
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
        let Ok(mut growth) = queues.incoming.pop() else {
            return;
        };
        if self.grow_with(&mut growth) {
            if growth.voices != 0 { self.voice_growths += 1; }
        } else {
            self.growth_failures += 1;
        }
        let queues = self.growth.as_mut().unwrap();
        // One growth is in flight at a time, so the return queue has room.
        if let Err(rtrb::PushError::Full(lost)) = queues.outgoing.push(growth) {
            std::mem::forget(lost);
        }
        if let Some(thread) = &self.growth.as_ref().unwrap().waker {
            thread.unpark();
        }
    }

    fn grow_with(&mut self, g: &mut Growth) -> bool {
        if let Some(params) = &mut g.note_params { return self.note_params.adopt(params); }
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
            gen_.modulation.adopt(&mut p.modulation);
        }
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
        if self.notes.occupied * 4 >= self.note_params.capacity() * 3
            && !self.note_pressure.swap(true, Ordering::Relaxed)
            && let Some(thread) = self.growth.as_ref().and_then(|q| q.waker.as_ref())
        { thread.unpark(); }
    }
}

pub(crate) type Pressure = Arc<AtomicBool>;
