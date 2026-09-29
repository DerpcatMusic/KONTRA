//! Kontakt effect chains: parsing from program chunks and real-time DSP.
//!
//! Real-time contract: all allocation happens in `prepare`; `process` never
//! allocates, locks or panics, and accepts any block length (it splits blocks
//! longer than `max_block`).

mod convolution;
mod kind;
pub mod params;
mod reverb;

pub use kind::Kind;
pub use params::Params;

use anyhow::{Context, Result, ensure};
use convolution::Convolver;
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{BParFX, BParamArrayBParFX8, InsertBus, Program},
};
use params::Impulse;
use reverb::Reverb;
use serde::Serialize;
use std::{collections::HashMap, fmt, sync::Arc};

/// Longest impulse response loaded (seconds at the IR's own rate).
const MAX_IR_SECONDS: usize = 20;

/// One slot of an 8-slot Kontakt effect rack.
#[derive(Debug, Serialize)]
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
    #[serde(skip)]
    runtime: Option<Runtime>,
}

/// An effect rack in slot order (empty slots omitted).
#[derive(Debug, Default, Serialize)]
pub struct Chain {
    pub slots: Vec<Effect>,
    #[serde(skip)]
    max_block: usize,
}

/// One of the 16 instrument buses (`BInsertBus`).
#[derive(Debug, Serialize)]
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
/// Signal flow: `insert` in series; a Send Levels slot in `insert` taps the
/// signal at its position into the parallel `send` slots, whose returns are
/// summed back; then `main`. Buses are parsed and preparable but not mixed
/// here: group-to-bus routing belongs to the engine.
#[derive(Debug, Default, Serialize)]
pub struct ProgramFx {
    pub insert: Chain,
    pub send: Chain,
    pub main: Chain,
    pub buses: Vec<Bus>,
    #[serde(skip)]
    send_buffers: Vec<[Vec<f32>; 2]>,
}

struct Runtime {
    processor: Processor,
    dry: [Vec<f32>; 2],
}

impl fmt::Debug for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Runtime")
    }
}

enum Processor {
    Gain(f32),
    Stereo(Stereo),
    Reverb(Box<Reverb>),
    Convolution(Box<[Convolver; 2]>),
}

/// Stereo Modeller: mid/side width then constant-sum balance.
struct Stereo {
    width: f32,
    gains: [f32; 2],
}

impl Processor {
    fn new(params: &Params, sample_rate: f32, max_block: usize) -> Option<Self> {
        Some(match params {
            Params::Gainer(p) => Processor::Gain(p.gain),
            Params::StereoModeller(p) => Processor::Stereo(Stereo {
                width: (1.0 + p.spread).clamp(0.0, 2.0),
                gains: [(1.0 - p.pan).clamp(0.0, 1.0), (1.0 + p.pan).clamp(0.0, 1.0)],
            }),
            Params::Reverb(p) => Processor::Reverb(Box::new(Reverb::new(p, sample_rate))),
            Params::Convolution(p) => {
                let ir = prepare_ir(p, sample_rate)?;
                Processor::Convolution(Box::new(ir.map(|ch| Convolver::new(&ch, max_block))))
            }
            _ => return None,
        })
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            Processor::Gain(g) => {
                for v in left.iter_mut() {
                    *v *= *g;
                }
                for v in right.iter_mut() {
                    *v *= *g;
                }
            }
            Processor::Stereo(s) => {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let (mid, side) = (0.5 * (*l + *r), 0.5 * (*l - *r) * s.width);
                    *l = (mid + side) * s.gains[0];
                    *r = (mid - side) * s.gains[1];
                }
            }
            Processor::Reverb(rv) => rv.process(left, right),
            Processor::Convolution(conv) => {
                conv[0].process(left);
                conv[1].process(right);
            }
        }
    }
}

/// Resamples (linear), trims and predelays the IR for `sample_rate`.
fn prepare_ir(p: &params::Convolution, sample_rate: f32) -> Option<[Vec<f32>; 2]> {
    let ir = &p.ir.as_ref()?.0;
    let ratio = ir.rate as f32 / sample_rate;
    let keep = p.late.length_ratio.clamp(0.0, 1.0);
    let len = ((ir.frames.len() as f32 / ratio) * keep) as usize;
    let pre = (p.predelay_ms.max(0.0) * 0.001 * sample_rate) as usize;
    // ponytail: linear interpolation aliases slightly on rate changes; use a
    // windowed-sinc resampler if IR brightness at 44.1k<->48k ever matters.
    Some(std::array::from_fn(|ch| {
        let mut out = vec![0.0; pre];
        out.extend((0..len.max(1)).map(|i| {
            let x = i as f32 * ratio;
            let (j, frac) = (x as usize, x.fract());
            let a = ir.frames.get(j).map_or(0.0, |f| f[ch]);
            let b = ir.frames.get(j + 1).map_or(0.0, |f| f[ch]);
            a + (b - a) * frac
        }));
        out
    }))
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
            runtime: None,
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

    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.runtime = (!self.bypass)
            .then(|| Processor::new(&self.params, sample_rate, max_block))
            .flatten()
            .map(|processor| Runtime {
                processor,
                dry: if self.dry_level == 0.0 {
                    Default::default()
                } else {
                    [vec![0.0; max_block], vec![0.0; max_block]]
                },
            });
    }

    /// `left.len()` must not exceed the prepared `max_block`.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let Some(rt) = &mut self.runtime else { return };
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        let keep_dry = rt.dry[0].len() >= n && self.dry_level != 0.0;
        if keep_dry {
            rt.dry[0][..n].copy_from_slice(left);
            rt.dry[1][..n].copy_from_slice(right);
        }
        rt.processor.process(left, right);
        let (wet, dry) = (self.output_gain, self.dry_level);
        if keep_dry {
            for (out, input) in [(left, &rt.dry[0]), (right, &rt.dry[1])] {
                for (y, x) in out.iter_mut().zip(input) {
                    *y = *y * wet + *x * dry;
                }
            }
        } else if wet != 1.0 {
            left.iter_mut().for_each(|y| *y *= wet);
            right.iter_mut().for_each(|y| *y *= wet);
        }
    }
}

impl Chain {
    fn read(chunk: &Chunk) -> Result<Self> {
        let array = BParamArrayBParFX8::try_from(chunk)?;
        let slots = array
            .items
            .iter()
            .enumerate()
            .filter_map(|(slot, item)| item.as_ref().map(|c| (slot, c)))
            .map(|(slot, c)| Effect::read(slot, c).with_context(|| format!("Effect slot {slot}")))
            .collect::<Result<_>>()?;
        Ok(Self {
            slots,
            max_block: 0,
        })
    }

    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        self.max_block = max_block;
        for fx in &mut self.slots {
            fx.prepare(sample_rate, max_block);
        }
    }

    /// Processes in place. Send Levels slots pass through here; only
    /// [`ProgramFx`] routes sends. Unprepared chains pass through.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.max_block == 0 {
            return;
        }
        let n = left.len().min(right.len());
        for (l, r) in left[..n]
            .chunks_mut(self.max_block)
            .zip(right[..n].chunks_mut(self.max_block))
        {
            for fx in &mut self.slots {
                fx.process(l, r);
            }
        }
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
            send_buffers: Vec::new(),
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

    fn effects_mut(&mut self) -> impl Iterator<Item = &mut Effect> {
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

    /// Decodes the IR of every active convolution slot; one decode per file.
    pub fn load_impulses(
        &mut self,
        mut load: impl FnMut(&str, usize) -> Result<crate::audio::Sample>,
    ) {
        let mut cache: HashMap<String, Result<Arc<crate::audio::Sample>, String>> = HashMap::new();
        for fx in self.effects_mut().filter(|fx| !fx.bypass) {
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
        if self
            .buses
            .iter()
            .any(|b| b.chain.slots.iter().any(|fx| !fx.bypass))
        {
            out.push("Instrument bus effects are parsed but bus routing is not applied".into());
        }
        out
    }

    /// Allocates all DSP state. Buses are left for the engine to prepare.
    pub fn prepare(&mut self, sample_rate: f32, max_block: usize) {
        let max_block = max_block.max(1);
        self.insert.prepare(sample_rate, max_block);
        self.send.prepare(sample_rate, max_block);
        self.main.prepare(sample_rate, max_block);
        self.send_buffers = self
            .send
            .slots
            .iter()
            .map(|_| [vec![0.0; max_block], vec![0.0; max_block]])
            .collect();
    }

    /// Processes the program output in place.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let step = self.insert.max_block;
        if step == 0 {
            return;
        }
        let n = left.len().min(right.len());
        for (l, r) in left[..n].chunks_mut(step).zip(right[..n].chunks_mut(step)) {
            self.process_block(l, r);
        }
        self.main.process(&mut left[..n], &mut right[..n]);
    }

    fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        let Self {
            insert,
            send,
            send_buffers,
            ..
        } = self;
        for [l, r] in send_buffers.iter_mut() {
            l[..n].fill(0.0);
            r[..n].fill(0.0);
        }
        for fx in &mut insert.slots {
            match &fx.params {
                Params::SendLevels(levels) if !fx.bypass => {
                    for (target, [bl, br]) in send.slots.iter().zip(send_buffers.iter_mut()) {
                        let level =
                            levels.sends.get(target.slot).copied().unwrap_or(0.0) * fx.output_gain;
                        if level == 0.0 {
                            continue;
                        }
                        for (b, x) in bl[..n].iter_mut().zip(&*left) {
                            *b += x * level;
                        }
                        for (b, x) in br[..n].iter_mut().zip(&*right) {
                            *b += x * level;
                        }
                    }
                }
                _ => fx.process(left, right),
            }
        }
        // Bypassed or unimplemented send slots return nothing rather than
        // doubling their input into the dry path.
        for (fx, [bl, br]) in send.slots.iter_mut().zip(send_buffers.iter_mut()) {
            if fx.runtime.is_none() {
                continue;
            }
            let (bl, br) = (&mut bl[..n], &mut br[..n]);
            fx.process(bl, br);
            for (y, s) in left.iter_mut().zip(&*bl) {
                *y += s;
            }
            for (y, s) in right.iter_mut().zip(&*br) {
                *y += s;
            }
        }
    }
}

#[cfg(test)]
mod tests;
