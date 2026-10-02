//! Real-time DSP state built from a [`ProgramFx`] description.

use super::{Effect, Kind, Params, ProgramFx, blocks::Block, convolution::Convolver, params, reverb::Reverb};
use std::ops::Range;

/// Input at or below this level (−120 dBFS) counts as silence for tail tracking.
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
    sleep: Sleep,
    /// Group taps have filled the current block's return inputs.
    group_inputs: bool,
    /// Output channels, `max_block` each; empty for the default processor.
    outs: Vec<[Box<[f32]>; 2]>,
    /// Channels written since they were last cleared.
    out_fed: u8,
    /// A block ended ([`mix_buses`](Self::mix_buses)): the next write to a
    /// channel starts the next block, clearing them all first.
    outs_done: bool,
}

/// When a chain may stop processing: its input has been silent (−120 dBFS)
/// for longer than its effects ring on the loudest input since it last
/// slept, to below −120 dBFS. A quiet part's reverb sleeps sooner than a
/// loud one's; the next input wakes it.
#[derive(Clone, Copy, Default)]
struct Sleep {
    /// Consecutive frames of silent input.
    silent: usize,
    /// Frames the current tail rings for.
    tail: usize,
    /// Loudest input since the chain last slept.
    peak: f32,
}

impl Sleep {
    /// Nothing rings.
    const ASLEEP: Self = Self { silent: 1, tail: 0, peak: 0.0 };

    fn sleeping(&self) -> bool {
        self.silent > self.tail
    }

    /// Count one input block; `tail(peak)` is how long the chain rings on
    /// input that loud. True while the chain must process.
    fn feed(&mut self, left: &[f32], right: &[f32], tail: impl Fn(f32) -> usize) -> bool {
        let peak = left.iter().chain(right).fold(0f32, |m, x| m.max(x.abs()));
        self.feed_peak(left.len(), peak, tail)
    }

    fn feed_peak(&mut self, n: usize, peak: f32, tail: impl Fn(f32) -> usize) -> bool {
        if peak > SILENCE {
            (self.silent, self.peak) = (0, self.peak.max(peak));
        } else {
            if self.silent == 0 {
                self.tail = tail(self.peak);
            }
            self.silent = self.silent.saturating_add(n);
            if self.sleeping() {
                self.peak = 0.0;
            }
        }
        !self.sleeping()
    }
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
    buffer: [Box<[f32]>; 2],
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
}

/// An instrument bus: groups render into `input`; its chain, fader and pan
/// feed the instrument signal ahead of the insert rack.
struct Bus {
    chain: Vec<Slot>,
    input: [Box<[f32]>; 2],
    volume: f32,
    pan: f32,
    /// Channel gains reached at the end of the last block.
    gains: [f32; 2],
    /// A voice rendered into `input` this block.
    fed: bool,
    sleep: Sleep,
    /// Kontakt bus index.
    index: u8,
    /// Output channel (0..[`OUTS`]) past the instrument output, or -1.
    output: i8,
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
                slot,
                buffer: [zeros(max_block), zeros(max_block)],
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
                    fed: false,
                    sleep: Sleep::ASLEEP,
                    index: b.index as u8,
                    output: channel(b.output as f32),
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
            // Nothing rings yet.
            sleep: Sleep::ASLEEP,
            group_inputs: false,
            outs: (0..OUTS).map(|_| [zeros(max_block), zeros(max_block)]).collect(),
            out_fed: 0,
            outs_done: false,
        }
    }
}

impl FxProcessor {
    /// Whether processing leaves audio unchanged.
    pub fn is_empty(&self) -> bool {
        self.insert.is_empty() && self.returns.is_empty() && self.main.is_empty()
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
            bus.sleep = Sleep::ASLEEP;
        }
        self.sleep = Sleep::ASLEEP;
        self.group_inputs = false;
        self.returns.iter_mut().flat_map(|r| &mut r.buffer).for_each(|b| b.fill(0.0));
        self.outs.iter_mut().flatten().for_each(|b| b.fill(0.0));
        self.out_fed = 0;
    }

    /// Processes the program output in place. Once the input has been silent
    /// for longer than every tail, blocks pass through untouched.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let group_inputs = std::mem::take(&mut self.group_inputs);
        if self.is_empty() {
            return;
        }
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        let mut sleep = self.sleep;
        // Include summed group feeds: a pre-Amplifier tap may feed the
        // returns while the instrument's dry signal is completely silent.
        let group_peak = if group_inputs {
            self.returns.iter().filter(|r| !r.slot.bypass).flat_map(|r| &r.buffer)
                .flat_map(|b| &b[..n]).fold(0.0f32, |p, x| p.max(x.abs()))
        } else { 0.0 };
        let peak = left.iter().chain(right.iter()).fold(group_peak, |p, x| p.max(x.abs()));
        let awake = sleep.feed_peak(n, peak, |peak| tail(self.slots(), peak));
        self.sleep = sleep;
        if !awake {
            return;
        }
        for (l, r) in left
            .chunks_mut(self.max_block)
            .zip(right.chunks_mut(self.max_block))
        {
            self.process_block(l, r, group_inputs);
        }
    }

    /// Maximum voice/send block supported by the fixed worker buffers.
    pub(crate) fn block_size(&self) -> usize {
        if self.max_block == 0 { usize::MAX } else { self.max_block }
    }

    /// Clear the fixed return inputs once, before all voice segments in a block.
    pub(crate) fn begin_group_sends(&mut self, n: usize) {
        for ret in &mut self.returns {
            ret.buffer.iter_mut().for_each(|b| b[..n].fill(0.0));
        }
        self.group_inputs = true;
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
                let input = (l.get_mut(frames.clone())?, r.get_mut(frames.clone())?);
                bus.fed = true;
                Some(input)
            }
        });
        (input, SendInputs { returns: &mut self.returns, offset: frames.start })
    }

    /// The channel gains of bus `bus` when it only passes its input to the
    /// instrument output through a fader at rest: no effects, no ramp this
    /// block. Voices can mix straight to the output through them, the same sum.
    pub fn bus_gains(&self, bus: u8) -> Option<[f32; 2]> {
        let bus = self.buses.get(*self.bus_of.get(bus as usize)? as usize)?;
        (bus.output < 0 && bus.chain.is_empty() && bus.gains == balance(bus.volume, bus.pan)).then_some(bus.gains)
    }

    /// `frames` of Kontakt bus `bus`'s input for the current block (within
    /// `max_block`); `None` when the program has no such bus.
    /// Buses [`DIRECT`]` + c` are output channel `c`.
    pub fn bus_input(&mut self, bus: u8, frames: Range<usize>) -> Option<(&mut [f32], &mut [f32])> {
        self.voice_inputs(Some(bus), frames).0
    }

    /// Runs every bus that was fed or still rings on its first `left.len()`
    /// input frames (at most `max_block`), adds it to `left`/`right` through
    /// its fader and pan, and clears the inputs for the next block.
    /// A bus routed to an output channel plays there instead.
    pub fn mix_buses(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(self.max_block);
        self.fresh_outs();
        for bus in &mut self.buses {
            if !bus.fed && bus.sleep.sleeping() {
                continue;
            }
            bus.fed = false;
            let [il, ir] = &mut bus.input;
            let (il, ir) = (&mut il[..n], &mut ir[..n]);
            bus.sleep.feed(il, ir, |peak| tail(bus.chain.iter(), peak));
            for slot in &mut bus.chain {
                slot.process(il, ir);
            }
            let target = balance(bus.volume, bus.pan);
            let start = bus.gains;
            let step = [
                (target[0] - start[0]) / n as f32,
                (target[1] - start[1]) / n as f32,
            ];
            let (to_l, to_r) = match self.outs.get_mut(bus.output as usize) {
                Some([ol, or]) if bus.output >= 0 => {
                    self.out_fed |= 1 << bus.output;
                    (&mut ol[..n], &mut or[..n])
                }
                _ => (&mut left[..n], &mut right[..n]),
            };
            for (i, ((l, r), (x, y))) in to_l
                .iter_mut()
                .zip(to_r.iter_mut())
                .zip(il.iter_mut().zip(ir.iter_mut()))
                .enumerate()
            {
                let i = i as f32;
                *l += *x * (start[0] + step[0] * i);
                *r += *y * (start[1] + step[1] * i);
                (*x, *y) = (0.0, 0.0);
            }
            bus.gains = target;
        }
        self.outs_done = true;
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

    fn out(&mut self, c: usize, frames: Range<usize>) -> Option<(&mut [f32], &mut [f32])> {
        self.fresh_outs();
        let [l, r] = self.outs.get_mut(c)?;
        let out = (l.get_mut(frames.clone())?, r.get_mut(frames)?);
        self.out_fed |= 1 << c;
        Some(out)
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

    fn slots(&self) -> impl Iterator<Item = &Slot> {
        let insert = self.insert.iter().filter_map(|stage| match stage {
            Stage::Effect(slot) => Some(slot),
            Stage::Tap(_) => None,
        });
        insert
            .chain(self.returns.iter().map(|r| &r.slot))
            .chain(&self.main)
    }

    /// `left.len() == right.len() <= max_block`.
    fn process_block(&mut self, left: &mut [f32], right: &mut [f32], group_inputs: bool) {
        let n = left.len();
        if !group_inputs {
            for ret in &mut self.returns {
                for buffer in &mut ret.buffer {
                    buffer[..n].fill(0.0);
                }
            }
        }
        for stage in &mut self.insert {
            match stage {
                Stage::Effect(slot) => slot.process(left, right),
                Stage::Tap(tap) if !tap.bypass => {
                    for (ret, &level) in self.returns.iter_mut().zip(tap.levels.iter()) {
                        let level = level * tap.gain;
                        if level != 0.0 {
                            let [bl, br] = &mut ret.buffer;
                            mix(&mut bl[..n], left, level);
                            mix(&mut br[..n], right, level);
                        }
                    }
                }
                Stage::Tap(_) => {}
            }
        }
        // Bypassed send slots return nothing (audits/EFFECTS.md).
        for ret in self.returns.iter_mut().filter(|r| !r.slot.bypass) {
            let [bl, br] = &mut ret.buffer;
            let (bl, br) = (&mut bl[..n], &mut br[..n]);
            ret.slot.process(bl, br);
            mix(left, bl, 1.0);
            mix(right, br, 1.0);
        }
        for slot in &mut self.main {
            slot.process(left, right);
        }
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
            for (out, input) in [(left, &self.dry_buffer[0]), (right, &self.dry_buffer[1])] {
                for (y, x) in out.iter_mut().zip(input.iter()) {
                    *y = *y * wet + *x * dry;
                }
            }
        } else if wet != 1.0 {
            left.iter_mut().for_each(|y| *y *= wet);
            right.iter_mut().for_each(|y| *y *= wet);
        }
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

    /// Frames of output above −120 dBFS after input at most `peak` falls silent.
    fn tail(&self, peak: f32) -> usize {
        match self {
            Dsp::Gain(_) | Dsp::Stereo { .. } => 0,
            Dsp::Reverb(rv, _) => rv.tail(peak),
            Dsp::Convolution(conv) => conv[0].tail(peak).max(conv[1].tail(peak)),
            Dsp::Block(b) => b.tail(peak),
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

/// Frames a chain of `slots` rings after input at most `peak`: series
/// stages add their tails; summing the parallel sends too keeps this an
/// upper bound. Slot gains are not followed, so the peak is taken 12 dB
/// louder.
fn tail<'a>(slots: impl Iterator<Item = &'a Slot>, peak: f32) -> usize {
    slots.map(|s| s.dsp.tail(peak * 4.0)).sum()
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
