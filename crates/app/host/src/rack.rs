//! The rack (07-APP.md §4): up to [`MAX_PARTS`] parts, each one loaded
//! instrument and the engine playing it. [`Rack`] lives on the control
//! thread and builds engines there; [`RackAudio`] lives on the audio thread
//! and only renders. They share nothing but the protocol's rings and cells,
//! so no lock is reachable from audio.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::{EffectRequest, Engine, Prepared, ir};

use crate::persist::Capture;
pub use kontra_engine::{Event, HostNote, HostPattern, NoteExpression, Plan};
use kontra_protocol::{MAX_PARTS, MeterCells, Meters, Ring, StatCells, Stats};
pub use kontra_stream::{Store, StoreConfig};
use kontra_ui_model::UiModifiers;

mod audio;
mod hold;
pub use audio::RackAudio;
pub use hold::{HOLD_STEPS, Hold};

/// A part's attack delays as auto-align measured them, by velocity range
/// in rising order: its top velocity, then how late a first note and a
/// legato sound, in ms after the note-on (`None` where nothing was measured).
pub type Delays = Vec<(u8, Option<f32>, Option<f32>)>;

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
        /// Which load it is: effect units built for another never go in.
        generation: u64,
    },
    /// Put the effect unit a script asked for into `part`'s slot, if the
    /// part is still the load that asked; the box comes back holding what
    /// it replaced, so audio frees nothing.
    Effect {
        part: usize,
        generation: u64,
        unit: Box<Option<Prepared>>,
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
    /// How long each note into `part` waits: auto-align.
    Hold(usize, Hold),
    /// Read a part's persistent script cells and send them back.
    Capture(Box<Capture>),
    /// Copy what a part's interface reads of its script and send it back.
    Look(Box<crate::view::LookFrame>),
    /// The user moved control `id` of script `slot` to `value`, holding
    /// `keys` at the click.
    Control {
        part: usize,
        slot: u8,
        id: i32,
        value: i64,
        keys: UiModifiers,
    },
    /// The user moved widget `n` of script `slot`'s own page to `value`.
    Widget {
        part: usize,
        slot: u8,
        n: u32,
        value: f64,
    },
    /// The user edited a group's sound (the Sound tab).
    Sound {
        part: usize,
        group: usize,
        envelope: kontra_engine::Ahdsr,
        insert: Option<(usize, ir::Effect)>,
    },
}

/// The channels between [`Rack`] and [`RackAudio`].
pub struct Channels {
    pub commands: Ring<Command>,
    pub retired: Ring<Box<Engine>>,
    pub captured: Ring<Box<Capture>>,
    /// Effect types the parts' scripts asked for: (part, its load, request).
    pub requests: Ring<(usize, u64, EffectRequest)>,
    /// Effect units swapped out, to free on the control side.
    pub units: Ring<Box<Option<Prepared>>>,
    /// Interface reads filled on the audio side ([`Command::Look`]).
    pub looks: Ring<Box<crate::view::LookFrame>>,
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
    /// How late its notes sound, once auto-align measured it.
    pub delays: Option<Delays>,
    /// Its library's interface, as `on init` left it.
    pub view: Option<crate::view::View>,
    /// Each group's envelope and filters, as the editor edits them.
    pub sounds: crate::sound::Sounds,
    /// Effect types its scripts loaded into slots after `on init`, as
    /// (rack, slot, type); a reload puts them back before `on init`.
    pub slot_types: Vec<(ir::RackAt, u8, u16)>,
    /// Library responses its scripts loaded into convolutions after
    /// `on init`, as (rack, slot, name); put back after the types.
    pub slot_responses: Vec<(ir::RackAt, u8, String)>,
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
            // An engine queues at most 16 at once.
            requests: Ring::new(16 * MAX_PARTS),
            units: Ring::new(16 * MAX_PARTS),
            looks: Ring::new(2 * MAX_PARTS),
            meters: MeterCells::default(),
            stats: StatCells::default(),
        });
        let audio = RackAudio::new(Arc::clone(&channels), store.client(), rate, max_block);
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

    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
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
    /// by `scripts` (as [`Rack::saved_scripts`] gave them) and the effect
    /// types and responses its scripts loaded put back in their slots before
    /// `on init`.
    ///
    /// # Errors
    /// As [`Rack::load`].
    pub fn load_saved(
        &mut self,
        part: usize,
        path: &Path,
        scripts: &[Vec<String>],
        slot_types: &[(ir::RackAt, u8, u16)],
        slot_responses: &[(ir::RackAt, u8, String)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut instrument = crate::instrument::read_saved(path, scripts)?;
        for (script, saved) in instrument.scripts.iter_mut().zip(scripts) {
            script.saved.clone_from(saved);
        }
        let path = Some(path.to_owned());
        self.load_built(part, instrument, path, slot_types, slot_responses)
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
        self.load_built(part, instrument, path, &[], &[])
    }

    fn load_built(
        &mut self,
        part: usize,
        instrument: ir::Instrument,
        path: Option<PathBuf>,
        slot_types: &[(ir::RackAt, u8, u16)],
        slot_responses: &[(ir::RackAt, u8, String)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if part >= MAX_PARTS {
            return Err(format!("part {part} is past the rack's {MAX_PARTS}").into());
        }
        let saved = instrument.scripts.iter().map(|s| s.saved.clone()).collect();
        let mut loaded = crate::instrument::build(instrument, self.rate, &mut self.store);
        let slot_types: Vec<_> = (slot_types.iter().copied())
            .filter(|&(at, slot, ty)| loaded.plan.load_type(at, slot, ty))
            .collect();
        let slot_responses: Vec<_> = (slot_responses.iter())
            .filter(|(at, slot, name)| loaded.plan.load_response(*at, *slot, name, self.rate))
            .cloned()
            .collect();
        let plan = Arc::new(loaded.plan);
        let engine = Engine::new(Arc::clone(&plan), self.store.client(), self.rate);
        let view = crate::view::View::of(&loaded.faces, &loaded.pages, &engine, path.as_deref());
        let sounds = crate::sound::Sounds::new(self.rate, engine.group_sounds());
        let part_info = Part {
            view,
            sounds,
            slot_types,
            slot_responses,
            plan: Some(plan),
            delays: None,
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
        let generation = self.generation;
        self.send(Command::Install {
            part,
            engine,
            mix: Mix {
                silent: self.silent(part),
                ..mix
            },
            generation,
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
    pub fn control(&mut self, part: usize, widget: usize, value: f64, keys: UiModifiers) {
        let Some(Some(p)) = self.parts.get(part) else {
            return;
        };
        let Some(view) = p.view.as_ref() else {
            return;
        };
        if let Some((slot, n)) = view.widget(widget) {
            self.send(Command::Widget {
                part,
                slot,
                n,
                value,
            });
        } else if let Some((slot, id, value)) = view.control(widget, value) {
            self.send(Command::Control {
                part,
                slot,
                id,
                value,
                keys,
            });
        }
    }

    /// The user set `param` of `part`'s group `group` (every group when
    /// `None`) to `value`, in the Sound tab's units: the next notes play it.
    pub fn set_sound(
        &mut self,
        part: usize,
        group: Option<usize>,
        param: crate::sound::Param,
        value: f64,
    ) {
        let Some(Some(p)) = self.parts.get_mut(part) else {
            return;
        };
        let groups = group.map_or(0..p.sounds.len(), |g| g..g + 1);
        let edits: Vec<_> = groups
            .filter_map(|g| Some((g, p.sounds.set(g, param, value)?)))
            .collect();
        for (group, (envelope, insert)) in edits {
            self.send(Command::Sound {
                part,
                group,
                envelope,
                insert,
            });
        }
    }

    /// Records `part`'s measured attack delay (auto-align), saved with it.
    pub fn set_delays(&mut self, part: usize, delays: Delays) {
        if let Some(Some(p)) = self.parts.get_mut(part) {
            p.delays = Some(delays);
        }
    }

    /// Holds each note into `part` by its `hold` from the next block on,
    /// so it sounds in time with a later part (auto-align).
    pub fn set_hold(&mut self, part: usize, hold: Hold) {
        self.send(Command::Hold(part, hold));
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
            keys: UiModifiers::default(),
        });
    }

    /// Frees what the audio thread let go of and resends refused commands.
    /// Call it from the control thread now and then.
    pub fn tick(&mut self) {
        while self.channels.retired.recv().is_some() {}
        while self.channels.units.recv().is_some() {}
        // Effect types scripts loaded: built here, off the audio thread.
        while let Some((part, generation, request)) = self.channels.requests.recv() {
            let Some(Some(p)) = self.parts.get_mut(part) else {
                continue;
            };
            if p.script.generation != generation {
                continue;
            }
            let here = (request.at, request.slot);
            let (asked, ty) = (request.response.is_some(), request.ty);
            let unit = Prepared::new(request, p.plan.as_deref(), self.rate);
            // A response that fails to decode leaves the slot playing what it
            // had, so the save keeps what it had too.
            if let Some(name) = unit.response_name() {
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_responses.push((here.0, here.1, name.to_owned()));
            } else if let Some(ty) = ty.filter(|_| !asked) {
                // None: the slot's own effect built again (a reverb's
                // network), which saves no type.
                p.slot_types.retain(|&(at, slot, _)| (at, slot) != here);
                p.slot_types.push((here.0, here.1, ty));
                p.slot_responses.retain(|&(at, slot, _)| (at, slot) != here);
            }
            let unit = Box::new(Some(unit));
            self.send(Command::Effect {
                part,
                generation,
                unit,
            });
        }
        // The interfaces catch up with what their scripts changed, and ask
        // again: the editor shows each at most one tick late.
        while let Some(frame) = self.channels.looks.recv() {
            if let Some(Some(p)) = self.parts.get_mut(frame.part)
                && p.script.generation == frame.generation
                && let Some(view) = p.view.as_mut()
            {
                view.refresh(&p.faces, p.path.as_deref(), frame);
            }
        }
        let looks: Vec<_> = (self.parts.iter_mut().enumerate())
            .filter_map(|(n, p)| {
                let p = p.as_mut()?;
                let mut frame = p.view.as_mut()?.frame.take()?;
                (frame.part, frame.generation) = (n, p.script.generation);
                Some(frame)
            })
            .collect();
        for frame in looks {
            self.send(Command::Look(frame));
        }
        while let Some(c) = self.channels.captured.recv() {
            if let Some(Some(p)) = self.parts.get_mut(c.part)
                && p.script.generation == c.generation
            {
                p.script.asked = false;
                let mut c = c;
                c.keep(p.script.latest.take());
                p.script.latest = Some(c);
            }
        }
        // Ask again for every part with persistent values: what a save
        // writes is at most one tick old.
        let ask: Vec<_> = (self.parts.iter_mut().enumerate())
            .filter_map(|(n, p)| {
                let s = &mut p.as_mut()?.script;
                if s.asked
                    || s.vars
                        .iter()
                        .all(|v| v.vars.is_empty() && v.widgets.is_empty() && !v.state)
                {
                    return None;
                }
                s.asked = true;
                let mut c = Capture::new(n, s.generation, &s.vars);
                c.after(s.latest.as_deref());
                Some(c)
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
