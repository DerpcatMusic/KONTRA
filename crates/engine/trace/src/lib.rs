//! Event data for opt-in engine inspection.
use kontra_vm::Action;
use kontra_voice::Gain;

/// One event captured by the engine at its sample clock.
#[derive(Clone, Copy, Debug)]
pub enum TraceEvent {
    /// Script callback starts or resumes.
    Callback {
        frame: u64,
        slot: u8,
        kind: u8,
        id: u32,
        note_id: Option<i32>,
        controller: u8,
        control: i32,
        resume: bool,
    },
    /// A script called an engine action; extra integer and all text arguments follow.
    Action {
        frame: u64,
        slot: u8,
        callback: u32,
        action: Action,
        call_id: u64,
        args: [i64; 8],
        len: u8,
        integer_count: usize,
        text_count: usize,
    },
    /// An integer argument beyond the fixed inline prefix.
    ActionInteger {
        call_id: u64,
        index: usize,
        value: i64,
    },
    /// A UTF-8 chunk of a text argument, including empty text.
    ActionText {
        call_id: u64,
        index: usize,
        chunk: usize,
        len: u8,
        value: [u8; 96],
    },
    /// Source sampled at voice start; some routes are subsequently re-read.
    RouteOnset {
        frame: u64,
        voice_slot: usize,
        zone: usize,
        mod_slot: u16,
        target: u16,
        kind: kontra_ir::ModKind,
        source: Option<f32>,
    },
    /// A voice was started, stolen, cut or ended.
    Voice {
        frame: u64,
        slot: usize,
        zone: usize,
        event: VoiceEvent,
        position: f64,
        step: f64,
        gain: Gain,
    },
}

/// Voice lifecycle transition. End timestamps are the end of the render span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceEvent {
    Start,
    Steal,
    Cut,
    End,
}

/// Unit of the exact route contribution used by a voice update.
#[derive(Clone, Copy, Debug)]
pub enum RouteUnit {
    GainMultiplier,
    Pan,
    Semitones,
    CutoffDepth,
    ResonanceDepth,
    ResonanceMultiplier,
    EqDecibels,
    EffectAmount,
    MixMultiplier,
    IntensityMultiplier,
    PhaseCycles,
    RateMultiplier,
    LfoHertz,
    SampleStartFraction,
    LoopFraction,
    EnvelopeFrames,
}

/// A route value as evaluated in a controller-triggered voice refresh.
#[derive(Clone, Copy, Debug)]
pub struct RouteValue {
    pub frame: u64,
    pub voice_slot: usize,
    pub zone: usize,
    pub controller: u8,
    pub mod_slot: u16,
    pub target: u16,
    pub kind: kontra_ir::ModKind,
    pub source: Option<f32>,
    pub value: f64,
    pub unit: RouteUnit,
    pub direct_controller_source: bool,
    pub controller_can_affect: bool,
    pub timed_refresh: bool,
    pub note_onset: bool,
    pub script_callback: Option<(u8, u32)>,
}

/// The evaluated part of a route value before its voice context is attached.
#[derive(Clone, Copy, Debug)]
pub struct RouteSample {
    pub frame: u64,
    pub controller: u8,
    pub mod_slot: u16,
    pub target: u16,
    pub kind: kontra_ir::ModKind,
    pub source: Option<f32>,
    pub value: f64,
    pub unit: RouteUnit,
    pub direct_controller_source: bool,
    pub controller_can_affect: bool,
    pub script_callback: Option<(u8, u32)>,
}

#[derive(Clone, Copy)]
struct RouteContext {
    voice: usize,
    zone: usize,
    controllers: u128,
    timed: bool,
    onset: bool,
}

/// A bounded, preallocated route buffer. Interior mutation permits observation
/// in read-only route evaluators; it is only constructed for an opt-in trace.
pub struct RouteBuffer {
    events: Box<[std::cell::Cell<Option<RouteValue>>]>,
    len: std::cell::Cell<usize>,
    dropped: std::cell::Cell<u64>,
    context: std::cell::Cell<Option<RouteContext>>,
    onset_mark: std::cell::Cell<Option<usize>>,
}

impl RouteBuffer {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            events: (0..capacity).map(|_| std::cell::Cell::new(None)).collect(),
            len: std::cell::Cell::new(0),
            dropped: std::cell::Cell::new(0),
            context: std::cell::Cell::new(None),
            onset_mark: std::cell::Cell::new(None),
        }
    }
    pub fn begin(&self, voice: usize, zone: usize, controller: u8) {
        self.context.set(Some(RouteContext {
            voice,
            zone,
            controllers: 1u128 << controller,
            timed: false,
            onset: false,
        }));
    }
    pub fn begin_mask(&self, voice: usize, zone: usize, controllers: u128) {
        self.context.set(Some(RouteContext {
            voice,
            zone,
            controllers,
            timed: true,
            onset: false,
        }));
    }
    /// Begin capturing controller-dependent evaluations for a prospective voice.
    pub fn begin_onset(&self, zone: usize) {
        self.onset_mark.set(Some(self.len.get()));
        self.context.set(Some(RouteContext {
            voice: usize::MAX,
            zone,
            controllers: u128::MAX,
            timed: false,
            onset: true,
        }));
    }
    /// Assign the admitted voice to its captured evaluations, or discard them.
    pub fn finish_onset(&self, voice: Option<usize>) {
        if let Some(mark) = self.onset_mark.replace(None) {
            if let Some(voice) = voice {
                for cell in self.events.get(mark..self.len.get()).unwrap_or_default() {
                    if let Some(mut event) = cell.get() {
                        event.voice_slot = voice;
                        cell.set(Some(event));
                    }
                }
            } else {
                self.len.set(mark);
            }
        }
        self.end();
    }
    #[must_use]
    pub fn is_onset(&self) -> bool {
        self.context.get().is_some_and(|x| x.onset)
    }
    pub fn end(&self) {
        self.context.set(None);
    }
    #[must_use]
    pub fn controllers(&self) -> u128 {
        self.context.get().map_or(0, |x| x.controllers)
    }
    pub fn push(&self, sample: RouteSample) {
        let Some(context) = self.context.get() else {
            return;
        };
        let at = self.len.get();
        if let Some(slot) = self.events.get(at) {
            slot.set(Some(RouteValue {
                frame: sample.frame,
                voice_slot: context.voice,
                zone: context.zone,
                controller: sample.controller,
                mod_slot: sample.mod_slot,
                target: sample.target,
                kind: sample.kind,
                source: sample.source,
                value: sample.value,
                unit: sample.unit,
                direct_controller_source: sample.direct_controller_source,
                controller_can_affect: sample.controller_can_affect,
                timed_refresh: context.timed,
                note_onset: context.onset,
                script_callback: sample.script_callback,
            }));
            self.len.set(at + 1);
        } else {
            self.dropped.set(self.dropped.get().saturating_add(1));
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = RouteValue> + '_ {
        self.events
            .get(..self.len.get())
            .unwrap_or_default()
            .iter()
            .filter_map(std::cell::Cell::get)
    }
    pub fn clear(&self) {
        self.len.set(0);
    }
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.get()
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    #[test]
    fn route_buffer_is_bounded_and_assigns_onset_voice() {
        let buffer = RouteBuffer::new(1);
        let route = kontra_ir::Mod {
            name: "x".into(),
            slot: 1,
            target: 0,
            bypass: false,
            kind: kontra_ir::ModKind::Pan {
                source: kontra_ir::Source::Controller(1),
                amount: 1.0,
            },
        };
        buffer.begin_onset(12);
        let sample = RouteSample {
            frame: 10,
            controller: 1,
            mod_slot: route.slot,
            target: route.target,
            kind: route.kind,
            source: Some(0.5),
            value: 0.5,
            unit: RouteUnit::Pan,
            direct_controller_source: true,
            controller_can_affect: true,
            script_callback: None,
        };
        buffer.push(sample);
        buffer.push(sample);
        buffer.finish_onset(Some(3));
        let values: Vec<_> = buffer.iter().collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].voice_slot, 3);
        assert!(values[0].note_onset);
        assert_eq!(buffer.dropped(), 1);
        buffer.clear();
        assert_eq!(buffer.iter().count(), 0);
        assert_eq!(buffer.dropped(), 1);
    }
}
