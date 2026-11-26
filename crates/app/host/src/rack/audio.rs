//! The rack's audio side: renders every part into its output pair, from
//! the commands the control side sends.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kontra_engine::{Engine, Play, RenderMode, SampleTime, Timed};
use kontra_protocol::{Meters, Slowest, Stats};

use super::hold::{Held, Hold};
use super::{Channels, Command, Control, Event, HostPattern, MACROS, MacroTarget, Mix, OUTPUTS};
use crate::persist::Capture;
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
    /// Highest UI command sequence executed on this part's audio thread.
    last_ui_seq: u64,
    /// Last observed script activity fingerprint, to detect script mutations
    /// without polling while idle.
    last_script_fp: u64,
    /// Monotonic audio-side UI/persist epochs published to [`Channels`].
    ui_epoch: u64,
    persist_epoch: u64,
    /// The engines this one replaced, fading out under it: an install never
    /// stops a sound dead, effect tails included.
    outgoing: [Option<Fading>; FADING],
    /// Whether this part produced audible output in its last rendered block
    /// (voices or effect tails), used to decide whether [`Command::Recall`]
    /// must fade out before lending the engine back.
    audible: bool,
    /// A pending [`Command::Recall`] fading this part smoothly to silence
    /// before lending its engine to the control thread.
    recalling: Option<PendingRecall>,
}

/// A recall waiting for its live audio to ramp smoothly to silence before
/// lending the part's engine back to the control thread (zero allocations).
struct PendingRecall {
    recalling: Box<super::command::Recalling>,
    left: u32,
    total: u32,
}

/// An engine an install replaced: its whole output, effects included,
/// ramps to silence over [`FADE`], then it is freed off audio.
struct Fading {
    engine: Box<Engine>,
    /// Frames of the ramp still to play.
    left: u32,
    /// The level the ramp starts from: below 1 when the install cut short a
    /// recall's fade, so the handoff keeps the level that was playing.
    gain: f32,
}

/// Replaced engines fading at once per part; past that the most faded goes.
const FADING: usize = 2;
/// How long a replaced engine fades, in seconds.
const FADE: f64 = 0.02;
/// Engines one command may retire: the part's and its fading ones.
const RETIRES: usize = 1 + FADING;

/// The audio side: renders every part into its output pair.
pub struct RackAudio {
    channels: Arc<Channels>,
    stream: kontra_stream::StreamClient,
    parts: Vec<Option<Playing>>,
    outputs: Vec<Box<[[f32; 2]]>>,
    scratch: Box<[[f32; 2]]>,
    /// Per-stem scratch buffers for multi-output [`kontra_ir::AudioGraph`] parts.
    stem_scratch: Vec<Box<[[f32; 2]]>>,
    /// What an outgoing engine renders, before it is added in.
    tail: Box<[[f32; 2]]>,
    /// [`FADE`] in frames.
    fade: u32,
    events: Vec<Timed<Event>>,
    /// The multi script's engine (no zones), and the events it passed on
    /// this block; a fixed capacity.
    multi: Option<Box<Engine>>,
    routed: Vec<(u32, Event)>,
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
    /// By part, the capture being read, a budget of cells a block.
    capturing: Box<[Option<Box<Capture>>]>,
    /// The part whose capture is served first this block: it turns, so a
    /// busy part never keeps the budget from the others.
    capture_turn: usize,
    /// Offline, voices wait for their pages.
    mode: RenderMode,
    /// What each host macro moves ([`Command::Macro`]).
    macros: [Option<MacroTarget>; MACROS],
    /// This block's time in the script callbacks a control or widget move
    /// ran: script work, as a note's callbacks, not the commands' own.
    ui_scripts_ns: u64,
    /// Pre-allocated deferred command buffers so a single full return ring
    /// (e.g. `looks` or `captured`) never head-of-line blocks unrelated
    /// commands such as `Control`, `Mix`, `SetParam`, or `Panic`.
    stalled: Vec<Command>,
    stalled_next: Vec<Command>,
    /// Monotonic epoch generator shared across UI/persist bumps so epochs
    /// never collide across preset swaps or reloads.
    next_epoch: u64,
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
            stem_scratch: (0..OUTPUTS)
                .map(|_| vec![[0.0; 2]; block].into_boxed_slice())
                .collect(),
            tail: vec![[0.0; 2]; block].into_boxed_slice(),
            fade: (FADE * f64::from(rate)).max(1.0) as u32,
            events: Vec::with_capacity(4096),
            multi: None,
            routed: Vec::with_capacity(4096),
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
            capturing: (0..MAX_PARTS).map(|_| None).collect(),
            capture_turn: 0,
            mode: RenderMode::Live,
            macros: [None; MACROS],
            ui_scripts_ns: 0,
            stalled: Vec::with_capacity(256),
            stalled_next: Vec::with_capacity(256),
            next_epoch: 1,
        }
    }

    /// Consumes the sticky audio→control wake flag (non-blocking, RT-safe).
    /// Returns `true` when any return ring received data or any part's UI or
    /// persistent epoch advanced since the last call.
    #[must_use]
    pub fn take_wake(&self) -> bool {
        self.channels.wake_tick.swap(false, Ordering::AcqRel)
    }

    /// Active voices across all parts (plus fading outgoing engines), RT-safe.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.parts
            .iter()
            .flatten()
            .map(|p| {
                p.engine.active_voices()
                    + p.outgoing
                        .iter()
                        .flatten()
                        .map(|f| f.engine.active_voices().max(1))
                        .sum::<usize>()
            })
            .sum()
    }

    fn signal_wake(&self) {
        self.channels.wake_tick.store(true, Ordering::Release);
    }

    fn alloc_epoch(&mut self) -> u64 {
        let e = self.next_epoch;
        self.next_epoch = self.next_epoch.wrapping_add(1).max(1);
        e
    }

    fn mark_part_dirty(&mut self, part: usize) {
        let epoch = self.alloc_epoch();
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.ui_epoch = epoch;
            p.persist_epoch = epoch;
            p.last_script_fp = script_fingerprint(&p.engine);
        }
        if let Some(cell) = self.channels.ui_epochs.get(part) {
            cell.store(epoch, Ordering::Release);
        }
        if let Some(cell) = self.channels.persist_epochs.get(part) {
            cell.store(epoch, Ordering::Release);
        }
        self.signal_wake();
    }

    /// Ends the current notes on the next render, keeping the loaded parts.
    /// Audio-owned and bounded: does not drain commands or wait for the
    /// control side (which may be loading a library).
    pub fn panic(&mut self) {
        self.panic = true;
        self.played.clear();
    }

    /// Whether the host renders offline (a bounce, faster or slower than
    /// real time): then a voice waits for the sample pages it reads, deferred
    /// heads among them, instead of playing silence. Never live: it blocks.
    /// Fading engines too: one installed during a bounce must not keep
    /// blocking once the host plays live.
    pub fn set_render_mode(&mut self, mode: RenderMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        for p in self.parts.iter_mut().flatten() {
            p.engine.set_render_mode(mode);
            for f in p.outgoing.iter_mut().flatten() {
                f.engine.set_render_mode(mode);
            }
        }
    }

    /// The host moved macro `n` to `normalized` (0..1): its control moves
    /// now, between the frames before and after, and its script hears it.
    /// False when the macro has no target here (the tick moves it).
    pub fn move_macro(&mut self, n: usize, normalized: f64) -> bool {
        self.apply_commands();
        let Some(Some(t)) = self.macros.get(n).copied() else {
            return false;
        };
        let Some(Some(p)) = self.parts.get_mut(t.part) else {
            return false;
        };
        if p.generation != t.generation {
            return false;
        }
        let start = std::time::Instant::now();
        let plain = t.plain(normalized);
        match t.control {
            Control::Integer(id) | Control::Real(id) => {
                let value = match t.control {
                    Control::Real(_) => plain.to_bits() as i64,
                    _ => plain.round() as i64,
                };
                p.engine.set_click(t.slot, id, [false; 3], 0);
                p.engine.set_control(t.slot, id, value);
            }
            Control::Widget(n) => p.engine.set_widget(t.slot, n, plain, None),
        }
        self.ui_scripts_ns += ns_since(start);
        self.mark_part_dirty(t.part);
        true
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

    /// Grows the audio scratch and output mix buffers to accommodate
    /// `max_block` frames without dropping any loaded parts.
    ///
    /// Main-thread activation only: allocates when `max_block` exceeds
    /// [`Self::max_block`]. Never call on the audio thread.
    pub fn resize_max_block(&mut self, max_block: usize) {
        let block = max_block.max(1);
        if block <= self.scratch.len() {
            return;
        }
        self.outputs = (0..OUTPUTS)
            .map(|_| vec![[0.0; 2]; block].into_boxed_slice())
            .collect();
        self.scratch = vec![[0.0; 2]; block].into_boxed_slice();
        self.stem_scratch = (0..OUTPUTS)
            .map(|_| vec![[0.0; 2]; block].into_boxed_slice())
            .collect();
        self.tail = vec![[0.0; 2]; block].into_boxed_slice();
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
        self.ui_scripts_ns = 0;
        self.apply_commands();
        self.read_captures(frames);
        let commands = ns_since(start).saturating_sub(self.ui_scripts_ns);
        for out in &mut self.outputs {
            if let Some(o) = out.get_mut(..frames) {
                o.fill([0.0; 2]);
            }
        }
        let mut stats = Stats::default();
        // A multi script hears every event first; what it passes or sends
        // is what the parts hear.
        let mut routed = std::mem::take(&mut self.routed);
        let input = match self.multi.as_deref_mut() {
            Some(multi) => {
                routed.clear();
                for &(at, event) in input {
                    multi.midi_in(event, &mut |e| {
                        if routed.len() < routed.capacity() {
                            routed.push((at, e));
                        }
                    });
                }
                if let Some(scratch) = self.scratch.get_mut(..frames) {
                    multi.process(scratch, &[]);
                }
                routed.as_slice()
            }
            None => input,
        };
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
            if p.recalling.is_none() {
                let input = (input.iter().copied()).filter(|(_, e)| listens(channel, e));
                self.events
                    .extend(played.chain(input).take(room).map(|(at, body)| Timed {
                        at: SampleTime(p.held.due(now.0 + u64::from(at), &body)),
                        body,
                    }));
            }
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
            let stems = p.engine.output_count().min(OUTPUTS);
            let multi_stem = stems > 1;
            if multi_stem {
                let mut stem_slices: [&mut [[f32; 2]]; OUTPUTS] =
                    std::array::from_fn(|_| &mut [][..]);
                for (dst, buf) in stem_slices
                    .iter_mut()
                    .zip(self.stem_scratch.iter_mut().take(stems))
                {
                    if let Some(slice) = buf.get_mut(..frames) {
                        *dst = slice;
                    }
                }
                if let Some(active) = stem_slices.get_mut(..stems) {
                    p.engine.process_multi(active, &self.events);
                }
            } else {
                let Some(scratch) = self.scratch.get_mut(..frames) else {
                    continue;
                };
                p.engine.process(scratch, &self.events);
            }
            let main_scratch = if multi_stem {
                self.stem_scratch
                    .first_mut()
                    .and_then(|b| b.get_mut(..frames))
            } else {
                self.scratch.get_mut(..frames)
            };
            if let Some(main_buf) = main_scratch {
                for slot in &mut p.outgoing {
                    let Some(f) = slot else { continue };
                    if let Some(tail) = self.tail.get_mut(..frames) {
                        f.engine.process(tail, &[]);
                        for (s, t) in main_buf.iter_mut().zip(tail.iter()) {
                            let g = f.gain * f.left as f32 / self.fade as f32;
                            f.left = f.left.saturating_sub(1);
                            (s[0], s[1]) = (s[0] + t[0] * g, s[1] + t[1] * g);
                        }
                    }
                    if f.left == 0
                        && let Some(f) = slot.take()
                    {
                        match self.channels.retired.send(f.engine) {
                            Ok(()) => self.channels.wake_tick.store(true, Ordering::Release),
                            Err(engine) => {
                                // No room this block: it stays, silent, until there is.
                                *slot = Some(Fading {
                                    engine,
                                    left: 0,
                                    gain: 0.0,
                                });
                            }
                        }
                    }
                }
            }
            // Apply pre-recall fade-out across all rendered outputs so lending
            // the engine back never produces a step discontinuity / click.
            if let Some(pending) = p.recalling.as_mut() {
                let total = pending.total.max(1) as f32;
                let start_left = pending.left;
                if multi_stem {
                    for stem_buf in self.stem_scratch.iter_mut().take(stems) {
                        let Some(buf) = stem_buf.get_mut(..frames) else {
                            continue;
                        };
                        let mut left = start_left;
                        for s in buf.iter_mut() {
                            let g = left as f32 / total;
                            left = left.saturating_sub(1);
                            (s[0], s[1]) = (s[0] * g, s[1] * g);
                        }
                    }
                    pending.left = start_left.saturating_sub(frames as u32);
                } else if let Some(buf) = self.scratch.get_mut(..frames) {
                    for s in buf.iter_mut() {
                        let g = pending.left as f32 / total;
                        pending.left = pending.left.saturating_sub(1);
                        (s[0], s[1]) = (s[0] * g, s[1] * g);
                    }
                }
            }
            let mut rendered_peak = 0.0f32;
            if multi_stem {
                for stem_buf in self.stem_scratch.iter().take(stems) {
                    if let Some(buf) = stem_buf.get(..frames) {
                        for s in buf {
                            rendered_peak = rendered_peak.max(s[0].abs()).max(s[1].abs());
                        }
                    }
                }
            } else if let Some(buf) = self.scratch.get(..frames) {
                for s in buf {
                    rendered_peak = rendered_peak.max(s[0].abs()).max(s[1].abs());
                }
            }
            p.audible = rendered_peak > 1e-6;
            let (requests, generation) = (&self.channels.requests, p.generation);
            let mut lost = 0;
            let mut wake = false;
            p.engine.drain_requests(|r| {
                if requests.send((n, generation, r)).is_err() {
                    lost += 1;
                } else {
                    wake = true;
                }
            });
            let edits = &self.channels.zones.edits;
            p.engine.drain_zone_edits(|e| {
                let ok = edits.send((n, generation, e)).is_ok();
                wake |= ok;
                ok
            });
            let asks = &self.channels.unpurges;
            p.engine.drain_unpurges(|u| {
                let ok = asks.send((n, generation, u)).is_ok();
                wake |= ok;
                ok
            });
            let files = &self.channels.files;
            p.engine.drain_files(|r| {
                let ok = files.send((n, generation, r)).is_ok();
                wake |= ok;
                ok
            });
            self.dropped += lost;
            let fp = script_fingerprint(&p.engine);
            if !self.events.is_empty() || fp != p.last_script_fp {
                p.last_script_fp = fp;
                let epoch = self.next_epoch;
                self.next_epoch = self.next_epoch.wrapping_add(1).max(1);
                p.ui_epoch = epoch;
                p.persist_epoch = epoch;
                if let Some(cell) = self.channels.ui_epochs.get(n) {
                    cell.store(epoch, Ordering::Release);
                }
                if let Some(cell) = self.channels.persist_epochs.get(n) {
                    cell.store(epoch, Ordering::Release);
                }
                wake = true;
            }
            if wake {
                self.channels.wake_tick.store(true, Ordering::Release);
            }
            let s = p.engine.stats();
            stats.voices += u32::try_from(p.engine.active_voices()).unwrap_or(u32::MAX);
            stats.voices_stolen += s.voices_stolen;
            stats.voices_cut += s.voices_cut;
            stats.notes_refused += s.notes_refused;
            stats.events_refused += s.events_refused;
            stats.missing_frames += s.missing_frames;
            // Taken every block, so a muted part's buses show silence.
            if let Some(buses) = self.meters.buses.get_mut(n) {
                for (b, peak) in buses.iter_mut().enumerate() {
                    let [l, r] = p.engine.take_bus_peak(b);
                    if !p.mix.silent {
                        *peak = [peak[0].max(l), peak[1].max(r)];
                    }
                }
            }
            let Some(peak) = self.meters.parts.get_mut(n) else {
                continue;
            };
            if !p.mix.silent {
                let (l, r) = pan_gains(p.mix.gain, p.mix.pan);
                let base_out = usize::from(p.mix.output);
                if multi_stem {
                    for (stem_idx, stem_buf) in self.stem_scratch.iter().take(stems).enumerate() {
                        let Some(out) = self
                            .outputs
                            .get_mut(base_out + stem_idx)
                            .and_then(|o| o.get_mut(..frames))
                        else {
                            continue;
                        };
                        let Some(src) = stem_buf.get(..frames) else {
                            continue;
                        };
                        for (o, s) in out.iter_mut().zip(src.iter()) {
                            let (a, b) = (s[0] * l, s[1] * r);
                            peak[0] = peak[0].max(a.abs());
                            peak[1] = peak[1].max(b.abs());
                            o[0] += a * self.master;
                            o[1] += b * self.master;
                        }
                    }
                } else if let (Some(out), Some(scratch)) =
                    (self.outputs.get_mut(base_out), self.scratch.get(..frames))
                {
                    for (o, s) in out.iter_mut().zip(scratch.iter()) {
                        let (a, b) = (s[0] * l, s[1] * r);
                        peak[0] = peak[0].max(a.abs());
                        peak[1] = peak[1].max(b.abs());
                        o[0] += a * self.master;
                        o[1] += b * self.master;
                    }
                }
            }
            if p.recalling.as_ref().is_some_and(|r| r.left == 0)
                && !self.channels.recalled.is_full()
                && self.channels.retired.room() >= RETIRES
                && let Some(mut finished) = slot.take()
                && let Some(mut pending) = finished.recalling.take()
            {
                pending.recalling.engine = Some(finished.engine);
                for f in finished.outgoing.into_iter().flatten() {
                    if let Err(engine) = self.channels.retired.send(f.engine) {
                        self.dropped += 1;
                        drop(engine);
                    }
                }
                if let Err(recalling) = self.channels.recalled.send(pending.recalling) {
                    self.dropped += 1;
                    drop(recalling);
                } else {
                    self.channels.wake_tick.store(true, Ordering::Release);
                }
            }
        }
        self.routed = routed;
        self.panic = false;
        self.played.clear();
        // The song moved on: the next call (a later part of the host's
        // block) starts where this one ended, not back at the block's start.
        if let (Some((Play::Running, quarters, _)), Some(bpm)) = (&mut self.transport, self.tempo) {
            *quarters += frames as f64 / self.rate * f64::from(bpm) / 60.0;
        }
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
        stats.slowest = Slowest {
            ns: busy,
            frames: u32::try_from(frames).unwrap_or(u32::MAX),
        };
        stats.slowest_commands_ns = commands;
        self.channels.meters.publish(&self.meters);
        self.channels.stats.publish(&stats);
        self.meters = Meters::default();
    }

    /// Returns the part index if `cmd` mutates a part's state and must stay
    /// ordered behind earlier stalled mutations on that part. Read-only
    /// snapshots (`Look`, `Capture`) return `None` so they never head-of-line
    /// block controls, mix, param writes, or panic.
    fn mutated_part(cmd: &Command) -> Option<usize> {
        match *cmd {
            Command::Install { part, .. }
            | Command::Zones { part, .. }
            | Command::Unpurged { part, .. }
            | Command::Effect { part, .. }
            | Command::Remove { part, .. }
            | Command::Mix { part, .. }
            | Command::Hold { part, .. }
            | Command::File { part, .. }
            | Command::Control { part, .. }
            | Command::ControlText { part, .. }
            | Command::DropItem { part, .. }
            | Command::ControlDrop { part, .. }
            | Command::Widget { part, .. }
            | Command::Sound { part, .. }
            | Command::SetParam { part, .. } => Some(part),
            Command::Recall(ref r) => Some(r.part),
            Command::Multi(_)
            | Command::Macro { .. }
            | Command::Master(_)
            | Command::Tempo(_)
            | Command::Panic
            | Command::Play(_)
            | Command::Capture(_)
            | Command::Look(_) => None,
        }
    }

    /// Per-command room check: only requires space in the exact return ring(s)
    /// that `cmd` actually writes to. Zero-return-ring commands (`Control`,
    /// `Mix`, `Hold`, `Master`, `Tempo`, `Panic`, `Play`, `Macro`, `Widget`,
    /// `SetParam`, `ControlDrop`, `Unpurged`) never wait on `looks` or
    /// `captured`.
    fn can_execute(&self, cmd: &Command, blocked: &[bool; MAX_PARTS]) -> bool {
        if let Some(part) = Self::mutated_part(cmd)
            && blocked.get(part).copied().unwrap_or(false)
        {
            return false;
        }
        match cmd {
            Command::Install { .. } => {
                self.channels.retired.room() >= 1 && !self.channels.recalled.is_full()
            }
            Command::Remove { .. } => {
                self.channels.retired.room() >= RETIRES && !self.channels.recalled.is_full()
            }
            Command::Multi(_) => !self.channels.retired.is_full(),
            Command::Zones { .. } => !self.channels.zones.done.is_full(),
            Command::Effect { .. } => !self.channels.units.is_full(),
            Command::Recall(_) => {
                !self.channels.recalled.is_full() && self.channels.retired.room() >= RETIRES
            }
            Command::File { .. } => !self.channels.filed.is_full(),
            Command::Capture(c) => {
                self.capturing.get(c.part).is_none_or(Option::is_none)
                    || !self.channels.captured.is_full()
            }
            Command::Look(_) => !self.channels.looks.is_full(),
            Command::Sound { .. } => !self.channels.sounds.is_full(),
            Command::ControlText { .. } | Command::DropItem { .. } => {
                !self.channels.typed.is_full()
            }
            Command::Control { .. }
            | Command::ControlDrop { .. }
            | Command::Widget { .. }
            | Command::SetParam { .. }
            | Command::Mix { .. }
            | Command::Hold { .. }
            | Command::Master(_)
            | Command::Tempo(_)
            | Command::Panic
            | Command::Play(_)
            | Command::Macro { .. }
            | Command::Unpurged { .. } => true,
        }
    }

    fn apply_commands(&mut self) {
        let mut blocked = [false; MAX_PARTS];
        // Retry previously deferred commands first, in arrival order.
        let mut prev = std::mem::take(&mut self.stalled);
        let mut next = std::mem::take(&mut self.stalled_next);
        next.clear();
        for command in prev.drain(..) {
            if self.can_execute(&command, &blocked) {
                self.execute_command(command);
            } else {
                if let Some(part) = Self::mutated_part(&command)
                    && let Some(b) = blocked.get_mut(part)
                {
                    *b = true;
                }
                next.push(command);
            }
        }
        // Drain newly arrived commands without head-of-line blocking behind a
        // full unrelated return ring (`looks`, `captured`, etc.).
        while next.len() < next.capacity() {
            let Some(command) = self.channels.commands.recv() else {
                break;
            };
            if self.can_execute(&command, &blocked) {
                self.execute_command(command);
            } else {
                if let Some(part) = Self::mutated_part(&command)
                    && let Some(b) = blocked.get_mut(part)
                {
                    *b = true;
                }
                next.push(command);
            }
        }
        self.stalled = next;
        self.stalled_next = prev;
    }

    fn execute_command(&mut self, command: Command) {
        match command {
            Command::Install {
                part,
                mut engine,
                mix,
                generation,
            } => {
                let fp = script_fingerprint(&engine);
                let epoch = self.alloc_epoch();
                if let Some(slot) = self.parts.get_mut(part) {
                    if let Some(bpm) = self.tempo {
                        engine.set_tempo(bpm);
                    }
                    let (mut outgoing, mut gone) = ([None, None], None);
                    let mut superseded_recall = None;
                    if let Some(mut old) = slot.take() {
                        superseded_recall = old.recalling.take();
                        // A recall fading this part already scaled its
                        // output (tails included): fade on from there.
                        let level = superseded_recall
                            .as_ref()
                            .map_or(1.0, |r| r.left as f32 / r.total.max(1) as f32);
                        old.engine.reset();
                        // Every replaced engine fades; with no room, the
                        // most faded goes now.
                        let [mut a, mut b] = old.outgoing;
                        for f in [&mut a, &mut b].into_iter().flatten() {
                            f.gain *= level;
                        }
                        let left = self.fade;
                        let mut all = [
                            Some(Fading {
                                engine: old.engine,
                                left,
                                gain: level,
                            }),
                            a,
                            b,
                        ];
                        all.sort_by_key(|f| std::cmp::Reverse(f.as_ref().map_or(0, |f| f.left)));
                        let [a, b, c] = all;
                        (outgoing, gone) = ([a, b], c);
                    }
                    let mut engine = engine;
                    engine.set_render_mode(self.mode);
                    *slot = Some(Playing {
                        engine,
                        mix,
                        release: None,
                        held: Held::new(Hold::default()),
                        generation,
                        last_ui_seq: 0,
                        last_script_fp: fp,
                        ui_epoch: epoch,
                        persist_epoch: epoch,
                        outgoing,
                        audible: false,
                        recalling: None,
                    });
                    if let Some(cell) = self.channels.ui_epochs.get(part) {
                        cell.store(epoch, Ordering::Release);
                    }
                    if let Some(cell) = self.channels.persist_epochs.get(part) {
                        cell.store(epoch, Ordering::Release);
                    }
                    if let Some(pending) = superseded_recall {
                        self.return_recalling(pending.recalling);
                    }
                    if let Some(gone) = gone {
                        self.retire_engine(gone.engine);
                    }
                } else {
                    self.retire(Some(Playing {
                        engine,
                        mix,
                        release: None,
                        held: Held::new(Hold::default()),
                        generation,
                        last_ui_seq: 0,
                        last_script_fp: fp,
                        ui_epoch: epoch,
                        persist_epoch: epoch,
                        outgoing: [None, None],
                        audible: false,
                        recalling: None,
                    }));
                }
            }
            Command::Zones {
                part,
                generation,
                mut swap,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.swap_plan(&mut swap.plan, &swap.edits);
                    if let Some(racks) = &mut swap.racks {
                        p.engine.swap_racks(racks);
                    }
                }
                if let Err(swap) = self.channels.zones.done.send(swap) {
                    // Checked for room first; if it ever fails, it frees here.
                    self.dropped += 1;
                    drop(swap);
                } else {
                    self.signal_wake();
                }
            }
            Command::Unpurged {
                part,
                generation,
                unpurge,
                took,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.unpurged(unpurge, took);
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
                } else {
                    self.signal_wake();
                }
            }
            Command::Recall(recalling) => {
                let part = recalling.part;
                let generation = recalling.generation;
                let fade = self.fade;
                enum Action {
                    Armed,
                    Superseded(Box<super::command::Recalling>),
                    Immediate(Box<super::command::Recalling>),
                    Stale(Box<super::command::Recalling>),
                }
                let action = if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    if let Some(mut prev) = p.recalling.take() {
                        let old = std::mem::replace(&mut prev.recalling, recalling);
                        p.recalling = Some(prev);
                        Action::Superseded(old)
                    } else if !p.mix.silent
                        && (p.audible
                            || p.engine.active_voices() > 0
                            || p.outgoing.iter().any(Option::is_some))
                    {
                        // Fade active voices / tails smoothly to silence
                        // before lending the engine back to the control thread.
                        p.recalling = Some(PendingRecall {
                            recalling,
                            left: fade,
                            total: fade,
                        });
                        Action::Armed
                    } else {
                        Action::Immediate(recalling)
                    }
                } else {
                    Action::Stale(recalling)
                };
                match action {
                    Action::Armed => {}
                    Action::Superseded(old) | Action::Stale(old) => {
                        self.return_recalling(old);
                    }
                    Action::Immediate(mut recalling) => {
                        if let Some(slot) = self.parts.get_mut(part)
                            && let Some(p) = slot.take()
                        {
                            recalling.engine = Some(p.engine);
                            for f in p.outgoing.into_iter().flatten() {
                                self.retire_engine(f.engine);
                            }
                        }
                        self.return_recalling(recalling);
                    }
                }
            }
            Command::File {
                part,
                generation,
                mut reply,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.file_done(&mut reply);
                    self.mark_part_dirty(part);
                }
                if let Err(reply) = self.channels.filed.send((part, generation, reply)) {
                    // Checked for room first; if it ever fails, it frees here.
                    self.dropped += 1;
                    drop(reply);
                } else {
                    self.signal_wake();
                }
            }
            Command::Remove { part, generation } => {
                let old = self
                    .parts
                    .get_mut(part)
                    .filter(|slot| slot.as_ref().is_some_and(|p| p.generation == generation))
                    .and_then(Option::take);
                self.retire(old);
            }
            Command::Multi(engine) => {
                if let Some(old) = std::mem::replace(&mut self.multi, engine) {
                    self.retire_engine(old);
                }
            }
            Command::Mix {
                part,
                generation,
                mix,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    if p.mix.channel != mix.channel {
                        p.release = p.release.max(Some(p.held.hold.max()));
                    }
                    p.mix = mix;
                }
            }
            // Notes held under the old hold let go after the later of
            // the two, so no note-off overtakes its note-on.
            Command::Hold {
                part,
                generation,
                hold,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                    && p.held.hold != hold
                {
                    // The latest release asked for wins: several changes
                    // may come before one block.
                    let longest = p.held.hold.max().max(hold.max());
                    p.release = p.release.max(Some(longest));
                    p.held.hold = hold;
                }
            }
            Command::Macro { n, target } => {
                if let Some(slot) = self.macros.get_mut(n) {
                    *slot = target;
                }
            }
            Command::Master(gain) => self.master = gain,
            Command::Tempo(bpm) => self.set_tempo(bpm),
            // What was played before the panic ends with it.
            Command::Panic => self.panic(),
            Command::Play(event) => {
                if self.played.len() < self.played.capacity() {
                    self.played.push(event);
                } else {
                    self.dropped += 1;
                }
            }
            // Read from the next block on, a budget at a time.
            Command::Capture(c) => match self.capturing.get_mut(c.part) {
                Some(slot @ None) => *slot = Some(c),
                // One is being read already: this one goes back unread.
                _ => {
                    if let Err(c) = self.channels.captured.send(c) {
                        self.dropped += 1;
                        drop(c);
                    } else {
                        self.signal_wake();
                    }
                }
            },
            Command::Look(mut frame) => {
                match self.parts.get(frame.part) {
                    Some(Some(p)) if p.generation == frame.generation => {
                        frame.applied_seq = p.last_ui_seq;
                        frame.ui_epoch = p.ui_epoch;
                        frame.fill(&p.engine);
                    }
                    _ => frame.unfilled(),
                }
                if let Err(frame) = self.channels.looks.send(frame) {
                    self.dropped += 1;
                    drop(frame);
                } else {
                    self.signal_wake();
                }
            }
            Command::Sound {
                part,
                generation,
                group,
                envelope,
                mut insert,
            } => {
                let edit = insert.take();
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.set_group_sound(group, envelope, edit);
                }
                if let Err(insert) = self.channels.sounds.send(insert) {
                    // Checked for room first; if it ever fails, it frees here.
                    self.dropped += 1;
                    drop(insert);
                } else {
                    self.signal_wake();
                }
            }
            Command::Control {
                part,
                generation,
                ui_seq,
                slot,
                id,
                cell,
                value,
                keys,
            } => {
                let t = std::time::Instant::now();
                let mut ran = false;
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    let held = [keys.shift, keys.alt, keys.ctrl || keys.cmd];
                    p.engine.set_click(slot, id, held, cell);
                    p.engine.set_control(slot, id, value);
                    p.last_ui_seq = p.last_ui_seq.max(ui_seq);
                    ran = true;
                }
                self.ui_scripts_ns += ns_since(t);
                if ran {
                    self.mark_part_dirty(part);
                }
            }
            Command::ControlText {
                part,
                generation,
                ui_seq,
                slot,
                id,
                cell,
                text,
            } => {
                let t = std::time::Instant::now();
                let mut ran = false;
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    match cell {
                        Some(cell) => {
                            if let Some(vm) = p.engine.vm_mut(slot) {
                                vm.set_text(cell as usize, *text);
                            }
                        }
                        None => p.engine.set_picked(slot, id, *text),
                    }
                    p.engine.set_click(slot, id, [false; 3], 0);
                    p.engine.set_control(slot, id, 0);
                    p.last_ui_seq = p.last_ui_seq.max(ui_seq);
                    ran = true;
                }
                self.ui_scripts_ns += ns_since(t);
                if ran {
                    self.mark_part_dirty(part);
                }
                // Freed on the control side: the loop takes no command while the ring is full.
                if self.channels.typed.send(text).is_err() {
                    self.dropped += 1;
                } else {
                    self.signal_wake();
                }
            }
            Command::DropItem {
                part,
                generation,
                kind,
                first,
                text,
            } => {
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.drop_item(first, kind, *text);
                }
                if self.channels.typed.send(text).is_err() {
                    self.dropped += 1;
                } else {
                    self.signal_wake();
                }
            }
            Command::ControlDrop {
                part,
                generation,
                slot,
                id,
                drop,
                over,
            } => {
                let t = std::time::Instant::now();
                let mut ran = false;
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.set_click(slot, id, [false; 3], 0);
                    p.engine.drop_event(slot, drop, over);
                    p.engine.set_control(slot, id, 0);
                    ran = true;
                }
                self.ui_scripts_ns += ns_since(t);
                if ran {
                    self.mark_part_dirty(part);
                }
            }
            Command::Widget {
                part,
                generation,
                ui_seq,
                slot,
                n,
                value,
                cell,
            } => {
                let t = std::time::Instant::now();
                let mut ran = false;
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.set_widget(slot, n, value, cell);
                    p.last_ui_seq = p.last_ui_seq.max(ui_seq);
                    ran = true;
                }
                self.ui_scripts_ns += ns_since(t);
                if ran {
                    self.mark_part_dirty(part);
                }
            }
            Command::SetParam {
                part,
                generation,
                ui_seq,
                scope,
                addr,
                value,
            } => {
                let mut ran = false;
                if let Some(Some(p)) = self.parts.get_mut(part)
                    && p.generation == generation
                {
                    p.engine.set_param(scope, addr, value);
                    p.last_ui_seq = p.last_ui_seq.max(ui_seq);
                    ran = true;
                }
                if ran {
                    self.mark_part_dirty(part);
                }
            }
        }
    }

    /// Reads on the captures being read, at most [`CAPTURE_CELLS`] a frame
    /// of this block in all; a finished one goes back to the control side,
    /// as does one whose part was replaced or removed, unfinished.
    fn read_captures(&mut self, frames: usize) {
        let mut budget = frames.saturating_mul(CAPTURE_CELLS);
        let (first, parts) = (self.capture_turn, self.capturing.len());
        self.capture_turn = (first + 1) % parts.max(1);
        for n in (first..parts).chain(0..first) {
            let Some(slot) = self.capturing.get_mut(n) else {
                continue;
            };
            let Some(c) = slot else { continue };
            let done = match self.parts.get_mut(n) {
                Some(Some(p)) if p.generation == c.generation => {
                    if c.is_at_start() {
                        c.persist_epoch = p.persist_epoch;
                    }
                    c.read(&mut p.engine, &mut budget)
                }
                _ => true,
            };
            if done && let Some(c) = slot.take() {
                match self.channels.captured.send(c) {
                    Ok(()) => self.channels.wake_tick.store(true, Ordering::Release),
                    Err(c) => {
                        // No room: it goes back in a later block.
                        *slot = Some(c);
                    }
                }
            }
            if budget == 0 {
                break;
            }
        }
    }

    fn retire(&mut self, old: Option<Playing>) {
        if let Some(mut p) = old {
            if let Some(pending) = p.recalling.take() {
                self.return_recalling(pending.recalling);
            }
            self.retire_engine(p.engine);
            for f in p.outgoing.into_iter().flatten() {
                self.retire_engine(f.engine);
            }
        }
    }

    fn return_recalling(&mut self, recalling: Box<super::command::Recalling>) {
        if let Err(recalling) = self.channels.recalled.send(recalling) {
            // Checked for room before the command was taken; if it ever fails,
            // it frees here.
            self.dropped += 1;
            drop(recalling);
        } else {
            self.signal_wake();
        }
    }

    fn retire_engine(&mut self, engine: Box<Engine>) {
        if let Err(engine) = self.channels.retired.send(engine) {
            // Checked for room before the command was taken, so this does
            // not happen; if it ever does, the engine is freed here.
            self.dropped += 1;
            drop(engine);
        } else {
            self.signal_wake();
        }
    }
}

/// Fast, allocation-free fingerprint of an engine's script execution counters
/// across all script slots (KSP and Lua). Unchanged while completely idle.
fn script_fingerprint(engine: &Engine) -> u64 {
    let s = engine.stats();
    let mut h = s
        .script_starts
        .wrapping_add(s.script_resumes.wrapping_mul(31))
        .wrapping_add(s.script_turns.wrapping_mul(131))
        .wrapping_add(s.messages.wrapping_mul(1009));
    for slot in 0..8u8 {
        if let Some(vm) = engine.vm(slot) {
            h = h.wrapping_mul(1_099_511_628_211) ^ u64::from(vm.runs());
        }
    }
    h
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

/// Persistent cells a capture reads per frame of a block: 64k cells (512
/// KiB) in a 512-frame block, about 1% of its real time on a desktop core
/// (measured 20-40 us), so a million-cell part is read in about 20 blocks
/// (0.2 s at 48 kHz) and a few hundred cells at once.
const CAPTURE_CELLS: usize = 128;

/// Nanoseconds since `t`.
fn ns_since(t: std::time::Instant) -> u64 {
    u64::try_from(t.elapsed().as_nanos()).unwrap_or(u64::MAX)
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

/// Whether a part on Kontakt's `channel` (1..=64 across ports A to D, 0
/// for all) hears `e`. A pattern on every port matches on its channel.
fn listens(channel: u8, e: &Event) -> bool {
    match (channel, e.channel()) {
        (0, _) | (_, None) => true,
        (_, Some(c)) if e.port().is_none() => (channel - 1) % 16 == c,
        _ => e.port_channel().is_some_and(|c| c + 1 == channel),
    }
}
