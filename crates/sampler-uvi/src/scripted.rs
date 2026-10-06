//! Runs a program's Lua scripts against the core runtime.
//!
//! The physical note is admitted but silent (`Runtime::note_on` selects no
//! source). Each `playNote` the script makes becomes a core child of it whose
//! group selection allows only the (layer, oscillator) groups named, then its
//! attack is forwarded: the same ownership path the other frontends use, with
//! no parallel voice mechanism. The script host only runs when a note event or
//! a `wait` is due, so an idle program costs nothing per block.
use crate::script::{Command, Play, ScriptHost};
use crate::OscGroup;
use sampler_core::{
    Error, Expression, Frame, Inheritance, Input, Limits, NoteId, Prepared, Protocol, Runtime,
};
use std::collections::HashMap;

/// A translated program whose scripts are loaded.
pub struct Program {
    pub instrument: sampler_ir::Instrument,
    pub plan: Prepared,
    pub host: ScriptHost,
    pub groups: Vec<OscGroup>,
}

/// Plays made once their originating note was released sound at most this long.
const DETACHED_MS: f64 = 5000.0;
/// Script wake-ups handled at one instant before time is forced forward.
const SPIN: usize = 64;

pub struct Player {
    rt: Runtime,
    host: ScriptHost,
    groups: Vec<OscGroup>,
    rate: f64,
    /// Script event / voice id -> core note.
    notes: HashMap<u64, NoteId>,
    /// Sounding physical notes by key (script event id).
    held: HashMap<u8, u64>,
    next: u64,
    /// Plays that asked for something the runtime does not model.
    unmodeled: Vec<&'static str>,
}

impl Player {
    pub fn new(program: Program, limits: Limits, rate: u32) -> Result<Self, Error> {
        Ok(Self {
            rt: Runtime::new(program.plan, limits)?,
            host: program.host,
            groups: program.groups,
            rate: f64::from(rate),
            notes: HashMap::new(),
            held: HashMap::new(),
            next: 1,
            unmodeled: Vec::new(),
        })
    }

    pub fn runtime(&self) -> &Runtime {
        &self.rt
    }

    /// What plays asked for that was ignored, once each.
    pub fn unmodeled(&self) -> &[&'static str] {
        &self.unmodeled
    }

    fn now_ms(&self) -> f64 {
        self.rt.now() as f64 * 1000.0 / self.rate
    }

    fn frames(&self, ms: f64) -> u64 {
        (ms * self.rate / 1000.0).ceil().max(0.0) as u64
    }

    pub fn note_on(&mut self, key: u8, velocity: f64) -> Result<(), Error> {
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key,
            external_id: None,
        };
        let note = self.rt.note_on(input, key, velocity)?;
        let id = self.next;
        self.next += 1;
        self.notes.insert(id, note);
        self.held.insert(key, id);
        self.host.set_time(self.now_ms());
        self.host
            .note_on(id, key, (velocity * 127.0).round() as u8, 0);
        if !self.host.handles_notes() {
            // No onNote: the program sounds as authored.
            self.rt.forward_attack(note)?;
        }
        self.apply(false)
    }

    pub fn note_off(&mut self, key: u8) -> Result<(), Error> {
        let Some(id) = self.held.remove(&key) else {
            return Ok(());
        };
        self.host.set_time(self.now_ms());
        self.host.note_off(id, key, 64, 0);
        // Plays made by onRelease must not be linked to the closing gate.
        self.apply(true)?;
        if let Some(note) = self.notes.get(&id).copied() {
            self.rt.key_up(note, None)?;
        }
        Ok(())
    }

    /// Render `out`, waking the script exactly when it asked to run.
    pub fn render(&mut self, out: &mut [Frame]) -> Result<(), Error> {
        let mut done = 0;
        let mut spins = 0;
        while done < out.len() {
            let left = out.len() - done;
            let due = self
                .host
                .next_due()
                .map(|ms| self.frames(ms).saturating_sub(self.rt.now()));
            match due {
                Some(0) if spins < SPIN => {
                    spins += 1;
                    self.host.advance(self.now_ms());
                    self.apply(false)?;
                    continue;
                }
                _ => {}
            }
            spins = 0;
            let step = due.map_or(left, |d| (d.max(1) as usize).min(left));
            self.rt.render(&mut out[done..done + step])?;
            done += step;
        }
        Ok(())
    }

    fn apply(&mut self, closing: bool) -> Result<(), Error> {
        for command in self.host.take_commands() {
            match command {
                Command::Play(play) => self.play(&play, closing)?,
                Command::Release { id, at_ms } => {
                    if let Some(note) = self.notes.get(&id).copied() {
                        self.release(note, at_ms)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn release(&mut self, note: NoteId, at_ms: f64) -> Result<(), Error> {
        let at = self.frames(at_ms);
        if at <= self.rt.now() {
            self.rt.key_up(note, None)
        } else {
            self.rt.release_at(note, at)
        }
    }

    fn play(&mut self, play: &Play, closing: bool) -> Result<(), Error> {
        let velocity = f64::from(play.velocity) / 127.0;
        let parent = play.parent.and_then(|p| self.notes.get(&p).copied());
        let open = parent.is_some() && !closing && play.duration_ms.is_none();
        let note = match parent {
            Some(parent) => {
                self.rt
                    .child(parent, play.key, velocity, open, Inheritance::Independent)
            }
            None => {
                let input = Input {
                    protocol: Protocol::Native,
                    port: 0,
                    group: 0,
                    channel: 0,
                    key: play.key,
                    external_id: None,
                };
                self.rt.note_on(input, play.key, velocity)
            }
        };
        let note = match note {
            Ok(note) => note,
            // The originating note ended meanwhile: nothing to attach to.
            Err(Error::ClosedNote | Error::StaleHandle) => return Ok(()),
            Err(e) => return Err(e),
        };
        self.notes.insert(play.id, note);
        self.select(note, play)?;
        let expression = Expression {
            gain: play.vol.clamp(0.0, 4.0),
            pan: play.pan.clamp(-1.0, 1.0),
            pitch_semitones: play.tune,
            ..Expression::default()
        };
        if expression != Expression::default() {
            let id = self.rt.expression_id(note)?;
            self.rt.set_expression(id, expression)?;
        }
        self.rt.forward_attack(note)?;
        let now = self.now_ms();
        match play.duration_ms {
            Some(ms) => self.release(note, now + ms)?,
            None if parent.is_none() || closing => self.release(note, now + DETACHED_MS)?,
            None => {}
        }
        Ok(())
    }

    /// Allow only the groups of the layers and oscillator the play names.
    fn select(&mut self, note: NoteId, play: &Play) -> Result<(), Error> {
        if play.layers.is_empty() && play.osc.is_none() {
            return Ok(());
        }
        self.rt.set_note_group(note, None, false)?;
        let wanted: Vec<u32> = self
            .groups
            .iter()
            .filter(|g| play.layers.is_empty() || play.layers.contains(&g.layer))
            .filter(|g| play.osc.is_none_or(|o| o == g.osc))
            .map(|g| g.group)
            .collect();
        for group in wanted {
            self.rt.set_note_group(note, Some(group), true)?;
        }
        Ok(())
    }
}
