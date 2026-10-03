//! Audio-side adapter for the allocating UVI worker. Construct and destroy this
//! endpoint on the loader; the controller stays there until endpoint retirement.
//! Native event policy/host-note ownership and shared rack mixing remain outside.

use super::{
    player,
    script::{HostedInput, InputKind},
    worker::{
        AudioPort, BLOCK_FRAMES, HostedRequest, MAX_HOSTED_INPUTS, MAX_UI_EDITS, Output,
        PacketError, QUEUE_CAPACITY, Request, Stamp, StampedCompletion, Status, UiInput,
    },
};

const MAX_HOST_FRAMES: usize = 65_536;
const MAX_LEAD_PACKETS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BridgeError {
    InvalidConfig,
    NotReady,
    InvalidBuffer,
    TimelineOverflow,
    RequestCapacity,
    Worker(PacketError),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessReport {
    pub submitted_packets: u32,
    /// A missed deadline silences the entire 256-frame packet, including when
    /// the caller splits that packet across several rendering calls.
    pub underrun_packets: u32,
    pub discarded_packets: u32,
}

/// Two actual bounded queues: pending requests and prefetched stereo packets.
/// Allocation and destruction belong to the loader, never to these operations.
struct Ring<T: Copy> {
    data: Box<[T]>,
    head: usize,
    len: usize,
}
impl<T: Copy> Ring<T> {
    fn new(capacity: usize, empty: T) -> Self {
        Self {
            data: vec![empty; capacity].into_boxed_slice(),
            head: 0,
            len: 0,
        }
    }
    fn push(&mut self, value: T) -> Result<(), BridgeError> {
        if self.len == self.data.len() {
            return Err(BridgeError::RequestCapacity);
        }
        self.data[(self.head + self.len) % self.data.len()] = value;
        self.len += 1;
        Ok(())
    }
    fn front(&self) -> Option<T> {
        (self.len != 0).then(|| self.data[self.head])
    }
    fn pop(&mut self) -> Option<T> {
        let value = self.front()?;
        self.head = (self.head + 1) % self.data.len();
        self.len -= 1;
        Some(value)
    }
    fn full(&self) -> bool {
        self.len == self.data.len()
    }
}

/// One activation, beginning at host/native frame zero. No canonical host ID,
/// MIDI decoder, Lua, mixer, controller handle or mutex is stored here.
///
/// The lead is an admitted buffering budget, not proof that the worker meets
/// deadlines. Host callbacks must not exceed the configured maximum. All packet
/// storage is prepared off audio; failures retain that storage for retirement.
pub struct Bridge {
    port: AudioPort,
    stamp: Stamp,
    maximum: usize,
    latency: u32,
    frame: u64,
    consumed_host_frame: u64,
    submitted_frame: u64,
    received_frame: u64,
    discard_before: u64,
    partial: HostedRequest,
    requests: Ring<HostedRequest>,
    audio: Ring<Output>,
    active_frame: Option<u64>,
    active_audio: Option<Output>,
    failed: Option<BridgeError>,
}

fn layout(maximum: usize, lead: usize) -> Result<(u32, usize), BridgeError> {
    if maximum == 0 || maximum > MAX_HOST_FRAMES || lead == 0 || lead > MAX_LEAD_PACKETS {
        return Err(BridgeError::InvalidConfig);
    }
    let packets = maximum.div_ceil(BLOCK_FRAMES) + lead;
    Ok((
        (packets * BLOCK_FRAMES) as u32,
        packets + QUEUE_CAPACITY + 1,
    ))
}

fn empty_request(stamp: Stamp) -> HostedRequest {
    // The activation stamp and its aligned frame were checked by the caller.
    HostedRequest::new(Request::new(stamp, &[]).unwrap(), &[]).unwrap()
}
fn empty_output(stamp: Stamp) -> Output {
    Output {
        stamp,
        audio: [[0.; 2]; BLOCK_FRAMES],
        commands: 0,
        host_commands: 0,
        logs: 0,
        dropped_logs: 0,
        rejected_ui: 0,
    }
}

impl Bridge {
    /// The exact buffering budget, for shared mixer storage prepared on Load.
    pub fn buffering_latency(maximum: usize, lead: usize) -> Result<u32, BridgeError> {
        layout(maximum, lead).map(|(latency, _)| latency)
    }

    /// Loader only. `worker_lead_packets` is explicitly selected by the caller.
    /// Latency = rounded maximum host block + this many 256-frame packets.
    pub fn new(
        mut port: AudioPort,
        epoch: u64,
        generation: u64,
        max_host_frames: usize,
        worker_lead_packets: usize,
    ) -> Result<Self, BridgeError> {
        let (latency, capacity) = layout(max_host_frames, worker_lead_packets)?;
        if port.realtime().status() != Status::Ready {
            return Err(BridgeError::NotReady);
        }
        let stamp = Stamp {
            epoch,
            generation,
            frame: 0,
        };
        let partial = empty_request(stamp);
        Ok(Self {
            port,
            stamp,
            maximum: max_host_frames,
            latency,
            frame: 0,
            consumed_host_frame: 0,
            submitted_frame: 0,
            received_frame: 0,
            discard_before: 0,
            partial,
            requests: Ring::new(capacity, partial),
            audio: Ring::new(capacity, empty_output(stamp)),
            active_frame: None,
            active_audio: None,
            failed: None,
        })
    }

    /// Atomic worker census; may lead consumed PCM by this adapter's latency.
    pub fn active_voices(&mut self) -> u64 { self.port.realtime().stats().active_voices }

    pub fn frame(&self) -> u64 {
        self.frame
    }
    pub fn latency_frames(&self) -> u32 {
        self.latency
    }
    pub fn failure(&self) -> Option<BridgeError> {
        self.failed
    }

    /// Mark activation abort without joining, destroying or clearing its owned
    /// storage. The host ledger owns cancellation; the loader owns retirement.
    pub fn abort(&mut self, reason: BridgeError) {
        self.failed.get_or_insert(reason);
    }

    fn check(&mut self) -> Result<(), BridgeError> {
        if let Some(error) = self.failed {
            return Err(error);
        }
        match self.port.realtime().status() {
            Status::Ready => Ok(()),
            Status::Failed => self.fail(BridgeError::Worker(PacketError::Failed)),
            Status::Stopped => self.fail(BridgeError::Worker(PacketError::Stopped)),
            Status::Starting => self.fail(BridgeError::NotReady),
        }
    }
    fn fail<T>(&mut self, error: BridgeError) -> Result<T, BridgeError> {
        self.abort(error);
        Err(error)
    }

    /// Append exactly at the next host frame. Native roots and controls share
    /// this one arrival order; no NoteOn/Off may bypass the host root ledger.
    pub fn push_hosted(&mut self, event: HostedInput) -> Result<(), BridgeError> {
        self.check()?;
        let valid = event.frame() == self.frame
            && match event {
                HostedInput::On { root, input } => {
                    self.root_valid(root)
                        && player::input_is_valid(&input)
                        && matches!(input.kind, InputKind::NoteOn { .. })
                }
                HostedInput::Off { root, .. } | HostedInput::Choke { root, .. } => self.root_valid(root),
                HostedInput::Event(input) => {
                    player::input_is_valid(&input)
                        && !matches!(
                            input.kind,
                            InputKind::NoteOn { .. } | InputKind::NoteOff { .. }
                        )
                }
            };
        let count = usize::from(self.partial.root_count);
        if !valid {
            return self.fail(BridgeError::Worker(PacketError::InvalidInput));
        }
        if count == MAX_HOSTED_INPUTS {
            return self.fail(BridgeError::Worker(PacketError::TooManyInputs));
        }
        self.partial.roots[count] = event;
        self.partial.root_count += 1;
        Ok(())
    }
    /// Fixed packet room for atomic host-pattern fanout; no reservation or allocation.
    pub fn remaining_hosted_capacity(&self) -> usize {
        MAX_HOSTED_INPUTS - usize::from(self.partial.root_count)
    }
    fn root_valid(&self, root: super::script::HostRoot) -> bool {
        root.epoch == self.stamp.epoch && root.generation == self.stamp.generation && root.token > 0
    }

    /// UI edits have their separate existing bound and native tie precedence.
    pub fn push_ui(&mut self, input: UiInput) -> Result<(), BridgeError> {
        self.check()?;
        let count = usize::from(self.partial.request.ui_count);
        if input.frame != self.frame || !player::ui_input_is_valid(&input) {
            return self.fail(BridgeError::Worker(PacketError::InvalidInput));
        }
        if count == MAX_UI_EDITS {
            return self.fail(BridgeError::Worker(PacketError::TooManyInputs));
        }
        self.partial.request.ui_inputs[count] = input;
        self.partial.request.ui_count += 1;
        Ok(())
    }

    fn service(&mut self, report: &mut ProcessReport) -> Result<(), BridgeError> {
        while let Some(request) = self.requests.front() {
            match self.port.realtime().try_submit_hosted(request) {
                Ok(()) => {
                    self.requests.pop();
                    self.submitted_frame += BLOCK_FRAMES as u64;
                    report.submitted_packets += 1;
                }
                Err(rejected) if rejected.reason == PacketError::Full => break,
                Err(rejected) => return self.fail(BridgeError::Worker(rejected.reason)),
            }
        }
        while !self.audio.full() && self.received_frame < self.submitted_frame {
            let expected = Stamp {
                frame: self.received_frame,
                ..self.stamp
            };
            match self.port.realtime().try_receive_available(expected) {
                Ok(Some(output)) => {
                    self.received_frame += BLOCK_FRAMES as u64;
                    if output.stamp.frame < self.discard_before {
                        report.discarded_packets += 1;
                    } else {
                        self.audio.push(output)?;
                    }
                }
                Ok(None) => break,
                Err(error) => return self.fail(BridgeError::Worker(error)),
            }
        }
        Ok(())
    }

    /// Render arbitrary host pieces into caller-owned stereo slices. The host
    /// must call this for every elapsed frame, even while the slot is muted.
    /// An error silences the entire provided piece and permanently aborts this
    /// endpoint. No callback operation allocates, locks, waits, wakes or drops
    /// the endpoint/controller/resource ownership.
    pub fn process(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<ProcessReport, BridgeError> {
        self.process_mode(left, right, false)
    }

    /// Offline bounce only: pump fixed queues and yield until due submitted
    /// audio is ready. Realtime callers must use `process`, which never waits.
    pub fn process_offline(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Result<ProcessReport, BridgeError> {
        self.process_mode(left, right, true)
    }

    fn process_mode(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        offline: bool,
    ) -> Result<ProcessReport, BridgeError> {
        left.fill(0.);
        right.fill(0.);
        self.check()?;
        if left.len() != right.len() || left.len() > self.maximum {
            return self.fail(BridgeError::InvalidBuffer);
        }
        let end = match self.frame.checked_add(left.len() as u64) {
            Some(end) if end.checked_add(BLOCK_FRAMES as u64).is_some() => end,
            _ => return self.fail(BridgeError::TimelineOverflow),
        };
        let result = self.render_piece(left, right, end, offline);
        if result.is_err() {
            left.fill(0.);
            right.fill(0.);
        }
        result
    }

    fn render_piece(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        end: u64,
        offline: bool,
    ) -> Result<ProcessReport, BridgeError> {
        let mut report = ProcessReport::default();
        let mut offset = 0;
        while self.frame < end {
            self.service(&mut report)?;
            let within = self.frame as usize % BLOCK_FRAMES;
            let n = (BLOCK_FRAMES - within).min((end - self.frame) as usize);
            if self.frame >= u64::from(self.latency) {
                let source = self.frame - u64::from(self.latency);
                let packet_frame = source - source % BLOCK_FRAMES as u64;
                if self.active_frame != Some(packet_frame) {
                    while self
                        .audio
                        .front()
                        .is_some_and(|output| output.stamp.frame < packet_frame)
                    {
                        self.audio.pop();
                        report.discarded_packets += 1;
                    }
                    if offline {
                        let packet_end = packet_frame + BLOCK_FRAMES as u64;
                        // Never wait for a packet whose input is still partial.
                        // Backpressured sealed requests may not yet be submitted;
                        // pumping both queues below lets that frontier advance.
                        if packet_end > self.partial.request.stamp.frame {
                            return self.fail(BridgeError::Worker(PacketError::WrongFrame));
                        }
                        while !self
                            .audio
                            .front()
                            .is_some_and(|output| output.stamp.frame == packet_frame)
                        {
                            self.check()?;
                            if self.received_frame >= packet_end {
                                return self.fail(BridgeError::Worker(PacketError::WrongFrame));
                            }
                            self.service(&mut report)?;
                            if !self
                                .audio
                                .front()
                                .is_some_and(|output| output.stamp.frame == packet_frame)
                            {
                                std::thread::yield_now();
                            }
                        }
                    }
                    self.active_audio = if self
                        .audio
                        .front()
                        .is_some_and(|output| output.stamp.frame == packet_frame)
                    {
                        self.audio.pop()
                    } else {
                        self.discard_before = packet_frame + BLOCK_FRAMES as u64;
                        report.underrun_packets += 1;
                        None
                    };
                    self.active_frame = Some(packet_frame);
                }
                if let Some(output) = &self.active_audio {
                    for i in 0..n {
                        left[offset + i] = output.audio[within + i][0];
                        right[offset + i] = output.audio[within + i][1];
                    }
                }
            }
            offset += n;
            self.frame += n as u64;
            if self.frame.is_multiple_of(BLOCK_FRAMES as u64) {
                if let Err(error) = self.requests.push(self.partial) {
                    return self.fail(error);
                }
                self.partial = empty_request(Stamp {
                    frame: self.frame,
                    ..self.stamp
                });
            }
        }
        self.service(&mut report)?;
        Ok(report)
    }

    /// `consumed_host_frame` is activation-relative host time at the START of
    /// the whole callback, or a later externally acknowledged playout boundary.
    /// Never pass this endpoint's future frame after rendering that callback.
    /// Prefetch and PCM discard do not advance this completion fence.
    pub fn try_completion(
        &mut self,
        consumed_host_frame: u64,
    ) -> Result<Option<StampedCompletion>, BridgeError> {
        self.check()?;
        if consumed_host_frame < self.consumed_host_frame || consumed_host_frame > self.frame {
            return self.fail(BridgeError::Worker(PacketError::WrongFrame));
        }
        self.consumed_host_frame = consumed_host_frame;
        if consumed_host_frame < u64::from(self.latency) {
            return Ok(None);
        }
        let expected = Stamp {
            frame: consumed_host_frame - u64::from(self.latency),
            ..self.stamp
        };
        match self.port.realtime().try_receive_completion(expected) {
            Ok(completion) => Ok(completion),
            Err(error) => self.fail(BridgeError::Worker(error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_storage_and_latency_are_bounded_without_rounding_down() {
        assert_eq!(layout(1, 1), Ok((512, 11)));
        assert_eq!(layout(257, 2), Ok((1024, 13)));
        assert_eq!(layout(2048, 1), Ok((2304, 18)));
        for (maximum, lead) in [
            (0, 1),
            (MAX_HOST_FRAMES + 1, 1),
            (128, 0),
            (128, MAX_LEAD_PACKETS + 1),
        ] {
            assert_eq!(layout(maximum, lead), Err(BridgeError::InvalidConfig));
        }
        let mut ring = Ring::new(2, 0);
        ring.push(1).unwrap();
        ring.push(2).unwrap();
        assert_eq!(ring.push(3), Err(BridgeError::RequestCapacity));
        assert_eq!(ring.pop(), Some(1));
        ring.push(3).unwrap();
        assert_eq!(ring.pop(), Some(2));
        assert_eq!(ring.pop(), Some(3));
        assert_eq!(ring.pop(), None);
    }
    fn authored_worker(
        source: &str,
        maximum: usize,
    ) -> (
        super::super::worker::Worker,
        Bridge,
        std::path::PathBuf,
        String,
    ) {
        let (config, program) = super::super::worker::tests::authored_bank_with_script(source);
        let bank = config.bank.clone();
        let mut worker = super::super::worker::Worker::start_hosted(config, 7, 9).unwrap();
        worker
            .wait_ready(std::time::Duration::from_secs(5))
            .unwrap();
        let bridge = Bridge::new(worker.take_audio_port().unwrap(), 7, 9, maximum, 1).unwrap();
        (worker, bridge, bank, program)
    }

    #[test]
    fn offline_real_worker_matches_native_pcm_across_host_block_sizes() {
        use super::super::{
            playback::Renderer,
            program::parse_program,
            script::{Command, HostRoot, Input, Note},
        };
        for maximum in [1, 256, 2048, 4096, MAX_HOST_FRAMES] {
            let (mut worker, mut bridge, bank, source) =
                authored_worker("function onNote(e)postEvent(e)end", maximum);
            let program = parse_program(&source).unwrap();
            let mut reference = Renderer::new(&program, Default::default(), 48000).unwrap();
            let expected = reference
                .render(
                    &[Command {
                        frame: 0,
                        action: super::super::script::Action::Start(Note {
                            id: 1,
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
                        }),
                    }],
                    &[],
                    4096,
                )
                .unwrap();
            assert!(expected.iter().any(|frame| frame[0].abs() > 0.1));
            bridge
                .push_hosted(HostedInput::On {
                    root: HostRoot {
                        epoch: 7,
                        generation: 9,
                        token: 1,
                    },
                    input: Input {
                        frame: 0,
                        kind: InputKind::NoteOn {
                            channel: 0,
                            note: 60,
                            velocity: 100,
                        },
                    },
                })
                .unwrap();
            let latency = bridge.latency_frames() as usize;
            let mut left = vec![0.; latency + expected.len()];
            let mut right = vec![0.; left.len()];
            let mut offset = 0;
            while offset < left.len() {
                let end = (offset + maximum).min(left.len());
                let report = bridge
                    .process_offline(&mut left[offset..end], &mut right[offset..end])
                    .unwrap();
                assert_eq!(report.underrun_packets, 0);
                assert_eq!(report.discarded_packets, 0);
                offset = end;
            }
            assert!(
                left[..latency]
                    .iter()
                    .chain(&right[..latency])
                    .all(|value| *value == 0.)
            );
            for (index, expected) in expected.iter().enumerate() {
                assert_eq!(
                    [left[latency + index], right[latency + index]],
                    *expected,
                    "host maximum {maximum}, source frame {index}"
                );
            }
            drop(bridge);
            worker.stop();
            std::fs::remove_file(bank).unwrap();
        }
    }

    #[test]
    fn offline_incomplete_packet_and_stopped_worker_abort_without_waiting() {
        let (mut worker, mut bridge, bank, _) = authored_worker("", 256);
        // Construction guarantees positive packet-aligned latency; exercise the
        // wait guard independently so a future integration cannot spin on input.
        bridge.latency = 0;
        let mut left = [1.; 1];
        let mut right = [1.; 1];
        assert_eq!(
            bridge.process_offline(&mut left, &mut right),
            Err(BridgeError::Worker(PacketError::WrongFrame))
        );
        assert_eq!(left, [0.]);
        assert_eq!(right, [0.]);
        assert_eq!(bridge.frame(), 0);
        drop(bridge);
        worker.stop();
        std::fs::remove_file(bank).unwrap();

        let (mut worker, mut bridge, bank, _) = authored_worker("", 256);
        worker.stop();
        left.fill(1.);
        right.fill(1.);
        assert_eq!(
            bridge.process_offline(&mut left, &mut right),
            Err(BridgeError::Worker(PacketError::Stopped))
        );
        assert_eq!(left, [0.]);
        assert_eq!(right, [0.]);
        drop(bridge);
        std::fs::remove_file(bank).unwrap();
    }

    #[test]
    fn offline_wait_observes_worker_execution_failure_and_silences_piece() {
        use super::super::script::{HostRoot, Input};
        let (mut worker, mut bridge, bank, _) = authored_worker(
            "function onNote(e)error('authored offline failure')end",
            256,
        );
        bridge
            .push_hosted(HostedInput::On {
                root: HostRoot {
                    epoch: 7,
                    generation: 9,
                    token: 1,
                },
                input: Input {
                    frame: 0,
                    kind: InputKind::NoteOn {
                        channel: 0,
                        note: 60,
                        velocity: 100,
                    },
                },
            })
            .unwrap();
        let mut left = [1.; 256];
        let mut right = [1.; 256];
        let mut failed = false;
        for _ in 0..3 {
            if let Err(error) = bridge.process_offline(&mut left, &mut right) {
                assert_eq!(error, BridgeError::Worker(PacketError::Failed));
                assert!(left.iter().chain(&right).all(|value| *value == 0.));
                failed = true;
                break;
            }
        }
        assert!(
            failed,
            "due audio must wait for worker output or observe its failure"
        );
        drop(bridge);
        worker.stop();
        std::fs::remove_file(bank).unwrap();
    }
}
