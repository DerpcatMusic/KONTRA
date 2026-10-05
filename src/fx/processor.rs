//! Real-time DSP state built from a [`ProgramFx`] description.

use super::{Effect, Kind, Params, ProgramFx, blocks::Block, convolution::Convolver, params, reverb::Reverb};
use std::ops::Range;

/// Input at or below this level (−120 dBFS) counts as silence for tail tracking.
#[cfg(test)]
pub(super) const SILENCE: f32 = 1e-6;
/// Kontakt's instrument buses.
const BUSES: usize = 16;
const NO_BUS: u8 = u8::MAX;
/// Kontakt output channels a program can route to past the instrument
/// output (`$ENGINE_PAR_OUTPUT_CHANNEL` 0.., shown "Out 1"…): a mic
/// mixer's separate outputs. Groups reach channel `c` as bus
/// [`DIRECT`]` + c` ([`bus_input`](FxProcessor::bus_input)).
pub const OUTS: usize = 8;
/// [`FxProcessor::bus_input`]'s first output channel.
pub const DIRECT: u8 = BUSES as u8;

/// An effect rack a script addresses (`$NI_INSERT_BUS`, `$NI_SEND_BUS`,
/// `$NI_MAIN_BUS`, `$NI_BUS_OFFSET + n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Rack {
    Insert,
    Send,
    Main,
    Bus(u8),
}

/// A script-controllable effect or bus value. Gains are linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxParam {
    /// 1.0 bypasses the slot.
    Bypass,
    /// Output gain of the processed signal (a send slot's return level).
    Wet,
    /// Level of the unprocessed signal added after the effect.
    Dry,
    /// Send Levels slot: level into send slot `n`.
    SendLevel(u8),
    /// Bus fader; the slot is ignored.
    Volume,
    /// Bus pan, -1..=1; the slot is ignored.
    Pan,
    /// Output channel the bus plays to (0..[`OUTS`]), -1 for the
    /// instrument output; the slot is ignored.
    Output,
    /// The slot's effect as its `$EFFECT_TYPE_*` value, 0 for an empty
    /// slot. Setting the type already loaded keeps the effect as it is;
    /// another loads only in `on init`, where the effects are built
    /// ([`ProgramFx::processor_with`]): here it would allocate.
    Type,
    /// Reverb value `n` in `$ENGINE_PAR_RV2_*` order, 0..=1.
    Reverb(u8),
    /// Convolution predelay, early size, late size, as normalized script values.
    Convolution(u8),
    /// Filter, EQ and Stereo Modeller knobs in instrument racks.
    Filter(super::FilterParam),
    /// Value `n` (layout order) of a `kind` effect, 0..=1 as scripts set
    /// it (`fx::blocks` maps it onto the stored value).
    Field(Kind, u8),
}

/// Owned DSP state for one program's insert, send and main racks and its
/// instrument buses.
///
/// Built by [`ProgramFx::processor`], which allocates everything;
/// [`process`](Self::process), [`mix_buses`](Self::mix_buses) and the
/// parameter setters never allocate, lock or panic. The default processor
/// passes audio through and has no buses.
#[derive(Default)]
pub struct FxProcessor {
    insert: Vec<Stage>,
    /// Parallel send slots fed by [`Stage::Tap`]s; empty when nothing taps.
    returns: Vec<Return>,
    main: Vec<Slot>,
    buses: Vec<Bus>,
    /// Position in `buses` of each Kontakt bus, or [`NO_BUS`].
    bus_of: [u8; BUSES],
    /// The `$EFFECT_TYPE_*` of every stored slot, those without DSP too.
    types: Box<[(Rack, u8, f32)]>,
    max_block: usize,
    tempo: f32,
    /// Group taps have filled the current block's return inputs.
    group_inputs: bool,
    /// Output channels, `max_block` each; empty for the default processor.
    outs: Vec<[Box<[f32]>; 2]>,
    /// Channels written since they were last cleared.
    out_fed: u8,
    /// A block ended ([`mix_buses`](Self::mix_buses)): the next write to a
    /// channel starts the next block, clearing them all first.
    outs_done: bool,
    meter_main: [f32; 2],
}

enum Stage {
    Effect(Slot),
    /// Send Levels: level into each of `returns`, taken here.
    Tap(Tap),
}

struct Tap {
    index: u8,
    bypass: bool,
    /// The slot's output gain, applied to every level.
    gain: f32,
    levels: Box<[f32]>,
}

struct Return {
    slot: Slot,
    main: bool,
    buffer: [Box<[f32]>; 2],
    fed: bool,
}

/// Borrowed inputs to the existing instrument send returns. A group tap adds
/// its signal here without changing the group's dry insert chain.
pub(crate) struct SendInputs<'a> {
    returns: &'a mut [Return],
    offset: usize,
}

impl SendInputs<'_> {
    pub(crate) fn tap(&mut self, levels: &[f32; 8], gain: f32, start: usize, left: &[f32], right: &[f32]) {
        let at = self.offset + start;
        for ret in self.returns.iter_mut().filter(|r| !r.slot.bypass) {
            let level = levels.get(ret.slot.index as usize).copied().unwrap_or(0.0) * gain;
            if level == 0.0 { continue; }
            ret.fed |= !left.is_empty() || !right.is_empty();
            let [l, r] = &mut ret.buffer;
            mix(&mut l[at..at + left.len()], left, level);
            mix(&mut r[at..at + right.len()], right, level);
        }
    }
}

/// One active effect with its slot's wet/dry levels.
struct Slot {
    /// Rack position 0..8.
    index: u8,
    bypass: bool,
    dsp: Dsp,
    wet: f32,
    dry: f32,
    /// The input kept for the dry mix.
    dry_buffer: [Box<[f32]>; 2],
    ir_settings: Option<params::IrSettings>,
    ir_dirty: bool,
    meter: [f32; 2],
}

/// An instrument bus: groups render into `input`; its chain, fader and pan
/// feed the instrument signal ahead of the insert rack.
struct Bus {
    chain: Vec<Slot>,
    input: [Box<[f32]>; 2],
    volume: f32,
    pan: f32,
    /// Channel gains reached after the last processed frame.
    gains: [f32; 2],
    gain_start: [f32; 2],
    gain_target: [f32; 2],
    gain_remaining: usize,
    /// Kontakt bus index.
    index: u8,
    /// Output channel (0..[`OUTS`]) past the instrument output, or -1.
    output: i8,
    meter: [f32; 2],
}

enum Dsp {
    Gain(f32),
    /// Stereo Modeller: mid/side width then constant-sum balance.
    Stereo {
        width: f32,
        gains: [f32; 2],
    },
    /// The settings are kept for scripts that change one at a time.
    Reverb(Box<Reverb>, params::Reverb),
    Convolution(Box<[Convolver; 2]>),
    /// Everything in `fx::blocks`.
    Block(Box<Block>),
}

/// A replacement convolution DSP built and retired on the worker thread.
pub struct PreparedIr {
    rack: Rack,
    slot: u8,
    dsp: Dsp,
    pub(super) settings: params::IrSettings,
}

impl PreparedIr {
    pub(super) fn new(rack: Rack, slot: u8, p: &params::Convolution, rate: f32, block: usize) -> Option<Self> {
        let ir = prepare_ir(p, rate)?;
        Some(Self { rack, slot, dsp: Dsp::Convolution(Box::new(ir.map(|ch| Convolver::new(&ch, block)))), settings: params::IrSettings::from_convolution(p) })
    }
}

impl ProgramFx {
    /// Builds the DSP state for `sample_rate`; `process` calls are split into
    /// blocks of at most `max_block` frames. Unimplemented slots are left
    /// out; bypassed ones are built so scripts can switch them on. Every
    /// stored bus gets input buffers of `max_block`.
    pub fn processor(&self, sample_rate: f32, max_block: usize) -> FxProcessor {
        self.processor_sends(sample_rate, max_block, false)
    }

    pub(super) fn processor_sends(&self, sample_rate: f32, max_block: usize, group_sends: bool) -> FxProcessor {
        let max_block = max_block.max(1);
        let build = |fx: &Effect| Slot::new(fx, sample_rate, max_block);
        let tapped = self
            .insert
            .slots
            .iter()
            .any(|fx| matches!(fx.params, Params::SendLevels(_)));
        // Unfed send slots would only ever process silence.
        let sends: Vec<_> = if tapped || group_sends {
            self.send.slots.iter().filter_map(build).collect()
        } else {
            Vec::new()
        };
        let insert = self
            .insert
            .slots
            .iter()
            .filter_map(|fx| match &fx.params {
                Params::SendLevels(levels) => Some(Stage::Tap(Tap {
                    index: fx.slot as u8,
                    bypass: fx.bypass,
                    gain: fx.output_gain,
                    levels: sends
                        .iter()
                        .map(|s| levels.sends.get(s.index as usize).copied().unwrap_or(0.0))
                        .collect(),
                })),
                _ => build(fx).map(Stage::Effect),
            })
            .collect();
        let returns = sends
            .into_iter()
            .map(|slot| Return {
                main: self.send_outputs.iter().find(|output| output.slot == slot.index as usize)
                    .is_none_or(|output| output.channel == -1),
                slot,
                buffer: [zeros(max_block), zeros(max_block)],
                fed: false,
            })
            .collect();
        let mut bus_of = [NO_BUS; BUSES];
        let buses = self
            .buses
            .iter()
            .filter(|b| b.index < BUSES)
            .enumerate()
            .map(|(i, b)| {
                bus_of[b.index] = i as u8;
                let chain: Vec<_> = b.chain.slots.iter().filter_map(build).collect();
                Bus {
                    chain,
                    input: [zeros(max_block), zeros(max_block)],
                    volume: b.volume,
                    pan: b.pan.clamp(-1.0, 1.0),
                    gains: balance(b.volume, b.pan),
                    gain_start: balance(b.volume, b.pan),
                    gain_target: balance(b.volume, b.pan),
                    gain_remaining: 0,
                    index: b.index as u8,
                    output: channel(b.output as f32),
                    meter: [0.; 2],
                }
            })
            .collect();
        let racks = [(Rack::Insert, &self.insert), (Rack::Send, &self.send), (Rack::Main, &self.main)];
        let racks = racks.into_iter().chain(
            (self.buses.iter())
                .filter(|b| b.index < BUSES)
                .map(|b| (Rack::Bus(b.index as u8), &b.chain)),
        );
        let types = racks
            .flat_map(|(rack, chain)| chain.slots.iter().map(move |fx| (rack, fx.slot as u8, f32::from(fx.kind.ser_id()))))
            .collect();
        FxProcessor {
            insert,
            returns,
            types,
            main: self.main.slots.iter().filter_map(build).collect(),
            buses,
            bus_of,
            max_block,
            tempo: 120.0,
            group_inputs: false,
            outs: (0..OUTS).map(|_| [zeros(max_block), zeros(max_block)]).collect(),
            out_fed: 0,
            outs_done: false,
            meter_main: [0.; 2],
        }
    }
}

impl FxProcessor {
    /// Host tempo reaches every rack, including instrument buses. Repeated
    /// unchanged transport snapshots do not walk or retune any effect.
    pub fn set_tempo(&mut self, tempo: f32) {
        if !tempo.is_finite() || tempo <= 0.0 || tempo == self.tempo { return }
        self.tempo = tempo;
        let insert = self.insert.iter_mut().filter_map(|s| match s {
            Stage::Effect(s) => Some(s), Stage::Tap(_) => None,
        });
        for s in insert.chain(self.returns.iter_mut().map(|r| &mut r.slot))
            .chain(&mut self.main).chain(self.buses.iter_mut().flat_map(|b| &mut b.chain)) {
            if let Dsp::Block(b) = &mut s.dsp { b.set_tempo(tempo); }
        }
    }

    /// Whether the processor has no program or bus processing to advance.
    pub fn is_empty(&self) -> bool {
        self.insert.is_empty() && self.returns.is_empty() && self.main.is_empty() && self.buses.is_empty()
    }

    /// Silences every tail without allocating.
    pub fn clear(&mut self) {
        let dsp = self.insert.iter_mut().filter_map(|stage| match stage {
            Stage::Effect(slot) => Some(&mut slot.dsp),
            Stage::Tap(_) => None,
        });
        dsp.chain(self.returns.iter_mut().map(|r| &mut r.slot.dsp))
            .chain(self.main.iter_mut().map(|s| &mut s.dsp))
            .chain(
                self.buses
                    .iter_mut()
                    .flat_map(|b| b.chain.iter_mut().map(|s| &mut s.dsp)),
            )
            .for_each(Dsp::clear);
        for bus in &mut self.buses {
            bus.input.iter_mut().for_each(|b| b.fill(0.0));
        }
        self.group_inputs = false;
        self.returns.iter_mut().for_each(|r| r.fed = false);
        self.returns.iter_mut().flat_map(|r| &mut r.buffer).for_each(|b| b.fill(0.0));
        self.outs.iter_mut().flatten().for_each(|b| b.fill(0.0));
        self.out_fed = 0;
    }

    /// Processes the program output in place, including quiet input and tails.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let group_inputs = std::mem::take(&mut self.group_inputs);
        if self.is_empty() {
            return;
        }
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        for (l, r) in left
            .chunks_mut(self.max_block)
            .zip(right.chunks_mut(self.max_block))
        {
            self.process_block(l, r, group_inputs, 0, true);
        }
    }

    /// Maximum voice/send block supported by the fixed worker buffers.
    pub(crate) fn block_size(&self) -> usize {
        if self.max_block == 0 { usize::MAX } else { self.max_block }
    }

    /// Clear fixed return and direct-output inputs before a render block.
    pub(crate) fn begin_group_sends(&mut self, n: usize) {
        self.fresh_outs();
        for ret in &mut self.returns {
            ret.buffer.iter_mut().for_each(|b| b[..n].fill(0.0));
            ret.fed = false;
        }
        self.group_inputs = true;
    }

    /// Process one event interval without consuming the other intervals' sends.
    pub(crate) fn process_group_segment(&mut self, left: &mut [f32], right: &mut [f32], offset: usize, input_work: bool) -> bool {
        if !self.is_empty() {
            self.process_block(left, right, true, offset, input_work)
        } else {
            input_work
        }
    }

    /// Mirror voice_inputs/mix_buses fallback routing without touching buffers.
    pub(crate) fn feeds_program(&self, bus: Option<u8>) -> bool {
        match bus {
            None => true,
            Some(b) if b >= DIRECT => usize::from(b - DIRECT) >= self.outs.len(),
            Some(b) => self.bus(Rack::Bus(b)).is_none_or(|bus|
                bus.output < 0 || bus.output as usize >= self.outs.len()),
        }
    }

    /// Keep all direct-output intervals readable until the next render block.
    pub(crate) fn end_group_sends(&mut self) {
        self.group_inputs = false;
        self.outs_done = true;
    }

    /// Disjoint output and send-input borrows for a voice's current segment.
    pub(crate) fn voice_inputs(&mut self, bus: Option<u8>, frames: Range<usize>)
        -> (Option<(&mut [f32], &mut [f32])>, SendInputs<'_>)
    {
        if bus.is_some_and(|b| b >= DIRECT) { self.fresh_outs(); }
        let input = bus.and_then(|b| {
            if let Some(c) = b.checked_sub(DIRECT) {
                let [l, r] = self.outs.get_mut(c as usize)?;
                self.out_fed |= 1 << c;
                Some((l.get_mut(frames.clone())?, r.get_mut(frames.clone())?))
            } else {
                let bus = self.buses.get_mut(*self.bus_of.get(b as usize)? as usize)?;
                let [l, r] = &mut bus.input;
                Some((l.get_mut(frames.clone())?, r.get_mut(frames.clone())?))
            }
        });
        (input, SendInputs { returns: &mut self.returns, offset: frames.start })
    }

    /// The channel gains of bus `bus` when it only passes its input to the
    /// instrument output through a fader at rest: no effects, no ramp this
    /// block. Voices can mix straight to the output through them, the same sum.
    pub fn bus_gains(&self, bus: u8) -> Option<[f32; 2]> {
        let bus = self.buses.get(*self.bus_of.get(bus as usize)? as usize)?;
        (bus.output < 0 && bus.chain.is_empty() && bus.gain_remaining == 0
            && bus.gains == balance(bus.volume, bus.pan)).then_some(bus.gains)
    }

    /// `frames` of Kontakt bus `bus`'s input for the current block (within
    /// `max_block`); `None` when the program has no such bus.
    /// Buses [`DIRECT`]` + c` are output channel `c`.
    pub fn bus_input(&mut self, bus: u8, frames: Range<usize>) -> Option<(&mut [f32], &mut [f32])> {
        self.voice_inputs(Some(bus), frames).0
    }

    /// Runs every bus on its first `left.len()`
    /// input frames (at most `max_block`), adds it to `left`/`right` through
    /// its fader and pan, and clears the inputs for the next block.
    /// A bus routed to an output channel plays there instead.
    pub fn begin_meter_block(&mut self) {
        self.meter_main = [0.; 2];
        for stage in &mut self.insert { if let Stage::Effect(slot) = stage { slot.meter = [0.; 2]; } }
        for slot in &mut self.main { slot.meter = [0.; 2]; }
        for ret in &mut self.returns { ret.slot.meter = [0.; 2]; }
        for bus in &mut self.buses { bus.meter = [0.; 2]; for slot in &mut bus.chain { slot.meter = [0.; 2]; } }
    }
    pub fn level_meter(&self, [group, index, channel, generic]: [i32; 4]) -> Option<f32> {
        if group != -1 || !(0..2).contains(&channel) { return None; }
        let channel = channel as usize;
        if index < 0 {
            return if generic >= 0 { self.bus(Rack::Bus(generic as u8)).map(|b| b.meter[channel]) } else { Some(self.meter_main[channel]) };
        }
        let rack = match generic { -2 => Rack::Main, -1 => Rack::Insert, 0..16 => Rack::Bus(generic as u8), _ => return None };
        let slot = self.slot(rack, index.try_into().ok()?)?;
        if slot.bypass { return Some(0.); }
        if let Dsp::Block(block) = &slot.dsp
            && let Some(gain) = block.reduction(channel)
        {
            return Some(1. - gain);
        }
        Some(slot.meter[channel])
    }
    pub fn mix_buses(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.mix_buses_segment(left, right, 0);
        self.outs_done = true;
    }

    pub(crate) fn mix_buses_segment(&mut self, left: &mut [f32], right: &mut [f32], offset: usize) -> bool {
        let n = left.len().min(right.len()).min(self.max_block.saturating_sub(offset));
        if n == 0 { return false; }
        self.fresh_outs();
        let mut work = false;
        for bus in &mut self.buses {
            if bus.output < 0 || bus.output as usize >= self.outs.len() {
                work |= bus.chain.iter().any(|slot| slot.work(false));
            }
            let [il, ir] = &mut bus.input;
            let (il, ir) = (&mut il[offset..offset + n], &mut ir[offset..offset + n]);
            for slot in &mut bus.chain {
                slot.process(il, ir);
            }
            let target = balance(bus.volume, bus.pan);
            if target != bus.gain_target {
                bus.gain_start = bus.gains;
                bus.gain_target = target;
                // Retain the established full prepared-block linear ramp.
                // Engine prepares 128 frames; this local receiver policy is
                // independent of incoming fragments and claims no native law.
                bus.gain_remaining = self.max_block;
            }
            let start = bus.gain_start;
            let step = [
                (target[0] - start[0]) / self.max_block as f32,
                (target[1] - start[1]) / self.max_block as f32,
            ];
            let (to_l, to_r) = match self.outs.get_mut(bus.output as usize) {
                Some([ol, or]) if bus.output >= 0 => {
                    self.out_fed |= 1 << bus.output;
                    (&mut ol[offset..offset + n], &mut or[offset..offset + n])
                }
                _ => (&mut left[..n], &mut right[..n]),
            };
            for ((l, r), (x, y)) in to_l
                .iter_mut()
                .zip(to_r.iter_mut())
                .zip(il.iter_mut().zip(ir.iter_mut()))
            {
                bus.meter[0] = bus.meter[0].max((*x * bus.gains[0]).abs());
                bus.meter[1] = bus.meter[1].max((*y * bus.gains[1]).abs());
                *l += *x * bus.gains[0];
                *r += *y * bus.gains[1];
                (*x, *y) = (0.0, 0.0);
                if bus.gain_remaining > 0 {
                    bus.gain_remaining -= 1;
                    let elapsed = (self.max_block - bus.gain_remaining) as f32;
                    bus.gains = if bus.gain_remaining == 0 { target }
                        else { [start[0] + step[0] * elapsed, start[1] + step[1] * elapsed] };
                }
            }
        }
        work
    }

    /// Clear the output channels once a new block writes to them.
    fn fresh_outs(&mut self) {
        if std::mem::take(&mut self.outs_done) {
            for (c, out) in self.outs.iter_mut().enumerate() {
                if self.out_fed & 1 << c != 0 {
                    out.iter_mut().for_each(|b| b.fill(0.0));
                }
            }
            self.out_fed = 0;
        }
    }

    /// Read only the just-rendered interval, without resetting the outer FX block.
    pub(crate) fn direct_outs_range(&self, frames: Range<usize>) -> impl Iterator<Item = (usize, &[f32], &[f32])> {
        (self.outs.iter().enumerate())
            .filter(move |(c, _)| self.out_fed & 1 << c != 0)
            .map(move |(c, [l, r])| (c, &l[frames.clone()], &r[frames.clone()]))
    }

    /// What the last block played to each output channel past the
    /// instrument output: `(channel, left, right)`, first `n` frames. Read
    /// it after the block, before the next.
    pub fn direct_outs(&self, n: usize) -> impl Iterator<Item = (usize, &[f32], &[f32])> {
        let n = n.min(self.max_block);
        (self.outs.iter().enumerate())
            .filter(move |(c, _)| self.out_fed & 1 << c != 0)
            .map(move |(c, [l, r])| (c, &l[..n], &r[..n]))
    }

    /// The first bus routed to each output channel, as its Kontakt index.
    pub fn routed(&self) -> [Option<u8>; OUTS] {
        let mut to = [None; OUTS];
        for bus in &self.buses {
            if let Some(t) = to.get_mut(bus.output as usize).filter(|_| bus.output >= 0) {
                t.get_or_insert(bus.index);
            }
        }
        to
    }

    /// Set a script-controllable value; false when this processor does not
    /// hold it (unknown or unimplemented slots).
    pub fn set_param(&mut self, rack: Rack, slot: u8, param: FxParam, value: f32) -> bool {
        let gain = value.max(0.0);
        if let FxParam::Volume | FxParam::Pan | FxParam::Output = param {
            let Some(bus) = self.bus_mut(rack) else {
                return false;
            };
            match param {
                FxParam::Volume => bus.volume = gain,
                FxParam::Output => bus.output = channel(value),
                _ => bus.pan = value.clamp(-1.0, 1.0),
            }
            return true;
        }
        if param == FxParam::Type {
            return self.param(rack, slot, param) == Some(value);
        }
        let returns = &self.returns;
        if let Some(tap) = tap_mut(&mut self.insert, rack, slot) {
            match param {
                FxParam::Bypass => tap.bypass = value != 0.0,
                FxParam::Wet => tap.gain = gain,
                FxParam::SendLevel(n) => {
                    let level = returns
                        .iter()
                        .position(|r| r.slot.index == n)
                        .and_then(|j| tap.levels.get_mut(j));
                    let Some(level) = level else {
                        return false;
                    };
                    *level = gain;
                }
                _ => return false,
            }
            return true;
        }
        let Some(s) = self.slot_mut(rack, slot) else {
            return false;
        };
        match (param, &mut s.dsp) {
            (FxParam::Bypass, _) => s.bypass = value != 0.0,
            (FxParam::Wet, _) => s.wet = gain,
            (FxParam::Dry, _) => s.dry = gain,
            (FxParam::Convolution(n), _) => {
                let Some(settings) = s.ir_settings.as_mut() else { return false };
                let before = *settings;
                if !settings.set(n, value) { return false }
                s.ir_dirty |= before != *settings;
            }
            (FxParam::Reverb(n), Dsp::Reverb(rv, p)) => {
                let Some(field) = p.field(n) else {
                    return false;
                };
                *field = value.clamp(0.0, 1.0);
                rv.set(p);
            }
            (FxParam::Filter(knob), Dsp::Block(b)) => return b.set_filter(knob, value),
            (FxParam::Filter(super::FilterParam::Spread), Dsp::Stereo { width, .. }) => *width = 1.0 + value.clamp(-1.0, 1.0),
            (FxParam::Filter(super::FilterParam::Pan), Dsp::Stereo { gains, .. }) => *gains = balance(1.0, value),
            (FxParam::Field(kind, n), Dsp::Block(b)) => return b.set(kind, n, value),
            _ => return false,
        }
        true
    }

    /// Swap only the convolution DSP; preserve all current routing and gains.
    pub fn replace_ir(&mut self, mut ir: PreparedIr) -> Result<PreparedIr, PreparedIr> {
        let Some(slot) = self.slot_mut(ir.rack, ir.slot) else { return Err(ir) };
        if !matches!(slot.dsp, Dsp::Convolution(_)) { return Err(ir) }
        std::mem::swap(&mut slot.dsp, &mut ir.dsp);
        slot.ir_settings = Some(ir.settings);
        slot.ir_dirty = false;
        Ok(ir)
    }

    pub fn ir_settings(&self, rack: Rack, slot: u8) -> Option<params::IrSettings> {
        self.slot(rack, slot)?.ir_settings
    }

    pub(super) fn init_ir_settings(&mut self, rack: Rack, slot: u8, settings: params::IrSettings) {
        if let Some(s) = self.slot_mut(rack, slot).filter(|s| s.ir_settings.is_some()) {
            s.ir_settings = Some(settings.inherit_flags(s.ir_settings.unwrap()));
            s.ir_dirty = false;
        }
    }

    pub fn ir_request_settings(&mut self, rack: Rack, slot: u8) -> Option<params::IrSettings> {
        let s = self.slot_mut(rack, slot)?;
        s.ir_dirty = false;
        s.ir_settings
    }

    pub fn mark_ir_changed(&mut self, rack: Rack, slot: u8) {
        if let Some(s) = self.slot_mut(rack, slot) { s.ir_dirty = s.ir_settings.is_some(); }
    }

    /// Coalesce writes made during the block into one worker request per slot.
    pub fn take_ir_change(&mut self) -> Option<(Rack, u8, params::IrSettings)> {
        let slots = self.insert.iter_mut().filter_map(|s| match s {
            Stage::Effect(s) => Some((Rack::Insert, s)), Stage::Tap(_) => None,
        }).chain(self.returns.iter_mut().map(|r| (Rack::Send, &mut r.slot)))
            .chain(self.main.iter_mut().map(|s| (Rack::Main, s)))
            .chain(self.buses.iter_mut().flat_map(|b| {
                let rack = Rack::Bus(b.index);
                b.chain.iter_mut().map(move |s| (rack, s))
            }));
        for (rack, s) in slots {
            if s.ir_dirty && let Some(settings) = s.ir_settings {
                s.ir_dirty = false;
                return Some((rack, s.index, settings));
            }
        }
        None
    }

    pub(crate) fn restore_delay(&mut self, saved: &super::DelayState) -> bool {
        let Some(slot) = self.slot_mut(saved.rack, saved.slot) else { return false };
        let Dsp::Block(block) = &mut slot.dsp else { return false };
        block.restore_delay(saved)
    }

    pub(crate) fn delay_fields(&self, rack: Rack, slot: u8) -> Option<(super::blocks::Fields, f32)> {
        let Dsp::Block(block) = &self.slot(rack, slot)?.dsp else { return None };
        Some((block.delay_fields()?, self.tempo))
    }

    /// Current value of a script-controllable parameter.
    pub fn param(&self, rack: Rack, slot: u8, param: FxParam) -> Option<f32> {
        if let FxParam::Volume | FxParam::Pan | FxParam::Output = param {
            let bus = self.bus(rack)?;
            return Some(match param {
                FxParam::Volume => bus.volume,
                FxParam::Output => f32::from(bus.output),
                _ => bus.pan,
            });
        }
        if param == FxParam::Type {
            let stored = self.types.iter().find(|t| (t.0, t.1) == (rack, slot));
            let rack_exists = !matches!(rack, Rack::Bus(_)) || self.bus(rack).is_some();
            // `$EFFECT_TYPE_NONE` for an empty slot.
            return stored.map(|t| t.2).or((rack_exists && slot < 8).then_some(0.0));
        }
        let tap = self.insert.iter().find_map(|stage| match stage {
            Stage::Tap(tap) if rack == Rack::Insert && tap.index == slot => Some(tap),
            _ => None,
        });
        if let Some(tap) = tap {
            return match param {
                FxParam::Bypass => Some(f32::from(tap.bypass)),
                FxParam::Wet => Some(tap.gain),
                FxParam::SendLevel(n) => {
                    let j = self.returns.iter().position(|r| r.slot.index == n)?;
                    tap.levels.get(j).copied()
                }
                _ => None,
            };
        }
        let s = self.slot(rack, slot)?;
        match (param, &s.dsp) {
            (FxParam::Bypass, _) => Some(f32::from(s.bypass)),
            (FxParam::Wet, _) => Some(s.wet),
            (FxParam::Dry, _) => Some(s.dry),
            (FxParam::Convolution(n), _) => s.ir_settings?.value(n),
            (FxParam::Reverb(n), Dsp::Reverb(_, p)) => { *p }.field(n).copied(),
            (FxParam::Filter(knob), Dsp::Block(b)) => b.filter_param(knob),
            (FxParam::Filter(super::FilterParam::Spread), Dsp::Stereo { width, .. }) => Some(*width - 1.0),
            (FxParam::Filter(super::FilterParam::Pan), Dsp::Stereo { gains, .. }) => Some(gains[1] - gains[0]),
            (FxParam::Field(kind, n), Dsp::Block(b)) => b.get(kind, n),
            _ => None,
        }
    }

    fn bus(&self, rack: Rack) -> Option<&Bus> {
        let Rack::Bus(b) = rack else { return None };
        self.buses.get(*self.bus_of.get(b as usize)? as usize)
    }

    fn bus_mut(&mut self, rack: Rack) -> Option<&mut Bus> {
        let Rack::Bus(b) = rack else { return None };
        self.buses.get_mut(*self.bus_of.get(b as usize)? as usize)
    }

    fn slot(&self, rack: Rack, index: u8) -> Option<&Slot> {
        let find = |s: &&Slot| s.index == index;
        match rack {
            Rack::Insert => self
                .insert
                .iter()
                .filter_map(|stage| match stage {
                    Stage::Effect(slot) => Some(slot),
                    Stage::Tap(_) => None,
                })
                .find(find),
            Rack::Send => self.returns.iter().map(|r| &r.slot).find(find),
            Rack::Main => self.main.iter().find(find),
            Rack::Bus(_) => self.bus(rack)?.chain.iter().find(find),
        }
    }

    fn slot_mut(&mut self, rack: Rack, index: u8) -> Option<&mut Slot> {
        let find = |s: &&mut Slot| s.index == index;
        match rack {
            Rack::Insert => self
                .insert
                .iter_mut()
                .filter_map(|stage| match stage {
                    Stage::Effect(slot) => Some(slot),
                    Stage::Tap(_) => None,
                })
                .find(find),
            Rack::Send => self.returns.iter_mut().map(|r| &mut r.slot).find(find),
            Rack::Main => self.main.iter_mut().find(find),
            Rack::Bus(_) => self.bus_mut(rack)?.chain.iter_mut().find(find),
        }
    }

    /// `left.len() == right.len() <= max_block`.
    fn process_block(&mut self, left: &mut [f32], right: &mut [f32], group_inputs: bool, offset: usize, mut work: bool) -> bool {
        let n = left.len();
        let frames = offset..offset + n;
        if !group_inputs {
            for ret in &mut self.returns {
                ret.fed = false;
                for buffer in &mut ret.buffer {
                    buffer[frames.clone()].fill(0.0);
                }
            }
        }
        for stage in &mut self.insert {
            match stage {
                Stage::Effect(slot) => {
                    slot.process(left, right);
                    work = slot.work(work);
                }
                Stage::Tap(tap) if !tap.bypass => {
                    for (ret, &level) in self.returns.iter_mut().zip(tap.levels.iter()) {
                        let level = level * tap.gain;
                        if level != 0.0 {
                            ret.fed |= work && n != 0;
                            let [bl, br] = &mut ret.buffer;
                            mix(&mut bl[frames.clone()], left, level);
                            mix(&mut br[frames.clone()], right, level);
                        }
                    }
                }
                Stage::Tap(_) => {}
            }
        }
        // Bypassed send slots return nothing (audits/EFFECTS.md).
        for ret in &mut self.returns {
            let fed = std::mem::take(&mut ret.fed);
            if ret.slot.bypass { continue; }
            let [bl, br] = &mut ret.buffer;
            let (bl, br) = (&mut bl[frames.clone()], &mut br[frames.clone()]);
            ret.slot.process(bl, br);
            if ret.main {
                work |= ret.slot.work(fed);
                mix(left, bl, 1.0);
                mix(right, br, 1.0);
            }
        }
        for slot in &mut self.main {
            slot.process(left, right);
            work = slot.work(work);
        }
        self.meter_main[0] = self.meter_main[0].max(left.iter().copied().map(f32::abs).fold(0., f32::max));
        self.meter_main[1] = self.meter_main[1].max(right.iter().copied().map(f32::abs).fold(0., f32::max));
        work
    }
}

fn tap_mut(insert: &mut [Stage], rack: Rack, index: u8) -> Option<&mut Tap> {
    insert.iter_mut().find_map(|stage| match stage {
        Stage::Tap(tap) if rack == Rack::Insert && tap.index == index => Some(tap),
        _ => None,
    })
}

/// Balance law shared with the engine: the far side attenuates linearly.
fn balance(gain: f32, pan: f32) -> [f32; 2] {
    [gain * (1.0 - pan.max(0.0)), gain * (1.0 + pan.min(0.0))]
}

impl Slot {
    /// Input receipts include quiet buffers. Gainer and the plain stereo matrix
    /// have no independent work; other enabled DSP retain conservative tails.
    fn work(&self, input: bool) -> bool {
        input || (!self.bypass && !matches!(self.dsp, Dsp::Gain(_) | Dsp::Stereo { .. }))
    }

    fn new(fx: &Effect, sample_rate: f32, max_block: usize) -> Option<Self> {
        let dsp = Dsp::new(&fx.params, sample_rate, max_block).or_else(|| Block::new(fx, sample_rate).map(Dsp::Block))?;
        // A filter or EQ storing both levels at 0 would mute the rack;
        // Kontakt plays ANALOG STRINGS' active insert EQ stored so, with no
        // script setting them: read them as unset (unity).
        let unset = matches!(fx.params, Params::Filter(_) | Params::Eq(_)) && fx.output_gain == 0.0 && fx.dry_level == 0.0;
        // Scripts may raise the dry level later, so the buffer always exists.
        Some(Self {
            index: fx.slot as u8,
            bypass: fx.bypass,
            dsp,
            wet: if unset { 1.0 } else { fx.output_gain },
            dry: fx.dry_level,
            dry_buffer: [zeros(max_block), zeros(max_block)],
            ir_settings: match &fx.params { Params::Convolution(p) => Some(params::IrSettings::from_convolution(p)), _ => None },
            ir_dirty: false,
            meter: [0.; 2],
        })
    }

    /// `left.len() == right.len() <= max_block`.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.bypass {
            return;
        }
        let n = left.len();
        let keep_dry = self.dry != 0.0;
        if keep_dry {
            self.dry_buffer[0][..n].copy_from_slice(left);
            self.dry_buffer[1][..n].copy_from_slice(right);
        }
        self.dsp.process(left, right);
        let (wet, dry) = (self.wet, self.dry);
        if keep_dry {
            for (out, input) in [(&mut *left, &self.dry_buffer[0]), (&mut *right, &self.dry_buffer[1])] {
                for (y, x) in out.iter_mut().zip(input.iter()) {
                    *y = *y * wet + *x * dry;
                }
            }
        } else if wet != 1.0 {
            left.iter_mut().for_each(|y| *y *= wet);
            right.iter_mut().for_each(|y| *y *= wet);
        }
        self.meter[0] = self.meter[0].max(left.iter().copied().map(f32::abs).fold(0., f32::max));
        self.meter[1] = self.meter[1].max(right.iter().copied().map(f32::abs).fold(0., f32::max));
    }
}

impl Dsp {
    fn new(params: &Params, sample_rate: f32, max_block: usize) -> Option<Self> {
        Some(match params {
            Params::Gainer(p) => Dsp::Gain(p.gain),
            Params::StereoModeller(p) => Dsp::Stereo {
                width: (1.0 + p.spread).clamp(0.0, 2.0),
                gains: [(1.0 - p.pan).clamp(0.0, 1.0), (1.0 + p.pan).clamp(0.0, 1.0)],
            },
            Params::Reverb(p) => Dsp::Reverb(Box::new(Reverb::new(p, sample_rate)), *p),
            Params::Convolution(p) => {
                let ir = prepare_ir(p, sample_rate)?;
                Dsp::Convolution(Box::new(ir.map(|ch| Convolver::new(&ch, max_block))))
            }
            _ => return None,
        })
    }

    fn clear(&mut self) {
        match self {
            Dsp::Gain(_) | Dsp::Stereo { .. } => {}
            Dsp::Reverb(rv, _) => rv.clear(),
            Dsp::Convolution(conv) => conv.iter_mut().for_each(Convolver::clear),
            Dsp::Block(b) => b.clear(),
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            Dsp::Gain(g) => {
                left.iter_mut()
                    .chain(right.iter_mut())
                    .for_each(|v| *v *= *g);
            }
            Dsp::Stereo { width, gains } => {
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    let (mid, side) = (0.5 * (*l + *r), 0.5 * (*l - *r) * *width);
                    *l = (mid + side) * gains[0];
                    *r = (mid - side) * gains[1];
                }
            }
            Dsp::Reverb(rv, _) => rv.process(left, right),
            Dsp::Block(b) => b.process(left, right),
            Dsp::Convolution(conv) => {
                conv[0].process(left);
                conv[1].process(right);
            }
        }
    }
}

fn zeros(len: usize) -> Box<[f32]> {
    vec![0.0; len].into_boxed_slice()
}

/// `out += input * level`.
fn mix(out: &mut [f32], input: &[f32], level: f32) {
    for (y, x) in out.iter_mut().zip(input) {
        *y += x * level;
    }
}

/// Resamples (linear), stretches and predelays the IR for `sample_rate`.
fn prepare_ir(p: &params::Convolution, sample_rate: f32) -> Option<[Vec<f32>; 2]> {
    let ir = &p.ir.as_ref()?.0;
    let reverse = p.reversed();
    // Non-unit Size currently resamples the kernel instead of native time
    // stretching. Unsupported split configurations retain uniform late size.
    let ratio = ir.rate as f32 / sample_rate / p.late.length_ratio.clamp(0.5, 1.5);
    let len = (ir.frames.len() as f32 / ratio).ceil() as usize;
    let pre = (p.predelay_ms.max(0.0) * 0.001 * sample_rate) as usize;
    // ponytail: linear interpolation aliases slightly on rate changes; use a
    // windowed-sinc resampler if IR brightness at 44.1k<->48k ever matters.
    let mut shaped = std::array::from_fn(|ch| {
        (0..len.max(1)).map(|i| {
            let x = i as f32 * ratio;
            let (j, frac) = (x as usize, x.fract());
            let sample = |j: usize| {
                let index = if reverse { ir.frames.len().checked_sub(j + 1) } else { Some(j) };
                index.and_then(|j| ir.frames.get(j)).map_or(0.0, |f| f[ch])
            };
            let a = sample(j);
            let b = sample(j + 1);
            a + (b - a) * frac
        }).collect()
    });
    let unequal = p.early.low_cut_hz != p.late.low_cut_hz || p.early.high_cut_hz != p.late.high_cut_hz;
    if p.explicit_split_supported() && ir.rate as f32 == sample_rate && !ir.frames.is_empty() {
        split_ir(&mut shaped, p, sample_rate);
    } else if !unequal {
        crate::engine::filter::filter_ir(&mut shaped, p.late.low_cut_hz, p.late.high_cut_hz, sample_rate);
    }
    if p.envelope_active() && p.envelope_supported() {
        // Native knots are sorted in time, rounded over the shaped IR's
        // duration, and interpolated in amplitude after converting from dB.
        // Predelay is independent: the envelope acts on the response only.
        let mut knots: [(f32, f32); 8] = std::array::from_fn(|n|
            (p.curve_x[n], (p.curve_db[n] * 0.05 * std::f32::consts::LN_10).exp()));
        knots.sort_by(|a, b| a.0.total_cmp(&b.0));
        let frames = shaped[0].len();
        for pair in knots.windows(2) {
            let start = (frames as f32 * pair[0].0.clamp(0.0, 1.0) + 0.5) as usize;
            let end = (frames as f32 * pair[1].0.clamp(0.0, 1.0) + 0.5) as usize;
            // Collapsed knots write no frames; outside the knots is unchanged.
            for n in start..end {
                let gain = pair[0].1 + (pair[1].1 - pair[0].1) * (n - start) as f32 / (end - start) as f32;
                shaped.iter_mut().for_each(|ch| ch[n] *= gain);
            }
        }
    }
    for channel in &mut shaped {
        channel.resize(channel.len() + pre, 0.0);
        channel.rotate_right(pre);
    }
    if p.auto_gain() {
        // Native Auto Gain uses the loudest prepared IR channel's energy,
        // not RMS or peak normalization. Baking the wet gain into this
        // linear kernel leaves the slot's dry signal unchanged.
        let energy = shaped.iter().map(|ch| ch.iter().map(|x| x * x).sum::<f32>()).fold(0.0f32, f32::max);
        let gain = if energy >= 0.001 { (0.5 / energy).sqrt().min(2.0) } else { 1.0 };
        shaped.iter_mut().flatten().for_each(|x| *x *= gain);
    }
    Some(shaped)
}

/// Native unit-size ER/LR preparation: two full filtered responses, a 50 ms
/// cos-squared overlap, and the original source duration. Worker thread only.
fn split_ir(ir: &mut [Vec<f32>; 2], p: &params::Convolution, rate: f32) {
    let frames = ir[0].len();
    let mut late = ir.clone();
    crate::engine::filter::filter_ir(ir, p.early.low_cut_hz, p.early.high_cut_hz, rate);
    crate::engine::filter::filter_ir(&mut late, p.late.low_cut_hz, p.late.high_cut_hz, rate);
    for channel in ir.iter_mut().chain(&mut late) { channel.truncate(frames); }
    let boundary = ((frames as f32 * p.early_late_xpoint() + 0.5) as usize).min(frames);
    let boundary = if p.reversed() { frames - boundary } else { boundary };
    let transition = (rate * 0.05 + 0.5) as usize;
    let raw_start = (boundary as f32 - transition as f32 * 0.5 + 0.5) as isize;
    let start = raw_start.max(0) as usize;
    let end = ((boundary as f32 + transition as f32 * 0.5 + 0.5) as usize).min(frames);
    for (early, late) in ir.iter_mut().zip(late) {
        for n in start..end {
            let phase = (n as isize - raw_start) as f32 / transition as f32;
            let weight = (phase * std::f32::consts::FRAC_PI_2).cos().powi(2);
            early[n] = early[n] * weight + late[n] * (1.0 - weight);
        }
        early[end..].copy_from_slice(&late[end..]);
    }
}

/// An output channel, 0..[`OUTS`], or -1 for the instrument output.
fn channel(value: f32) -> i8 {
    if (0.0..OUTS as f32).contains(&value) { value as i8 } else { -1 }
}

#[cfg(test)]
mod bus_clock_tests {
    use super::*;
    use crate::{audio::Sample, engine::{Ahdsr, Bank, Engine, MAX_BLOCK}, import::{Group, Zone}};
    use std::sync::Arc;

    #[test]
    fn bus_faders_retarget_mute_and_ring_after_bank_removal_on_frame_clocks_without_heap() {
        const FRAMES: usize = 2304;
        const SOURCE: usize = 1280;
        // A processor's prepared128-frame receiver ramp is the existing core
        // policy, not an asserted Kontakt timing law or negotiated host size.
        let timeline = [(0, 1., 0.), (257, 0.2, 0.), (311, 0.8, -0.5),
            (399, 0.8, 0.75), (511, 0., 0.75), (769, 1., 0.75),
            (1151, 1., 0.), (1407, 0., 0.), (1601, 1., 0.), (FRAMES, 1., 0.)];
        let bank = |routed: bool| Box::new(Bank::from_samples(vec![Group::default()], vec![Zone::default()],
            vec![(Default::default(), Sample { rate: 48_000, frames: vec![[0.25, -0.125]; SOURCE] })]).unwrap()
            .with_prepared_settings(|settings| {
                settings[0].envelope = Some(Ahdsr::UNITY);
                settings[0].bus = routed.then_some(0);
            }).unwrap());
        let echo = || {
            let mut bytes: Vec<_> = [-1_f32, 0., 0., 1., 20., 24_000., 1., 20., 24_000., -1.]
                .into_iter().flat_map(f32::to_le_bytes).collect();
            bytes.extend([0, 0, 1, 1, 0]); bytes.extend([0; 8]); bytes.extend(0_i32.to_le_bytes());
            let mut parameters = params::parse(Kind::Convolution, &bytes);
            let Params::Convolution(p) = &mut parameters else { unreachable!() };
            let mut impulse = vec![[0.; 2]; 513]; impulse[0] = [1.; 2]; impulse[512] = [0.5; 2];
            p.ir = Some(params::Impulse(Arc::new(Sample { rate: 48_000, frames: impulse })));
            Effect { slot: 0, kind: Kind::Convolution, version: 0, bypass: false,
                output_gain: 1., dry_level: 0., params: parameters }
        };
        let mut source = Engine::default(); source.reset(48_000.); source.attack = 0.;
        source.set_bank(Some(bank(false))); source.note_on(0, 60, 127);
        let mut source_pcm = [[0.; FRAMES]; 2];
        let [left, right] = &mut source_pcm;
        assert_eq!(crate::plugin::tests::allocations(|| source.render(left, right)), 0);
        assert!(source_pcm[0][64..SOURCE - 64].iter().all(|x| (*x - 0.25).abs() < 1e-6));
        assert!(source_pcm[1][64..SOURCE - 64].iter().all(|x| (*x + 0.125).abs() < 1e-6));
        assert!(source_pcm.iter().all(|channel| channel[SOURCE..].iter().all(|x| *x == 0.)));

        for output in [-1, 2] { for tail in [false, true] {
            let create = || {
                let mut engine = Engine::default(); engine.reset(48_000.); engine.attack = 0.;
                engine.set_bank(Some(bank(true)));
                engine.set_fx(ProgramFx { buses: vec![super::super::Bus {
                    index: 0, name: String::new(), volume: 1., pan: 0., output,
                    chain: super::super::Chain { slots: if tail { vec![echo()] } else { Vec::new() } },
                }], ..Default::default() }.processor(48_000., MAX_BLOCK));
                engine
            };
            let mut pcm = [[[0.; FRAMES]; 2]; 2];
            for (mode, result) in pcm.iter_mut().enumerate() {
                let mut engine = create();
                let mut retired = None;
                assert!(!engine.fx().is_empty(), "bus clocks and tails are processor work");
                assert_eq!(crate::plugin::tests::allocations(|| {
                    engine.note_on(0, 60, 127);
                    for changes in timeline.windows(2) {
                        let [(at, volume, pan), (end, ..)] = changes else { unreachable!() };
                        // Move the existing processor for scalar edits; retain
                        // every DSP buffer and ramp, with no clone or disposal.
                        let mut fx = engine.set_fx(FxProcessor::default());
                        assert!(fx.set_param(Rack::Bus(0), 0, FxParam::Volume, *volume));
                        assert!(fx.set_param(Rack::Bus(0), 0, FxParam::Pan, *pan));
                        let before = (fx.buses[0].gains, fx.buses[0].gain_remaining);
                        fx.mix_buses_segment(&mut [], &mut [], 0);
                        assert_eq!(before, (fx.buses[0].gains, fx.buses[0].gain_remaining));
                        engine.set_fx(fx);
                        if *at == 1407 {
                            assert_eq!(engine.active_voices(), 0);
                            retired = engine.set_bank(None);
                            assert!(engine.bank().is_none() && engine.script().is_none());
                        }
                        engine.render(&mut [], &mut []);
                        let sizes: &[usize] = if mode == 0 { &[128] } else { &[1, 7, 31, 128] };
                        let (mut frame, mut block) = (*at, 0);
                        while frame < *end {
                            let n = sizes[block % sizes.len()].min(*end - frame);
                            let (mut left, mut right) = ([0.; MAX_BLOCK], [0.; MAX_BLOCK]);
                            engine.render_checked(&mut left[..n], &mut right[..n]).unwrap();
                            if output < 0 {
                                result[0][frame..frame + n].copy_from_slice(&left[..n]);
                                result[1][frame..frame + n].copy_from_slice(&right[..n]);
                            } else {
                                assert!(left[..n].iter().chain(&right[..n]).all(|x| *x == 0.));
                                let (_, left, right) = engine.fx().direct_outs(n).find(|(channel, ..)| *channel == output as usize).unwrap();
                                result[0][frame..frame + n].copy_from_slice(left);
                                result[1][frame..frame + n].copy_from_slice(right);
                            }
                            frame += n; block += 1;
                        }
                    }
                }), 0);
                assert_eq!(engine.fx().buses[0].gain_remaining, 0);
                assert_eq!(engine.fx().buses[0].gains, [1.; 2]);
                assert_eq!(engine.underruns(), 0);
                drop(retired); // Worker-side retired-bank disposal.
            }
            // Independent linear frame-domain oracle, using the independently
            // measured source only for its existing onset/EOF declick policy.
            let (mut current, mut start, mut target) = ([1_f32; 2], [1_f32; 2], [1_f32; 2]);
            let (mut elapsed, mut event) = (MAX_BLOCK, 0);
            for frame in 0..FRAMES {
                if frame == timeline[event].0 {
                    let (_, volume, pan) = timeline[event];
                    let next = [volume * (1. - pan.max(0.)), volume * (1. + pan.min(0.))];
                    if next != target { start = current; target = next; elapsed = 0; }
                    event += 1;
                }
                for channel in 0..2 {
                    let input = source_pcm[channel][frame] + if tail && frame >= 512 { 0.5 * source_pcm[channel][frame - 512] } else { 0. };
                    let expected = input * current[channel];
                    for actual in &pcm {
                        assert!((actual[channel][frame] - expected).abs() < 2e-6,
                            "route{output} tail{tail} channel{channel} frame{frame}: {} vs {expected}", actual[channel][frame]);
                    }
                }
                if elapsed < MAX_BLOCK {
                    elapsed += 1;
                    current = if elapsed == MAX_BLOCK { target } else {
                        std::array::from_fn(|channel| start[channel]
                            + (target[channel] - start[channel]) / MAX_BLOCK as f32 * elapsed as f32)
                    };
                }
            }
            assert!(pcm[0].iter().flatten().zip(pcm[1].iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-6));
            assert!(pcm[0].iter().all(|channel| channel[640..769].iter().all(|x| *x == 0.)), "settled mute remains exact zero");
            if tail { assert!(pcm[0].iter().any(|channel| channel[1696..1760].iter().any(|x| x.abs() > 0.01)),
                "bus-only tail must advance and unmute after bank removal"); }
        }}
    }
}
