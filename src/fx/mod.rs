//! Kontakt effect chains: parsing from program chunks and real-time DSP.
//!
//! Ownership: [`ProgramFx`] and its parts are an immutable description,
//! cheap to clone and share (impulse responses sit behind `Arc`).
//! [`ProgramFx::processor`] builds an owned [`FxProcessor`] holding all DSP
//! state; that call allocates everything, and [`FxProcessor::process`] never
//! allocates, locks or panics.

mod convolution;
mod kind;
pub mod params;
mod processor;
mod reverb;

pub use kind::{Kind, ksp_effect_type};
pub use params::Params;
pub use processor::{DIRECT, FxParam, FxProcessor, OUTS, Rack};

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
        let fx = chain.slots.iter().find(|fx| fx.slot == slot as usize)?;
        Some(match (param, &fx.params) {
            (FxParam::Bypass, _) => f32::from(fx.bypass),
            (FxParam::Wet, _) => fx.output_gain,
            (FxParam::Dry, _) => fx.dry_level,
            (FxParam::Type, _) => f32::from(fx.kind.ser_id()),
            (FxParam::Reverb(n), Params::Reverb(p)) => *{ *p }.field(n)?,
            (FxParam::SendLevel(n), Params::SendLevels(levels)) => {
                *levels.sends.get(n as usize)?
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

    /// Compatibility notes: active effects that pass through, ignored params.
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (location, fx) in self.effects() {
            let at = format!("{} in {location} slot {}", fx.kind.name(), fx.slot + 1);
            if fx.bypass {
                continue;
            }
            if !fx.is_implemented() {
                out.push(format!(
                    "{at} is active but not implemented; audio passes through"
                ));
            }
            match &fx.params {
                Params::Convolution(c) => {
                    if let Some(e) = &c.ir_error {
                        out.push(format!("{at}: impulse response unavailable ({e})"));
                    }
                    let band = |b: &params::IrBand| {
                        b.low_cut_hz > 20.5 || b.high_cut_hz < 19_990.0 || b.length_ratio != 1.0
                    };
                    if band(&c.early) || band(&c.late) {
                        out.push(format!("{at}: IR filter/length shaping is not applied"));
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
