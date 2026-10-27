//! Instrument and bus racks run in slot order on their summed audio, so a
//! slot with state (an EQ, a convolution) sits where the format put it.
//! Group racks fold into each voice's matrix instead (voice_gain.rs).

use kontra_dsp::eq::Biquad;
use kontra_ir as ir;
use kontra_voice::Gain;

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
                Some(ir::EffectKind::Eq(_)) => {
                    let bells = filters::bells(std::slice::from_ref(slot));
                    Dsp::Eq(filters::biquads(&bells, rate))
                }
                _ => Dsp::None,
            })
            .collect();
        Self {
            slots: slots.into(),
            dsp,
        }
    }

    pub(crate) fn process(&mut self, io: &mut [[f32; 2]]) {
        for (slot, dsp) in self.slots.iter().zip(self.dsp.iter_mut()) {
            let Some(e) = slot.filter(|e| !e.bypass) else {
                continue;
            };
            if let Dsp::Eq(f) = dsp {
                filters::run(f, io);
            }
            apply(effects::matrix(&e), io);
        }
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
