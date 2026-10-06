//! [`Core`] over the existing engine: the rack of [`Engine`]s, their
//! articulation routers and the cross-part alignment scheduler.
//!
//! The shell's v1-only services (IR, zone-map and array jobs, sample heads,
//! sound-editor overrides and probes) are still reached through
//! [`V1Core::rack`] directly; they have no v2 counterpart yet.

use super::event::{HostNote, In};
use super::view::{Persisted, Refresh};
use super::{BlockInfo, Core, CoreError, CoreLoader, Description, LoadRequest, Progress, Rendered, Transport, Voices};
use crate::articulate::{self, Router};
use crate::engine::{Bank, Engine, Heads, Rack};
use crate::fx::FxProcessor;
use crate::ksp::{Live, Runtime};
use crate::timing::Align;

/// The v1 engine rack behind the seam.
pub struct V1Core {
    pub rack: Box<Rack>,
    pub routers: Vec<Router>,
    pub align: Align,
}

impl Default for V1Core {
    fn default() -> Self {
        Self { rack: Box::default(), routers: (0..super::RACK_SLOTS).map(|_| Router::default()).collect(), align: Align::default() }
    }
}

impl V1Core {
    /// Storage for `parts` parts at `rate`, allocated on a worker.
    pub fn with_parts(parts: usize, rate: f64) -> Self {
        let mut rack = Box::new(Rack::with_slots(parts));
        rack.reset(rate);
        Self { rack, align: Align::with_slots(parts), routers: (0..parts).map(|_| Router::default()).collect() }
    }

    /// Adopt larger worker-prepared storage, keeping every playing part.
    /// The replaced storage stays in `grown` to be dropped off audio.
    pub fn adopt(&mut self, grown: &mut Self) {
        let rate = self.sample_rate();
        for engine in &mut grown.rack.parts[self.rack.parts.len()..] {
            engine.reset(rate);
        }
        self.rack.adopt_parts(&mut grown.rack);
        self.align.adopt_parts(&mut grown.align);
        for (old, new) in self.routers.iter_mut().zip(&mut grown.routers) {
            std::mem::swap(old, new);
        }
        std::mem::swap(&mut self.routers, &mut grown.routers);
    }

    fn engine(&self, part: usize) -> Option<&Engine> {
        self.rack.parts.get(part)
    }
}

/// One part's loader output, installed between blocks.
pub enum Prepared {
    /// A new instrument, or an empty slot, with its effects and initialized scripts.
    Part { bank: Option<Box<Bank>>, fx: FxProcessor, script: Option<Box<Runtime>> },
    /// Effects rebuilt for a new host sample rate; the bank stays.
    Fx(FxProcessor),
    /// Restored scripts; synchronous zone init may also prepare new geometry.
    Script { script: Option<Box<Runtime>>, bank: Option<Box<Bank>> },
    /// The part's samples loaded whole (RAM only): replaces its streaming
    /// bank under the playing voices once its zone geometry matches.
    Bank(Box<Bank>),
    /// Sample heads the smart memory resized.
    Heads(Heads),
}

/// What [`V1Core::install`] replaced, to drop on a worker.
#[derive(Default)]
pub struct Retired {
    pub bank: Option<Box<Bank>>,
    pub fx: Option<FxProcessor>,
    pub script: Option<Box<Runtime>>,
    /// The previous heads; the shell may hand them back to the smart memory.
    pub heads: Option<Heads>,
    /// A RAM bank whose zone geometry was not ready: not installed; the shell
    /// rebases it on the worker and offers it again.
    pub deferred: Option<Box<Bank>>,
}

/// A part's saved state: persistent script values, IR slot settings and
/// native engine edits, refreshed in place on the audio thread.
pub(crate) struct PersistenceSnapshot {
    pub(crate) script: Vec<Persisted>,
    /// Worker-prepared slot list: audio updates values in place, including
    /// edits whose new kernel has not finished building yet.
    pub(crate) ir: Vec<crate::fx::IrSlotSettings>,
    pub(crate) native: crate::engine::native_state::NativeSnapshot,
}

impl V1Core {
    /// Refresh `saved` from `part`, about `budget` values at a time.
    /// `Some(changed)` once whole; IR slot settings are folded in then.
    pub(crate) fn refresh_state(&self, part: usize, saved: &mut PersistenceSnapshot, at: &mut Refresh, budget: usize) -> Option<bool> {
        let engine = self.engine(part)?;
        let whole = engine.script().is_none_or(|rt| {
            rt.refresh_persistence_within(&mut saved.script, at, budget) && rt.native_state.refresh(&mut saved.native, 256)
        });
        if !whole {
            return None;
        }
        let mut changed = at.changed || saved.native.changed;
        for value in &mut saved.ir {
            if let Some(settings) = engine.fx().ir_settings(value.rack, value.slot) {
                changed |= value.settings != settings;
                value.settings = settings;
            }
        }
        Some(changed)
    }
}

impl Core for V1Core {
    type Prepared = Prepared;
    type Retired = Retired;
    type Live = Live;

    fn parts(&self) -> usize {
        self.rack.parts.len()
    }

    fn sample_rate(&self) -> f64 {
        self.rack.parts[0].rate()
    }

    fn reset(&mut self, sample_rate: f64) {
        self.rack.reset(sample_rate);
        self.align.clear();
        for router in &mut self.routers {
            router.reset_midi();
        }
    }

    fn panic(&mut self) {
        self.align.clear();
        for router in &mut self.routers {
            router.reset_midi();
        }
        self.rack.panic();
    }

    fn install(&mut self, part: usize, prepared: Prepared) -> Retired {
        let rate = self.sample_rate();
        let Some(engine) = self.rack.parts.get_mut(part) else {
            return match prepared {
                Prepared::Part { bank, fx, script } => Retired { bank, fx: Some(fx), script, ..Retired::default() },
                Prepared::Fx(fx) => Retired { fx: Some(fx), ..Retired::default() },
                Prepared::Script { script, bank } => Retired { script, bank, ..Retired::default() },
                Prepared::Bank(bank) => Retired { bank: Some(bank), ..Retired::default() },
                Prepared::Heads(heads) => Retired { heads: Some(heads), ..Retired::default() },
            };
        };
        match prepared {
            Prepared::Part { bank, fx, script } => {
                engine.reset(rate);
                Retired { script: engine.set_script(script), bank: engine.set_bank(bank), fx: Some(engine.set_fx(fx)), ..Retired::default() }
            }
            Prepared::Fx(fx) => Retired { fx: Some(engine.set_fx(fx)), ..Retired::default() },
            Prepared::Bank(bank) if engine.zone_upgrade_ready(&bank) => Retired { bank: engine.upgrade_bank(bank), ..Retired::default() },
            Prepared::Bank(bank) => Retired { deferred: Some(bank), ..Retired::default() },
            Prepared::Script { script, bank } => {
                engine.reset(rate);
                let mut retired = Retired { script: engine.set_script(script), ..Retired::default() };
                if let Some(bank) = bank {
                    retired.bank = engine.set_bank(Some(bank));
                }
                retired
            }
            Prepared::Heads(mut heads) => {
                engine.swap_heads(&mut heads);
                Retired { heads: Some(heads), ..Retired::default() }
            }
        }
    }

    fn holding(&self, playing: bool) -> bool {
        self.align.holding(playing)
    }

    fn set_transport(&mut self, t: Transport) {
        self.rack.set_transport(t.playing, t.tempo, t.beats, t.signature);
    }

    fn begin_block(&mut self, block: &BlockInfo) {
        let scripted = self.rack.parts.iter().filter(|e| e.script().is_some()).count();
        for engine in &mut self.rack.parts {
            engine.begin_audio_block(block.frames, scripted, block.offline);
        }
        self.set_transport(block.transport);
        if !block.holding && self.align.next_due().is_some() {
            self.align.flush(&mut self.rack, &mut self.routers);
        }
    }

    fn event(&mut self, port: u8, ev: In, offset: u32, holding: bool) {
        if holding {
            let rate = self.sample_rate();
            self.align.arrive(&mut self.rack, &mut self.routers, port, ev, self.align.clock + u64::from(offset), rate);
        } else {
            articulate::dispatch(&mut self.rack, &mut self.routers, port, ev);
        }
    }

    fn key_held(&self, channel: u8, key: u8, holding: bool) -> bool {
        self.rack.parts.iter().any(|e| e.host_key_held(channel, key)) || holding && self.align.host_key_held(channel, key)
    }

    fn release_due(&mut self, at: usize, limit: usize) -> usize {
        let now = self.align.clock + at as u64;
        self.align.release(now, &mut self.rack, &mut self.routers);
        self.align.next_due().map_or(limit, |held| limit.min(at + (held - now).min(limit as u64) as usize))
    }

    fn render(&mut self, frames: usize) -> Rendered<'_> {
        let (buses, live) = self.rack.render_live(frames);
        Rendered { buses, live }
    }

    fn end_block(&mut self, frames: usize, end: &mut dyn FnMut(HostNote) -> bool) -> u64 {
        let refused = self.finish_host_notes(end);
        self.align.clock += frames as u64;
        refused
    }

    fn ui_control(&mut self, part: usize, slot: usize, control: usize, value: i32) {
        let Some(engine) = self.rack.parts.get_mut(part) else { return };
        engine.ui_control(slot, control, value);
        self.routers[part].forget();
        let picked = self.routers[part].articulation_of_control(slot, control);
        self.align.picked(part, picked);
    }

    fn script_revision(&self, part: usize) -> u64 {
        self.engine(part).and_then(Engine::script).map_or(0, Runtime::changes)
    }

    fn refresh_persistence(&self, part: usize, saved: &mut [Persisted], at: &mut Refresh, budget: usize) -> bool {
        self.engine(part).and_then(Engine::script).is_none_or(|rt| rt.refresh_persistence_within(saved, at, budget))
    }

    fn refresh_live(&self, part: usize, live: &mut Live, at: &mut Refresh, budget: usize) -> bool {
        self.engine(part).and_then(Engine::script).is_none_or(|rt| rt.refresh_live_within(live, at, budget))
    }

    fn voices(&self) -> Voices {
        Voices {
            active: self.rack.parts.iter().map(Engine::active_voices).sum(),
            audible: self.rack.parts.iter().map(Engine::audible_voices).sum(),
            dropouts: self.rack.parts.iter().map(|e| e.underruns() + e.dropped_commands()).sum(),
        }
    }
}

impl V1Core {
    /// Offer each ended host note once nothing still owns it: every part's
    /// voices and scripts, and any input the alignment holds back.
    fn finish_host_notes(&mut self, end: &mut dyn FnMut(HostNote) -> bool) -> u64 {
        for e in &mut self.rack.parts {
            e.mark_host_notes();
        }
        for part in 0..self.rack.parts.len() {
            let mut index = 0;
            while let Some((note, pinned)) = self.rack.parts[part].host_note_at(index) {
                if pinned || self.rack.parts.iter().any(|e| e.host_note_pending(note)) || self.align.host_note_waiting(note) {
                    index += 1;
                    continue;
                }
                if !end(note) {
                    return 1;
                }
                for e in &mut self.rack.parts {
                    e.retire_host_note(note);
                }
                self.align.retire_host_note(note);
            }
        }
        // A disabled/remapped-away input can have a held alignment record but
        // no engine root. Close it after key-up without leaking adapter owners.
        let mut index = 0;
        while let Some((note, held)) = self.align.host_note_at(index) {
            if held || self.align.host_note_waiting(note) || self.rack.parts.iter().any(|e| e.host_note_present(note)) {
                index += 1;
                continue;
            }
            if !end(note) {
                return 1;
            }
            self.align.retire_host_note(note);
        }
        0
    }

    /// Whether an exact host note-on created no owner anywhere (an unmatched
    /// route or a consumed key switch): its end is due at once.
    pub fn host_note_unowned(&self, note: HostNote) -> bool {
        !self.align.host_note_waiting(note) && !self.rack.parts.iter().any(|e| e.host_note_present(note))
    }
}

/// Prepares v1 parts: Kontakt import, scripts, effects and a bare bank.
///
/// The plugin's own loader still runs its richer path (snapshots, preload
/// budgets, artwork, diagnostics); this is the minimal, reusable one.
pub struct V1Loader;

impl CoreLoader for V1Loader {
    type Core = V1Core;

    fn prepare(&self, request: &LoadRequest, progress: &mut dyn FnMut(Progress), canceled: &(dyn Fn() -> bool + Sync)) -> Result<Prepared, CoreError> {
        let load = |e: anyhow::Error| CoreError::Load(format!("{e:#}"));
        let instrument = crate::import::shared_program(&request.path, request.program).map_err(load)?;
        progress(Progress(50));
        if canceled() {
            return Err(CoreError::Canceled);
        }
        // Like the plugin's loader, a script whose init fails is left out, not fatal.
        let (script, _skipped) = crate::engine::load_scripts(&instrument, request.persisted.clone(), request.sample_rate);
        progress(Progress(100));
        let bank = if instrument.zones.is_empty() {
            None
        } else {
            Some(Box::new(Bank::load_bare_cancelable(&instrument, canceled).map_err(load)?))
        };
        if canceled() {
            return Err(CoreError::Canceled);
        }
        let fx = crate::engine::effects(&instrument, script.as_deref(), request.sample_rate as f32);
        progress(Progress::DONE);
        Ok(Prepared::Part { bank, fx, script })
    }

    fn describe(&self, path: &std::path::Path, program: u32) -> Result<Description, CoreError> {
        let instrument = crate::import::shared_program(path, program).map_err(|e| CoreError::Load(format!("{e:#}")))?;
        Ok(Description { name: instrument.name.clone(), zones: instrument.zones.len(), scripts: instrument.scripts.len() })
    }
}
