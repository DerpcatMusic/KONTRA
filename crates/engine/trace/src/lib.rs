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
    pub value: f32,
    pub unit: RouteUnit,
    pub direct_controller_source: bool,
}

/// A bounded, preallocated route buffer. Interior mutation permits observation
/// in read-only route evaluators; it is only constructed for an opt-in trace.
pub struct RouteBuffer {
    events: Box<[std::cell::Cell<Option<RouteValue>>]>,
    len: std::cell::Cell<usize>,
    dropped: std::cell::Cell<u64>,
    context: std::cell::Cell<Option<(usize, usize, u8)>>,
}

impl RouteBuffer {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            events: (0..capacity).map(|_| std::cell::Cell::new(None)).collect(),
            len: std::cell::Cell::new(0),
            dropped: std::cell::Cell::new(0),
            context: std::cell::Cell::new(None),
        }
    }
    pub fn begin(&self, voice: usize, zone: usize, controller: u8) {
        self.context.set(Some((voice, zone, controller)));
    }
    pub fn end(&self) {
        self.context.set(None);
    }
    #[must_use]
    pub fn controller(&self) -> Option<u8> {
        self.context.get().map(|x| x.2)
    }
    pub fn push(
        &self,
        frame: u64,
        route: &kontra_ir::Mod,
        source: Option<f32>,
        value: f32,
        unit: RouteUnit,
        direct: bool,
    ) {
        let Some((voice_slot, zone, controller)) = self.context.get() else {
            return;
        };
        let at = self.len.get();
        if let Some(slot) = self.events.get(at) {
            slot.set(Some(RouteValue {
                frame,
                voice_slot,
                zone,
                controller,
                mod_slot: route.slot,
                target: route.target,
                kind: route.kind,
                source,
                value,
                unit,
                direct_controller_source: direct,
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
