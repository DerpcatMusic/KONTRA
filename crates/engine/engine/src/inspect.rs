//! What a script did since load, for diagnostics and the parity tools.

use kontra_vm::{Action, HostApi, HostCall, Text};

use crate::Engine;

impl Engine {
    /// Groups a script purged: the host may drop their samples.
    pub fn purged(&self) -> impl Iterator<Item = usize> + '_ {
        self.groups
            .iter()
            .enumerate()
            .filter_map(|(n, g)| g.purged.then_some(n))
    }

    /// EQ chains on summed voices, and the zones each feeds.
    pub fn eq_chains(&self) -> (usize, usize) {
        let zones = self.zone_chain.iter().flatten().count();
        (self.chains.len(), zones)
    }

    /// Every group's routes as scripts have left them.
    pub fn mods(&self) -> impl Iterator<Item = &kontra_ir::Mod> + '_ {
        self.groups.iter().flat_map(|g| &g.mods)
    }

    /// Groups whose filter runs per voice, and each sounding voice's cutoff.
    pub fn voice_filters(&self) -> (usize, Vec<f32>) {
        self.voice_cutoffs()
    }

    /// Each sounding voice's zone, position in source frames and gain, for
    /// inspectors (never the audio thread).
    pub fn voices(&self) -> impl Iterator<Item = (usize, f64, kontra_voice::Gain)> + '_ {
        self.voices
            .iter()
            .filter(|s| s.voice.is_active())
            .map(|s| (s.zone, s.voice.position(), s.voice.target_gain()))
    }

    /// The last text a script printed with `message`.
    #[must_use]
    pub fn last_message(&self) -> &str {
        self.last_message.as_str()
    }

    /// Script actions this engine left unanswered, most called first.
    #[must_use]
    pub fn unhandled_actions(&self) -> Vec<(kontra_vm::Action, u32)> {
        ranked(&self.unhandled)
    }

    /// Every script action called since load, most called first.
    #[must_use]
    pub fn action_calls(&self) -> Vec<(kontra_vm::Action, u32)> {
        ranked(&self.called)
    }

    /// Whether this engine answers `action`, called once with zero arguments.
    /// Coverage tools only: the call acts on this engine (a probe engine).
    /// An action answered only for some arguments can read as unanswered.
    pub fn answers(&mut self, action: Action) -> bool {
        let before = self.unhandled.get(usize::from(action.id())).copied();
        let mut cells = [0i64; 64];
        let mut text_out = Text::EMPTY;
        self.call(HostCall {
            action,
            context: 0,
            ints: &[0; 8],
            texts: &[Text::EMPTY; 4],
            cells: &mut cells,
            text_cells: &mut [],
            text_out: &mut text_out,
        });
        self.unhandled.get(usize::from(action.id())).copied() == before
    }

    /// A script variable cell, for inspection.
    #[must_use]
    pub fn script_cell(&self, n: usize) -> Option<i64> {
        self.vms.first()?.as_ref()?.cell(n)
    }

    /// Runtime faults of each script slot, counted since load.
    #[must_use]
    pub fn script_faults(&self) -> Vec<kontra_vm::Faults> {
        self.vms
            .iter()
            .flatten()
            .map(kontra_vm::Vm::faults)
            .collect()
    }
}

fn ranked(counts: &[u32]) -> Vec<(kontra_vm::Action, u32)> {
    let mut out: Vec<_> = kontra_vm::Action::all()
        .iter()
        .zip(counts)
        .filter(|(_, n)| **n > 0)
        .map(|(a, n)| (*a, *n))
        .collect();
    out.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    out
}

/// A voice's current gain, before its sample and effects, sampled between blocks.
#[derive(Clone, Copy, Debug)]
pub struct VoiceObservation {
    pub zone: usize,
    pub position: f64,
    pub released: bool,
    pub gain: kontra_voice::Gain,
    /// Source frames this voice read as silence because data was unavailable.
    pub missing_frames: u64,
    /// Peak of the last voice render, before group effects.
    #[cfg(feature = "trace")]
    pub last_peak: f32,
}

/// A live EQ route, sampled from the same source and law used by the engine.
#[derive(Clone, Debug)]
pub struct EqRouteObservation {
    pub voice_slot: usize,
    pub zone: usize,
    pub name: String,
    pub insert: u8,
    pub band: u8,
    pub source: Option<f32>,
    pub contribution_db: Option<f32>,
    pub band_gain_db: Option<f32>,
}

impl Engine {
    /// Allocate a bounded trace buffer off the audio thread and enable event capture.
    #[cfg(feature = "trace")]
    pub fn enable_trace(&mut self, capacity: usize) {
        self.trace = Some(crate::trace::Trace {
            events: Vec::with_capacity(capacity),
            dropped: 0,
        });
    }

    /// Captured events in order since the last clear.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn trace_events(&self) -> &[crate::TraceEvent] {
        self.trace.as_ref().map_or(&[], |t| &t.events)
    }

    /// Total events rejected because the bounded buffer was full.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn trace_dropped(&self) -> u64 {
        self.trace.as_ref().map_or(0, |t| t.dropped)
    }

    /// Clear events between blocks without releasing the allocation.
    #[cfg(feature = "trace")]
    pub fn clear_trace(&mut self) {
        if let Some(t) = &mut self.trace {
            t.events.clear();
        }
    }

    /// Enable blocking page waits in every voice for offline rendering only.
    #[cfg(feature = "trace")]
    pub fn set_offline_wait(&mut self, enabled: bool) {
        for slot in &mut self.voices {
            slot.voice.set_offline_wait(enabled);
        }
    }

    /// Count offline waits that timed out across all voice slots.
    #[cfg(feature = "trace")]
    #[must_use]
    pub fn offline_wait_timeouts(&self) -> u64 {
        self.voices
            .iter()
            .map(|s| s.voice.offline_wait_timeouts())
            .sum()
    }

    /// Samples active voices between process calls; gain is not isolated audio level.
    pub fn inspect_voices(&self) -> impl Iterator<Item = VoiceObservation> + '_ {
        self.voices
            .iter()
            .filter(|s| s.voice.is_active())
            .map(|s| VoiceObservation {
                zone: s.zone,
                position: s.voice.position(),
                released: s.released,
                gain: s.voice.current_gain(),
                missing_frames: s.voice.missing_frames(),
                #[cfg(feature = "trace")]
                last_peak: s.voice.last_peak(),
            })
    }

    /// Samples active voices' EQ routes without changing the processing path.
    pub fn inspect_eq_routes(&self) -> Vec<EqRouteObservation> {
        let mut result = Vec::new();
        for (voice_slot, slot) in self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, s)| s.voice.is_active())
        {
            let Some((zone, note)) = self
                .plan
                .zones
                .get(slot.zone)
                .zip(slot.note.and_then(|n| self.notes.get(n)))
            else {
                continue;
            };
            let Some(group) = self.groups.get(zone.group as usize) else {
                continue;
            };
            let age = crate::modulators::Age {
                warp: slot.warp,
                chaos: slot.chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.released_at)
            };
            for m in group.mods.iter().filter(|m| !m.bypass) {
                if let kontra_ir::ModKind::EqGain {
                    source,
                    insert,
                    band,
                    depth_db,
                    ..
                } = m.kind
                {
                    let x = self.routed(zone, source, note, age);
                    let band_gain_db = self
                        .voice_bells(zone, insert, note, age)
                        .and_then(|bells| bells.get(usize::from(band)).map(|b| b.gain_db));
                    result.push(EqRouteObservation {
                        voice_slot,
                        zone: slot.zone,
                        name: m.name.clone(),
                        insert,
                        band,
                        source: x,
                        contribution_db: x.map(|v| v * depth_db),
                        band_gain_db,
                    });
                }
            }
        }
        result
    }
}

/// An active voice's route and the current input observed between spans.
#[derive(Clone, Debug)]
pub struct RouteObservation {
    pub voice_slot: usize,
    pub zone: usize,
    pub name: String,
    pub kind: kontra_ir::ModKind,
    /// Current raw source; `None` when its source is fixed at note-on or unavailable.
    pub source: Option<f32>,
    /// Current source after timed-source intensity scaling.
    pub routed: Option<f32>,
}

fn route_source(kind: kontra_ir::ModKind) -> Option<kontra_ir::Source> {
    use kontra_ir::ModKind;
    match kind {
        ModKind::Envelope => None,
        ModKind::Volume { source, .. }
        | ModKind::Pitch { source, .. }
        | ModKind::Pan { source, .. }
        | ModKind::Cutoff { source, .. }
        | ModKind::Resonance { source, .. }
        | ModKind::EqGain { source, .. }
        | ModKind::SampleStart { source, .. }
        | ModKind::Attack { source, .. }
        | ModKind::Release { source, .. }
        | ModKind::LfoPhase { source, .. }
        | ModKind::LfoRate { source, .. }
        | ModKind::LfoHz { source, .. }
        | ModKind::Intensity { source, .. }
        | ModKind::LoopShift { source, .. }
        | ModKind::Mix { source, .. }
        | ModKind::EffectParam { source, .. }
        | ModKind::Send { source, .. } => Some(source),
    }
}

impl Engine {
    /// Observe all active routes, including shaped and blended controller inputs.
    pub fn inspect_routes(&self) -> Vec<RouteObservation> {
        let mut out = Vec::new();
        for (voice_slot, slot) in self
            .voices
            .iter()
            .enumerate()
            .filter(|(_, s)| s.voice.is_active())
        {
            let Some((zone, note)) = self
                .plan
                .zones
                .get(slot.zone)
                .zip(slot.note.and_then(|n| self.notes.get(n)))
            else {
                continue;
            };
            let Some(group) = self.groups.get(zone.group as usize) else {
                continue;
            };
            let age = crate::modulators::Age {
                warp: slot.warp,
                chaos: slot.chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.released_at)
            };
            for m in group.mods.iter().filter(|m| !m.bypass) {
                let input = route_source(m.kind);
                out.push(RouteObservation {
                    voice_slot,
                    zone: slot.zone,
                    name: m.name.clone(),
                    kind: m.kind,
                    source: input.and_then(|s| self.source(zone, s, note, age)),
                    routed: input.and_then(|s| self.routed(zone, s, note, age)),
                });
            }
        }
        out
    }
}

/// One bus route's last applied amount.
#[derive(Clone, Copy, Debug)]
pub struct BusRouteObservation {
    pub bus: usize,
    pub kind: kontra_ir::ModKind,
    pub amount: f32,
}

impl Engine {
    /// Read bus route amounts as last applied by the rack.
    pub fn inspect_bus_routes(&self) -> impl Iterator<Item = BusRouteObservation> + '_ {
        self.buses.iter().enumerate().flat_map(|(bus, state)| {
            state
                .rack
                .route_amounts()
                .map(move |(kind, amount)| BusRouteObservation { bus, kind, amount })
        })
    }
}
