//! Audio-thread UVI adoption and ancestry mapping. The plugin's existing host
//! registry and End emission remain authoritative across all rack backends.
use crate::{
    articulate::In,
    engine::{HostNote, HostPattern},
    uvi::{
        bridge::{Bridge, BridgeError},
        host::UiEdit,
        player::UiInput,
        script::{HOST_ROOT_CAPACITY, HostRoot, HostedInput, Input, InputKind},
        worker::PacketError,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    Bridge(BridgeError),
    InvalidInput,
    Capacity,
    DuplicateNote,
    TokenExhausted,
    UnsupportedMpe,
    UnsupportedExpression,
    UnsupportedChoke,
    UnsupportedInitialTuning,
    Aborted,
}
impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self, formatter)
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy)]
struct Owner {
    root: HostRoot,
    host: Option<HostNote>,
    port: u8,
    channel: u8,
    key: u8,
    held: bool,
    completed: bool,
}

struct Notes {
    epoch: u64,
    generation: u64,
    token: u64,
    owners: Box<[Option<Owner>]>,
}

impl Notes {
    fn new(epoch: u64, generation: u64) -> Self {
        Self {
            epoch,
            generation,
            token: 0,
            owners: vec![None; HOST_ROOT_CAPACITY].into_boxed_slice(),
        }
    }
    fn allocate(
        &mut self,
        host: Option<HostNote>,
        port: u8,
        channel: u8,
        key: u8,
    ) -> Result<HostRoot, Error> {
        if host.is_some_and(|note| note.id != -1 && self.present(note)) {
            return Err(Error::DuplicateNote);
        }
        // ponytail: bounded 4096-row scan; use a fixed free list only if measured.
        let index = self
            .owners
            .iter()
            .position(Option::is_none)
            .ok_or(Error::Capacity)?;
        let token = self.token.checked_add(1).ok_or(Error::TokenExhausted)?;
        let root = HostRoot {
            epoch: self.epoch,
            generation: self.generation,
            token,
        };
        self.token = token;
        self.owners[index] = Some(Owner {
            root,
            host,
            port,
            channel,
            key,
            held: true,
            completed: false,
        });
        Ok(root)
    }
    fn present(&self, note: HostNote) -> bool {
        self.owners
            .iter()
            .flatten()
            .any(|owner| owner.host == Some(note))
    }
    fn pending(&self, note: HostNote) -> bool {
        self.owners
            .iter()
            .flatten()
            .any(|owner| owner.host == Some(note) && (owner.held || !owner.completed))
    }
    fn release_typed(&mut self, port: u8, channel: u8, key: u8) -> Option<HostRoot> {
        let index = self
            .owners
            .iter()
            .enumerate()
            .filter_map(|(index, owner)| {
                owner
                    .filter(|owner| {
                        owner.host.is_none()
                            && owner.held
                            && owner.port == port
                            && owner.channel == channel
                            && owner.key == key
                    })
                    .map(|owner| (index, owner.root.token))
            })
            .min_by_key(|(_, token)| *token)
            .map(|(index, _)| index)?;
        let owner = self.owners[index].as_mut().unwrap();
        owner.held = false;
        Some(owner.root)
    }
    fn release_matching(
        &mut self,
        matches: impl Fn(Owner) -> bool,
        mut send: impl FnMut(HostRoot) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut failure = None;
        for owner in self
            .owners
            .iter_mut()
            .flatten()
            .filter(|owner| owner.held && matches(**owner))
        {
            owner.held = false;
            if failure.is_none() {
                failure = send(owner.root).err();
            }
        }
        failure.map_or(Ok(()), Err)
    }
    fn complete(&mut self, root: HostRoot) {
        for entry in &mut self.owners {
            if let Some(owner) = entry.as_mut().filter(|owner| owner.root == root) {
                owner.completed = true;
                if owner.host.is_none() && !owner.held {
                    *entry = None;
                }
                return;
            }
        }
    }
    fn abort(&mut self) {
        for entry in &mut self.owners {
            if let Some(owner) = entry {
                owner.completed = true;
                if owner.host.is_none() && !owner.held {
                    *entry = None;
                }
            }
        }
    }
    fn stop_channel(&mut self, port: u8, channel: u8) {
        let _ = self.release_matching(
            |owner| owner.port == port && owner.channel == channel,
            |_| Ok(()),
        );
        self.abort();
    }
    fn panic(&mut self) {
        let _ = self.release_matching(|_| true, |_| Ok(()));
        self.abort();
    }
    fn retire(&mut self, note: HostNote) {
        if self.pending(note) {
            return;
        }
        for entry in &mut self.owners {
            if entry.is_some_and(|owner| owner.host == Some(note) && !owner.held && owner.completed)
            {
                *entry = None;
            }
        }
    }
}

/// Construct and destroy on the loader/control thread. Audio-thread adoption
/// must swap the entire value back for retirement, never drop its Bridge/Box.
pub(crate) struct Slot {
    bridge: Bridge,
    notes: Notes,
    frame: u64,
    error: Option<Error>,
    active_voices: u64,
    underruns: u64,
    discarded_packets: u64,
    host_tempo: f64,
    host_beat: f64,
}

impl Slot {
    pub(crate) fn new(bridge: Bridge, epoch: u64, generation: u64) -> Result<Self, Error> {
        if epoch == 0 || generation == 0 {
            return Err(Error::InvalidInput);
        }
        let frame = bridge.frame();
        Ok(Self {
            bridge,
            notes: Notes::new(epoch, generation),
            frame,
            error: None,
            active_voices: 0,
            underruns: 0,
            discarded_packets: 0,
            host_tempo: 120.,
            host_beat: 0.,
        })
    }
    pub(crate) fn frame(&self) -> u64 {
        self.frame
    }
    pub(crate) fn latency_frames(&self) -> u32 {
        self.bridge.latency_frames()
    }
    pub(crate) fn error(&self) -> Option<Error> {
        self.error
    }
    pub(crate) fn active_voices(&self) -> u64 { self.active_voices }
    pub(crate) fn underruns(&self) -> u64 {
        self.underruns
    }
    pub(crate) fn discarded_packets(&self) -> u64 {
        self.discarded_packets
    }
    pub(crate) fn has_host_note(&self, note: HostNote) -> bool {
        self.notes.present(note)
    }
    pub(crate) fn host_note_pending(&self, note: HostNote) -> bool {
        self.notes.pending(note)
    }
    pub(crate) fn host_note_at(&self, index: usize) -> Option<(HostNote, bool)> {
        self.notes
            .owners
            .iter()
            .flatten()
            .filter_map(|owner| {
                owner
                    .host
                    .map(|note| (note, owner.held || !owner.completed))
            })
            .nth(index)
    }
    /// Only after all UVI/Kontakt descendants and alignment owners ended, and
    /// the core accepted End into host output. Queue-full retains every row.
    pub(crate) fn retire_host_note(&mut self, note: HostNote) {
        self.notes.retire(note);
    }
    pub(crate) fn host_key_held(&self, channel: u8, key: u8) -> bool {
        self.notes.owners.iter().flatten().any(|owner| {
            owner.host.is_some() && owner.channel == channel && owner.key == key && owner.held
        })
    }
    fn fail(&mut self, error: Error) -> Error {
        self.error.get_or_insert(error);
        self.bridge.abort(match error {
            Error::Bridge(reason) => reason,
            _ => BridgeError::Worker(PacketError::InvalidInput),
        });
        self.notes.abort();
        error
    }
    /// Only core panic/reset authorizes releasing every physical gate. The
    /// activation stays aborted; replace it off audio to begin playback again.
    pub(crate) fn panic(&mut self) {
        self.notes.panic();
        self.fail(Error::Aborted);
    }
    /// Replacement/reset closes this activation's gates and discards its
    /// playback. Retain the whole Slot until canonical End output is accepted.
    pub(crate) fn abort_activation(&mut self) {
        self.panic();
    }
    pub(crate) fn has_host_owners(&self) -> bool {
        self.notes
            .owners
            .iter()
            .flatten()
            .any(|owner| owner.host.is_some())
    }
    fn send(&mut self, event: HostedInput) -> Result<(), Error> {
        if self.error.is_some() {
            return Ok(());
        }
        self.bridge
            .push_hosted(event)
            .map_err(|error| self.fail(Error::Bridge(error)))
    }
    fn release_pattern(&mut self, pattern: HostPattern) -> Result<(), Error> {
        let frame = self.frame;
        let failed = self.error.is_some();
        let bridge = &mut self.bridge;
        let result = self.notes.release_matching(
            |owner| owner.host.is_some_and(|note| pattern.matches(note)),
            |root| {
                if failed {
                    Ok(())
                } else {
                    bridge
                        .push_hosted(HostedInput::Off { root, frame })
                        .map_err(Error::Bridge)
                }
            },
        );
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        if failed {
            self.notes.abort();
        }
        Ok(())
    }
    /// `reached` comes from articulate::reaches using the accepted controls
    /// and Router. Key-up resolves stored original targets even after rerouting.
    /// Feed at the current segment boundary, before Kontakt articulation.
    pub(crate) fn feed(
        &mut self,
        event: In,
        port: u8,
        reached: bool,
        mpe: bool,
    ) -> Result<(), Error> {
        match event {
            In::NoteOn(channel, key, 0) => {
                return self.feed(In::NoteOff(channel, key), port, reached, mpe);
            }
            In::HostOff(pattern) => return self.release_pattern(pattern),
            In::HostChoke(pattern) => {
                if self
                    .notes
                    .owners
                    .iter()
                    .flatten()
                    .any(|owner| owner.host.is_some_and(|note| pattern.matches(note)))
                {
                    self.notes.release_matching(
                        |owner| owner.host.is_some_and(|note| pattern.matches(note)),
                        |_| Ok(()),
                    )?;
                    return Err(self.fail(Error::UnsupportedChoke));
                }
                return Ok(());
            }
            In::NoteOff(channel, key) => {
                if channel >= 16 || key >= 128 {
                    return Err(Error::InvalidInput);
                }
                if let Some(root) = self.notes.release_typed(port, channel, key) {
                    self.send(HostedInput::Off {
                        root,
                        frame: self.frame,
                    })?;
                    if self.error.is_some() {
                        self.notes.abort();
                    }
                }
                return Ok(());
            }
            In::HostExpression(pattern, _) => {
                if self
                    .notes
                    .owners
                    .iter()
                    .flatten()
                    .any(|owner| owner.host.is_some_and(|note| pattern.matches(note)))
                {
                    return Err(Error::UnsupportedExpression);
                }
                return Ok(());
            }
            _ if !reached => return Ok(()),
            _ => {}
        }
        let (host, channel, key, velocity, tune) = match event {
            In::HostOn(note, velocity, tune) => {
                (Some(note), note.channel, note.key, velocity, tune)
            }
            In::NoteOn(channel, key, velocity) => (None, channel, key, velocity, 0.),
            _ => {
                if mpe && !matches!(event, In::Cc(_, 120 | 123, _)) {
                    return Err(self.fail(Error::UnsupportedMpe));
                }
                let kind = match event {
                    In::Cc(channel, controller, value) => {
                        if channel >= 16 || controller >= 128 || value >= 128 {
                            return Err(Error::InvalidInput);
                        }
                        if controller == 120 {
                            self.notes.stop_channel(port, channel);
                            return Err(self.fail(Error::UnsupportedChoke));
                        }
                        if controller == 123 {
                            let frame = self.frame;
                            let failed = self.error.is_some();
                            let bridge = &mut self.bridge;
                            let result = self.notes.release_matching(
                                |owner| owner.port == port && owner.channel == channel,
                                |root| {
                                    if failed {
                                        Ok(())
                                    } else {
                                        bridge
                                            .push_hosted(HostedInput::Off { root, frame })
                                            .map_err(Error::Bridge)
                                    }
                                },
                            );
                            if let Err(error) = result {
                                return Err(self.fail(error));
                            }
                            if failed {
                                self.notes.abort();
                            }
                        }
                        InputKind::Controller {
                            channel,
                            controller,
                            value,
                        }
                    }
                    In::Bend(channel, value) => InputKind::PitchBend {
                        channel,
                        bend: normalized_bend(value),
                    },
                    In::Pressure(channel, value) => InputKind::AfterTouch { channel, value },
                    In::PolyAt(channel, note, value) => InputKind::PolyAfterTouch {
                        channel,
                        note,
                        value,
                    },
                    In::NoteTune(..)
                    | In::NotePressure(..)
                    | In::NoteGain(..)
                    | In::NotePan(..)
                    | In::NoteBrightness(..) => return Err(Error::UnsupportedExpression),
                    _ => return Err(Error::InvalidInput),
                };
                return self.push_event(kind);
            }
        };
        if channel >= 16 || key >= 128 || velocity > 127 || !tune.is_finite() {
            return Err(Error::InvalidInput);
        }
        let root = self.notes.allocate(host, port, channel, key)?;
        if self.error.is_some() || mpe || tune != 0. || velocity == 0 {
            // A rejected exact attack still owns its tuple until canonical End.
            if let Some(owner) = self
                .notes
                .owners
                .iter_mut()
                .flatten()
                .find(|owner| owner.root == root)
            {
                owner.held = false;
            }
            self.notes.complete(root);
            let error = if mpe {
                Error::UnsupportedMpe
            } else if tune != 0. {
                Error::UnsupportedInitialTuning
            } else {
                self.error.unwrap_or(Error::InvalidInput)
            };
            return Err(self.fail(error));
        }
        self.send(HostedInput::On {
            root,
            input: Input {
                frame: self.frame,
                kind: InputKind::NoteOn {
                    channel,
                    note: key,
                    velocity,
                },
            },
        })
    }
    /// Format wrappers use zero/nonfinite timing when the host supplies none.
    /// Keep the last valid timing while still delivering play/stop transitions.
    pub(crate) fn set_host_transport(&mut self, playing: bool, beat: f64, tempo: f64) -> Result<(), Error> {
        if (1. ..=1000.).contains(&tempo) { self.host_tempo = tempo; }
        if beat.is_finite() { self.host_beat = beat; }
        self.push_event(InputKind::Transport { playing, beat: self.host_beat, tempo: self.host_tempo })
    }
    pub(crate) fn push_event(&mut self, kind: InputKind) -> Result<(), Error> {
        let input = Input {
            frame: self.frame,
            kind,
        };
        if !crate::uvi::player::input_is_valid(&input)
            || matches!(kind, InputKind::NoteOn { .. } | InputKind::NoteOff { .. })
        {
            return Err(Error::InvalidInput);
        }
        self.send(HostedInput::Event(input))
    }
    pub(crate) fn push_ui(&mut self, edit: UiEdit) -> Result<(), Error> {
        if let Some(error) = self.error {
            return Err(error);
        }
        self.bridge
            .push_ui(UiInput {
                frame: self.frame,
                edit,
            })
            .map_err(|error| self.fail(Error::Bridge(error)))
    }
    pub(crate) fn process(&mut self, left: &mut [f32], right: &mut [f32]) -> Result<(), Error> {
        self.process_mode(left, right, false)
    }
    pub(crate) fn process_mode(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        offline: bool,
    ) -> Result<(), Error> {
        if left.len() != right.len() {
            left.fill(0.);
            right.fill(0.);
            return Err(self.fail(Error::InvalidInput));
        }
        let next = self
            .frame
            .checked_add(left.len() as u64)
            .ok_or(Error::InvalidInput)?;
        let result = if let Some(error) = self.error {
            left.fill(0.);
            right.fill(0.);
            Err(error)
        } else {
            let rendered = if offline {
                self.bridge.process_offline(left, right)
            } else {
                self.bridge.process(left, right)
            };
            rendered
                .map(|report| {
                    self.underruns = self
                        .underruns
                        .saturating_add(u64::from(report.underrun_packets));
                    self.discarded_packets = self
                        .discarded_packets
                        .saturating_add(u64::from(report.discarded_packets));
                })
                .map_err(|error| self.fail(Error::Bridge(error)))
        };
        if result.is_err() {
            left.fill(0.);
            right.fill(0.);
        }
        self.active_voices = if self.error.is_none() { self.bridge.active_voices() } else { 0 };
        self.frame = next;
        result
    }
    /// Use the whole host callback's consumed start boundary, never its future
    /// end. Bridge maps buffering latency before yielding durable completions.
    pub(crate) fn collect_completions(&mut self, consumed_host_frame: u64) -> Result<(), Error> {
        if self.error.is_some() {
            return Ok(());
        }
        for _ in 0..HOST_ROOT_CAPACITY {
            match self
                .bridge
                .try_completion(consumed_host_frame)
                .map_err(|error| self.fail(Error::Bridge(error)))?
            {
                Some(completion) => self.notes.complete(completion.root),
                None => break,
            }
        }
        Ok(())
    }
}

fn normalized_bend(value: u16) -> f64 {
    let delta = f64::from(value) - 8192.;
    delta / if value >= 8192 { 8191. } else { 8192. }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn note(id: i32) -> HostNote {
        HostNote {
            port: 0,
            channel: 2,
            key: 60,
            id,
            clap: true,
        }
    }
    fn pattern(id: i32) -> HostPattern {
        HostPattern {
            port: 0,
            channel: 2,
            key: 60,
            id,
            clap: true,
        }
    }
    #[test]
    fn typed_retrigger_fifo_and_exact_identity_survive_end_backpressure() {
        let mut notes = Notes::new(7, 9);
        let first = notes.allocate(None, 0, 2, 60).unwrap();
        let second = notes.allocate(None, 0, 2, 60).unwrap();
        assert_eq!(notes.release_typed(1, 2, 60), None);
        assert_eq!(notes.release_typed(0, 2, 60), Some(first));
        notes.complete(first);
        assert_eq!(notes.release_typed(0, 2, 60), Some(second));
        notes.complete(second);
        let exact = notes.allocate(Some(note(42)), 0, 2, 60).unwrap();
        notes
            .release_matching(
                |owner| owner.host.is_some_and(|note| pattern(42).matches(note)),
                |_| Ok(()),
            )
            .unwrap();
        notes.complete(exact);
        assert!(notes.present(note(42)) && !notes.pending(note(42)));
        assert!(matches!(
            notes.allocate(Some(note(42)), 0, 2, 60),
            Err(Error::DuplicateNote)
        ));
        assert!(
            notes.present(note(42)),
            "rejected End leaves the tuple owned"
        );
        notes.retire(note(42));
        assert!(!notes.present(note(42)));
        assert!(notes.allocate(Some(note(42)), 0, 2, 60).unwrap().token > exact.token);
    }
    #[test]
    fn anonymous_tuple_end_waits_all_roots_and_stale_completion_cannot_close_reuse() {
        let mut notes = Notes::new(7, 9);
        let first = notes.allocate(Some(note(-1)), 0, 2, 60).unwrap();
        let second = notes.allocate(Some(note(-1)), 0, 2, 60).unwrap();
        notes
            .release_matching(
                |owner| owner.host.is_some_and(|note| pattern(-1).matches(note)),
                |_| Ok(()),
            )
            .unwrap();
        notes.complete(first);
        assert!(notes.pending(note(-1)));
        notes.complete(HostRoot { epoch: 8, ..second });
        assert!(notes.pending(note(-1)));
        notes.complete(second);
        assert!(!notes.pending(note(-1)));
        notes.retire(note(-1));
        let next = notes.allocate(Some(note(-1)), 0, 2, 60).unwrap();
        notes.complete(first);
        assert!(notes.pending(note(-1)) && next.token > second.token);
    }
    #[test]
    fn scoped_choke_abort_preserves_other_held_gates_and_full_ledger() {
        let mut notes = Notes::new(7, 9);
        let first = notes.allocate(Some(note(1)), 0, 2, 60).unwrap();
        notes.allocate(Some(note(2)), 0, 2, 60).unwrap();
        notes
            .release_matching(
                |owner| owner.host.is_some_and(|note| pattern(1).matches(note)),
                |_| Ok(()),
            )
            .unwrap();
        notes.abort();
        assert!(!notes.pending(note(1)) && notes.pending(note(2)));
        notes.retire(note(1));
        notes.complete(first);
        assert!(notes.pending(note(2)));
        notes
            .release_matching(
                |owner| owner.host.is_some_and(|note| pattern(2).matches(note)),
                |_| Ok(()),
            )
            .unwrap();
        assert!(!notes.pending(note(2)));
        notes.retire(note(2));
        for _ in 0..HOST_ROOT_CAPACITY {
            notes.allocate(None, 0, 2, 60).unwrap();
        }
        let token = notes.token;
        assert!(matches!(
            notes.allocate(None, 0, 2, 60),
            Err(Error::Capacity)
        ));
        assert_eq!(notes.token, token);
    }
    #[test]
    fn bend_endpoints_and_channel_stop_preserve_other_held_roots() {
        assert_eq!(normalized_bend(0), -1.);
        assert_eq!(normalized_bend(8192), 0.);
        assert_eq!(normalized_bend(16383), 1.);
        let mut notes = Notes::new(7, 9);
        let a = note(1);
        let b = HostNote {
            channel: 3,
            ..note(2)
        };
        let c = HostNote { port: 1, ..note(3) };
        notes.allocate(Some(a), 0, 2, 60).unwrap();
        notes.allocate(Some(b), 0, 3, 60).unwrap();
        notes.allocate(Some(c), 1, 2, 60).unwrap();
        notes.stop_channel(0, 2);
        assert!(!notes.pending(a));
        assert!(notes.pending(b) && notes.pending(c));
        assert!(notes.owners.iter().flatten().all(|owner| owner.completed));
        notes.panic();
        assert!(!notes.pending(b) && !notes.pending(c));
        assert!(notes.present(a) && notes.present(b) && notes.present(c));
    }
    #[test]
    fn adopted_note_mapping_and_retirement_do_not_allocate() {
        let mut notes = Notes::new(7, 9);
        assert_eq!(
            crate::plugin::tests::allocations(|| {
                let typed = notes.allocate(None, 0, 2, 60).unwrap();
                let exact = notes.allocate(Some(note(42)), 0, 2, 60).unwrap();
                assert_eq!(notes.release_typed(0, 2, 60), Some(typed));
                notes.complete(typed);
                notes
                    .release_matching(
                        |owner| owner.host.is_some_and(|note| pattern(42).matches(note)),
                        |_| Ok(()),
                    )
                    .unwrap();
                notes.complete(exact);
                assert!(!notes.pending(note(42)));
                notes.retire(note(42));
                assert!(!notes.present(note(42)));
            }),
            0
        );
    }
}
