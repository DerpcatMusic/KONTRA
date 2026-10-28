//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use std::sync::Arc;

use kontra_dsp::dynamics::{Compressor, CompressorSettings, Limiter, LimiterSettings};
use kontra_dsp::eq::Biquad;
use kontra_dsp::reverb::{Convolver, Fdn, Settings};
use kontra_ir as ir;
use kontra_voice::Gain;

use crate::effects;
use crate::filters;
use crate::{Engine, MAX_SPAN, Response};

enum Dsp {
    None,
    Eq(Box<[Biquad]>),
    /// A wet path (convolver or reverb) and a span of its output.
    Wet(Box<dyn Wet>, Box<[[f32; 2]]>),
}

/// A processor whose span replaces the slot's output; the slot's dry level
/// mixes the input back.
trait Wet: Send {
    fn process(&mut self, io: &mut [[f32; 2]]);
    /// The slot's reverb settings, before each block. Never allocates.
    fn set_reverb(&mut self, _reverb: ir::Reverb) {}
}

impl Wet for Convolver {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Convolver::process(self, io);
    }
}

impl Wet for Compressor {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Compressor::process(self, io);
    }
}

impl Wet for Limiter {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Limiter::process(self, io);
    }
}

impl Wet for Fdn {
    fn process(&mut self, io: &mut [[f32; 2]]) {
        Fdn::process(self, io);
    }

    fn set_reverb(&mut self, reverb: ir::Reverb) {
        self.set(settings(reverb));
    }
}

fn wet(processor: Box<dyn Wet>) -> Dsp {
    Dsp::Wet(processor, vec![[0.0; 2]; MAX_SPAN as usize].into())
}

pub(crate) struct Rack {
    /// The plan's slots as scripts have changed them.
    pub(crate) slots: Box<[Option<ir::Effect>]>,
    dsp: Box<[Dsp]>,
}

impl Rack {
    /// `impulses` are the plan's convolution responses. Not real-time.
    pub(crate) fn new(
        slots: &[Option<ir::Effect>],
        rate: u32,
        impulses: &[Option<Response>],
    ) -> Self {
        let dsp = slots
            .iter()
            .map(|slot| match slot.map(|e| e.kind) {
                // Built even while bypassed: a script may enable it later.
                Some(ir::EffectKind::Eq(_) | ir::EffectKind::Filter(_)) => {
                    let live = slot.map(|e| ir::Effect { bypass: false, ..e });
                    let sections = filters::sections(&[live]);
                    Dsp::Eq(filters::biquads(&sections, rate))
                }
                Some(ir::EffectKind::Convolution { impulse }) => {
                    match impulses.get(usize::from(impulse)).and_then(Option::as_ref) {
                        Some(r) => wet(Box::new(Convolver::new(&r.left, &r.right))),
                        None => Dsp::None,
                    }
                }
                Some(ir::EffectKind::Reverb(r)) => wet(Box::new(Fdn::new(settings(r), rate))),
                Some(ir::EffectKind::Compressor(c)) => {
                    wet(Box::new(Compressor::new(compressor(c), rate)))
                }
                Some(ir::EffectKind::Limiter(l)) => wet(Box::new(Limiter::new(limiter(l), rate))),
                _ => Dsp::None,
            })
            .collect();
        Self {
            slots: slots.into(),
            dsp,
        }
    }

    /// Processes `io`; a send slot adds it to `taps` (one buffer per send slot).
    pub(crate) fn process(&mut self, io: &mut [[f32; 2]], taps: &mut [Box<[[f32; 2]]>]) {
        for (slot, dsp) in self.slots.iter().zip(self.dsp.iter_mut()) {
            let Some(e) = slot.filter(|e| !e.bypass) else {
                continue;
            };
            match (dsp, e.kind) {
                (Dsp::Eq(f), _) => filters::run(f, io),
                // The wet path from the input, then the dry level, then the wet at the output gain.
                (Dsp::Wet(processor, wet), kind) => {
                    let Some(wet) = wet.get_mut(..io.len()) else {
                        continue;
                    };
                    // A script may have moved the reverb's controls since the last block.
                    if let ir::EffectKind::Reverb(r) = kind {
                        processor.set_reverb(r);
                    }
                    wet.copy_from_slice(io);
                    processor.process(wet);
                    apply(effects::matrix(&e), io);
                    for (x, w) in io.iter_mut().zip(wet.iter()) {
                        x[0] += w[0] * e.output;
                        x[1] += w[1] * e.output;
                    }
                    continue;
                }
                (_, ir::EffectKind::Sends(levels)) => {
                    for (tap, level) in taps.iter_mut().zip(levels) {
                        for (t, x) in tap.iter_mut().zip(io.iter()) {
                            t[0] += x[0] * level;
                            t[1] += x[1] * level;
                        }
                    }
                }
                _ => {}
            }
            apply(effects::matrix(&e), io);
        }
    }

    /// Whether any slot is active (a send slot with nothing in it returns nothing).
    pub(crate) fn active(&self) -> bool {
        self.slots.iter().flatten().any(|e| !e.bypass)
    }
}

fn compressor(c: ir::Compressor) -> CompressorSettings {
    CompressorSettings {
        threshold_db: c.threshold_db,
        ratio: c.ratio,
        knee_db: c.knee_db,
        attack_ms: c.attack_ms,
        release_ms: c.release_ms,
        makeup_db: c.makeup_db,
        link: c.link,
        mix: c.mix,
    }
}

fn limiter(l: ir::Limiter) -> LimiterSettings {
    LimiterSettings {
        input_db: l.input_db,
        ceiling_db: l.ceiling_db,
        release_ms: l.release_ms,
    }
}

fn settings(r: ir::Reverb) -> Settings {
    Settings {
        seconds: r.seconds,
        size: r.size,
        damping: r.damping,
        diffusion: r.diffusion,
        predelay_ms: r.predelay_ms,
        high_cut_hz: r.high_cut_hz,
        low_shelf_db: r.low_shelf_db,
        stereo: r.stereo,
    }
}

pub(crate) fn apply(m: Gain, io: &mut [[f32; 2]]) {
    if m == effects::IDENTITY {
        return;
    }
    let [[ll, lr], [rl, rr]] = m;
    for f in io {
        let [l, r] = *f;
        *f = [ll * l + lr * r, rl * l + rr * r];
    }
}

impl Engine {
    /// Rebuilds every rack's processors from its slots as the init callback
    /// left them. Not real-time.
    pub(crate) fn rebuild_racks(&mut self) {
        let rate = self.rate;
        let plan = Arc::clone(&self.plan);
        let racks = std::iter::once(&mut self.rack)
            .chain(self.sends.iter_mut())
            .chain(std::iter::once(&mut self.main))
            .chain(self.buses.iter_mut().map(|b| &mut b.rack));
        for rack in racks {
            *rack = Rack::new(&rack.slots, rate, &plan.impulses);
        }
    }

    /// Adds the buses into `span` through their racks, then runs the
    /// instrument's insert, send and main racks on it.
    pub(crate) fn mix_out(&mut self, span: &mut [[f32; 2]]) {
        let len = span.len();
        for tap in &mut self.taps {
            tap.fill([0.0; 2]);
        }
        for bus in &mut self.buses {
            if let Some(mix) = bus.mix.get_mut(..len) {
                bus.rack.process(mix, &mut self.taps);
                apply(effects::balance(bus.pan), mix);
            }
            for (o, m) in span.iter_mut().zip(bus.mix.iter()) {
                o[0] += m[0] * bus.gain;
                o[1] += m[1] * bus.gain;
            }
        }
        self.rack.process(span, &mut self.taps);
        for (send, tap) in self.sends.iter_mut().zip(self.taps.iter_mut()) {
            if let Some(tap) = tap.get_mut(..len).filter(|_| send.active()) {
                send.process(tap, &mut []);
                for (o, t) in span.iter_mut().zip(tap.iter()) {
                    o[0] += t[0];
                    o[1] += t[1];
                }
            }
        }
        self.main.process(span, &mut []);
    }
}
