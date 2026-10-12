//! Explicit KONTRA note allocation. Saved Kontakt flags and MIDI mode CCs do
//! not select this policy. Held candidates remain in the canonical root rows.
use super::{
    Ahdsr, Bank, Engine, EventId, GroupMask, HostRef, NoteEvent, STEAL_FADE,
    instrument::InstrumentState,
    player::{InputKey, Player},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NoteMode {
    #[default]
    Poly,
    /// Last accepted held note wins on each delivered channel. Every change
    /// starts a fresh sample and envelope; replacement uses the normal steal fade.
    MonoRetrigger,
}

/// Inline masks cost 512 bytes per prepared root, with no callback allocation.
/// Raw input without a successfully published source has no accepted receipt.
#[derive(Clone, Copy)]
pub(super) struct AcceptedNote {
    pub event: EventId,
    pub ordinal: u64,
    pub groups: GroupMask,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Root {
    Input { channel: u8, key: u8 },
    Host(HostRef),
}

#[derive(Clone, Copy)]
pub(super) struct Selected {
    pub root: Root,
    pub event: EventId,
    pub ordinal: u64,
}

pub(super) fn resident_sources(bank: &Bank) -> bool {
    bank.samples.iter().all(|sample| {
        !sample.streamed
            && usize::try_from(sample.frames).is_ok_and(|frames| sample.resident_window(0, frames))
    })
}

impl Engine {
    /// Local authored policy, independent of imported native modes and CC126.
    /// Configure an idle delivered channel after installing its bank. Initially
    /// only script-free resident sources without a sealed shared range qualify.
    /// Shared LFOs keep their group-owned waveform. Shared pitch admission also
    /// requires a current host range declared by begin_audio_block before input.
    /// Bank replacement, reset and script installation restore Poly.
    pub fn set_note_mode(&mut self, channel: u8, mode: NoteMode) -> anyhow::Result<()> {
        anyhow::ensure!(channel < 16, "Invalid note mode channel");
        anyhow::ensure!(
            self.script.is_none(),
            "Note mode requires script-free direct delivery"
        );
        anyhow::ensure!(
            !self.player.voices.iter().any(|v| v.channel == channel)
                && !self
                    .player
                    .pending_releases
                    .iter()
                    .any(|r| r.channel == channel)
                && !self
                    .commands
                    .iter()
                    .any(|c| c.channel == channel && !c.applied)
                && !self
                    .player
                    .input_keys
                    .iter()
                    .flatten()
                    .any(|k| k.channel == channel && k.onset_velocity.is_some())
                && !self
                    .player
                    .host_notes
                    .active
                    .iter()
                    .filter_map(|id| self.player.host_notes.get(*id))
                    .any(|o| o.channel == channel && (o.held || o.pinned)),
            "Note mode channel must be idle"
        );
        if mode == NoteMode::MonoRetrigger {
            let bank = self
                .bank
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("Note mode requires an installed bank"))?;
            anyhow::ensure!(
                self.state.shared_ahdsr.is_none(),
                "MonoRetrigger cannot use a sealed shared source range"
            );
            anyhow::ensure!(
                resident_sources(bank),
                "MonoRetrigger requires resident sources"
            );
        }
        self.state.note_modes[channel as usize] = mode;
        self.player.note_selected[channel as usize] = None;
        Ok(())
    }

    pub fn note_mode(&self, channel: u8) -> Option<NoteMode> {
        self.state.note_modes.get(channel as usize).copied()
    }

    pub(super) fn clear_note_modes(&mut self, stop: bool) {
        let channels =
            self.state
                .note_modes
                .iter()
                .enumerate()
                .fold(0u16, |mask, (channel, mode)| {
                    mask | if *mode == NoteMode::MonoRetrigger {
                        1 << channel
                    } else {
                        0
                    }
                });
        if stop && channels != 0 {
            let fade = self.player.fade_frames(STEAL_FADE);
            for voice in self
                .player
                .voices
                .iter_mut()
                .filter(|v| channels & (1 << v.channel) != 0)
            {
                voice.fade.start(0., fade, true);
                voice.held = false;
            }
            self.player
                .pending_releases
                .retain(|r| channels & (1 << r.channel) == 0);
            for key in self
                .player
                .input_keys
                .iter_mut()
                .flatten()
                .filter(|k| channels & (1 << k.channel) != 0)
            {
                *key = InputKey::default();
            }
            for channel in (0..16).filter(|c| channels & (1 << c) != 0) {
                self.player.keys[channel].fill(0);
                self.player.key_up[channel].fill(self.player.now);
            }
        }
        self.state.note_modes.fill(NoteMode::Poly);
        self.player.note_selected.fill(None);
        self.player.note_off_pending = 0;
        for key in self.player.input_keys.iter_mut().flatten() {
            key.accepted = None;
        }
        for i in 0..self.player.host_notes.active.len() {
            let id = self.player.host_notes.active[i];
            if let Some(owner) = self.player.host_notes.get_mut(id) {
                owner.accepted = None;
            }
        }
    }

    pub(super) fn finish_note_offs(&mut self) {
        if let Some(bank) = self.bank.as_deref() {
            let defaults = self.defaults();
            self.player.finish_note_offs(bank, &self.state, defaults);
        }
    }
}

impl Player {
    pub(super) fn event_retained(&self, event: EventId) -> bool {
        self.voices.iter().any(|v| v.event == event)
            || self.pending_releases.iter().any(|r| r.source == event)
            || self
                .note_selected
                .iter()
                .flatten()
                .any(|s| s.event == event)
            || self
                .input_keys
                .iter()
                .flatten()
                .any(|k| k.onset_velocity.is_some() && k.accepted.is_some_and(|a| a.event == event))
            || self
                .host_notes
                .active
                .iter()
                .filter_map(|id| self.host_notes.get(*id))
                .any(|o| o.event.live() == Some(event))
    }

    pub(super) fn start_input(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        ev: &NoteEvent,
        event: EventId,
        root: Root,
        defaults: Ahdsr,
    ) -> Option<EventId> {
        if state.note_modes[ev.channel as usize] == NoteMode::Poly {
            return self.start(bank, state, ev, event, false, defaults);
        }
        let Some(ordinal) = self.note_ordinal.checked_add(1) else {
            self.dropped_commands = self.dropped_commands.saturating_add(1);
            return None;
        };
        let before = self.admission_clock;
        let mut groups = GroupMask::none();
        let result = self.start_admission(
            bank,
            state,
            ev,
            event,
            false,
            defaults,
            false,
            Some(&mut groups),
        );
        if before == self.admission_clock {
            return None;
        }
        // Physical birth remains in InputKey.started / Owner.start. A fallback
        // is a new source admission and must not rewind native RTC timestamps.
        let accepted = AcceptedNote {
            event,
            ordinal,
            groups,
        };
        match root {
            Root::Input { channel, key } => {
                self.input_keys[channel as usize][key as usize].accepted = Some(accepted)
            }
            Root::Host(id) => self.host_notes.get_mut(id)?.accepted = Some(accepted),
        }
        self.select_note(
            ev.channel,
            Selected {
                root,
                event,
                ordinal,
            },
        );
        self.note_ordinal = ordinal;
        result
    }

    fn select_note(&mut self, channel: u8, selected: Selected) {
        if let Some(previous) = self.note_selected[channel as usize] {
            let fade = self.fade_frames(STEAL_FADE);
            for voice in self.voices.iter_mut().filter(|v| {
                v.event == previous.event
                    && v.channel == channel
                    && !v.release_trigger
                    && !v.fade.dying()
            }) {
                voice.fade.start(0., fade, true);
            }
        }
        self.note_selected[channel as usize] = Some(selected);
    }

    pub(super) fn input_key_up(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        key: u8,
        input: Option<u8>,
        defaults: Ahdsr,
    ) {
        if self.note_selected[channel as usize].is_some_and(|s| matches!(s.root,
            Root::Input { channel: physical, key: held } if held == key && input.is_none_or(|i| i == physical))) {
            self.queue_note_off(bank, state, channel, defaults);
        }
    }

    pub(super) fn host_key_up(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        root: HostRef,
        defaults: Ahdsr,
    ) {
        if self.note_selected[channel as usize].is_some_and(|s| s.root == Root::Host(root)) {
            self.queue_note_off(bank, state, channel, defaults);
        }
    }

    fn queue_note_off(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        channel: u8,
        defaults: Ahdsr,
    ) {
        if state.note_modes[channel as usize] != NoteMode::MonoRetrigger {
            return;
        }
        self.note_off_pending |= 1 << channel;
        if self.note_off_depth == 0 {
            self.finish_note_offs(bank, state, defaults);
        }
    }

    pub(super) fn finish_note_offs(
        &mut self,
        bank: &Bank,
        state: &InstrumentState,
        defaults: Ahdsr,
    ) {
        if self.note_off_depth != 0 {
            return;
        }
        let channels = std::mem::take(&mut self.note_off_pending);
        for channel in (0..16u8).filter(|c| channels & (1 << c) != 0) {
            if state.note_modes[channel as usize] != NoteMode::MonoRetrigger {
                continue;
            }
            let mut candidate: Option<(Root, AcceptedNote)> = None;
            for (input, row) in self.input_keys.iter().enumerate() {
                for (key, note) in row.iter().enumerate() {
                    if note.channel != channel || note.onset_velocity.is_none() {
                        continue;
                    }
                    if let Some(a) = note.accepted
                        && candidate.is_none_or(|(_, old)| a.ordinal > old.ordinal)
                    {
                        candidate = Some((
                            Root::Input {
                                channel: input as u8,
                                key: key as u8,
                            },
                            a,
                        ));
                    }
                }
            }
            for &id in &self.host_notes.active {
                let Some(owner) = self
                    .host_notes
                    .get(id)
                    .filter(|o| o.channel == channel && o.held && !o.silenced)
                else {
                    continue;
                };
                if let Some(a) = owner.accepted
                    && candidate.is_none_or(|(_, old)| a.ordinal > old.ordinal)
                {
                    candidate = Some((Root::Host(id), a));
                }
            }
            let Some((root, accepted)) = candidate else {
                continue;
            };
            if self.note_selected[channel as usize]
                .is_some_and(|s| s.root == root && s.ordinal == accepted.ordinal)
            {
                continue;
            }
            let (key, velocity, per_note, physical, host) = match root {
                Root::Input {
                    channel: input,
                    key,
                } => {
                    let note = self.input_keys[input as usize][key as usize];
                    (
                        key,
                        note.onset_velocity.unwrap(),
                        note.per_note,
                        input,
                        None,
                    )
                }
                Root::Host(id) => {
                    let owner = self.host_notes.get(id).unwrap();
                    (
                        owner.key,
                        owner.velocity,
                        owner.per_note,
                        owner.note.channel,
                        Some(id),
                    )
                }
            };
            let ev = NoteEvent {
                owner: Some((physical, key)),
                input_channel: Some(physical),
                host_note: host,
                per_note,
                groups: Some(&accepted.groups),
                group_start_applied: true,
                ..NoteEvent::new_velocity(channel, key, velocity)
            };
            let before = self.admission_clock;
            self.note_history.observe(key);
            self.start_admission(
                bank,
                state,
                &ev,
                accepted.event,
                false,
                defaults,
                false,
                None,
            );
            if before != self.admission_clock {
                self.select_note(
                    channel,
                    Selected {
                        root,
                        event: accepted.event,
                        ordinal: accepted.ordinal,
                    },
                );
            }
        }
    }
}
