//! Typed MIDI decoder facade. Unbound mode owns the actual prepared Core
//! receiver state; consuming bind transfers it rather than cloning a cache.
use crate::{Applied,ApplyError,Packet,Version};
use sampler_core::{Error,Input,NoteId,PerformanceId,Runtime,OwnedReceiver};
pub use sampler_core::{ReceiverZone as Zone,ReceiverTerminalRelease as TerminalRelease};
pub struct Mpe { pub(crate) state:OwnedReceiver }
impl Mpe {
    pub fn new(
        runtime: &Runtime,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        OwnedReceiver::new(runtime, port, group, zone, members, notes).map(|state|Self{state})
    }

    pub fn new_in(
        runtime: &Runtime,
        performance: PerformanceId,
        port: u16,
        group: u8,
        zone: Zone,
        members: u8,
        notes: usize,
    ) -> Result<Self, Error> {
        OwnedReceiver::new_in(runtime, performance, port, group, zone, members, notes).map(|state|Self{state})
    }

    pub fn set_manual_active(&mut self, active: bool) {
        self.state.set_manual_active(active)
    }

    pub fn configuration(&self) -> Option<[u8; 2]> {
        self.state.configuration()
    }

    pub fn accepts_channel(&self, channel: u8) -> bool {
        self.state.accepts_channel(channel)
    }

    pub fn enable_host_channel_projection(&mut self) -> Result<(), Error> {
        self.state.enable_host_channel_projection()
    }

    pub fn use_midi2_channel_voice(&mut self) -> Result<(), Error> {
        self.state.use_midi2_channel_voice()
    }

    pub fn set_timbre_controller(&mut self, controller: Option<u8>) {
        self.state.set_timbre_controller(controller)
    }

    pub fn set_bend_range(&mut self, semitones: Option<u8>) {
        self.state.set_bend_range(semitones)
    }

    pub fn pitch_ranges(&self) -> (u8, u8) {
        self.state.pitch_ranges()
    }

    pub fn pitch_sensitivities(&self) -> (f64, f64) {
        self.state.pitch_sensitivities()
    }

    pub fn pitch_sensitivities_for(&self, version: Version) -> (f64, f64) {
        self.state.pitch_sensitivities_for(version)
    }

    pub fn channel_pitch_sensitivity(&self, version: Version, channel: u8) -> Result<f64, Error> {
        self.state.channel_pitch_sensitivity(version, channel)
    }

    pub fn manager_channel(&self) -> u8 {
        self.state.manager_channel()
    }

    pub fn apply(
        &mut self,
        runtime: &mut Runtime,
        packet: Packet<'_>,
    ) -> Result<Applied, ApplyError> {
        self.state.apply(runtime, packet.channel_voice())
    }

    pub fn apply_silent(
        &mut self,
        runtime: &mut Runtime,
        packet: Packet<'_>,
    ) -> Result<Applied, ApplyError> {
        self.state.apply_silent(runtime, packet.channel_voice())
    }

    pub fn apply_with_source(&mut self, runtime: &mut Runtime, packet: Packet<'_>, source: Input) -> Result<Applied, ApplyError> {
        self.state.apply_with_source(runtime, packet.channel_voice(), source)
    }

    pub fn apply_silent_with_source(&mut self, runtime: &mut Runtime, packet: Packet<'_>, source: Input) -> Result<Applied, ApplyError> {
        self.state.apply_silent_with_source(runtime, packet.channel_voice(), source)
    }

    pub fn trigger(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, velocity: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger(runtime, channel, input, velocity)
    }

    pub fn trigger_with_tuning(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, velocity: f64, tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger_with_tuning(runtime, channel, input, velocity, tuning)
    }

    pub fn trigger_silent(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, velocity: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger_silent(runtime, channel, input, velocity)
    }

    pub fn trigger_silent_with_tuning(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, velocity: f64, tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger_silent_with_tuning(runtime, channel, input, velocity, tuning)
    }

    pub fn trigger_with_source(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, source: Input,
        velocity: f64, tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger_with_source(runtime, channel, input, source, velocity, tuning)
    }

    pub fn trigger_silent_with_source(
        &mut self, runtime: &mut Runtime, channel: u8, input: Input, source: Input,
        velocity: f64, tuning: f64,
    ) -> Result<NoteId, ApplyError> {
        self.state.trigger_silent_with_source(runtime, channel, input, source, velocity, tuning)
    }

    pub fn set_note_tuning(
        &mut self, runtime: &mut Runtime, note: NoteId, tuning: f64,
    ) -> Result<(), Error> {
        self.state.set_note_tuning(runtime, note, tuning)
    }

    pub fn transpose(
        &mut self,
        runtime: &mut Runtime,
        semitones: f64,
    ) -> Result<Applied, ApplyError> {
        self.state.transpose(runtime, semitones)
    }

    pub fn drain_terminal_releases(&mut self, mut receive:impl FnMut(TerminalRelease)) {
        self.state.drain_terminal_releases(receive)
    }

    pub fn native_physical_slots(&self)->usize {
        self.state.native_physical_slots()
    }

    pub fn native_physical_at(&self,index:usize)->Option<(NoteId,Input,bool)> {
        self.state.native_physical_at(index)
    }

    pub fn has_native_physical_note(&self,note:NoteId)->bool {
        self.state.has_native_physical_note(note)
    }

    pub fn has_native_physical_source(&self,source:Input)->bool {
        self.state.has_native_physical_source(source)
    }

    pub fn validate_native_physical_releases(&self,matches:impl Fn(NoteId,Input)->bool)->Result<(),Error> {
        self.state.validate_native_physical_releases(matches)
    }

    pub fn release_native_physical(&mut self,runtime:&mut Runtime,matches:impl Fn(NoteId,Input)->bool,velocity:Option<f64>)->Result<usize,Error> {
        self.state.release_native_physical(runtime, matches, velocity)
    }

    pub fn choke_native_physical(&mut self,runtime:&mut Runtime,matches:impl Fn(NoteId,Input)->bool)->Result<usize,Error> {
        self.state.choke_native_physical(runtime, matches)
    }

    pub fn validate_all_physical_releases(&self)->Result<(),Error> {
        self.state.validate_all_physical_releases()
    }

    pub fn queue_all_physical_releases(&mut self)->Result<usize,Error> {
        self.state.queue_all_physical_releases()
    }

    pub fn apply_script_controller(
        &mut self, runtime: &mut Runtime, address: sampler_core::ChannelAddress,
        number: u8, value: u32,
    ) -> Result<Applied, Error> {
        self.state.apply_script_controller(runtime, address, number, value)
    }

    pub fn apply_global_script_controller(&mut self, runtime: &mut Runtime, number: u8, value: u32) -> Result<Applied, Error> {
        self.state.apply_global_script_controller(runtime, number, value)
    }

    /// Explicit offaudio owner transfer. No registration happens in apply/render.
    pub fn bind(self,runtime:&mut Runtime)->Result<crate::BoundMpe,Error> {
        let manager=self.state.manager_channel();
        let id=runtime.bind_receiver(self.state)?;
        Ok(crate::BoundMpe::new(runtime.id(),id,manager))
    }
}

impl Mpe {
    /// A full decision roster observed an unrecordable attack. This is a
    /// fail-closed source-debt fence; CC/reset/panic do not rearm it.
    pub fn wire_fenced(&self)->bool{self.state.wire_fenced()}
}
