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
    UnsupportedRouterInput,
    Aborted,
}
impl Error {
    /// Stable, allocation-free audio/control transport. Zero means no failure.
    pub(crate) fn code(self) -> u32 {
        match self {
            Self::InvalidInput => 1,
            Self::Capacity => 2,
            Self::DuplicateNote => 3,
            Self::TokenExhausted => 4,
            Self::UnsupportedMpe => 5,
            Self::UnsupportedExpression => 6,
            Self::UnsupportedChoke => 7,
            Self::UnsupportedInitialTuning => 8,
            Self::Aborted => 9,
            Self::UnsupportedRouterInput => 10,
            Self::Bridge(error) => match error {
                BridgeError::InvalidConfig => 32,
                BridgeError::NotReady => 33,
                BridgeError::InvalidBuffer => 34,
                BridgeError::TimelineOverflow => 35,
                BridgeError::RequestCapacity => 36,
                BridgeError::Worker(error) => match error {
                    PacketError::WrongEpoch => 64,
                    PacketError::WrongGeneration => 65,
                    PacketError::WrongFrame => 66,
                    PacketError::InvalidInput => 67,
                    PacketError::TooManyInputs => 68,
                    PacketError::Full => 69,
                    PacketError::Underrun => 70,
                    PacketError::Failed => 71,
                    PacketError::Stopped => 72,
                    PacketError::PortTaken => 73,
                },
            },
        }
    }
    pub(crate) fn from_code(code: u32) -> Option<Self> {
        Some(match code {
            1 => Self::InvalidInput,
            2 => Self::Capacity,
            3 => Self::DuplicateNote,
            4 => Self::TokenExhausted,
            5 => Self::UnsupportedMpe,
            6 => Self::UnsupportedExpression,
            7 => Self::UnsupportedChoke,
            8 => Self::UnsupportedInitialTuning,
            9 => Self::Aborted,
            10 => Self::UnsupportedRouterInput,
            32 => Self::Bridge(BridgeError::InvalidConfig),
            33 => Self::Bridge(BridgeError::NotReady),
            34 => Self::Bridge(BridgeError::InvalidBuffer),
            35 => Self::Bridge(BridgeError::TimelineOverflow),
            36 => Self::Bridge(BridgeError::RequestCapacity),
            64 => Self::Bridge(BridgeError::Worker(PacketError::WrongEpoch)),
            65 => Self::Bridge(BridgeError::Worker(PacketError::WrongGeneration)),
            66 => Self::Bridge(BridgeError::Worker(PacketError::WrongFrame)),
            67 => Self::Bridge(BridgeError::Worker(PacketError::InvalidInput)),
            68 => Self::Bridge(BridgeError::Worker(PacketError::TooManyInputs)),
            69 => Self::Bridge(BridgeError::Worker(PacketError::Full)),
            70 => Self::Bridge(BridgeError::Worker(PacketError::Underrun)),
            71 => Self::Bridge(BridgeError::Worker(PacketError::Failed)),
            72 => Self::Bridge(BridgeError::Worker(PacketError::Stopped)),
            73 => Self::Bridge(BridgeError::Worker(PacketError::PortTaken)),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FailureStage {
    Feed = 1,
    Router,
    Completions,
    Transport,
    UiEdit,
    AuditionRelease,
    AuditionNote,
    AuditionEnd,
    Process,
    Panic,
}
impl FailureStage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Feed => "feed",
            Self::Router => "router",
            Self::Completions => "completions",
            Self::Transport => "transport",
            Self::UiEdit => "ui_edit",
            Self::AuditionRelease => "audition_release",
            Self::AuditionNote => "audition_note",
            Self::AuditionEnd => "audition_end",
            Self::Process => "process",
            Self::Panic => "panic",
        }
    }
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Feed,
            2 => Self::Router,
            3 => Self::Completions,
            4 => Self::Transport,
            5 => Self::UiEdit,
            6 => Self::AuditionRelease,
            7 => Self::AuditionNote,
            8 => Self::AuditionEnd,
            9 => Self::Process,
            10 => Self::Panic,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Failure {
    pub(crate) error: Error,
    pub(crate) frame: u64,
    pub(crate) stage: FailureStage,
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    pub(crate) source: &'static str,
    pub(crate) line: u32,
}

/// One audio writer; the loader only takes snapshots. Claim before metadata,
/// publish the complete cause before the enclosing PartShared failed flag.
#[derive(Default)]
pub(crate) struct FailureAtoms {
    code: std::sync::atomic::AtomicU32,
    frame: std::sync::atomic::AtomicU64,
    origin: std::sync::atomic::AtomicU64,
    epoch: std::sync::atomic::AtomicU64,
    generation: std::sync::atomic::AtomicU64,
}
impl FailureAtoms {
    pub(crate) fn record(&self, failure: Failure) -> bool {
        use std::sync::atomic::Ordering;
        if self.code.compare_exchange(0, u32::MAX, Ordering::Acquire, Ordering::Relaxed).is_err() {
            return false;
        }
        self.frame.store(failure.frame, Ordering::Relaxed);
        self.origin.store(u64::from(failure.line) | ((failure.stage as u64) << 32)
            | (u64::from(failure.source == "src/plugin/uvi.rs") << 40), Ordering::Relaxed);
        self.epoch.store(failure.epoch, Ordering::Relaxed);
        self.generation.store(failure.generation, Ordering::Relaxed);
        self.code.store(failure.error.code(), Ordering::Release);
        true
    }
    /// Audio adoption only, before publishing the new native generation.
    pub(crate) fn reset(&self) {
        self.code.store(0, std::sync::atomic::Ordering::Release);
    }
    pub(crate) fn snapshot(&self, epoch: u64, generation: u64) -> Option<Failure> {
        use std::sync::atomic::Ordering;
        let code = self.code.load(Ordering::Acquire);
        let error = Error::from_code(code)?;
        let origin = self.origin.load(Ordering::Relaxed);
        let failure = Failure {
            error,
            frame: self.frame.load(Ordering::Relaxed),
            stage: FailureStage::from_code((origin >> 32) as u8)?,
            epoch: self.epoch.load(Ordering::Relaxed),
            generation: self.generation.load(Ordering::Relaxed),
            source: if origin & (1 << 40) != 0 { "src/plugin/uvi.rs" } else { "src/plugin.rs" },
            line: origin as u32,
        };
        (failure.epoch == epoch && failure.generation == generation
            && self.code.load(Ordering::Acquire) == code
            && self.epoch.load(Ordering::Relaxed) == epoch
            && self.generation.load(Ordering::Relaxed) == generation).then_some(failure)
    }
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
    choked: bool,
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
            choked: false,
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
    error_frame: u64,
    error_line: u32,
    active_voices: u64,
    underruns: u64,
    discarded_packets: u64,
    host_tempo: f64,
    host_beat: f64,
    host_playing: bool,
    host_transport_frame: Option<u64>,
    sample_rate: u32,
}

impl Slot {
    pub(crate) fn new(
        bridge: Bridge,
        epoch: u64,
        generation: u64,
        sample_rate: u32,
    ) -> Result<Self, Error> {
        if epoch == 0 || generation == 0 || !(8000..=192000).contains(&sample_rate) {
            return Err(Error::InvalidInput);
        }
        let frame = bridge.frame();
        Ok(Self {
            bridge,
            notes: Notes::new(epoch, generation),
            frame,
            error: None,
            error_frame: 0,
            error_line: 0,
            active_voices: 0,
            underruns: 0,
            discarded_packets: 0,
            host_tempo: 120.,
            host_beat: 0.,
            host_playing: false,
            host_transport_frame: None,
            sample_rate,
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
    pub(crate) fn error_origin(&self) -> Option<(Error, u64, u32)> {
        self.error.map(|error| (error, self.error_frame, self.error_line))
    }
    pub(crate) fn active_voices(&self) -> u64 {
        self.active_voices
    }
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
    #[track_caller]
    fn fail(&mut self, error: Error) -> Error {
        if self.error.is_none() {
            self.error = Some(error);
            self.error_frame = self.frame;
            self.error_line = std::panic::Location::caller().line();
        }
        self.bridge.abort(match error {
            Error::Bridge(reason) => reason,
            _ => BridgeError::Worker(PacketError::InvalidInput),
        });
        self.notes.abort();
        self.error.unwrap()
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
    pub(crate) fn abort_unsupported_router(&mut self) -> Error {
        self.fail(Error::UnsupportedRouterInput)
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
    fn choke_matching(&mut self, matches: impl Fn(Owner) -> bool) -> Result<(), Error> {
        let count = self
            .notes
            .owners
            .iter()
            .flatten()
            .filter(|owner| !owner.completed && !owner.choked && matches(**owner))
            .count();
        // Admit the entire bounded fanout before changing gates or queueing a prefix.
        if self.error.is_none() && count > self.bridge.remaining_hosted_capacity() {
            for owner in self
                .notes
                .owners
                .iter_mut()
                .flatten()
                .filter(|owner| matches(**owner))
            {
                owner.held = false;
            }
            return Err(self.fail(Error::Bridge(BridgeError::Worker(
                PacketError::TooManyInputs,
            ))));
        }
        for owner in self
            .notes
            .owners
            .iter_mut()
            .flatten()
            .filter(|owner| matches(**owner))
        {
            owner.held = false;
        }
        for owner in self
            .notes
            .owners
            .iter_mut()
            .flatten()
            .filter(|owner| matches(**owner))
        {
            if self.error.is_none() && !owner.completed && !owner.choked {
                if let Err(error) = self.bridge.push_hosted(HostedInput::Choke {
                    root: owner.root,
                    frame: self.frame,
                }) {
                    return Err(self.fail(Error::Bridge(error)));
                }
                owner.choked = true;
            }
        }
        if self.error.is_some() {
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
                return self.choke_matching(|owner| {
                    owner.host.is_some_and(|note| pattern.matches(note))
                });
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
                            // Detached onInit/UI voices have no physical-port owner.
                            // Keep channel panic gated until their native stop law is proven.
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
    /// Synchronize only host timing discontinuities. Session advances an accepted
    /// snapshot at the activation sample rate between callbacks.
    pub(crate) fn set_host_transport(
        &mut self,
        playing: bool,
        beat: f64,
        tempo: f64,
    ) -> Result<(), Error> {
        let elapsed = self.frame - self.host_transport_frame.unwrap_or(self.frame);
        let predicted = self.host_beat
            + if self.host_playing {
                elapsed as f64 / f64::from(self.sample_rate) * self.host_tempo / 60.
            } else {
                0.
            };
        let tempo = if (1. ..=1000.).contains(&tempo) {
            tempo
        } else {
            self.host_tempo
        };
        let beat = if beat.is_finite() { beat } else { predicted };
        // Host beat calculations can reassociate the same sample-rate arithmetic.
        // Permit only floating rounding, not a sample-sized seek/deviation.
        let rounding = 8. * f64::EPSILON * predicted.abs().max(beat.abs()).max(1.);
        if self.host_transport_frame.is_some()
            && playing == self.host_playing
            && tempo == self.host_tempo
            && (beat - predicted).abs() <= rounding
        {
            return Ok(());
        }
        self.push_event(InputKind::Transport {
            playing,
            beat,
            tempo,
        })?;
        self.host_playing = playing;
        self.host_beat = beat;
        self.host_tempo = tempo;
        self.host_transport_frame = Some(self.frame);
        Ok(())
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
        self.active_voices = if self.error.is_none() {
            self.bridge.active_voices()
        } else {
            0
        };
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
    fn authored_slot() -> (crate::uvi::worker::Worker, Slot, std::path::PathBuf, usize) {
        let (config, source) = crate::uvi::worker::tests::authored_bank_with_script(
            r#"
            knob=Knob{name='persist',value=.25}
            function onNote(e) postEvent(e) end
            function onRelease(e) postEvent(e) end
        "#,
        );
        let path = config.bank.clone();
        let program = crate::uvi::program::parse_program(&source).unwrap();
        let processor = program
            .nodes
            .iter()
            .position(|n| n.kind == "ScriptProcessor")
            .unwrap();
        let mut worker = crate::uvi::worker::Worker::start_hosted(config, 7, 9).unwrap();
        worker
            .wait_ready(std::time::Duration::from_secs(5))
            .unwrap();
        let bridge = Bridge::new(worker.take_audio_port().unwrap(), 7, 9, 256, 1).unwrap();
        (
            worker,
            Slot::new(bridge, 7, 9, 48000).unwrap(),
            path,
            processor,
        )
    }
    fn render_slot(slot: &mut Slot, blocks: usize) -> bool {
        let mut audible = false;
        for _ in 0..blocks {
            let mut left = [0.; 256];
            let mut right = [0.; 256];
            slot.process_mode(&mut left, &mut right, true).unwrap();
            audible |= left.iter().chain(&right).any(|v| v.abs() > 1e-5);
        }
        audible
    }
    #[test]
    fn actual_worker_choke_keeps_activation_ui_and_consumed_end_fence() {
        let (mut worker, mut slot, path, processor) = authored_slot();
        slot.feed(In::HostOn(note(10), 100, 0.), 0, true, false)
            .unwrap();
        slot.feed(In::HostOn(note(11), 100, 0.), 0, true, false)
            .unwrap();
        // Root A's physical gate is released, but sustain retains its DSP tail.
        slot.feed(In::Cc(2, 64, 127), 0, true, false).unwrap();
        slot.feed(In::HostOff(pattern(10)), 0, false, false)
            .unwrap();
        assert!(render_slot(&mut slot, 4));
        assert_eq!(
            super::super::tests::allocations(|| slot
                .feed(In::HostChoke(pattern(10)), 0, false, false)
                .unwrap()),
            0
        );
        let room = slot.bridge.remaining_hosted_capacity();
        assert_eq!(
            super::super::tests::allocations(|| slot
                .feed(In::HostChoke(pattern(10)), 0, false, false)
                .unwrap()),
            0
        );
        assert_eq!(
            slot.bridge.remaining_hosted_capacity(),
            room,
            "duplicate stop consumed packet room"
        );
        assert!(
            !slot
                .notes
                .owners
                .iter()
                .flatten()
                .find(|o| o.host == Some(note(10)))
                .unwrap()
                .held
        );
        assert!(
            slot.host_note_pending(note(10)),
            "admission is not a fabricated completion"
        );
        assert!(render_slot(&mut slot, 3));
        let stop = slot.frame();
        slot.collect_completions(stop - 256).unwrap();
        assert!(
            slot.host_note_pending(note(10)),
            "prefetched End crossed consumed PCM fence"
        );
        render_slot(&mut slot, 1);
        slot.collect_completions(slot.frame()).unwrap();
        assert!(!slot.host_note_pending(note(10)) && slot.host_note_pending(note(11)));
        assert!(
            slot.has_host_note(note(10)),
            "core End backpressure must retain identity"
        );
        slot.retire_host_note(note(10));
        slot.feed(In::HostOn(note(12), 100, 0.), 0, true, false)
            .unwrap();
        assert!(render_slot(&mut slot, 3));
        assert!(slot.error().is_none());
        let request = worker.request_ui_snapshot(processor).unwrap();
        let started = std::time::Instant::now();
        let snapshot = loop {
            if let Some(reply) = worker.poll_ui_snapshot() {
                assert_eq!(reply.request, request);
                break reply.snapshot.unwrap();
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(5));
            std::thread::yield_now();
        };
        assert!(matches!(
            snapshot.widgets[0].value,
            Some(crate::uvi::host::UiValue::Number(0.25))
        ));
        drop(slot);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cc120_stays_gated_with_detached_voices_and_released_owned_tails() {
        let (config, _) = crate::uvi::worker::tests::authored_bank_with_script(
            "function onInit() playNote(60,100,-1) end function onNote(e)postEvent(e)end function onRelease(e)postEvent(e)end",
        );
        let path = config.bank.clone();
        let mut worker = crate::uvi::worker::Worker::start_hosted(config, 7, 9).unwrap();
        worker
            .wait_ready(std::time::Duration::from_secs(5))
            .unwrap();
        let bridge = Bridge::new(worker.take_audio_port().unwrap(), 7, 9, 256, 1).unwrap();
        let mut slot = Slot::new(bridge, 7, 9, 48000).unwrap();
        slot.feed(In::HostOn(note(10), 100, 0.), 0, true, false)
            .unwrap();
        let other = HostNote {
            port: 1,
            ..note(11)
        };
        slot.feed(In::HostOn(other, 100, 0.), 1, true, false)
            .unwrap();
        slot.feed(In::Cc(2, 64, 127), 0, true, false).unwrap();
        slot.feed(In::HostOff(pattern(10)), 0, false, false)
            .unwrap();
        assert!(render_slot(&mut slot, 4));
        let room = slot.bridge.remaining_hosted_capacity();
        assert_eq!(
            super::super::tests::allocations(|| assert!(matches!(
                slot.feed(In::Cc(2, 120, 0), 0, true, false),
                Err(Error::UnsupportedChoke)
            ))),
            0
        );
        assert_eq!(slot.bridge.remaining_hosted_capacity(), room);
        assert_eq!(slot.error(), Some(Error::UnsupportedChoke));
        assert!(slot.bridge.failure().is_some());
        assert!(!slot.host_note_pending(note(10)) && slot.host_note_pending(other));
        assert!(
            slot.has_host_note(note(10)),
            "core End backpressure retains identity"
        );
        drop(slot);
        worker.stop();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn worker_stop_during_choke_releases_every_matched_gate_and_keeps_other_held() {
        let (mut worker, mut slot, path, _) = authored_slot();
        slot.feed(In::HostOn(note(10), 100, 0.), 0, true, false)
            .unwrap();
        slot.feed(In::HostOn(note(11), 100, 0.), 0, true, false)
            .unwrap();
        let other = HostNote {
            channel: 3,
            ..note(12)
        };
        slot.feed(In::HostOn(other, 100, 0.), 0, true, false)
            .unwrap();
        worker.stop();
        assert_eq!(
            super::super::tests::allocations(|| assert!(
                slot.feed(
                    In::HostChoke(HostPattern {
                        id: -1,
                        ..pattern(10)
                    }),
                    0,
                    false,
                    false
                )
                .is_err()
            )),
            0
        );
        assert!(!slot.host_note_pending(note(10)) && !slot.host_note_pending(note(11)));
        assert!(slot.host_note_pending(other));
        drop(slot);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn choke_fanout_admits_all_or_aborts_before_enqueueing_a_prefix() {
        let (mut worker, mut slot, path, _) = authored_slot();
        slot.feed(In::HostOn(note(10), 100, 0.), 0, true, false)
            .unwrap();
        slot.feed(In::HostOn(note(11), 100, 0.), 0, true, false)
            .unwrap();
        let other = HostNote {
            channel: 3,
            ..note(12)
        };
        slot.feed(In::HostOn(other, 100, 0.), 0, true, false)
            .unwrap();
        for _ in 0..crate::uvi::worker::MAX_HOSTED_INPUTS - 4 {
            slot.feed(In::Cc(2, 1, 0), 0, true, false).unwrap();
        }
        assert_eq!(slot.bridge.remaining_hosted_capacity(), 1);
        let all = HostPattern {
            id: -1,
            ..pattern(10)
        };
        assert_eq!(
            super::super::tests::allocations(|| assert!(matches!(
                slot.feed(In::HostChoke(all), 0, false, false),
                Err(Error::Bridge(BridgeError::Worker(
                    PacketError::TooManyInputs
                )))
            ))),
            0
        );
        assert_eq!(
            slot.bridge.remaining_hosted_capacity(),
            1,
            "a fanout prefix escaped admission"
        );
        assert!(slot.bridge.failure().is_some());
        assert!(slot.notes.owners.iter().flatten().all(|o| o.completed));
        assert!(!slot.host_note_pending(note(10)) && !slot.host_note_pending(note(11)));
        assert!(
            slot.host_note_pending(other),
            "overflow abort preserves unrelated physical gate"
        );
        drop(slot);
        worker.stop();
        std::fs::remove_file(path).unwrap();
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
            crate::test_support::allocations(|| {
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
    use crate::uvi::{
        playback::Renderer,
        program::parse_program,
        script::{Action, Command, Note},
        worker::Worker,
    };
    fn render_note(id: u32) -> Note {
        Note {
            id,
            note: 60,
            velocity: 100,
            channel: 0,
            dim1: 0,
            dim2: None,
            layers: None,
            oscillator: None,
            volume: 1.,
            pan: 0.,
            tune: 0.,
            offset_us: 0,
        }
    }
    #[test]
    fn transport_maximum_one_host_frame_redundant_snapshots_leave_notes_and_pcm_exact() {
        for playing in [false, true] {
            let expected_changes = usize::from(playing);
            let lua = format!(
                "local changes=0 function onTransport(p)changes=changes+1 end function onNote(e)assert(changes=={expected_changes});postEvent(e)end"
            );
            let (config, xml) = crate::uvi::worker::tests::authored_bank_with_script(&lua);
            let bank = config.bank.clone();
            let mut worker = Worker::start_hosted(config, 7, 9).unwrap();
            worker
                .wait_ready(std::time::Duration::from_secs(5))
                .unwrap();
            let bridge = Bridge::new(worker.take_audio_port().unwrap(), 7, 9, 1, 1).unwrap();
            let mut slot = Slot::new(bridge, 7, 9, 48000).unwrap();
            let latency = slot.latency_frames() as usize;
            let program = parse_program(&xml).unwrap();
            let mut renderer = Renderer::new(&program, Default::default(), 48000).unwrap();
            let commands = [0, 127, 255]
                .into_iter()
                .enumerate()
                .map(|(i, frame)| Command {
                    frame,
                    action: Action::Start(render_note(i as u32 + 1)),
                })
                .collect::<Vec<_>>();
            let expected = renderer.render(&commands, &[], 1024).unwrap();
            assert!(expected.iter().any(|x| x[0].abs() > 0.1));
            for frame in 0..latency + 1024 {
                let beat = if playing {
                    4. + frame as f64 / 48000. * 2.
                } else {
                    4.
                };
                let mut l = [0.];
                let mut r = [0.];
                assert_eq!(
                    crate::test_support::allocations(|| {
                        slot.set_host_transport(playing, beat, 120.).unwrap();
                        if [0, 127, 255].contains(&frame) {
                            slot.feed(In::NoteOn(0, 60, 100), 0, true, false).unwrap()
                        }
                        slot.process_mode(&mut l, &mut r, true).unwrap();
                    }),
                    0
                );
                if frame < latency {
                    assert_eq!([l[0], r[0]], [0., 0.])
                } else {
                    assert_eq!([l[0], r[0]], expected[frame - latency])
                }
            }
            assert_eq!(slot.error(), None);
            assert_eq!(slot.underruns(), 0);
            assert_eq!(slot.discarded_packets(), 0);
            drop(slot);
            worker.stop();
            std::fs::remove_file(bank).unwrap();
        }
    }
    #[test]
    fn transport_seek_tempo_stop_and_missing_snapshots_keep_time_and_callback_order() {
        let source = r#"
 local changes=0
 function onTransport(p)changes=changes+1 end
 function onNote(e)
  local stage=e.note-60
  if stage==0 then assert(changes==1 and getBeatTime()==4 and getTempo()==120)
  elseif stage==1 then assert(changes==1 and math.abs(getBeatTime()-(4+1/24000))<1e-12)
  elseif stage==2 then assert(changes==1 and math.abs(getBeatTime()-(4+3/24000))<1e-12)
  elseif stage==3 then assert(changes==1 and getBeatTime()==9 and getTempo()==90)
  elseif stage==4 then assert(changes==2 and getBeatTime()==10)
  else assert(changes==2 and getBeatTime()==11 and getTempo()==100)end
  postEvent(e)
 end
 "#;
        let (config, _) = crate::uvi::worker::tests::authored_bank_with_script(source);
        let bank = config.bank.clone();
        let mut worker = Worker::start_hosted(config, 7, 9).unwrap();
        worker
            .wait_ready(std::time::Duration::from_secs(5))
            .unwrap();
        let bridge = Bridge::new(worker.take_audio_port().unwrap(), 7, 9, 1, 1).unwrap();
        let mut slot = Slot::new(bridge, 7, 9, 48000).unwrap();
        for (frame, playing, beat, tempo) in [
            (0, true, 4., 120.),
            (1, true, f64::NAN, 0.),
            (2, true, 4. + 3. / 24000., 120.),
            (3, true, 9., 90.),
            (4, false, 10., 90.),
            (5, false, 11., 100.),
        ] {
            slot.set_host_transport(playing, beat, tempo).unwrap();
            slot.feed(In::NoteOn(0, 60 + frame as u8, 100), 0, true, false)
                .unwrap();
            slot.process_mode(&mut [0.], &mut [0.], true).unwrap();
        }
        for _ in 6..1024 {
            slot.process_mode(&mut [0.], &mut [0.], true).unwrap();
        }
        assert_eq!(slot.error(), None);
        drop(slot);
        worker.stop();
        std::fs::remove_file(bank).unwrap();
    }
}
