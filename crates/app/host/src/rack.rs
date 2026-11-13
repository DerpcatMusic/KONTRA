//! The rack (07-APP.md §4): up to [`MAX_PARTS`] parts, each one loaded
//! instrument and the engine playing it. [`Rack`] lives on the control
//! thread and builds engines there; [`RackAudio`] lives on the audio thread
//! and only renders. They share nothing but the protocol's rings and cells,
//! so no lock is reachable from audio.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kontra_engine::{EffectRequest, Engine, Prepared};

use crate::persist::Capture;
use crate::view::Interfaces;
pub(crate) use files::DiskFiles;
pub use files::user_saves;
pub use kontra_engine::{Event, HostNote, HostPattern, NoteExpression, Plan, ir};
use kontra_protocol::{MAX_PARTS, MeterCells, Meters, Ring, StatCells, Stats};
pub use kontra_stream::{Store, StoreConfig};
use kontra_ui_model::UiModifiers;
pub use picks::Pick;

mod audio;
mod command;
mod files;
mod hold;
mod named;
mod native;
mod picks;
mod recall;
mod requests;
pub use audio::RackAudio;
pub use command::Command;
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

/// The channels between [`Rack`] and [`RackAudio`].
pub struct Channels {
    pub commands: Ring<Command>,
    pub retired: Ring<Box<Engine>>,
    pub captured: Ring<Box<Capture>>,
    /// Parts' engines lent for a recall ([`Command::Recall`]).
    pub recalled: Ring<Box<command::Recalling>>,
    /// Effect types the parts' scripts asked for: (part, its load, request).
    pub requests: Ring<(usize, u64, EffectRequest)>,
    /// Array files the parts' scripts asked for: (part, its load, request).
    pub files: Ring<(usize, u64, kontra_engine::files::FileRequest)>,
    /// Answered file requests, by part and load: a save's values to write,
    /// the rest to free.
    pub filed: Ring<(usize, u64, Box<kontra_engine::files::FileReply>)>,
    /// Zone edits scripts made, and plans swapped out to free here.
    pub zones: requests::ZoneRings,
    /// Effect units swapped out, to free on the control side.
    pub units: Ring<Box<Option<Prepared>>>,
    /// Sound edits' emptied boxes ([`Command::Sound`]), to free here.
    pub sounds: Ring<Box<Option<(usize, ir::Effect)>>>,
    /// Typed texts the audio side wrote ([`Command::ControlText`]), to free here.
    pub typed: Ring<Box<kontra_engine::Text>>,
    /// Interface reads filled on the audio side ([`Command::Look`]).
    pub looks: Ring<Box<crate::view::LookFrame>>,
    pub meters: MeterCells,
    pub stats: StatCells,
}

/// A loaded part as the control thread knows it.
pub struct Part {
    /// The instrument file, or None for one built in memory.
    pub path: Option<PathBuf>,
    /// The instrument file its IR was read from (a snapshot's base
    /// instrument): a preset of the same source recalls into this part.
    pub source: Option<PathBuf>,
    /// What a recall does to its scripts, as they asked in `on init`.
    pub recall: kontra_engine::Recall,
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
    /// The player changed one of its library's controls since its preset
    /// loaded: what plays is no longer the preset as saved.
    pub modified: bool,
    /// What its script named lately ([`Rack::take_named`]).
    pub named: named::Named,
}

/// What the rack keeps to save a part's scripts.
#[derive(Default)]
pub struct ScriptState {
    /// By script slot, its persistent variables.
    pub vars: Vec<crate::persist::Vars>,
    /// By instrument script, the saved values it loaded with.
    pub loaded: Vec<Vec<String>>,
    /// By instrument script, its script slot ([`ir::Script::slot`]).
    pub slots: Vec<usize>,
    generation: u64,
    latest: Option<Box<Capture>>,
    /// The capture before `latest`, sent again: its room is reused.
    spare: Option<Box<Capture>>,
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
    /// Where scripts' array saves go ([`files`]); none writes nothing.
    saves: Option<PathBuf>,
    /// Script file requests waiting for the player to pick ([`picks`]).
    picks: std::collections::VecDeque<picks::Parked>,
    /// Whether anyone looks at the parts' interfaces.
    interfaces: Interfaces,
    /// Snapshot recalls whose plans are being made ([`native`]).
    native: Vec<native::Native>,
    /// The instruments the player has, by the catalog generation they are
    /// of: a snapshot kept apart from its library binds to one by name.
    known: (u64, Vec<PathBuf>),
}

impl Rack {
    /// A rack rendering at `rate`, and the audio side that plays it.
    #[must_use]
    pub fn new(rate: u32, store: Store, max_block: usize) -> (Self, RackAudio) {
        let channels = Arc::new(Channels {
            commands: Ring::new(256),
            retired: Ring::new(64),
            captured: Ring::new(2 * MAX_PARTS),
            recalled: Ring::new(2 * MAX_PARTS),
            // An engine queues at most 16 at once.
            requests: Ring::new(16 * MAX_PARTS),
            units: Ring::new(16 * MAX_PARTS),
            sounds: Ring::new(256),
            zones: requests::ZoneRings::new(MAX_PARTS),
            files: Ring::new(16 * MAX_PARTS),
            filed: Ring::new(16 * MAX_PARTS),
            looks: Ring::new(2 * MAX_PARTS),
            typed: Ring::new(16),
            meters: MeterCells::default(),
            stats: StatCells::default(),
        });
        let audio = RackAudio::new(Arc::clone(&channels), store.client(), rate, max_block);
        let rack = Self {
            channels,
            rate,
            store,
            parts: (0..MAX_PARTS).map(|_| None).collect(),
            saves: None,
            picks: std::collections::VecDeque::new(),
            pending: std::collections::VecDeque::new(),
            interfaces: Interfaces::Shown,
            generation: 0,
            native: Vec::new(),
            known: (0, Vec::new()),
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
        if let Some(progress) = self.store.progress() {
            progress.begin();
        }
        // A preset of the instrument the part plays recalls into it,
        // without reading the instrument.
        if let Some(done) = self.load_preset(part, path) {
            return done;
        }
        let instrument = crate::instrument::read_saved(path, &[], &self.known.1)?;
        self.load_ir(part, instrument, Some(path.to_owned()))
    }

    /// Takes the instruments of `browser`'s catalog as the ones a snapshot
    /// may bind to ([`crate::instrument::preset`]), when it changed.
    pub fn know(&mut self, browser: &crate::browser::Browser) {
        if self.known.0 != browser.generation {
            self.known = (browser.generation, browser.instruments());
        }
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
        if let Some(progress) = self.store.progress() {
            progress.begin();
        }
        let mut instrument = crate::instrument::read_saved(path, scripts, &self.known.1)?;
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
        let mut instrument = instrument;
        // The files its scripts saved come back as the bank's own.
        let own = picks::script_files(self.saves.as_deref(), instrument.source_path.as_deref());
        for s in (instrument.scripts.iter_mut()).filter(|s| s.dialect == ir::Dialect::Lua) {
            s.data.extend(own.iter().cloned());
        }
        let saved = instrument.scripts.iter().map(|s| s.saved.clone()).collect();
        let source = instrument.source_path.clone();
        let slots = instrument.scripts.iter().map(|s| s.slot).collect();
        let waves = crate::ui::Waves::of(&instrument);
        let mut loaded = crate::instrument::build(instrument, self.rate, &mut self.store);
        // A store set to defer heads read them here, behind the part.
        if let Some(heads) = self.store.take_heads() {
            let lanes = std::thread::available_parallelism().map_or(4, |n| n.get() * 3);
            // A failed spawn leaves the heads to stream on demand.
            let _ = std::thread::Builder::new()
                .name("kontra-heads".into())
                .spawn(move || heads.load(lanes));
        }
        if let Some(progress) = self.store.progress() {
            progress.stage(kontra_protocol::LoadStage::Interface);
        }
        let saves = self.saves.clone();
        for s in loaded.plan.scripts.iter_mut().filter(|s| s.files.is_some()) {
            s.files = Some(Arc::new(DiskFiles {
                saves: saves.clone(),
            }));
        }
        let (slot_types, slot_responses) =
            recall::put_slots(&mut loaded.plan, slot_types, slot_responses, self.rate);
        let plan = Arc::new(loaded.plan);
        let mut engine = Engine::new(plan, self.store.client(), self.rate);
        requests::init_tables(&mut engine, &self.store.client());
        // Its library's resources are found near the instrument file.
        let near = source.as_deref().or(path.as_deref());
        let view = crate::view::View::of(&loaded.faces, &loaded.pages, &engine, near)
            .map(|view| view.with_waves(waves));
        let sounds = crate::sound::Sounds::new(self.rate, engine.group_sounds());
        let part_info = Part {
            source,
            recall: engine.recall_law(),
            view,
            sounds,
            slot_types,
            slot_responses,
            modified: false,
            named: named::Named::default(),
            // With the zone edits its `on init` made.
            plan: Some(Arc::clone(engine.plan())),
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
                slots,
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
    pub fn control(
        &mut self,
        part: usize,
        widget: usize,
        cell: Option<u32>,
        value: f64,
        keys: UiModifiers,
    ) {
        let Some(Some(p)) = self.parts.get_mut(part) else {
            return;
        };
        let Some(view) = p.view.as_mut() else {
            return;
        };
        p.named.touch();
        if let Some(set) = view.tap(widget, value) {
            let vars = &p.script.vars;
            p.modified |= set.iter().any(|&(slot, id, _)| saves(vars, slot, id));
            for (slot, id, value) in set {
                let cell = 0;
                self.send(Command::Control {
                    part,
                    slot,
                    id,
                    cell,
                    value,
                    keys,
                });
            }
        } else if let Some((slot, n)) = view.widget(widget) {
            p.modified = true;
            self.send(Command::Widget {
                part,
                slot,
                n,
                value,
                cell,
            });
        } else if let Some((slot, id, value)) = view.control(widget, value) {
            p.modified |= saves(&p.script.vars, slot, id);
            self.send(Command::Control {
                part,
                slot,
                id,
                cell: cell.unwrap_or(0),
                value,
                keys,
            });
        }
    }

    /// The user typed `text` into text edit `widget` of `part`'s interface:
    /// its variable reads it, then its callbacks run.
    pub fn control_text(&mut self, part: usize, widget: usize, text: &str) {
        let Some(Some(p)) = self.parts.get_mut(part) else {
            return;
        };
        p.modified = true;
        p.named.touch();
        let Some(view) = p.view.as_ref() else {
            return;
        };
        let Some((slot, id, _)) = view.control(widget, 0.) else {
            return;
        };
        let name = view.tree().widgets.get(widget).map(|w| w.name.as_str());
        let face = p.faces.get(usize::from(slot)).and_then(Option::as_ref);
        let Some(cell) = face.zip(name).and_then(|(f, name)| f.text_cell(name)) else {
            return;
        };
        let text = Box::new(kontra_engine::Text::from_str_cut(text).0);
        self.send(Command::ControlText {
            part,
            slot,
            id,
            cell,
            text,
        });
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
                insert: Box::new(insert),
            });
        }
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
            cell: 0,
            value,
            keys: UiModifiers::default(),
        });
    }

    /// Puts scripts' array saves under `root` (the plugin's is
    /// [`user_saves`]); a later load of a saved path reads it first.
    pub fn set_saves(&mut self, root: PathBuf) {
        self.saves = Some(root);
    }

    /// Whether the next ticks keep the parts' interfaces current.
    pub fn show_interfaces(&mut self, interfaces: Interfaces) {
        self.interfaces = interfaces;
    }

    /// Frees what the audio thread let go of and resends refused commands.
    /// Call it from the control thread now and then.
    pub fn tick(&mut self) {
        while self.channels.retired.recv().is_some() {}
        while self.channels.units.recv().is_some() {}
        while self.channels.sounds.recv().is_some() {}
        while self.channels.typed.recv().is_some() {}
        while let Some(recalling) = self.channels.recalled.recv() {
            self.recalled(*recalling);
        }
        self.files();
        // Effect types and zone edits scripts asked for, built here.
        self.native_ready();
        self.script_requests();
        // The interfaces catch up with what their scripts changed, and ask
        // again: the editor shows each at most one tick late.
        while let Some(frame) = self.channels.looks.recv() {
            let Some(Some(p)) = self.parts.get_mut(frame.part) else {
                continue;
            };
            let Some(view) = p.view.as_mut() else {
                continue;
            };
            if p.script.generation == frame.generation {
                view.refresh(&p.faces, p.path.as_deref(), frame);
                p.named.look(view.tree());
            } else if view.frame.is_none() {
                // A recall kept this view and moved its generation on: the
                // frame goes back unread, or the view could never ask again.
                view.frame = Some(frame);
            }
        }
        let looked = match self.interfaces {
            Interfaces::Shown => self.parts.len(),
            Interfaces::Hidden => 0,
        };
        for n in 0..looked {
            let Some(Some(p)) = self.parts.get_mut(n) else {
                continue;
            };
            let Some(mut frame) = p.view.as_mut().and_then(|v| v.frame.take()) else {
                continue;
            };
            (frame.part, frame.generation) = (n, p.script.generation);
            self.send(Command::Look(frame));
        }
        while let Some(mut c) = self.channels.captured.recv() {
            if let Some(Some(p)) = self.parts.get_mut(c.part)
                && p.script.generation == c.generation
            {
                let files = c.take_files();
                let source = p.source.as_deref();
                let failed = picks::write_script_files(self.saves.as_deref(), source, files);
                p.diagnostics
                    .extend(failed.into_iter().map(|detail| ir::Diagnostic {
                        impact: ir::Impact::Cosmetic,
                        feature: "preset".into(),
                        detail,
                    }));
                p.script.asked = false;
                crate::persist::landed(c, &mut p.script.latest, &mut p.script.spare);
            }
        }
        // Ask again for every part with persistent values: what a save
        // writes is at most one tick old.
        for n in 0..self.parts.len() {
            let Some(Some(p)) = self.parts.get_mut(n) else {
                continue;
            };
            let s = &mut p.script;
            let nothing =
                |v: &crate::persist::Vars| v.vars.is_empty() && v.widgets.is_empty() && !v.state;
            if s.asked || s.vars.iter().all(nothing) {
                continue;
            }
            s.asked = true;
            let spare = s.spare.take().filter(|c| c.generation == s.generation);
            let mut c = spare.unwrap_or_else(|| Capture::new(n, s.generation, &s.vars));
            c.after(s.latest.as_deref());
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

/// A preset of the part saves control `id` of script `slot`: its value is
/// persistent, and not the instrument's alone (a page tab, a view's
/// settings), so moving it changes what plays as that preset.
fn saves(vars: &[crate::persist::Vars], slot: u8, id: i32) -> bool {
    let of = vars.get(usize::from(slot)).map(|v| v.vars.as_slice());
    of.unwrap_or_default()
        .iter()
        .any(|v| v.control == Some(id) && !v.instrument)
}

#[cfg(test)]
mod tests {
    use crate::persist::{Var, Vars};

    #[test]
    fn only_a_control_a_snapshot_saves_marks_the_preset_changed() {
        let var = |control, instrument| Var {
            name: "$x".into(),
            real: false,
            text: false,
            base: 0,
            len: 1,
            control: Some(control),
            instrument,
        };
        let vars = [Vars {
            vars: vec![var(1, false), var(2, true)],
            ..Vars::default()
        }];
        assert!(super::saves(&vars, 0, 1));
        assert!(!super::saves(&vars, 0, 2), "a page tab");
        assert!(!super::saves(&vars, 0, 3), "not persistent");
        assert!(!super::saves(&vars, 1, 1), "no such script");
    }
}
