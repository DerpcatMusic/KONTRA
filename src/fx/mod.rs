//! Kontakt effect chains: parsing from program chunks and real-time DSP.
//!
//! Ownership: [`ProgramFx`] and its parts are an immutable description,
//! cheap to clone and share (impulse responses sit behind `Arc`).
//! [`ProgramFx::processor`] builds an owned [`FxProcessor`] holding all DSP
//! state; that call allocates everything, and [`FxProcessor::process`] never
//! allocates, locks or panics.

pub(crate) mod blocks;
mod convolution;
mod kind;
pub mod params;
mod processor;
mod reverb;

pub use kind::{Kind, ksp_effect_type};
pub use crate::engine::filter::Knob as FilterParam;
pub use params::Params;
pub(crate) use processor::SendInputs;
pub use processor::{DIRECT, FxParam, FxProcessor, OUTS, PreparedIr, Rack};

use anyhow::{Context, Result, ensure};
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{BParFX, BParamArrayBParFX8, InsertBus, Program},
};
use params::Impulse;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

/// Longest impulse response loaded (seconds at the IR's own rate).
const MAX_IR_SECONDS: usize = 20;

/// What a script's `on init` loaded into an effect slot; effects build
/// with it ([`ProgramFx::processor_with`]).
#[derive(Clone)]
pub struct ScriptIr {
    pub rack: Rack,
    pub slot: u8,
    pub load: Load,
}

#[derive(Clone)]
pub enum Load {
    /// `load_ir_sample`: an impulse response for a convolution slot.
    Ir { file: std::path::PathBuf, ir: Impulse },
    /// `$ENGINE_PAR_EFFECT_TYPE`: another effect, at its defaults; `None`
    /// empties the slot.
    Kind(Option<Kind>),
    /// Convolution settings, retained across worker/rate rebuilds.
    Convolution(params::IrSettings),
}

/// Requested convolution values owned by one effect slot in saved host state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrSlotSettings {
    pub rack: Rack,
    pub slot: u8,
    pub settings: params::IrSettings,
    #[serde(default)]
    pub file: Option<std::path::PathBuf>,
}

impl PartialEq for ScriptIr {
    fn eq(&self, o: &Self) -> bool {
        (self.rack, self.slot) == (o.rack, o.slot)
            && match (&self.load, &o.load) {
                (Load::Ir { file: a, .. }, Load::Ir { file: b, .. }) => a == b,
                (Load::Kind(a), Load::Kind(b)) => a == b,
                (Load::Convolution(a), Load::Convolution(b)) => a == b,
                _ => false,
            }
    }
}

impl ScriptIr {
    /// Decode `file` as the impulse response for `rack`/`slot`.
    pub fn load(rack: Rack, slot: u8, file: std::path::PathBuf) -> Result<Self> {
        let ir = crate::audio::decode(&file, MAX_IR_SECONDS * 192_000)?;
        Ok(Self { rack, slot, load: Load::Ir { file, ir: Impulse(Arc::new(ir)) } })
    }
}

/// One slot of an 8-slot Kontakt effect rack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Effect {
    /// Rack position 0..8; send levels address send slots by this index.
    pub slot: usize,
    pub kind: Kind,
    pub version: u16,
    pub bypass: bool,
    /// Linear gain on the processed signal (slot state, not an effect param).
    pub output_gain: f32,
    /// Linear level of the unprocessed signal added after the effect.
    pub dry_level: f32,
    pub params: Params,
}

/// An effect rack in slot order (empty slots omitted).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Chain {
    pub slots: Vec<Effect>,
}

/// One of the 16 instrument buses (`BInsertBus`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bus {
    pub index: usize,
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    /// -1 = instrument output.
    pub output: i32,
    pub chain: Chain,
}

/// All effects of one program.
///
/// Signal flow: groups routed to a bus (by script, `$ENGINE_PAR_OUTPUT_CHANNEL`)
/// render into it; each bus runs its chain, fader and pan and joins the other
/// groups. Then `insert` in series; a Send Levels slot in `insert` taps the
/// signal at its position into the parallel `send` slots, whose returns are
/// summed back; then `main` (see `audits/EFFECTS.md`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProgramFx {
    pub insert: Chain,
    pub send: Chain,
    pub main: Chain,
    pub buses: Vec<Bus>,
}

impl Effect {
    fn read(slot: usize, chunk: &Chunk) -> Result<Self> {
        let wrapper = BParFX::try_from(chunk)?;
        let state = wrapper.params()?;
        ensure!(
            state.output_gain.is_finite() && state.dry_level.is_finite(),
            "Non-finite effect levels"
        );
        let effect = wrapper.effect().context("Effect slot has no effect")?;
        let object = StructuredObject::try_from(effect)?;
        let kind = Kind::from_ser_id(effect.id);
        Ok(Self {
            slot,
            kind,
            version: object.version,
            bypass: state.bypass,
            output_gain: state.output_gain,
            dry_level: state.dry_level,
            params: params::parse(kind, &object.public_data),
        })
    }

    /// Whether this effect changes audio once prepared (bypassed slots do not).
    pub fn is_implemented(&self) -> bool {
        match &self.params {
            Params::Gainer(_) | Params::StereoModeller(_) | Params::Reverb(_) => true,
            Params::SendLevels(_) => true,
            Params::Convolution(c) => c.ir.is_some(),
            Params::Filter(f) => crate::engine::filter::filter_type(f.filter_type).is_some(),
            Params::Eq(_) => true,
            Params::Fields(_) => self.kind.has_dsp(),
            _ => false,
        }
    }
}

impl Chain {
    fn read(chunk: &Chunk) -> Result<Self> {
        Self::from_array(&BParamArrayBParFX8::try_from(chunk)?)
    }

    /// A rack from its parameter array (group racks live in private data).
    pub fn from_array(array: &BParamArrayBParFX8) -> Result<Self> {
        let slots = array
            .items
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| item.as_ref().map(|c| (slot, c)))
            .map(|(slot, c)| Effect::read(slot, c).with_context(|| format!("Effect slot {slot}")))
            .collect::<Result<_>>()?;
        Ok(Self { slots })
    }

    fn effects(&self) -> impl Iterator<Item = &Effect> {
        self.slots.iter()
    }
}

impl Bus {
    fn read(index: usize, chunk: &Chunk) -> Result<Self> {
        let bus = InsertBus::try_from(chunk)?;
        let p = bus.params()?;
        let chain = match bus.0.find_first(0x3a) {
            Some(c) => Chain::read(c)?,
            None => Chain::default(),
        };
        Ok(Self {
            index,
            name: p.name,
            volume: p.volume,
            pan: p.pan,
            output: p.output,
            chain,
        })
    }
}

impl ProgramFx {
    /// Reads the program's three racks (insert, send, main, in stored order)
    /// and its instrument buses.
    pub fn read(program: &Program) -> Result<Self> {
        let mut racks = Vec::new();
        let mut buses = Vec::new();
        for child in &program.0.children {
            match child.id {
                0x3a => racks.push(
                    Chain::read(child)
                        .with_context(|| format!("Instrument effect rack {}", racks.len()))?,
                ),
                0x45 => buses.push(
                    Bus::read(buses.len(), child)
                        .with_context(|| format!("Instrument bus {}", buses.len()))?,
                ),
                _ => {}
            }
        }
        ensure!(racks.len() <= 3, "Program has {} effect racks", racks.len());
        let mut racks = racks.into_iter();
        Ok(Self {
            insert: racks.next().unwrap_or_default(),
            send: racks.next().unwrap_or_default(),
            main: racks.next().unwrap_or_default(),
            buses,
        })
    }

    /// Stored value of a script-controllable parameter, as
    /// [`FxProcessor::param`] reports it once built.
    pub fn param(&self, rack: Rack, slot: u8, param: FxParam) -> Option<f32> {
        let chain = match rack {
            Rack::Insert => &self.insert,
            Rack::Send => &self.send,
            Rack::Main => &self.main,
            Rack::Bus(b) => {
                let bus = self.buses.iter().find(|bus| bus.index == b as usize)?;
                match param {
                    FxParam::Volume => return Some(bus.volume),
                    FxParam::Pan => return Some(bus.pan),
                    FxParam::Output => {
                        return Some(if (0..OUTS as i32).contains(&bus.output) { bus.output as f32 } else { -1.0 });
                    }
                    _ => &bus.chain,
                }
            }
        };
        let Some(fx) = chain.slots.iter().find(|fx| fx.slot == slot as usize) else {
            // `$EFFECT_TYPE_NONE`.
            return (param == FxParam::Type && slot < 8).then_some(0.0);
        };
        Some(match (param, &fx.params) {
            (FxParam::Bypass, _) => f32::from(fx.bypass),
            (FxParam::Wet, _) => fx.output_gain,
            (FxParam::Dry, _) => fx.dry_level,
            (FxParam::Type, _) => f32::from(fx.kind.ser_id()),
            (FxParam::Reverb(n), Params::Reverb(p)) => *{ *p }.field(n)?,
            (FxParam::Convolution(n), Params::Convolution(p)) => params::IrSettings::from_convolution(p).value(n)?,
            (FxParam::Filter(knob), _) => crate::engine::filter::effect_knob(fx, knob)?,
            (FxParam::SendLevel(n), Params::SendLevels(levels)) => {
                *levels.sends.get(n as usize)?
            }
            (FxParam::Field(kind, n), params) if kind == fx.kind => {
                blocks::normalized(kind, n, *blocks::fields(params)?.get(n as usize)?)
            }
            _ => return None,
        })
    }

    /// Every effect with a human-readable location.
    pub fn effects(&self) -> impl Iterator<Item = (String, &Effect)> {
        let racks = [
            ("instrument insert", &self.insert),
            ("instrument send", &self.send),
            ("instrument main", &self.main),
        ];
        racks
            .into_iter()
            .flat_map(|(name, chain)| chain.effects().map(move |fx| (name.to_owned(), fx)))
            .chain(self.buses.iter().flat_map(|bus| {
                bus.chain
                    .effects()
                    .map(move |fx| (format!("bus {} ({})", bus.index + 1, bus.name), fx))
            }))
    }

    pub(crate) fn effects_mut(&mut self) -> impl Iterator<Item = &mut Effect> {
        [&mut self.insert, &mut self.send, &mut self.main]
            .into_iter()
            .chain(self.buses.iter_mut().map(|b| &mut b.chain))
            .flat_map(|chain| chain.slots.iter_mut())
    }

    /// Fills `ir_file` for convolution slots from the preset's "other files" table.
    pub fn name_impulses(&mut self, files: &HashMap<u32, String>) {
        for fx in self.effects_mut() {
            if let Params::Convolution(c) = &mut fx.params {
                c.ir_file = u32::try_from(c.ir_index)
                    .ok()
                    .and_then(|i| files.get(&i).cloned());
            }
        }
    }

    /// Decodes the IR of every convolution slot, bypassed ones included since
    /// scripts may switch them on; one decode per file.
    pub fn load_impulses(
        &mut self,
        mut load: impl FnMut(&str, usize) -> Result<crate::audio::Sample>,
    ) {
        let mut cache: HashMap<String, Result<Arc<crate::audio::Sample>, String>> = HashMap::new();
        for fx in self.effects_mut() {
            let Params::Convolution(c) = &mut fx.params else {
                continue;
            };
            let Some(file) = c.ir_file.clone() else {
                c.ir_error = Some(format!("IR index {} is not in the file table", c.ir_index));
                continue;
            };
            let loaded = cache.entry(file).or_insert_with_key(|file| {
                load(file, MAX_IR_SECONDS * 192_000)
                    .map(Arc::new)
                    .map_err(|e| format!("{e:#}"))
            });
            match loaded {
                Ok(sample) => c.ir = Some(Impulse(Arc::clone(sample))),
                Err(e) => c.ir_error = Some(e.clone()),
            }
        }
    }

    fn chain_mut(&mut self, rack: Rack) -> Option<&mut Chain> {
        Some(match rack {
            Rack::Insert => &mut self.insert,
            Rack::Send => &mut self.send,
            Rack::Main => &mut self.main,
            Rack::Bus(b) => &mut self.buses.iter_mut().find(|bus| bus.index == b as usize)?.chain,
        })
    }

    /// The convolution slot at `rack`/`slot`, if there is one.
    fn convolution(&mut self, rack: Rack, slot: u8) -> Option<&mut params::Convolution> {
        let chain = self.chain_mut(rack)?;
        match &mut chain.slots.iter_mut().find(|fx| fx.slot == slot as usize)?.params {
            Params::Convolution(c) => Some(c),
            _ => None,
        }
    }

    /// Load `kind` at its defaults into `rack`/`slot` (`None` empties it),
    /// as `$ENGINE_PAR_EFFECT_TYPE` does. Bypass and levels stay with the
    /// slot. False when there is no such rack or slot.
    pub fn load_kind(&mut self, rack: Rack, slot: u8, kind: Option<Kind>) -> bool {
        let Some(chain) = self.chain_mut(rack).filter(|_| slot < 8) else {
            return false;
        };
        let at = chain.slots.iter().position(|fx| fx.slot == slot as usize);
        match (kind, at) {
            (None, Some(i)) => {
                chain.slots.remove(i);
            }
            (None, None) => {}
            (Some(kind), Some(i)) => {
                let fx = &mut chain.slots[i];
                (fx.kind, fx.params) = (kind, params::defaults(kind));
            }
            (Some(kind), None) => {
                let i = chain.slots.partition_point(|fx| fx.slot < slot as usize);
                let fx = Effect {
                    slot: slot as usize,
                    kind,
                    version: 0,
                    bypass: false,
                    output_gain: 1.0,
                    dry_level: 0.0,
                    params: params::defaults(kind),
                };
                chain.slots.insert(i, fx);
            }
        }
        true
    }

    /// [`processor`](Self::processor) with what scripts loaded in `on init`
    /// (`loads`, in order) in place of the preset's effects.
    pub fn processor_with(&self, sample_rate: f32, max_block: usize, loads: &[ScriptIr]) -> FxProcessor {
        self.processor_loaded(sample_rate, max_block, loads, false)
    }

    /// Build return DSP when decoded group taps can feed it, even without an
    /// instrument insert tap. Unknown Amplifier placement remains unsupported.
    pub fn processor_for_groups(&self, sample_rate: f32, max_block: usize, loads: &[ScriptIr], groups: &[crate::import::Group]) -> FxProcessor {
        let tapped = groups.iter().any(|g| g.amp_split_slot.is_some()
            && g.fx.slots.iter().any(|fx| fx.slot < 8 && matches!(fx.params, Params::SendLevels(_))));
        self.processor_loaded(sample_rate, max_block, loads, tapped)
    }

    fn processor_loaded(&self, sample_rate: f32, max_block: usize, loads: &[ScriptIr], group_sends: bool) -> FxProcessor {
        if loads.is_empty() {
            return self.processor_sends(sample_rate, max_block, group_sends);
        }
        let mut out = self.with_loads(loads).processor_sends(sample_rate, max_block, group_sends);
        for l in loads {
            if let Load::Convolution(settings) = l.load {
                out.init_ir_settings(l.rack, l.slot, settings);
            }
        }
        out
    }

    /// Prepare the fixed slot list for the audio thread's lent state snapshot.
    pub fn ir_settings_with(&self, loads: &[ScriptIr]) -> Vec<IrSlotSettings> {
        let fx = self.with_loads(loads);
        [(Rack::Insert, &fx.insert), (Rack::Send, &fx.send), (Rack::Main, &fx.main)]
            .into_iter().chain(fx.buses.iter().filter_map(|b| Some((Rack::Bus(u8::try_from(b.index).ok()?), &b.chain))))
            .flat_map(|(rack, chain)| chain.slots.iter().filter_map(move |s| {
                let Params::Convolution(p) = &s.params else { return None };
                let slot = u8::try_from(s.slot).ok()?;
                let settings = loads.iter().rev().find_map(|l| match l.load {
                    Load::Convolution(v) if (l.rack, l.slot) == (rack, slot) => Some(v), _ => None,
                }).unwrap_or_else(|| params::IrSettings::from_convolution(p)).inherit_flags(params::IrSettings::from_convolution(p));
                let file = loads.iter().rev().find_map(|l| match &l.load {
                    Load::Ir { file, .. } if (l.rack, l.slot) == (rack, slot) => Some(file.clone()), _ => None,
                });
                Some(IrSlotSettings { rack, slot, settings, file })
            })).collect()
    }

    /// Build just the IR's slot off the audio thread.
    pub fn prepare_ir(&self, rack: Rack, slot: u8, sample_rate: f32, max_block: usize, loads: &[ScriptIr]) -> Option<PreparedIr> {
        let mut fx = self.with_loads(loads);
        let p = fx.convolution(rack, slot)?;
        let mut ir = PreparedIr::new(rack, slot, p, sample_rate, max_block)?;
        if let Some(settings) = loads.iter().rev().find_map(|l| match l.load {
            Load::Convolution(s) if (l.rack, l.slot) == (rack, slot) => Some(s),
            _ => None,
        }) { ir.settings = settings.inherit_flags(ir.settings); }
        Some(ir)
    }

    fn with_loads(&self, loads: &[ScriptIr]) -> Self {
        let mut fx = self.clone();
        for l in loads {
            match &l.load {
                Load::Kind(kind) => {
                    fx.load_kind(l.rack, l.slot, *kind);
                }
                Load::Ir { file, ir } => {
                    if let Some(c) = fx.convolution(l.rack, l.slot) {
                        (c.ir, c.ir_file, c.ir_error) = (Some(ir.clone()), Some(file.display().to_string()), None);
                    }
                }
                Load::Convolution(settings) => {
                    if let Some(p) = fx.convolution(l.rack, l.slot) { settings.apply(p); }
                }
            }
        }
        fx
    }

    /// Compatibility notes: active effects that pass through, ignored params.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (location, fx) in self.effects() {
            let at = format!("{} in {location} slot {}", fx.kind.name(), fx.slot + 1);
            if fx.bypass {
                continue;
            }
            if fx.kind == Kind::SurroundPanner && blocks::fields(&fx.params).is_some_and(|f| f[1] != 0.0) {
                out.push(format!("{at}: Enhanced/Drums modes use an unverified transfer-curve proxy"));
            }
            if fx.kind == Kind::Distortion {
                out.push(format!("{at}: native DC filtering and Damping parameter smoothing are not applied"));
            }
            if !fx.is_implemented() {
                out.push(format!(
                    "{at} is active but not implemented; audio passes through"
                ));
            }
            match &fx.params {
                Params::Convolution(c) => {
                    if c.auto_gain() && ((c.envelope_active() && !c.envelope_supported())
                        || c.early.low_cut_hz != c.late.low_cut_hz
                        || c.early.high_cut_hz != c.late.high_cut_hz
                        || c.early.length_ratio != 1.0 || c.late.length_ratio != 1.0) {
                        out.push(format!("{at}: Auto Gain uses the approximated IR when native shaping is unsupported; its energy can differ"));
                    }
                    if c.envelope_active() && !c.envelope_supported() {
                        out.push(format!("{at}: native IR Volume Envelope is not applied; eight finite time/level knots are required"));
                    }
                    if let Some(e) = &c.ir_error {
                        out.push(format!("{at}: impulse response unavailable ({e})"));
                    }
                    if c.early.low_cut_hz != c.late.low_cut_hz || c.early.high_cut_hz != c.late.high_cut_hz {
                        out.push(if c.explicit_split_supported() {
                            format!("{at}: independent early/late IR filtering requires matching IR/host rates; other rates remain unsupported")
                        } else {
                            format!("{at}: independent early/late IR filtering is not applied")
                        });
                    }
                    if c.early.length_ratio != c.late.length_ratio {
                        out.push(format!("{at}: independent early/late IR sizing is not applied; late size stretches the whole IR"));
                    }
                    if c.early.length_ratio != 1.0 || c.late.length_ratio != 1.0 {
                        out.push(format!("{at}: IR Size uses a resampling approximation; native pitch-preserving time stretching is not applied"));
                    }
                }
                Params::SendLevels(s) => {
                    if location != "instrument insert" {
                        out.push(format!("{at}: send taps are not applied in this rack"));
                    }
                    if s.outputs.iter().any(|&v| v != 1.0) {
                        out.push(format!("{at}: the undecoded output-routing table is not applied"));
                    }
                }
                Params::StereoModeller(s) if s.pseudo_stereo => {
                    out.push(format!("{at}: pseudo stereo is not applied"));
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
