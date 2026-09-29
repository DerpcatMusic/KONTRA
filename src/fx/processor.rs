//! Real-time DSP state built from a [`ProgramFx`] description.

use super::{Effect, Params, ProgramFx, convolution::Convolver, params, reverb::Reverb};

/// Input at or below this level (−120 dBFS) counts as silence for tail tracking.
const SILENCE: f32 = 1e-6;

/// Owned DSP state for one program's insert, send and main racks.
///
/// Built by [`ProgramFx::processor`], which allocates everything;
/// [`process`](Self::process) never allocates, locks or panics and accepts
/// any block length. The default processor passes audio through.
#[derive(Default)]
pub struct FxProcessor {
    insert: Vec<Stage>,
    /// Parallel send slots fed by [`Stage::Tap`]s; empty when nothing taps.
    returns: Vec<Return>,
    main: Vec<Slot>,
    max_block: usize,
    /// Upper bound on the frames an effect keeps sounding after silent input.
    tail: usize,
    /// Consecutive frames of silent input.
    silent: usize,
}

enum Stage {
    Effect(Slot),
    /// Send Levels: linear level into each of `returns`, taken here.
    Tap(Box<[f32]>),
}

struct Return {
    slot: Slot,
    buffer: [Box<[f32]>; 2],
}

/// One active effect with its slot's wet/dry levels.
struct Slot {
    dsp: Dsp,
    wet: f32,
    dry: f32,
    /// The input kept for the dry mix; empty when `dry` is 0.
    dry_buffer: [Box<[f32]>; 2],
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
    /// blocks of at most `max_block` frames. Bypassed and unimplemented slots
    /// are left out, and buses are not built (their routing is unknown).
    pub fn processor(&self, sample_rate: f32, max_block: usize) -> FxProcessor {
        let max_block = max_block.max(1);
        let build = |fx: &Effect| Slot::new(fx, sample_rate, max_block);
        let tapped = self
            .insert
            .slots
            .iter()
            .any(|fx| !fx.bypass && matches!(fx.params, Params::SendLevels(_)));
        // Unfed send slots would only ever process silence.
        let sends: Vec<_> = match tapped {
            true => self
                .send
                .slots
                .iter()
                .filter_map(|fx| Some((fx.slot, build(fx)?)))
                .collect(),
            false => Vec::new(),
        };
        let insert = self
            .insert
            .slots
            .iter()
            .filter_map(|fx| match &fx.params {
                Params::SendLevels(levels) if !fx.bypass => Some(Stage::Tap(
                    sends
                        .iter()
                        .map(|(slot, _)| {
                            levels.sends.get(*slot).copied().unwrap_or(0.0) * fx.output_gain
                        })
                        .collect(),
                )),
                _ => build(fx).map(Stage::Effect),
            })
            .collect();
        let returns = sends
            .into_iter()
            .map(|(_, slot)| Return {
                slot,
                buffer: [zeros(max_block), zeros(max_block)],
            })
            .collect();
        let mut fx = FxProcessor {
            insert,
            returns,
            main: self.main.slots.iter().filter_map(build).collect(),
            max_block,
            tail: 0,
            silent: 0,
        };
        // Series stages add their tails; summing the parallel sends too keeps
        // this an upper bound.
        fx.tail = fx.slots().map(|s| s.dsp.tail()).sum();
        fx
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
            .for_each(Dsp::clear);
        self.silent = self.tail;
    }

    /// Processes the program output in place. Once the input has been silent
    /// longer than the longest tail, blocks pass through untouched.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.is_empty() {
            return;
        }
        let n = left.len().min(right.len());
        let (left, right) = (&mut left[..n], &mut right[..n]);
        let quiet = left.iter().chain(&*right).all(|x| x.abs() <= SILENCE);
        self.silent = if quiet {
            self.silent.saturating_add(n)
        } else {
            0
        };
        if self.silent > self.tail {
            return;
        }
        for (l, r) in left
            .chunks_mut(self.max_block)
            .zip(right.chunks_mut(self.max_block))
        {
            self.process_block(l, r);
        }
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
                Stage::Tap(levels) => {
                    for (ret, &level) in self.returns.iter_mut().zip(levels.iter()) {
                        if level != 0.0 {
                            let [bl, br] = &mut ret.buffer;
                            mix(&mut bl[..n], left, level);
                            mix(&mut br[..n], right, level);
                        }
                    }
                }
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

    fn slots(&self) -> impl Iterator<Item = &Slot> {
        let insert = self.insert.iter().filter_map(|stage| match stage {
            Stage::Effect(slot) => Some(slot),
            Stage::Tap(_) => None,
        });
        insert
            .chain(self.returns.iter().map(|r| &r.slot))
            .chain(&self.main)
    }
}

impl Slot {
    fn new(fx: &Effect, sample_rate: f32, max_block: usize) -> Option<Self> {
        if fx.bypass {
            return None;
        }
        let dsp = Dsp::new(&fx.params, sample_rate, max_block)?;
        let buffer = || zeros(if fx.dry_level == 0.0 { 0 } else { max_block });
        Some(Self {
            dsp,
            wet: fx.output_gain,
            dry: fx.dry_level,
            dry_buffer: [buffer(), buffer()],
        })
    }

    /// `left.len() == right.len() <= max_block`.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
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

    /// Frames of output after the input falls silent.
    fn tail(&self) -> usize {
        match self {
            Dsp::Gain(_) | Dsp::Stereo { .. } => 0,
            Dsp::Reverb(rv) => rv.tail(),
            Dsp::Convolution(conv) => conv[0].ir_len(),
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
