//! Live release admission: actual voice Off and the corresponding native
//! release sample share one physical-pedal/capture decision. Callers borrow
//! their existing root or queued snapshot; no future input is projected.
use super::{Ahdsr, Bank, EventId, Expression, GroupMask, HostRef, NoteEvent, PerNoteInput,
    instrument::InstrumentState, player::Player};
use crate::midi::Velocity;

/// The queued physical-root fallback applies only without an actual attack.
/// Exact Host roots additionally retain capture alongside existing attacks.
pub(super) enum Capture {
    Actual,
    RootAdditional(bool),
    PhysicalFallback(bool),
}

pub(super) struct ReleaseRequest<'a> {
    pub source: EventId,
    pub release_velocity: Option<Velocity>,
    pub capture: Capture,
    /// None releases a source without creating a native release sample.
    pub sample: Option<ReleaseSample<'a>>,
}

pub(super) enum ReleaseSample<'a> {
    FromVoice(&'a GroupMask),
    Snapshot(ReleaseSnapshot<'a>),
}

/// Immutable accepted input/command geometry. None expression remains None;
/// script modulation storage belongs to the existing input or queue payload.
pub(super) struct ReleaseSnapshot<'a> {
    pub channel: u8,
    pub note: u8,
    pub velocity: Velocity,
    pub groups: &'a GroupMask,
    pub expression: Option<Expression>,
    pub input_channel: Option<u8>,
    pub host_note: Option<HostRef>,
    pub per_note: Option<PerNoteInput>,
    pub script_mods: Option<&'a [i32]>,
    pub held_ms: Option<f32>,
}

struct ReleasedVoice {
    channel: u8,
    note: u8,
    velocity: Velocity,
    captured: bool,
    input_channel: Option<u8>,
}

/// One released event, retaining the script's release-sample selection until
/// its pedals let go. Prepared storage also covers events with no attack zones.
#[derive(Clone, Copy)]
pub(super) struct PendingRelease {
    pub(super) host_note: Option<HostRef>,
    pub(super) per_note: Option<super::PerNoteInput>,
    pub(super) source: EventId,
    pub(super) input_channel: Option<u8>,
    pub(super) channel: u8,
    pub(super) note: u8,
    pub(super) onset_velocity: Velocity,
    pub(super) release_velocity: Option<Velocity>,
    pub(super) script_mods: [i32; super::SCRIPT_MOD_SOURCES],
    pub(super) groups: GroupMask,
    pub(super) captured: bool,
    pub(super) expression: Option<Expression>,
    pub(super) held_ms: f32,
}

impl Player {
    /// Returns whether this Off found an ordinary attack voice. The sample
    /// admission follows the same actual receipt, including release-only roots.
    pub(super) fn accept_release(&mut self, bank: &Bank, state: &InstrumentState,
        request: ReleaseRequest<'_>, defaults: Ahdsr) -> bool {
        let actual = self.release_voices(bank, state, request.source, request.release_velocity);
        let found = actual.is_some();
        let captured = actual.as_ref().is_some_and(|voice| voice.captured);
        let latched = match request.capture {
            Capture::Actual => captured,
            Capture::RootAdditional(root) => captured || root,
            Capture::PhysicalFallback(root) => if found { captured } else { root },
        };
        let snapshot = match request.sample {
            None => None,
            Some(ReleaseSample::Snapshot(snapshot)) => Some(snapshot),
            Some(ReleaseSample::FromVoice(groups)) => actual.map(|voice| ReleaseSnapshot {
                channel: voice.channel,
                note: voice.note,
                velocity: voice.velocity,
                groups,
                expression: self.release_expression(request.source, voice.channel, voice.note, voice.input_channel),
                input_channel: voice.input_channel,
                host_note: None,
                per_note: None,
                script_mods: None,
                held_ms: None,
            }),
        };
        if let Some(snapshot) = snapshot {
            self.trigger_release(bank, state, request.source, snapshot, latched, request.release_velocity, defaults);
        }
        found
    }

    /// Release one event's voices; a held sustain pedal defers them like a key
    /// release. Native release samples run independently of further note-off
    /// events. Returns the first ordinary voice's channel, note and velocity.
    fn release_voices(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        id: EventId,
        release_velocity: Option<Velocity>,
    ) -> Option<ReleasedVoice> {
        let mut first = None;
        for v in self
            .voices
            .iter_mut()
            .filter(|v| v.event == id && !v.released && !v.release_trigger)
        {
            let event = first.get_or_insert(ReleasedVoice {
                channel: v.channel,
                note: v.note,
                velocity: v.onset_velocity,
                captured: false,
                input_channel: v.input_channel,
            });
            if let Some(value) = release_velocity {
                v.release_velocity = Some(value);
                v.settled = None;
            }
            event.captured |= v.sostenuto;
            v.counter_stop.get_or_insert(self.now);
            let mut source = v.source_note();
            if source.key_up(&self.pedals)
                && !super::ahdsr_shared::release_admitted(state.shared_ahdsr.as_deref(),v,self.now,&self.stream_error) { continue; }
            if v.source_key_up(&self.pedals) {
                v.release(bank, &state.settings, &mut self.free);
            }
        }
        first
    }

    /// Start the release-trigger zones for a key release, limited to `groups`;
    /// pedals defer the exact event, velocity and group mask until pedal-up.
    fn trigger_release(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        source: EventId,
        snapshot: ReleaseSnapshot<'_>,
        latched: bool,
        release_velocity: Option<Velocity>,
        defaults: Ahdsr,
    ) {
        if !self.native_release_triggers {
            return;
        }
        let ReleaseSnapshot { channel, note, velocity: onset_velocity, groups, expression,
            input_channel, host_note: root, per_note, script_mods, held_ms: captured_held_ms } = snapshot;
        let mut release_mods=[0;super::SCRIPT_MOD_SOURCES];
        let mods=script_mods.or_else(||self.voices.iter().find(|v|v.event==source).map(|v|v.script_mods.as_slice())).unwrap_or(&[]);
        let len=mods.len().min(release_mods.len());release_mods[..len].copy_from_slice(&mods[..len]);
        let host_note = root
            .or_else(|| {
                self.voices
                    .iter()
                    .find(|v| v.event == source)
                    .and_then(|v| v.host_note)
            })
            .or_else(|| {
                self.host_notes
                    .active
                    .iter()
                    .copied()
                    .find(|r| self.host_notes.get(*r).is_some_and(|o| o.event.live() == Some(source)))
            });
        let per_note = per_note
            .or_else(|| self.voices.iter().find(|v| v.event == source).and_then(|v| v.per_note))
            .or_else(|| host_note.and_then(|r| self.host_notes.get(r)).and_then(|o| o.per_note));
        let release_velocity = release_velocity
            .or_else(|| {
                self.voices
                    .iter()
                    .find(|v| v.event == source)
                    .and_then(|v| v.release_velocity)
            })
            .or_else(|| {
                host_note
                    .and_then(|r| self.host_notes.get(r))
                    .and_then(|o| o.release_velocity)
            });
        let held_ms = captured_held_ms.unwrap_or_else(|| {
            host_note.and_then(|r| self.host_notes.get(r)).map_or_else(
                || self.event_held_ms(source, channel, note),
                |o| o.stop.unwrap_or(self.now).saturating_sub(o.start) as f32 * 1000. / self.rate as f32,
            )
        });
        if channel < 16 && note < 128 && self.pedals.holds(input_channel.unwrap_or(channel), false, latched) {
            if self.pending_releases.iter().any(|r| r.source == source) {
                return;
            }
            if self.pending_releases.len() == crate::ksp::EVENT_CAPACITY {
                // Only the excess release sample is lost; voice and pedal
                // lifetimes have already advanced, and the loss is counted.
                self.dropped_commands += 1;
                return;
            }
            self.pending_releases.push(PendingRelease {
                host_note,
                per_note,
                source,
                input_channel,
                channel,
                note,
                onset_velocity,
                release_velocity,
                script_mods: release_mods,
                groups: *groups,
                captured: latched,
                expression,
                held_ms,
            });
            return;
        }
        let id = self.next_id();
        let event = NoteEvent {
            script_mods: &release_mods,
            host_note,
            per_note,
            velocity: if host_note.is_some() {
                onset_velocity.midi1()
            } else {
                onset_velocity.midi1().max(1)
            },
            groups: Some(groups),
            native_release: true,
            frozen_expression: expression,
            release_held_ms: Some(held_ms),
            input_channel,
            release_velocity,
            ..NoteEvent::new_velocity(channel, note, onset_velocity)
        };
        self.start(bank, state, &event, id, true, defaults);
    }
}
