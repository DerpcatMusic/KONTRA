//! The rack (07-APP.md §4): up to [`MAX_PARTS`] parts, each one loaded
//! instrument and the engine playing it. [`Rack`] lives on the control
//! thread and builds engines there; [`RackAudio`] lives on the audio thread
//! and only renders. They share nothing but the protocol's rings and cells,
//! so no lock is reachable from audio.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::{Engine, SampleTime, Timed, ir};

use crate::persist::Capture;
pub use kontra_engine::{Event, HostNote, HostPattern, NoteExpression, Plan};
use kontra_protocol::{MAX_PARTS, MeterCells, Meters, Ring, StatCells, Stats};
pub use kontra_stream::{Store, StoreConfig};

/// Stereo outputs a rack renders: one pair per part at most.
pub const OUTPUTS: usize = MAX_PARTS;

/// How a part sounds in the rack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mix {
    /// MIDI channel 1..=16 it listens on, or 0 for every channel.
    pub channel: u8,
    /// The stereo output pair it plays into, from 0.
    pub output: u8,
    /// Linear gain, and pan from -1 (left) to 1.
    pub gain: f32,
    pub pan: f32,
    /// Silenced by its own mute, or by another part's solo.
    pub silent: bool,
}

impl Mix {
    pub const DEFAULT: Self = Self {
        channel: 0,
        output: 0,
        gain: 1.0,
        pan: 0.0,
        silent: false,
    };
}

/// What the control thread asks of the audio thread.
pub enum Command {
    /// Put `engine` in `part`, retiring the one there.
    Install {
        part: usize,
        engine: Box<Engine>,
        mix: Mix,
    },
    Remove(usize),
    Mix(usize, Mix),
    /// Linear master gain.
    Master(f32),
    /// Tempo in quarter notes per minute.
    Tempo(f32),
    /// End every note on every part at once.
    Panic,
    /// Play `Event` on every part at the next block (the editor's keyboard
    /// and wheels).
    Play(Event),
    /// Hold every event into `part` this many frames: auto-align.
    Hold(usize, u32),
    /// Read a part's persistent script cells and send them back.
    Capture(Box<Capture>),
    /// The user moved control `id` of script `slot` to `value`.
    Control {
        part: usize,
        slot: u8,
        id: i32,
        value: i64,
    },
}

/// The channels between [`Rack`] and [`RackAudio`].
pub struct Channels {
    pub commands: Ring<Command>,
    pub retired: Ring<Box<Engine>>,
    pub captured: Ring<Box<Capture>>,
    pub meters: MeterCells,
    pub stats: StatCells,
}

/// A loaded part as the control thread knows it.
pub struct Part {
    /// The instrument file, or None for one built in memory.
    pub path: Option<PathBuf>,
    pub name: String,
    pub diagnostics: Vec<ir::Diagnostic>,
    /// By script slot, what its UI tree is built from.
    pub faces: Vec<Option<crate::ui::Face>>,
    pub mix: Mix,
    pub solo: bool,
    pub mute: bool,
    pub script: ScriptState,
    /// What the engine plays, to measure it again (auto-align); None for
    /// an engine built elsewhere.
    pub plan: Option<Arc<Plan>>,
    /// How late it sounds at its latest, in ms, once auto-align measured it.
    pub delay_ms: Option<f32>,
    /// Its library's interface, as `on init` left it.
    pub view: Option<crate::view::View>,
}

/// What the rack keeps to save a part's scripts.
#[derive(Default)]
pub struct ScriptState {
    /// By script slot, its persistent variables.
    pub vars: Vec<crate::persist::Vars>,
    /// By instrument script, the saved values it loaded with.
    pub loaded: Vec<Vec<String>>,
    generation: u64,
    latest: Option<Box<Capture>>,
    asked: bool,
}

impl ScriptState {
    /// Moves on every install: a part with the same generation is the same
    /// load.
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

/// Plays events on every part from any thread, straight into the audio
/// side's command ring: it needs no lock on the [`Rack`], so the editor's
/// keyboard is not held up by a load.
#[derive(Clone)]
pub struct Player(Arc<Channels>);

impl Player {
    /// Plays `event` on every part at the next block; false when the ring
    /// is full.
    pub fn play(&self, event: Event) -> bool {
        self.0.commands.send(Command::Play(event)).is_ok()
    }

    /// Ends every note; false when the ring is full.
    pub fn panic(&self) -> bool {
        self.0.commands.send(Command::Panic).is_ok()
    }
}

/// The control side: loads parts, sets the mix and asks the audio side.
pub struct Rack {
    channels: Arc<Channels>,
    rate: u32,
    store: Store,
    parts: Vec<Option<Part>>,
    /// Commands the ring refused, sent again on the next [`Rack::tick`].
    pending: std::collections::VecDeque<Command>,
    /// Counts installs, so a capture of a replaced engine is not taken for
    /// its successor's.
    generation: u64,
}

impl Rack {
    /// A rack rendering at `rate`, and the audio side that plays it.
    #[must_use]
    pub fn new(rate: u32, store: Store, max_block: usize) -> (Self, RackAudio) {
        let channels = Arc::new(Channels {
            commands: Ring::new(256),
            retired: Ring::new(64),
            captured: Ring::new(2 * MAX_PARTS),
            meters: MeterCells::default(),
            stats: StatCells::default(),
        });
        let audio = RackAudio::new(Arc::clone(&channels), store.client(), max_block);
        let rack = Self {
            channels,
            rate,
            store,
            parts: (0..MAX_PARTS).map(|_| None).collect(),
            pending: std::collections::VecDeque::new(),
            generation: 0,
        };
        (rack, audio)
    }

    /// A [`Player`] for this rack.
    #[must_use]
    pub fn player(&self) -> Player {
        Player(Arc::clone(&self.channels))
    }

    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    #[must_use]
    pub fn parts(&self) -> &[Option<Part>] {
        &self.parts
    }

    /// The first part with nothing loaded.
    #[must_use]
    pub fn free(&self) -> Option<usize> {
        self.parts.iter().position(Option::is_none)
    }

    /// Reads, prepares and starts the instrument at `path` (its `on init`
    /// runs here, off the audio thread), then hands it to `part`.
    ///
    /// # Errors
    /// When the file is not an instrument this can read, or `part` is past
    /// the rack.
    pub fn load(&mut self, part: usize, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        self.load_ir(part, crate::instrument::read(path)?, Some(path.to_owned()))
    }

    /// [`Rack::load`], with each instrument script's saved values replaced
    /// by `scripts` (as [`Rack::saved_scripts`] gave them).
    ///
    /// # Errors
    /// As [`Rack::load`].
    pub fn load_saved(
        &mut self,
        part: usize,
        path: &Path,
        scripts: &[Vec<String>],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut instrument = crate::instrument::read(path)?;
        for (script, saved) in instrument.scripts.iter_mut().zip(scripts) {
            script.saved.clone_from(saved);
        }
        self.load_ir(part, instrument, Some(path.to_owned()))
    }

    /// By instrument script, the saved values that bring `part`'s scripts
    /// back as they are now (as of the last [`Rack::tick`]).
    #[must_use]
    pub fn saved_scripts(&self, part: usize) -> Vec<Vec<String>> {
        let Some(Some(p)) = self.parts.get(part) else {
            return Vec::new();
        };
        let s = &p.script;
        match &s.latest {
            Some(capture) => crate::persist::saved(&s.loaded, &s.vars, capture),
            None => s.loaded.clone(),
        }
    }

    /// [`Rack::load`] for an instrument already read into the IR.
    ///
    /// # Errors
    /// When `part` is past the rack.
    pub fn load_ir(
        &mut self,
        part: usize,
        instrument: ir::Instrument,
        path: Option<PathBuf>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if part >= MAX_PARTS {
            return Err(format!("part {part} is past the rack's {MAX_PARTS}").into());
        }
        let saved = instrument.scripts.iter().map(|s| s.saved.clone()).collect();
        let loaded = crate::instrument::build(instrument, self.rate, &mut self.store);
        let plan = Arc::new(loaded.plan);
        let engine = Engine::new(Arc::clone(&plan), self.store.client(), self.rate);
        let view = crate::view::View::of(&loaded.faces, &engine, path.as_deref());
        let part_info = Part {
            view,
            plan: Some(plan),
            delay_ms: None,
            path,
            name: loaded.name,
            diagnostics: loaded.diagnostics,
            faces: loaded.faces,
            mix: self.mix_of(part),
            solo: false,
            mute: false,
            script: ScriptState {
                vars: loaded.persist,
                loaded: saved,
                ..ScriptState::default()
            },
        };
        self.install(part, Box::new(engine), part_info);
        Ok(())
    }

    /// Hands a built engine to `part`; `info` is what the rack keeps of it.
    pub fn install(&mut self, part: usize, engine: Box<Engine>, info: Part) {
        let Some(slot) = self.parts.get_mut(part) else {
            return;
        };
        self.generation += 1;
        let mut info = info;
        info.script.generation = self.generation;
        let (solo, mute, mix) = slot.as_ref().map_or((info.solo, info.mute, info.mix), |p| {
            (p.solo, p.mute, p.mix)
        });
        *slot = Some(Part {
            solo,
            mute,
            mix,
            ..info
        });
        self.send(Command::Install {
            part,
            engine,
            mix: Mix {
                silent: self.silent(part),
                ..mix
            },
        });
        self.resend_mix();
    }

    pub fn remove(&mut self, part: usize) {
        if let Some(slot) = self.parts.get_mut(part) {
            *slot = None;
            self.send(Command::Remove(part));
            self.resend_mix();
        }
    }

    /// Sets a part's channel, output, gain and pan; `silent` is the rack's.
    pub fn set_mix(&mut self, part: usize, mix: Mix) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.mix = mix;
            self.resend_mix();
        }
    }

    pub fn set_solo_mute(&mut self, part: usize, solo: bool, mute: bool) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.solo = solo;
            p.mute = mute;
            self.resend_mix();
        }
    }

    /// The user moved `widget` of `part`'s interface to `value`: the
    /// control's script gets it and runs its callback at the next block.
    pub fn control(&mut self, part: usize, widget: usize, value: f64) {
        let Some(Some(p)) = self.parts.get(part) else {
            return;
        };
        if let Some((slot, id, value)) = p.view.as_ref().and_then(|v| v.control(widget, value)) {
            self.set_control(part, slot, id, value);
        }
    }

    /// Records `part`'s measured attack delay (auto-align), saved with it.
    pub fn set_delay(&mut self, part: usize, ms: f32) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.delay_ms = Some(ms);
        }
    }

    /// Holds every event into `part` by `frames` from the next block on,
    /// so it sounds in time with a later part (auto-align).
    pub fn set_hold(&mut self, part: usize, frames: u32) {
        self.send(Command::Hold(part, frames));
    }

    pub fn set_master(&mut self, gain: f32) {
        self.send(Command::Master(gain));
    }

    pub fn set_tempo(&mut self, bpm: f32) {
        self.send(Command::Tempo(bpm));
    }

    pub fn panic(&mut self) {
        self.send(Command::Panic);
    }

    /// Plays `event` on every part at the next block, whatever its channel.
    pub fn play(&mut self, event: Event) {
        self.send(Command::Play(event));
    }

    /// Moves control `id` of `part`'s script `slot` to `value`, as the user
    /// turning it: its callbacks run on the audio thread.
    pub fn set_control(&mut self, part: usize, slot: u8, id: i32, value: i64) {
        self.send(Command::Control {
            part,
            slot,
            id,
            value,
        });
    }

    /// Frees what the audio thread let go of and resends refused commands.
    /// Call it from the control thread now and then.
    pub fn tick(&mut self) {
        while self.channels.retired.recv().is_some() {}
        while let Some(c) = self.channels.captured.recv() {
            if let Some(Some(p)) = self.parts.get_mut(c.part)
                && p.script.generation == c.generation
            {
                p.script.asked = false;
                p.script.latest = Some(c);
            }
        }
        // Ask again for every part with persistent values: what a save
        // writes is at most one tick old.
        let ask: Vec<_> = (self.parts.iter_mut().enumerate())
            .filter_map(|(n, p)| {
                let s = &mut p.as_mut()?.script;
                if s.asked || s.vars.iter().all(|v| v.vars.is_empty()) {
                    return None;
                }
                s.asked = true;
                Some(Capture::new(n, s.generation, &s.vars))
            })
            .collect();
        for c in ask {
            self.send(Command::Capture(c));
        }
        while let Some(c) = self.pending.pop_front() {
            if let Err(c) = self.channels.commands.send(c) {
                self.pending.push_front(c);
                break;
            }
        }
    }

    /// The highest levels the audio thread published since the last read.
    #[must_use]
    pub fn meters(&self) -> Meters {
        self.channels.meters.take()
    }

    /// The stats the audio thread published last, if any since the last read.
    #[must_use]
    pub fn stats(&self) -> Option<Stats> {
        self.channels.stats.take()
    }

    fn mix_of(&self, part: usize) -> Mix {
        self.parts
            .get(part)
            .and_then(|p| p.as_ref().map(|p| p.mix))
            // A new part plays on the main output: a host often has only
            // that one active.
            .unwrap_or(Mix::DEFAULT)
    }

    fn silent(&self, part: usize) -> bool {
        let soloed = self.parts.iter().flatten().any(|p| p.solo);
        self.parts
            .get(part)
            .and_then(Option::as_ref)
            .is_some_and(|p| p.mute || (soloed && !p.solo))
    }

    fn resend_mix(&mut self) {
        for part in 0..MAX_PARTS {
            if let Some(Some(p)) = self.parts.get(part) {
                let mix = Mix {
                    silent: self.silent(part),
                    ..p.mix
                };
                self.send(Command::Mix(part, mix));
            }
        }
    }

    fn send(&mut self, command: Command) {
        if !self.pending.is_empty() {
            self.pending.push_back(command);
            return;
        }
        if let Err(c) = self.channels.commands.send(command) {
            self.pending.push_back(c);
        }
    }
}

struct Playing {
    engine: Box<Engine>,
    mix: Mix,
    /// Its channel changed: the notes and pedal it held on the old one are
    /// let go at the next block, since their note-offs no longer reach it;
    /// or its hold changed. The frames past the block start to let go at.
    release: Option<u32>,
    /// Frames every event into it waits (auto-align).
    hold: u32,
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
    panic: bool,
    /// Events from [`Command::Play`] for the next block; a fixed capacity.
    played: Vec<Event>,
    meters: Meters,
    dropped: u64,
}

impl RackAudio {
    fn new(channels: Arc<Channels>, stream: kontra_stream::StreamClient, max_block: usize) -> Self {
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
            panic: false,
            played: Vec::with_capacity(PLAYED),
            meters: Meters::default(),
            dropped: 0,
        }
    }

    /// Waits until the sample pages the parts asked for are read, so an
    /// offline bounce misses none. Offline rendering only: it blocks.
    pub fn settle(&self) {
        // ponytail: a fixed 5 s cap; a disk slower than that drops frames.
        self.stream.settle(std::time::Duration::from_secs(5));
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
            let held = SampleTime(now.0 + u64::from(p.hold));
            let channel = p.mix.channel;
            self.events.clear();
            if self.panic {
                // What sounds ends now; what is held ends as it comes due.
                self.events
                    .extend(PANIC.map(|body| Timed { at: now, body }));
                if p.hold > 0 {
                    self.events
                        .extend(PANIC.map(|body| Timed { at: held, body }));
                }
            }
            // What the old channel held lets go before this block's notes.
            if let Some(after) = p.release.take() {
                let at = SampleTime(now.0 + u64::from(after));
                self.events.extend(release().map(|body| Timed { at, body }));
            }
            self.events
                .extend(self.played.iter().map(|&body| Timed { at: held, body }));
            let room = self.events.capacity() - self.events.len();
            self.events.extend(
                input
                    .iter()
                    .filter(|(_, e)| channel == 0 || e.channel().is_none_or(|c| c + 1 == channel))
                    .take(room)
                    .map(|&(at, body)| Timed {
                        at: SampleTime(held.0 + u64::from(at)),
                        body,
                    }),
            );
            if let Some(bpm) = self.tempo {
                p.engine.set_tempo(bpm);
            }
            let Some(scratch) = self.scratch.get_mut(..frames) else {
                continue;
            };
            p.engine.process(scratch, &self.events);
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
        self.channels.meters.publish(&self.meters);
        self.channels.stats.publish(&stats);
        self.meters = Meters::default();
    }

    fn apply_commands(&mut self) {
        // An install retires the engine it replaces; with no room to retire
        // it, the command waits for the next block rather than free here.
        while !self.channels.retired.is_full() && !self.channels.captured.is_full() {
            let Some(command) = self.channels.commands.recv() else {
                break;
            };
            match command {
                Command::Install {
                    part,
                    mut engine,
                    mix,
                } => {
                    if let Some(slot) = self.parts.get_mut(part) {
                        if let Some(bpm) = self.tempo {
                            engine.set_tempo(bpm);
                        }
                        let old = slot.replace(Playing {
                            engine,
                            mix,
                            release: None,
                            hold: 0,
                        });
                        self.retire(old);
                    } else {
                        self.retire(Some(Playing {
                            engine,
                            mix,
                            release: None,
                            hold: 0,
                        }));
                    }
                }
                Command::Remove(part) => {
                    let old = self.parts.get_mut(part).and_then(Option::take);
                    self.retire(old);
                }
                Command::Mix(part, mix) => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        if p.mix.channel != mix.channel {
                            p.release = p.release.max(Some(p.hold));
                        }
                        p.mix = mix;
                    }
                }
                // Notes held under the old hold let go after the later of
                // the two, so no note-off overtakes its note-on.
                Command::Hold(part, frames) => {
                    if let Some(Some(p)) = self.parts.get_mut(part)
                        && p.hold != frames
                    {
                        // The latest release asked for wins: several changes
                        // may come before one block.
                        p.release = p.release.max(Some(p.hold.max(frames)));
                        p.hold = frames;
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
                    if let Some(Some(p)) = self.parts.get(c.part) {
                        c.read(&p.engine);
                    }
                    if let Err(c) = self.channels.captured.send(c) {
                        self.dropped += 1;
                        drop(c);
                    }
                }
                Command::Control {
                    part,
                    slot,
                    id,
                    value,
                } => {
                    if let Some(Some(p)) = self.parts.get_mut(part) {
                        p.engine.set_control(slot, id, value);
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
