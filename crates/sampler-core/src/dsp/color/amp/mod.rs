//! Kontakt amp, cabinet and pedal models as configurations of the color unit's shaper stage.
pub(super) mod cabinet;

use sampler_ir::fx::color::{Amp, AmpModel};

pub(super) fn compile(a: Amp, rate: u32) -> super::Curve {
    let k = a.knobs;
    match a.model {
        AmpModel::Cabinet => super::Curve::Cabinet(cabinet::Engine::new(k[4], k[0], k[1], k[2], k[3], rate)),
    }
}

/// Delay storage a model needs.
pub(super) fn frames(a: Amp, rate: u32) -> usize {
    match a.model {
        AmpModel::Cabinet => cabinet::frames(rate),
    }
}
