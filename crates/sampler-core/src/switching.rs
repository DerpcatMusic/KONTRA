//! Non-key articulation drivers, resolved at preparation for input adapters.
//! The runtime does not read them: an adapter in front of note admission turns
//! a driver value into a [`Switch`] before the event reaches any behavior.
use crate::{Error, Prepared, Runtime};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Driver {
    /// Only keyswitch keys select.
    #[default]
    Keys,
    Velocity,
    Channel,
    Controller,
    Program,
}

/// What played keyswitch keys do while a non-key driver is active.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SwitchKeys {
    #[default]
    Keep,
    /// Prepared without native switches, so they play notes.
    Play,
    /// The adapter drops them and their releases.
    Swallow,
}

/// How a driver value selects an articulation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    /// Set the performance's native articulation.
    Articulation(u32),
    /// Tap this key (on, then off) so behaviors that own switching see it.
    Tap(u8),
    /// Invoke an authored choice control for a keyless articulation.
    Control { id: u128, articulation: u32 },
}

/// One driver range. `controller` is the CC number for [`Driver::Controller`]
/// and 0 otherwise; `low..=high` bounds the velocity, channel, CC value or program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selector {
    pub controller: u8,
    pub low: u8,
    pub high: u8,
    pub switch: Switch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Switching {
    driver: Driver,
    keys: SwitchKeys,
    /// Bit `k` set for keyswitch key `k`.
    switch_keys: u128,
    selectors: std::sync::Arc<[Selector]>,
    key_inputs: Option<std::sync::Arc<[Option<Switch>; 128]>>,
    blocked: u128,
}

impl Default for Switching {
    fn default() -> Self {
        Self {
            driver: Driver::Keys,
            keys: SwitchKeys::Keep,
            switch_keys: 0,
            selectors: Vec::new().into(),
            key_inputs: None,
            blocked: 0,
        }
    }
}

impl Switching {
    pub fn new(
        driver: Driver,
        keys: SwitchKeys,
        switch_keys: impl IntoIterator<Item = u8>,
        selectors: Vec<Selector>,
    ) -> Result<Self, Error> {
        let mut table = 0u128;
        for key in switch_keys {
            table |= 1u128
                .checked_shl(u32::from(key))
                .ok_or(Error::InvalidInput)?;
        }
        if selectors.iter().any(|s| {
            s.low > s.high
                || s.high > 127
                || s.controller > 127
                || matches!(s.switch, Switch::Tap(key) if key > 127)
        }) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            driver,
            keys,
            switch_keys: table,
            selectors: selectors.into(),
            key_inputs: None,
            blocked: 0,
        })
    }

    /// Worker-prepared user inputs. Duplicates/bad keys are rejected, never clamped.
    pub fn set_key_inputs(
        &mut self,
        inputs: Vec<(u8, Switch)>,
        blocked: u128,
    ) -> Result<(), Error> {
        let inputs_nonempty = !inputs.is_empty();
        let mut keys = [None; 128];
        for (key, switch) in inputs {
            if matches!(switch, Switch::Tap(k) if k > 127) {
                return Err(Error::InvalidInput);
            }
            if keys
                .get_mut(usize::from(key))
                .ok_or(Error::InvalidInput)?
                .replace(switch)
                .is_some()
            {
                return Err(Error::InvalidInput);
            }
        }
        self.key_inputs = inputs_nonempty.then(|| std::sync::Arc::new(keys));
        self.blocked = blocked;
        Ok(())
    }

    pub fn key_input(&self, key: u8) -> Option<Switch> {
        self.key_inputs
            .as_ref()?
            .get(usize::from(key))
            .copied()
            .flatten()
    }
    pub fn blocked(&self, key: u8) -> bool {
        key < 128 && self.blocked >> key & 1 != 0
    }

    pub fn driver(&self) -> Driver {
        self.driver
    }

    pub fn keys(&self) -> SwitchKeys {
        self.keys
    }

    /// Whether `key` is one of the source's keyswitch keys.
    pub fn is_switch_key(&self, key: u8) -> bool {
        key < 128 && self.switch_keys >> key & 1 == 1
    }

    /// Whether a [`Driver::Controller`] selector reads `controller`.
    pub fn listens(&self, controller: u8) -> bool {
        self.driver == Driver::Controller
            && self.selectors.iter().any(|s| s.controller == controller)
    }

    /// The first selector containing `value` (and naming `controller` for
    /// [`Driver::Controller`]). Allocation-free.
    pub fn select(&self, controller: u8, value: u8) -> Option<Switch> {
        self.selectors
            .iter()
            .find(|s| s.controller == controller && (s.low..=s.high).contains(&value))
            .map(|s| s.switch)
    }
}

impl Prepared {
    pub fn with_switching(mut self, switching: Switching) -> Self {
        self.switching = switching;
        self
    }

    pub fn switching(&self) -> &Switching {
        &self.switching
    }
}

impl Runtime {
    /// The active plan's articulation drivers.
    pub fn switching(&self) -> &Switching {
        &self
            .plans
            .get(self.active_plan.0)
            .unwrap()
            .prepared
            .switching
    }
}

impl Runtime {
    /// Replace the active plan's articulation drivers and native keyswitch
    /// keys, as a driver remap does. Playing notes keep their snapshots.
    pub fn set_switching(
        &mut self,
        switching: Switching,
        switches: Vec<crate::Keyswitch>,
    ) -> Result<(), Error> {
        self.set_switching_table(switching, &switches)
    }

    /// Install worker-prepared tables without allocating on the audio thread.
    pub fn set_switching_table(
        &mut self,
        switching: Switching,
        switches: &[crate::Keyswitch],
    ) -> Result<(), Error> {
        let mut keys = [None; 128];
        for switch in switches {
            let slot = keys
                .get_mut(usize::from(switch.key))
                .ok_or(Error::InvalidInput)?;
            if slot.replace(switch.articulation).is_some() {
                return Err(Error::InvalidInput);
            }
        }
        let plan = &mut self.plans.get_mut(self.active_plan.0).unwrap().prepared;
        *plan.keyswitches = keys;
        plan.switching = switching;
        Ok(())
    }
}
