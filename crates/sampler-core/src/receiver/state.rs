//! Actual prepared receiver state, shared by legacy unbound adapters and
//! explicit Core-owned registration. No packet parser or vendor DSP lives here.
use super::types::{Applied, ApplyError, ChannelVoice, Controllers, Message, Value, Version};
use crate::{
    ChannelScope, Error, Expression, ExpressionId, Input, NoteId, NotePitch, PerformanceId,
    Protocol, Runtime, RuntimeId,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Lower,
    Upper,
}
impl Zone {
    pub(super) fn manager(self) -> u8 {
        match self {
            Self::Lower => 0,
            Self::Upper => 15,
        }
    }
    pub(super) fn index(self) -> usize {
        usize::from(self == Self::Upper)
    }
    pub(super) fn contains(self, channel: u8, members: u8) -> bool {
        channel < 16 && channel.abs_diff(self.manager()) <= members
    }
}

/// Two physical wire domains and one explicit Part-aggregated native-host
/// execution domain. The actual host source tuple remains in Runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChannelDomain {
    Midi1,
    Midi2,
    HostChannel,
}
impl ChannelDomain {
    pub(super) const ALL: [Self; 3] = [Self::Midi1, Self::Midi2, Self::HostChannel];
    pub(super) fn index(self) -> usize {
        match self {
            Self::Midi1 => 0,
            Self::Midi2 => 1,
            Self::HostChannel => 2,
        }
    }
    pub(super) fn bit(self) -> u8 {
        1 << self.index()
    }
    pub(super) fn protocol(self) -> Protocol {
        match self {
            Self::Midi1 => Protocol::Midi1,
            Self::Midi2 => Protocol::Midi2,
            Self::HostChannel => Protocol::Native,
        }
    }
}

/// An exact raw physical terminal owed to a script after a synthetic
/// all-notes-off. The original source is not the Part's collapsed execution
/// address. Storage is reserved by Mpe::new; draining never allocates.
#[derive(Clone, Copy, Debug)]
pub struct TerminalRelease {
    pub note: NoteId,
    pub source: Input,
    pub velocity: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PhysicalKind {
    Wire,
    Native,
}
#[derive(Clone, Copy)]
pub(super) struct PhysicalPair {
    pub(super) note: NoteId,
    pub(super) expression: Option<ExpressionId>,
    pub(super) execution: Input,
    pub(super) source: Input,
    pub(super) kind: PhysicalKind,
    pub(super) released: bool,
    pub(super) scripted: bool,
    pub(super) terminal_received: bool,
    pub(super) choked: bool,
}
pub(super) fn native_source(source: Input) -> bool {
    matches!(source.protocol, Protocol::Clap | Protocol::Vst3)
}

#[derive(Clone, Copy)]
pub(super) struct Binding {
    pub(super) note: NoteId,
    pub(super) channel: u8,
    pub(super) address: crate::ChannelAddress,
    /// Captured wire protocol; never inferred from later channel traffic.
    pub(super) domain: ChannelDomain,
    pub(super) member: Controls,
    /// Transport's per-note tuning, independent of zone bends and part transpose.
    pub(super) tuning: f64,
    pub(super) key: u8,
    pub(super) per_note: Option<super::per_note::Snapshot>,
    pub(super) source: Input,
    pub(super) scripted: bool,
    pub(super) terminal_released: bool,
}

/// Pressure and timbre at 32 bits (MIDI 1.0 values upscaled, MIDI 2.0 as sent).
#[derive(Clone, Copy)]
pub(crate) struct Controls {
    pub(super) pitch: f64,
    /// Raw bend position, -1..=1.
    pub(super) bend: f64,
    pub(super) pressure: u32,
    pub(super) timbre: Value,
}
const CENTER: u32 = 0x8000_0000;
impl Default for Controls {
    fn default() -> Self {
        Self {
            pitch: 0.0,
            bend: 0.0,
            pressure: 0,
            timbre: Value::Bits7(64),
        }
    }
}
impl Controls {
    pub(super) fn expression(self, manager: Self) -> Expression {
        // Offsets from centre add; MIDI 1.0 pairs combine at their own 7 bits.
        let timbre = match (self.timbre, manager.timbre) {
            (Value::Bits7(a), Value::Bits7(b)) => {
                Value::Bits7((i16::from(a) + i16::from(b) - 64).clamp(0, 127) as u8).full_scale()
            }
            (a, b) => {
                let sum = i64::from(a.full_scale()) + i64::from(b.full_scale()) - i64::from(CENTER);
                sum.clamp(0, i64::from(u32::MAX)) as u32
            }
        };
        Expression {
            pitch_semitones: self.pitch + manager.pitch,
            bend: (self.bend + manager.bend).clamp(-1.0, 1.0),
            pressure: self.pressure.max(manager.pressure),
            timbre,
            ..Expression::default()
        }
    }
    pub(super) fn with(mut self, control: Control) -> Self {
        match control {
            Control::Pitch(value, bend) => (self.pitch, self.bend) = (value, bend),
            Control::Pressure(value) => self.pressure = value,
            Control::Timbre(value) => self.timbre = value,
        }
        self
    }
}
#[derive(Clone, Copy)]
pub(super) enum Control {
    /// Semitones and the raw bend position.
    Pitch(f64, f64),
    Pressure(u32),
    Timbre(Value),
}

#[derive(Clone, Copy)]
pub(super) struct Parameter {
    pub(super) rpn: [u8; 2],
    pub(super) registered: bool,
}
impl Default for Parameter {
    fn default() -> Self {
        Self {
            rpn: [127; 2],
            registered: false,
        }
    }
}

/// RPN 0 fields retain their serialized coarse value even when cents exceed
/// 99; relative RP-018 steps carry only when applying the relative operation.
#[derive(Clone, Copy)]
pub(super) struct Sensitivity {
    pub(super) coarse: u8,
    pub(super) cents: u8,
}
impl Sensitivity {
    pub(super) fn semitones(self) -> f64 {
        f64::from(self.coarse) + f64::from(self.cents) / 100.
    }
    pub(super) fn relative(self, increment: bool) -> Self {
        let total = i32::from(self.coarse) * 100 + i32::from(self.cents);
        let total = (total + if increment { 1 } else { -1 }).clamp(0, 9600) as u16;
        Self {
            coarse: (total / 100) as u8,
            cents: (total % 100) as u8,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct WireState {
    pub(super) controls: [Controls; 16],
    pub(super) bends: [Value; 16],
    pub(super) ranges: [Sensitivity; 2],
    pub(super) channel_ranges: [Sensitivity; 16],
    pub(super) parameters: [Parameter; 16],
}
impl Default for WireState {
    fn default() -> Self {
        Self {
            controls: [Controls::default(); 16],
            bends: [Value::Bits14(8192); 16],
            ranges: [
                Sensitivity {
                    coarse: 2,
                    cents: 0,
                },
                Sensitivity {
                    coarse: 48,
                    cents: 0,
                },
            ],
            channel_ranges: [Sensitivity {
                coarse: 2,
                cents: 0,
            }; 16],
            parameters: [Parameter::default(); 16],
        }
    }
}

/// Effective manager component for one admitted target-channel epoch. A
/// manager input can survive a captured wait for only a subset of targets.
/// Keep its typed bend here so range changes preserve original arithmetic.
#[derive(Clone, Copy)]
pub(crate) struct ManagerState {
    pub controls: Controls,
    pub bend: Value,
}
impl Default for ManagerState {
    fn default() -> Self {
        Self {
            controls: Controls::default(),
            bend: Value::Bits14(8192),
        }
    }
}

/// One explicitly configured zone in one runtime/port/group domain. MIDI 1.0
/// and 2.0 channel voice messages both play it; 2.0 values keep their
/// precision (velocity, controllers, pressure, pitch bend).
/// Construct and drop on the control thread. The binding budget includes tails
/// and unaccepted terminal notes. No pins are added and no heap work occurs in apply.
///
/// MCM/RPN6 tracks both zones for overlap arbitration; this instrument receives
/// its explicitly selected zone (MPE 1.1 section3.1). Other RPNs and unsupported
/// channel modes are reported Unsupported. Do not forward those messages to ordinary channel
/// ingress as a substitute for zone semantics. Raw-event consumers run before
/// this adapter; a consumed message must not be passed to apply.
pub struct OwnedReceiver {
    pub(super) registration: Option<super::ReceiverId>,
    pub(super) runtime: RuntimeId,
    pub(super) performance: PerformanceId,
    pub(super) port: u16,
    pub(super) group: u8,
    pub(super) zone: Zone,
    pub(super) members: u8,
    pub(super) fixed_members: u8,
    pub(super) zones: [u8; 2],
    pub(super) manual_active: bool,
    pub(super) configured: bool,
    pub(super) limit: usize,
    pub(super) wire: [WireState; 3],
    pub(super) managers: [[ManagerState; 16]; 3],
    pub(super) host_projection: bool,
    pub(super) host_mpe_active: bool,
    pub(super) midi2_channel_voice: bool,
    pub(super) bend_override: Option<u8>,
    pub(super) bindings: Vec<Binding>,
    pub(super) changes: Vec<(ExpressionId, Expression)>,
    pub(super) transpose: f64,
    pub(super) timbre_controller: Option<u8>,
    pub(super) per_note: super::per_note::PerNoteState,
    pub(super) note_changes: Vec<(usize, super::per_note::Snapshot)>,
    pub(super) physical: Vec<PhysicalPair>,
    pub(super) physical_limit: usize,
    pub(super) terminal_releases: Vec<TerminalRelease>,
    pub(super) terminal_limit: usize,
    pub(super) reset_members: Vec<usize>,
}
impl OwnedReceiver {
    pub fn new(
        runtime: &Runtime,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        Self::new_in(
            runtime,
            runtime.performance(0)?,
            port,
            group,
            zone,
            members,
            notes,
        )
    }

    /// Bind all expressive members to one explicit musical routing domain.
    pub fn new_in(
        runtime: &Runtime,
        performance: PerformanceId,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        runtime.articulation(performance)?;
        if group >= 16 || !(1..=15).contains(&members) || notes == 0 {
            return Err(Error::InvalidInput);
        }
        // Registry and expression work capacity are fixed before any attack.
        let reset_bytes = std::alloc::Layout::array::<usize>(notes)
            .map_err(|_| Error::Capacity)?
            .size();
        let bindings_bytes = std::alloc::Layout::array::<Binding>(notes)
            .map_err(|_| Error::Capacity)?
            .size();
        let changes_bytes = std::alloc::Layout::array::<(ExpressionId, Expression)>(notes)
            .map_err(|_| Error::Capacity)?
            .size();
        let note_changes_bytes =
            std::alloc::Layout::array::<(usize, super::per_note::Snapshot)>(notes)
                .map_err(|_| Error::Capacity)?
                .size();
        let physical_limit = notes.checked_mul(2).ok_or(Error::Capacity)?;
        let physical_bytes = std::alloc::Layout::array::<PhysicalPair>(physical_limit)
            .map_err(|_| Error::Capacity)?
            .size();
        let terminal_limit = notes.checked_mul(2).ok_or(Error::Capacity)?;
        let terminal_bytes = std::alloc::Layout::array::<TerminalRelease>(terminal_limit)
            .map_err(|_| Error::Capacity)?
            .size();
        bindings_bytes
            .checked_add(std::mem::size_of::<Self>())
            .and_then(|n| n.checked_add(reset_bytes))
            .and_then(|n| n.checked_add(changes_bytes))
            .and_then(|n| n.checked_add(note_changes_bytes))
            .and_then(|n| n.checked_add(physical_bytes))
            .and_then(|n| n.checked_add(terminal_bytes))
            .and_then(|n| {
                n.checked_add(super::per_note::PerNoteState::required_bytes(1 << group).ok()?)
            })
            .ok_or(Error::Capacity)?;
        let per_note = super::per_note::PerNoteState::new(1 << group)?;
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        let mut changes = Vec::new();
        changes
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        let mut note_changes = Vec::new();
        note_changes
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        let mut reset_members = Vec::new();
        reset_members
            .try_reserve_exact(notes)
            .map_err(|_| Error::Capacity)?;
        let mut physical = Vec::new();
        physical
            .try_reserve_exact(physical_limit)
            .map_err(|_| Error::Capacity)?;
        let mut terminal_releases = Vec::new();
        terminal_releases
            .try_reserve_exact(terminal_limit)
            .map_err(|_| Error::Capacity)?;
        let bytes = std::alloc::Layout::array::<Binding>(bindings.capacity())
            .map_err(|_| Error::Capacity)?
            .size()
            .checked_add(std::mem::size_of::<Self>())
            .ok_or(Error::Capacity)?;
        let bytes = bytes
            .checked_add(
                std::alloc::Layout::array::<(ExpressionId, Expression)>(changes.capacity())
                    .map_err(|_| Error::Capacity)?
                    .size(),
            )
            .ok_or(Error::Capacity)?;
        let bytes = bytes
            .checked_add(
                std::alloc::Layout::array::<(usize, super::per_note::Snapshot)>(
                    note_changes.capacity(),
                )
                .map_err(|_| Error::Capacity)?
                .size(),
            )
            .ok_or(Error::Capacity)?;
        let bytes = bytes
            .checked_add(
                std::alloc::Layout::array::<PhysicalPair>(physical.capacity())
                    .map_err(|_| Error::Capacity)?
                    .size(),
            )
            .ok_or(Error::Capacity)?;
        let bytes = bytes
            .checked_add(
                std::alloc::Layout::array::<usize>(reset_members.capacity())
                    .map_err(|_| Error::Capacity)?
                    .size(),
            )
            .ok_or(Error::Capacity)?;
        bytes
            .checked_add(
                std::alloc::Layout::array::<TerminalRelease>(terminal_releases.capacity())
                    .map_err(|_| Error::Capacity)?
                    .size(),
            )
            .and_then(|n| n.checked_add(per_note.retained_bytes().ok()?))
            .ok_or(Error::Capacity)?;
        // Explicit channel-less startup state is sampled once at receiver
        // construction. Later captured channel inputs remain independent.
        let raw = Value::Bits14((runtime.controller(performance, 128)? >> 18) as u16);
        let pressure = runtime.controller(performance, 129)?;
        let manager = Controls {
            pitch: pitch(raw, 2.),
            bend: pitch(raw, 1.),
            pressure,
            ..Controls::default()
        };
        let mut wire = [WireState::default(); 3];
        for state in &mut wire {
            state.controls[usize::from(zone.manager())] = manager;
            state.bends[usize::from(zone.manager())] = raw;
        }
        Ok(Self {
            registration: None,
            runtime: runtime.id(),
            performance,
            port,
            group,
            zone,
            members,
            fixed_members: members,
            zones: match zone {
                Zone::Lower => [members, 0],
                Zone::Upper => [0, members],
            },
            manual_active: true,
            configured: false,
            limit: notes,
            wire,
            managers: [[ManagerState {
                controls: manager,
                bend: raw,
            }; 16]; 3],
            host_projection: false,
            host_mpe_active: true,
            midi2_channel_voice: false,
            bend_override: None,
            bindings,
            changes,
            transpose: 0.0,
            timbre_controller: None,
            per_note,
            note_changes,
            physical,
            physical_limit,
            terminal_releases,
            terminal_limit,
            reset_members,
        })
    }

    /// Existing on-board fixed-zone mode before the first incoming MCM.
    /// A product using this adapter for ordinary manager-channel MIDI can keep
    /// that mode distinct from a manually selected MPE zone. Never overrides MCM.
    /// Pure source-component candidate. No bank, Core expression, callback,
    /// selector, retained source or renderer is changed while staging.
    pub(super) fn prepare_gesture(
        &self,
        domain: ChannelDomain,
        channel: u8,
        gesture: super::components::Gesture,
        value: Value,
    ) -> Result<super::components::GestureProposal, Error> {
        if !self.accepts_wire_channel(domain, channel) {
            return Err(Error::InvalidInput);
        }
        let domains = self.affected_domains(domain, channel);
        let mut controls = self.control_rows();
        let mut bends = [None; 3];
        let mut managers = self.managers;
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let selected = if target == ChannelDomain::HostChannel {
                self.host_channel(channel)
            } else {
                channel
            };
            let control = match gesture {
                super::components::Gesture::Pitch => {
                    let range = self
                        .bend_override
                        .map(f64::from)
                        .unwrap_or_else(|| self.sensitivity(target, selected).semitones());
                    bends[target.index()] = Some((selected, value));
                    Control::Pitch(pitch(value, range), pitch(value, 1.))
                }
                super::components::Gesture::Pressure => Control::Pressure(value.full_scale()),
                super::components::Gesture::Timbre => Control::Timbre(value),
                super::components::Gesture::Reset | super::components::Gesture::All => {
                    return Err(Error::InvalidInput);
                }
            };
            controls[target.index()][usize::from(selected)] =
                controls[target.index()][usize::from(selected)].with(control);
            if self.zone_domain(target) && selected == self.zone.manager() {
                for manager in &mut managers[target.index()] {
                    manager.controls = manager.controls.with(control);
                    if matches!(gesture, super::components::Gesture::Pitch) {
                        manager.bend = value;
                    }
                }
            }
        }
        Ok(super::components::GestureProposal {
            controls,
            managers,
            domains,
            channels: self.gesture_channels(channel),
            gesture,
            bends,
            all_notes: false,
        })
    }

    pub(super) fn project_component(
        &self,
        component: super::components::ReceiverComponents,
        old: Expression,
        proposal: super::components::GestureProposal,
        allowed: [u16; 3],
    ) -> Option<(Expression, super::components::ReceiverComponents)> {
        let domain = component.domain;
        let at = domain.index();
        if proposal.domains & domain.bit() == 0 || allowed[at] & (1 << component.channel) == 0 {
            return None;
        }
        let zone = self.zone_domain(domain);
        let channel = proposal.channels[at];
        let manager = zone && channel == Some(self.zone.manager());
        let matching = channel.is_none_or(|channel| component.channel == channel);
        let member = zone && component.channel != self.zone.manager();
        if !(proposal.all_notes || manager || (matching && (!zone || (member && component.held)))) {
            return None;
        }
        let value = if zone && component.channel == self.zone.manager() {
            Controls::default()
        } else if matching && (!zone || component.held) {
            proposal.controls[at][usize::from(component.channel)]
        } else {
            component.member
        };
        let manager = if zone {
            proposal.managers[at][usize::from(component.channel)].controls
        } else {
            Controls::default()
        };
        let mut combined = value.expression(manager);
        combined.pitch_semitones +=
            self.transpose + component.tuning + component.per_note.map_or(0., |state| state.pitch);
        let mut expression = old;
        match proposal.gesture {
            super::components::Gesture::Pitch => {
                expression.pitch_semitones = combined.pitch_semitones;
                expression.bend = combined.bend;
            }
            super::components::Gesture::Pressure => expression.pressure = combined.pressure,
            super::components::Gesture::Timbre => expression.timbre = combined.timbre,
            super::components::Gesture::Reset => {
                expression.pitch_semitones = combined.pitch_semitones;
                expression.bend = combined.bend;
                expression.pressure = combined.pressure;
            }
            super::components::Gesture::All => {
                expression.pitch_semitones = combined.pitch_semitones;
                expression.bend = combined.bend;
                expression.pressure = combined.pressure;
                expression.timbre = combined.timbre;
            }
        }
        let mut next = component;
        next.manager = manager;
        if (!zone || component.channel != self.zone.manager())
            && matching
            && (!zone || component.held)
        {
            next.member = proposal.controls[at][usize::from(component.channel)];
        }
        Some((expression, next))
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        if self.performance != other.performance
            || self.port != other.port
            || self.group != other.group
        {
            return false;
        }
        ChannelDomain::ALL.into_iter().any(|domain| {
            (0..16u8).any(|channel| {
                self.accepts_wire_channel(domain, channel)
                    && other.accepts_wire_channel(domain, channel)
            })
        })
    }

    pub fn set_manual_active(&mut self, active: bool) {
        if !self.configured {
            self.manual_active = active;
            self.host_mpe_active = active;
        }
    }

    /// Actual received MCM state, independent of the synth's selected zone.
    /// No message is inferred from note/channel traffic.
    pub fn configuration(&self) -> Option<[u8; 2]> {
        self.configured.then_some(self.zones)
    }

    /// Selected-zone acceptance, including the documented conventional manager
    /// fallback only when BOTH negotiated zones have been disabled.
    pub fn accepts_channel(&self, channel: u8) -> bool {
        if channel >= 16 {
            return false;
        }
        if self.configured && self.members == 0 {
            return self.zones == [0, 0] && channel == self.zone.manager();
        }
        self.zone.contains(channel, self.members)
    }

    /// Explicit setup-time native-host projection. Wire-only adapters keep
    /// strict independent protocols without paying for logical host pedal slots.
    pub fn enable_host_channel_projection(&mut self) -> Result<(), Error> {
        if !self.bindings.is_empty() {
            return Err(Error::InvalidInput);
        }
        self.host_projection = true;
        Ok(())
    }

    /// Ordinary MIDI2 channel-voice receiver mode. Existing explicit Mpe
    /// adapters retain their selected-zone compatibility mode by default;
    /// the product selects this real receiver law before any input owners.
    pub fn use_midi2_channel_voice(&mut self) -> Result<(), Error> {
        if !self.bindings.is_empty() {
            return Err(Error::InvalidInput);
        }
        if !self.midi2_channel_voice {
            let manager =
                self.wire[ChannelDomain::Midi2.index()].controls[usize::from(self.zone.manager())];
            let bend =
                self.wire[ChannelDomain::Midi2.index()].bends[usize::from(self.zone.manager())];
            self.wire[ChannelDomain::Midi2.index()].controls = [manager; 16];
            self.wire[ChannelDomain::Midi2.index()].bends = [bend; 16];
        }
        self.midi2_channel_voice = true;
        Ok(())
    }

    pub(super) fn zone_domain(&self, domain: ChannelDomain) -> bool {
        domain != ChannelDomain::Midi2 || !self.midi2_channel_voice
    }

    pub(super) fn accepts_wire_channel(&self, domain: ChannelDomain, channel: u8) -> bool {
        match domain {
            ChannelDomain::Midi1 => self.accepts_channel(channel),
            ChannelDomain::Midi2 => {
                channel < 16
                    && (self.midi2_channel_voice || self.zone.contains(channel, self.fixed_members))
            }
            ChannelDomain::HostChannel => {
                self.host_projection
                    && self.accepts_channel(channel)
                    && (self.host_mpe_active || channel == self.zone.manager())
            }
        }
    }

    pub(super) fn domain(protocol: Protocol) -> Result<ChannelDomain, Error> {
        match protocol {
            Protocol::Midi1 => Ok(ChannelDomain::Midi1),
            Protocol::Midi2 => Ok(ChannelDomain::Midi2),
            Protocol::Native => Ok(ChannelDomain::HostChannel),
            _ => Err(Error::InvalidInput),
        }
    }

    pub(super) fn control_rows(&self) -> [[Controls; 16]; 3] {
        self.wire.map(|wire| wire.controls)
    }

    pub(super) fn host_channel(&self, channel: u8) -> u8 {
        if self.host_mpe_active {
            channel
        } else {
            self.zone.manager()
        }
    }

    pub(super) fn gesture_channels(&self, channel: u8) -> [Option<u8>; 3] {
        [
            Some(channel),
            Some(channel),
            Some(self.host_channel(channel)),
        ]
    }

    pub(super) fn affected_domains(&self, source: ChannelDomain, channel: u8) -> u8 {
        source.bit()
            | if self.host_projection
                && self.accepts_wire_channel(ChannelDomain::HostChannel, self.host_channel(channel))
            {
                ChannelDomain::HostChannel.bit()
            } else {
                0
            }
    }

    pub(super) fn host_targets(&self, channel: u8, per_note: bool) -> Option<ChannelScope> {
        let channel = self.host_channel(channel);
        if !self.host_projection
            || !self.accepts_wire_channel(ChannelDomain::HostChannel, channel)
            || (!per_note && channel != self.zone.manager())
        {
            return None;
        }
        Some(ChannelScope {
            protocol: Protocol::Native,
            port: self.port,
            group: self.group,
            channels: if channel == self.zone.manager() {
                self.manager_scope(ChannelDomain::HostChannel).channels
            } else {
                1 << channel
            },
        })
    }

    pub(super) fn controller_channels(&self, domain: ChannelDomain, channel: u8) -> u16 {
        if self.zone_domain(domain) && channel == self.zone.manager() {
            self.manager_scope(domain).channels
        } else {
            1 << channel
        }
    }

    pub(super) fn sensitivity(&self, domain: ChannelDomain, channel: u8) -> Sensitivity {
        if domain == ChannelDomain::Midi2 && self.midi2_channel_voice {
            self.wire[domain.index()].channel_ranges[usize::from(channel)]
        } else {
            self.wire[domain.index()].ranges[usize::from(channel != self.zone.manager())]
        }
    }

    pub(super) fn expression_for(
        &self,
        domain: ChannelDomain,
        member: Controls,
        rows: &[[Controls; 16]; 3],
    ) -> Expression {
        if self.zone_domain(domain) {
            member.expression(rows[domain.index()][usize::from(self.zone.manager())])
        } else {
            member.expression(Controls::default())
        }
    }

    /// Also write each note's timbre (CC74) to its own member channel as this
    /// controller, so an instrument's dynamics position follows Y per note.
    pub fn set_timbre_controller(&mut self, controller: Option<u8>) {
        self.timbre_controller = controller.filter(|&c| c < 120);
    }

    /// Fix the bend range of every channel in whole semitones, whatever RPN 0
    /// says; `None` returns to RPN 0. Held bends keep their pitch until the
    /// next bend message.
    pub fn set_bend_range(&mut self, semitones: Option<u8>) {
        self.bend_override = semitones.filter(|&r| r <= 96);
    }

    /// Stored coarse manager and shared member bend sensitivities.
    pub fn pitch_ranges(&self) -> (u8, u8) {
        (self.wire[0].ranges[0].coarse, self.wire[0].ranges[1].coarse)
    }

    /// Effective manager/member sensitivities, including RPN 0 cents.
    pub fn pitch_sensitivities(&self) -> (f64, f64) {
        (
            self.wire[0].ranges[0].semitones(),
            self.wire[0].ranges[1].semitones(),
        )
    }

    /// Sensitivity in the actual wire protocol domain; MIDI1 RPN and MIDI2
    /// Registered Controllers cannot overwrite each other's retained ranges.
    pub fn pitch_sensitivities_for(&self, version: Version) -> (f64, f64) {
        let domain = if version == Version::Midi2 {
            ChannelDomain::Midi2
        } else {
            ChannelDomain::Midi1
        };
        (
            self.wire[domain.index()].ranges[0].semitones(),
            self.wire[domain.index()].ranges[1].semitones(),
        )
    }

    /// Actual channel range; ordinary MIDI2 has sixteen independent ranges,
    /// rather than borrowing the selected MIDI1 zone's member sensitivity.
    pub fn channel_pitch_sensitivity(&self, version: Version, channel: u8) -> Result<f64, Error> {
        if channel >= 16 {
            return Err(Error::InvalidInput);
        }
        let domain = if version == Version::Midi2 {
            ChannelDomain::Midi2
        } else {
            ChannelDomain::Midi1
        };
        Ok(self.sensitivity(domain, channel).semitones())
    }

    /// Manager channel for a part's keyboard and shared performance controls.
    pub fn manager_channel(&self) -> u8 {
        self.zone.manager()
    }

    /// Apply at the current sample boundary, after due native work. Only this
    /// adapter should admit external notes in its configured input domain.
    pub fn apply(
        &mut self,
        runtime: &mut Runtime,
        packet: Option<ChannelVoice>,
    ) -> Result<Applied, ApplyError> {
        self.apply_with_attack(runtime, packet, false, None)
    }

    /// Preserve MIDI pairing and gestures, leaving note attacks to a script owner.
    pub fn apply_silent(
        &mut self,
        runtime: &mut Runtime,
        packet: Option<ChannelVoice>,
    ) -> Result<Applied, ApplyError> {
        self.apply_with_attack(runtime, packet, true, None)
    }

    /// Preserve a transport's physical tuple before the first normal note
    /// callback, independently of the adapter's execution channel mapping.
    pub fn apply_with_source(
        &mut self,
        runtime: &mut Runtime,
        packet: Option<ChannelVoice>,
        source: Input,
    ) -> Result<Applied, ApplyError> {
        if source.group >= 16 || source.channel >= 16 || source.key >= 128 {
            return Err(Error::InvalidInput.into());
        }
        self.apply_with_attack(runtime, packet, false, Some(source))
    }

    pub fn apply_silent_with_source(
        &mut self,
        runtime: &mut Runtime,
        packet: Option<ChannelVoice>,
        source: Input,
    ) -> Result<Applied, ApplyError> {
        if source.group >= 16 || source.channel >= 16 || source.key >= 128 {
            return Err(Error::InvalidInput.into());
        }
        self.apply_with_attack(runtime, packet, true, Some(source))
    }

    pub(super) fn apply_with_attack(
        &mut self,
        runtime: &mut Runtime,
        packet: Option<ChannelVoice>,
        silent: bool,
        source: Option<Input>,
    ) -> Result<Applied, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        if source
            .is_some_and(|source| source.group >= 16 || source.channel >= 16 || source.key >= 128)
        {
            return Err(Error::InvalidInput.into());
        }
        let Some(voice) = packet else {
            return Ok(Applied::Unsupported);
        };
        if !voice.valid() {
            return Err(Error::InvalidInput.into());
        }
        if voice.group != self.group {
            return Err(ApplyError::DisabledGroup);
        }
        let midi2 = voice.version == Version::Midi2;
        let domain = if midi2 {
            ChannelDomain::Midi2
        } else {
            ChannelDomain::Midi1
        };
        let component_action = matches!(
            voice.message,
            Message::PitchBend(_)
                | Message::ChannelPressure(_)
                | Message::Control { index: 121, .. }
        ) || matches!(voice.message, Message::Control { index: 74, .. })
            && self.zone_domain(domain);
        if self.registration.is_none()
            && component_action
            && runtime
                .plans
                .get(runtime.active_plan.0)
                .is_some_and(|plan| {
                    plan.prepared
                        .stages
                        .iter()
                        .any(|stage| stage.controller.is_some())
                })
        {
            // An unbound facade has no canonical component publication authority.
            // Refuse before raw/cache/expression mutation rather than bypassing
            // the authored consume/transform/wait chain.
            return Err(Error::InvalidInput.into());
        }
        if !midi2
            && let Message::Control {
                index,
                value: Value::Bits7(value),
            } = voice.message
        {
            let selected = self.wire[0].parameters[usize::from(voice.channel)];
            if selected.registered && selected.rpn == [0, 6] {
                match index {
                    6 => {
                        return self
                            .configure_zone(runtime, voice.channel, value)
                            .map_err(Into::into);
                    }
                    // MCM uses the data-entry MSB only. Relative data entry is
                    // not a zone-size operation and cannot leak into CC state.
                    38 | 96 | 97 => return Ok(Applied::Ignored),
                    _ => {}
                }
            }
            // The other zone manager must be able to configure overlap even
            // when it is currently outside this selected instrument's range.
            if !self.accepts_wire_channel(domain, voice.channel)
                && matches!(voice.channel, 0 | 15)
                && matches!(index, 98..=101)
            {
                return self
                    .parameter(runtime, voice.channel, index, value)
                    .map_err(Into::into);
            }
        }
        let terminal_source = if let Message::NoteOff { key, .. } = voice.message {
            Some(source.unwrap_or(Input {
                protocol: domain.protocol(),
                port: self.port,
                group: self.group,
                channel: voice.channel,
                key,
                external_id: None,
            }))
        } else {
            None
        };
        let captured_terminal = terminal_source.is_some_and(|source| {
            self.physical
                .iter()
                .any(|pair| pair.kind == PhysicalKind::Wire && pair.source == source)
        });
        if !self.accepts_wire_channel(domain, voice.channel) && !captured_terminal {
            return Ok(if self.configured || terminal_source.is_some() {
                Applied::Ignored
            } else {
                Applied::Unsupported
            });
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        // Preserve the accepted-input due-work boundary even for a harmless
        // unmatched terminal; it cannot invent a recipient or a callback.
        if terminal_source.is_some() && !captured_terminal {
            return Ok(Applied::Ignored);
        }
        let input = Input {
            protocol: domain.protocol(),
            port: self.port,
            group: self.group,
            channel: voice.channel,
            key: 0,
            external_id: None,
        };
        Ok(match voice.message {
            Message::NoteOn {
                key,
                velocity,
                attribute,
            } if attribute.kind == 0 || midi2 => Applied::Started(self.admit(
                runtime,
                voice.channel,
                (
                    Input { key, ..input },
                    source,
                    if midi2 && attribute.kind == 3 {
                        NotePitch::Absolute(f64::from(attribute.data) / 512.)
                    } else {
                        NotePitch::Key(key)
                    },
                ),
                // Native MIDI2 velocity0 remains a real silent gate; the MIDI1
                // translation minimum does not belong to this receiver.
                velocity.normalized(),
                silent,
                0.0,
            )?),
            Message::NoteOff {
                key,
                velocity,
                attribute,
            } => Applied::Released {
                note: self.release_physical(
                    runtime,
                    source.unwrap_or(Input { key, ..input }),
                    velocity.map(Value::normalized),
                )?,
                velocity,
                attribute,
            },
            Message::PitchBend(value @ (Value::Bits14(_) | Value::Bits32(_))) => {
                self.bend(runtime, domain, voice.channel, value)?
            }
            Message::ChannelPressure(value @ (Value::Bits7(_) | Value::Bits32(_))) => self
                .control(
                    runtime,
                    domain,
                    voice.channel,
                    Control::Pressure(value.full_scale()),
                )?,
            Message::PolyPressure { key, value } if midi2 => {
                let (_, owners) = runtime.dispatch_poly_pressure(
                    self.performance,
                    input.channel_address(),
                    key,
                    value.full_scale(),
                )?;
                Applied::Expression { owners }
            }
            Message::Control {
                index: 121,
                value: Value::Bits7(0) | Value::Bits32(0),
            } => {
                // The supported MPE1.1 selected-zone profile handles this
                // zone-wide message on its manager, like pedals/AllNotesOff.
                if self.zone_domain(domain) && voice.channel != self.zone.manager() {
                    Applied::Ignored
                } else {
                    self.reset_channel(runtime, domain, voice.channel)?
                }
            }
            Message::Control {
                index: index @ (120 | 123),
                value: Value::Bits7(0) | Value::Bits32(0),
            } => {
                if self.zone_domain(domain) && voice.channel != self.zone.manager() {
                    Applied::Ignored
                } else {
                    self.channel_terminal(runtime, domain, voice.channel, index == 120)?
                }
            }
            Message::PerNotePitch { key, value } if midi2 => {
                self.per_note_bend(runtime, voice.channel, key, value)?
            }
            Message::PerNoteManagement { key, detach, reset } if midi2 => {
                self.per_note_management(runtime, voice.channel, key, detach, reset)?
            }
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 0,
                index: 7,
                value,
            } if midi2 => {
                self.per_note_range(runtime, voice.channel, f64::from(value) / 33_554_432.)?
            }
            // MIDI 2.0 Registered Controller 0:0, pitch bend sensitivity
            // (semitones in the top 7 bits, cents in the next 7).
            Message::ChannelControl {
                space: Controllers::Registered,
                bank: 0,
                index: 0,
                value,
            } => self.range(
                runtime,
                domain,
                voice.channel,
                Sensitivity {
                    coarse: (value >> 25) as u8,
                    cents: ((value >> 18) & 127) as u8,
                },
            )?,
            Message::Control {
                index: index @ (64 | 66),
                value: value @ (Value::Bits7(_) | Value::Bits32(_)),
            } => {
                if self.zone_domain(domain) && voice.channel != self.zone.manager() {
                    Applied::Ignored
                } else {
                    let channels = self.controller_channels(domain, voice.channel);
                    self.dispatch_controller(
                        runtime,
                        input.channel_address(),
                        channels,
                        index,
                        value.full_scale(),
                        false,
                    )?;
                    Applied::Pedal
                }
            }
            Message::Control {
                index: 74,
                value: value @ (Value::Bits7(_) | Value::Bits32(_)),
            } if !self.zone_domain(domain) => {
                // Ordinary MIDI2 CC74 is channel Sound Controller5, not an
                // MPE member gesture. Preserve its actual number/precision and
                // captured script consume/transform/forward boundary.
                self.dispatch_controller(
                    runtime,
                    input.channel_address(),
                    self.controller_channels(domain, voice.channel),
                    74,
                    value.full_scale(),
                    false,
                )?;
                Applied::Controller
            }
            Message::Control {
                index: 74,
                value: value @ (Value::Bits7(_) | Value::Bits32(_)),
            } => {
                if let Some(cc) = self.timbre_controller {
                    self.dispatch_controller(
                        runtime,
                        input.channel_address(),
                        self.controller_channels(domain, voice.channel),
                        cc,
                        value.full_scale(),
                        true,
                    )?;
                }
                self.control(runtime, domain, voice.channel, Control::Timbre(value))?
            }
            // MIDI 2.0 carries RPNs as Registered Controllers, not CC 6/38/98..=101.
            Message::Control { index, .. } if midi2 && super::is_midi2_legacy_control(index) => {
                Applied::Ignored
            }
            Message::Control {
                index: index @ (6 | 38 | 96..=101),
                value: Value::Bits7(value),
            } => {
                self.dispatch_controller(
                    runtime,
                    input.channel_address(),
                    self.controller_channels(domain, voice.channel),
                    index,
                    Value::Bits7(value).full_scale(),
                    true,
                )?;
                self.parameter(runtime, voice.channel, index, value)?
            }
            Message::Control { index, value }
                if index < 120
                    && !matches!(index, 96 | 97)
                    && (!self.zone_domain(domain) || voice.channel == self.zone.manager()) =>
            {
                self.dispatch_controller(
                    runtime,
                    input.channel_address(),
                    self.controller_channels(domain, voice.channel),
                    index,
                    value.full_scale(),
                    false,
                )?;
                Applied::Controller
            }
            _ => Applied::Unsupported,
        })
    }

    /// Admit a note whose identity another transport owns (a host note ID) as
    /// if it arrived on `channel` of this zone, so the zone's pedals and gestures
    /// reach it. `input` must lie in this zone's MIDI 1.0 port and group for
    /// pedals to hold it; its `external_id` keeps it distinct from wire notes.
    pub fn trigger(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(runtime, channel, (input, None), velocity, 0.0, false)
    }

    /// Host-owned attack with its initial per-note tuning, composed independently
    /// with member/manager bends and the part's transpose.
    pub fn trigger_with_tuning(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
        tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(runtime, channel, (input, None), velocity, tuning, false)
    }

    /// Host-owned physical gate whose authored script will select the attack.
    pub fn trigger_silent(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(runtime, channel, (input, None), velocity, 0.0, true)
    }

    /// The same silent host admission with initial native note tuning present
    /// before the authored script can create expression-linked child notes.
    pub fn trigger_silent_with_tuning(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        velocity: f64,
        tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(runtime, channel, (input, None), velocity, tuning, true)
    }

    /// Host transport source retained before synchronous native/KSP callbacks.
    pub fn trigger_with_source(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        source: Input,
        velocity: f64,
        tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(
            runtime,
            channel,
            (input, Some(source)),
            velocity,
            tuning,
            false,
        )
    }

    /// The same source-bearing admission when an external script owns attack.
    pub fn trigger_silent_with_source(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        input: Input,
        source: Input,
        velocity: f64,
        tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.trigger_with_attack(
            runtime,
            channel,
            (input, Some(source)),
            velocity,
            tuning,
            true,
        )
    }

    pub(super) fn trigger_with_attack(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        inputs: (Input, Option<Input>),
        velocity: f64,
        tuning: f64,
        silent: bool,
    ) -> Result<NoteId, ApplyError> {
        let (input, source) = inputs;
        if source
            .is_some_and(|source| source.group >= 16 || source.channel >= 16 || source.key >= 128)
        {
            return Err(Error::InvalidInput.into());
        }
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        let domain = Self::domain(input.protocol)?;
        if !self.accepts_wire_channel(domain, channel)
            || !tuning.is_finite()
            || (domain == ChannelDomain::HostChannel
                && (input.port != self.port
                    || input.group != self.group
                    || input.channel != channel))
        {
            return Err(Error::InvalidInput.into());
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        Ok(self.admit(
            runtime,
            channel,
            (input, source, NotePitch::Key(input.key)),
            velocity,
            silent,
            tuning,
        )?)
    }

    /// Update one exact host note's retained tuning component. Later member or
    /// manager bends preserve it; this never retargets another note on a reused
    /// channel, and all unrelated expression dimensions remain unchanged.
    pub fn set_note_tuning(
        &mut self,
        runtime: &mut Runtime,
        note: NoteId,
        tuning: f64,
    ) -> Result<(), Error> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle);
        }
        if !tuning.is_finite() {
            return Err(Error::InvalidInput);
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let index = self
            .bindings
            .iter()
            .position(|binding| binding.note == note)
            .ok_or(Error::StaleHandle)?;
        let binding = self.bindings[index];
        let combined = self.expression_for(binding.domain, binding.member, &self.control_rows());
        let owner = runtime.expression_id(note)?;
        let mut expression = runtime.expression(owner)?;
        expression.pitch_semitones = combined.pitch_semitones
            + self.transpose
            + tuning
            + binding.per_note.map_or(0., |state| state.pitch);
        runtime.set_expression(owner, expression)?;
        self.bindings[index].tuning = tuning;
        Ok(())
    }

    /// Offset every note of the zone, held or not, by `semitones` on top of its
    /// bends (a part's tuning). Commits only if every owner accepts it.
    pub fn transpose(
        &mut self,
        runtime: &mut Runtime,
        semitones: f64,
    ) -> Result<Applied, ApplyError> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle.into());
        }
        if !semitones.is_finite() {
            return Err(Error::InvalidInput.into());
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let previous = std::mem::replace(&mut self.transpose, semitones);
        self.project(
            runtime,
            self.control_rows(),
            0b111,
            [None; 3],
            true,
            Control::Pitch(0.0, 0.0),
        )
        .inspect_err(|_| self.transpose = previous)
        .map_err(Into::into)
    }

    pub(super) fn admit(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        inputs: (Input, Option<Input>, NotePitch),
        velocity: f64,
        silent: bool,
        tuning: f64,
    ) -> Result<NoteId, Error> {
        let (input, source, note_pitch) = inputs;
        let source = source.unwrap_or(input);
        if source.group >= 16 || source.channel >= 16 || source.key >= 128 {
            return Err(Error::InvalidInput);
        }
        if !tuning.is_finite() {
            return Err(Error::InvalidInput);
        }
        let domain = Self::domain(input.protocol)?;
        let kind = if domain != ChannelDomain::HostChannel {
            Some(PhysicalKind::Wire)
        } else if native_source(source) {
            Some(PhysicalKind::Native)
        } else {
            None
        };
        if kind == Some(PhysicalKind::Native)
            && source.external_id.is_some_and(|id| id >= 0)
            && self.physical.iter().any(|pair| {
                pair.kind == PhysicalKind::Native
                    && pair.source == source
                    && !pair.terminal_received
            })
        {
            return Err(Error::DuplicateInput);
        }
        if self.bindings.len() == self.limit {
            return Err(Error::Capacity);
        }
        let wire_debt = self
            .physical
            .iter()
            .filter(|pair| pair.kind == PhysicalKind::Wire)
            .count();
        let native_debt = self
            .physical
            .iter()
            .filter(|pair| pair.kind == PhysicalKind::Native)
            .count()
            + self
                .bindings
                .iter()
                .filter(|binding| {
                    binding.domain == ChannelDomain::HostChannel && !native_source(binding.source)
                })
                .count();
        if kind == Some(PhysicalKind::Wire) && wire_debt == self.limit
            || domain == ChannelDomain::HostChannel && native_debt == self.limit
            || kind.is_some() && self.physical.len() == self.physical_limit
        {
            return Err(Error::Capacity);
        }
        let member = if self.zone_domain(domain) && channel == self.zone.manager() {
            Controls::default()
        } else {
            self.wire[domain.index()].controls[usize::from(channel)]
        };
        let per_note = if domain == ChannelDomain::Midi2 {
            Some(
                self.per_note
                    .snapshot(self.group, input.channel, input.key)?,
            )
        } else {
            None
        };
        let mut expression = self.expression_for(domain, member, &self.control_rows());
        expression.pitch_semitones += self.transpose;
        expression.pitch_semitones += tuning;
        expression.pitch_semitones += per_note.map_or(0., |state| state.pitch);
        let note = if silent {
            let note = runtime.note_on_pitched_in(
                self.performance,
                input,
                note_pitch,
                velocity,
                expression,
            )?;
            runtime.set_input_source(note, source)?;
            note
        } else {
            runtime.trigger_in_with_source(
                self.performance,
                input,
                source,
                note_pitch,
                velocity,
                expression,
            )?
        };
        if let Some(kind) = kind {
            self.physical.push(PhysicalPair {
                note,
                expression: None,
                execution: input,
                source,
                kind,
                released: false,
                scripted: silent,
                terminal_received: false,
                choked: false,
            });
        }
        self.bindings.push(Binding {
            note,
            channel,
            address: input.channel_address(),
            domain,
            member,
            tuning,
            key: input.key,
            per_note,
            source,
            scripted: silent,
            terminal_released: false,
        });
        Ok(note)
    }

    /// Physical pair ownership survives Core cancellation/retirement. Never
    /// borrow a different source's earlier same-key note in a collapsed Part.
    pub(super) fn release_physical(
        &mut self,
        runtime: &mut Runtime,
        source: Input,
        velocity: Option<f64>,
    ) -> Result<NoteId, Error> {
        if source.group >= 16 || source.channel >= 16 || source.key >= 128 {
            return Err(Error::InvalidInput);
        }
        let at = self
            .physical
            .iter()
            .position(|pair| pair.kind == PhysicalKind::Wire && pair.source == source)
            .ok_or(Error::InvalidInput)?;
        let note = self.physical[at].note;
        match runtime.note_off_exact(note, velocity) {
            Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
            Err(error) => return Err(error),
        }
        self.physical.remove(at);
        Ok(note)
    }

    /// Deliver queued synthetic physical releases exactly once. A later real
    /// key-up consumes its retained tombstone, not another source's owner.
    pub fn drain_terminal_releases(&mut self, mut receive: impl FnMut(TerminalRelease)) {
        self.terminal_releases.drain(..).for_each(&mut receive);
        self.physical.retain(|pair| !pair.terminal_received);
    }

    /// Full slot count, including raw rows; native_physical_at is an O(1)
    /// generation-fenced query. Callers never collect this population in HELD scratch.
    pub fn native_physical_slots(&self) -> usize {
        self.physical.len()
    }
    pub fn native_physical_at(&self, index: usize) -> Option<(NoteId, Input, bool)> {
        self.physical
            .get(index)
            .filter(|pair| pair.kind == PhysicalKind::Native && !pair.terminal_received)
            .map(|pair| (pair.note, pair.source, pair.released))
    }
    pub fn has_native_physical_note(&self, note: NoteId) -> bool {
        self.physical
            .iter()
            .any(|pair| pair.kind == PhysicalKind::Native && pair.note == note)
            || self
                .terminal_releases
                .iter()
                .any(|release| native_source(release.source) && release.note == note)
    }
    pub fn has_native_physical_source(&self, source: Input) -> bool {
        self.physical.iter().any(|pair| {
            pair.kind == PhysicalKind::Native && pair.source == source && !pair.terminal_received
        })
    }
    pub fn validate_native_physical_releases(
        &self,
        matches: impl Fn(NoteId, Input) -> bool,
    ) -> Result<(), Error> {
        let needed = self
            .physical
            .iter()
            .filter(|pair| {
                pair.kind == PhysicalKind::Native
                    && !pair.terminal_received
                    && !pair.released
                    && pair.scripted
                    && matches(pair.note, pair.source)
            })
            .count();
        if self
            .terminal_releases
            .len()
            .checked_add(needed)
            .is_none_or(|n| n > self.terminal_limit)
        {
            return Err(Error::Capacity);
        }
        Ok(())
    }
    /// Operate exact retained rows selected by a pure full-tuple predicate.
    /// Generation identity survives Core retirement; no source/key fallback exists.
    pub fn release_native_physical(
        &mut self,
        runtime: &mut Runtime,
        matches: impl Fn(NoteId, Input) -> bool,
        velocity: Option<f64>,
    ) -> Result<usize, Error> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle);
        }
        if velocity.is_some_and(|v| !(0.0..=1.0).contains(&v)) {
            return Err(Error::InvalidInput);
        }
        self.validate_native_physical_releases(&matches)?;
        let mut count = 0;
        for index in 0..self.physical.len() {
            let pair = self.physical[index];
            if pair.kind != PhysicalKind::Native
                || pair.terminal_received
                || !matches(pair.note, pair.source)
            {
                continue;
            }
            match runtime.note_off_exact(pair.note, velocity) {
                Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
                Err(error) => return Err(error),
            }
            if !pair.released && pair.scripted {
                self.terminal_releases.push(TerminalRelease {
                    note: pair.note,
                    source: pair.source,
                    velocity,
                });
            }
            self.physical[index].released = true;
            self.physical[index].terminal_received = true;
            count += 1;
        }
        // The exact pending receipt owns its own captured NoteId/source/value.
        // Physical acknowledgment frees debt even if that receipt has not yet
        // reached the application callback boundary.
        self.physical.retain(|pair| !pair.terminal_received);
        Ok(count)
    }
    pub fn choke_native_physical(
        &mut self,
        runtime: &mut Runtime,
        matches: impl Fn(NoteId, Input) -> bool,
    ) -> Result<usize, Error> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle);
        }
        let mut count = 0;
        for index in 0..self.physical.len() {
            let pair = self.physical[index];
            if pair.kind != PhysicalKind::Native
                || pair.terminal_received
                || pair.choked
                || !matches(pair.note, pair.source)
            {
                continue;
            }
            match runtime.cancel_input_note(pair.note) {
                Ok(()) | Err(Error::StaleHandle) | Err(Error::ClosedNote) => {}
                Err(error) => return Err(error),
            }
            self.physical[index].choked = true;
            count += 1;
        }
        Ok(count)
    }

    /// Pure complete queue admission for transport stop. Physical pairs retain
    /// exact source tuples even when their original Core generation has retired.
    pub fn validate_all_physical_releases(&self) -> Result<(), Error> {
        let needed = self
            .physical
            .iter()
            .filter(|pair| pair.scripted && !pair.released)
            .count();
        if self
            .terminal_releases
            .len()
            .checked_add(needed)
            .is_none_or(|total| total > self.terminal_limit)
        {
            return Err(Error::Capacity);
        }
        Ok(())
    }

    /// Queue musical physical key-ups for the ordinary owner boundary, including
    /// actual source-backed Native owners after their Core roots have retired.
    /// No live Core lookup or key-only fallback is used here. The caller then
    /// drains the queue and invokes the existing global Core key/pedal release.
    pub fn queue_all_physical_releases(&mut self) -> Result<usize, Error> {
        self.validate_all_physical_releases()?;
        let mut count = 0;
        for pair in &mut self.physical {
            if pair.released {
                continue;
            }
            if pair.scripted {
                self.terminal_releases.push(TerminalRelease {
                    note: pair.note,
                    source: pair.source,
                    velocity: None,
                });
            }
            pair.released = true;
            count += 1;
        }
        Ok(count)
    }

    pub(super) fn channel_terminal(
        &mut self,
        runtime: &mut Runtime,
        domain: ChannelDomain,
        channel: u8,
        sound_off: bool,
    ) -> Result<Applied, Error> {
        let channels = self.controller_channels(domain, channel);
        // This is the same explicitly enabled Part HostChannel projection as
        // accepted channel gestures, never a raw key-only Native lookup.
        let host = if sound_off {
            None
        } else {
            self.host_targets(channel, false)
        };
        let raw = self
            .physical
            .iter()
            .filter(|pair| {
                pair.scripted
                    && !pair.released
                    && (pair.kind == PhysicalKind::Wire
                        && pair.execution.protocol == domain.protocol()
                        && channels & (1 << pair.execution.channel) != 0
                        || pair.kind == PhysicalKind::Native
                            && host.is_some_and(|scope| {
                                scope.channels & (1 << pair.execution.channel) != 0
                            }))
            })
            .count();
        let host_count = host.map_or(0, |scope| {
            self.bindings
                .iter()
                .filter(|binding| {
                    binding.domain == ChannelDomain::HostChannel
                        && !native_source(binding.source)
                        && binding.scripted
                        && !binding.terminal_released
                        && scope.channels & (1 << binding.channel) != 0
                        && runtime.input_held(binding.note).unwrap_or(false)
                })
                .count()
        });
        if !sound_off
            && self
                .terminal_releases
                .len()
                .checked_add(raw)
                .and_then(|n| n.checked_add(host_count))
                .is_none_or(|n| n > self.terminal_limit)
        {
            return Err(Error::Capacity);
        }
        // Capture exact native IDs/sources before all_notes_off can retire
        // them. Queue rows remain private until success; refusal rolls back.
        let pending = self.terminal_releases.len();
        if let Some(scope) = host {
            for binding in &self.bindings {
                if binding.domain == ChannelDomain::HostChannel
                    && !native_source(binding.source)
                    && binding.scripted
                    && !binding.terminal_released
                    && scope.channels & (1 << binding.channel) != 0
                    && runtime.input_held(binding.note).unwrap_or(false)
                {
                    self.terminal_releases.push(TerminalRelease {
                        note: binding.note,
                        source: binding.source,
                        velocity: None,
                    });
                }
            }
        }
        let result = (|| {
            let mut count = 0;
            for channel in 0..16u8 {
                if channels & (1 << channel) != 0 {
                    let address = crate::ChannelAddress {
                        protocol: domain.protocol(),
                        port: self.port,
                        group: self.group,
                        channel,
                    };
                    count += if sound_off {
                        runtime.all_sound_off(address)?
                    } else {
                        runtime.all_notes_off(address)?
                    };
                }
                if let Some(scope) = host
                    && scope.channels & (1 << channel) != 0
                {
                    count += runtime.all_notes_off(crate::ChannelAddress {
                        protocol: scope.protocol,
                        port: scope.port,
                        group: scope.group,
                        channel,
                    })?;
                }
            }
            Ok::<usize, Error>(count)
        })();
        let count = match result {
            Ok(count) => count,
            Err(error) => {
                self.terminal_releases.truncate(pending);
                return Err(error);
            }
        };
        if !sound_off {
            for pair in &mut self.physical {
                if !pair.released
                    && (pair.kind == PhysicalKind::Wire
                        && pair.execution.protocol == domain.protocol()
                        && channels & (1 << pair.execution.channel) != 0
                        || pair.kind == PhysicalKind::Native
                            && host.is_some_and(|scope| {
                                scope.channels & (1 << pair.execution.channel) != 0
                            }))
                {
                    if pair.scripted {
                        self.terminal_releases.push(TerminalRelease {
                            note: pair.note,
                            source: pair.source,
                            velocity: None,
                        });
                    }
                    pair.released = true;
                }
            }
            if let Some(scope) = host {
                for binding in &mut self.bindings {
                    if binding.domain == ChannelDomain::HostChannel
                        && scope.channels & (1 << binding.channel) != 0
                    {
                        binding.terminal_released = true;
                    }
                }
            }
        }
        // CC120 is a hard raw-domain sound kill, not a fictional physical
        // key release. Its Lua gate remains until actual NoteOff.
        Ok(if sound_off {
            Applied::AllSoundOff { stopped: count }
        } else {
            Applied::AllNotesOff { released: count }
        })
    }

    pub(super) fn reset_channel(
        &mut self,
        runtime: &mut Runtime,
        domain: ChannelDomain,
        channel: u8,
    ) -> Result<Applied, Error> {
        // apply_with_attack already drained due work and pruned stale owners.
        let primary = ChannelScope {
            protocol: domain.protocol(),
            port: self.port,
            group: self.group,
            channels: self.controller_channels(domain, channel),
        };
        let secondary = self.host_targets(channel, false);
        runtime.validate_protocol_controller_reset_targets(self.performance, primary, secondary)?;
        let mut controls = self.control_rows();
        for (target, scope) in [
            (domain, Some(primary)),
            (ChannelDomain::HostChannel, secondary),
        ] {
            if let Some(scope) = scope {
                for channel in 0..16u8 {
                    if scope.channels & (1 << channel) != 0 {
                        let value = &mut controls[target.index()][usize::from(channel)];
                        value.pitch = 0.;
                        value.bend = 0.;
                        value.pressure = 0;
                    }
                }
            }
        }
        self.changes.clear();
        self.reset_members.clear();
        for (index, binding) in self.bindings.iter().enumerate() {
            let scope = if binding.domain == domain {
                Some(primary)
            } else if binding.domain == ChannelDomain::HostChannel {
                secondary
            } else {
                None
            };
            let Some(scope) = scope else {
                continue;
            };
            if scope.channels & (1 << binding.channel) == 0 {
                continue;
            }
            let held = runtime.input_held(binding.note)?;
            let zone = self.zone_domain(binding.domain);
            let member = if zone && binding.channel == self.zone.manager() {
                Controls::default()
            } else if zone && !held {
                binding.member
            } else {
                controls[binding.domain.index()][usize::from(binding.channel)]
            };
            let combined = self.expression_for(binding.domain, member, &controls);
            let owner = runtime.expression_id(binding.note)?;
            let old = runtime.expression(owner)?;
            self.changes.push((
                owner,
                Expression {
                    pitch_semitones: combined.pitch_semitones
                        + self.transpose
                        + binding.tuning
                        + binding.per_note.map_or(0., |value| value.pitch),
                    bend: combined.bend,
                    pressure: combined.pressure,
                    ..old
                },
            ));
            if (!zone || binding.channel != self.zone.manager()) && (!zone || held) {
                self.reset_members.push(index);
            }
        }
        runtime.validate_expressions(&self.changes)?;
        runtime.set_expressions(&self.changes)?;
        // No callback/admission lies between the union preflight and bank
        // commit; expression publication does not alter any reset epoch/row.
        runtime.reset_protocol_controller_targets(self.performance, primary, secondary)?;
        for (target, scope) in [
            (domain, Some(primary)),
            (ChannelDomain::HostChannel, secondary),
        ] {
            if let Some(scope) = scope {
                self.wire[target.index()].controls = controls[target.index()];
                for channel in 0..16u8 {
                    if scope.channels & (1 << channel) != 0 {
                        self.wire[target.index()].bends[usize::from(channel)] =
                            if target == ChannelDomain::Midi1 {
                                Value::Bits14(8192)
                            } else {
                                Value::Bits32(CENTER)
                            };
                        self.wire[target.index()].parameters[usize::from(channel)] =
                            Parameter::default();
                    }
                }
            }
        }
        for index in self.reset_members.drain(..) {
            let binding = &mut self.bindings[index];
            binding.member = controls[binding.domain.index()][usize::from(binding.channel)];
        }
        Ok(Applied::ResetControllers)
    }

    pub(super) fn per_note_bend(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        key: u8,
        bend: u32,
    ) -> Result<Applied, Error> {
        let (id, before) = self.per_note.key(self.group, channel, key)?;
        let channel_id = self.per_note.channel(self.group, channel)?;
        let next = super::per_note::KeyState { bend, ..before };
        let projected =
            super::per_note::PerNoteState::project(next, self.per_note.range(channel_id));
        self.changes.clear();
        for binding in &self.bindings {
            if binding.domain != ChannelDomain::Midi2
                || binding.channel != channel
                || binding.key != key
            {
                continue;
            }
            let Some(previous) = binding
                .per_note
                .filter(|state| state.cohort == before.cohort)
            else {
                continue;
            };
            let owner = runtime.expression_id(binding.note)?;
            let mut value = runtime.expression(owner)?;
            value.pitch_semitones += projected.pitch - previous.pitch;
            self.changes.push((owner, value));
        }
        runtime.set_expressions(&self.changes)?;
        for binding in &mut self.bindings {
            if binding.domain == ChannelDomain::Midi2
                && binding.channel == channel
                && binding.key == key
                && binding
                    .per_note
                    .is_some_and(|state| state.cohort == before.cohort)
            {
                binding.per_note = Some(projected);
            }
        }
        self.per_note.commit_key(id, next);
        Ok(Applied::Expression {
            owners: self.changes.len(),
        })
    }

    pub(super) fn per_note_management(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        key: u8,
        detach: bool,
        reset: bool,
    ) -> Result<Applied, Error> {
        if !detach && !reset {
            return Ok(Applied::Ignored);
        }
        let (id, before) = self.per_note.key(self.group, channel, key)?;
        let next = super::per_note::PerNoteState::managed(before, detach, reset)?;
        if !detach && reset {
            self.per_note_bend(runtime, channel, key, super::per_note::CENTER)?;
        }
        // D precedes S: no old owner is reset/rebound when both are set.
        self.per_note.commit_key(id, next);
        Ok(Applied::Configuration)
    }

    pub(super) fn per_note_range(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        range: f64,
    ) -> Result<Applied, Error> {
        let id = self.per_note.channel(self.group, channel)?;
        self.changes.clear();
        self.note_changes.clear();
        for (index, binding) in self.bindings.iter().enumerate() {
            if binding.domain != ChannelDomain::Midi2 || binding.channel != channel {
                continue;
            }
            let (_, key) = self.per_note.key(self.group, channel, binding.key)?;
            let Some(previous) = binding.per_note.filter(|state| state.cohort == key.cohort) else {
                continue;
            };
            let next = super::per_note::PerNoteState::project(key, range);
            let owner = runtime.expression_id(binding.note)?;
            let mut value = runtime.expression(owner)?;
            value.pitch_semitones += next.pitch - previous.pitch;
            self.changes.push((owner, value));
            self.note_changes.push((index, next));
        }
        runtime.set_expressions(&self.changes)?;
        for (index, next) in self.note_changes.drain(..) {
            self.bindings[index].per_note = Some(next);
        }
        self.per_note.commit_range(id, range);
        Ok(Applied::Configuration)
    }

    pub(super) fn dispatch_controller(
        &self,
        runtime: &mut Runtime,
        origin: crate::ChannelAddress,
        channels: u16,
        number: u8,
        value: u32,
        native_per_note: bool,
    ) -> Result<(), Error> {
        runtime.dispatch_controller_with_host_targets(
            self.performance,
            origin,
            channels,
            number,
            value,
            self.host_targets(origin.channel, native_per_note),
        )?;
        Ok(())
    }

    pub(super) fn manager_scope(&self, domain: ChannelDomain) -> ChannelScope {
        let members = if domain == ChannelDomain::Midi2 {
            self.fixed_members
        } else {
            self.members
        };
        let width = (1u32 << (members + 1)) - 1;
        ChannelScope {
            protocol: domain.protocol(),
            port: self.port,
            group: self.group,
            channels: match self.zone {
                Zone::Lower => width as u16,
                Zone::Upper => (width << (15 - members)) as u16,
            },
        }
    }

    pub(super) fn control(
        &mut self,
        runtime: &mut Runtime,
        domain: ChannelDomain,
        channel: u8,
        control: Control,
    ) -> Result<Applied, Error> {
        let mut controls = self.control_rows();
        let domains = self.affected_domains(domain, channel);
        for target in ChannelDomain::ALL {
            if domains & target.bit() != 0 {
                let channel = if target == ChannelDomain::HostChannel {
                    self.host_channel(channel)
                } else {
                    channel
                };
                controls[target.index()][usize::from(channel)] =
                    controls[target.index()][usize::from(channel)].with(control);
            }
        }
        self.project(
            runtime,
            controls,
            domains,
            self.gesture_channels(channel),
            false,
            control,
        )
    }

    /// Project an already accepted script virtual-controller write in its real
    /// logical channel domain, without synthesizing input or rerunning callbacks.
    pub fn apply_script_controller(
        &mut self,
        runtime: &mut Runtime,
        address: crate::ChannelAddress,
        number: u8,
        value: u32,
    ) -> Result<Applied, Error> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle);
        }
        if address.port != self.port || address.group != self.group || !matches!(number, 128 | 129)
        {
            return Err(Error::InvalidInput);
        }
        let domain = Self::domain(address.protocol)?;
        if !self.accepts_wire_channel(domain, address.channel) {
            return Err(Error::InvalidInput);
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let mut controls = self.control_rows();
        let channel = usize::from(address.channel);
        let control = if number == 128 {
            let raw = Value::Bits14((value >> 18) as u16);
            let range = self
                .bend_override
                .map(f64::from)
                .unwrap_or_else(|| self.sensitivity(domain, address.channel).semitones());
            Control::Pitch(pitch(raw, range), pitch(raw, 1.))
        } else {
            Control::Pressure(value)
        };
        controls[domain.index()][channel] = controls[domain.index()][channel].with(control);
        let mut channels = [None; 3];
        channels[domain.index()] = Some(address.channel);
        let result = self.project(runtime, controls, domain.bit(), channels, false, control)?;
        if number == 128 {
            self.wire[domain.index()].bends[channel] = Value::Bits14((value >> 18) as u16);
        }
        Ok(result)
    }

    /// One channel-less instrument gesture. Each zone receives its manager
    /// component once; ordinary MIDI2 channels retain their own sensitivities.
    pub fn apply_global_script_controller(
        &mut self,
        runtime: &mut Runtime,
        number: u8,
        value: u32,
    ) -> Result<Applied, Error> {
        if runtime.id() != self.runtime {
            return Err(Error::StaleHandle);
        }
        if !matches!(number, 128 | 129) {
            return Err(Error::InvalidInput);
        }
        runtime.render(&mut [])?;
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let mut controls = self.control_rows();
        let raw = Value::Bits14((value >> 18) as u16);
        let mut domains = 0;
        for domain in ChannelDomain::ALL {
            if domain == ChannelDomain::HostChannel && !self.host_projection {
                continue;
            }
            domains |= domain.bit();
            for channel in 0..16u8 {
                if self.zone_domain(domain) && channel != self.zone.manager() {
                    continue;
                }
                let control = if number == 128 {
                    let range = self
                        .bend_override
                        .map(f64::from)
                        .unwrap_or_else(|| self.sensitivity(domain, channel).semitones());
                    Control::Pitch(pitch(raw, range), pitch(raw, 1.))
                } else {
                    Control::Pressure(value)
                };
                controls[domain.index()][usize::from(channel)] =
                    controls[domain.index()][usize::from(channel)].with(control);
            }
        }
        let dimension = if number == 128 {
            Control::Pitch(0., 0.)
        } else {
            Control::Pressure(0)
        };
        let result = self.project(runtime, controls, domains, [None; 3], true, dimension)?;
        if number == 128 {
            for domain in ChannelDomain::ALL {
                if domains & domain.bit() == 0 {
                    continue;
                }
                for channel in 0..16u8 {
                    if !self.zone_domain(domain) || channel == self.zone.manager() {
                        self.wire[domain.index()].bends[usize::from(channel)] = raw;
                    }
                }
            }
        }
        Ok(result)
    }

    pub(super) fn bend(
        &mut self,
        runtime: &mut Runtime,
        domain: ChannelDomain,
        channel: u8,
        value: Value,
    ) -> Result<Applied, Error> {
        let domains = self.affected_domains(domain, channel);
        let mut controls = self.control_rows();
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let channel = if target == ChannelDomain::HostChannel {
                self.host_channel(channel)
            } else {
                channel
            };
            let range = self
                .bend_override
                .map(f64::from)
                .unwrap_or_else(|| self.sensitivity(target, channel).semitones());
            controls[target.index()][usize::from(channel)] = controls[target.index()]
                [usize::from(channel)]
            .with(Control::Pitch(pitch(value, range), pitch(value, 1.)));
        }
        let applied = self.project(
            runtime,
            controls,
            domains,
            self.gesture_channels(channel),
            false,
            Control::Pitch(0., 0.),
        )?;
        for target in ChannelDomain::ALL {
            if domains & target.bit() != 0 {
                let channel = if target == ChannelDomain::HostChannel {
                    self.host_channel(channel)
                } else {
                    channel
                };
                self.wire[target.index()].bends[usize::from(channel)] = value;
            }
        }
        Ok(applied)
    }

    pub(super) fn project(
        &mut self,
        runtime: &mut Runtime,
        controls: [[Controls; 16]; 3],
        domains: u8,
        channels: [Option<u8>; 3],
        all_notes: bool,
        control: Control,
    ) -> Result<Applied, Error> {
        self.changes.clear();
        for binding in &self.bindings {
            if domains & binding.domain.bit() == 0 {
                continue;
            }
            let zone = self.zone_domain(binding.domain);
            let channel = channels[binding.domain.index()];
            let manager = zone && channel == Some(self.zone.manager());
            let matching = channel.is_none_or(|channel| binding.channel == channel);
            let member = zone && binding.channel != self.zone.manager();
            let held = runtime.input_held(binding.note)?;
            if all_notes || manager || (matching && ((!zone) || (member && held))) {
                let owner = runtime.expression_id(binding.note)?;
                let mut expression = runtime.expression(owner)?;
                let value = if zone && binding.channel == self.zone.manager() {
                    Controls::default()
                } else if matching && (!zone || held) {
                    controls[binding.domain.index()][usize::from(binding.channel)]
                } else {
                    binding.member
                };
                let mut combined = self.expression_for(binding.domain, value, &controls);
                combined.pitch_semitones += self.transpose
                    + binding.tuning
                    + binding.per_note.map_or(0., |state| state.pitch);
                match control {
                    Control::Pitch(..) => {
                        expression.pitch_semitones = combined.pitch_semitones;
                        expression.bend = combined.bend;
                    }
                    Control::Pressure(_) => expression.pressure = combined.pressure,
                    Control::Timbre(_) => expression.timbre = combined.timbre,
                }
                self.changes.push((owner, expression));
            }
        }
        runtime.set_expressions(&self.changes)?;
        for (wire, controls) in self.wire.iter_mut().zip(controls) {
            wire.controls = controls;
        }
        let ordinary_midi2 = self.midi2_channel_voice;
        for binding in &mut self.bindings {
            if domains & binding.domain.bit() == 0 {
                continue;
            }
            let zone = binding.domain != ChannelDomain::Midi2 || !ordinary_midi2;
            let channel = channels[binding.domain.index()];
            if (!zone || binding.channel != self.zone.manager())
                && channel.is_none_or(|channel| binding.channel == channel)
                && (!zone || runtime.input_held(binding.note)?)
            {
                binding.member = controls[binding.domain.index()][usize::from(binding.channel)];
            }
        }
        Ok(Applied::Expression {
            owners: self.changes.len(),
        })
    }
    pub(super) fn configure_zone(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        members: u8,
    ) -> Result<Applied, Error> {
        let zone = match channel {
            0 => Zone::Lower,
            15 => Zone::Upper,
            _ => return Ok(Applied::Ignored),
        };
        if members > 15 {
            return Ok(Applied::Ignored);
        }
        let mut zones = if self.configured || self.manual_active {
            self.zones
        } else {
            [0, 0]
        };
        zones[zone.index()] = members;
        if members > 0 {
            // Two managers need their own channels. A newly claimed manager
            // channel disables its old zone; otherwise shrink its member edge.
            let other = 1 - zone.index();
            let maximum = 14u8.saturating_sub(members);
            zones[other] = zones[other].min(maximum);
        }
        let role = |zones: [u8; 2], channel: u8| -> u8 {
            for zone in [Zone::Lower, Zone::Upper] {
                let members = zones[zone.index()];
                if members > 0 && zone.contains(channel, members) {
                    return 1 + (zone.index() as u8) * 2 + u8::from(channel != zone.manager());
                }
            }
            0
        };
        let previous = if self.configured || self.manual_active {
            self.zones
        } else {
            [0, 0]
        };
        let changed = (0..16u8).fold(0u16, |mask, channel| {
            mask | if role(previous, channel) != role(zones, channel) {
                1 << channel
            } else {
                0
            }
        });
        runtime.render(&mut [])?;
        let scope = ChannelScope {
            protocol: Protocol::Midi1,
            port: self.port,
            group: self.group,
            channels: changed,
        };
        let host_scope = self.host_projection.then_some(ChannelScope {
            protocol: Protocol::Native,
            port: self.port,
            group: self.group,
            channels: changed,
        });
        if changed != 0 {
            runtime.validate_channel_controller_reset_targets(
                self.performance,
                scope,
                host_scope,
            )?;
        }
        let domains = ChannelDomain::Midi1.bit()
            | if self.host_projection {
                ChannelDomain::HostChannel.bit()
            } else {
                0
            };
        self.bindings
            .retain(|binding| runtime.note(binding.note).is_ok());
        let port = self.port;
        let group = self.group;
        let changed_owner = |binding: &Binding| {
            domains & binding.domain.bit() != 0
                && (changed & (1 << binding.channel) != 0
                    || (binding.address.protocol == binding.domain.protocol()
                        && binding.address.port == port
                        && binding.address.group == group
                        && changed & (1 << binding.address.channel) != 0))
        };
        let mut controls = self.control_rows();
        let reset_sensitivity = zone == self.zone;
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            for channel in 0..16u8 {
                if changed & (1 << channel) != 0 {
                    controls[target.index()][usize::from(channel)] = Controls::default();
                }
                if reset_sensitivity && self.zone.contains(channel, zones[self.zone.index()]) {
                    let range = if channel == self.zone.manager() {
                        2.
                    } else {
                        48.
                    };
                    let bend = if changed & (1 << channel) != 0 {
                        Value::Bits14(8192)
                    } else {
                        self.wire[target.index()].bends[usize::from(channel)]
                    };
                    controls[target.index()][usize::from(channel)].pitch = pitch(bend, range);
                }
            }
        }
        self.changes.clear();
        for binding in &self.bindings {
            if domains & binding.domain.bit() == 0
                || changed_owner(binding)
                || runtime.note(binding.note).is_err()
            {
                continue;
            }
            let row = &controls[binding.domain.index()];
            let manager = row[usize::from(self.zone.manager())];
            let member = if binding.channel == self.zone.manager() {
                Controls::default()
            } else if runtime.input_held(binding.note)? {
                row[usize::from(binding.channel)]
            } else {
                binding.member
            };
            let combined = member.expression(manager);
            let owner = runtime.expression_id(binding.note)?;
            let old = runtime.expression(owner)?;
            self.changes.push((
                owner,
                Expression {
                    pitch_semitones: combined.pitch_semitones
                        + self.transpose
                        + binding.tuning
                        + binding.per_note.map_or(0., |state| state.pitch),
                    bend: combined.bend,
                    pressure: combined.pressure,
                    timbre: combined.timbre,
                    ..old
                },
            ));
        }
        runtime.validate_expressions(&self.changes)?;
        runtime.set_expressions(&self.changes)?;
        for binding in &self.bindings {
            if changed_owner(binding) {
                match runtime.cancel_input_note(binding.note) {
                    Ok(()) | Err(Error::StaleHandle) => {}
                    Err(error) => return Err(error),
                }
            }
        }
        if changed != 0 {
            runtime.reset_channel_controller_targets(self.performance, scope, host_scope)?;
        }
        for channel in 0..16u8 {
            if changed & (1 << channel) == 0 {
                continue;
            }
            for target in ChannelDomain::ALL {
                if domains & target.bit() == 0 {
                    continue;
                }
                runtime.all_sound_off(crate::ChannelAddress {
                    protocol: target.protocol(),
                    port: self.port,
                    group: self.group,
                    channel,
                })?;
            }
        }
        self.bindings
            .retain(|binding| !changed_owner(binding) && runtime.note(binding.note).is_ok());
        for binding in &mut self.bindings {
            if domains & binding.domain.bit() != 0
                && binding.channel != self.zone.manager()
                && runtime.input_held(binding.note)?
            {
                binding.member = controls[binding.domain.index()][usize::from(binding.channel)];
            }
        }
        let selector = self.wire[0].parameters[usize::from(channel)];
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let wire = &mut self.wire[target.index()];
            wire.controls = controls[target.index()];
            for channel in 0..16usize {
                if changed & (1 << channel) != 0 {
                    wire.bends[channel] = Value::Bits14(8192);
                    wire.parameters[channel] = Parameter::default();
                }
            }
            if reset_sensitivity {
                wire.ranges = [
                    Sensitivity {
                        coarse: 2,
                        cents: 0,
                    },
                    Sensitivity {
                        coarse: 48,
                        cents: 0,
                    },
                ];
            }
        }
        self.wire[0].parameters[usize::from(channel)] = selector;
        if reset_sensitivity {
            self.bend_override = None;
        }
        self.zones = zones;
        self.members = zones[self.zone.index()];
        self.configured = true;
        self.host_mpe_active = true;
        Ok(Applied::Configuration)
    }

    pub(super) fn parameter(
        &mut self,
        runtime: &mut Runtime,
        channel: u8,
        index: u8,
        value: u8,
    ) -> Result<Applied, Error> {
        let parameter = &mut self.wire[0].parameters[usize::from(channel)];
        match index {
            100 | 101 => {
                parameter.rpn[usize::from(index == 100)] = value;
                parameter.registered = true;
                Ok(Applied::Configuration)
            }
            98 | 99 => {
                parameter.registered = false;
                Ok(Applied::Configuration)
            }
            6 if parameter.registered && parameter.rpn == [0, 0] => self.range(
                runtime,
                ChannelDomain::Midi1,
                channel,
                Sensitivity {
                    coarse: value,
                    cents: 0,
                },
            ),
            38 if parameter.registered && parameter.rpn == [0, 0] => {
                let coarse =
                    self.wire[0].ranges[usize::from(channel != self.zone.manager())].coarse;
                self.range(
                    runtime,
                    ChannelDomain::Midi1,
                    channel,
                    Sensitivity {
                        coarse,
                        cents: value,
                    },
                )
            }
            96 | 97 if parameter.registered && parameter.rpn == [0, 0] => {
                let value = self.wire[0].ranges[usize::from(channel != self.zone.manager())]
                    .relative(index == 96);
                self.range(runtime, ChannelDomain::Midi1, channel, value)
            }
            _ => Ok(Applied::Unsupported),
        }
    }

    pub(super) fn range(
        &mut self,
        runtime: &mut Runtime,
        domain: ChannelDomain,
        channel: u8,
        value: Sensitivity,
    ) -> Result<Applied, Error> {
        if value.semitones() > 96. {
            return Err(Error::InvalidInput);
        }
        let domains = self.affected_domains(domain, channel);
        let mut controls = self.control_rows();
        let mut channels = [None; 3];
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let target_channel = if target == ChannelDomain::HostChannel {
                self.host_channel(channel)
            } else {
                channel
            };
            let manager = target_channel == self.zone.manager();
            let zone = self.zone_domain(target);
            channels[target.index()] = if zone {
                manager.then_some(target_channel)
            } else {
                Some(target_channel)
            };
            if self.bend_override.is_some() {
                continue;
            }
            for (index, control) in controls[target.index()].iter_mut().enumerate() {
                let ch = index as u8;
                if (!zone && ch == target_channel)
                    || (zone
                        && ((manager && ch == self.zone.manager())
                            || (!manager
                                && ch != self.zone.manager()
                                && self.accepts_wire_channel(target, ch))))
                {
                    control.pitch =
                        pitch(self.wire[target.index()].bends[index], value.semitones());
                }
            }
        }
        let applied = if self.bend_override.is_none() {
            self.project(
                runtime,
                controls,
                domains,
                channels,
                false,
                Control::Pitch(0., 0.),
            )?
        } else {
            Applied::Configuration
        };
        for target in ChannelDomain::ALL {
            if domains & target.bit() == 0 {
                continue;
            }
            let target_channel = if target == ChannelDomain::HostChannel {
                self.host_channel(channel)
            } else {
                channel
            };
            if target == ChannelDomain::Midi2 && self.midi2_channel_voice {
                self.wire[target.index()].channel_ranges[usize::from(target_channel)] = value;
            } else {
                self.wire[target.index()].ranges
                    [usize::from(target_channel != self.zone.manager())] = value;
            }
        }
        Ok(applied)
    }
}

pub(super) fn pitch(value: Value, range: f64) -> f64 {
    let (value, center) = match value {
        Value::Bits32(v) => (f64::from(v), f64::from(CENTER)),
        Value::Bits14(v) => (f64::from(v), 8192.0),
        other => (f64::from(other.full_scale()), f64::from(CENTER)),
    };
    let centered = value - center;
    // Exact center and endpoints; MPE permits meaningful receiver combination.
    range * centered / if centered < 0.0 { center } else { center - 1.0 }
}
