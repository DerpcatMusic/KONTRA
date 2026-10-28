//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use kontra_dsp::eq::Biquad;
use kontra_ir as ir;
use kontra_voice::Gain;

use crate::Engine;
use crate::effects;
use crate::filters;

enum Dsp {
    None,
    Eq(Box<[Biquad]>),
}

pub(crate) struct Rack {
    /// The plan's slots as scripts have changed them.
    pub(crate) slots: Box<[Option<ir::Effect>]>,
    dsp: Box<[Dsp]>,
}

impl Rack {
    /// Not real-time.
    pub(crate) fn new(slots: &[Option<ir::Effect>], rate: u32) -> Self {
        let dsp = slots
            .iter()
            .map(|slot| match slot.map(|e| e.kind) {
                // Built even while bypassed: a script may enable it later.
                Some(ir::EffectKind::Eq(_) | ir::EffectKind::Filter(_)) => {
                    let live = slot.map(|e| ir::Effect { bypass: false, ..e });
                    let sections = filters::sections(&[live]);
                    Dsp::Eq(filters::biquads(&sections, rate))
                }
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
        let racks = std::iter::once(&mut self.rack)
            .chain(self.sends.iter_mut())
            .chain(std::iter::once(&mut self.main))
            .chain(self.buses.iter_mut().map(|b| &mut b.rack));
        for rack in racks {
            *rack = Rack::new(&rack.slots, rate);
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
