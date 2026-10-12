//! Accepted native pedal state, addressed by physical input channel.

#[derive(Clone, Copy, Default)]
pub(super) struct Pedals {
    sustain: u16,
    sostenuto: u16,
}

pub(super) struct SostenutoChange {
    pub changed: u16,
    pub capture: u16,
    pub up: u16,
}

impl Pedals {
    /// Return the sustain up edge after committing the new state.
    pub(super) fn set_sustain(&mut self, input: u8, down: bool) -> bool {
        let bit = 1 << input;
        let was_down = self.sustain & bit != 0;
        if down { self.sustain |= bit; } else { self.sustain &= !bit; }
        was_down && !down
    }

    pub(super) fn set_sostenuto(&mut self, inputs: u16, down: bool) -> SostenutoChange {
        let previous = self.sostenuto;
        if down { self.sostenuto |= inputs; } else { self.sostenuto &= !inputs; }
        SostenutoChange {
            changed: (previous ^ self.sostenuto) & inputs,
            capture: if down { inputs & !previous } else { 0 },
            // Accepted repeated up/reset commands still clear root snapshots.
            up: if down { 0 } else { inputs },
        }
    }

    #[cfg(test)]
    pub(super) fn sostenuto_channels(&self) -> u16 { self.sostenuto }

    /// An effective Off requires the key, sustain and captured sostenuto holds
    /// to be gone. A newly accepted key-up passes false for `key_down`.
    pub(super) fn holds(&self, input: u8, key_down: bool, captured: bool) -> bool {
        key_down || captured || self.sustain & (1 << input) != 0
    }

    pub(super) fn clear(&mut self, inputs: u16) {
        self.sustain &= !inputs;
        self.sostenuto &= !inputs;
    }
}
