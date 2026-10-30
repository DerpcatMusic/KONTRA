//! The host's bus controls: gain and whether a bus plays (a purged mic).

use crate::{Engine, MAX_SPAN, plan, racks};
use kontra_ir as ir;

/// A bus: what it carries mixed, then its rack, fader and balance.
pub(crate) struct BusState {
    pub(crate) gain: f32,
    pub(crate) enabled: bool,
    /// Its rack, on what it carries.
    pub(crate) rack: racks::Rack,
    /// -1..=1 balance after the rack.
    pub(crate) pan: f32,
    /// Pan and volume changes glide as measured natively: each channel's
    /// balance gain at most 1 per 50 ms, the volume a one-pole of 3.7 ms.
    /// An effect's bypass switches at once.
    pub(crate) balance: kontra_dsp::core::Fader,
    pub(crate) fader: kontra_dsp::core::Fader,
    /// Later buses it feeds, and their levels.
    pub(crate) sends: Box<[ir::BusSend]>,
    pub(crate) to_main: bool,
    pub(crate) mix: Box<[[f32; 2]]>,
    /// Its output's left and right peaks since they were last taken.
    pub(crate) peak: [f32; 2],
}

impl BusState {
    pub(crate) fn new(b: &plan::Bus, rate: u32) -> Self {
        Self {
            gain: b.gain,
            enabled: b.enabled,
            rack: racks::Rack::routed(
                &b.inserts,
                rate,
                racks::Responses(&[], &[]),
                &b.mods,
                &b.blends,
            ),
            pan: b.pan,
            balance: kontra_dsp::core::Fader::new(rate, 0.0, 50.0),
            fader: kontra_dsp::core::Fader::new(rate, 3.7, 0.0),
            sends: b.sends.clone().into(),
            to_main: b.to_main,
            mix: vec![[0.0; 2]; MAX_SPAN as usize].into(),
            peak: [0.0; 2],
        }
    }
}

impl Engine {
    /// Instrument bus `n`'s output peaks since the last take (silence when
    /// it has none), and starts again from silence.
    pub fn take_bus_peak(&mut self, n: usize) -> [f32; 2] {
        // Plan bus 0 is the main bus; instrument bus n is plan bus n + 1.
        self.buses
            .get_mut(n + 1)
            .map_or([0.0; 2], |b| std::mem::take(&mut b.peak))
    }

    pub fn set_bus_gain(&mut self, bus: usize, gain: f32) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.gain = gain;
        }
    }

    /// A disabled bus (purged mic) fades its voices and starts no new ones.
    pub fn set_bus_enabled(&mut self, bus: usize, enabled: bool) {
        if let Some(b) = self.buses.get_mut(bus) {
            b.enabled = enabled;
        }
        if !enabled {
            let fade = self.fade_frames();
            let plan = self.plan.clone();
            let groups = std::mem::take(&mut self.groups);
            self.fade_where(fade, |s| {
                plan.zones
                    .get(s.zone)
                    .is_some_and(|z| plan::bus(&groups, z) == bus)
            });
            self.groups = groups;
        }
    }
}
