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
    pub fn automation_bindings(&self) -> &[AutomationBinding] {
        &self.automation
    }
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
#[derive(Clone, Copy, Default)]
pub(super) struct AutomationState {
    previous: Option<f64>,
    applied: Option<crate::WidgetValue>,
    latched: bool,
}
impl Runtime {
    /// Normalized host automation follows the same saved widget route as MIDI CC.
    pub fn dispatch_host_parameter(
        &mut self,
        context: ControlContext,
        address: u16,
        value: f64,
    ) -> Result<(), Error> {
        self.dispatch_host_parameter_in(context, self.active_plan, None, address, value)
            .map(|_| ())
    }

    /// Generation-aware admission for the existing control handoff.
    pub fn dispatch_host_parameter_in(
        &mut self,
        context: ControlContext,
        plan: crate::PlanId,
        expected_revision: Option<u64>,
        address: u16,
        value: f64,
    ) -> Result<u64, Error> {
        self.apply_due();
        let revision = self.control_revision(plan)?;
        if expected_revision.is_some_and(|r| r != revision) {
            return Err(Error::RevisionConflict);
        }
        self.dispatch_automation(
            context,
            plan,
            AutomationSource::HostParameter(address),
            value,
            0,
        )?;
        self.control_revision(plan)
    }

    pub(super) fn dispatch_automation(
        &mut self,
        context: ControlContext,
        plan: crate::PlanId,
        source: AutomationSource,
        input: f64,
        reserved_callbacks: usize,
    ) -> Result<(), Error> {
        let performance = self.performance_index(context.performance)?;
        if !input.is_finite()
            || !(0.0..=1.0).contains(&input)
            || context.origin.group >= 16
            || context.origin.channel >= 16
            || context.channels == 0
        {
            return Err(Error::InvalidInput);
        }
        self.apply_due();
        // Reserve room for the complete input admission before any widget write.
        let generation = self.plans.get(plan.0).unwrap();
        let needed = generation
            .prepared
            .automation
            .iter()
            .filter(|b| b.source == source)
            .filter(|b| {
                generation.prepared.widgets.iter().any(|w| {
                    w.source_slot == b.source_slot && w.ui_id == b.ui_id && w.program.is_some()
                })
            })
            .count();
        let len = generation.prepared.automation.len();
        let reserved = needed
            .checked_add(reserved_callbacks)
            .ok_or(Error::Capacity)?;
        if !self.behavior_room(reserved) {
            return Err(Error::Capacity);
        }
        if let AutomationSource::Controller(number) = source
            && needed != 0
        {
            self.performance_state.input_controllers[performance][usize::from(number)] =
                (input * f64::from(u32::MAX)).round() as u32;
        }
        self.behaviors.reserve(reserved);
        let mut remaining = needed;
        let result = (|| {
            for index in 0..len {
                let binding = self.plans.get(plan.0).unwrap().prepared.automation[index];
                if binding.source != source {
                    continue;
                }
                let id = self.widget_id(plan, binding.source_slot, binding.ui_id)?;
                let widget = *self
                    .widget_definitions(plan)?
                    .iter()
                    .find(|w| w.id == id)
                    .unwrap();
                let WidgetStorage::Control(control) = widget.storage else {
                    return Err(Error::InvalidInput);
                };
                let domain = self.control_definition(plan, control)?.domain;
                let position = binding.low + (binding.high - binding.low) * input;
                let value = match domain {
                    crate::ControlDomain::Integer { min, max } => crate::WidgetValue::Integer(
                        (min as f64 + (max as f64 - min as f64) * position).round() as i64,
                    ),
                    crate::ControlDomain::Real { min, max } => {
                        crate::WidgetValue::Real(min + (max - min) * position)
                    }
                    crate::ControlDomain::Toggle => {
                        crate::WidgetValue::Integer(i64::from(position >= 0.5))
                    }
                };
                let current = self.widget_value(plan, id, 0)?;
                let current_position = match (domain, current) {
                    (
                        crate::ControlDomain::Integer { min, max },
                        crate::WidgetValue::Integer(v),
                    ) if max != min => (v as f64 - min as f64) / (max as f64 - min as f64),
                    (crate::ControlDomain::Real { min, max }, crate::WidgetValue::Real(v))
                        if max != min =>
                    {
                        (v - min) / (max - min)
                    }
                    (_, crate::WidgetValue::Integer(v)) => v as f64,
                    _ => 0.0,
                };
                let mut state = self.plans.get(plan.0).unwrap().controls.automation[index];
                if state.applied.is_some_and(|last| last != current) {
                    state.latched = false;
                }
                if !binding.soft_takeover
                    || value == current
                    || state.previous.is_some_and(|p| {
                        (p <= current_position && position >= current_position)
                            || (p >= current_position && position <= current_position)
                    })
                {
                    state.latched = true;
                }
                state.previous = Some(position);
                if widget.program.is_some() {
                    self.behaviors.unreserve(1);
                    remaining -= 1;
                }
                if state.latched {
                    self.invoke_widget(
                        context,
                        plan,
                        None,
                        &[crate::WidgetEdit {
                            id,
                            index: 0,
                            value,
                            interaction: Default::default(),
                        }],
                    )?;
                    state.applied = Some(self.widget_value(plan, id, 0)?);
                }
                self.plans.get_mut(plan.0).unwrap().controls.automation[index] = state;
            }
            Ok(())
        })();
        self.behaviors.unreserve(remaining + reserved_callbacks);
        result
    }
}
