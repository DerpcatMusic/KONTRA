//! Saved MIDI-learn/host assignments, independent of UI presentation order.
use crate::{ControlContext, Error, Prepared, Runtime, WidgetStorage};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutomationSource {
    Controller(u8),
    HostParameter(u16),
}
#[derive(Clone, Copy, Debug)]
pub struct AutomationBinding {
    pub source: AutomationSource,
    pub source_slot: u8,
    pub ui_id: i32,
    pub low: f64,
    pub high: f64,
    pub soft_takeover: bool,
}
impl Prepared {
    pub fn with_automation_bindings(
        mut self,
        bindings: Vec<AutomationBinding>,
    ) -> Result<Self, Error> {
        for b in &bindings {
            let w = self
                .widgets
                .iter()
                .find(|w| w.source_slot == b.source_slot && w.ui_id == b.ui_id)
                .ok_or(Error::InvalidInput)?;
            if !matches!(w.storage, WidgetStorage::Control(_))
                || !b.low.is_finite()
                || !b.high.is_finite()
                || !(0.0..=1.0).contains(&b.low)
                || !(0.0..=1.0).contains(&b.high)
                || matches!(b.source, AutomationSource::Controller(c) if c>=128)
            {
                return Err(Error::InvalidInput);
            }
        }
        self.automation = bindings.into_boxed_slice();
        Ok(self)
    }
}
impl Runtime {
    pub fn dispatch_host_parameter(
        &mut self,
        _context: ControlContext,
        _address: u16,
        value: f64,
    ) -> Result<(), Error> {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(Error::InvalidInput);
        }
        Ok(())
    }
}
