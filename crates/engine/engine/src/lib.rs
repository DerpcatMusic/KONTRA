//! The audio-thread engine: one clock, one queue, one note ledger, one voice
//! pool, one render loop (02-ENGINE.md, 13-FRESH-CORE.md).
//!
//! Every event carries a sample time and takes effect at it; the loop renders
//! up to the next due item, so the host block size never changes the output
//! (rule 3). A note owns the voices it starts, and they all begin on the
//! note's sample (rule 4). Nothing allocates after [`Engine::new`].

mod ledger;
mod plan;
mod queue;

use std::sync::Arc;

use kontra_events::{Event, HostNote, HostPattern, NoteExpression};
use kontra_stream::StreamClient;
use kontra_time::{Frames, SampleTime};
use kontra_voice::{Voice, VoiceStart};

pub use ledger::NoteRef;
pub use plan::{Plan, Zone};
pub use queue::Timed;

use ledger::{Ledger, Note};
use queue::{EventQueue, Work};

/// Counters since the engine was created. Each names a refusal or a loss.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Voices taken from a sounding note because the pool was full.
    pub voices_stolen: u64,
    /// Note-ons refused because the note table was full.
    pub notes_refused: u64,
    /// Events refused because the queue was full.
    pub events_refused: u64,
    /// Source frames voices played as silence because data was late.
    pub missing_frames: u64,
}

struct Slot {
    voice: Voice,
    /// The owner. `None` only while the voice is idle.
    note: Option<NoteRef>,
    zone: usize,
    started: SampleTime,
    released: bool,
}

pub struct Engine {
    now: SampleTime,
    rate: u32,
    plan: Arc<Plan>,
    stream: StreamClient,
    queue: EventQueue,
    notes: Ledger,
    voices: Vec<Slot>,
    /// Sustain pedal down, per MIDI channel.
    pedal: [bool; 16],
    /// Scratch for ledger queries, capacity = note table size.
    hits: Vec<NoteRef>,
    ended: Vec<HostNote>,
    stats: Stats,
}

impl Engine {
    /// Allocates every pool the plan asks for. Not real-time.
    #[must_use]
    pub fn new(plan: Arc<Plan>, stream: StreamClient, rate: u32) -> Self {
        let voices = (0..plan.voices)
            .map(|_| Slot {
                voice: Voice::new(),
                note: None,
                zone: 0,
                started: SampleTime(0),
                released: false,
            })
            .collect();
        let notes = Ledger::new(plan.notes);
        Self {
            now: SampleTime(0),
            rate: rate.max(1),
            queue: EventQueue::new(plan.queue),
            hits: Vec::with_capacity(notes.capacity()),
            ended: Vec::with_capacity(notes.capacity()),
            notes,
            plan,
            stream,
            voices,
            pedal: [false; 16],
            stats: Stats::default(),
        }
    }

    #[must_use]
    pub fn now(&self) -> SampleTime {
        self.now
    }

    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Host notes that ended during the last [`Engine::process`] (CLAP note end).
    #[must_use]
    pub fn ended(&self) -> &[HostNote] {
        &self.ended
    }

    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|s| s.voice.is_active()).count()
    }

    /// Renders `out.len()` frames from [`Engine::now`], applying each input at
    /// its sample time. Input stamped before now applies at once.
    pub fn process(&mut self, out: &mut [[f32; 2]], input: &[Timed<Event>]) {
        out.fill([0.0; 2]);
        self.ended.clear();
        for e in input {
            if !self.queue.push(e.at, Work::Input(e.body)) {
                self.stats.events_refused += 1;
            }
        }
        let start = self.now;
        let end = start.advance(Frames(u32::try_from(out.len()).unwrap_or(u32::MAX)));
        while self.now < end {
            while let Some(work) = self.queue.pop_due(self.now) {
                self.dispatch(work);
            }
            let until = self.queue.next_at().map_or(end, |at| at.min(end));
            let span = (self.now.0 - start.0) as usize..(until.0 - start.0) as usize;
            if let Some(span) = out.get_mut(span) {
                for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
                    let before = slot.voice.missing_frames();
                    slot.voice.render(&self.stream, self.now.0, span);
                    self.stats.missing_frames += slot.voice.missing_frames() - before;
                }
            }
            self.now = until;
        }
        self.end_silent_notes();
    }

    fn dispatch(&mut self, work: Work) {
        let Work::Input(event) = work;
        match event {
            Event::NoteOn {
                note,
                velocity,
                tune,
            } => self.note_on(note, velocity, tune),
            Event::NoteOff(pattern) | Event::NoteOffVelocity(pattern, _) => self.note_off(pattern),
            Event::Choke(pattern) => self.choke(pattern),
            Event::Expression(pattern, expression) => self.expression(pattern, expression),
            Event::Ump(words) => self.ump(words),
        }
    }

    fn note_on(&mut self, host: HostNote, velocity: f64, tune: f64) {
        let Some(note) = self.notes.insert(Note::new(host, velocity, tune)) else {
            self.stats.notes_refused += 1;
            return;
        };
        let Some(n) = self.notes.get(note).copied() else {
            return;
        };
        let velocity_byte = (velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u8;
        let (plan, rate, now) = (self.plan.clone(), self.rate, self.now);
        for (index, zone) in plan.zones.iter().enumerate() {
            if !zone.keys.contains(&host.key) || !zone.velocities.contains(&velocity_byte) {
                continue;
            }
            let Some(slot) = self.free_slot() else {
                continue;
            };
            slot.voice.start(VoiceStart {
                region: zone.region.clone(),
                step: step(zone, &n, rate),
                gain: gain(zone, &n),
                envelope: zone.envelope,
            });
            slot.note = Some(note);
            slot.zone = index;
            slot.started = now;
            slot.released = false;
        }
    }

    /// A free voice, else the oldest released one, else the oldest (stolen).
    fn free_slot(&mut self) -> Option<&mut Slot> {
        let index = match self.voices.iter().position(|s| !s.voice.is_active()) {
            Some(i) => i,
            None => {
                let (i, _) = self
                    .voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, s)| (!s.released, s.started))?;
                self.stats.voices_stolen += 1;
                i
            }
        };
        let slot = self.voices.get_mut(index)?;
        slot.voice.kill();
        Some(slot)
    }

    fn note_off(&mut self, pattern: HostPattern) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            if self
                .pedal
                .get(usize::from(n.host.channel))
                .copied()
                .unwrap_or(false)
            {
                n.pedal_held = true;
            } else {
                self.release(note);
            }
        }
        self.hits = hits;
    }

    fn release(&mut self, note: NoteRef) {
        if let Some(n) = self.notes.get_mut(note) {
            n.released = true;
            n.pedal_held = false;
        }
        for slot in self
            .voices
            .iter_mut()
            .filter(|s| s.note == Some(note) && !s.released)
        {
            slot.voice.release();
            slot.released = true;
        }
    }

    fn choke(&mut self, pattern: HostPattern) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            for slot in self.voices.iter_mut().filter(|s| s.note == Some(note)) {
                slot.voice.kill();
            }
        }
        self.hits = hits;
    }

    fn expression(&mut self, pattern: HostPattern, expression: NoteExpression) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.matching(pattern, &mut hits);
        for &note in &hits {
            let Some(n) = self.notes.get_mut(note) else {
                continue;
            };
            match expression {
                NoteExpression::Tune(semitones) => n.expression_tune = semitones,
                NoteExpression::Gain(g) => n.expression_gain = g as f32,
                NoteExpression::Pan(p) => n.expression_pan = p as f32,
                // ponytail: pressure and brightness reach the modulation matrix once it exists.
                NoteExpression::Pressure(_) | NoteExpression::Brightness(_) => continue,
            }
            let n = *n;
            for slot in self.voices.iter_mut().filter(|s| s.note == Some(note)) {
                if let Some(zone) = self.plan.zones.get(slot.zone) {
                    slot.voice.set_step(step(zone, &n, self.rate));
                    slot.voice.set_gain(gain(zone, &n));
                }
            }
        }
        self.hits = hits;
    }

    /// MIDI 1.0 channel messages the engine itself acts on. Notes arrive as
    /// [`Event::NoteOn`]; other controllers feed scripts and modulation.
    fn ump(&mut self, [word, _]: [u32; 2]) {
        if word >> 28 != 0x2 {
            return;
        }
        let status = (word >> 16) as u8;
        let channel = status & 0x0F;
        let (data1, data2) = ((word >> 8) as u8 & 0x7F, word as u8 & 0x7F);
        if status & 0xF0 == 0xB0 && data1 == 64 {
            let down = data2 >= 64;
            if let Some(p) = self.pedal.get_mut(usize::from(channel)) {
                *p = down;
            }
            if !down {
                let mut hits = std::mem::take(&mut self.hits);
                self.notes
                    .select(&mut hits, |n| n.pedal_held && n.host.channel == channel);
                for &note in &hits {
                    self.release(note);
                }
                self.hits = hits;
            }
        }
    }

    /// Ends notes with no sounding voice and reports them to the host.
    fn end_silent_notes(&mut self) {
        let mut hits = std::mem::take(&mut self.hits);
        self.notes.select(&mut hits, |_| true);
        for &note in &hits {
            if !self
                .voices
                .iter()
                .any(|s| s.note == Some(note) && s.voice.is_active())
                && let Some(n) = self.notes.remove(note)
            {
                self.ended.push(n.host);
            }
        }
        self.hits = hits;
        for slot in self.voices.iter_mut().filter(|s| !s.voice.is_active()) {
            slot.note = None;
        }
    }
}

fn step(zone: &Zone, note: &Note, rate: u32) -> f64 {
    let semitones = f64::from(note.host.key) - f64::from(zone.root)
        + zone.tune
        + note.tune
        + note.expression_tune;
    2f64.powf(semitones / 12.0) * f64::from(zone.sample_rate) / f64::from(rate)
}

// ponytail: linear-in-dB velocity and a balance pan law; Kontakt's curves come
// from the law table once a measurement shows they differ audibly.
fn gain(zone: &Zone, note: &Note) -> [f32; 2] {
    let db = -zone.velocity_db * (1.0 - note.velocity as f32);
    let g = zone.gain * 10f32.powf(db / 20.0) * note.expression_gain;
    let pan = (zone.pan + note.expression_pan).clamp(-1.0, 1.0);
    [g * (1.0 - pan).min(1.0), g * (1.0 + pan).min(1.0)]
}
