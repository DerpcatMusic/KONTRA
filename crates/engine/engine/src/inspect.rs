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

    /// Current group levels and bus gain, pan and enabled state.
    #[cfg(feature = "trace")]
    pub fn inspect_levels(&self) -> (Vec<crate::Level>, Vec<(f32, f32, bool)>) {
        (
            self.groups.iter().map(|g| g.level).collect(),
            self.buses
                .iter()
                .map(|b| (b.gain, b.pan, b.enabled))
                .collect(),
        )
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
    /// Current pitch in source frames per output frame.
    #[cfg(feature = "trace")]
    pub step: f64,
    /// Current amplitude envelope stage.
    pub envelope_stage: &'static str,
    /// Its note's event id (0 once gone), origin (host, child, script),
    /// and whether the note was released or the pedal holds it: why a
    /// voice plays on after every key is up.
    pub id: i32,
    pub origin: Option<&'static str>,
    pub note_released: bool,
    pub pedal_held: bool,
    /// Fading out after a steal or `fade_out`.
    pub fading: bool,
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
        self.trace
            .as_ref()
            .map_or(0, |t| t.dropped.saturating_add(t.routes.dropped()))
    }

    /// Clear events between blocks without releasing the allocation.
    #[cfg(feature = "trace")]
    pub fn clear_trace(&mut self) {
        if let Some(t) = &mut self.trace {
            t.events.clear();
            t.routes.clear();
        }
    }

    /// Exact route values captured during controller-triggered voice updates.
    #[cfg(feature = "trace")]
    pub fn trace_route_values(&self) -> impl Iterator<Item = crate::RouteValue> + '_ {
        self.trace.iter().flat_map(|t| t.routes.iter())
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
            .map(|s| (s, s.note.and_then(|r| Some((r, self.notes.get(r)?)))))
            .map(|(s, note)| VoiceObservation {
                zone: s.zone,
                position: s.voice.position(),
                released: s.released,
                gain: s.voice.current_gain(),
                missing_frames: s.voice.missing_frames(),
                #[cfg(feature = "trace")]
                step: s.voice.step(),
                envelope_stage: s.voice.envelope_stage(),
                id: note.map_or(0, |(r, _)| r.event_id()),
                origin: note.map(|(_, n)| match n.origin {
                    crate::ledger::Origin::Host => "host",
                    crate::ledger::Origin::Child(_) => "child",
                    crate::ledger::Origin::Script => "script",
                }),
                note_released: note.is_some_and(|(_, n)| n.released),
                pedal_held: note.is_some_and(|(_, n)| n.pedal_held),
                fading: s.fading,
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
    /// Current raw source; `None` if the source cannot be observed.
    pub source: Option<f32>,
    /// Current source after timed-source intensity scaling.
    pub routed: Option<f32>,
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
                let input = kontra_plan::route_source(m.kind);
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

/// Target parameters for one per-voice effect, sampled between spans.
#[derive(Clone, Copy, Debug)]
pub struct VoiceEffectObservation {
    pub voice_slot: usize,
    pub zone: usize,
    pub insert: u8,
    pub target: kontra_ir::EffectKind,
}

impl Engine {
    /// Effect targets calculated by the same functions the voice rack uses.
    pub fn inspect_voice_effects(&self) -> Vec<VoiceEffectObservation> {
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
            let age = crate::modulators::Age {
                warp: slot.warp,
                chaos: slot.chaos.map(|c| c[3]),
                ..self.age(slot.started, slot.released_at)
            };
            for unit in &slot.rack {
                let Some(mut target) = self.voice_effect(zone, unit.insert, note, age) else {
                    continue;
                };
                match &mut target {
                    kontra_ir::EffectKind::Eq(bells) => {
                        if let Some(v) = self.voice_bells(zone, unit.insert, note, age) {
                            *bells = v;
                        }
                    }
                    kontra_ir::EffectKind::Filter(f) => {
                        if let Some(kontra_ir::EffectKind::Filter(base)) = self
                            .groups
                            .get(zone.group as usize)
                            .and_then(|g| g.inserts.get(usize::from(unit.insert)))
                            .and_then(Option::as_ref)
                            .map(|e| e.kind)
                        {
                            *f = base;
                        }
                        if let Some((hz, damping)) = self.voice_filter(zone, unit.insert, note, age)
                        {
                            f.hz = hz;
                            f.damping = damping;
                        }
                    }
                    _ => {}
                }
                out.push(VoiceEffectObservation {
                    voice_slot,
                    zone: slot.zone,
                    insert: unit.insert,
                    target,
                });
            }
        }
        out
    }
}

/// An instrument, main, send or bus effect after its last route update.
#[derive(Clone, Copy, Debug)]
pub struct SharedEffectObservation {
    pub scope: &'static str,
    pub owner: usize,
    pub slot: usize,
    pub effect: Option<kontra_ir::Effect>,
}

impl Engine {
    /// Snapshot shared rack settings, including the last applied bus routes.
    pub fn inspect_shared_effects(&self) -> Vec<SharedEffectObservation> {
        let mut out = Vec::new();
        let mut add = |scope, owner, rack: &crate::racks::Rack| {
            out.extend(
                rack.inspect_slots()
                    .into_iter()
                    .enumerate()
                    .map(|(slot, effect)| SharedEffectObservation {
                        scope,
                        owner,
                        slot,
                        effect,
                    }),
            );
        };
        add("instrument", 0, &self.rack);
        add("main", 0, &self.main);
        for (owner, rack) in self.sends.iter().enumerate() {
            add("send", owner, rack);
        }
        for (owner, bus) in self.buses.iter().enumerate() {
            add("bus", owner, &bus.rack);
        }
        out
    }
}
