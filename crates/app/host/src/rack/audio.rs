//! The rack's audio side: renders every part into its output pair, from
//! the commands the control side sends.

use std::sync::Arc;

use kontra_engine::{Engine, Play, SampleTime, Timed};
use kontra_protocol::{Meters, Stats};

use super::hold::{Held, Hold};
use super::{Channels, Command, Event, HostPattern, Mix, OUTPUTS};
use kontra_protocol::MAX_PARTS;

struct Playing {
    engine: Box<Engine>,
    mix: Mix,
    /// Its channel changed: the notes and pedal it held on the old one are
    /// let go at the next block, since their note-offs no longer reach it;
    /// or its hold changed. The frames past the block start to let go at.
    release: Option<u32>,
    /// What each note into it waits (auto-align).
    held: Held,
    /// Which load it is ([`Command::Install`]).
    generation: u64,
}

/// The audio side: renders every part into its output pair.
pub struct RackAudio {
    channels: Arc<Channels>,
    stream: kontra_stream::StreamClient,
    parts: Vec<Option<Playing>>,
    outputs: Vec<Box<[[f32; 2]]>>,
    scratch: Box<[[f32; 2]]>,
    events: Vec<Timed<Event>>,
    master: f32,
    tempo: Option<f32>,
    /// Frames per second.
    rate: f64,
    /// The host's transport for the next block: running, the song
    /// position in quarters, the time signature.
    transport: Option<(Play, f64, (u8, u8))>,
    panic: bool,
    /// Events from [`Command::Play`] for the next block; a fixed capacity.
    played: Vec<Event>,
    meters: Meters,
    dropped: u64,
    /// Time in [`RackAudio::process`] and the real time it rendered, in ns.
    busy_ns: u64,
    span_ns: u64,
}

impl RackAudio {
    pub(super) fn new(
        channels: Arc<Channels>,
        stream: kontra_stream::StreamClient,
        rate: u32,
        max_block: usize,
    ) -> Self {
        let block = max_block.max(1);
        Self {
            channels,
            stream,
            parts: (0..MAX_PARTS).map(|_| None).collect(),
            outputs: (0..OUTPUTS)
                .map(|_| vec![[0.0; 2]; block].into_boxed_slice())
                .collect(),
            scratch: vec![[0.0; 2]; block].into_boxed_slice(),
            events: Vec::with_capacity(4096),
            master: 1.0,
            tempo: None,
            rate: f64::from(rate.max(1)),
            transport: None,
            panic: false,
            played: Vec::with_capacity(PLAYED),
            meters: Meters::default(),
            dropped: 0,
            busy_ns: 0,
            span_ns: 0,
        }
    }

    /// Waits until the sample pages the parts asked for are read, so an
    /// offline bounce misses none. Offline rendering only: it blocks.
    pub fn settle(&self) {
        // ponytail: a fixed 5 s cap; a disk slower than that drops frames.
        self.stream.settle(std::time::Duration::from_secs(5));
    }

    /// The host's transport at the start of the next block: whether it
    /// runs, where it is in quarter notes and its time signature. Scripts
    /// read it, and hear it start and stop.
    pub fn set_transport(&mut self, playing: bool, quarters: f64, signature: (u8, u8)) {
        let play = if playing {
            Play::Running
        } else {
            Play::Stopped
        };
        // The host stopped: what its notes held lets go (tails and release
        // callbacks stay), as a part's old channel does. A host that sends
        // no note-off at stop leaves nothing sounding (v2's
        // `host_playing_transition`, src/plugin.rs).
        if !playing && self.transport.is_some_and(|(p, ..)| p == Play::Running) {
            for p in self.parts.iter_mut().flatten() {
                p.release = p.release.max(Some(p.held.hold.max()));
            }
        }
        self.transport = Some((play, quarters, signature));
    }

    /// The host's tempo from the next block on, in quarter notes per minute;
    /// parts installed later start at it too.
    pub fn set_tempo(&mut self, bpm: f32) {
        // A host without a transport reports 0.
        if bpm.is_finite() && bpm > 0.0 {
            self.tempo = Some(bpm);
        }
    }

    /// The largest block [`RackAudio::process`] renders at once.
    #[must_use]
    pub fn max_block(&self) -> usize {
        self.scratch.len()
    }

    /// Output pair `n` as the last [`RackAudio::process`] left it.
    #[must_use]
    pub fn output(&self, n: usize, frames: usize) -> &[[f32; 2]] {
        self.outputs
            .get(n)
            .and_then(|o| o.get(..frames))
            .unwrap_or_default()
    }

    /// Renders `frames` (at most [`RackAudio::max_block`]) into the outputs.
    /// `input` is (frame offset in this block, event), in time order; each
    /// part gets the events on its channel.
    pub fn process(&mut self, frames: usize, input: &[(u32, Event)]) {
        let start = std::time::Instant::now();
        let frames = frames.min(self.scratch.len());
        self.apply_commands();
        for out in &mut self.outputs {
            if let Some(o) = out.get_mut(..frames) {
                o.fill([0.0; 2]);
            }
        }
        let mut stats = Stats::default();
        for (n, slot) in self.parts.iter_mut().enumerate() {
            let Some(p) = slot else {
                continue;
            };
            let now = p.engine.now();
            let held = SampleTime(now.0 + u64::from(p.held.hold.max()));
            let channel = p.mix.channel;
            self.events.clear();
            if self.panic {
                // What sounds ends now; what is held ends as it comes due.
                self.events
                    .extend(PANIC.map(|body| Timed { at: now, body }));
                if held > now {
                    self.events
                        .extend(PANIC.map(|body| Timed { at: held, body }));
                }
            }
            // What the old channel held lets go before this block's notes.
            if let Some(after) = p.release.take() {
                p.held.clear();
                let at = SampleTime(now.0 + u64::from(after));
                self.events.extend(release().map(|body| Timed { at, body }));
            }
            let played = self.played.iter().map(|&body| (0, body));
            let room = self.events.capacity() - self.events.len();
            let input = (input.iter().copied())
                .filter(|(_, e)| channel == 0 || e.channel().is_none_or(|c| c + 1 == channel));
            self.events
                .extend(played.chain(input).take(room).map(|(at, body)| Timed {
                    at: SampleTime(p.held.due(now.0 + u64::from(at), &body)),
                    body,
                }));
            if let Some(bpm) = self.tempo {
                p.engine.set_tempo(bpm);
            }
            if let Some((play, quarters, signature)) = self.transport {
                // A held part plays its notes late, so it hears the song
                // where they land, and its start and stop as late: tempo-
                // synced scripts stay on the grid.
                let late = self.tempo.map_or(0.0, |bpm| {
                    f64::from(p.held.hold.max()) / self.rate * f64::from(bpm) / 60.0
                });
                let play = p.held.heard(play, now.0, frames as u64);
                p.engine.set_transport(play, quarters - late, signature);
            }
            let Some(scratch) = self.scratch.get_mut(..frames) else {
                continue;
            };
            p.engine.process(scratch, &self.events);
            let (requests, generation) = (&self.channels.requests, p.generation);
            let mut lost = 0;
            p.engine.drain_requests(|r| {
                if requests.send((n, generation, r)).is_err() {
                    lost += 1;
                }
            });
            self.dropped += lost;
            let s = p.engine.stats();
            stats.voices += u32::try_from(p.engine.active_voices()).unwrap_or(u32::MAX);
            stats.voices_stolen += s.voices_stolen;
            stats.notes_refused += s.notes_refused;
            stats.missing_frames += s.missing_frames;
            let peak = self.meters.parts.get_mut(n);
            let out = self.outputs.get_mut(usize::from(p.mix.output));
            let (Some(peak), Some(out)) = (peak, out) else {
                continue;
            };
            if p.mix.silent {
                continue;
            }
            let (l, r) = pan_gains(p.mix.gain, p.mix.pan);
            for (o, s) in out.iter_mut().zip(scratch.iter()) {
                let (a, b) = (s[0] * l, s[1] * r);
                peak[0] = peak[0].max(a.abs());
                peak[1] = peak[1].max(b.abs());
                o[0] += a * self.master;
                o[1] += b * self.master;
            }
        }
        self.panic = false;
        self.played.clear();
        for o in self.outputs.iter().filter_map(|o| o.get(..frames)) {
            for s in o {
                self.meters.master[0] = self.meters.master[0].max(s[0].abs());
                self.meters.master[1] = self.meters.master[1].max(s[1].abs());
            }
        }
        stats.dropped = self.dropped;
        let busy = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        self.busy_ns = self.busy_ns.saturating_add(busy);
        self.span_ns += (frames as f64 * 1e9 / self.rate) as u64;
        (stats.busy_ns, stats.span_ns) = (self.busy_ns, self.span_ns);
        self.channels.meters.publish(&self.meters);
        self.channels.stats.publish(&stats);
        self.meters = Meters::default();
    }

    fn apply_commands(&mut self) {
        // An install retires the engine it replaces; with no room to retire
        // it, the command waits for the next block rather than free here.
        while !self.channels.retired.is_full()
            && !self.channels.captured.is_full()
            && !self.channels.units.is_full()
            && !self.channels.looks.is_full()
        {
            let Some(command) = self.channels.commands.recv() else {
                break;
            };
            match command {
                Command::Install {
                    part,
                    mut engine,
                    mix,
                    generation,
                } => {
                    if let Some(slot) = self.parts.get_mut(part) {
                        if let Some(bpm) = self.tempo {
                            engine.set_tempo(bpm);
                        }
                        let old = slot.replace(Playing {
                            engine,
                            mix,
                            release: None,
                            held: Held::new(Hold::default()),
                            generation,
                        });
                        self.retire(old);
                    } else {
                        self.retire(Some(Playing {
                            engine,
                            mix,
                            release: None,
                            held: Held::new(Hold::default()),
                            generation,
                        }));
                    }
                }
                Command::Effect {
                    part,
                    generation,
                    mut unit,
                } => {
                    if let Some(Some(p)) = self.parts.get_mut(part)
                        && p.generation == generation
                        && let Some(new) = unit.take()
                    {
                        *unit = Some(p.engine.install(new));
                    }
                    if let Err(unit) = self.channels.units.send(unit) {
                        // Checked for room first; if it ever fails, it frees here.
                        self.dropped += 1;
                        drop(unit);
                    }
                }
                Command::Remove(part) => {
                    let old = self.parts.get_mut(part).and_then(Option::take);
                    self.retire(old);
                }
                Command::Mix(part, mix) => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        if p.mix.channel != mix.channel {
                            p.release = p.release.max(Some(p.held.hold.max()));
                        }
                        p.mix = mix;
                    }
                }
                // Notes held under the old hold let go after the later of
                // the two, so no note-off overtakes its note-on.
                Command::Hold(part, hold) => {
                    if let Some(Some(p)) = self.parts.get_mut(part)
                        && p.held.hold != hold
                    {
                        // The latest release asked for wins: several changes
                        // may come before one block.
                        let longest = p.held.hold.max().max(hold.max());
                        p.release = p.release.max(Some(longest));
                        p.held.hold = hold;
                    }
                }
                Command::Master(gain) => self.master = gain,
                Command::Tempo(bpm) => self.set_tempo(bpm),
                // What was played before the panic ends with it.
                Command::Panic => {
                    self.panic = true;
                    self.played.clear();
                }
                Command::Play(event) => {
                    if self.played.len() < self.played.capacity() {
                        self.played.push(event);
                    } else {
                        self.dropped += 1;
                    }
                }
                Command::Capture(mut c) => {
                    if let Some(Some(p)) = self.parts.get_mut(c.part) {
                        c.read(&mut p.engine);
                    }
                    if let Err(c) = self.channels.captured.send(c) {
                        self.dropped += 1;
                        drop(c);
                    }
                }
                Command::Look(mut frame) => {
                    match self.parts.get(frame.part) {
                        Some(Some(p)) if p.generation == frame.generation => frame.fill(&p.engine),
                        _ => frame.unfilled(),
                    }
                    if let Err(frame) = self.channels.looks.send(frame) {
                        self.dropped += 1;
                        drop(frame);
                    }
                }
                Command::Sound {
                    part,
                    group,
                    envelope,
                    insert,
                } => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        p.engine.set_group_sound(group, envelope, insert);
                    }
                }
                Command::Control {
                    part,
                    slot,
                    id,
                    cell,
                    value,
                    keys,
                } => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        let held = [keys.shift, keys.alt, keys.ctrl || keys.cmd];
                        p.engine.set_click(slot, id, held, cell);
                        p.engine.set_control(slot, id, value);
                    }
                }
                Command::Widget {
                    part,
                    slot,
                    n,
                    value,
                } => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        p.engine.set_widget(slot, n, value);
                    }
                }
            }
        }
    }

    fn retire(&mut self, old: Option<Playing>) {
        if let Some(p) = old
            && let Err(engine) = self.channels.retired.send(p.engine)
        {
            // Checked for room before the command was taken, so this does
            // not happen; if it ever does, the engine is freed here.
            self.dropped += 1;
            drop(engine);
        }
    }
}

const fn wildcard(clap: bool) -> HostPattern {
    HostPattern {
        port: -1,
        channel: -1,
        key: -1,
        id: -1,
        clap,
    }
}

/// What a part's old channel held: every note let go (CLAP-addressed or
/// not), and the sustain pedal up on every channel.
fn release() -> [Event; 18] {
    let off = |clap| Event::NoteOff(wildcard(clap));
    std::array::from_fn(|n| match u8::try_from(n) {
        Ok(ch @ 0..=15) => Event::midi1(0xB0 | ch, 64, 0),
        _ => off(n == 16),
    })
}

/// Events [`Command::Play`] holds for one block.
const PLAYED: usize = 64;

/// Every note, CLAP-addressed or not.
const PANIC: [Event; 2] = [Event::Choke(wildcard(true)), Event::Choke(wildcard(false))];

/// A balance control: unity on both sides at the centre, so a part's own
/// stereo image stays as its library made it.
fn pan_gains(gain: f32, pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    (gain * (1.0 - pan).min(1.0), gain * (1.0 + pan).min(1.0))
}
