//! The host's bus controls: gain and whether a bus plays (a purged mic).

use crate::{Engine, plan};

impl Engine {
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
            for slot in self.voices.iter_mut().filter(|s| s.voice.is_active()) {
                if plan
                    .zones
                    .get(slot.zone)
                    .is_some_and(|z| plan::bus(&self.groups, z) == bus)
                {
                    slot.voice.fade(fade);
                    slot.fading = true;
                }
            }
        }
    }
}
