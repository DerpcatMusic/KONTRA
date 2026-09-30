//! Real-time DSP state built from a [`ProgramFx`] description.

use super::{Effect, Params, ProgramFx, convolution::Convolver, params, reverb::Reverb};
use std::ops::Range;

/// Input at or below this level (−120 dBFS) counts as silence for tail tracking.
pub(super) const SILENCE: f32 = 1e-6;
/// Kontakt's instrument buses.
const BUSES: usize = 16;
const NO_BUS: u8 = u8::MAX;

/// An effect rack a script addresses (`$NI_INSERT_BUS`, `$NI_SEND_BUS`,
/// `$NI_MAIN_BUS`, `$NI_BUS_OFFSET + n`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rack {
    Insert,
    Send,
    Main,
    Bus(u8),
}

/// A script-controllable effect or bus value. Gains are linear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    max_block: usize,
    sleep: Sleep,
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
        if peak > SILENCE {
            (self.silent, self.peak) = (0, self.peak.max(peak));
        } else {
            if self.silent == 0 {
                self.tail = tail(self.peak);
            }
            self.silent = self.silent.saturating_add(left.len());
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
}

enum Dsp {
    Gain(f32),
    /// Stereo Modeller: mid/side width then constant-sum balance.
    Stereo {
        width: f32,
        gains: [f32; 2],
    },
    Reverb(Box<Reverb>),
    Convolution(Box<[Convolver; 2]>),
}

impl ProgramFx {
    /// Builds the DSP state for `sample_rate`; `process` calls are split into
    /// blocks of at most `max_block` frames. Unimplemented slots are left
    /// out; bypassed ones are built so scripts can switch them on. Every
    /// stored bus gets input buffers of `max_block`.
    pub fn processor(&self, sample_rate: f32, max_block: usize) -> FxProcessor {
        let max_block = max_block.max(1);
        let build = |fx: &Effect| Slot::new(fx, sample_rate, max_block);
        let tapped = self
            .insert
            .slots
            .iter()
            .any(|fx| matches!(fx.params, Params::SendLevels(_)));
        // Unfed send slots would only ever process silence.
        let sends: Vec<_> = if tapped {
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
                }
            })
            .collect();
        FxProcessor {
            insert,
            returns,
            main: self.main.slots.iter().filter_map(build).collect(),
            buses,
            bus_of,
            max_block,
            // Nothing rings yet.
            sleep: Sleep::ASLEEP,
        }
    }
}

impl FxProcessor {
    /// Whether processing leaves audio unchanged.
    pub fn is_empty(&self) -> bool {
        self.insert.is_empty() && self.main.is_empty()
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
    }

    /// Processes the program output in place. Once the input has been silent
    /// for longer than every tail, blocks pass through untouched.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.is_empty() {
            return;
        }
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        let mut sleep = self.sleep;
        let awake = sleep.feed(left, right, |peak| tail(self.slots(), peak));
        self.sleep = sleep;
        if !awake {
            return;
        }
        for (l, r) in left
            .chunks_mut(self.max_block)
            .zip(right.chunks_mut(self.max_block))
        {
            self.process_block(l, r);
        }
    }

    /// The channel gains of bus `bus` when it only passes its input to the
    /// output through a fader at rest: no effects, no ramp this block.
    /// Voices can mix straight to the output through them, the same sum.
    pub fn bus_gains(&self, bus: u8) -> Option<[f32; 2]> {
        let bus = self.buses.get(*self.bus_of.get(bus as usize)? as usize)?;
        (bus.chain.is_empty() && bus.gains == balance(bus.volume, bus.pan)).then_some(bus.gains)
    }

    /// `frames` of Kontakt bus `bus`'s input for the current block (within
    /// `max_block`); `None` when the program has no such bus.
    pub fn bus_input(&mut self, bus: u8, frames: Range<usize>) -> Option<(&mut [f32], &mut [f32])> {
        let i = *self.bus_of.get(bus as usize)?;
        let bus = self.buses.get_mut(i as usize)?;
        let [l, r] = &mut bus.input;
        let input = (l.get_mut(frames.clone())?, r.get_mut(frames)?);
        bus.fed = true;
        Some(input)
    }

    /// Runs every bus that was fed or still rings on its first `left.len()`
    /// input frames (at most `max_block`), adds it to `left`/`right` through
    /// its fader and pan, and clears the inputs for the next block.
    pub fn mix_buses(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len()).min(self.max_block);
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
            for (i, ((l, r), (x, y))) in left
                .iter_mut()
                .zip(right.iter_mut())
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
    }

    /// Set a script-controllable value; false when this processor does not
    /// hold it (unknown or unimplemented slots).
    pub fn set_param(&mut self, rack: Rack, slot: u8, param: FxParam, value: f32) -> bool {
        let gain = value.max(0.0);
        if let FxParam::Volume | FxParam::Pan = param {
            let Some(bus) = self.bus_mut(rack) else {
                return false;
            };
            match param {
                FxParam::Volume => bus.volume = gain,
                _ => bus.pan = value.clamp(-1.0, 1.0),
            }
            return true;
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
        match param {
            FxParam::Bypass => s.bypass = value != 0.0,
            FxParam::Wet => s.wet = gain,
            FxParam::Dry => s.dry = gain,
            _ => return false,
        }
        true
    }

    /// Current value of a script-controllable parameter.
    pub fn param(&self, rack: Rack, slot: u8, param: FxParam) -> Option<f32> {
        if let FxParam::Volume | FxParam::Pan = param {
            let bus = self.bus(rack)?;
            return Some(if param == FxParam::Volume {
                bus.volume
            } else {
                bus.pan
            });
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
        match param {
            FxParam::Bypass => Some(f32::from(s.bypass)),
            FxParam::Wet => Some(s.wet),
            FxParam::Dry => Some(s.dry),
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
    fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len();
        for ret in &mut self.returns {
            for buffer in &mut ret.buffer {
                buffer[..n].fill(0.0);
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
        for ret in &mut self.returns {
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
        let dsp = Dsp::new(&fx.params, sample_rate, max_block)?;
        // Scripts may raise the dry level later, so the buffer always exists.
        Some(Self {
            index: fx.slot as u8,
            bypass: fx.bypass,
            dsp,
            wet: fx.output_gain,
            dry: fx.dry_level,
            dry_buffer: [zeros(max_block), zeros(max_block)],
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
            Params::Reverb(p) => Dsp::Reverb(Box::new(Reverb::new(p, sample_rate))),
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
            Dsp::Reverb(rv) => rv.clear(),
            Dsp::Convolution(conv) => conv.iter_mut().for_each(Convolver::clear),
        }
    }

    /// Frames of output above −120 dBFS after input at most `peak` falls silent.
    fn tail(&self, peak: f32) -> usize {
        match self {
            Dsp::Gain(_) | Dsp::Stereo { .. } => 0,
            Dsp::Reverb(rv) => rv.tail(peak),
            Dsp::Convolution(conv) => conv[0].tail(peak).max(conv[1].tail(peak)),
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
            Dsp::Reverb(rv) => rv.process(left, right),
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
