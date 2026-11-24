//! The host's bus controls: gain and whether a bus plays (a purged mic).

use crate::{Engine, plan};

pub(crate) use kontra_racks::bus::BusState;

impl Engine {
    /// Instrument bus `n`'s output peaks since the last take (silence when
    /// it has none), and starts again from silence.
    pub fn take_bus_peak(&mut self, n: usize) -> [f32; 2] {
        // Plan bus 0 is the main bus; instrument bus n is plan bus n + 1.
        n.checked_add(1)
            .and_then(|n| self.buses.get_mut(n))
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
